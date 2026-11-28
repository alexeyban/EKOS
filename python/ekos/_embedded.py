"""``Knowledge`` over the native engine: a compiled ledger read in this process."""

from __future__ import annotations

import json
import os
import re
import warnings
from collections.abc import Sequence
from datetime import datetime, timezone
from typing import Any

from . import _native
from ._types import (
    Answer,
    Diff,
    Fact,
    Graph,
    Hit,
    ImpactHop,
    JsonDict,
    Neighbor,
    Object,
    ObjectState,
    Status,
)

Timestamp = str | datetime


def _rfc3339(when: Timestamp) -> str:
    """A time as the engine wants it. A bare date is midnight UTC; a naive datetime is UTC."""
    if isinstance(when, datetime):
        if when.tzinfo is None:
            when = when.replace(tzinfo=timezone.utc)
        return when.isoformat()
    if len(when) == 10:
        return f"{when}T00:00:00Z"
    return when


def _present(**arguments: Any) -> JsonDict:
    """The arguments that were given: ``None`` means "use the engine's default"."""
    return {key: value for key, value in arguments.items() if value is not None}


class Knowledge:
    """A read-only view of one compiled ledger.

    A handle is a snapshot taken when it was opened; :meth:`refresh` brings it up to date.
    It serves one call at a time (a second thread waits), and the interpreter lock is released
    while a call runs. Nothing here can change the ledger.
    """

    def __init__(self, native: _native.Ledger) -> None:
        self._native = native

    def _call(
        self, name: str, arguments: JsonDict, embedding: Sequence[float] | None = None
    ) -> Any:
        vector = None if embedding is None else [float(x) for x in embedding]
        return json.loads(self._native.call(name, json.dumps(arguments), vector))

    # -- the ledger itself -------------------------------------------------------------------

    def status(self) -> Status:
        """Counts, and where the store is."""
        return Status(**self._call("status", {}))

    def refresh(self) -> bool:
        """Fold in what another process committed since the open; ``True`` if anything changed.

        Reads never refresh on their own, so two reads in a row always agree.
        """
        return bool(self._native.refresh())

    # -- finding things ----------------------------------------------------------------------

    def search(
        self,
        query: str,
        *,
        mode: str = "lexical",
        limit: int = 20,
        embedding: Sequence[float] | None = None,
    ) -> list[Hit]:
        """Objects matching ``query``, best first.

        ``mode`` is ``"lexical"``, ``"vector"`` or ``"hybrid"``. The last two need ``embedding``,
        the query as a vector from the model the ledger's index was built with: this package
        makes no network call, so it does not compute one.
        """
        raw = self._call("search", {"query": query, "mode": mode, "limit": limit}, embedding)
        return [Hit(id=m["id"], name=m["name"]) for m in raw["matches"]]

    def ekl(self, query: str) -> list[JsonDict]:
        """Run an Enterprise Knowledge Language query; one dict per row."""
        rows: list[JsonDict] = self._call("ekl", {"query": query})["rows"]
        return rows

    def query(self, question: str) -> Answer:
        """A compiled answer to a question: claims with their evidence. No model is called."""
        return Answer.from_json(self._call("query", {"question": question}))

    def retrieve(self, question: str) -> JsonDict:
        """The same answer with its working: the query plan, the evidence set, what ran."""
        raw: JsonDict = self._call("retrieve", {"question": question})
        return raw

    # -- one object --------------------------------------------------------------------------

    def get(self, object_id: str) -> ObjectState | None:
        """The object, its relationships and its evidence; ``None`` when there is no such id."""
        try:
            return ObjectState.from_json(self._call("state", {"id": object_id}))
        except _native.NotFound:
            return None

    def facts(self, object_id: str) -> list[Fact]:
        """Everything compiled about one object, attribute by attribute."""
        raw = self._call("facts", {"id": object_id})
        return [Fact(attribute=f["attribute"], value=f["value"]) for f in raw["facts"]]

    def history(self, object_id: str) -> list[Object]:
        """Every version of one object, oldest first. The ledger never edits: a correction is a
        new version."""
        raw = self._call("history", {"id": object_id})
        return [Object.from_json(v) for v in raw["versions"]]

    def audit_trail(self, object_id: str) -> list[JsonDict]:
        """Every write to this id, oldest first: its run, stage and source artifact."""
        writes: list[JsonDict] = self._call("audit", {"id": object_id})["writes"]
        return writes

    # -- around an object --------------------------------------------------------------------

    def neighborhood(
        self, object_id: str, *, depth: int = 1, max_objects: int | None = None
    ) -> Graph:
        """The objects within ``depth`` relationships of one object (at most 3)."""
        arguments = _present(id=object_id, depth=depth, max_objects=max_objects)
        return Graph.from_json(self._call("neighborhood", arguments))

    def dependents(self, object_id: str) -> list[Neighbor]:
        """What points at this object: one relationship away, confirmed relationships only."""
        raw = self._call("dependents", {"id": object_id})
        return [Neighbor.from_json(n) for n in raw["dependents"]]

    def dependencies(self, object_id: str) -> list[Neighbor]:
        """What this object points at: one relationship away, confirmed relationships only."""
        raw = self._call("dependents", {"id": object_id})
        return [Neighbor.from_json(n) for n in raw["dependencies"]]

    def impact(
        self,
        object_id: str,
        *,
        direction: str = "dependents",
        hops: int = 5,
        kinds: Sequence[str] | None = None,
    ) -> list[ImpactHop]:
        """Everything reachable from one object, hop by hop.

        ``direction="dependents"`` is what would be affected by a change to it;
        ``"dependencies"`` is what it relies on. ``kinds`` limits the relationship kinds followed.
        """
        arguments = _present(
            id=object_id,
            direction=direction,
            max_hops=hops,
            kinds=None if kinds is None else list(kinds),
        )
        return [ImpactHop(**hop) for hop in self._call("impact", arguments)["hops"]]

    # -- the whole ledger, and time ----------------------------------------------------------

    def graph(self, **options: Any) -> JsonDict:
        """The compiled graph in the compact export format (``ekos graph export``'s options:
        ``level``, ``kinds``, ``rel_kinds``, ``max_nodes``, ``as_of``, …)."""
        raw: JsonDict = self._call("graph_export", _present(**options))
        return raw

    def diff(self, since: Timestamp, until: Timestamp | None = None) -> Diff:
        """What was written between two times (``until`` defaults to now)."""
        arguments = _present(
            **{"from": _rfc3339(since), "to": None if until is None else _rfc3339(until)}
        )
        return Diff.from_json(self._call("diff", arguments))

    def at(self, when: Timestamp) -> KnowledgeAt:
        """The ledger as it was at ``when``: a view with the reads that can be answered for a
        past moment."""
        return KnowledgeAt(self, _rfc3339(when))


