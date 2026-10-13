"""RFC 0180 through the HTTP API: two tenants, people signed in through OIDC (the session
cookie is signed here with the test secret, the way the callback would set it), API tokens,
the audit log, the schema migration and production mode."""

from __future__ import annotations

import base64
import json
import sqlite3
from collections.abc import Iterator
from datetime import timedelta
from pathlib import Path

import itsdangerous
import pytest
from fastapi.testclient import TestClient
from pydantic import ValidationError

from app import models
from app.main import create_app
from app.routes.auth import user_for_claims

SECRET = "test-session-secret"


def _cookie(user_id: str, claims: list[str] | None = None) -> str:
    payload = {"user": {"kind": "user", "id": user_id, "claims": claims or []}}
    data = base64.b64encode(json.dumps(payload).encode())
    return itsdangerous.TimestampSigner(SECRET).sign(data).decode()


class As:
    """A client bound to one identity."""

    def __init__(self, client: TestClient, *, cookie: str | None = None, bearer: str | None = None):
        self._client = client
        self._headers = {}
        if cookie:
            self._headers["Cookie"] = f"session={cookie}"
        if bearer:
            self._headers["Authorization"] = f"Bearer {bearer}"

    def request(self, method: str, url: str, **kw):
        self._client.cookies.clear()
        return self._client.request(method, url, headers=self._headers, **kw)

    def get(self, url: str, **kw):
        return self.request("GET", url, **kw)

    def post(self, url: str, body: dict | None = None):
        return self.request("POST", url, json=body or {})

    def put(self, url: str, body: dict):
        return self.request("PUT", url, json=body)

    def patch(self, url: str, body: dict):
        return self.request("PATCH", url, json=body)

    def delete(self, url: str):
        return self.request("DELETE", url)


def _workspace(root: Path, name: str) -> Path:
    path = root / name
    (path / ".ekos").mkdir(parents=True)
    (path / "ekos.toml").write_text("[observe]\npaths = []\n")
    return path


@pytest.fixture
def env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None) -> Path:
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_ISSUER", "https://idp.test")
    monkeypatch.setenv("EKOS_CONSOLE_OWNERS", "root@corp.io")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", SECRET)
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "console.db"))
    monkeypatch.setenv("EKOS_CONSOLE_RUNS_DIR", str(tmp_path / "runs"))
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_ROOT", str(tmp_path / "src"))
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_JSON", "[]")
    # Access comes from bindings only, as in production mode, so the tests see the real rules
    # and not the RFC 0131 mapping.
    monkeypatch.setenv("EKOS_CONSOLE_PRODUCTION", "true")
    monkeypatch.setenv("EKOS_BIN", "/bin/false")
    monkeypatch.delenv("EKOS_HOME", raising=False)
    (tmp_path / "src" / "acme").mkdir(parents=True)
    (tmp_path / "src" / "globex").mkdir(parents=True)
    return tmp_path


@pytest.fixture
def client(env: Path) -> Iterator[TestClient]:
    with TestClient(create_app()) as c:
        yield c


def _person(client: TestClient, email: str, claims: list[str] | None = None) -> As:
    """Sign a person in: the same record the OIDC callback would create."""
    user = user_for_claims({"sub": f"sub-{email}", "email": email})
    return As(client, cookie=_cookie(user.id, claims))


@pytest.fixture
def world(client: TestClient, env: Path) -> dict[str, As]:
    """Two tenants with one workspace each; ann is admin of acme, gus of globex, both is in
    both; vic views acme, olga operates it, rita reviews it."""
    root = _person(client, "root@corp.io")
    for tenant in ("acme", "globex"):
        r = root.post(
            "/api/tenants",
            {"id": tenant, "name": tenant.title(), "source_roots": [str(env / "src" / tenant)]},
        )
        assert r.status_code == 201, r.text
        ws = _workspace(env / "src" / tenant, "api")
        r = root.post(
            "/api/workspaces", {"id": "api", "name": "API", "path": str(ws), "tenant": tenant}
        )
        assert r.status_code == 201, r.text
        assert r.json()["id"] == f"{tenant}.api"

    def grant(tenant: str, email: str, role: str, workspace: str | None = None) -> None:
        body = {"email": email, "role": role}
        if workspace:
            body["workspace_id"] = workspace
        r = root.post(f"/api/tenants/{tenant}/bindings", body)
        assert r.status_code == 201, r.text

    grant("acme", "ann@acme.io", "admin")
    grant("globex", "gus@globex.io", "admin")
    grant("acme", "both@x.io", "viewer")
    grant("globex", "both@x.io", "viewer")
    grant("acme", "vic@acme.io", "viewer")
    grant("acme", "olga@acme.io", "operator")
    grant("acme", "rita@acme.io", "reviewer")
    people = {"root": root}
    for name, email in [
        ("ann", "ann@acme.io"),
        ("gus", "gus@globex.io"),
        ("both", "both@x.io"),
        ("vic", "vic@acme.io"),
        ("olga", "olga@acme.io"),
        ("rita", "rita@acme.io"),
        ("nobody", "nobody@x.io"),
    ]:
        people[name] = _person(client, email)
    return people


