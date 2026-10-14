"""RFC 0184 §3: local mode — one person on their own machine, signed in by a one-time code
written into the home, loopback only."""

from __future__ import annotations

import os
import sys
import time
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import local_signin
from app.main import create_app
from app.settings import Settings


# Windows cannot run this test's assumption (see the reason); the job on Windows is informational.
def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


LOOPBACK = ("127.0.0.1", 50000)
BASE = "http://127.0.0.1:8000"


@pytest.fixture
def home(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, reset_settings: None) -> Path:
    root = tmp_path / "home"
    (root / "control").mkdir(parents=True)
    monkeypatch.setenv("EKOS_HOME", str(root))
    monkeypatch.setenv("EKOS_CONSOLE_MODE", "local")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    return root


@pytest.fixture
def client(home: Path) -> Iterator[TestClient]:
    with TestClient(create_app(), base_url=BASE, client=LOOPBACK) as c:
        yield c


def _control(home: Path) -> Path:
    return home / "control"


def test_a_code_signs_in_once(client: TestClient, home: Path) -> None:
    assert client.get("/api/auth/me").status_code == 401
    code = local_signin.issue(_control(home))
    r = client.get(f"/api/auth/local?code={code}", follow_redirects=False)
    assert r.status_code in (302, 303) and r.headers["location"] == "/"
    me = client.get("/api/auth/me").json()
    assert me["kind"] == "local" and me["owner"] is True and me["mode"] == "local"
    # The same code does not work twice, in a fresh browser.
    other = TestClient(client.app, base_url=BASE, client=LOOPBACK)  # its own cookies
    r = other.get(f"/api/auth/local?code={code}", follow_redirects=False)
    assert r.status_code == 401
    assert other.get("/api/auth/me").status_code == 401


def test_a_pasted_code_signs_in(client: TestClient, home: Path) -> None:
    code = local_signin.issue(_control(home))
    r = client.post("/api/auth/local", json={"code": code})
    assert r.status_code == 200
    assert client.get("/api/auth/me").json()["owner"] is True


def test_a_wrong_or_old_code_does_not(client: TestClient, home: Path) -> None:
    assert client.get("/api/auth/local?code=nope", follow_redirects=False).status_code == 401
    code = local_signin.issue(_control(home))
    for f in (_control(home) / "local-signin").iterdir():
        old = time.time() - 600
        os.utime(f, (old, old))
    assert client.get(f"/api/auth/local?code={code}", follow_redirects=False).status_code == 401
    # An expired code is also gone, not left to pile up.
    assert list((_control(home) / "local-signin").iterdir()) == []


def test_the_local_person_can_administer(client: TestClient, home: Path) -> None:
    client.post("/api/auth/local", json={"code": local_signin.issue(_control(home))})
    assert client.get("/api/platform/settings").status_code == 200
    assert client.post("/api/tenants", json={"id": "acme", "name": "Acme"}).status_code in (
        200,
        201,
    )


def test_static_tokens_are_refused(client: TestClient) -> None:
    assert client.get("/api/auth/me", headers={"Authorization": "Bearer w-tok"}).status_code == 401
    r = client.post("/api/auth/token-login", json={"token": "w-tok"})
    assert r.status_code == 400


def test_a_foreign_host_is_refused(client: TestClient, home: Path) -> None:
    client.post("/api/auth/local", json={"code": local_signin.issue(_control(home))})
    # DNS rebinding: the browser sends the attacker's name in Host.
    r = client.get("/api/auth/me", headers={"Host": "evil.example:8000"})
    assert r.status_code == 403
    for host in ("localhost:8000", "127.0.0.1", "[::1]:8000"):
        assert client.get("/api/auth/me", headers={"Host": host}).status_code == 200, host


def test_a_remote_client_is_refused(home: Path) -> None:
    with TestClient(create_app(), base_url=BASE, client=("10.0.0.5", 4000)) as c:
        assert c.get("/api/health").status_code == 403


