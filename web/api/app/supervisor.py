"""Per-workspace MCP-server supervision (RFC 0129 §2).

The console owns one `ekos mcp serve --tcp` process per registered workspace: it picks a loopback
port, generates a fresh bearer token (RFC 0128 R4), spawns the server, waits for it to answer
`tools/list`, and restarts it with exponential backoff if it crashes.

This is deliberately **not** the Phase 3 job runner. MCP servers are long-lived, idle-cheap, and
want restart-on-crash; pipeline jobs are bursty, heavy, queued, and cancellable. The only shared
code is `_proc` (spawn / SIGTERM-then-SIGKILL).
"""

from __future__ import annotations

import asyncio
import contextlib
import secrets
import tempfile
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

from . import _proc
from .mcp_client import EkosMcpClient
from .models import Workspace
from .settings import Settings

_MAX_RETRIES = 5
_BACKOFF_CAP = 30.0
_READY_TIMEOUT = 12.0


class ServerState(StrEnum):
    STARTING = "starting"
    READY = "ready"
    FAILED = "failed"


@dataclass
class ServerHandle:
    workspace_id: str
    port: int
    token: str
    token_file: Path
    proc: asyncio.subprocess.Process | None = None
    state: ServerState = ServerState.STARTING
    retries: int = 0
    client: EkosMcpClient | None = None
    detail: str = ""

    def status(self) -> dict:
        return {
            "state": self.state.value,
            "port": self.port,
            "retries": self.retries,
            "detail": self.detail,
        }


class McpSupervisor:
    def __init__(self, settings: Settings) -> None:
        self._settings = settings
        self._handles: dict[str, ServerHandle] = {}
        self._monitors: dict[str, asyncio.Task] = {}
        self._lock = asyncio.Lock()
        self._next_port = settings.mcp_port_base
        self._tmpdir = Path(tempfile.mkdtemp(prefix="ekos-console-mcp-"))

    # ── lifecycle ────────────────────────────────────────────────────────────

    async def start(self, workspaces: list[Workspace]) -> None:
        for ws in workspaces:
            await self.ensure(ws)

    async def ensure(self, ws: Workspace) -> ServerHandle:
        """Return a handle for `ws`, spawning the server if there isn't a live one."""
        async with self._lock:
            existing = self._handles.get(ws.id)
            if existing is not None and existing.state is not ServerState.FAILED:
                return existing
            handle = await self._spawn(ws)
            self._handles[ws.id] = handle
            self._monitors[ws.id] = asyncio.create_task(self._monitor(ws, handle))
            return handle

    async def stop(self, workspace_id: str) -> None:
        async with self._lock:
            monitor = self._monitors.pop(workspace_id, None)
            handle = self._handles.pop(workspace_id, None)
        if monitor is not None:
            monitor.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await monitor
        if handle is not None:
            await self._teardown(handle)

    async def aclose(self) -> None:
        for workspace_id in list(self._handles):
            await self.stop(workspace_id)
        with contextlib.suppress(OSError):
            for f in self._tmpdir.iterdir():
                f.unlink(missing_ok=True)
            self._tmpdir.rmdir()

    # ── accessors ────────────────────────────────────────────────────────────

    def handle(self, workspace_id: str) -> ServerHandle | None:
        return self._handles.get(workspace_id)

    def client_for(self, workspace_id: str) -> EkosMcpClient:
        handle = self._handles.get(workspace_id)
        if handle is None or handle.state is not ServerState.READY or handle.client is None:
            raise _NotReady(handle.detail if handle else "no server for this workspace")
        return handle.client

    # ── internals ────────────────────────────────────────────────────────────

    def _alloc_port(self) -> int:
        port = self._next_port
        self._next_port += 1
        return port

    async def _spawn(self, ws: Workspace) -> ServerHandle:
        port = self._alloc_port()
        token = secrets.token_hex(32)
        token_file = self._tmpdir / f"{ws.id}-{port}.token"
        token_file.write_text(token + "\n")
        token_file.chmod(0o600)

        argv = [
            self._settings.ekos_bin,
            "mcp",
            "serve",
            "--workspace",
            ws.path,
            "--tcp",
            f"127.0.0.1:{port}",
            "--tcp-token-file",
            str(token_file),
        ]
        handle = ServerHandle(workspace_id=ws.id, port=port, token=token, token_file=token_file)
        try:
            handle.proc = await _proc.spawn(argv)
        except OSError as exc:
            handle.state = ServerState.FAILED
            handle.detail = f"could not launch {self._settings.ekos_bin!r}: {exc}"
            return handle

        try:
            client = EkosMcpClient("127.0.0.1", port, token)
            await asyncio.wait_for(self._connect_when_up(client, handle), timeout=_READY_TIMEOUT)
            await client.list_tools()
            handle.client = client
            handle.state = ServerState.READY
            handle.detail = ""
        except Exception as exc:  # report it on the handle; the monitor decides whether to retry
            handle.state = ServerState.FAILED
            handle.detail = f"never became ready: {exc}"
        return handle

    @staticmethod
    async def _connect_when_up(client: EkosMcpClient, handle: ServerHandle) -> None:
        while True:
            if handle.proc is not None and handle.proc.returncode is not None:
                raise RuntimeError(f"process exited with code {handle.proc.returncode}")
            try:
                await client.connect()
                return
            except (ConnectionError, OSError):
                await asyncio.sleep(0.15)

    async def _monitor(self, ws: Workspace, handle: ServerHandle) -> None:
        """Wait for the current process to exit, then restart with backoff — unless it was
        stopped deliberately (the handle is no longer the registered one)."""
        while True:
            current = self._handles.get(ws.id)
            if current is None or current is not handle:
                return
            if handle.proc is not None:
                await handle.proc.wait()
            async with self._lock:
                if self._handles.get(ws.id) is not handle:
                    return
                handle.retries += 1
                if handle.retries > _MAX_RETRIES:
                    handle.state = ServerState.FAILED
                    handle.detail = f"gave up after {_MAX_RETRIES} restarts"
                    return
            await asyncio.sleep(min(_BACKOFF_CAP, 2.0 ** (handle.retries - 1)))
            async with self._lock:
                if self._handles.get(ws.id) is not handle:
                    return
                replacement = await self._spawn(ws)
                replacement.retries = (
                    0 if replacement.state is ServerState.READY else handle.retries
                )
                self._handles[ws.id] = replacement
            handle = replacement

    async def _teardown(self, handle: ServerHandle) -> None:
        if handle.client is not None:
            with contextlib.suppress(Exception):
                await handle.client.aclose()
        if handle.proc is not None:
            await _proc.terminate(handle.proc)
        handle.token_file.unlink(missing_ok=True)


class _NotReady(RuntimeError):
    """Raised by `client_for` when a workspace's MCP server is not (yet) serving."""