def _ids(who: As) -> list[str]:
    return sorted(w["id"] for w in who.get("/api/workspaces").json())


# ── who sees what ────────────────────────────────────────────────────────────


def test_each_person_sees_only_their_tenants_workspaces(world: dict[str, As]) -> None:
    assert _ids(world["ann"]) == ["acme.api"]
    assert _ids(world["gus"]) == ["globex.api"]
    assert _ids(world["both"]) == ["acme.api", "globex.api"]
    assert _ids(world["nobody"]) == []
    assert _ids(world["root"]) == ["acme.api", "globex.api"]

    assert [t["id"] for t in world["ann"].get("/api/tenants").json()] == ["acme"]
    assert [t["id"] for t in world["nobody"].get("/api/tenants").json()] == []


def test_another_tenants_workspace_is_404_like_one_that_does_not_exist(
    world: dict[str, As],
) -> None:
    ann = world["ann"]
    foreign = ann.get("/api/workspaces/globex.api/config")
    missing = ann.get("/api/workspaces/no-such/config")
    assert foreign.status_code == missing.status_code == 404
    assert foreign.json()["detail"].replace("globex.api", "X") == missing.json()["detail"].replace(
        "no-such", "X"
    )
    # Every kind of workspace route, including a write.
    for method, url in [
        ("GET", "/api/workspaces/globex.api/stats"),
        ("GET", "/api/workspaces/globex.api/semantics/items"),
        ("GET", "/api/workspaces/globex.api/conflicts"),
        ("GET", "/api/workspaces/globex.api/graph"),
        ("POST", "/api/workspaces/globex.api/commands/status"),
        ("DELETE", "/api/workspaces/globex.api"),
    ]:
        assert ann.request(method, url).status_code == 404, (method, url)
    # And the tenant itself.
    assert ann.get("/api/tenants/globex/bindings").status_code == 404
    assert ann.get("/api/tenants/no-such/bindings").status_code == 404


def test_unauthenticated_is_401_before_anything_is_looked_up(client: TestClient) -> None:
    anon = As(client)
    for url in ["/api/workspaces", "/api/workspaces/acme.api/config", "/api/tenants", "/api/runs"]:
        assert anon.get(url).status_code == 401, url


def test_me_lists_tenants_and_roles(world: dict[str, As]) -> None:
    me = world["olga"].get("/api/auth/me").json()
    assert me["tenants"] == [{"id": "acme", "name": "Acme", "roles": ["operator"]}]
    assert me["owner"] is False and me["role"] == "write"
    assert world["vic"].get("/api/auth/me").json()["role"] == "read"
    root = world["root"].get("/api/auth/me").json()
    assert root["owner"] is True
    assert {t["id"] for t in root["tenants"]} == {"default", "acme", "globex"}


# ── what a role may do ───────────────────────────────────────────────────────


def test_a_viewer_reads_and_cannot_run_edit_review_or_administer(world: dict[str, As]) -> None:
    vic = world["vic"]
    assert vic.get("/api/workspaces/acme.api/config").status_code == 200
    assert vic.post("/api/workspaces/acme.api/commands/build").status_code == 403
    assert vic.put("/api/workspaces/acme.api/config", {"raw": ""}).status_code == 403
    review = {"action": "confirm", "reviewer": "x"}
    url = "/api/workspaces/acme.api/semantics/items/00000000-0000-0000-0000-000000000000/review"
    assert vic.post(url, review).status_code == 403
    assert vic.delete("/api/workspaces/acme.api").status_code == 403
    assert vic.get("/api/tenants/acme/bindings").status_code == 403
    assert vic.post("/api/tenants", {"id": "x", "name": "X"}).status_code == 403


