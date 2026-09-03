"""Minimal subprocess helpers shared by the MCP supervisor (RFC 0129 §2) and the read-only
subprocess seam (§5). Deliberately tiny — this is not the Phase 3 job runner.

Every caller passes an argument list; nothing here ever runs a shell.
"""

from __future__ import annotations

import asyncio
import signal


async def spawn(argv: list[str], *, cwd: str | None = None) -> asyncio.subprocess.Process:
    """`create_subprocess_exec(*argv)` with stdout/stderr piped, optionally in `cwd`."""
    return await asyncio.create_subprocess_exec(
        *argv,
        cwd=cwd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )


async def terminate(proc: asyncio.subprocess.Process, *, grace: float = 5.0) -> None:
    """SIGTERM, then SIGKILL after `grace` seconds. Safe to call on an already-exited process."""
    if proc.returncode is not None:
        return
    try:
        proc.send_signal(signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        await asyncio.wait_for(proc.wait(), timeout=grace)
    except TimeoutError:
        proc.kill()
        await proc.wait()
