"""RFC 0182: servers on demand, idle shutdown and the cap. No real process: `_spawn` is
replaced, so this is about which servers the supervisor keeps."""

from __future__ import annotations

from pathlib import Path

import pytest

from app.models import Workspace
from app.settings import Settings
from app.supervisor import McpSupervisor, ServerHandle, ServerState, _NotReady


class FakeClient:
    async def aclose(self) -> None:
        pass


def supervisor(tmp_path: Path, **overrides) -> tuple[McpSupervisor, list[str]]:
    settings = Settings(
        console_db=str(tmp_path / "c.db"), console_token="t", session_secret="s", **overrides
    )
    sup = McpSupervisor(settings)
    spawned: list[str] = []

    async def spawn(ws: Workspace) -> ServerHandle:
        spawned.append(ws.id)
        token_file = tmp_path / f"{ws.id}-{len(spawned)}.token"
        token_file.write_text("x")
        handle = ServerHandle(workspace_id=ws.id, port=1, token="x", token_file=token_file)
        handle.state = ServerState.READY
        handle.client = FakeClient()
        return handle

    sup._spawn = spawn
    return sup, spawned


def ws(name: str) -> Workspace:
    return Workspace(id=name, name=name, path="/x")


async def test_nothing_starts_until_it_is_needed(tmp_path: Path) -> None:
    sup, spawned = supervisor(tmp_path)
    await sup.start([])
    assert spawned == [] and sup.handle("a") is None
    with pytest.raises(_NotReady):
        sup.client_for("a")
    assert await sup.acquire(ws("a")) is not None
    assert spawned == ["a"]
    # A second use does not start a second server.
    await sup.acquire(ws("a"))
    assert spawned == ["a"]
    await sup.aclose()


async def test_an_idle_server_is_stopped_and_starts_again_on_the_next_use(tmp_path: Path) -> None:
    sup, spawned = supervisor(tmp_path, mcp_idle_seconds=60)
    await sup.acquire(ws("a"))
    await sup.acquire(ws("b"))
    last = sup.handle("b").last_used
    sup.handle("a").last_used = last - 100

    assert await sup.reap_idle(now=last + 1) == ["a"]
    assert sup.handle("a") is None and sup.handle("b") is not None

    await sup.acquire(ws("a"))
    assert spawned == ["a", "b", "a"]
    await sup.aclose()


async def test_zero_idle_seconds_keeps_servers(tmp_path: Path) -> None:
    sup, _ = supervisor(tmp_path, mcp_idle_seconds=0)
    await sup.acquire(ws("a"))
    assert await sup.reap_idle(now=sup.handle("a").last_used + 10**6) == []
    assert sup.handle("a") is not None
    await sup.aclose()


async def test_the_cap_stops_the_least_recently_used(tmp_path: Path) -> None:
    sup, spawned = supervisor(tmp_path, mcp_max_servers=2)
    await sup.acquire(ws("a"))
    await sup.acquire(ws("b"))
    # `a` is used again, so `b` is now the one used longest ago.
    sup.handle("b").last_used -= 10
    await sup.acquire(ws("a"))

    await sup.acquire(ws("c"))
    assert sup.handle("b") is None
    assert sup.handle("a") is not None and sup.handle("c") is not None
    assert spawned == ["a", "b", "c"]
    # Its token file went with it.
    assert not (tmp_path / "b-2.token").exists()
    await sup.aclose()


async def test_using_a_server_keeps_it_from_the_reaper(tmp_path: Path) -> None:
    sup, _ = supervisor(tmp_path, mcp_idle_seconds=60)
    await sup.acquire(ws("a"))
    first = sup.handle("a").last_used
    sup.handle("a").last_used = first - 1000
    await sup.acquire(ws("a"))
    assert sup.handle("a").last_used >= first
    assert await sup.reap_idle() == []
    await sup.aclose()


def test_a_port_in_use_is_skipped(tmp_path: Path) -> None:
    """Another console on the machine (or any program) may already listen on the next port:
    its MCP server answered the probe and the project's server "never became ready"."""
    import socket

    taken = socket.socket()
    taken.bind(("127.0.0.1", 0))
    taken.listen()
    base = taken.getsockname()[1]
    try:
        sup, _ = supervisor(tmp_path, mcp_port_base=base)
        first = sup._alloc_port()
        assert first != base
        assert sup._alloc_port() > first
    finally:
        taken.close()
