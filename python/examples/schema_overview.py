"""List every table in a ledger with its columns and what references it.

    python schema_overview.py <workspace>

Uses EKL, the query language of the ledger, and the asyncio facade to read the tables
concurrently.
"""

from __future__ import annotations

import asyncio
import sys

import ekos.aio


async def describe(kb: ekos.aio.AsyncKnowledge, table_id: str) -> str:
    state, referenced_by = await asyncio.gather(kb.get(table_id), kb.dependents(table_id))
    assert state is not None
    columns = state.object.properties.get("columns") or []
    lines = [f"{state.object.name}  ({len(columns)} columns)"]
    for column in columns:
        key = "  primary key" if column.get("primary_key") else ""
        lines.append(f"    {column['name']:<24} {column.get('data_type', '')}{key}")
    for other in referenced_by:
        lines.append(f"    <- {other.name}  ({other.relationship})")
    return "\n".join(lines)


async def main(workspace: str) -> None:
    kb = await ekos.aio.open(workspace)
    tables = await kb.ekl("FIND Object WHERE kind = 'Table' ORDER BY name")
    print(f"{len(tables)} table(s)\n")
    # Separate awaits on one handle queue up; they do not block the event loop.
    for text in await asyncio.gather(*(describe(kb, row["id"]) for row in tables)):
        print(text, end="\n\n")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    asyncio.run(main(sys.argv[1]))
