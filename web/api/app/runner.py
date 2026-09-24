"""Job runner (RFC 0127 §8.5, RFC 0131 §3).

One bounded queue and one worker task per workspace — the single worker naturally serialises
runs on that workspace, which is what RFC 0104 requires (EKOS takes a real cross-process write
lock, so two writes on one workspace is a guaranteed conflict). Different workspaces run
concurrently. The queue rejects with `QueueFull` (→ HTTP 429) when it's full.

`create_subprocess_exec` only, never a shell. Cancellation is SIGTERM → SIGKILL. Chained
`pipeline` runs are one `Run` row with per-stage status.
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import uuid
from collections.abc import Awaitable, Callable
from pathlib import Path

from . import _afile, _proc, models
from .commands import BY_NAME, Command
from .models import Run
from .settings import Settings

log = logging.getLogger("ekos.console.runner")

OnDone = Callable[[Run], Awaitable[None]]


class QueueFull(RuntimeError):
    pass


class JobRunner:
    def __init__(self, settings: Settings) -> None:
        self._settings = settings
        self._queues: dict[str, asyncio.Queue[tuple[str, dict]]] = {}
        self._workers: dict[str, asyncio.Task] = {}
        self._running: dict[str, asyncio.subprocess.Process] = {}  # run_id -> live process
        self._cancelled: set[str] = set()
        self._on_done: dict[str, OnDone] = {}  # run_id -> terminal-status callback (RFC 0132)
        self._bg: set[asyncio.Task] = set()  # fire-and-forget callback tasks (keep a strong ref)

    def start(self) -> None:
        models.sweep_stale_runs()

    async def aclose(self) -> None:
        for run_id, proc in list(self._running.items()):
            self._cancelled.add(run_id)
            await _proc.terminate(proc)
        for task in self._workers.values():
            task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await task
        # Fire-and-forget `on_done` callbacks (RFC 0132 webhooks) are otherwise still pending when
        # the loop closes, which both loses the notification and logs "Task was destroyed but it
        # is pending!". Give them a bounded chance to finish, then drop them.
        if self._bg:
            _done, pending = await asyncio.wait(set(self._bg), timeout=5.0)
            for task in pending:
                task.cancel()

    # ── submission ───────────────────────────────────────────────────────────

    async def submit(
        self,
        workspace_id: str,
        ws_path: str,
        command: Command,
        params: dict,
        *,
        on_done: OnDone | None = None,
    ) -> str:
        command.render_argv(params)  # validate params up front → ValueError to the caller

        run_id = uuid.uuid4().hex
        if on_done is not None:
            self._on_done[run_id] = on_done
        log_path = str(Path(self._settings.runs_dir) / f"{run_id}.log")
        stages = (
            [{"name": s, "status": "pending", "exit_code": None} for s in command.stages]
            if command.stages
            else []
        )
        models.add_run(
            Run(
                id=run_id,
                workspace_id=workspace_id,
                command=command.name,
                params=params,
                status="queued",
                stages=stages,
                log_path=log_path,
            )
        )

        queue = self._queues.get(workspace_id)
        if queue is None:
            queue = asyncio.Queue(maxsize=self._settings.run_queue_depth)
            self._queues[workspace_id] = queue
            self._workers[workspace_id] = asyncio.create_task(self._worker(workspace_id, ws_path))
        try:
            queue.put_nowait((run_id, params))
        except asyncio.QueueFull as exc:
            models.update_run(run_id, status="failed", exit_code=None, ended_at=models._now())
            self._notify_done(run_id)
            raise QueueFull(f"workspace {workspace_id!r} run queue is full") from exc
        return run_id

    def _notify_done(self, run_id: str) -> None:
        """Fire the run's `on_done` callback (once), off the critical path."""
        cb = self._on_done.pop(run_id, None)
        if cb is None:
            return
        run = models.get_run(run_id)
        if run is None:  # pragma: no cover
            return

        async def _run_cb() -> None:
            try:
                await cb(run)
            except Exception:  # a webhook failure must not take the runner down
                log.exception("on_done callback failed for run %s", run_id)

        task = asyncio.create_task(_run_cb())
        self._bg.add(task)
        task.add_done_callback(self._bg.discard)

    async def cancel(self, run_id: str) -> bool:
        self._cancelled.add(run_id)
        proc = self._running.get(run_id)
        if proc is not None:
            await _proc.terminate(proc)
            return True
        # still queued — the worker will skip it
        run = models.get_run(run_id)
        if run is not None and run.status == "queued":
            models.update_run(run_id, status="cancelled", ended_at=models._now())
            self._notify_done(run_id)
            return True
        return False

    # ── worker ───────────────────────────────────────────────────────────────

    async def _worker(self, workspace_id: str, ws_path: str) -> None:
        """One worker per workspace, for the lifetime of the process.

        The loop body is guarded because **this task dying is unrecoverable**: there is one worker
        per workspace, nothing supervises or restarts it, and `submit` only creates one when the
        queue does not yet exist. If an exception escapes here the task ends, that workspace's
        queue is never drained again, and every subsequent run sits at "queued" until the queue
        fills and every request 429s — with no error anywhere pointing at the cause.

        `_execute` has its own handler, but not everything runs inside it: the cancelled-branch
        `update_run` below, `_notify_done`, and `_execute`'s own final status write all sit
        outside it, and any of them can raise if the database is briefly unavailable. A single
        transient failure would have cost that workspace its runner permanently.

        `CancelledError` is BaseException and is deliberately not caught — `aclose` cancels these
        tasks on shutdown and must be able to.
        """
        queue = self._queues[workspace_id]
        while True:
            run_id, params = await queue.get()
            try:
                if run_id in self._cancelled:
                    models.update_run(run_id, status="cancelled", ended_at=models._now())
                    self._notify_done(run_id)
                    continue
                await self._execute(run_id, ws_path, params)
            except Exception:
                log.exception(
                    "run %s failed outside the execute handler; worker for workspace %s survives",
                    run_id,
                    workspace_id,
                )
                # Best effort: never leave the row stuck at "running"/"queued" just because the
                # bookkeeping is what failed.
                with contextlib.suppress(Exception):
                    models.update_run(run_id, status="failed", ended_at=models._now())
            finally:
                queue.task_done()

    async def _execute(self, run_id: str, ws_path: str, params: dict) -> None:
        run = models.get_run(run_id)
        if run is None:  # pragma: no cover
            return
        command = BY_NAME[run.command]
        log_path = Path(run.log_path)
        await _afile.mkdirs(log_path.parent)
        models.update_run(run_id, status="running", started_at=models._now())

        def register(proc: asyncio.subprocess.Process) -> None:
            self._running[run_id] = proc

        final = "failed"
        try:
            if command.stages:
                final = await self._run_chain(run_id, ws_path, command, log_path, register)
            else:
                argv = [self._settings.ekos_bin, *command.render_argv(params)]
                code = await _proc.run_streaming(
                    argv,
                    cwd=ws_path,
                    log_path=log_path,
                    register=register,
                    timeout_s=command.timeout,
                )
                final = _status_for(run_id, code, self._cancelled)
                models.update_run(run_id, exit_code=code)
        except Exception as exc:  # never leave a run stuck at "running"
            final = "failed"
            # The log write is itself allowed to fail (full disk, unlinked run directory) without
            # masking the original error or escaping into the worker loop.
            with contextlib.suppress(OSError):
                await _afile.append_text(log_path, f"\n[console] run failed: {exc!r}\n")
        finally:
            self._running.pop(run_id, None)
            self._cancelled.discard(run_id)

        models.update_run(run_id, status=final, ended_at=models._now())
        self._notify_done(run_id)

    async def _run_chain(
        self,
        run_id: str,
        ws_path: str,
        command: Command,
        log_path: Path,
        register,
    ) -> str:
        run = models.get_run(run_id)
        if run is None:  # deleted mid-flight; nothing to chain
            return "failed"
        stages = list(run.stages)
        for i, stage in enumerate(stages):
            if run_id in self._cancelled:
                stage["status"] = "cancelled"
                models.update_run(run_id, stages=stages)
                return "cancelled"
            await _afile.append_text(log_path, f"\n[console] === stage: {stage['name']} ===\n")
            stage["status"] = "running"
            models.update_run(run_id, stages=list(stages))
            stage_argv = [self._settings.ekos_bin, stage["name"]]
            if stage["name"] == "commit":
                stage_argv.append("--yes")
            code = await _proc.run_streaming(
                stage_argv,
                cwd=ws_path,
                log_path=log_path,
                register=register,
                timeout_s=command.timeout,
            )
            stage["exit_code"] = code
            stage["status"] = "succeeded" if code == 0 else "failed"
            models.update_run(run_id, stages=list(stages), exit_code=code)
            if code != 0:
                for later in stages[i + 1 :]:
                    later["status"] = "skipped"
                models.update_run(run_id, stages=list(stages))
                return _status_for(run_id, code, self._cancelled)
        return _status_for(run_id, 0, self._cancelled)


def _status_for(run_id: str, code: int, cancelled: set[str]) -> str:
    if run_id in cancelled:
        return "cancelled"
    if code == 124:
        return "timed_out"
    return "succeeded" if code == 0 else "failed"
