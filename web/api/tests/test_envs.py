"""RFC 0183 §2: which environment variables a tenant's `ekos` processes receive."""

from __future__ import annotations

import re
import sys
import time
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import envs, models
from app.main import create_app
from app.settings import Settings


# Windows cannot run this test's assumption (see the reason); the job on Windows is informational.
def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


ENVIRON = {
    "PATH": "/usr/bin",
    "HOME": "/home/ekos",
    "LANG": "C.UTF-8",
    "LC_ALL": "C.UTF-8",
    "HTTPS_PROXY": "http://proxy:3128",
    "OLLAMA_HOST": "http://host.docker.internal:11434",
    "EKOS_HOME": "/var/lib/ekos",
    "ACME_ANTHROPIC_KEY": "sk-acme",
    "GLOBEX_OPENAI_KEY": "sk-globex",
    "UNNAMED_SECRET": "x",
    "EKOS_CONSOLE_SESSION_SECRET": "session",
    "EKOS_CONSOLE_OIDC_CLIENT_SECRET": "oidc",
    "EKOS_READ_SCOPE": "/tmp/scope.json",
}
BASE_KEYS = {"PATH", "HOME", "LANG", "LC_ALL", "HTTPS_PROXY", "OLLAMA_HOST", "EKOS_HOME"}


@pytest.fixture(autouse=True)
def db(tmp_path: Path) -> None:
    models.init_engine(str(tmp_path / "c.db"))
    models.add(
        models.Tenant(id="acme", name="A", model_env=["ACME_ANTHROPIC_KEY"], allow_metered=True)
    )
    models.add(
        models.Tenant(id="globex", name="G", model_env=["GLOBEX_OPENAI_KEY"], allow_metered=False)
    )


def settings(**kw) -> Settings:
    base = {"console_db": "x", "console_token": "t", "session_secret": "s"}
    if kw.get("production"):
        base |= {"EKOS_CONSOLE_OIDC_ISSUER": "https://idp.test", "owners": "root@x.io"}
    return Settings(**(base | kw))


def env_of(tenant_id: str | None, **kw) -> dict[str, str]:
    tenant = models.get_tenant(tenant_id) if tenant_id else None
    return envs.for_tenant(tenant, settings(**kw), dict(ENVIRON))


def test_a_tenant_gets_the_base_and_its_own_named_variables() -> None:
    assert set(env_of("acme")) == BASE_KEYS | {"ACME_ANTHROPIC_KEY"}
    assert env_of("acme")["ACME_ANTHROPIC_KEY"] == "sk-acme"


def test_with_metered_calls_off_a_tenant_gets_no_credential() -> None:
    env = env_of("globex")
    assert set(env) == BASE_KEYS
    # A local model server is still reachable: it needs no key.
    assert env["OLLAMA_HOST"].startswith("http://")


def test_nobody_gets_the_consoles_own_secrets_or_an_unnamed_variable() -> None:
    for tenant in ("acme", "globex", "default", None):
        for production in (False, True):
            env = env_of(tenant, production=production)
            assert not [k for k in env if k.startswith("EKOS_CONSOLE_")], (tenant, production)
    for tenant in ("acme", "globex", None):
        env = env_of(tenant)
        assert "UNNAMED_SECRET" not in env and "EKOS_READ_SCOPE" not in env


def test_the_default_tenant_keeps_its_environment_outside_production() -> None:
    env = env_of("default")
    assert env["UNNAMED_SECRET"] == "x"
    # But not what was named for another tenant.
    assert "ACME_ANTHROPIC_KEY" not in env and "GLOBEX_OPENAI_KEY" not in env


def test_in_production_the_default_tenant_follows_the_rule_too() -> None:
    assert set(env_of("default", production=True)) == BASE_KEYS
    models.update_tenant("default", model_env=["UNNAMED_SECRET"], allow_metered=True)
    assert set(env_of("default", production=True)) == BASE_KEYS | {"UNNAMED_SECRET"}


def test_an_unknown_tenant_gets_the_base_only() -> None:
    assert set(env_of(None)) == BASE_KEYS


def test_a_named_variable_the_console_does_not_have_is_simply_absent() -> None:
    models.update_tenant("acme", model_env=["ACME_ANTHROPIC_KEY", "NOT_SET_ANYWHERE"])
    assert "NOT_SET_ANYWHERE" not in env_of("acme")


