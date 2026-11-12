"""What the data says about one table (RFC 0252): `ekos profile show --json <table>`.

A read (`read` role). The console never measures: it serves what the latest `ekos profile run`
wrote — counts only. Per coded column: rows per code value, first/last month, rows per month (a
month with fewer than the run's `min_cell` rows has no count), and what the code's history says
about each value (`code_history`); per table: its grain and the usage evidence the database's own
workload statistics give. No data value is in any of it except a literal that code names.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Query

from .. import models, readproc
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST

router = APIRouter(prefix="/workspaces/{workspace_id}/profile", tags=["profile"])

# The CLI's own words when there is nothing to show yet: not a failure of the console.
_NOT_MEASURED = ("no profile run yet", "nothing measured for")


@router.get(
    "", dependencies=[Depends(allow("read"))], responses={400: BAD_REQUEST, 502: BAD_GATEWAY}
)
async def profile(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    table: str = Query(min_length=1, max_length=257),
) -> dict:
    """One table's measured profile, or `{"measured": false, "reason": …}` before any run."""
    if not readproc._TABLE.match(table):
        raise HTTPException(status_code=400, detail="table must be a name, optionally schema.name")
    try:
        body = await readproc.read_json(
            settings.ekos_bin, ws.path, ["profile", "show", "--json", table], time_limit=60.0
        )
    except readproc.ReadProcError as exc:
        text = str(exc)
        if any(m in text for m in _NOT_MEASURED):
            return {"measured": False, "reason": text.rsplit(": ", 1)[-1][:300]}
        raise HTTPException(status_code=502, detail=text) from exc
    if not isinstance(body, dict):
        raise HTTPException(status_code=502, detail="unexpected output from `ekos profile show`")
    return {"measured": True, **body}
