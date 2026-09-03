from __future__ import annotations

import os
import socket
import subprocess
import time
from collections.abc import Iterator
from pathlib import Path

import pytest


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture
def reset_settings() -> Iterator[None]:
    """Clear the module-level settings singleton so a test's monkeypatched env is picked up."""
    import app.settings as settings_mod

    settings_mod._settings = None
    yield
    settings_mod._settings = None


@pytest.fixture
def fake_workspace(tmp_path: Path) -> Path:
    """A directory that *looks* like a compiled workspace (has `ekos.toml` + `.ekos/`) without
    running the pipeline — enough for registry / path-validation tests."""
    (tmp_path / "ekos.toml").write_text("[observe]\npaths = []\n")
    (tmp_path / ".ekos").mkdir()
    return tmp_path


@pytest.fixture(scope="session")
def ekos_bin() -> str:
    """Path to a built `ekos` binary, from $EKOS_BIN. Tests that need a real MCP server skip
    when it is not set (CI builds it and exports the variable)."""
    path = os.environ.get("EKOS_BIN")
    if not path or not Path(path).is_file():
        pytest.skip("EKOS_BIN not set to a built `ekos` binary")
    return path


@pytest.fixture
def workspace(tmp_path: Path, ekos_bin: str) -> Path:
    subprocess.run([ekos_bin, "init"], cwd=tmp_path, check=True, capture_output=True)
    return tmp_path


@pytest.fixture
def mcp_server(workspace: Path, ekos_bin: str) -> Iterator[tuple[str, int, str]]:
    """A real `ekos mcp serve --tcp` with bearer-token auth. Yields (host, port, token)."""
    token = "test-token-42"
    tok_file = workspace / ".mcp-token"
    tok_file.write_text(token + "\n")
    port = _free_port()
    proc = subprocess.Popen(
        [
            ekos_bin,
            "mcp",
            "serve",
            "--workspace",
            str(workspace),
            "--tcp",
            f"127.0.0.1:{port}",
            "--tcp-token-file",
            str(tok_file),
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        deadline = time.time() + 15
        while time.time() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                    break
            except OSError:
                time.sleep(0.1)
        else:
            raise RuntimeError("ekos mcp serve --tcp did not come up")
        yield "127.0.0.1", port, token
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