def test_a_tenant_cannot_be_given_a_console_variable_even_by_a_stored_row() -> None:
    models.update_tenant("acme", model_env=["EKOS_CONSOLE_SESSION_SECRET", "bad name"])
    assert set(env_of("acme")) == BASE_KEYS


@pytest.mark.parametrize("name", ["ANTHROPIC_API_KEY", "openai_key", "_X", "A1"])
def test_valid_names(name: str) -> None:
    assert envs.is_valid_name(name)


@pytest.mark.parametrize(
    "name", ["", "1X", "A-B", "A B", "sk-123", "EKOS_CONSOLE_OWNERS", "ekos_console_x", "X" * 200]
)
def test_invalid_names(name: str) -> None:
    assert not envs.is_valid_name(name)


def test_every_process_the_console_starts_is_given_an_environment() -> None:
    """A spawn without `env=` inherits everything. Each call in the app passes one."""
    app = Path(__file__).resolve().parents[1] / "app"
    missing = []
    for path in sorted(app.rglob("*.py")):
        if path.name == "_proc.py":
            continue
        text = path.read_text()
        for match in re.finditer(r"_proc\.(spawn|run_streaming)\(", text):
            depth, i = 0, match.end() - 1
            while True:
                depth += {"(": 1, ")": -1}.get(text[i], 0)
                if depth == 0:
                    break
                i += 1
            if "env=" not in text[match.start() : i]:
                line = text.count("\n", 0, match.start()) + 1
                missing.append(f"{path.relative_to(app)}:{line}")
    assert missing == [], f"spawned without an environment: {missing}"
    # And the helper itself reaches the OS with what it was given.
    proc_text = (app / "_proc.py").read_text()
    assert proc_text.count("env=env") == 2


# ── through the API and a real process ───────────────────────────────────────

AUTH = {"Authorization": "Bearer w"}


@pytest.fixture
def client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[TestClient]:
    # Stands in for `ekos`: writes the environment it was started with into its directory.
    fake = tmp_path / "fake-ekos"
    # Only inside a workspace: the MCP supervisor starts it with no directory of its own.
    fake.write_text('#!/bin/sh\n[ -f "$PWD/ekos.toml" ] && env > "$PWD/env.dump"\nexit 0\n')
    fake.chmod(0o755)
    monkeypatch.setenv("EKOS_BIN", str(fake))
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "session-secret-value")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "console.db"))
    monkeypatch.setenv("EKOS_CONSOLE_RUNS_DIR", str(tmp_path / "runs"))
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_JSON", "[]")
    monkeypatch.setenv("EKOS_CONSOLE_MCP_IDLE_SECONDS", "0")
    monkeypatch.delenv("EKOS_HOME", raising=False)
    monkeypatch.delenv("EKOS_CONSOLE_WORKSPACES_ROOT", raising=False)
    monkeypatch.setenv("ACME_MODEL_KEY", "sk-acme-value")
    monkeypatch.setenv("GLOBEX_MODEL_KEY", "sk-globex-value")
    with TestClient(create_app()) as c:
        yield c


def _workspace(client: TestClient, root: Path, tenant: str) -> Path:
    path = root / tenant / "ws"
    (path / ".ekos").mkdir(parents=True)
    (path / "ekos.toml").write_text("[observe]\npaths = []\n")
    r = client.post(
        "/api/tenants",
        headers=AUTH,
        json={"id": tenant, "name": tenant, "source_roots": [str(root / tenant)]},
    )
    assert r.status_code == 201, r.text
    r = client.post(
        "/api/workspaces",
        headers=AUTH,
        json={"id": "ws", "name": "WS", "path": str(path), "tenant": tenant},
    )
    assert r.status_code == 201, r.text
    return path


def _run_and_dump(client: TestClient, workspace_id: str, path: Path) -> dict[str, str]:
    (path / "env.dump").unlink(missing_ok=True)
    run = client.post(f"/api/workspaces/{workspace_id}/commands/build", headers=AUTH, json={})
    assert run.status_code == 200, run.text
    deadline = time.time() + 20
    while time.time() < deadline:
        status = client.get(f"/api/runs/{run.json()['run_id']}", headers=AUTH).json()["status"]
        if status not in ("queued", "running"):
            break
        time.sleep(0.1)
    lines = (path / "env.dump").read_text().splitlines()
    return dict(line.split("=", 1) for line in lines if "=" in line)


