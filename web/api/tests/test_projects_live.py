"""RFC 0179 §2 against the real binary: the console creates, runs, lists and removes a project
kept in the EKOS home, and never writes into its source directory. Skips without `EKOS_BIN`."""

from __future__ import annotations

import hashlib
import time
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

AUTH = {"Authorization": "Bearer secret-console"}
WAUTH = {"Authorization": "Bearer secret-write"}


def _tree(root: Path) -> dict[str, str]:
    return {
        str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(root.rglob("*"))
        if p.is_file()
    }


@pytest.fixture
def sources(tmp_path: Path) -> Path:
    root = tmp_path / "sources"
    (root / "shop" / "db").mkdir(parents=True)
    (root / "shop" / "db" / "schema.sql").write_text(
        "CREATE TABLE orders (id SERIAL PRIMARY KEY, total NUMERIC);\n"
    )
    return root


@pytest.fixture
def env(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    sources: Path,
    ekos_bin: str,
    reset_settings: None,
) -> Path:
    home = tmp_path / "home"
    monkeypatch.setenv("EKOS_HOME", str(home))
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_ROOT", str(sources))
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "secret-console")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "secret-write")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "test-secret")
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_JSON", "[]")
    monkeypatch.delenv("EKOS_CONSOLE_CONSOLE_DB", raising=False)
    monkeypatch.delenv("EKOS_CONSOLE_RUNS_DIR", raising=False)
    return home


@pytest.fixture
def client(env: Path) -> Iterator[TestClient]:
    with TestClient(create_app()) as c:
        yield c


def _create(client: TestClient, sources: Path, name: str = "shop"):
    return client.post(
        "/api/workspaces",
        headers=WAUTH,
        json={"id": name, "name": "Shop", "source": str(sources / "shop")},
    )


def _wait_ready(client: TestClient, workspace_id: str) -> dict:
    deadline = time.time() + 30
    row: dict = {}
    while time.time() < deadline:
        rows = client.get("/api/workspaces", headers=AUTH).json()
        row = next(r for r in rows if r["id"] == workspace_id)
        if row["server"] and row["server"]["state"] == "ready":
            return row
        time.sleep(0.2)
    raise AssertionError(f"MCP server never became ready: {row}")


def test_create_a_project_from_a_source_directory(
    client: TestClient, env: Path, sources: Path
) -> None:
    before = _tree(sources)

    r = _create(client, sources)
    assert r.status_code == 201, r.text
    project_dir = env / "data" / "default" / "shop"
    assert r.json()["path"] == str(project_dir)
    assert (project_dir / "ekos.toml").is_file()
    assert (project_dir / "state").is_dir()

    # The console's own state is in the home too (§3).
    assert (env / "control" / "console.db").is_file()

    # Read commands and the MCP server both reach the project.
    status = client.get("/api/workspaces/shop/stats", headers=AUTH)
    assert status.status_code == 200, status.text
    assert status.json()["workspace"] == str(sources / "shop")
    _wait_ready(client, "shop")

    # The pipeline runs against it.
    run = client.post("/api/workspaces/shop/commands/build", headers=WAUTH, json={})
    assert run.status_code in (200, 201, 202), run.text
    run_id = run.json()["run_id"]
    deadline = time.time() + 120
    state = ""
    while time.time() < deadline:
        state = client.get(f"/api/runs/{run_id}", headers=AUTH).json()["status"]
        if state not in ("queued", "running"):
            break
        time.sleep(0.3)
    log = env / "control" / "runs" / f"{run_id}.log"
    assert state == "succeeded", log.read_text() if log.is_file() else "no log"
    assert any((project_dir / "state" / "artifacts").iterdir())
    assert log.is_file()

    # Nothing was written into the source directory.
    assert _tree(sources) == before
    assert not (sources / "shop" / ".ekos").exists()
    assert not (sources / "shop" / "ekos.toml").exists()


def test_refusals(client: TestClient, env: Path, sources: Path, tmp_path: Path) -> None:
    def post(body: dict, headers: dict = WAUTH):
        return client.post("/api/workspaces", headers=headers, json=body)

    src = str(sources / "shop")
    assert post({"id": "shop", "name": "S", "source": src}, AUTH).status_code == 403
    assert post({"id": "Shop", "name": "S", "source": src}).status_code == 400
    assert post({"id": "../x", "name": "S", "source": src}).status_code == 400
    assert post({"id": "shop", "name": "S", "source": str(sources / "nope")}).status_code == 400
    outside = tmp_path / "elsewhere"
    outside.mkdir()
    r = post({"id": "shop", "name": "S", "source": str(outside)})
    assert r.status_code == 400 and "outside" in r.json()["detail"]
    assert post({"id": "shop", "name": "S"}).status_code == 422
    assert post({"id": "shop", "name": "S", "source": src, "path": src}).status_code == 422
    assert not (env / "data").exists() or not any((env / "data").rglob("ekos.toml"))

    assert _create(client, sources).status_code == 201
    assert _create(client, sources).status_code == 409


