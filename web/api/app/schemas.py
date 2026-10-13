"""Pydantic response models for the console API.

The stats models mirror the Rust wire formats (RFC 0127 R2, RFC 0129 R5/R6) loosely — the console
passes the compiled payload through rather than re-validating every field.
"""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, model_validator


class Health(BaseModel):
    status: str = "ok"
    service: str = "ekos-console-api"
    version: str


class ServerStatus(BaseModel):
    state: str  # "starting" | "ready" | "failed"
    port: int
    retries: int
    detail: str = ""


class WorkspaceOut(BaseModel):
    id: str
    name: str
    path: str
    tenant_id: str = "default"
    # The caller's roles on this workspace (RFC 0180), so the UI can hide what they cannot do.
    roles: list[str] = []
    server: ServerStatus | None = None


class WorkspaceCreate(BaseModel):
    """Exactly one of `path` (register a directory that already holds a workspace or a project)
    and `source` (create a project in the EKOS home for the sources in that directory, RFC 0179)."""

    id: str
    name: str
    path: str | None = None
    source: str | None = None
    # RFC 0180. For a tenant other than `default` the workspace id becomes `<tenant>.<id>`.
    tenant: str = "default"

    @model_validator(mode="after")
    def _one_of_path_and_source(self) -> WorkspaceCreate:
        if (self.path is None) == (self.source is None):
            raise ValueError("give exactly one of `path` and `source`")
        return self


class StatusOut(BaseModel):
    """RFC 0127 R2 `ekos status --json`, passed through. Kept permissive."""

    schema_version: int
    workspace: str
    backend: str
    entries: int
    objects: int
    relationships: int
    evidence: int | None = None
    integrity: str
    last_write: str | None = None
    storage: dict[str, Any]


class DoctorCheck(BaseModel):
    name: str
    status: str
    detail: str


class DoctorOut(BaseModel):
    schema_version: int
    ok: bool
    checks: list[DoctorCheck]


class KindCount(BaseModel):
    kind: str
    count: int


class TimelinePoint(BaseModel):
    t: str
    objects: int
    relationships: int


class TimelineOut(BaseModel):
    schema_version: int
    bucket: str
    points: list[TimelinePoint]


class QueryStats(BaseModel):
    total: int
    by_tool: dict[str, int]
    cache_hit_rate: float
    p50_ms: float
    p95_ms: float


class LayoutIn(BaseModel):
    """RFC 0136 §4 — ids only. The console already has full node/edge objects from its own prior
    `/graph` call; there is no reason to round-trip them."""

    nodes: list[str]
    edges: list[tuple[str, str]]


class LayoutOut(BaseModel):
    positions: dict[str, tuple[float, float]]


class GraphOut(BaseModel):
    """Pass-through of a RFC 0127 R1 `GraphExport`. Kept permissive on purpose.

    Nodes/edges are raw dicts, so RFC 0134's per-element ``fs`` (first-seen) key rides through
    untouched; ``as_of`` is echoed at the top level when the export was time-sliced.
    """

    schema_version: int
    level: str
    as_of: str | None = None
    counts: dict[str, int]
    truncated: dict[str, Any]
    nodes: list[dict[str, Any]]
    edges: list[dict[str, Any]]
    kind_index: list[str]
    rel_kind_index: list[str]
