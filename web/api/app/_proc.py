"""Minimal subprocess helpers shared by the MCP supervisor (RFC 0129 §2) and the read-only
subprocess seam (§5). Deliberately tiny — this is not the Phase 3 job runner.

Every caller passes an argument list; nothing here ever runs a shell.
"""

from __future__ import annotations

import asyncio
import contextlib
import re
import signal
from collections.abc import Awaitable, Callable
from pathlib import Path

from . import _afile

_ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")

# asyncio's StreamReader defaults to a 64 KiB limit and `readline()` raises ValueError — not EOF,
# not a truncated line — the moment a single line exceeds it. `ekos` emits long lines routinely
# (a `--json` payload, a diagnostics path list, a wrapped evidence excerpt), and the failure took
# down the whole pump, so one long line failed an entire multi-minute run. Raised here, and
# `run_streaming` degrades instead of raising if even this is exceeded.
STREAM_LIMIT = 4 * 1024 * 1024


def strip_ansi(text: str) -> str:
    return _ANSI.sub("", text)


async def spawn(argv: list[str], *, cwd: str | None = None) -> asyncio.subprocess.Process:
    """`create_subprocess_exec(*argv)` with stdout/stderr piped, optionally in `cwd`."""
    return await asyncio.create_subprocess_exec(
        *argv,
        cwd=cwd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        limit=STREAM_LIMIT,
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
        # The process can still exit on its own between the timeout firing and the kill landing,
        # and `kill()` on a reaped process raises. Without this, a cancel that raced a natural
        # exit propagated ProcessLookupError out of `cancel()` and into the request handler.
        with contextlib.suppress(ProcessLookupError):
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
    await _afile.mkdirs(log_path.parent)
    proc = await asyncio.create_subprocess_exec(
        *argv,
        cwd=cwd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
        limit=STREAM_LIMIT,
    )
    register(proc)

    async def pump() -> None:
        assert proc.stdout is not None
        # Opened, written and closed off the event loop (`_afile`): this handle is written once
        # per output line of a process that can run for minutes, so it is by far the largest
        # source of blocking I/O in the console.
        fh = await _afile.open_append(log_path)
        try:
            while True:
                try:
                    raw = await proc.stdout.readline()
                except (ValueError, asyncio.LimitOverrunError):
                    # A single line longer than STREAM_LIMIT. `readline()` resets the internal
                    # buffer when it raises, so the stream stays usable and the right move is to
                    # note the loss and keep going — failing the run over one pathological line
                    # is strictly worse than losing the line.
                    await _afile.write_line(
                        fh, f"[console] dropped a line longer than {STREAM_LIMIT} bytes\n"
                    )
                    continue
                if not raw:
                    return
                line = strip_ansi(raw.decode(errors="replace").rstrip("\n"))
                await _afile.write_line(fh, line + "\n")
                if on_line is not None:
                    await on_line(line)
        finally:
            await _afile.close(fh)

    try:
        async with asyncio.timeout(timeout_s):
            await asyncio.gather(pump(), proc.wait())
    except TimeoutError:
        with contextlib.suppress(ProcessLookupError):
            proc.kill()
        await proc.wait()
        await _afile.append_text(log_path, f"\n[console] killed after {timeout_s}s timeout\n")
        return 124
    except BaseException:
        # Any other failure — a log write error, `on_line` raising, the caller cancelling — used
        # to propagate with the child still running. The runner pops it from `_running` in its own
        # `finally`, so nothing tracked it afterwards and it ran on unsupervised until it finished
        # or the API process died. Reap it before the exception leaves.
        await terminate(proc)
        raise
    # `or 0` would turn a `None` returncode into a clean exit. After `wait()` it cannot be None,
    # but being explicit means a future change to the gather above cannot quietly report success.
    return proc.returncode if proc.returncode is not None else 0