class KnowledgeAt:
    """A :class:`Knowledge` as of one moment. Only the reads the ledger can answer for the past
    are here; for anything else, run an EKL query with ``AS OF``."""

    def __init__(self, knowledge: Knowledge, when: str) -> None:
        self._knowledge = knowledge
        self.when = when

    def get(self, object_id: str) -> ObjectState | None:
        """The object as it was, or ``None`` if it did not exist yet."""
        try:
            raw = self._knowledge._call("state", {"id": object_id, "at": self.when})
        except _native.NotFound:
            return None
        return ObjectState.from_json(raw)

    def graph(self, **options: Any) -> JsonDict:
        """The compiled graph as it was."""
        return self._knowledge.graph(**{**options, "as_of": self.when})


def _project_dir(project: str, tenant: str | None, home: str | os.PathLike[str] | None) -> str:
    """Where a home project lives: ``<home>/data/<tenant>/<project>``.

    The home is ``home``, then ``EKOS_HOME``, then ``~/.ekos`` — the ``ekos`` command's order.
    """
    for what, name in (("project", project), ("tenant", tenant or "default")):
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name):
            raise _native.QueryError(f"{name!r} is not a {what} name")
    root = home or os.environ.get("EKOS_HOME") or os.path.join("~", ".ekos")
    return os.path.join(os.fspath(root), "data", tenant or "default", project)


def open(  # noqa: A001 - the package's entry point
    path: str | os.PathLike[str] | None = None,
    *,
    project: str | None = None,
    tenant: str | None = None,
    home: str | os.PathLike[str] | None = None,
    read_scope: JsonDict | None = None,
) -> Knowledge:
    """Open a compiled ledger read-only: a workspace directory, or a project in the EKOS home.

        ekos.open("~/work/shop")                  # a workspace (it has `.ekos/`)
        ekos.open(project="shop")                 # a home project, as `ekos --project shop`
        ekos.open(project="erp", tenant="acme")

    Never creates anything: a place with no compiled ledger raises :class:`LedgerError`.
    ``read_scope`` is a read scope (the ``--read-scope`` format): every read is then filtered
    by it, and the reads that cannot be filtered raise :class:`NotSupported`. If the ledger's
    search index was written by another EKOS version, a ``UserWarning`` says so.
    """
    if (path is None) == (project is None):
        raise _native.QueryError("give a path or project=, one of the two")
    if project is not None:
        path = _project_dir(project, tenant, home)
    elif tenant is not None or home is not None:
        raise _native.QueryError("tenant= and home= select a home project: add project=")
    assert path is not None
    resolved = os.path.abspath(os.path.expanduser(os.fspath(path)))
    scope = None if read_scope is None else json.dumps(read_scope)
    native = _native.open(resolved, scope)
    for message in native.warnings():
        warnings.warn(message, UserWarning, stacklevel=2)
    return Knowledge(native)
