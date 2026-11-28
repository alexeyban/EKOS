"""EKOS — an embedded knowledge engine for Python (RFC 0269, provisional API).

``ekos.open(path)`` opens a compiled, evidence-backed EKOS knowledge ledger read-only, in this
process: no server and no Rust toolchain. Knowledge enters a ledger by compiling sources with
the ``ekos`` command; this package reads it and cannot change it.
"""

from __future__ import annotations

from . import _native
from ._embedded import Knowledge, KnowledgeAt, open  # noqa: A004 - `ekos.open` is the API
from ._native import EkosError, LedgerError, NotFound, NotSupported, QueryError
from ._types import (
    Answer,
    Change,
    Claim,
    ClaimEvidence,
    Diff,
    Evidence,
    Fact,
    Graph,
    Hit,
    ImpactHop,
    Neighbor,
    Object,
    ObjectState,
    Relationship,
    Status,
)

__all__ = [
    "Answer",
    "Change",
    "Claim",
    "ClaimEvidence",
    "Diff",
    "EkosError",
    "Evidence",
    "Fact",
    "Graph",
    "Hit",
    "ImpactHop",
    "Knowledge",
    "KnowledgeAt",
    "LedgerError",
    "Neighbor",
    "NotFound",
    "NotSupported",
    "Object",
    "ObjectState",
    "QueryError",
    "Relationship",
    "Status",
    "open",
]

__version__: str = _native.engine_version()
