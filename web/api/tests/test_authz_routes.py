"""RFC 0180: every route declares the action it enforces. A new route without an `authz`
dependency fails here instead of shipping open."""

from __future__ import annotations

from pathlib import Path

import pytest
from fastapi.routing import APIRoute

from app import authz, main
from app.main import create_app

# Reachable without a credential, by design.
PUBLIC = {
    ("GET", "/api/health"),
    ("GET", "/api/auth/me"),  # answers 401 itself, with the sign-in mode
    ("POST", "/api/auth/logout"),
    ("POST", "/api/auth/token-login"),
    ("GET", "/api/auth/login"),
    ("GET", "/api/auth/callback"),
    # RFC 0182: MCP's "no stream / no session here", the same for every path.
    ("GET", "/mcp/{tenant_id}/{project}"),
    ("DELETE", "/mcp/{tenant_id}/{project}"),
}

# Routes whose action depends on the request (which command, which workspace in the body, which
# run). They take `authenticated` and call `authz.require` in the handler; each is listed here
# so that adding one is a reviewed decision, not a default.
DYNAMIC = {
    ("GET", "/api/commands"),  # a static catalogue
    ("POST", "/api/workspaces/{workspace_id}/commands/{name}"),
    ("GET", "/api/runs"),
    ("GET", "/api/runs/{run_id}"),
    ("POST", "/api/runs/{run_id}/cancel"),
    ("GET", "/api/runs/{run_id}/logs"),
    ("GET", "/api/schedules"),
    ("POST", "/api/schedules"),
    ("PATCH", "/api/schedules/{schedule_id}"),
    ("DELETE", "/api/schedules/{schedule_id}"),
    ("POST", "/api/schedules/{schedule_id}/run-now"),
    ("GET", "/api/workspaces"),
    ("POST", "/api/workspaces"),
    ("GET", "/api/tenants"),
    # RFC 0182: the action depends on the tool each message calls.
    ("POST", "/mcp/{tenant_id}/{project}"),
}


def _actions(dependant) -> set[str]:
    found = set()
    action = getattr(dependant.call, authz.ACTION_ATTR, None)
    if action is not None:
        found.add(action)
    for sub in dependant.dependencies:
        found |= _actions(sub)
    return found


