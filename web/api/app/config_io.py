"""`ekos.toml` read / write / observe-diff (RFC 0130 §2).

`tomlkit` only — it round-trips comments and formatting; `tomli-w` would flatten them. Validation
and the preview-scan are delegated to `ekos config …` through the read-only subprocess allowlist
(`readproc`); this module owns only the file read, the `.bak` write, and the `[observe]` diff that
drives the append-only warning.
"""

from __future__ import annotations

import logging
import os
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

import tomlkit

log = logging.getLogger("ekos.console.config_io")

# `ekos.toml` is a hand-written config file; a real one in this repository is ~5 KB. The cap is
# generous by three orders of magnitude and exists only so an unbounded read or parse cannot be
# used to exhaust memory: `raw` arrives straight from an HTTP request body, and tomlkit builds a
# full document tree in memory before anything validates it.
MAX_CONFIG_BYTES = 1024 * 1024


class ConfigError(RuntimeError):
    """The submitted TOML does not parse."""


@dataclass
class ObserveView:
    paths: list[str] = field(default_factory=list)
    ignore_patterns: list[str] = field(default_factory=list)


@dataclass
class ObserveDelta:
    added_paths: list[str] = field(default_factory=list)
    removed_paths: list[str] = field(default_factory=list)
    added_patterns: list[str] = field(default_factory=list)
    removed_patterns: list[str] = field(default_factory=list)

    @property
    def narrows(self) -> bool:
        """True if anything was removed — the case that needs the append-only warning."""
        return bool(self.removed_paths or self.removed_patterns)


def _observe_of(doc: tomlkit.TOMLDocument) -> ObserveView:
    observe = doc.get("observe", {}) or {}
    return ObserveView(
        paths=[str(p) for p in observe.get("paths", [])],
        ignore_patterns=[str(p) for p in observe.get("ignore-patterns", [])],
    )


def parse(raw: str) -> tomlkit.TOMLDocument:
    if len(raw.encode("utf-8")) > MAX_CONFIG_BYTES:
        raise ConfigError(f"config is larger than the {MAX_CONFIG_BYTES} byte limit")
    try:
        return tomlkit.parse(raw)
    except Exception as exc:  # tomlkit raises several exception types for malformed input
        raise ConfigError(f"invalid TOML: {exc}") from exc


def config_path(workspace_path: str) -> Path:
    """Resolve to a real, canonical path and verify it still lands directly inside
    `workspace_path` (SonarCloud pythonsecurity:S2083 hardening). This also rejects a `ekos.toml`
    that is itself a symlink pointing outside the workspace — `resolve()` follows it, so the
    parent-directory check catches the escape.

    Note the check alone is not the whole protection: it is a point-in-time test, and the file can
    be replaced by a symlink between here and the write. `write_config` closes that window by
    renaming over the target rather than opening it — see there.
    """
    root = Path(workspace_path).resolve()
    path = (root / "ekos.toml").resolve()
    if path.parent != root:
        # Deliberately no paths in the message. `ConfigError` is surfaced verbatim as an
        # HTTPException detail by `routes/config.py`, so anything interpolated here is returned to
        # the caller — and absolute server paths tell an unauthenticated-ish client the deployment
        # layout, the OS user and the directory structure (CWE-209). The real paths still reach
        # the operator through the log below.
        log.warning("config path %s escapes workspace root %s", path, root)
        raise ConfigError("ekos.toml resolves outside the workspace root")
    return path


def read_config(workspace_path: str) -> tuple[str, ObserveView]:
    path = config_path(workspace_path)
    if not path.is_file():
        raise ConfigError("ekos.toml does not exist in this workspace")
    size = path.stat().st_size
    if size > MAX_CONFIG_BYTES:
        raise ConfigError(f"ekos.toml is larger than the {MAX_CONFIG_BYTES} byte limit")
    raw = path.read_text(encoding="utf-8")
    return raw, _observe_of(parse(raw))


def diff_observe(before_raw: str, after_raw: str) -> ObserveDelta:
    b, a = _observe_of(parse(before_raw)), _observe_of(parse(after_raw))
    bp, ap = set(b.paths), set(a.paths)
    bi, ai = set(b.ignore_patterns), set(a.ignore_patterns)
    return ObserveDelta(
        added_paths=sorted(ap - bp),
        removed_paths=sorted(bp - ap),
        added_patterns=sorted(ai - bi),
        removed_patterns=sorted(bi - ai),
    )


def _atomic_write(path: Path, text: str) -> None:
    """Write `text` to `path` by creating a private temp file beside it and renaming over the top.

    Three problems with the obvious `path.write_text(text)`, all of which this avoids:

    * **It follows symlinks.** `config_path` resolves first, so a symlink that already exists is
      followed and its *target* is what gets range-checked — deliberate, and safe, because the
      target must still sit directly inside the workspace. The gap is the other ordering: the
      check passes on an ordinary path, and a symlink appears at that name before the write
      lands. `write_text` would follow it and hand out an arbitrary file write with this
      process's privileges. `os.replace` swaps the directory entry, so a link planted in that
      window is destroyed rather than obeyed.
    * **It truncates in place.** A crash, a full disk or two concurrent writers leaves a
      half-written `ekos.toml`, and the workspace's config is the input to every later command.
      `os.replace` is atomic on POSIX: readers see the old file or the new one, never a partial.
    * **It uses the process umask**, typically 0644. `mkstemp` creates at 0600.

    0600 is also what the temp file needs regardless: it briefly holds the full config contents in
    a directory other users may be able to read.
    """
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=".ekos-toml-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(text)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp, path)
    except BaseException:
        # Leave nothing behind on any failure path, cancellation included.
        Path(tmp).unlink(missing_ok=True)
        raise


def write_config(workspace_path: str, raw: str) -> ObserveDelta:
    """Parse `raw`, copy the current file to `ekos.toml.bak`, then write `raw`. Returns the
    `[observe]` delta vs. the file that was there. Does **not** validate semantics — the caller
    runs `ekos config validate` first.

    Both writes are atomic and 0600 — see `_atomic_write`.
    """
    parse(raw)  # reject malformed input (and oversized input) before touching anything
    path = config_path(workspace_path)
    before = path.read_text(encoding="utf-8") if path.is_file() else ""
    delta = diff_observe(before, raw) if before else ObserveDelta(added_paths=[], added_patterns=[])
    if before:
        _atomic_write(path.with_suffix(".toml.bak"), before)
    _atomic_write(path, raw)
    return delta


APPEND_ONLY_WARNING = (
    "{n_paths} path(s) and {n_patterns} ignore-pattern(s) were removed. This affects FUTURE "
    "builds only — the append-only ledger keeps everything already compiled for the removed "
    "scope. To actually drop it you must wipe `.ekos/` and rebuild (a Phase 3 job)."
)


def append_only_warning(delta: ObserveDelta) -> str | None:
    if not delta.narrows:
        return None
    return APPEND_ONLY_WARNING.format(
        n_paths=len(delta.removed_paths), n_patterns=len(delta.removed_patterns)
    )