@_not_on_windows("the fake ekos is a shell script")
def test_a_tenants_pipeline_sees_its_own_key_and_not_another_tenants(
    client: TestClient, tmp_path: Path
) -> None:
    acme = _workspace(client, tmp_path, "acme")
    globex = _workspace(client, tmp_path, "globex")

    # Nothing named yet: neither sees either key, nor the console's secrets.
    env = _run_and_dump(client, "acme.ws", acme)
    assert "ACME_MODEL_KEY" not in env and "GLOBEX_MODEL_KEY" not in env
    assert not [k for k in env if k.startswith("EKOS_CONSOLE_")]
    assert "session-secret-value" not in "\n".join(env.values())
    assert "PATH" in env

    r = client.patch(
        "/api/tenants/acme",
        headers=AUTH,
        json={"model_env": ["ACME_MODEL_KEY"], "allow_metered": True},
    )
    assert r.status_code == 200, r.text
    assert r.json()["model_env"] == [{"name": "ACME_MODEL_KEY", "set": True}]
    assert "sk-acme-value" not in r.text
    client.patch("/api/tenants/globex", headers=AUTH, json={"model_env": ["GLOBEX_MODEL_KEY"]})

    env = _run_and_dump(client, "acme.ws", acme)
    assert env.get("ACME_MODEL_KEY") == "sk-acme-value" and "GLOBEX_MODEL_KEY" not in env
    # globex named its key and has metered calls off.
    env = _run_and_dump(client, "globex.ws", globex)
    assert "GLOBEX_MODEL_KEY" not in env and "ACME_MODEL_KEY" not in env

    # Turning the switch off withholds the key again, on the next run.
    client.patch("/api/tenants/acme", headers=AUTH, json={"allow_metered": False})
    assert "ACME_MODEL_KEY" not in _run_and_dump(client, "acme.ws", acme)


def test_names_are_validated_and_only_an_owner_sets_them(
    client: TestClient, tmp_path: Path
) -> None:
    _workspace(client, tmp_path, "acme")
    for bad in (["sk-ant-123"], ["EKOS_CONSOLE_SESSION_SECRET"], ["A B"]):
        r = client.patch("/api/tenants/acme", headers=AUTH, json={"model_env": bad})
        assert r.status_code == 400, bad
    assert client.patch(
        "/api/tenants/acme", headers={"Authorization": "Bearer r"}, json={"allow_metered": True}
    ).status_code in (403, 404)
    shown = client.get("/api/tenants", headers=AUTH).json()
    acme = next(t for t in shown if t["id"] == "acme")
    assert acme["allow_metered"] is False and acme["model_env"] == []
    r = client.patch(
        "/api/tenants/acme", headers=AUTH, json={"model_env": ["NOT_IN_THE_ENVIRONMENT"]}
    )
    assert r.json()["model_env"] == [{"name": "NOT_IN_THE_ENVIRONMENT", "set": False}]


def test_a_windows_process_gets_what_it_needs_to_start() -> None:
    """RFC 0184 §4: without SYSTEMROOT a child process on Windows fails before `main`."""
    environ = {
        "SYSTEMROOT": r"C:\Windows",
        "PATHEXT": ".EXE;.BAT",
        "USERPROFILE": r"C:\Users\a",
        "LOCALAPPDATA": r"C:\Users\a\AppData\Local",
        "OPENAI_API_KEY": "not-for-you",
    }
    env = envs.for_tenant(None, environ=environ)
    assert env == {k: v for k, v in environ.items() if k != "OPENAI_API_KEY"}


def test_a_source_credential_is_granted_with_metered_calls_off(memory_keyring) -> None:
    """RFC 0223: a database password is not a metered call."""
    from app import keychain

    models.update_tenant("globex", source_env=["EKOS_DB_GLOBEX_SHOP_ERP"])
    keychain.put_env("EKOS_DB_GLOBEX_SHOP_ERP", "pw")
    for production in (False, True):
        env = env_of("globex", production=production)
        assert env["EKOS_DB_GLOBEX_SHOP_ERP"] == "pw", production
        assert "GLOBEX_OPENAI_KEY" not in env
    # Another tenant never gets it.
    assert "EKOS_DB_GLOBEX_SHOP_ERP" not in env_of("acme")
    assert "EKOS_DB_GLOBEX_SHOP_ERP" not in env_of("default")
