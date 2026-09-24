"""Reliability fixes in `_proc.run_streaming` / `_proc.terminate` (devlog_203).

Each test reproduces a failure that took down more than the thing that failed.
"""

from __future__ import annotations

import asyncio
import sys

import pytest

from app import _proc


def _python(code: str) -> list[str]:
    return [sys.executable, "-c", code]


@pytest.mark.asyncio
async def test_a_line_longer_than_64kib_does_not_fail_the_run(tmp_path):
    """asyncio's StreamReader defaults to a 64 KiB limit and `readline()` raises ValueError —
    not EOF — past it. One long line used to abort the pump and fail an entire run."""
    log = tmp_path / "run.log"
    # Generated inside the child: a 200 KB literal in argv hits the OS ARG_MAX limit, not the
    # stream limit this test is about.
    code = await _proc.run_streaming(
        _python("print('A' * (200 * 1024)); print('AFTER')"),
        cwd=str(tmp_path),
        log_path=log,
        register=lambda p: None,
        timeout_s=30,
    )

    assert code == 0, "a long line must not fail the run"
    text = log.read_text()
    assert "AFTER" in text, "output after the long line must still be captured"


@pytest.mark.asyncio
async def test_a_line_over_the_raised_limit_is_dropped_not_fatal(tmp_path, monkeypatch):
    """Even past the raised limit the run survives; the line is dropped with a marker."""
    monkeypatch.setattr(_proc, "STREAM_LIMIT", 1024)
    log = tmp_path / "run.log"
    code = await _proc.run_streaming(
        _python("print('B' * 50000); print('AFTER')"),
        cwd=str(tmp_path),
        log_path=log,
        register=lambda p: None,
        timeout_s=30,
    )

    assert code == 0
    text = log.read_text()
    assert "dropped a line longer than" in text
    assert "AFTER" in text


@pytest.mark.asyncio
async def test_a_failing_on_line_callback_does_not_orphan_the_child(tmp_path):
    """If the pump raises, the subprocess used to keep running with nothing tracking it: the
    runner pops it from `_running` in its own `finally`, so it was never reaped."""
    log = tmp_path / "run.log"
    captured: list[asyncio.subprocess.Process] = []

    async def explode(_line: str) -> None:
        raise RuntimeError("consumer failed")

    # Long-lived child: if it is not reaped, it is still alive when we check.
    with pytest.raises(RuntimeError, match="consumer failed"):
        await _proc.run_streaming(
            _python("import time; print('hello', flush=True); time.sleep(30)"),
            cwd=str(tmp_path),
            log_path=log,
            register=captured.append,
            on_line=explode,
            timeout_s=30,
        )

    assert captured, "the process should have been registered"
    assert captured[0].returncode is not None, "the child must be reaped, not left running"


@pytest.mark.asyncio
async def test_terminate_survives_a_process_that_exits_during_the_grace_period(tmp_path):
    """`kill()` on an already-reaped process raises ProcessLookupError; a cancel that raced a
    natural exit used to propagate it into the request handler."""
    proc = await _proc.spawn(_python("import time; time.sleep(0.2)"), cwd=str(tmp_path))
    # grace=0 forces the timeout branch immediately, then the child exits on its own.
    await _proc.terminate(proc, grace=0.0)
    assert proc.returncode is not None


@pytest.mark.asyncio
async def test_timeout_still_reports_124(tmp_path):
    log = tmp_path / "run.log"
    code = await _proc.run_streaming(
        _python("import time; time.sleep(30)"),
        cwd=str(tmp_path),
        log_path=log,
        register=lambda p: None,
        timeout_s=0.5,
    )
    assert code == 124
    assert "killed after" in log.read_text()


@pytest.mark.asyncio
async def test_log_lines_keep_their_order_through_the_threaded_writer(tmp_path):
    """The risk introduced by moving writes off the event loop (`_afile`, python:S7493): each
    write now happens on a worker thread, so ordering is only preserved because every call is
    awaited before the next is issued. If that ever stops being true, the log interleaves."""
    log = tmp_path / "run.log"
    code = await _proc.run_streaming(
        _python("import sys\nfor i in range(500): print(i)"),
        cwd=str(tmp_path),
        log_path=log,
        register=lambda p: None,
        timeout_s=30,
    )

    assert code == 0
    lines = [ln for ln in log.read_text().splitlines() if ln.strip()]
    assert lines == [str(i) for i in range(500)], "log lines must stay in emission order"


@pytest.mark.asyncio
async def test_the_event_loop_keeps_running_while_a_chatty_process_streams(tmp_path):
    """The point of the whole change: a run that writes thousands of log lines must not stall
    the loop that is also serving every other request."""
    log = tmp_path / "run.log"
    ticks = 0
    stop = asyncio.Event()

    async def heartbeat() -> None:
        nonlocal ticks
        while not stop.is_set():
            ticks += 1
            await asyncio.sleep(0.001)

    beat = asyncio.create_task(heartbeat())
    try:
        code = await _proc.run_streaming(
            _python("for i in range(2000): print('line', i)"),
            cwd=str(tmp_path),
            log_path=log,
            register=lambda p: None,
            timeout_s=30,
        )
    finally:
        stop.set()
        await beat

    assert code == 0
    assert len(log.read_text().splitlines()) == 2000
    assert ticks > 10, f"the event loop only got {ticks} slices while the run streamed"