def test_a_project_is_listed_again_after_a_restart_with_an_empty_registry(
    env: Path, sources: Path
) -> None:
    with TestClient(create_app()) as c:
        assert _create(c, sources).status_code == 201

    # The registry is lost; the project in the home is not.
    (env / "control" / "console.db").unlink()
    import app.settings as settings_mod

    settings_mod._settings = None
    with TestClient(create_app()) as c:
        rows = c.get("/api/workspaces", headers=AUTH).json()
        assert [(r["id"], r["path"]) for r in rows] == [
            ("shop", str(env / "data" / "default" / "shop"))
        ]


def test_removing_a_project_moves_it_to_the_trash(
    client: TestClient, env: Path, sources: Path
) -> None:
    assert _create(client, sources).status_code == 201
    assert client.delete("/api/workspaces/shop", headers=WAUTH).status_code == 204
    assert client.get("/api/workspaces", headers=AUTH).json() == []
    assert not (env / "data" / "default" / "shop").exists()
    trashed = list((env / "trash" / "default").iterdir())
    assert len(trashed) == 1 and trashed[0].name.startswith("shop-")
    assert (sources / "shop" / "db" / "schema.sql").is_file()


def test_a_config_save_pointing_outside_the_root_is_refused(
    client: TestClient, env: Path, sources: Path
) -> None:
    assert _create(client, sources).status_code == 201
    raw = client.get("/api/workspaces/shop/config", headers=AUTH).json()["raw"]
    assert str(sources / "shop") in raw

    moved = raw.replace(str(sources / "shop"), "/etc")
    r = client.put("/api/workspaces/shop/config", headers=WAUTH, json={"raw": moved})
    assert r.status_code == 422, r.text
    assert "outside" in str(r.json()["detail"])
    assert "/etc" not in (env / "data" / "default" / "shop" / "ekos.toml").read_text()

    # An unrelated edit still saves.
    ok = client.put("/api/workspaces/shop/config", headers=WAUTH, json={"raw": raw + "\n# note\n"})
    assert ok.status_code == 200, ok.text


# ── RFC 0181, end to end: console → MCP server → store filter ────────────────


def _pipeline(client: TestClient, workspace_id: str) -> None:
    run = client.post(f"/api/workspaces/{workspace_id}/commands/pipeline", headers=WAUTH, json={})
    assert run.status_code == 200, run.text
    run_id = run.json()["run_id"]
    deadline = time.time() + 180
    state = ""
    while time.time() < deadline:
        state = client.get(f"/api/runs/{run_id}", headers=AUTH).json()["status"]
        if state not in ("queued", "running"):
            break
        time.sleep(0.3)
    assert state == "succeeded", state


def test_a_restricted_token_does_not_get_what_its_rule_hides(
    client: TestClient, env: Path, sources: Path
) -> None:
    (sources / "shop" / "secrets").mkdir()
    (sources / "shop" / "secrets" / "payroll.sql").write_text(
        "CREATE TABLE payroll_zebra (id SERIAL PRIMARY KEY, salary NUMERIC);\n"
    )
    assert _create(client, sources).status_code == 201
    _pipeline(client, "shop")
    _wait_ready(client, "shop")

    def names(headers: dict, query: str) -> str:
        r = client.get(f"/api/workspaces/shop/search?q={query}", headers=headers)
        assert r.status_code == 200, r.text
        return r.text

    # The owner sees both tables.
    assert "payroll_zebra" in names(WAUTH, "payroll_zebra")
    assert "orders" in names(WAUTH, "orders")

    made = client.post(
        "/api/tenants/default/tokens",
        headers=WAUTH,
        json={"name": "partner", "role": "viewer", "workspace_id": "shop"},
    )
    assert made.status_code == 201, made.text
    bot = {"Authorization": f"Bearer {made.json()['token']}"}
    # Before the rule the token sees everything a viewer does.
    assert "payroll_zebra" in names(bot, "payroll_zebra")

    rule = client.post(
        "/api/tenants/default/bindings",
        headers=WAUTH,
        json={
            "token_id": made.json()["id"],
            "role": "viewer",
            "workspace_id": "shop",
            "effect": "deny",
            "path": "secrets/**",
        },
    )
    assert rule.status_code == 201, rule.text

    # The rule takes effect on the next request, through the real MCP server.
    hidden = names(bot, "payroll_zebra")
    assert "payroll" not in hidden and "secrets/" not in hidden, hidden
    assert "orders" in names(bot, "orders")

    graph = client.get("/api/workspaces/shop/graph", headers=bot)
    assert graph.status_code == 200, graph.text
    assert "payroll" not in graph.text and "orders" in graph.text
    assert "payroll" in client.get("/api/workspaces/shop/graph", headers=WAUTH).text

    kinds = client.get("/api/workspaces/shop/stats/kinds", headers=bot).json()
    all_kinds = client.get("/api/workspaces/shop/stats/kinds", headers=WAUTH).json()
    count = lambda rows: sum(r["count"] for r in rows)  # noqa: E731
    assert 0 < count(kinds) < count(all_kinds)

    # And the pages that show more than the ledger are closed to it.
    assert client.get("/api/workspaces/shop/stats", headers=bot).status_code == 403
    assert client.get("/api/workspaces/shop/config", headers=bot).status_code == 403
    # The owner's next call is unaffected by the token's scope.
    assert "payroll_zebra" in names(WAUTH, "payroll_zebra")
