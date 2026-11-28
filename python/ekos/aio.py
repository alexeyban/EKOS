"""The same reads for asyncio code.

Every call runs on a worker thread; the engine releases the interpreter lock while it works, so
the event loop is never blocked. One handle still serves one call at a time: concurrent awaits
on the same handle queue up, and separate handles are independent.

    kb = await ekos.aio.open("path/to/workspace")
    hits = await kb.search("payment status")
"""

from __future__ import annotations

import asyncio
import os
from collections.abc import Sequence
from typing import Any

from . import _embedded
from ._embedded import Timestamp
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


class AsyncKnowledge:
    """:class:`ekos.Knowledge` with awaitable reads. ``sync`` is the handle underneath."""

    def __init__(self, knowledge: _embedded.Knowledge) -> None:
        self.sync = knowledge

    async def status(self) -> Status:
        return await asyncio.to_thread(self.sync.status)

    async def refresh(self) -> bool:
        return await asyncio.to_thread(self.sync.refresh)

    async def search(
        self,
        query: str,
        *,
        mode: str = "lexical",
        limit: int = 20,
        embedding: Sequence[float] | None = None,
    ) -> list[Hit]:
        return await asyncio.to_thread(
            self.sync.search, query, mode=mode, limit=limit, embedding=embedding
        )

    async def ekl(self, query: str) -> list[JsonDict]:
        return await asyncio.to_thread(self.sync.ekl, query)

    async def query(self, question: str) -> Answer:
        return await asyncio.to_thread(self.sync.query, question)

    async def retrieve(self, question: str) -> JsonDict:
        return await asyncio.to_thread(self.sync.retrieve, question)

    async def get(self, object_id: str) -> ObjectState | None:
        return await asyncio.to_thread(self.sync.get, object_id)

    async def facts(self, object_id: str) -> list[Fact]:
        return await asyncio.to_thread(self.sync.facts, object_id)

    async def history(self, object_id: str) -> list[Object]:
        return await asyncio.to_thread(self.sync.history, object_id)

    async def audit_trail(self, object_id: str) -> list[JsonDict]:
        return await asyncio.to_thread(self.sync.audit_trail, object_id)

    async def neighborhood(
        self, object_id: str, *, depth: int = 1, max_objects: int | None = None
    ) -> Graph:
        return await asyncio.to_thread(
            self.sync.neighborhood, object_id, depth=depth, max_objects=max_objects
        )

    async def dependents(self, object_id: str) -> list[Neighbor]:
        return await asyncio.to_thread(self.sync.dependents, object_id)

    async def dependencies(self, object_id: str) -> list[Neighbor]:
        return await asyncio.to_thread(self.sync.dependencies, object_id)

    async def impact(
        self,
        object_id: str,
        *,
        direction: str = "dependents",
        hops: int = 5,
        kinds: Sequence[str] | None = None,
    ) -> list[ImpactHop]:
        return await asyncio.to_thread(
            self.sync.impact, object_id, direction=direction, hops=hops, kinds=kinds
        )

    async def graph(self, **options: Any) -> JsonDict:
        return await asyncio.to_thread(lambda: self.sync.graph(**options))

    async def diff(self, since: Timestamp, until: Timestamp | None = None) -> Diff:
        return await asyncio.to_thread(self.sync.diff, since, until)


async def open(  # noqa: A001 - mirrors ekos.open
    path: str | os.PathLike[str] | None = None,
    *,
    project: str | None = None,
    tenant: str | None = None,
    home: str | os.PathLike[str] | None = None,
    read_scope: JsonDict | None = None,
) -> AsyncKnowledge:
    """:func:`ekos.open` without blocking the event loop (an open reads the store's index)."""
    knowledge = await asyncio.to_thread(
        lambda: _embedded.open(
            path, project=project, tenant=tenant, home=home, read_scope=read_scope
        )
    )
    return AsyncKnowledge(knowledge)
