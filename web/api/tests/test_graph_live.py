"""Graph + object-state endpoints against a real compiled workspace (RFC 0133 §3).

Skipped unless $EKOS_BIN points at a built `ekos` binary. Uses this repo as the workspace.
"""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

AUTH = {"Authorization": "Bearer gtok"}
_REPO = Path(__file__).resolve().parents[3]


@pytest.fixture
def client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> Iterator[TestClient]:
    if not (_REPO / ".ekos").is_dir():
        pytest.skip("this repo has no compiled .ekos/")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "gtok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "gtok")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    monkeypatch.setenv("EKOS_CONSOLE_MCP_PORT_BASE", "19900")
    with TestClient(create_app()) as c:
        c.post("/api/workspaces", headers=AUTH, json={"id": "s", "name": "S", "path": str(_REPO)})
        yield c


def test_aggregate_then_expand_a_kind(client: TestClient) -> None:
    agg = client.get("/api/workspaces/s/graph?level=aggregate&group_by=kind", headers=AUTH).json()
    assert agg["level"] == "aggregate"
    kinds = agg["kind_index"]
    assert "RustSymbol" in kinds

    obj = client.get(
        "/api/workspaces/s/graph?level=object&kind=RustSymbol&max_nodes=25", headers=AUTH
    ).json()
    assert obj["level"] == "object"
    assert 0 < len(obj["nodes"]) <= 25


def test_object_state_returns_evidence(client: TestClient) -> None:
    hit = client.get("/api/workspaces/s/search?q=ledger&limit=1", headers=AUTH).json()
    oid = hit["matches"][0]["id"]
    r = client.get(f"/api/workspaces/s/objects/{oid}", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["object"]["id"] == oid
    assert "relationships" in body and "evidence" in body


def test_object_state_404_for_a_bogus_id(client: TestClient) -> None:
    r = client.get("/api/workspaces/s/objects/00000000-0000-0000-0000-000000000000", headers=AUTH)
    assert r.status_code == 404


def test_include_properties_flag_is_accepted(client: TestClient) -> None:
    r = client.get("/api/workspaces/s/graph?level=aggregate&include_properties=true", headers=AUTH)
    assert r.status_code == 200


def test_as_of_and_first_seen_against_the_repo(client: TestClient) -> None:
    # RFC 0134 — first-seen stamps every node when asked.
    latest = client.get(
        "/api/workspaces/s/graph?level=aggregate&group_by=kind&include_first_seen=true",
        headers=AUTH,
    ).json()
    assert latest["nodes"]
    assert all("fs" in n for n in latest["nodes"])

    # A cutoff years before this repo existed → the graph is empty, and `as_of` is echoed.
    past = client.get(
        "/api/workspaces/s/graph?level=aggregate&group_by=kind&as_of=2020-01-01T00:00:00Z",
        headers=AUTH,
    ).json()
    assert past["as_of"] == "2020-01-01T00:00:00Z"
    assert past["nodes"] == []


def test_object_state_accepts_as_of(client: TestClient) -> None:
    hit = client.get("/api/workspaces/s/search?q=ledger&limit=1", headers=AUTH).json()
    oid = hit["matches"][0]["id"]
    # Far future → still current state; the point is the param is accepted end to end.
    r = client.get(f"/api/workspaces/s/objects/{oid}?as_of=2030-01-01T00:00:00Z", headers=AUTH)
    assert r.status_code == 200, r.text


def test_large_object_level_payload_does_not_overflow_the_ndjson_reader(client: TestClient) -> None:
    # >64 KiB on one NDJSON line — the MCP client's StreamReader limit must be raised past the
    # asyncio default or `readline()` raises "Separator is not found".
    r = client.get(
        "/api/workspaces/s/graph?level=object&kind=RustSymbol&max_nodes=500", headers=AUTH
    )
    assert r.status_code == 200, r.text
    assert len(r.json()["nodes"]) > 100
    assert client.get("/api/workspaces/s/search?q=ledger&limit=50", headers=AUTH).status_code == 200
