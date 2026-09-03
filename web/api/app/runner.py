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
import uuid
from pathlib import Path

from . import _proc, models
from .commands import BY_NAME, Command
from .models import Run
from .settings import Settings


class QueueFull(RuntimeError):
    pass


class JobRunner:
    def __init__(self, settings: Settings) -> None:
        self._settings = settings
        self._queues: dict[str, asyncio.Queue[tuple[str, dict]]] = {}
        self._workers: dict[str, asyncio.Task] = {}
        self._running: dict[str, asyncio.subprocess.Process] = {}  # run_id -> live process
        self._cancelled: set[str] = set()

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

    # ── submission ───────────────────────────────────────────────────────────

    async def submit(self, workspace_id: str, ws_path: str, command: Command, params: dict) -> str:
        command.render_argv(params)  # validate params up front → ValueError to the caller

        run_id = uuid.uuid4().hex
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
            models.update_run(run_id, status="failed", exit_code=None)
            raise QueueFull(f"workspace {workspace_id!r} run queue is full") from exc
        return run_id

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
            return True
        return False

    # ── worker ───────────────────────────────────────────────────────────────

    async def _worker(self, workspace_id: str, ws_path: str) -> None:
        queue = self._queues[workspace_id]
        while True:
            run_id, params = await queue.get()
            try:
                if run_id in self._cancelled:
                    models.update_run(run_id, status="cancelled", ended_at=models._now())
                    continue
                await self._execute(run_id, ws_path, params)
            finally:
                queue.task_done()

    async def _execute(self, run_id: str, ws_path: str, params: dict) -> None:
        run = models.get_run(run_id)
        if run is None:  # pragma: no cover
            return
        command = BY_NAME[run.command]
        log_path = Path(run.log_path)
        log_path.parent.mkdir(parents=True, exist_ok=True)
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
            with log_path.open("a") as fh:
                fh.write(f"\n[console] run failed: {exc!r}\n")
            final = "failed"
        finally:
            self._running.pop(run_id, None)
            self._cancelled.discard(run_id)

        models.update_run(run_id, status=final, ended_at=models._now())

    async def _run_chain(
        self,
        run_id: str,
        ws_path: str,
        command: Command,
        log_path: Path,
        register,
    ) -> str:
        stages = list(models.get_run(run_id).stages)
        for i, stage in enumerate(stages):
            if run_id in self._cancelled:
                stage["status"] = "cancelled"
                models.update_run(run_id, stages=stages)
                return "cancelled"
            with log_path.open("a") as fh:
                fh.write(f"\n[console] === stage: {stage['name']} ===\n")
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