def test_an_operator_runs_and_cannot_review_or_administer(world: dict[str, As]) -> None:
    olga = world["olga"]
    run = olga.post("/api/workspaces/acme.api/commands/build")
    assert run.status_code == 200, run.text
    assert olga.get(f"/api/runs/{run.json()['run_id']}").status_code == 200
    url = "/api/workspaces/acme.api/semantics/review-bulk"
    assert olga.post(url, {"action": "confirm", "ids": []}).status_code == 403
    assert olga.delete("/api/workspaces/acme.api").status_code == 403
    assert olga.get("/api/tenants/acme/tokens").status_code == 403


def test_a_reviewer_reviews_and_cannot_run(world: dict[str, As]) -> None:
    rita = world["rita"]
    assert rita.post("/api/workspaces/acme.api/commands/build").status_code == 403
    url = "/api/workspaces/acme.api/semantics/items/00000000-0000-0000-0000-000000000000/review"
    # Past authorization: the CLI (here /bin/false) then fails, which is not a 401/403/404.
    assert rita.post(url, {"action": "confirm"}).status_code not in (401, 403, 404)


def test_runs_of_another_tenant_are_not_listed_or_readable(world: dict[str, As]) -> None:
    run_id = world["olga"].post("/api/workspaces/acme.api/commands/build").json()["run_id"]
    assert [r["id"] for r in world["ann"].get("/api/runs").json()] == [run_id]
    assert world["gus"].get("/api/runs").json() == []
    assert world["gus"].get(f"/api/runs/{run_id}").status_code == 404
    assert world["gus"].get(f"/api/runs/{run_id}/logs").status_code == 404
    assert world["gus"].post(f"/api/runs/{run_id}/cancel").status_code == 404
    assert world["vic"].post(f"/api/runs/{run_id}/cancel").status_code == 403


def test_schedules_follow_the_workspace(world: dict[str, As]) -> None:
    body = {
        "workspace_id": "acme.api",
        "command": "status",
        "trigger_kind": "interval",
        "trigger_expr": "3600",
        "notify_url": "https://hooks.test/x",
    }
    assert world["vic"].post("/api/schedules", body).status_code == 403
    assert world["gus"].post("/api/schedules", body).status_code == 404
    made = world["olga"].post("/api/schedules", body)
    assert made.status_code == 201, made.text
    sid = made.json()["id"]
    assert [s["id"] for s in world["both"].get("/api/schedules").json()] == [sid]
    assert world["gus"].get("/api/schedules").json() == []
    assert world["gus"].delete(f"/api/schedules/{sid}").status_code == 404
    assert world["vic"].delete(f"/api/schedules/{sid}").status_code == 403
    assert world["olga"].delete(f"/api/schedules/{sid}").status_code == 204


def test_a_project_binding_shows_one_workspace(world: dict[str, As], env: Path) -> None:
    ws = _workspace(env / "src" / "acme", "web")
    made = world["ann"].post(
        "/api/workspaces", {"id": "web", "name": "Web", "path": str(ws), "tenant": "acme"}
    )
    assert made.status_code == 201, made.text
    world["ann"].post(
        "/api/tenants/acme/bindings",
        {"email": "pat@acme.io", "role": "operator", "workspace_id": "acme.web"},
    )
    pat = As(world["ann"]._client, cookie=_cookie(models.user_by_email("pat@acme.io").id))
    # pat was given the role before signing in; the first sign-in claims the record.
    assert (
        user_for_claims({"sub": "sub-pat", "email": "pat@acme.io"}).id
        == models.user_by_email("pat@acme.io").id
    )
    assert _ids(pat) == ["acme.web"]
    assert pat.get("/api/workspaces/acme.api/config").status_code == 404
    assert pat.post("/api/workspaces/acme.web/commands/build").status_code == 200


# ── administration ───────────────────────────────────────────────────────────


def test_an_admin_manages_their_tenant_only(world: dict[str, As]) -> None:
    ann = world["ann"]
    assert ann.get("/api/tenants/acme/members").status_code == 200
    emails = [m["email"] for m in ann.get("/api/tenants/acme/members").json()]
    assert "vic@acme.io" in emails and "gus@globex.io" not in emails
    # Not a platform owner.
    assert ann.post("/api/tenants", {"id": "evil", "name": "E"}).status_code == 403
    assert ann.patch("/api/tenants/acme", {"source_roots": ["/"]}).status_code == 403
    # Cannot bind in another tenant, or to another tenant's workspace.
    assert (
        ann.post(
            "/api/tenants/globex/bindings", {"email": "ann@acme.io", "role": "admin"}
        ).status_code
        == 404
    )
    r = ann.post(
        "/api/tenants/acme/bindings",
        {"email": "x@acme.io", "role": "viewer", "workspace_id": "globex.api"},
    )
    assert r.status_code == 400


