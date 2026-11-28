"""The embedded engine through the built wheel (RFC 0269).

The ledger tests need a compiled workspace with a `customers` table and an `orders` table that
references it: `EKOS_PY_TEST_WORKSPACE=<workspace dir>` (scripts/python-package-test.sh makes
one). Without it they are skipped; the tests that need no ledger always run.
"""

from __future__ import annotations

import asyncio
import hashlib
import inspect
import os
import re
from datetime import datetime, timezone
from pathlib import Path

import pytest

import ekos
import ekos.aio
from ekos import _embedded, _native

WORKSPACE = os.environ.get("EKOS_PY_TEST_WORKSPACE")
needs_ledger = pytest.mark.skipif(not WORKSPACE, reason="EKOS_PY_TEST_WORKSPACE is not set")
NOBODY = "00000000-0000-0000-0000-000000000000"


def tree_digest(root: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    count = 0
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        digest.update(str(path.relative_to(root)).encode())
        digest.update(path.read_bytes())
        count += 1
    return digest.hexdigest(), count


@pytest.fixture(scope="module")
def workspace() -> Path:
    assert WORKSPACE is not None
    return Path(WORKSPACE)


@pytest.fixture(scope="module")
def kb(workspace: Path) -> ekos.Knowledge:
    return ekos.open(workspace)


@pytest.fixture(scope="module")
def tables(kb: ekos.Knowledge) -> dict[str, str]:
    rows = kb.ekl("FIND Object WHERE kind = 'Table' ORDER BY name")
    return {row["name"]: row["id"] for row in rows}


# -- without a ledger ---------------------------------------------------------------------------


def test_a_directory_with_no_ledger_is_an_error_and_stays_empty(tmp_path: Path) -> None:
    with pytest.raises(ekos.LedgerError, match="no compiled ledger"):
        ekos.open(tmp_path)
    assert list(tmp_path.iterdir()) == []


def test_the_error_types_form_one_family() -> None:
    for error in (ekos.LedgerError, ekos.QueryError, ekos.NotSupported):
        assert issubclass(error, ekos.EkosError)
    assert issubclass(ekos.NotFound, ekos.QueryError)


def test_every_native_read_has_a_method() -> None:
    """A read added to the engine is not reachable from Python until `Knowledge` calls it."""
    called = set(re.findall(r'_call\(\s*"(\w+)"', inspect.getsource(_embedded)))
    assert called == set(_native.reads())


def test_a_bare_date_and_a_naive_datetime_are_utc() -> None:
    assert _embedded._rfc3339("2026-10-01") == "2026-10-01T00:00:00Z"
    assert _embedded._rfc3339("2026-10-01T12:00:00Z") == "2026-10-01T12:00:00Z"
    assert _embedded._rfc3339(datetime(2026, 10, 1)) == "2026-10-01T00:00:00+00:00"
    aware = datetime(2026, 10, 1, tzinfo=timezone.utc)
    assert _embedded._rfc3339(aware) == "2026-10-01T00:00:00+00:00"


def test_a_home_project_is_found_by_name(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    assert _embedded._project_dir("shop", None, tmp_path) == str(
        tmp_path / "data" / "default" / "shop"
    )
    monkeypatch.setenv("EKOS_HOME", str(tmp_path / "env"))
    assert _embedded._project_dir("erp", "acme", None) == str(
        tmp_path / "env" / "data" / "acme" / "erp"
    )
    with pytest.raises(ekos.LedgerError, match="not a directory"):
        ekos.open(project="shop", home=tmp_path)
    with pytest.raises(ekos.QueryError, match="name"):
        ekos.open(project="../etc")
    with pytest.raises(ekos.QueryError, match="name"):
        ekos.open(project="shop", tenant="a/b")
    with pytest.raises(ekos.QueryError, match="one of the two"):
        ekos.open()
    with pytest.raises(ekos.QueryError, match="one of the two"):
        ekos.open(tmp_path, project="shop")
    with pytest.raises(ekos.QueryError, match="add project="):
        ekos.open(tmp_path, tenant="acme")


# -- reads --------------------------------------------------------------------------------------


@needs_ledger
def test_status_and_search(kb: ekos.Knowledge, tables: dict[str, str]) -> None:
    status = kb.status()
    assert status.objects >= 2 and status.relationships >= 1
    assert status.to_dict()["ledger_path"] == status.ledger_path
    hits = kb.search("orders", limit=5)
    assert hits[0] == ekos.Hit(id=tables["orders"], name="orders")
    assert kb.refresh() is False


@needs_ledger
def test_an_object_comes_with_its_relationships_and_its_evidence(
    kb: ekos.Knowledge, tables: dict[str, str]
) -> None:
    state = kb.get(tables["orders"])
    assert state is not None
    assert (state.object.name, state.object.kind) == ("orders", "Table")
    assert [c["name"] for c in state.object.properties["columns"]] == [
        "id",
        "customer_id",
        "status",
    ]
    assert state.evidence, "a compiled table has evidence"
    evidence = state.evidence[0]
    assert evidence.path == "schema.sql" and "orders" in evidence.fragment
    assert evidence.id in state.object.evidence_ids
    (link,) = state.relationships
    assert (link.kind, link.from_id, link.to_id) == (
        "ForeignKey",
        tables["orders"],
        tables["customers"],
    )
    assert state.open_conflicts == []
    assert state.to_dict()["object"]["name"] == "orders"


@needs_ledger
def test_an_unknown_id_is_none_and_a_malformed_one_is_an_error(kb: ekos.Knowledge) -> None:
    assert kb.get(NOBODY) is None
    with pytest.raises(ekos.QueryError):
        kb.get("orders")
    with pytest.raises(ekos.NotFound):
        kb.facts(NOBODY)
    with pytest.raises(ekos.NotFound):
        kb.dependents(NOBODY)


@needs_ledger
def test_facts_history_and_audit_trail(kb: ekos.Knowledge, tables: dict[str, str]) -> None:
    facts = {f.attribute: f.value for f in kb.facts(tables["orders"])}
    assert facts["name"] == "orders" and facts["kind"] == "Table"
    assert facts["primary_key"] == ["id"]
    (version,) = kb.history(tables["orders"])
    assert version.name == "orders"
    assert kb.history(NOBODY) == []
    writes = kb.audit_trail(tables["orders"])
    assert writes and writes[0]["stage"] == "commit"


@needs_ledger
def test_the_graph_around_an_object(kb: ekos.Knowledge, tables: dict[str, str]) -> None:
    orders, customers = tables["orders"], tables["customers"]
    (dependent,) = kb.dependents(customers)
    assert (dependent.id, dependent.relationship) == (orders, "ForeignKey")
    assert kb.dependents(orders) == []
    assert [n.id for n in kb.dependencies(orders)] == [customers]

    (hop,) = kb.impact(customers)
    assert (hop.hop, hop.id, hop.via) == (1, orders, "ForeignKey")
    assert [h.id for h in kb.impact(orders, direction="dependencies", hops=2)] == [customers]
    assert kb.impact(customers, kinds=["DependsOn"]) == []

    graph = kb.neighborhood(orders, depth=1)
    assert {o.name for o in graph.objects} >= {"orders", "customers"}
    assert not graph.truncated
    assert len(kb.neighborhood(orders, depth=1, max_objects=1).objects) == 1
    with pytest.raises(ekos.QueryError, match="between"):
        kb.neighborhood(orders, depth=9)


@needs_ledger
def test_ekl_and_compiled_answers(kb: ekos.Knowledge, tables: dict[str, str]) -> None:
    assert [r["name"] for r in kb.ekl("FIND Object WHERE kind = 'Table' ORDER BY name")] == [
        "customers",
        "orders",
    ]
    with pytest.raises(ekos.QueryError, match="EKL"):
        kb.ekl("FIND")

    answer = kb.query("what depends on customers?")
    assert answer.claims
    for claim in answer.claims:
        assert isinstance(claim.evidence, list)
        assert claim.disputed is False
    about_orders = [c for c in answer.claims if c.entity == tables["orders"]]
    assert about_orders and about_orders[0].evidence[0].location.startswith("schema.sql")

    shown = kb.retrieve("orders")
    assert {"plan", "evidence", "understanding"} <= set(shown)


@needs_ledger
def test_time(kb: ekos.Knowledge, tables: dict[str, str]) -> None:
    everything = kb.diff("2000-01-01")
    assert everything.changed_total >= 3
    assert tables["orders"] in {c.id for c in everything.changed}
    link = next(c for c in everything.changed if c.entity == "Relationship")
    assert (link.from_id, link.to_id) == (tables["orders"], tables["customers"])
    assert kb.diff("2000-01-01", "2000-01-02").changed_total == 0

    assert kb.at("2000-01-01").get(tables["orders"]) is None
    now = datetime.now(timezone.utc)
    then = kb.at(now).get(tables["orders"])
    assert then is not None and then.object.name == "orders"
    assert kb.at("2000-01-01").graph()["nodes"] == []
    assert len(kb.graph(kinds=["Table"])["nodes"]) == 2


@needs_ledger
def test_vector_search_takes_the_query_vector_from_the_caller(kb: ekos.Knowledge) -> None:
    with pytest.raises(ekos.QueryError, match="vector"):
        kb.search("orders", mode="hybrid")
    with pytest.raises(ekos.QueryError, match="unknown mode"):
        kb.search("orders", mode="fuzzy")
    # This ledger has no vector index: the vector arm does not run, the lexical one answers.
    assert kb.search("orders", mode="hybrid", embedding=[0.0, 1.0])[0].name == "orders"


@needs_ledger
def test_a_read_scope_filters_every_read(workspace: Path, tables: dict[str, str]) -> None:
    nothing = ekos.open(workspace, read_scope={"default": "deny", "rules": []})
    assert nothing.search("orders") == []
    assert nothing.get(tables["orders"]) is None
    assert nothing.ekl("FIND Object") == []
    for refused in (nothing.status, lambda: nothing.audit_trail(tables["orders"])):
        with pytest.raises(ekos.NotSupported, match="restricted scope"):
            refused()
    with pytest.raises(ekos.QueryError, match="read scope"):
        ekos.open(workspace, read_scope={"rules": []})


@needs_ledger
def test_a_search_index_of_another_version_is_a_warning(workspace: Path, tmp_path: Path) -> None:
    import shutil

    copy = tmp_path / "workspace"
    shutil.copytree(workspace, copy)
    (version,) = copy.rglob("content_version")
    version.write_text("1")
    with pytest.warns(UserWarning, match="another EKOS version"):
        kb = ekos.open(copy)
    assert kb.search("orders")[0].name == "orders"
    assert version.read_text() == "1"


@needs_ledger
def test_asyncio_reads_match_the_blocking_ones(workspace: Path, tables: dict[str, str]) -> None:
    async def read() -> None:
        kb = await ekos.aio.open(workspace)
        orders, customers = tables["orders"], tables["customers"]
        status, hits, state = await asyncio.gather(
            kb.status(), kb.search("orders", limit=3), kb.get(orders)
        )
        assert status == kb.sync.status()
        assert hits == kb.sync.search("orders", limit=3)
        assert state == kb.sync.get(orders)
        assert await kb.facts(orders) == kb.sync.facts(orders)
        assert await kb.history(orders) == kb.sync.history(orders)
        assert await kb.audit_trail(orders) == kb.sync.audit_trail(orders)
        assert await kb.neighborhood(orders) == kb.sync.neighborhood(orders)
        assert await kb.dependents(customers) == kb.sync.dependents(customers)
        assert await kb.dependencies(orders) == kb.sync.dependencies(orders)
        assert await kb.impact(customers, hops=2) == kb.sync.impact(customers, hops=2)
        assert await kb.ekl("FIND Object ORDER BY name") == kb.sync.ekl("FIND Object ORDER BY name")
        assert (await kb.query("orders")).claims == kb.sync.query("orders").claims
        assert (await kb.retrieve("orders"))["plan"] == kb.sync.retrieve("orders")["plan"]
        assert (await kb.graph(kinds=["Table"]))["nodes"] == kb.sync.graph(kinds=["Table"])["nodes"]
        until = "2100-01-01"
        assert await kb.diff("2000-01-01", until) == kb.sync.diff("2000-01-01", until)
        assert await kb.refresh() is False

    asyncio.run(read())


@needs_ledger
def test_all_of_the_above_left_the_ledger_byte_identical(workspace: Path) -> None:
    """Runs last in this module: every read before it used the same directory."""
    before = tree_digest(workspace)
    kb = ekos.open(workspace)
    kb.status()
    kb.search("orders")
    kb.query("what depends on customers?")
    kb.graph()
    kb.diff("2000-01-01")
    kb.refresh()
    del kb
    assert tree_digest(workspace) == before
