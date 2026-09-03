"""The stats endpoints end to end against a real compiled workspace + supervisor (RFC 0129 §4).

Skipped unless $EKOS_BIN points at a built `ekos` binary. Uses this repo itself as the workspace
so there is real compiled knowledge to report.
"""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

AUTH = {"Authorization": "Bearer live-console"}

_REPO = Path(__file__).resolve().parents[3]


@pytest.fixture
def live_client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> Iterator[TestClient]:
    if not (_REPO / ".ekos").is_dir():
        pytest.skip("this repo has no compiled .ekos/ to serve")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "live-console")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "console.db"))
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    monkeypatch.setenv("EKOS_CONSOLE_MCP_PORT_BASE", "18200")
    with TestClient(create_app()) as c:
        r = c.post(
            "/api/workspaces",
            headers=AUTH,
            json={"id": "self", "name": "EKOS", "path": str(_REPO)},
        )
        assert r.status_code == 201, r.text
        yield c


def test_stats_returns_the_r2_payload(live_client: TestClient) -> None:
    r = live_client.get("/api/workspaces/self/stats", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["schema_version"] == 1
    assert body["objects"] > 0
    assert body["backend"] in {"fact-segment", "sqlite-v1", "sqlite-v2", "partitioned"}
    assert "components" in body["storage"]


def test_health_returns_the_doctor_checklist(live_client: TestClient) -> None:
    r = live_client.get("/api/workspaces/self/health", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body["ok"], bool)
    assert any(c["name"] == ".ekos/" for c in body["checks"])


def test_timeline_is_cumulative(live_client: TestClient) -> None:
    r = live_client.get("/api/workspaces/self/stats/timeline?bucket=month", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["bucket"] == "month"
    if body["points"]:
        assert body["points"][-1]["objects"] > 0


def test_kinds_comes_back_sorted_desc(live_client: TestClient) -> None:
    r = live_client.get("/api/workspaces/self/stats/kinds", headers=AUTH)
    assert r.status_code == 200, r.text
    rows = r.json()
    assert rows and rows[0]["count"] >= rows[-1]["count"]


def test_queries_aggregates_the_usage_log(live_client: TestClient) -> None:
    r = live_client.get("/api/workspaces/self/stats/queries", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"total", "by_tool", "cache_hit_rate", "p50_ms", "p95_ms"}
