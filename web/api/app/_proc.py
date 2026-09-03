"""Minimal subprocess helpers shared by the MCP supervisor (RFC 0129 §2) and the read-only
subprocess seam (§5). Deliberately tiny — this is not the Phase 3 job runner.

Every caller passes an argument list; nothing here ever runs a shell.
"""

from __future__ import annotations

import asyncio
import re
import signal
from collections.abc import Awaitable, Callable
from pathlib import Path

_ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")


def strip_ansi(text: str) -> str:
    return _ANSI.sub("", text)


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


async def run_streaming(
    argv: list[str],
    *,
    cwd: str,
    log_path: Path,
    register: Callable[[asyncio.subprocess.Process], None],
    on_line: Callable[[str], Awaitable[None]] | None = None,
    timeout_s: float,
) -> int:
    """Run `argv` in `cwd`, merging stdout+stderr, appending each ANSI-stripped line to `log_path`
    and (optionally) calling `on_line`. `register` is handed the live process so the caller can
    cancel it. Returns the exit code; a timeout SIGKILLs and returns 124.
    """
    log_path.parent.mkdir(parents=True, exist_ok=True)
    proc = await asyncio.create_subprocess_exec(
        *argv,
        cwd=cwd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
    )
    register(proc)

    async def pump() -> None:
        assert proc.stdout is not None
        with log_path.open("a") as fh:
            async for raw in proc.stdout:
                line = strip_ansi(raw.decode(errors="replace").rstrip("\n"))
                fh.write(line + "\n")
                fh.flush()
                if on_line is not None:
                    await on_line(line)

    try:
        async with asyncio.timeout(timeout_s):
            await asyncio.gather(pump(), proc.wait())
    except TimeoutError:
        proc.kill()
        await proc.wait()
        with log_path.open("a") as fh:
            fh.write(f"\n[console] killed after {timeout_s}s timeout\n")
        return 124
    return proc.returncode or 0
