"""RFC 0134 — the graph route forwards `as_of` / `include_first_seen` to the MCP tools.

No `ekos` binary: `mcp_for_workspace` is overridden with a recorder.
"""

from __future__ import annotations

from collections.abc import Iterator
from typing import Any

import pytest
from fastapi.testclient import TestClient

from app.deps import mcp_for_workspace
from app.main import create_app

AUTH = {"Authorization": "Bearer gtok"}


class RecordingMcp:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, Any]]] = []

    async def call_tool(self, name: str, args: dict[str, Any] | None = None) -> Any:
        self.calls.append((name, args or {}))
        if name == "ekos_graph_export":
            return {
                "schema_version": 1,
                "level": args.get("level", "object") if args else "object",
                "as_of": (args or {}).get("as_of"),
                "counts": {},
                "truncated": {},
                "nodes": [],
                "edges": [],
                "kind_index": [],
                "rel_kind_index": [],
            }
        return {"object": {"id": "x"}, "relationships": [], "evidence": []}


@pytest.fixture
def rec(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Any, reset_settings: None
) -> Iterator[tuple[TestClient, RecordingMcp]]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "gtok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "gtok")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    app = create_app()
    recorder = RecordingMcp()
    app.dependency_overrides[mcp_for_workspace] = lambda: recorder
    with TestClient(app) as c:
        yield c, recorder


def test_graph_forwards_as_of_and_first_seen(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, recorder = rec
    r = client.get(
        "/api/workspaces/any/graph?as_of=2026-08-01T00:00:00Z&include_first_seen=true",
        headers=AUTH,
    )
    assert r.status_code == 200, r.text
    _, args = recorder.calls[-1]
    assert args["as_of"] == "2026-08-01T00:00:00Z"
    assert args["include_first_seen"] is True
    assert r.json()["as_of"] == "2026-08-01T00:00:00Z"


def test_graph_omits_as_of_when_absent(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, recorder = rec
    client.get("/api/workspaces/any/graph", headers=AUTH)
    _, args = recorder.calls[-1]
    assert "as_of" not in args
    assert args["include_first_seen"] is False


def test_object_state_forwards_as_of_as_at(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, recorder = rec
    client.get("/api/workspaces/any/objects/abc?as_of=2026-08-01T00:00:00Z", headers=AUTH)
    name, args = recorder.calls[-1]
    assert name == "ekos_state"
    assert args == {"id": "abc", "at": "2026-08-01T00:00:00Z"}