def test_admin_is_a_tenant_role(world: dict[str, As]) -> None:
    r = world["ann"].post(
        "/api/tenants/acme/bindings",
        {"email": "x@acme.io", "role": "admin", "workspace_id": "acme.api"},
    )
    assert r.status_code == 400


def test_removing_a_binding_takes_effect_on_the_next_request(world: dict[str, As]) -> None:
    ann, vic = world["ann"], world["vic"]
    assert _ids(vic) == ["acme.api"]
    binding = next(
        b for b in ann.get("/api/tenants/acme/bindings").json() if b["principal"] == "vic@acme.io"
    )
    assert ann.delete(f"/api/tenants/acme/bindings/{binding['id']}").status_code == 204
    assert _ids(vic) == []
    assert vic.get("/api/workspaces/acme.api/config").status_code == 404


def test_groups_by_membership_and_by_claim(world: dict[str, As], client: TestClient) -> None:
    ann = world["ann"]
    group = ann.post("/api/tenants/acme/groups", {"name": "ops", "oidc_value": "acme-ops"}).json()
    ann.post("/api/tenants/acme/bindings", {"group_id": group["id"], "role": "operator"})

    by_claim = _person(client, "cleo@acme.io", claims=["acme-ops"])
    assert _ids(by_claim) == ["acme.api"]
    without = _person(client, "dan@acme.io", claims=["something-else"])
    assert _ids(without) == []

    ann.post(f"/api/tenants/acme/groups/{group['id']}/members", {"email": "dan@acme.io"})
    assert _ids(without) == ["acme.api"]
    assert without.post("/api/workspaces/acme.api/commands/build").status_code == 200

    assert ann.delete(f"/api/tenants/acme/groups/{group['id']}").status_code == 204
    assert _ids(without) == [] and _ids(by_claim) == []


def test_an_unverified_email_cannot_claim_a_record_made_for_it(world: dict[str, As]) -> None:
    world["ann"].post("/api/tenants/acme/bindings", {"email": "eve@acme.io", "role": "admin"})
    pending = models.user_by_email("eve@acme.io")
    impostor = user_for_claims(
        {"sub": "sub-impostor", "email": "eve@acme.io", "email_verified": False}
    )
    assert impostor.id != pending.id
    assert models.get_user(pending.id).subject == ""


# ── source roots ─────────────────────────────────────────────────────────────


def test_a_workspace_outside_the_tenants_roots_is_refused(world: dict[str, As], env: Path) -> None:
    foreign = _workspace(env / "src" / "globex", "stolen")
    r = world["ann"].post(
        "/api/workspaces", {"id": "stolen", "name": "S", "path": str(foreign), "tenant": "acme"}
    )
    assert r.status_code == 400 and "outside" in r.json()["detail"]


def test_a_tenant_without_roots_can_add_nothing(world: dict[str, As], env: Path) -> None:
    root = world["root"]
    assert root.post("/api/tenants", {"id": "bare", "name": "Bare"}).status_code == 201
    ws = _workspace(env / "src", "anything")
    r = root.post("/api/workspaces", {"id": "any", "name": "A", "path": str(ws), "tenant": "bare"})
    assert r.status_code == 400 and "no source roots" in r.json()["detail"]


def test_tenant_roots_must_be_inside_the_console_root(world: dict[str, As], env: Path) -> None:
    outside = env / "elsewhere"
    outside.mkdir()
    r = world["root"].patch("/api/tenants/acme", {"source_roots": [str(outside)]})
    assert r.status_code == 400
    assert world["root"].post("/api/tenants", {"id": "Bad Name", "name": "x"}).status_code == 400
    assert world["root"].post("/api/tenants", {"id": "acme", "name": "x"}).status_code == 409


# ── API tokens ───────────────────────────────────────────────────────────────


def _token(ann: As, role: str = "operator", **extra) -> dict:
    r = ann.post("/api/tenants/acme/tokens", {"name": "ci", "role": role, **extra})
    assert r.status_code == 201, r.text
    return r.json()


