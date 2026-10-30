"""Read-only subprocess seam (RFC 0129 §5).

R2 / R5 / R6 are CLI-only — the MCP tools expose leaner payloads — so the dashboard has to shell
out for them. It does **not** need the Phase 3 job runner (queue, per-workspace mutex,
cancellation, SSE, write-role gate): these three verbs are read-only, idempotent, and safe to run
concurrently.

The allowlist is a fixed set of exact argv shapes. There is no code path that runs an
arbitrary command.
"""

from __future__ import annotations

import asyncio
import json
import re
from pathlib import Path
from typing import Any

from . import _proc, envs, home

_MAX_OUTPUT = 1 << 20  # 1 MiB
_TIMEOUT = 20.0

# Each entry: an exact argv prefix (before `--workspace`/`cwd` handling) mapped to the set of
# `--flag value` options that may follow it. Nothing else is runnable — there is no code path
# that accepts an arbitrary command.
_ALLOWED: dict[tuple[str, ...], frozenset[str]] = {
    ("status", "--json"): frozenset(),
    ("doctor", "--json"): frozenset(),
    ("ledger", "timeline", "--json"): frozenset({"--bucket", "--since"}),
    ("config", "validate", "--json"): frozenset({"--file"}),
    ("config", "preview-scan", "--json"): frozenset({"--max-files"}),
    # RFC 0170 business semantics — reads only (the CLI opens the ledger read-only for these).
    ("semantics", "list", "--json"): frozenset({"--kind", "--status"}),
    ("semantics", "gaps", "--json"): frozenset(),
    ("export", "linkml", "--json"): frozenset({"--status"}),
    # The same schema as YAML text, for the console's editor (`read_text`).
    ("export", "linkml", "--yaml"): frozenset({"--status"}),
    # RFC 0172 conflicting evidence — read only.
    ("conflicts", "list", "--json"): frozenset({"--status", "--type"}),
    # RFC 0233 the integrity index the last commit wrote — one fixed argv, read only.
    ("check", "refs", "--from-index", "--format", "json", "--reachability"): frozenset(),
    # RFC 0213 review and the workflow bar — read only.
    ("review", "list", "--json"): frozenset(),
    ("workflow", "status", "--json"): frozenset(),
    # RFC 0223 a project's sources and databases (names and variable names only).
    ("source", "list", "--json"): frozenset(),
    ("database", "list", "--json"): frozenset(),
}

# RFC 0213: `review show <subject> --json [--undecided] [--limit <n>]` — the subject is one of a
# fixed set, the limit digits only.
REVIEW_SUBJECTS = ("scope", "pii", "conflicts", "identity", "write-behaviour", "glossary")
_DIGITS = re.compile(r"^[0-9]{1,6}$")


def _check_review_show(extra: list[str]) -> None:
    if not extra or extra[0] not in REVIEW_SUBJECTS:
        raise ReadProcError(f"unknown review subject {extra[:1]!r}")
    rest = extra[1:]
    if rest[:1] != ["--json"]:
        raise ReadProcError("review show needs --json")
    i = 1
    while i < len(rest):
        if rest[i] == "--undecided":
            i += 1
        elif rest[i] == "--limit" and i + 1 < len(rest) and _DIGITS.match(rest[i + 1]):
            i += 2
        else:
            raise ReadProcError(f"disallowed argument {rest[i]!r}")


# Shapes that end in exactly one positional argument, which must match the pattern. The only one
# is an item id — a UUID, never a name, so nothing user-typed reaches the argv beyond 36 hex/dash
# characters.
_UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
_POSITIONAL: dict[tuple[str, ...], re.Pattern[str]] = {
    ("semantics", "show", "--json"): _UUID,
}


class ReadProcError(RuntimeError):
    pass


