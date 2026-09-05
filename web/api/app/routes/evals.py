"""Eval report history + detail (RFC 0138 web console integration).

Read-only: `ekos eval run` already writes a timestamped JSON report to
`<workspace>/evals/reports/` on every run (triggering a new run itself goes through the existing
generic `eval-run` allowlisted command — `commands.py`/`routes/commands.py` — unrelated to this
file). This route only reads those files back, the same relationship `routes/config.py` has to
`ekos.toml`.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from fastapi import APIRouter, Depends, HTTPException

from .. import models
from ..auth import require_role
from ..deps import require_workspace

router = APIRouter(prefix="/workspaces/{workspace_id}/evals", tags=["evals"])


class ReportError(Exception):
    pass


def _reports_dir(ws: models.Workspace) -> Path:
    return Path(ws.path) / "evals" / "reports"


def _report_path(ws: models.Workspace, filename: str) -> Path:
    """Resolve to a real, canonical path and verify it still lands directly inside the
    workspace's `evals/reports/` directory (same SonarCloud pythonsecurity:S2083-hardening
    pattern as `config_io.config_path` — `resolve()` + a parent-directory check, not just
    rejecting `/`/`\\` in the input, so a symlink escape is caught too)."""
    reports_dir = _reports_dir(ws).resolve()
    path = (reports_dir / filename).resolve()
    if path.parent != reports_dir:
        raise ReportError(f"{filename!r} escapes {reports_dir}")
    return path


def _summary(filename: str, data: dict[str, Any]) -> dict[str, Any]:
    m = data.get("metrics", {})
    return {
        "file": filename,
        "dataset": data.get("dataset"),
        "agent": data.get("agent"),
        "runtime": data.get("runtime"),
        "generated_at": data.get("generated_at"),
        "status_pass": m.get("status_pass"),
        "scenarios": m.get("scenarios"),
        "passed": m.get("passed"),
        "failed": m.get("failed"),
        "answer_correctness": m.get("answer_correctness"),
        "evidence_groundedness": m.get("evidence_groundedness"),
        "completeness": m.get("completeness"),
        "recall_at_10": m.get("recall_at_10"),
        "hallucination_rate": m.get("hallucination_rate"),
        "avg_tokens": m.get("avg_tokens"),
        "p95_latency_ms": m.get("p95_latency_ms"),
        "cache_hits": m.get("cache_hits"),
        "cache_misses": m.get("cache_misses"),
        "tokens_saved": m.get("tokens_saved"),
        "peak_rss_kb": m.get("peak_rss_kb"),
        "total_cpu_time_ms": m.get("total_cpu_time_ms"),
    }


@router.get("/reports", dependencies=[Depends(require_role("read"))])
async def list_reports(ws: models.Workspace = Depends(require_workspace)) -> list[dict]:
    """Every saved report, oldest first (a trend line reads top-to-bottom) — the same ordering
    `ekos eval history` prints. A `.json` file that fails to parse is skipped, not a 500: one
    corrupt/partial report (e.g. from a run killed mid-write) shouldn't hide every other one."""
    reports_dir = _reports_dir(ws)
    if not reports_dir.is_dir():
        return []
    out: list[dict] = []
    for path in sorted(reports_dir.glob("*.json")):
        try:
            data = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        out.append(_summary(path.name, data))
    out.sort(key=lambda r: r.get("generated_at") or "")
    return out


@router.get("/reports/{filename}", dependencies=[Depends(require_role("read"))])
async def get_report(filename: str, ws: models.Workspace = Depends(require_workspace)) -> dict:
    """The full report — every headline metric plus the per-scenario breakdown — for one saved
    run, verbatim as `ekos eval run` wrote it."""
    try:
        path = _report_path(ws, filename)
    except ReportError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    if not path.is_file() or path.suffix != ".json":
        raise HTTPException(status_code=404, detail="no such report")
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise HTTPException(status_code=500, detail=f"corrupt report file: {exc}") from exc
