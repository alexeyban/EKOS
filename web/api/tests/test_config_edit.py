"""RFC 0186 §2: a project's `ekos.toml` edited field by field from a schema-driven form. The
server applies the changes to the text (comments and unknown keys kept) and returns it; saving
is the existing validated write."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import catalogue, config_io, models
from app.main import create_app

OWNER = {"Authorization": "Bearer w-tok"}
READER = {"Authorization": "Bearer r-tok"}

RAW = """# my project
[workspace]
root = "."  # here

[llm]
provider = "mock"
custom-thing = 1
"""


def test_changes_keep_comments_and_other_keys() -> None:
    out = config_io.apply_changes(
        RAW,
        [
            {"path": ["llm", "provider"], "value": "ollama"},
            {"path": ["llm", "model"], "value": "qwen2.5:1.5b"},
            {"path": ["recover", "sql", "default-dialect"], "value": "postgres"},
            {"path": ["observe", "paths"], "value": ["src", "db"]},
        ],
    )
    assert "# my project" in out and "# here" in out and "custom-thing = 1" in out
    doc = config_io.parse(out).unwrap()
    assert doc["llm"] == {"provider": "ollama", "custom-thing": 1, "model": "qwen2.5:1.5b"}
    assert doc["recover"]["sql"]["default-dialect"] == "postgres"
    assert doc["observe"]["paths"] == ["src", "db"]


def test_null_removes_a_key() -> None:
    out = config_io.apply_changes(RAW, [{"path": ["llm", "provider"], "value": None}])
    assert "provider" not in config_io.parse(out).unwrap()["llm"]
    # Removing what is not there is not an error.
    config_io.apply_changes(RAW, [{"path": ["nope", "x"], "value": None}])


@pytest.mark.parametrize(
    "change",
    [
        {"path": [], "value": 1},
        {"path": ["workspace"], "value": "replace a whole table"},
        {"path": ["workspace", "root", "deeper"], "value": 1},  # through a value
        {"path": ["a" * 200], "value": 1},
    ],
)
def test_changes_that_make_no_sense_are_refused(change: dict) -> None:
    with pytest.raises(config_io.ConfigError):
        config_io.apply_changes(RAW, [change])


@pytest.fixture
def client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text(RAW)
    with TestClient(create_app()) as c:
        models.add_workspace(models.Workspace(id="w", name="w", path=str(ws)))
        yield c


def test_the_config_carries_its_values(client: TestClient) -> None:
    body = client.get("/api/workspaces/w/config", headers=READER).json()
    assert body["values"]["llm"]["provider"] == "mock"


def test_edit_returns_text_and_writes_nothing(client: TestClient, tmp_path: Path) -> None:
    r = client.post(
        "/api/workspaces/w/config/edit",
        headers=READER,
        json={"raw": RAW, "changes": [{"path": ["llm", "provider"], "value": "ollama"}]},
    )
    assert r.status_code == 200, r.text
    assert 'provider = "ollama"' in r.json()["raw"]
    assert (tmp_path / "ws" / "ekos.toml").read_text() == RAW
    r = client.post(
        "/api/workspaces/w/config/edit",
        headers=READER,
        json={"raw": RAW, "changes": [{"path": [], "value": 1}]},
    )
    assert r.status_code == 422


def test_the_schema_comes_from_the_binary(
    client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(catalogue, "_schema", {"type": "object", "properties": {"llm": {}}})
    r = client.get("/api/config-schema", headers=READER)
    assert r.status_code == 200 and "llm" in r.json()["properties"]
    assert client.get("/api/config-schema").status_code == 401


def test_the_real_schema(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> None:
    monkeypatch.setattr(catalogue, "_schema", None)
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    with TestClient(create_app()) as c:
        schema = c.get("/api/config-schema", headers=READER).json()
    assert {"llm", "observe", "recover", "semantics"} <= set(schema["properties"])
    assert "LlmConfig" in schema["$defs"]
