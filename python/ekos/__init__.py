"""EKOS — an embedded knowledge engine for Python (RFC 0269, provisional).

``ekos.open(path)`` opens a compiled EKOS ledger read-only, in this process.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from typing import Any

from . import _native
from ._native import EkosError, LedgerError, NotSupported, QueryError

__all__ = [
    "EkosError",
    "Hit",
    "Knowledge",
    "LedgerError",
    "NotSupported",
    "QueryError",
    "Status",
    "open",
]

__version__: str = _native.engine_version()


@dataclass(frozen=True)
class Status:
    entries: int
    objects: int
    relationships: int
    ledger_path: str


@dataclass(frozen=True)
class Hit:
    id: str
    name: str
    kind: str | None
    score: float


class Knowledge:
    """A read-only view of one compiled ledger. One call at a time per handle."""

    def __init__(self, native: _native.Ledger) -> None:
        self._native = native

    def status(self) -> Status:
        return Status(**json.loads(self._native.status()))

    def refresh(self) -> bool:
        """Fold in what another process committed since the open; ``True`` if anything changed.

        A handle is a snapshot: reads never refresh on their own.
        """
        return bool(self._native.refresh())

    def search(self, query: str, *, mode: str = "lexical", limit: int = 20) -> list[Hit]:
        return [Hit(**row) for row in json.loads(self._native.search(query, mode, limit))]

    def get(self, object_id: str) -> dict[str, Any] | None:
        """The object, its relationships and its evidence; ``None`` when there is no such id."""
        state: dict[str, Any] | None = json.loads(self._native.get(object_id))
        return state


def open(path: str | os.PathLike[str]) -> Knowledge:  # noqa: A001 - the package's entry point
    """Open the ledger of the workspace or home project at ``path``, read-only."""
    resolved = os.path.abspath(os.path.expanduser(os.fspath(path)))
    return Knowledge(_native.open(resolved))
