"""Per-workspace MCP-server supervision (RFC 0129 §2).

The console owns an `ekos mcp serve --tcp` process per workspace in use: it picks a loopback
port, generates a fresh bearer token (RFC 0128 R4), spawns the server, waits for it to answer
`tools/list`, and restarts it with exponential backoff if it crashes.

RFC 0182: a server starts when something first needs it (`acquire`), is stopped after
`mcp_idle_seconds` without use, and at most `mcp_max_servers` run at once. Each server holds its
project's search index in memory, so one per registered project, forever, does not scale.

This is deliberately **not** the Phase 3 job runner. MCP servers are long-lived, idle-cheap, and
want restart-on-crash; pipeline jobs are bursty, heavy, queued, and cancellable. The only shared
code is `_proc` (spawn / SIGTERM-then-SIGKILL).
"""

from __future__ import annotations

import asyncio
import contextlib
import secrets
import tempfile
import time
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path

from . import _proc, home
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
    # Monotonic time of the last `acquire`; what idle shutdown and the cap go by.
    last_used: float = 0.0

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
        self._reaper: asyncio.Task | None = None

    # ── lifecycle ────────────────────────────────────────────────────────────

    async def start(self, workspaces: list[Workspace]) -> None:
        """Start the idle reaper, and a server for each workspace given. The console passes
        none: servers start on first use."""
        for ws in workspaces:
            await self.ensure(ws)
        if self._settings.mcp_idle_seconds > 0 and self._reaper is None:
            self._reaper = asyncio.create_task(self._reap_forever())

    async def ensure(self, ws: Workspace) -> ServerHandle:
        """Return a handle for `ws`, spawning the server if there isn't a live one."""
        evicted: list[tuple[asyncio.Task | None, ServerHandle]] = []
        async with self._lock:
            existing = self._handles.get(ws.id)
            if existing is not None and existing.state is not ServerState.FAILED:
                return existing
            if existing is None:
                evicted = self._make_room()
            handle = await self._spawn(ws)
            handle.last_used = time.monotonic()
            self._handles[ws.id] = handle
            self._monitors[ws.id] = asyncio.create_task(self._monitor(ws, handle))
        for monitor, old in evicted:
            await self._retire(monitor, old)
        return handle

    async def acquire(self, ws: Workspace) -> EkosMcpClient:
        """The client for `ws`'s server, starting the server if it is not running, and marking
        it used. Raises `_NotReady` when it will not serve."""
        if self._handles.get(ws.id) is None:
            await self.ensure(ws)
        handle = self._handles.get(ws.id)
        if handle is not None:
            handle.last_used = time.monotonic()
        return self.client_for(ws.id)

    def _make_room(self) -> list[tuple[asyncio.Task | None, ServerHandle]]:
        """Called with the lock held, before one more server starts: take the least recently
        used servers out of the table until there is room under the cap. The caller tears them
        down after releasing the lock."""
        cap = self._settings.mcp_max_servers
        out: list[tuple[asyncio.Task | None, ServerHandle]] = []
        while cap > 0 and len(self._handles) >= cap:
            oldest = min(self._handles.values(), key=lambda h: h.last_used)
            out.append(
                (
                    self._monitors.pop(oldest.workspace_id, None),
                    self._handles.pop(oldest.workspace_id),
                )
            )
        return out

    async def _retire(self, monitor: asyncio.Task | None, handle: ServerHandle) -> None:
        if monitor is not None:
            monitor.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await monitor
        await self._teardown(handle)

    async def reap_idle(self, now: float | None = None) -> list[str]:
        """Stop every server unused for longer than `mcp_idle_seconds`. Returns their ids."""
        idle = self._settings.mcp_idle_seconds
        if idle <= 0:
            return []
        now = time.monotonic() if now is None else now
        stale = [h.workspace_id for h in self._handles.values() if now - h.last_used > idle]
        for workspace_id in stale:
            await self.stop(workspace_id)
        return stale

    async def _reap_forever(self) -> None:
        interval = max(1.0, min(30.0, self._settings.mcp_idle_seconds / 4))
        while True:
            await asyncio.sleep(interval)
            with contextlib.suppress(Exception):
                await self.reap_idle()

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
        if self._reaper is not None:
            self._reaper.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._reaper
            self._reaper = None
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

        argv = home.mcp_command(
            self._settings.ekos_bin,
            ws.path,
            ["--tcp", f"127.0.0.1:{port}", "--tcp-token-file", str(token_file)],
        )
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
                # A restart is not a use; without this the reaper would see a server last
                # used at time zero.
                replacement.last_used = handle.last_used
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