def _check_allowed(argv: list[str]) -> None:
    if argv[:2] == ["review", "show"]:
        _check_review_show(argv[2:])
        return
    for prefix, pattern in _POSITIONAL.items():
        if tuple(argv[: len(prefix)]) == prefix:
            extra = argv[len(prefix) :]
            if len(extra) != 1 or not pattern.match(extra[0]):
                raise ReadProcError(f"disallowed arguments {extra!r}")
            return
    for prefix, allowed_flags in _ALLOWED.items():
        if tuple(argv[: len(prefix)]) != prefix:
            continue
        extra = argv[len(prefix) :]
        i = 0
        while i < len(extra):
            if extra[i] not in allowed_flags:
                raise ReadProcError(f"disallowed argument {extra[i]!r}")
            i += 2  # skip the flag's value
        return
    raise ReadProcError(f"argv not on the read-only allowlist: {argv}")


async def read_text(
    ekos_bin: str, workspace_path: str, argv: list[str], *, max_output: int = _MAX_OUTPUT
) -> str:
    """[`read_json`] for a command whose output is text. `--yaml` is a console-side marker on the
    allowlist only; it is stripped before the CLI runs (YAML is `export linkml`'s default)."""
    _check_allowed(argv)
    argv = [a for a in argv if a != "--yaml"]
    root = Path(workspace_path).resolve()
    if not root.is_dir():
        raise ReadProcError(f"workspace path is not a directory: {root}")
    cmd, cwd = home.command(ekos_bin, str(root), argv)
    proc = await _proc.spawn(cmd, cwd=cwd, env=envs.for_path(str(root)))
    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=_TIMEOUT)
    except TimeoutError:
        proc.kill()
        await proc.wait()
        raise ReadProcError(f"`ekos {' '.join(argv)}` timed out after {_TIMEOUT}s") from None
    if proc.returncode != 0:
        tail = stderr[:500].decode(errors="replace")
        raise ReadProcError(f"`ekos {' '.join(argv)}` exited {proc.returncode}: {tail}")
    if len(stdout) > max_output:
        raise ReadProcError(f"output exceeded {max_output} bytes")
    return stdout.decode(errors="replace")


async def read_json(
    ekos_bin: str,
    workspace_path: str,
    argv: list[str],
    *,
    max_output: int = _MAX_OUTPUT,
    time_limit: float = _TIMEOUT,
) -> Any:
    """Run `<ekos_bin> <argv...>` in the workspace directory and parse stdout as JSON.

    `status` / `doctor` / `ledger timeline` take the workspace from the working directory (they
    have no `--workspace` flag — that is a `mcp serve` argument), so the subprocess is launched
    with `cwd` set to the resolved workspace root. `argv` must match one of the three allowlisted
    shapes; the caller has already confirmed `workspace_path` is a registered workspace root.
    """
    _check_allowed(argv)
    return await _run_json(
        ekos_bin, workspace_path, argv, max_output=max_output, time_limit=time_limit
    )


async def read_checked_json(
    ekos_bin: str, workspace_path: str, argv: list[str], *, max_output: int = _MAX_OUTPUT
) -> Any:
    """[`read_json`] for an argv the caller has checked another way: an extension panel's
    command, which `extensions.panel_argv` accepts only when the catalogue says it is a read
    command with fixed flags (RFC 0198). Never call it with anything a request supplied."""
    return await _run_json(ekos_bin, workspace_path, argv, max_output=max_output)


async def _run_json(
    ekos_bin: str,
    workspace_path: str,
    argv: list[str],
    *,
    max_output: int,
    time_limit: float = _TIMEOUT,
) -> Any:
    root = Path(workspace_path).resolve()
    if not root.is_dir():
        raise ReadProcError(f"workspace path is not a directory: {root}")

    cmd, cwd = home.command(ekos_bin, str(root), argv)
    proc = await _proc.spawn(cmd, cwd=cwd, env=envs.for_path(str(root)))
    try:
        stdout, _stderr = await asyncio.wait_for(proc.communicate(), timeout=time_limit)
    except TimeoutError:
        proc.kill()
        await proc.wait()
        raise ReadProcError(f"`ekos {' '.join(argv)}` timed out after {time_limit}s") from None

    if proc.returncode != 0:
        tail = _stderr[:500].decode(errors="replace")
        raise ReadProcError(f"`ekos {' '.join(argv)}` exited {proc.returncode}: {tail}")
    if len(stdout) > max_output:
        raise ReadProcError(f"output exceeded {max_output} bytes")
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise ReadProcError(f"output was not JSON: {exc}") from exc