def test_a_token_reads_and_operates_in_its_tenant(world: dict[str, As], client: TestClient) -> None:
    made = _token(world["ann"])
    assert made["token"].startswith("ekos_")
    bot = As(client, bearer=made["token"])
    assert _ids(bot) == ["acme.api"]
    assert bot.post("/api/workspaces/acme.api/commands/build").status_code == 200
    assert bot.get("/api/workspaces/globex.api/config").status_code == 404
    assert bot.get("/api/auth/me").json()["kind"] == "token"

    listed = world["ann"].get("/api/tenants/acme/tokens").json()
    assert [t["prefix"] for t in listed] == [made["prefix"]]
    assert "token" not in listed[0] and listed[0]["last_used_at"] is not None


def test_only_the_hash_of_a_token_is_stored(world: dict[str, As], env: Path) -> None:
    made = _token(world["ann"])
    raw = (env / "console.db").read_bytes()
    assert made["token"].encode() not in raw
    secret_part = made["token"][len(made["prefix"]) :]
    assert secret_part.encode() not in raw


def test_a_token_cannot_review_or_administer(world: dict[str, As], client: TestClient) -> None:
    ann = world["ann"]
    for role in ("reviewer", "admin"):
        assert ann.post("/api/tenants/acme/tokens", {"name": "x", "role": role}).status_code == 422
    made = _token(ann)
    for role in ("reviewer", "admin"):
        r = ann.post("/api/tenants/acme/bindings", {"token_id": made["id"], "role": role})
        assert r.status_code == 400, role

    # A row the API will not write, forced into the database.
    models.add(
        models.RoleBinding(
            id="forced",
            tenant_id="acme",
            principal_kind="token",
            principal_id=made["id"],
            role="admin",
        )
    )
    bot = As(client, bearer=made["token"])
    url = "/api/workspaces/acme.api/semantics/review-bulk"
    assert bot.post(url, {"action": "confirm", "ids": []}).status_code == 403
    assert bot.post("/api/tenants/acme/tokens", {"name": "y", "role": "viewer"}).status_code == 403
    assert bot.delete("/api/workspaces/acme.api").status_code == 403
    assert bot.get("/api/tenants/acme/bindings").status_code == 403


def test_a_revoked_or_expired_or_unknown_token_is_refused(
    world: dict[str, As], client: TestClient
) -> None:
    ann = world["ann"]
    made = _token(ann, role="viewer")
    bot = As(client, bearer=made["token"])
    assert bot.get("/api/workspaces").status_code == 200

    assert ann.delete(f"/api/tenants/acme/tokens/{made['id']}").status_code == 204
    assert bot.get("/api/workspaces").status_code == 401

    expiring = _token(ann, role="viewer", expires_days=1)
    soon = As(client, bearer=expiring["token"])
    assert soon.get("/api/workspaces").status_code == 200
    models.update_token(expiring["id"], expires_at=models._now() - timedelta(seconds=1))
    assert soon.get("/api/workspaces").status_code == 401

    assert As(client, bearer="ekos_" + "x" * 43).get("/api/workspaces").status_code == 401
    # A token is not a session: an API token never works as a static console token either.
    assert As(client, bearer="dev-console-token").get("/api/workspaces").status_code == 401


def test_a_token_scoped_to_one_workspace(
    world: dict[str, As], client: TestClient, env: Path
) -> None:
    ws = _workspace(env / "src" / "acme", "web")
    world["ann"].post(
        "/api/workspaces", {"id": "web", "name": "Web", "path": str(ws), "tenant": "acme"}
    )
    made = _token(world["ann"], role="viewer", workspace_id="acme.web")
    assert _ids(As(client, bearer=made["token"])) == ["acme.web"]


# ── audit ────────────────────────────────────────────────────────────────────


def test_the_audit_log_records_allowed_and_denied_changes(world: dict[str, As]) -> None:
    world["olga"].post("/api/workspaces/acme.api/commands/build")
    world["vic"].post("/api/workspaces/acme.api/commands/build")
    world["vic"].get("/api/workspaces/acme.api/config")  # a read: not recorded

    log = world["ann"].get("/api/tenants/acme/audit").json()
    builds = [e for e in log if e["action"].endswith("/commands/build")]
    assert {(e["actor"], e["outcome"]) for e in builds} == {
        ("olga@acme.io", "allowed"),
        ("vic@acme.io", "denied"),
    }
    assert all(e["action"].startswith("operate: POST") for e in builds)
    assert not [e for e in log if e["action"].startswith("read")]

    # Looking at an admin page changes nothing and is not recorded; being refused one is.
    world["ann"].get("/api/tenants/acme/bindings")
    world["vic"].get("/api/tenants/acme/bindings")
    views = [
        (e["actor"], e["outcome"])
        for e in world["ann"].get("/api/tenants/acme/audit").json()
        if e["action"].endswith("GET /api/tenants/acme/bindings")
    ]
    assert views == [("vic@acme.io", "denied")]
    # Another tenant's admin does not see it.
    assert world["gus"].get("/api/tenants/acme/audit").status_code == 404
    assert world["vic"].get("/api/tenants/acme/audit").status_code == 403


