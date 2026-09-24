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
