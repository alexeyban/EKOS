"""Scheduled runs end to end: fire → real JobRunner → webhook (RFC 0132).

Skipped unless $EKOS_BIN points at a built `ekos` binary.
"""

from __future__ import annotations

import asyncio
import subprocess
from collections.abc import AsyncIterator
from pathlib import Path

import httpx
import pytest

from app import models
from app.main import create_app

W = {"Authorization": "Bearer stok"}


@pytest.fixture
async def sched_client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> AsyncIterator[tuple[httpx.AsyncClient, list[dict]]]:
    ws = tmp_path / "ws"
    ws.mkdir()
    subprocess.run(  # noqa: ASYNC221
        [ekos_bin, "init"], cwd=ws, check=True, capture_output=True
    )
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "stok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "stok")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_RUNS_DIR", str(tmp_path / "runs"))
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    monkeypatch.setenv("EKOS_CONSOLE_MCP_PORT_BASE", "19800")

    hits: list[dict] = []

    app = create_app()
    async with app.router.lifespan_context(app):

        async def capture(url, body):
            hits.append({"url": url, "body": body})

        app.state.scheduler.post_webhook = capture
        async with httpx.AsyncClient(
            transport=httpx.ASGITransport(app=app), base_url="http://t", headers=W
        ) as client:
            r = await client.post("/api/workspaces", json={"id": "w", "name": "W", "path": str(ws)})
            assert r.status_code == 201, r.text
            yield client, hits


async def test_run_now_submits_a_real_run(sched_client) -> None:
    client, _hits = sched_client
    sid = (
        await client.post(
            "/api/schedules",
            json={
                "workspace_id": "w",
                "command": "doctor",
                "params": {},
                "trigger_kind": "interval",
                "trigger_expr": "86400",
                "notify_url": "https://hook.test/ok",
            },
        )
    ).json()["id"]

    run_id = (await client.post(f"/api/schedules/{sid}/run-now")).json()["run_id"]
    for _ in range(100):
        await asyncio.sleep(0.2)
        if (await client.get(f"/api/runs/{run_id}")).json()["status"] in models.TERMINAL:
            break
    body = (await client.get(f"/api/runs/{run_id}")).json()
    assert body["status"] == "succeeded"
    s = next(x for x in (await client.get("/api/schedules")).json() if x["id"] == sid)
    assert s["last_run_id"] == run_id and s["last_status"] == "succeeded"


async def test_failed_scheduled_run_posts_the_webhook(sched_client) -> None:
    client, hits = sched_client
    # A syntactically-invalid EKL query makes `ekos ekl` exit non-zero → a failed run → webhook.
    sid = (
        await client.post(
            "/api/schedules",
            json={
                "workspace_id": "w",
                "command": "ekl",
                "params": {"query": "!!! not valid ekl !!!"},
                "trigger_kind": "interval",
                "trigger_expr": "86400",
                "notify_url": "https://hook.test/fail",
            },
        )
    ).json()["id"]

    run_id = (await client.post(f"/api/schedules/{sid}/run-now")).json()["run_id"]
    for _ in range(100):
        await asyncio.sleep(0.2)
        if (await client.get(f"/api/runs/{run_id}")).json()["status"] in models.TERMINAL:
            break
    await asyncio.sleep(0.3)  # let the on_done task run
    assert (await client.get(f"/api/runs/{run_id}")).json()["status"] == "failed"
    hit = next(h for h in hits if h["url"] == "https://hook.test/fail")
    assert hit["body"]["schedule_id"] == sid
    assert hit["body"]["status"] == "failed"
