"""The built UI is served with client-side routes falling back to the app (RFC 0179)."""

from __future__ import annotations

from pathlib import Path

import pytest
from fastapi import FastAPI
from fastapi.testclient import TestClient

from app.main import _SpaFiles


@pytest.fixture
def ui(tmp_path: Path) -> TestClient:
    (tmp_path / "assets").mkdir()
    (tmp_path / "index.html").write_text("<title>app</title>")
    (tmp_path / "assets" / "main.js").write_text("js")
    (tmp_path / "logo.png").write_text("png")
    app = FastAPI()
    app.mount("/", _SpaFiles(directory=tmp_path, html=True), name="ui")
    return TestClient(app)


@pytest.mark.parametrize(
    "path",
    [
        "/w/shop",
        "/w/shop/semantics",
        # A workspace of a tenant other than `default` is `<tenant>.<project>` (RFC 0180). The
        # dot is not a file extension. Found in a browser: a reload of this page was a 404.
        "/w/acme.shop",
        "/w/acme.shop/graph",
        "/w/acme.shop/evals/report-2026-10-13.json",
        "/runs/abc",
        "/admin",
    ],
)
def test_a_page_of_the_app_gets_the_app(ui: TestClient, path: str) -> None:
    r = ui.get(path)
    assert r.status_code == 200
    assert "<title>app</title>" in r.text


@pytest.mark.parametrize(
    "path", ["/assets/gone.js", "/gone.png", "/favicon.ico", "/api/nope", "/mcp/a/b"]
)
def test_a_missing_file_or_api_path_is_404(ui: TestClient, path: str) -> None:
    assert ui.get(path).status_code == 404


def test_files_that_exist_are_served(ui: TestClient) -> None:
    assert ui.get("/assets/main.js").text == "js"
    assert ui.get("/logo.png").text == "png"
