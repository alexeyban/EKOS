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
        if name == "ekos_neighborhood":
            return {"objects": [{"id": (args or {}).get("id")}], "relationships": []}
        if name == "ekos_impact":
            return {
                "target": {"id": (args or {}).get("id")},
                "direction": (args or {}).get("direction", "dependents"),
                "max_hops": (args or {}).get("max_hops"),
                "count": 0,
                "hops": [],
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


def test_neighborhood_forwards_id_and_depth(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, recorder = rec
    r = client.get("/api/workspaces/any/neighborhood/abc?depth=2", headers=AUTH)
    assert r.status_code == 200, r.text
    name, args = recorder.calls[-1]
    assert name == "ekos_neighborhood"
    assert args == {"id": "abc", "depth": 2}
    assert r.json()["objects"] == [{"id": "abc"}]


def test_neighborhood_defaults_depth_to_one(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, recorder = rec
    client.get("/api/workspaces/any/neighborhood/abc", headers=AUTH)
    _, args = recorder.calls[-1]
    assert args["depth"] == 1


def test_neighborhood_rejects_depth_above_three(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, _ = rec
    r = client.get("/api/workspaces/any/neighborhood/abc?depth=4", headers=AUTH)
    assert r.status_code == 422


def test_impact_forwards_direction_max_hops_and_kinds(
    rec: tuple[TestClient, RecordingMcp],
) -> None:
    client, recorder = rec
    r = client.get(
        "/api/workspaces/any/impact/abc?direction=dependencies&max_hops=3&kind=ForeignKey&kind=DependsOn",
        headers=AUTH,
    )
    assert r.status_code == 200, r.text
    name, args = recorder.calls[-1]
    assert name == "ekos_impact"
    assert args == {
        "id": "abc",
        "direction": "dependencies",
        "max_hops": 3,
        "kinds": ["ForeignKey", "DependsOn"],
    }


def test_impact_defaults_direction_to_dependents_and_omits_kinds(
    rec: tuple[TestClient, RecordingMcp],
) -> None:
    client, recorder = rec
    client.get("/api/workspaces/any/impact/abc", headers=AUTH)
    _, args = recorder.calls[-1]
    assert args["direction"] == "dependents"
    assert "kinds" not in args


def test_impact_rejects_an_invalid_direction(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, _ = rec
    r = client.get("/api/workspaces/any/impact/abc?direction=sideways", headers=AUTH)
    assert r.status_code == 422


def test_impact_translates_a_tool_error_into_a_404(
    rec: tuple[TestClient, RecordingMcp],
) -> None:
    client, recorder = rec

    async def failing_call_tool(name: str, args: dict[str, Any] | None = None) -> Any:
        from app.mcp_client import McpToolError

        raise McpToolError("object not found: abc")

    recorder.call_tool = failing_call_tool  # type: ignore[method-assign]
    r = client.get("/api/workspaces/any/impact/abc", headers=AUTH)
    assert r.status_code == 404


def test_graph_layout_returns_a_position_per_node(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, _ = rec
    r = client.post(
        "/api/workspaces/any/graph/layout",
        headers=AUTH,
        json={"nodes": ["a", "b", "c"], "edges": [["a", "b"], ["b", "c"]]},
    )
    assert r.status_code == 200, r.text
    positions = r.json()["positions"]
    assert set(positions.keys()) == {"a", "b", "c"}
    for xy in positions.values():
        assert len(xy) == 2


def test_graph_layout_rejects_a_malformed_body(rec: tuple[TestClient, RecordingMcp]) -> None:
    client, _ = rec
    r = client.post(
        "/api/workspaces/any/graph/layout",
        headers=AUTH,
        json={"nodes": ["a"]},  # missing required `edges`
    )
    assert r.status_code == 422