@pytest.fixture
def routes(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> dict[tuple[str, str], set[str]]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    out = {}
    # Each router's own routes: they carry the router-level dependencies and the router's
    # prefix. The app's route table is not walked, because how an included router is stored
    # there is a FastAPI internal that has already changed once.
    for prefix, routers in main.MOUNTS:
        for module in routers:
            for route in module.router.routes:
                assert isinstance(route, APIRoute), route
                for method in route.methods - {"HEAD", "OPTIONS"}:
                    out[(method, prefix + route.path)] = _actions(route.dependant)
    return out


def test_the_walk_sees_the_api(routes: dict[tuple[str, str], set[str]]) -> None:
    """Guards the guard: an empty walk would pass every check below."""
    assert len(routes) > 50
    assert ("GET", "/api/workspaces/{workspace_id}/stats") in routes
    # Every path the running app serves under /api is one the walk covered.
    served = {
        (method.upper(), path)
        for path, item in create_app().openapi()["paths"].items()
        for method in item
        if path.startswith(("/api", "/mcp"))
    }
    # The two 405 answers of the gateway are routes without a schema entry.
    unlisted = {
        ("GET", "/mcp/{tenant_id}/{project}"),
        ("DELETE", "/mcp/{tenant_id}/{project}"),
    }
    assert served == set(routes) - unlisted


def test_every_route_declares_an_action(routes: dict[tuple[str, str], set[str]]) -> None:
    undeclared = sorted(k for k, actions in routes.items() if not actions and k not in PUBLIC)
    assert undeclared == [], f"routes with no authz dependency: {undeclared}"


def test_only_listed_routes_decide_their_action_in_the_handler(
    routes: dict[tuple[str, str], set[str]],
) -> None:
    only_authenticated = {k for k, actions in routes.items() if actions == {"authenticated"}}
    assert only_authenticated == DYNAMIC


def test_public_routes_are_exactly_the_listed_ones(
    routes: dict[tuple[str, str], set[str]],
) -> None:
    assert {k for k, actions in routes.items() if not actions} == PUBLIC


def test_workspace_routes_resolve_the_workspace(routes: dict[tuple[str, str], set[str]]) -> None:
    """A path with `{workspace_id}` must check a role on that workspace, not merely a sign-in."""
    for (method, path), actions in routes.items():
        if "{workspace_id}" not in path or (method, path) in DYNAMIC:
            continue
        assert actions & {"read", "review", "operate", "admin"}, (method, path, actions)


def test_review_and_tenant_routes_use_the_expected_actions(
    routes: dict[tuple[str, str], set[str]],
) -> None:
    semantics = "/api/workspaces/{workspace_id}/semantics"
    assert "review" in routes[("POST", semantics + "/items/{item_id}/review")]
    assert "review" in routes[("POST", semantics + "/review-bulk")]
    assert "review" in routes[("POST", "/api/workspaces/{workspace_id}/semantics/linkml/import")]
    assert (
        "review"
        in routes[("POST", "/api/workspaces/{workspace_id}/conflicts/{conflict_id}/resolve")]
    )
    assert "operate" in routes[("PUT", "/api/workspaces/{workspace_id}/config")]
    assert "admin" in routes[("DELETE", "/api/workspaces/{workspace_id}")]
    assert "platform" in routes[("POST", "/api/tenants")]
    assert "platform" in routes[("PATCH", "/api/tenants/{tenant_id}")]
    for (method, path), actions in routes.items():
        if path.startswith("/api/tenants/{tenant_id}/"):
            assert "tenant:admin" in actions, (method, path)


# ── RFC 0181: which routes a restricted reader may reach ─────────────────────

# Routes that read knowledge only through the workspace's MCP server, with the caller's scope
# attached. Everything else under a workspace refuses someone who sees part of it.
SCOPED = {
    ("GET", "/api/workspaces/{workspace_id}/graph"),
    ("GET", "/api/workspaces/{workspace_id}/objects/{object_id}"),
    ("GET", "/api/workspaces/{workspace_id}/search"),
    ("GET", "/api/workspaces/{workspace_id}/neighborhood/{object_id}"),
    ("GET", "/api/workspaces/{workspace_id}/impact/{object_id}"),
    ("POST", "/api/workspaces/{workspace_id}/graph/layout"),
    ("GET", "/api/workspaces/{workspace_id}/stats/kinds"),
}


def _scoped_flags(dependant) -> set[bool]:
    found = set()
    flag = getattr(dependant.call, authz.SCOPED_ATTR, None)
    if flag is not None:
        found.add(flag)
    for sub in dependant.dependencies:
        found |= _scoped_flags(sub)
    return found


def _calls(dependant) -> set:
    found = {dependant.call}
    for sub in dependant.dependencies:
        found |= _calls(sub)
    return found


def test_only_the_listed_routes_are_open_to_a_restricted_reader(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    from app.deps import mcp_for_workspace

    scoped, uses_mcp = set(), set()
    for prefix, routers in main.MOUNTS:
        for module in routers:
            for route in module.router.routes:
                for method in route.methods - {"HEAD", "OPTIONS"}:
                    key = (method, prefix + route.path)
                    flags = _scoped_flags(route.dependant)
                    assert len(flags) <= 1, f"{key} is marked both scoped and not"
                    if flags == {True}:
                        scoped.add(key)
                    if mcp_for_workspace in _calls(route.dependant):
                        uses_mcp.add(key)
    assert scoped == SCOPED
    # A scoped route reads through the scoped MCP client and nothing else. The layout route
    # computes positions from the ids in the request body and reads nothing.
    assert scoped - uses_mcp == {("POST", "/api/workspaces/{workspace_id}/graph/layout")}
    # And no route uses the MCP client without being marked: its result would be right, and the
    # route beside it would not be.
    assert uses_mcp <= scoped
