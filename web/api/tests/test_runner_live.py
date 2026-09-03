"""JobRunner end to end against a real `ekos` binary (RFC 0131 §3).

Async (httpx.AsyncClient + ASGITransport) so the event loop keeps pumping the runner's worker
task between requests — a sync TestClient would let a long subprocess deadlock on a full pipe.

Skipped unless $EKOS_BIN points at a built `ekos` binary.
"""

from __future__ import annotations

import asyncio
import subprocess
from collections.abc import AsyncIterator
from pathlib import Path

import httpx
import pytest

from app.main import create_app
from app.models import TERMINAL

W = {"Authorization": "Bearer runtok"}


@pytest.fixture
async def runner_client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> AsyncIterator[tuple[httpx.AsyncClient, Path]]:
    ws = tmp_path / "ws"
    ws.mkdir()
    subprocess.run(  # noqa: ASYNC221 - one-off setup before the loop matters
        [ekos_bin, "init"], cwd=ws, check=True, capture_output=True
    )

    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "runtok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "runtok")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_RUNS_DIR", str(tmp_path / "runs"))
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    monkeypatch.setenv("EKOS_CONSOLE_MCP_PORT_BASE", "19500")

    app = create_app()
    transport = httpx.ASGITransport(app=app)
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=transport, base_url="http://t", headers=W) as client:
            r = await client.post("/api/workspaces", json={"id": "w", "name": "W", "path": str(ws)})
            assert r.status_code == 201, r.text
            yield client, ws


async def _wait_terminal(client: httpx.AsyncClient, run_id: str, budget_s: float = 240.0) -> dict:
    loop = asyncio.get_event_loop()
    deadline = loop.time() + budget_s
    body: dict = {"status": "?"}
    while loop.time() < deadline:
        body = (await client.get(f"/api/runs/{run_id}")).json()
        if body["status"] in TERMINAL:
            return body
        await asyncio.sleep(0.3)
    raise AssertionError(f"run {run_id} never terminated (last status {body['status']})")


async def test_doctor_runs_to_succeeded_with_log_output(runner_client) -> None:
    client, _ws = runner_client
    run_id = (await client.post("/api/workspaces/w/commands/doctor")).json()["run_id"]
    body = await _wait_terminal(client, run_id)
    assert body["status"] == "succeeded"
    assert body["exit_code"] == 0
    assert any("EKOS Doctor" in line for line in body["log_tail"])


async def test_pipeline_runs_every_stage(runner_client) -> None:
    client, _ws = runner_client
    run_id = (await client.post("/api/workspaces/w/commands/pipeline")).json()["run_id"]
    body = await _wait_terminal(client, run_id, budget_s=600)
    assert [s["name"] for s in body["stages"]] == [
        "build",
        "recover",
        "resolve",
        "compile",
        "commit",
    ]
    # every stage that ran should have exited 0 (a fresh workspace compiles to a small ledger)
    assert body["status"] == "succeeded", body["log_tail"][-25:]
    assert all(s["status"] == "succeeded" for s in body["stages"])


async def test_sse_stream_replays_and_ends(runner_client) -> None:
    client, _ws = runner_client
    run_id = (await client.post("/api/workspaces/w/commands/doctor")).json()["run_id"]
    await _wait_terminal(client, run_id)
    async with client.stream("GET", f"/api/runs/{run_id}/logs") as r:
        assert r.status_code == 200
        text = "".join([chunk async for chunk in r.aiter_text()])
    assert "event: end" in text and "data:" in text


async def test_two_writes_on_one_workspace_serialise(runner_client) -> None:
    client, _ws = runner_client
    a = (await client.post("/api/workspaces/w/commands/build")).json()["run_id"]
    b = (await client.post("/api/workspaces/w/commands/status")).json()["run_id"]
    # b is queued behind a — it must not be running while a is
    await asyncio.sleep(1.0)
    sa = (await client.get(f"/api/runs/{a}")).json()["status"]
    sb = (await client.get(f"/api/runs/{b}")).json()["status"]
    assert not (sa == "running" and sb == "running")
    await _wait_terminal(client, a)
    await _wait_terminal(client, b)


async def test_cancel_a_running_pipeline_terminates_it(runner_client) -> None:
    client, _ws = runner_client
    run_id = (await client.post("/api/workspaces/w/commands/pipeline")).json()["run_id"]
    for _ in range(60):
        await asyncio.sleep(0.2)
        if (await client.get(f"/api/runs/{run_id}")).json()["status"] == "running":
            break
    await client.post(f"/api/runs/{run_id}/cancel")
    body = await _wait_terminal(client, run_id, budget_s=60)
    # cancelled if we caught it mid-stage; succeeded if the empty-workspace pipeline beat us to it
    assert body["status"] in {"cancelled", "succeeded"}
    if body["status"] == "cancelled":
        assert not all(s["status"] == "succeeded" for s in body["stages"])


async def test_read_role_cannot_cancel(runner_client) -> None:
    client, _ws = runner_client
    run_id = (await client.post("/api/workspaces/w/commands/build")).json()["run_id"]
    r = await client.post(f"/api/runs/{run_id}/cancel", headers={"Authorization": "Bearer nope"})
    assert r.status_code == 401
    await client.post(f"/api/runs/{run_id}/cancel")  # cleanup
    await _wait_terminal(client, run_id)


async def test_stale_running_rows_are_swept_on_startup(runner_client) -> None:
    _client, _ws = runner_client
    from app import models

    models.add_run(models.Run(id="stale", workspace_id="w", command="build", status="running"))
    app2 = create_app()
    async with app2.router.lifespan_context(app2):
        assert models.get_run("stale").status == "interrupted"
