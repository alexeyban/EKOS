"""Pydantic response models for the console API.

The stats models mirror the Rust wire formats (RFC 0127 R2, RFC 0129 R5/R6) loosely — the console
passes the compiled payload through rather than re-validating every field.
"""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel


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
    server: ServerStatus | None = None


class WorkspaceCreate(BaseModel):
    id: str
    name: str
    path: str


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


class GraphOut(BaseModel):
    """Pass-through of a RFC 0127 R1 `GraphExport`. Kept permissive on purpose."""

    schema_version: int
    level: str
    counts: dict[str, int]
    truncated: dict[str, Any]
    nodes: list[dict[str, Any]]
    edges: list[dict[str, Any]]
    kind_index: list[str]
    rel_kind_index: list[str]
