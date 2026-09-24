"""Async wrappers over the handful of blocking file operations the console performs.

Every call site is inside a request handler or a worker task, sharing one event loop with every
other request. `open()`, `write()`, `flush()` and `mkdir()` are blocking syscalls: on a busy, slow
or full disk they stall the *whole* API, not just the caller that triggered them — the health
endpoint stops answering, queued runs stop being picked up, and the console appears hung for a
reason nothing logs. SonarQube flags the direct calls as `python:S7493`; the underlying problem is
real, so these offload to a worker thread rather than suppressing the rule.

`asyncio.to_thread` rather than a new `aiofiles` dependency: it is stdlib, this project targets
3.12+, and the set of operations needed here is small enough that a dependency would be the more
expensive answer.

**Ordering:** each wrapper is awaited before the next is issued at every call site, so writes
reach the file in the order they were made. Issuing two of these concurrently against one handle
would not preserve order — don't.
"""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import IO


def _mkdirs(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


async def mkdirs(path: Path) -> None:
    await asyncio.to_thread(_mkdirs, path)


def _append_text(path: Path, text: str) -> None:
    with path.open("a", encoding="utf-8") as fh:
        fh.write(text)


async def append_text(path: Path, text: str) -> None:
    """Open, append, close. For one-off notes — a stage header, a failure line."""
    await asyncio.to_thread(_append_text, path, text)


def _open_append(path: Path) -> IO[str]:
    return path.open("a", encoding="utf-8")


async def open_append(path: Path) -> IO[str]:
    """A handle kept open across many writes — the streaming log pump."""
    return await asyncio.to_thread(_open_append, path)


def _write_line(fh: IO[str], line: str) -> None:
    fh.write(line)
    fh.flush()


async def write_line(fh: IO[str], line: str) -> None:
    """Write and flush. The flush is what makes the log tailable while the run is in flight, and
    it is also the expensive half — which is exactly why it belongs off the event loop."""
    await asyncio.to_thread(_write_line, fh, line)


async def close(fh: IO[str]) -> None:
    await asyncio.to_thread(fh.close)


def _write_new(path: Path, text: str) -> None:
    with path.open("w", encoding="utf-8") as fh:
        fh.write(text)


async def write_new(path: Path, text: str) -> None:
    await asyncio.to_thread(_write_new, path, text)


def _unlink(path: Path) -> None:
    path.unlink(missing_ok=True)


async def unlink(path: Path) -> None:
    await asyncio.to_thread(_unlink, path)
