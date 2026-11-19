"""RFC 0185 §2: under `ekos console`, stop and restart go through files in the control
directory; the console watches for them and shuts down cleanly. And the keychain command the
installer uses."""

from __future__ import annotations

import asyncio
import io
import json
from pathlib import Path

import pytest

from app import keychain, keychain_cli, lifecycle


async def _watch_once(control: Path, stopped: list[str]) -> None:
    task = asyncio.create_task(lifecycle.watch(control, lambda: stopped.append("x"), interval=0.01))
    await asyncio.sleep(0.1)
    task.cancel()


@pytest.mark.parametrize("marker", [lifecycle.STOP, lifecycle.RESTART])
async def test_a_marker_shuts_the_console_down(tmp_path: Path, marker: str) -> None:
    stopped: list[str] = []
    await _watch_once(tmp_path, stopped)
    assert stopped == []
    (tmp_path / marker).touch()
    await _watch_once(tmp_path, stopped)
    assert stopped == ["x"]
    # The marker is left for the launcher: it tells stop from restart.
    assert (tmp_path / marker).exists()


def test_put_and_list_through_the_command(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("sys.stdin", io.StringIO("value-from-stdin\n"))
    assert keychain_cli.main(["put", "env", "OPENAI_API_KEY"]) == 0
    assert keychain.get_env("OPENAI_API_KEY") == "value-from-stdin"
    monkeypatch.setattr("sys.stdin", io.StringIO("s3cret"))
    assert keychain_cli.main(["put", "console", "oidc_client_secret"]) == 0
    assert keychain.get_console("oidc_client_secret") == "s3cret"
    out = io.StringIO()
    monkeypatch.setattr("sys.stdout", out)
    assert keychain_cli.main(["status"]) == 0
    status = json.loads(out.getvalue())
    assert status["available"] is True and status["env"] == ["OPENAI_API_KEY"]
    assert "value-from-stdin" not in out.getvalue()


def test_the_command_refuses_what_the_console_refuses(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("sys.stdin", io.StringIO("x"))
    assert keychain_cli.main(["put", "env", "EKOS_CONSOLE_SESSION_SECRET"]) == 2
    monkeypatch.setattr("sys.stdin", io.StringIO("x"))
    assert keychain_cli.main(["put", "console", "owners"]) == 2  # not a secret field
    monkeypatch.setattr("sys.stdin", io.StringIO(""))
    assert keychain_cli.main(["put", "env", "OPENAI_API_KEY"]) == 2  # empty
    monkeypatch.setenv("EKOS_CONSOLE_KEYCHAIN", "off")
    monkeypatch.setattr("sys.stdin", io.StringIO("x"))
    assert keychain_cli.main(["put", "env", "OPENAI_API_KEY"]) == 3  # no keychain


# ── RFC 0261 §2.3: a stop never loses a run ─────────────────────────────────────


RUN = {
    "id": "r1",
    "workspace": "shop",
    "command": "build",
    "status": "running",
    "since": "2026-11-19T10:00:00",
}


async def _watch_with(control: Path, stopped: list[str], runs: list[dict]) -> None:
    task = asyncio.create_task(
        lifecycle.watch(
            control, lambda: stopped.append("x"), interval=0.01, active_runs=lambda: runs
        )
    )
    await asyncio.sleep(0.1)
    task.cancel()


async def test_a_stop_is_refused_while_a_run_is_active(tmp_path: Path) -> None:
    stopped: list[str] = []
    (tmp_path / lifecycle.STOP).touch()
    await _watch_with(tmp_path, stopped, [RUN])
    assert stopped == []
    # The request is answered and removed; the runs that hold the console are named.
    assert not (tmp_path / lifecycle.STOP).exists()
    assert json.loads((tmp_path / lifecycle.REFUSED).read_text())["active_runs"] == [RUN]
    assert json.loads((tmp_path / lifecycle.ACTIVE).read_text()) == [RUN]


async def test_a_forced_stop_and_an_idle_console_stop(tmp_path: Path) -> None:
    stopped: list[str] = []
    (tmp_path / lifecycle.STOP).write_text("force")
    await _watch_with(tmp_path, stopped, [RUN])
    assert stopped == ["x"]
    idle = tmp_path / "idle"
    idle.mkdir()
    (idle / lifecycle.REFUSED).write_text("{}")  # left from an earlier refusal
    (idle / lifecycle.STOP).touch()
    await _watch_with(idle, stopped, [])
    assert stopped == ["x", "x"]
    assert not (idle / lifecycle.REFUSED).exists()
    assert json.loads((idle / lifecycle.ACTIVE).read_text()) == []


async def test_a_restart_is_not_held_by_runs(tmp_path: Path) -> None:
    # Restart is not part of RFC 0261's refusal: it keeps its RFC 0185 behaviour.
    stopped: list[str] = []
    (tmp_path / lifecycle.RESTART).touch()
    await _watch_with(tmp_path, stopped, [RUN])
    assert stopped == ["x"]
