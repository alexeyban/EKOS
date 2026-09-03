"""Schedule CRUD / validation / ConsoleScheduler wiring (RFC 0132) — no real subprocess."""

from __future__ import annotations

import asyncio
from collections.abc import Iterator

import pytest
from fastapi.testclient import TestClient

from app import models
from app.main import create_app
from app.scheduler import ScheduleError, build_trigger

W = {"Authorization": "Bearer w"}
R = {"Authorization": "Bearer r"}


@pytest.fixture
def client(monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("[observe]\n")
    (ws / ".ekos").mkdir()
    with TestClient(create_app()) as c:
        c.post("/api/workspaces", headers=W, json={"id": "w", "name": "W", "path": str(ws)})
        yield c


def _mk(client: TestClient, **over) -> dict:
    body = {
        "workspace_id": "w",
        "command": "status",
        "params": {},
        "trigger_kind": "interval",
        "trigger_expr": "3600",
        "notify_url": "https://example.test/hook",
        **over,
    }
    return client.post("/api/schedules", headers=W, json=body)


# ── trigger builder ──────────────────────────────────────────────────────────


def test_build_trigger_validates() -> None:
    assert build_trigger("cron", "0 3 * * *") is not None
    assert build_trigger("interval", "900") is not None
    for bad in [("cron", "not a crontab"), ("interval", "-1"), ("interval", "x"), ("weird", "1")]:
        with pytest.raises(ScheduleError):
            build_trigger(*bad)


# ── CRUD + validation ────────────────────────────────────────────────────────


def test_create_requires_write(client: TestClient) -> None:
    assert _mk(client).status_code == 201
    r = client.post(
        "/api/schedules",
        headers=R,
        json={
            "workspace_id": "w",
            "command": "status",
            "params": {},
            "trigger_kind": "interval",
            "trigger_expr": "3600",
            "notify_url": "https://x.test/h",
        },
    )
    assert r.status_code == 403  # authenticated read token, but write role required


def test_validation_errors(client: TestClient) -> None:
    assert _mk(client, command="nope").status_code == 422
    assert _mk(client, trigger_kind="cron", trigger_expr="bogus").status_code == 422
    assert _mk(client, trigger_kind="interval", trigger_expr="0").status_code == 422
    assert _mk(client, command="ekl", params={}).status_code == 422  # missing required query
    assert _mk(client, notify_url="ftp://x/h").status_code == 422


def test_list_patch_delete_roundtrip(client: TestClient) -> None:
    sid = _mk(client).json()["id"]
    assert [s["id"] for s in client.get("/api/schedules", headers=R).json()] == [sid]

    patched = client.patch(
        f"/api/schedules/{sid}", headers=W, json={"enabled": False, "trigger_expr": "60"}
    )
    assert patched.status_code == 200
    assert patched.json()["enabled"] is False
    assert patched.json()["trigger_expr"] == "60"

    assert client.delete(f"/api/schedules/{sid}", headers=W).status_code == 204
    assert client.get("/api/schedules", headers=R).json() == []


def test_enabled_flag_controls_registration_on_rebuild(client: TestClient) -> None:
    sid = _mk(client).json()["id"]
    assert sid in client.app.state.scheduler.job_ids()

    client.patch(f"/api/schedules/{sid}", headers=W, json={"enabled": False})
    assert sid not in client.app.state.scheduler.job_ids()

    # a fresh app rebuilds jobs from the table — disabled row → no job
    app2 = create_app()
    with TestClient(app2):
        assert sid not in app2.state.scheduler.job_ids()

    client.patch(f"/api/schedules/{sid}", headers=W, json={"enabled": True})
    app3 = create_app()
    with TestClient(app3):
        assert sid in app3.state.scheduler.job_ids()


async def test_fire_submits_and_records(client: TestClient, monkeypatch) -> None:
    sid = _mk(client).json()["id"]
    scheduler = client.app.state.scheduler

    captured: list = []

    async def fake_submit(ws_id, ws_path, command, params, *, on_done=None):
        run = models.Run(id="r1", workspace_id=ws_id, command=command.name, status="succeeded")
        models.add_run(run)
        if on_done:
            await on_done(run)
        captured.append(command.name)
        return "r1"

    monkeypatch.setattr(scheduler._runner, "submit", fake_submit)
    run_id = await scheduler.fire(sid)
    assert run_id == "r1"
    assert captured == ["status"]
    s = models.get_schedule(sid)
    assert s.last_run_id == "r1" and s.last_status == "succeeded"


async def test_failed_run_posts_the_webhook(client: TestClient, monkeypatch) -> None:
    hits: list[dict] = []

    async def fake_post(self, url, json=None, **kw):
        hits.append({"url": url, "body": json})

        class _R:
            status_code = 200

        return _R()

    monkeypatch.setattr("httpx.AsyncClient.post", fake_post)

    sid = _mk(client, notify_url="https://hook.test/x").json()["id"]
    scheduler = client.app.state.scheduler

    async def fake_submit(ws_id, ws_path, command, params, *, on_done=None):
        run = models.Run(id="rf", workspace_id=ws_id, command=command.name, status="failed")
        models.add_run(run)
        if on_done:
            await on_done(run)
        return "rf"

    monkeypatch.setattr(scheduler._runner, "submit", fake_submit)
    await scheduler.fire(sid)
    await asyncio.sleep(0)
    assert hits and hits[0]["url"] == "https://hook.test/x"
    assert hits[0]["body"]["status"] == "failed" and hits[0]["body"]["schedule_id"] == sid


async def test_succeeded_run_posts_nothing(client: TestClient, monkeypatch) -> None:
    posted = []
    monkeypatch.setattr(
        "httpx.AsyncClient.post",
        lambda *a, **k: posted.append(1),  # would be awaited; never reached on success
    )
    sid = _mk(client).json()["id"]
    scheduler = client.app.state.scheduler

    async def fake_submit(ws_id, ws_path, command, params, *, on_done=None):
        run = models.Run(id="rs", workspace_id=ws_id, command=command.name, status="succeeded")
        models.add_run(run)
        if on_done:
            await on_done(run)
        return "rs"

    monkeypatch.setattr(scheduler._runner, "submit", fake_submit)
    await scheduler.fire(sid)
    assert posted == []
