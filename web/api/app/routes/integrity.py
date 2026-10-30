"""Cross-reference integrity: broken references, cycles, unused tables, reachability (RFC 0233).

A read (`read` role). The console never recomputes the check: it serves the integrity index the
last `ekos commit` wrote (`ekos check refs --from-index`), so a page load costs a file read, not a
pass over the ledger. The page filters; this route passes the report through, and with
`summary=true` drops the findings, for the dashboard tile.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Query

from .. import models, readproc
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY

router = APIRouter(prefix="/workspaces/{workspace_id}/integrity", tags=["integrity"])

ARGV = ["check", "refs", "--from-index", "--format", "json", "--reachability"]
_BIG = 64 << 20


@router.get("", dependencies=[Depends(allow("read"))], responses={502: BAD_GATEWAY})
async def integrity(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    summary: bool = Query(default=False),
) -> dict:
    """The last commit's integrity report: metrics, and (unless `summary`) every finding."""
    try:
        report = await readproc.read_json(
            settings.ekos_bin, ws.path, ARGV, max_output=_BIG, time_limit=60.0
        )
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    if not isinstance(report, dict):
        raise HTTPException(status_code=502, detail="unexpected output from `ekos check refs`")
    if summary:
        by_severity: dict[str, int] = {}
        for d in report.get("diagnostics", []):
            sev = str(d.get("severity", ""))
            by_severity[sev] = by_severity.get(sev, 0) + 1
        return {
            "computed_at": report.get("computed_at"),
            "metrics": report.get("metrics", {}),
            "by_severity": by_severity,
        }
    return report
