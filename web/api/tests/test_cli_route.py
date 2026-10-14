"""RFC 0186 §1: any command of the catalogue from the web, as a validated argv — never a shell,
never an argument the catalogue does not list, never a console-managed one."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import catalogue, models
from app.main import create_app

OWNER = {"Authorization": "Bearer w-tok"}
READER = {"Authorization": "Bearer r-tok"}


def _arg(id_, long=None, index=None, kind="value", **kw):
    return {
        "id": id_,
        "long": long,
        "index": index,
        "kind": kind,
        "multiple": kw.get("multiple", False),
        "required": kw.get("required", False),
        "possible": kw.get("possible", []),
        "default": [],
        "value_name": None,
        "help": "",
    }


CATALOGUE = [
    {
        "path": ["query", "find"],
        "about": "find",
        "class": "read",
        "metered": False,
        "args": [
            _arg("text", index=1, required=True),
            _arg("mode", "mode", possible=["lexical", "vector", "hybrid"]),
            _arg("limit", "limit"),
            _arg("json", "json", kind="flag"),
        ],
    },
    {
        "path": ["semantics", "confirm"],
        "about": "confirm",
        "class": "review",
        "metered": False,
        "args": [
            _arg("targets", index=1, required=True, multiple=True),
            _arg("by", "as"),
            _arg("note", "note"),
        ],
    },
    {
        "path": ["import", "linkml"],
        "about": "import",
        "class": "review",
        "metered": False,
        "args": [_arg("file", index=1, kind="path", required=True), _arg("by", "as")],
    },
    {"path": ["clean"], "about": "clean", "class": "admin", "metered": False, "args": []},
    {
        "path": ["build"],
        "about": "build",
        "class": "write",
        "metered": False,
        "args": [_arg("source", "source")],
    },
    {
        "path": ["commit"],
        "about": "commit",
        "class": "write",
        "metered": True,
        "args": [_arg("yes", "yes", kind="flag")],
    },
    {"path": ["mcp", "serve"], "about": "serve", "class": "local", "metered": False, "args": []},
]


@pytest.fixture
def client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    monkeypatch.setattr(catalogue, "_cached", {"commands": CATALOGUE, "version": "test"})
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("")
    (ws / "model.yaml").write_text("x: 1\n")
    submitted: list[dict] = []
    with TestClient(create_app()) as c:
        models.add_workspace(models.Workspace(id="w", name="w", path=str(ws)))

        async def fake_submit(workspace_id, ws_path, command, params, **_):
            command.render_argv(params)
            submitted.append({"command": command.name, **params})
            return "run-1"

        monkeypatch.setattr(c.app.state.runner, "submit", fake_submit)
        c.submitted = submitted  # type: ignore[attr-defined]
        c.ws = ws  # type: ignore[attr-defined]
        yield c


def _run(c: TestClient, path: list[str], args: dict | None = None, who=OWNER, **extra):
    return c.post(
        "/api/workspaces/w/cli", headers=who, json={"command": path, "args": args or {}, **extra}
    )


def test_the_catalogue_for_a_workspace_hides_local_commands(client: TestClient) -> None:
    body = client.get("/api/workspaces/w/cli", headers=READER).json()
    paths = [" ".join(e["path"]) for e in body["commands"]]
    assert "mcp serve" not in paths and "query find" in paths
    allowed = {" ".join(e["path"]): e["allowed"] for e in body["commands"]}
    assert allowed["query find"] is True and allowed["build"] is False  # a reader


def test_a_read_command_becomes_an_argv(client: TestClient) -> None:
    r = _run(client, ["query", "find"], {"text": "customers", "mode": "hybrid", "json": True})
    assert r.status_code == 200, r.text
    argv = client.submitted[-1]["argv"]  # type: ignore[attr-defined]
    # Options first, as --long=value; positionals after `--`, so none can pose as an option.
    assert argv == ["query", "find", "--mode=hybrid", "--json", "--", "customers"]
    assert client.submitted[-1]["label"] == "query find"  # type: ignore[attr-defined]


def test_a_positional_that_looks_like_an_option_stays_a_value(client: TestClient) -> None:
    _run(client, ["query", "find"], {"text": "--home=/etc"})
    assert client.submitted[-1]["argv"][-2:] == ["--", "--home=/etc"]  # type: ignore[attr-defined]


@pytest.mark.parametrize(
    ("args", "says"),
    [
        ({"text": "x", "home": "/etc"}, "home"),  # console-managed / not listed
        ({"text": "x", "nope": "1"}, "nope"),
        ({}, "text"),  # required
        ({"text": "x", "mode": "sql"}, "mode"),  # not a possible value
        ({"text": "x", "json": "yes"}, "json"),  # a flag takes true/false
        ({"text": ["a", "b"]}, "text"),  # not multiple
    ],
)
def test_bad_arguments_are_refused(client: TestClient, args: dict, says: str) -> None:
    r = _run(client, ["query", "find"], args)
    assert r.status_code == 422 and says in r.text, r.text
    assert client.submitted == []  # type: ignore[attr-defined]


def test_unknown_and_local_commands_are_refused(client: TestClient) -> None:
    assert _run(client, ["rm", "-rf"]).status_code == 404
    r = _run(client, ["mcp", "serve"])
    assert r.status_code == 403 and "not run from the web" in r.text


def test_rights_follow_the_class(client: TestClient) -> None:
    assert _run(client, ["build"], who=READER).status_code == 403
    assert _run(client, ["build"]).status_code == 200


def test_a_review_names_the_person_not_the_form(client: TestClient) -> None:
    r = _run(client, ["semantics", "confirm"], {"targets": ["a", "b"], "by": "someone-else"})
    assert r.status_code == 200, r.text
    argv = client.submitted[-1]["argv"]  # type: ignore[attr-defined]
    assert "--as=someone-else" not in argv
    assert "--as=static:write" in argv
    assert argv[-3:] == ["--", "a", "b"]


def test_a_token_cannot_review(client: TestClient, monkeypatch: pytest.MonkeyPatch) -> None:
    from app import auth

    t, h, p = auth.new_token()
    models.add(
        models.ApiToken(
            id="t1", tenant_id="default", name="bot", token_hash=h, prefix=p, created_by="x"
        )
    )
    models.add(
        models.RoleBinding(
            id="b1", tenant_id="default", principal_kind="token", principal_id="t1", role="operator"
        )
    )
    r = _run(
        client, ["semantics", "confirm"], {"targets": ["a"]}, who={"Authorization": f"Bearer {t}"}
    )
    assert r.status_code == 403


def test_admin_commands_need_the_typed_name(client: TestClient) -> None:
    r = _run(client, ["clean"])
    assert r.status_code == 409 and "type" in r.text
    assert _run(client, ["clean"], confirm="build").status_code == 409
    assert _run(client, ["clean"], confirm="clean").status_code == 200


def test_a_path_must_stay_inside_the_project(client: TestClient) -> None:
    r = _run(client, ["import", "linkml"], {"file": "/etc/passwd"})
    assert r.status_code == 422 and "outside" in r.text
    r = _run(client, ["import", "linkml"], {"file": "../../etc/passwd"})
    assert r.status_code == 422
    r = _run(client, ["import", "linkml"], {"file": "model.yaml"})
    assert r.status_code == 200, r.text
    file_arg = client.submitted[-1]["argv"][-1]  # type: ignore[attr-defined]
    assert file_arg == str(client.ws / "model.yaml")  # type: ignore[attr-defined]


def test_the_named_command_route_cannot_reach_it(client: TestClient) -> None:
    r = client.post("/api/workspaces/w/commands/cli", headers=OWNER, json={"argv": ["clean"]})
    assert r.status_code == 404


def test_the_real_catalogue_runs_a_real_command(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> None:
    """The binary's own catalogue, and a command outside the old allowlist run to the end."""
    import time

    import app.catalogue as cat_mod

    monkeypatch.setattr(cat_mod, "_cached", None)
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    ws = tmp_path / "ws"
    ws.mkdir()
    import subprocess

    subprocess.run([ekos_bin, "init"], cwd=ws, check=True, capture_output=True)
    with TestClient(create_app()) as c:
        models.add_workspace(models.Workspace(id="w", name="w", path=str(ws)))
        listed = c.get("/api/workspaces/w/cli", headers=OWNER).json()["commands"]
        paths = {" ".join(e["path"]) for e in listed}
        assert {"config validate", "semantics confirm", "coverage"} <= paths
        assert "setup" not in paths and "console start" not in paths
        r = _run(c, ["config", "validate"], {"json": True})
        assert r.status_code == 200, r.text
        run_id = r.json()["run_id"]
        for _ in range(100):
            run = c.get(f"/api/runs/{run_id}", headers=OWNER).json()
            if run["status"] not in ("queued", "running"):
                break
            time.sleep(0.1)
        assert run["status"] == "succeeded", run
        log = c.get(f"/api/runs/{run_id}/logs", headers=OWNER).text
        assert '"ok"' in log, log[:500]
