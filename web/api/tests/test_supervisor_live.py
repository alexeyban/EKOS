"""McpSupervisor against a real `ekos` binary (RFC 0129 §2).

Skipped unless $EKOS_BIN points at a built `ekos` binary.
"""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

from app.models import Workspace
from app.settings import Settings
from app.supervisor import McpSupervisor, ServerState, _NotReady


def _settings(ekos_bin: str, tmp_path: Path) -> Settings:
    return Settings(
        ekos_bin=ekos_bin,
        console_db=str(tmp_path / "console.db"),
        mcp_port_base=17000,
        console_token="t",
    )


@pytest.fixture
def compiled_workspace(tmp_path: Path, ekos_bin: str) -> Path:
    import subprocess

    ws = tmp_path / "ws"
    ws.mkdir()
    subprocess.run([ekos_bin, "init"], cwd=ws, check=True, capture_output=True)
    return ws


async def test_spawns_a_ready_server_and_serves_a_tool(compiled_workspace, ekos_bin, tmp_path):
    sup = McpSupervisor(_settings(ekos_bin, tmp_path))
    ws = Workspace(id="w", name="W", path=str(compiled_workspace))
    try:
        handle = await sup.ensure(ws)
        assert handle.state is ServerState.READY, handle.detail
        client = sup.client_for("w")
        names = {t["name"] for t in await client.list_tools()}
        assert "ekos_graph_export" in names
    finally:
        await sup.aclose()


async def test_restarts_the_server_after_it_is_killed(compiled_workspace, ekos_bin, tmp_path):
    sup = McpSupervisor(_settings(ekos_bin, tmp_path))
    ws = Workspace(id="w", name="W", path=str(compiled_workspace))
    try:
        handle = await sup.ensure(ws)
        assert handle.state is ServerState.READY
        handle.proc.kill()

        for _ in range(100):
            await asyncio.sleep(0.1)
            current = sup.handle("w")
            if current is not handle and current is not None and current.state is ServerState.READY:
                break
        else:
            pytest.fail("supervisor did not bring a fresh server back up")
    finally:
        await sup.aclose()


async def test_stop_removes_the_handle_and_kills_the_process(
    compiled_workspace, ekos_bin, tmp_path
):
    sup = McpSupervisor(_settings(ekos_bin, tmp_path))
    ws = Workspace(id="w", name="W", path=str(compiled_workspace))
    handle = await sup.ensure(ws)
    proc = handle.proc
    await sup.stop("w")
    assert sup.handle("w") is None
    assert proc.returncode is not None
    with pytest.raises(_NotReady):
        sup.client_for("w")
    await sup.aclose()
