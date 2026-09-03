"""Pydantic response models for the console API (RFC 0128 §3.2).

`GraphOut` / `StatusOut` mirror the Rust wire formats from RFC 0127 R1 / R2 loosely — the console
does not re-validate every field, it passes the compiled payload through.
"""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel


class Health(BaseModel):
    status: str = "ok"
    service: str = "ekos-console-api"
    version: str


class WorkspaceOut(BaseModel):
    id: str
    name: str
    path: str


class StatusOut(BaseModel):
    """The `ekos_status` MCP tool result. Phase 1 swaps this for the richer `ekos status --json`
    (RFC 0127 R2) once the job runner can shell out."""

    entries: int
    objects: int
    relationships: int | None = None


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