# ── RFC 0181: a restricted viewer ────────────────────────────────────────────


class RecordingMcp:
    """Stands in for the workspace's MCP connection and records the scope each call carried."""

    def __init__(self) -> None:
        self.calls: list[tuple[str, dict | None]] = []

    async def call_tool(self, name: str, arguments=None, *, scope=None):
        self.calls.append((name, scope))
        if name == "ekos_ekl":
            return {"rows": []}
        if name == "ekos_search":
            return []
        return {
            "schema_version": 1,
            "level": "object",
            "as_of": None,
            "counts": {},
            "truncated": {},
            "nodes": [],
            "edges": [],
            "kind_index": [],
            "rel_kind_index": [],
        }


@pytest.fixture
def recorder(client: TestClient) -> RecordingMcp:
    rec = RecordingMcp()
    client.app.state.supervisor.client_for = lambda _workspace_id: rec
    return rec


def _grant_rule(world: dict[str, As], email: str, **rule) -> dict:
    body = {"email": email, "role": "viewer", "workspace_id": "acme.api", **rule}
    r = world["ann"].post("/api/tenants/acme/bindings", body)
    assert r.status_code == 201, r.text
    return r.json()


def test_a_restricted_viewer_reads_through_the_scope_and_nothing_else(
    world: dict[str, As], client: TestClient, recorder: RecordingMcp
) -> None:
    _grant_rule(world, "pia@partner.io", source="payments")
    _grant_rule(world, "pia@partner.io", effect="deny", source="payments", path="secrets/**")
    pia = _person(client, "pia@partner.io")

    listed = pia.get("/api/workspaces").json()
    assert [(w["id"], w["roles"], w["restricted"]) for w in listed] == [
        ("acme.api", ["viewer"], True)
    ]

    # The scoped routes answer, and every call carried her scope.
    expected = {
        "default": "deny",
        "rules": [
            {"effect": "allow", "source": "payments"},
            {"effect": "deny", "source": "payments", "path": "secrets/**"},
        ],
    }
    for url in [
        "/api/workspaces/acme.api/graph",
        "/api/workspaces/acme.api/search?q=orders",
        "/api/workspaces/acme.api/stats/kinds",
    ]:
        assert pia.get(url).status_code == 200, url
    assert [scope for _tool, scope in recorder.calls] == [expected, expected, expected]

    # Everything that shows more than the ledger refuses her.
    for method, url in [
        ("GET", "/api/workspaces/acme.api/stats"),
        ("GET", "/api/workspaces/acme.api/health"),
        ("GET", "/api/workspaces/acme.api/stats/timeline"),
        ("GET", "/api/workspaces/acme.api/stats/queries"),
        ("GET", "/api/workspaces/acme.api/config"),
        ("POST", "/api/workspaces/acme.api/config/preview-scan"),
        ("GET", "/api/workspaces/acme.api/evals/reports"),
        ("GET", "/api/workspaces/acme.api/semantics/items"),
        ("GET", "/api/workspaces/acme.api/semantics/linkml/yaml"),
        ("GET", "/api/workspaces/acme.api/conflicts"),
        ("POST", "/api/workspaces/acme.api/commands/status"),
    ]:
        r = pia.request(method, url)
        assert r.status_code == 403, (method, url, r.text)
        assert "restricted access" in r.json()["detail"]

    # Runs and their logs name the files a run read.
    run_id = world["olga"].post("/api/workspaces/acme.api/commands/build").json()["run_id"]
    assert pia.get("/api/runs").json() == []
    assert pia.get(f"/api/runs/{run_id}").status_code == 404
    assert pia.get(f"/api/runs/{run_id}/logs").status_code == 404
    assert pia.get("/api/schedules").json() == []


def test_an_unrestricted_viewer_sends_no_scope(
    world: dict[str, As], recorder: RecordingMcp
) -> None:
    assert world["vic"].get("/api/workspaces/acme.api/graph").status_code == 200
    assert recorder.calls == [("ekos_graph_export", None)]
    assert world["vic"].get("/api/workspaces").json()[0]["restricted"] is False


