"""RFC 0180: `authz.authorize` as a table — every role against every action, tenant and project
bindings, groups, tokens, owners, and the RFC 0131 compatibility mapping. No HTTP."""

from __future__ import annotations

from pathlib import Path

import pytest

from app import authz, models
from app.auth import Principal

ACTIONS = ("read", "review", "operate", "admin")


@pytest.fixture(autouse=True)
def db(tmp_path: Path) -> None:
    models.init_engine(str(tmp_path / "c.db"))
    models.add(models.Tenant(id="acme", name="Acme"))
    models.add(models.Tenant(id="globex", name="Globex"))
    models.add_workspace(models.Workspace(id="acme.api", name="api", path="/a", tenant_id="acme"))
    models.add_workspace(models.Workspace(id="acme.web", name="web", path="/w", tenant_id="acme"))
    models.add_workspace(models.Workspace(id="globex.erp", name="e", path="/e", tenant_id="globex"))
    models.add_workspace(models.Workspace(id="legacy", name="l", path="/l"))


def user(uid: str = "u1", **kw) -> Principal:
    models.add(models.User(id=uid, subject=f"sub-{uid}", email=f"{uid}@x.io"))
    return Principal(kind="user", id=uid, subject=f"sub-{uid}", email=f"{uid}@x.io", **kw)


def bind(kind: str, pid: str, role: str, tenant: str = "acme", workspace: str | None = None):
    models.add(
        models.RoleBinding(
            id=f"b-{kind}-{pid}-{role}-{workspace}",
            tenant_id=tenant,
            principal_kind=kind,
            principal_id=pid,
            role=role,
            workspace_id=workspace,
        )
    )


def can(p: Principal, tenant: str, workspace: str | None = None) -> set[str]:
    return {a for a in ACTIONS if authz.authorize(p, a, tenant, workspace)}


@pytest.mark.parametrize(
    ("role", "expected"),
    [
        ("viewer", {"read"}),
        ("reviewer", {"read", "review"}),
        ("operator", {"read", "operate"}),
        ("admin", {"read", "review", "operate", "admin"}),
    ],
)
def test_each_role_grants_exactly_its_actions(role: str, expected: set[str]) -> None:
    p = user()
    bind("user", "u1", role)
    assert can(p, "acme", "acme.api") == expected
    assert can(p, "acme") == expected
    # Nothing in another tenant, and no platform right.
    assert can(p, "globex", "globex.erp") == set()
    assert not authz.authorize(p, "platform", "")


def test_no_binding_no_access_and_not_visible() -> None:
    p = user()
    assert can(p, "acme", "acme.api") == set()
    assert not authz.sees_tenant(p, "acme")
    assert not authz.sees_workspace(p, models.get_workspace("acme.api"))


def test_a_project_binding_covers_only_that_project() -> None:
    p = user()
    bind("user", "u1", "operator", workspace="acme.api")
    assert can(p, "acme", "acme.api") == {"read", "operate"}
    assert can(p, "acme", "acme.web") == set()
    assert can(p, "acme") == set()
    # The tenant is visible (it has to be listed), its other workspace is not.
    assert authz.sees_tenant(p, "acme")
    assert not authz.sees_workspace(p, models.get_workspace("acme.web"))


def test_roles_add_up() -> None:
    p = user()
    bind("user", "u1", "reviewer")
    bind("user", "u1", "operator", workspace="acme.api")
    assert can(p, "acme", "acme.api") == {"read", "review", "operate"}
    assert can(p, "acme", "acme.web") == {"read", "review"}


def test_group_membership_by_record() -> None:
    p = user()
    models.add(models.Group(id="g1", tenant_id="acme", name="ops"))
    bind("group", "g1", "operator")
    assert can(p, "acme", "acme.api") == set()
    models.set_group_member("g1", "u1", True)
    assert can(p, "acme", "acme.api") == {"read", "operate"}
    models.set_group_member("g1", "u1", False)
    assert can(p, "acme", "acme.api") == set()


def test_group_membership_by_claim_value() -> None:
    models.add(models.Group(id="g2", tenant_id="acme", name="reviewers", oidc_value="acme-review"))
    bind("group", "g2", "reviewer")
    assert can(user("u1", claims=["other"]), "acme", "acme.api") == set()
    assert can(user("u2", claims=["acme-review"]), "acme", "acme.api") == {"read", "review"}


def test_a_group_without_a_claim_value_does_not_match_an_empty_claim() -> None:
    models.add(models.Group(id="g3", tenant_id="acme", name="manual"))
    bind("group", "g3", "admin")
    assert can(user("u1", claims=[""]), "acme") == set()


def test_an_owner_can_do_everything_everywhere() -> None:
    p = user(owner=True)
    for tenant, ws in (("acme", "acme.api"), ("globex", "globex.erp"), ("default", "legacy")):
        assert can(p, tenant, ws) == set(ACTIONS)
    assert authz.authorize(p, "platform", "")


# ── tokens ───────────────────────────────────────────────────────────────────


def token(tid: str = "t1", tenant: str = "acme") -> Principal:
    return Principal(kind="token", id=tid, subject="ci", tenant_id=tenant)


def test_a_token_reads_and_operates_within_its_tenant() -> None:
    bind("token", "t1", "operator")
    assert can(token(), "acme", "acme.api") == {"read", "operate"}
    assert can(token(), "globex", "globex.erp") == set()


def test_a_token_never_reviews_or_administers_even_with_a_forced_binding() -> None:
    # Rows the API refuses to create, written straight to the database.
    bind("token", "t1", "reviewer")
    bind("token", "t1", "admin")
    assert can(token(), "acme", "acme.api") == set()
    assert authz.roles_in(token(), "acme", "acme.api") == set()
    bind("token", "t1", "viewer")
    assert can(token(), "acme", "acme.api") == {"read"}


def test_a_token_is_never_an_owner() -> None:
    forged = Principal(kind="token", id="t9", tenant_id="acme", owner=True)
    assert can(forged, "acme", "acme.api") == set()
    assert not authz.authorize(forged, "platform", "")
    assert not authz.can_anywhere(forged, "operate")


def test_a_binding_in_another_tenant_does_not_follow_a_token() -> None:
    bind("token", "t1", "operator", tenant="globex")
    assert can(token(tenant="acme"), "globex", "globex.erp") == set()
    assert not authz.sees_tenant(token(tenant="acme"), "globex")


# ── static tokens and the RFC 0131 mapping ───────────────────────────────────


def test_static_tokens() -> None:
    read = Principal(kind="static", id="read", compat="read")
    write = Principal(kind="static", id="write", compat="write", owner=True)
    assert can(read, "default", "legacy") == {"read"}
    assert can(read, "acme", "acme.api") == set()
    assert can(write, "acme", "acme.api") == set(ACTIONS)


def test_the_compatibility_mapping_reaches_the_default_tenant_only() -> None:
    reader = user("u1", compat="read")
    writer = user("u2", compat="write")
    assert can(reader, "default", "legacy") == {"read"}
    assert can(writer, "default", "legacy") == {"read", "review", "operate"}
    for p in (reader, writer):
        assert can(p, "acme", "acme.api") == set()


def test_without_the_mapping_the_default_tenant_needs_a_binding() -> None:
    p = user("u1", compat=None)  # production mode
    assert can(p, "default", "legacy") == set()
    bind("user", "u1", "viewer", tenant="default")
    assert can(p, "default", "legacy") == {"read"}


def test_an_unknown_role_in_a_binding_grants_nothing() -> None:
    p = user()
    bind("user", "u1", "superuser")
    assert can(p, "acme", "acme.api") == set()
