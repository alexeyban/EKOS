"""Read-only subprocess seam (RFC 0129 §5).

R2 / R5 / R6 are CLI-only — the MCP tools expose leaner payloads — so the dashboard has to shell
out for them. It does **not** need the Phase 3 job runner (queue, per-workspace mutex,
cancellation, SSE, write-role gate): these three verbs are read-only, idempotent, and safe to run
concurrently.

The allowlist is three exact argv shapes. There is no code path that runs an arbitrary command.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

from . import _proc

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
}


class ReadProcError(RuntimeError):
    pass


def _check_allowed(argv: list[str]) -> None:
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


async def read_json(ekos_bin: str, workspace_path: str, argv: list[str]) -> Any:
    """Run `<ekos_bin> <argv...>` in the workspace directory and parse stdout as JSON.

    `status` / `doctor` / `ledger timeline` take the workspace from the working directory (they
    have no `--workspace` flag — that is a `mcp serve` argument), so the subprocess is launched
    with `cwd` set to the resolved workspace root. `argv` must match one of the three allowlisted
    shapes; the caller has already confirmed `workspace_path` is a registered workspace root.
    """
    _check_allowed(argv)

    root = Path(workspace_path).resolve()
    if not root.is_dir():
        raise ReadProcError(f"workspace path is not a directory: {root}")

    proc = await _proc.spawn([ekos_bin, *argv], cwd=str(root))
    try:
        stdout, _stderr = await asyncio.wait_for(proc.communicate(), timeout=_TIMEOUT)
    except TimeoutError:
        proc.kill()
        await proc.wait()
        raise ReadProcError(f"`ekos {' '.join(argv)}` timed out after {_TIMEOUT}s") from None

    if proc.returncode != 0:
        tail = _stderr[:500].decode(errors="replace")
        raise ReadProcError(f"`ekos {' '.join(argv)}` exited {proc.returncode}: {tail}")
    if len(stdout) > _MAX_OUTPUT:
        raise ReadProcError("output exceeded 1 MiB")
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise ReadProcError(f"output was not JSON: {exc}") from exc