def test_a_deny_on_a_plain_viewer_restricts_them(
    world: dict[str, As], recorder: RecordingMcp
) -> None:
    vic = world["vic"]
    assert vic.get("/api/workspaces/acme.api/config").status_code == 200
    _grant_rule(world, "vic@acme.io", effect="deny", path="secrets/**")
    assert vic.get("/api/workspaces/acme.api/config").status_code == 403
    assert vic.get("/api/workspaces/acme.api/graph").status_code == 200
    assert recorder.calls[-1][1] == {
        "default": "allow",
        "rules": [{"effect": "deny", "path": "secrets/**"}],
    }


def test_a_restricted_reviewer_cannot_review(world: dict[str, As]) -> None:
    _grant_rule(world, "rita@acme.io", effect="deny", source="payments")
    url = "/api/workspaces/acme.api/semantics/review-bulk"
    r = world["rita"].post(url, {"action": "confirm", "ids": []})
    assert r.status_code == 403 and "restricted access" in r.json()["detail"]


def test_a_rule_does_not_bind_an_operator_and_the_api_says_so(
    world: dict[str, As], recorder: RecordingMcp
) -> None:
    made = _grant_rule(world, "olga@acme.io", effect="deny", source="payments")
    assert "no effect" in made["warning"]
    assert world["olga"].get("/api/workspaces/acme.api/config").status_code == 200
    world["olga"].get("/api/workspaces/acme.api/graph")
    assert recorder.calls[-1][1] is None
    # No warning for someone it does bind.
    assert "warning" not in _grant_rule(world, "vic@acme.io", effect="deny", source="payments")


def test_rule_bindings_are_validated(world: dict[str, As]) -> None:
    ann = world["ann"]

    def post(**body):
        return ann.post("/api/tenants/acme/bindings", {"email": "x@acme.io", **body})

    base = {"role": "viewer", "workspace_id": "acme.api"}
    assert post(**base, effect="deny").status_code == 400  # hides nothing in particular
    assert post(role="viewer", source="payments").status_code == 400  # no workspace
    assert post(**{**base, "role": "operator"}, source="payments").status_code == 400
    assert post(**{**base, "role": "reviewer"}, source="payments").status_code == 400
    assert post(**base, source="Not A Name").status_code == 400
    assert post(**base, object="not-a-uuid").status_code == 400
    oid = "692f6b3a-52d1-58fc-ad6a-ed624ef3d86f"
    assert post(**base, object=oid, source="payments").status_code == 400
    assert post(**base, path="   ").status_code == 400
    assert post(**base, effect="maybe", source="payments").status_code == 422

    ok = post(**base, source="payments", path="src/**")
    assert ok.status_code == 201
    assert (ok.json()["effect"], ok.json()["source"], ok.json()["path"]) == (
        "allow",
        "payments",
        "src/**",
    )
    assert post(**base, object=oid).status_code == 201


def test_a_token_can_be_restricted_too(
    world: dict[str, As], client: TestClient, recorder: RecordingMcp
) -> None:
    ann = world["ann"]
    made = ann.post(
        "/api/tenants/acme/tokens",
        {"name": "partner", "role": "viewer", "workspace_id": "acme.api"},
    ).json()
    r = ann.post(
        "/api/tenants/acme/bindings",
        {
            "token_id": made["id"],
            "role": "viewer",
            "workspace_id": "acme.api",
            "effect": "deny",
            "source": "payments",
        },
    )
    assert r.status_code == 201, r.text
    bot = As(client, bearer=made["token"])
    assert bot.get("/api/workspaces/acme.api/config").status_code == 403
    assert bot.get("/api/workspaces/acme.api/graph").status_code == 200
    assert recorder.calls[-1][1] == {
        "default": "allow",
        "rules": [{"effect": "deny", "source": "payments"}],
    }


# ── migration and production mode ────────────────────────────────────────────