def test_team_mode_is_unchanged(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, reset_settings: None
) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    with TestClient(create_app(), client=("10.0.0.5", 4000)) as c:
        r = c.get("/api/auth/me", headers={"Authorization": "Bearer w-tok"})
        assert r.status_code == 200 and r.json()["mode"] == "token"
        assert c.get("/api/auth/local?code=x").status_code == 400


@pytest.mark.parametrize(
    "extra",
    [
        {"EKOS_CONSOLE_OIDC_ISSUER": "https://id.example.com"},
        {"EKOS_CONSOLE_PRODUCTION": "true", "EKOS_CONSOLE_OWNERS": "a@b.c"},
    ],
)
def test_local_mode_refuses_a_provider_or_production(
    home: Path, monkeypatch: pytest.MonkeyPatch, extra: dict[str, str]
) -> None:
    for k, v in extra.items():
        monkeypatch.setenv(k, v)
    with pytest.raises(ValueError, match="local"):
        Settings()


# ── uninstall from the console (RFC 0188) ────────────────────────────────────


def _fake_ekos(tmp: Path, *, prints: str) -> tuple[Path, Path]:
    """An `ekos` that records its arguments and prints what `uninstall --ui --detach` prints."""
    args = tmp / "args"
    script = tmp / "fake-ekos"
    script.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > "{args}"\nprintf "{prints}\\n"\n')
    script.chmod(0o755)
    return script, args


def _signed_in(client: TestClient, home: Path) -> None:
    client.post("/api/auth/local", json={"code": local_signin.issue(_control(home))})


def test_uninstall_needs_an_install_by_setup(client: TestClient, home: Path) -> None:
    _signed_in(client, home)
    body = client.get("/api/platform/uninstall").json()
    assert body["available"] is False and "ekos setup" in body["reason"]
    assert client.post("/api/platform/uninstall").status_code == 409


@_not_on_windows("the fake ekos is a shell script")
def test_uninstall_starts_the_page_and_returns_its_link(
    home: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (home / "control" / "install.json").write_text("{}")
    link = "http://127.0.0.1:45678/?t=abc"
    script, args = _fake_ekos(tmp_path, prints=f"EKOS uninstall: {link}")
    monkeypatch.setenv("EKOS_BIN", str(script))
    with TestClient(create_app(), base_url=BASE, client=LOOPBACK) as client:
        assert client.post("/api/platform/uninstall").status_code == 401
        _signed_in(client, home)
        assert client.get("/api/platform/uninstall").json() == {"available": True, "reason": None}
        r = client.post("/api/platform/uninstall")
        assert r.status_code == 200, r.text
        assert r.json() == {"url": link}
    assert args.read_text().split("\n")[:6] == [
        "--home",
        str(home),
        "uninstall",
        "--ui",
        "--detach",
        "",
    ]


@_not_on_windows("the fake ekos is a shell script")
def test_uninstall_refuses_a_link_off_this_machine(
    home: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (home / "control" / "install.json").write_text("{}")
    script, _ = _fake_ekos(tmp_path, prints="EKOS uninstall: http://evil.example/?t=x")
    monkeypatch.setenv("EKOS_BIN", str(script))
    with TestClient(create_app(), base_url=BASE, client=LOOPBACK) as client:
        _signed_in(client, home)
        assert client.post("/api/platform/uninstall").status_code == 502


@_not_on_windows("the fake ekos is a shell script")
def test_uninstall_answers_while_the_page_holds_the_pipe(
    home: Path, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The page keeps running after `--detach` returns, and may have inherited the console's
    pipe: the link is read line by line, never to end of file (found by the package check)."""
    (home / "control" / "install.json").write_text("{}")
    script = tmp_path / "fake-ekos"
    script.write_text(
        "#!/bin/sh\nprintf 'EKOS uninstall: http://127.0.0.1:45678/?t=abc\\n'\nsleep 30 &\n"
    )
    script.chmod(0o755)
    monkeypatch.setenv("EKOS_BIN", str(script))
    with TestClient(create_app(), base_url=BASE, client=LOOPBACK) as client:
        _signed_in(client, home)
        started = time.monotonic()
        r = client.post("/api/platform/uninstall")
        assert r.status_code == 200, r.text
        assert time.monotonic() - started < 10
