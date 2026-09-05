"""Eval report history + detail routes (RFC 0138 web console integration)."""

from __future__ import annotations

import json
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}


def _report(dataset: str, generated_at: str, *, status_pass: bool = True) -> dict:
    return {
        "dataset": dataset,
        "agent": "ollama (llama3:latest)",
        "runtime": "local",
        "generated_at": generated_at,
        "gates": {
            "min_answer_correctness": 0.85,
            "min_evidence_groundedness": 0.90,
            "min_completeness": 0.80,
            "min_recall_at_10": 0.80,
            "max_hallucination_rate": 0.05,
        },
        "metrics": {
            "scenarios": 3,
            "passed": 2,
            "failed": 1,
            "answer_correctness": 0.75,
            "evidence_groundedness": 1.0,
            "completeness": 0.6,
            "recall_at_10": None,
            "hallucination_rate": 0.0,
            "avg_tokens": 1221.0,
            "p95_latency_ms": 49400.0,
            "cache_hits": 0,
            "cache_misses": 3,
            "tokens_saved": None,
            "peak_rss_kb": 71372,
            "total_cpu_time_ms": 14700.0,
            "status_pass": status_pass,
        },
        "scenarios": [
            {"id": "s1", "passed": True, "hallucinated": False},
            {"id": "s2", "passed": True, "hallucinated": False},
            {"id": "s3", "passed": False, "hallucinated": False},
        ],
    }


@pytest.fixture
def client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("[observe]\n")
    (ws / ".ekos").mkdir()
    with TestClient(create_app()) as c:
        c.post("/api/workspaces", headers=W, json={"id": "w", "name": "W", "path": str(ws)})
        yield c


def _write_report(ws_path: Path, filename: str, report: dict) -> None:
    reports = ws_path / "evals" / "reports"
    reports.mkdir(parents=True, exist_ok=True)
    (reports / filename).write_text(json.dumps(report))


def test_list_reports_is_empty_before_any_run(client: TestClient) -> None:
    r = client.get("/api/workspaces/w/evals/reports", headers=R)
    assert r.status_code == 200
    assert r.json() == []


def test_list_reports_returns_summaries_oldest_first(client: TestClient, tmp_path: Path) -> None:
    ws_path = tmp_path / "ws"
    _write_report(ws_path, "b-newer.json", _report("code", "2026-09-05T15:00:00Z"))
    _write_report(ws_path, "a-older.json", _report("architecture", "2026-09-05T14:00:00Z"))

    r = client.get("/api/workspaces/w/evals/reports", headers=R)
    assert r.status_code == 200
    rows = r.json()
    assert [row["dataset"] for row in rows] == ["architecture", "code"]
    # Summary carries the new RFC 0138 Phase 2 metrics, not just the original five.
    assert rows[0]["cache_misses"] == 3
    assert rows[0]["peak_rss_kb"] == 71372


def test_list_reports_skips_a_corrupt_file_rather_than_500ing(
    client: TestClient, tmp_path: Path
) -> None:
    ws_path = tmp_path / "ws"
    _write_report(ws_path, "good.json", _report("security", "2026-09-05T15:00:00Z"))
    (ws_path / "evals" / "reports" / "partial.json").write_text('{"dataset": "broken", "metr')

    r = client.get("/api/workspaces/w/evals/reports", headers=R)
    assert r.status_code == 200
    assert [row["dataset"] for row in r.json()] == ["security"]


def test_get_report_returns_full_detail(client: TestClient, tmp_path: Path) -> None:
    ws_path = tmp_path / "ws"
    report = _report("adversarial", "2026-09-05T15:00:00Z", status_pass=False)
    _write_report(ws_path, "r1.json", report)

    r = client.get("/api/workspaces/w/evals/reports/r1.json", headers=R)
    assert r.status_code == 200
    body = r.json()
    assert body["dataset"] == "adversarial"
    assert len(body["scenarios"]) == 3
    assert body["metrics"]["status_pass"] is False


def test_get_report_404_for_missing_file(client: TestClient) -> None:
    r = client.get("/api/workspaces/w/evals/reports/nope.json", headers=R)
    assert r.status_code == 404


@pytest.mark.parametrize("evil", ["../../../etc/passwd", "..%2Fsecret.json", "sub/dir.json"])
def test_get_report_rejects_path_traversal(client: TestClient, evil: str) -> None:
    r = client.get(f"/api/workspaces/w/evals/reports/{evil}", headers=R)
    assert r.status_code in (400, 404)


def test_reports_require_only_read_role(client: TestClient, tmp_path: Path) -> None:
    ws_path = tmp_path / "ws"
    _write_report(ws_path, "r1.json", _report("code", "2026-09-05T15:00:00Z"))
    assert client.get("/api/workspaces/w/evals/reports", headers=R).status_code == 200
    assert client.get("/api/workspaces/w/evals/reports/r1.json", headers=R).status_code == 200


def test_eval_run_is_a_registered_write_command(client: TestClient) -> None:
    cat = {c["name"]: c for c in client.get("/api/commands", headers=R).json()}
    assert "eval-run" in cat
    assert cat["eval-run"]["is_write"] is True
    assert set(cat["eval-run"]["params"]) == {"dataset", "agent", "category", "limit"}
