"""`ekos.toml` read / write / observe-diff (RFC 0130 §2).

`tomlkit` only — it round-trips comments and formatting; `tomli-w` would flatten them. Validation
and the preview-scan are delegated to `ekos config …` through the read-only subprocess allowlist
(`readproc`); this module owns only the file read, the `.bak` write, and the `[observe]` diff that
drives the append-only warning.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import tomlkit


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
    try:
        return tomlkit.parse(raw)
    except Exception as exc:  # tomlkit raises several exception types for malformed input
        raise ConfigError(f"invalid TOML: {exc}") from exc


def config_path(workspace_path: str) -> Path:
    """Resolve to a real, canonical path and verify it still lands directly inside
    `workspace_path` (SonarCloud pythonsecurity:S2083 hardening). This also rejects a `ekos.toml`
    that is itself a symlink pointing outside the workspace — `resolve()` follows it, so the
    parent-directory check catches the escape.
    """
    root = Path(workspace_path).resolve()
    path = (root / "ekos.toml").resolve()
    if path.parent != root:
        raise ConfigError(f"{path} escapes workspace root {root}")
    return path


def read_config(workspace_path: str) -> tuple[str, ObserveView]:
    path = config_path(workspace_path)
    if not path.is_file():
        raise ConfigError(f"{path} does not exist")
    raw = path.read_text()
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


def write_config(workspace_path: str, raw: str) -> ObserveDelta:
    """Parse `raw`, copy the current file to `ekos.toml.bak`, then write `raw`. Returns the
    `[observe]` delta vs. the file that was there. Does **not** validate semantics — the caller
    runs `ekos config validate` first.
    """
    parse(raw)  # reject malformed input before touching anything
    path = config_path(workspace_path)
    before = path.read_text() if path.is_file() else ""
    delta = diff_observe(before, raw) if before else ObserveDelta(added_paths=[], added_patterns=[])
    if before:
        path.with_suffix(".toml.bak").write_text(before)
    path.write_text(raw)
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