def test_a_database_from_before_this_rfc_is_upgraded(tmp_path: Path) -> None:
    db = tmp_path / "old.db"
    con = sqlite3.connect(db)
    con.executescript(
        """
        CREATE TABLE workspace (id VARCHAR NOT NULL PRIMARY KEY, name VARCHAR NOT NULL,
                                path VARCHAR NOT NULL, created_at DATETIME NOT NULL);
        INSERT INTO workspace VALUES ('self', 'EKOS', '/repo', '2026-09-01 10:00:00');
        CREATE TABLE run (id VARCHAR NOT NULL PRIMARY KEY, workspace_id VARCHAR NOT NULL,
                          command VARCHAR NOT NULL, params JSON, status VARCHAR NOT NULL,
                          stages JSON, exit_code INTEGER, log_path VARCHAR NOT NULL,
                          created_at DATETIME NOT NULL, started_at DATETIME, ended_at DATETIME);
        INSERT INTO run VALUES ('r1', 'self', 'build', '{}', 'succeeded', '[]', 0, '/x',
                                '2026-09-01 10:00:00', NULL, NULL);
        """
    )
    con.commit()
    con.close()

    models.init_engine(str(db))
    ws = models.get_workspace("self")
    assert ws is not None and ws.tenant_id == "default" and ws.path == "/repo"
    assert models.get_run("r1").status == "succeeded"
    assert models.get_tenant("default") is not None

    # Opening it again changes nothing.
    models.init_engine(str(db))
    assert [w.id for w in models.list_workspaces()] == ["self"]
    version = sqlite3.connect(db).execute("SELECT version FROM schemaversion").fetchone()[0]
    assert version == len(models.MIGRATIONS)


def test_a_database_from_rfc_0180_gains_the_rule_columns(tmp_path: Path) -> None:
    db = tmp_path / "v1.db"
    models.init_engine(str(db))
    con = sqlite3.connect(db)
    # Back to how RFC 0180 left it: no rule columns, version 1, one binding.
    for column in ("effect", "source", "path", "object_id"):
        con.execute(f"ALTER TABLE rolebinding DROP COLUMN {column}")
    con.execute("UPDATE schemaversion SET version = 1")
    con.execute(
        "INSERT INTO rolebinding (id, tenant_id, principal_kind, principal_id, role, "
        "workspace_id, created_by, created_at) "
        "VALUES ('b1', 'default', 'user', 'u1', 'viewer', NULL, '', '2026-10-13 10:00:00')"
    )
    con.commit()
    con.close()

    models.init_engine(str(db))
    row = models.get_binding("b1")
    assert row is not None and row.effect == "allow"
    assert (row.source, row.path, row.object_id) == (None, None, None)


def test_a_new_database_starts_at_the_latest_version(tmp_path: Path) -> None:
    db = tmp_path / "new.db"
    models.init_engine(str(db))
    version = sqlite3.connect(db).execute("SELECT version FROM schemaversion").fetchone()[0]
    assert version == len(models.MIGRATIONS)


@pytest.mark.parametrize(
    "missing", ["EKOS_CONSOLE_OIDC_ISSUER", "EKOS_CONSOLE_OWNERS", "EKOS_CONSOLE_SESSION_SECRET"]
)
def test_production_mode_refuses_to_start_on_development_credentials(
    env: Path, monkeypatch: pytest.MonkeyPatch, missing: str
) -> None:
    from app.settings import Settings

    monkeypatch.chdir(env)  # no stray .env
    monkeypatch.delenv(missing)
    with pytest.raises(ValidationError) as err:
        Settings()
    assert missing in str(err.value)


def test_production_mode_does_not_accept_static_tokens(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    assert (
        client.post("/api/auth/token-login", json={"token": "dev-console-token"}).status_code == 400
    )


def test_outside_production_the_default_tenant_keeps_the_old_mapping(
    env: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv("EKOS_CONSOLE_PRODUCTION")
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_WRITE_VALUES", "ekos-write")
    monkeypatch.delenv("EKOS_CONSOLE_WORKSPACES_ROOT")
    import app.settings as settings_mod

    settings_mod._settings = None
    with TestClient(create_app()) as c:
        root = _person(c, "root@corp.io")
        legacy = _workspace(env, "legacy")
        assert (
            root.post(
                "/api/workspaces", {"id": "legacy", "name": "L", "path": str(legacy)}
            ).status_code
            == 201
        )
        assert root.post("/api/tenants", {"id": "acme", "name": "Acme"}).status_code == 201

        reader = _person(c, "any@x.io")
        writer = _person(c, "w@x.io", claims=["ekos-write"])
        assert _ids(reader) == ["legacy"]
        assert reader.post("/api/workspaces/legacy/commands/build").status_code == 403
        assert writer.post("/api/workspaces/legacy/commands/build").status_code == 200
        # The mapping never reaches another tenant, and never administers.
        assert [t["id"] for t in writer.get("/api/tenants").json()] == ["default"]
        assert writer.delete("/api/workspaces/legacy").status_code == 403
