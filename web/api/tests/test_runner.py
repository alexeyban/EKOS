"""JobRunner unit tests — deterministic, no real subprocess (RFC 0131 §3)."""

from __future__ import annotations

import asyncio

import pytest

from app import _proc, models
from app.commands import BY_NAME
from app.runner import JobRunner, QueueFull
from app.settings import Settings


@pytest.fixture
def _db(tmp_path, reset_settings):
    models.init_engine(str(tmp_path / "c.db"))
    return tmp_path


def _settings(tmp_path) -> Settings:
    return Settings(
        ekos_bin="/bin/true",
        console_db=str(tmp_path / "c.db"),
        runs_dir=str(tmp_path / "runs"),
        run_queue_depth=2,
        session_secret="s",
    )


def _slow_stream(hold: asyncio.Event | None = None):
    """A stand-in for `_proc.run_streaming` that blocks until cancelled."""

    async def stream(argv, *, cwd, log_path, register, timeout_s, on_line=None):
        log_path.parent.mkdir(parents=True, exist_ok=True)
        log_path.touch()
        if hold is not None:
            hold.set()
        await asyncio.sleep(timeout_s)
        return 0

    return stream


async def test_cancel_a_queued_run_is_deterministic(_db, monkeypatch):
    tmp = _db
    running = asyncio.Event()
    monkeypatch.setattr(_proc, "run_streaming", _slow_stream(running))

    runner = JobRunner(_settings(tmp))
    runner.start()
    try:
        a = await runner.submit("w", str(tmp), BY_NAME["build"], {})
        b = await runner.submit("w", str(tmp), BY_NAME["compile"], {})
        await asyncio.wait_for(running.wait(), timeout=2)  # a is executing

        assert models.get_run(b).status == "queued"
        assert await runner.cancel(b) is True
        assert models.get_run(b).status == "cancelled"
    finally:
        await runner.aclose()

    # b never ran even though the worker would reach it after a
    assert models.get_run(b).status == "cancelled"
    assert models.get_run(a).status in {"running", "failed", "cancelled"}


async def test_queue_full_raises(_db, monkeypatch):
    tmp = _db
    running = asyncio.Event()
    monkeypatch.setattr(_proc, "run_streaming", _slow_stream(running))

    runner = JobRunner(_settings(tmp))  # depth 2
    try:
        await runner.submit("w", str(tmp), BY_NAME["build"], {})  # picked up by the worker
        await asyncio.wait_for(running.wait(), timeout=2)
        await runner.submit("w", str(tmp), BY_NAME["build"], {})  # queued (1/2)
        await runner.submit("w", str(tmp), BY_NAME["build"], {})  # queued (2/2)
        with pytest.raises(QueueFull):
            await runner.submit("w", str(tmp), BY_NAME["build"], {})
    finally:
        await runner.aclose()


async def test_bad_param_is_rejected_before_a_run_row_exists(_db):
    tmp = _db
    runner = JobRunner(_settings(tmp))
    with pytest.raises(ValueError):
        await runner.submit("w", str(tmp), BY_NAME["ekl"], {})  # missing required `query`
    assert models.list_runs("w") == []


# ── worker survival (devlog_203) ─────────────────────────────────────────────


async def test_a_bookkeeping_failure_does_not_kill_the_worker(_db, monkeypatch):
    """There is one worker task per workspace and nothing restarts it. If an exception escapes
    the loop, that workspace's queue is never drained again and every later run sits at "queued"
    until the queue fills and every request 429s — with nothing pointing at the cause.

    `_execute` has its own handler, but its final status write and `_notify_done` sit outside it.
    This simulates exactly that: the first run's terminal bookkeeping raises, the second must
    still run.
    """
    runner = JobRunner(_settings(_db))
    monkeypatch.setattr(_proc, "run_streaming", _ok_stream())

    calls = {"n": 0}
    real_update = models.update_run

    def flaky_update(run_id, **kw):
        # Fail only the first run's terminal write — the one outside _execute's try.
        if kw.get("status") in {"succeeded", "failed"} and calls["n"] == 0:
            calls["n"] += 1
            raise RuntimeError("database briefly unavailable")
        return real_update(run_id, **kw)

    monkeypatch.setattr(models, "update_run", flaky_update)

    first = await runner.submit("ws", str(_db), BY_NAME["doctor"], {})
    second = await runner.submit("ws", str(_db), BY_NAME["doctor"], {})
    await asyncio.wait_for(runner._queues["ws"].join(), timeout=10)

    assert calls["n"] == 1, "the failure must actually have been triggered"
    worker = runner._workers["ws"]
    assert not worker.done(), "the worker task must survive a bookkeeping failure"
    assert models.get_run(second).status == "succeeded", (
        "the run queued behind the failure must still execute"
    )
    assert models.get_run(first).status in {"failed", "succeeded"}, (
        "the failed run must not be left stuck at 'running'"
    )
    await runner.aclose()


def _ok_stream():
    async def stream(argv, *, cwd, log_path, register, timeout_s, on_line=None):
        log_path.parent.mkdir(parents=True, exist_ok=True)
        log_path.touch()
        return 0

    return stream
