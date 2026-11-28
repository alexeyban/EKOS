"""Open a compiled EKOS ledger, search it, and show one object with its evidence.

    pip install ekos
    python quickstart.py <workspace> [search words]

<workspace> is a directory that `ekos commit` has compiled (it holds `.ekos/`).
"""

from __future__ import annotations

import sys

import ekos


def main(workspace: str, words: str) -> None:
    kb = ekos.open(workspace)  # read-only: nothing here can change the ledger

    status = kb.status()
    print(f"{status.objects} objects, {status.relationships} relationships")

    hits = kb.search(words, limit=5)
    if not hits:
        print(f"nothing matches {words!r}")
        return
    print(f"\nsearch {words!r}:")
    for hit in hits:
        print(f"  {hit.name}  ({hit.id})")

    state = kb.get(hits[0].id)
    assert state is not None
    print(f"\n{state.object.name} is a {state.object.kind}")
    for fact in kb.facts(state.object.id)[:6]:
        print(f"  {fact.attribute} = {fact.value!r}")

    # Every conclusion in a ledger carries the place it was read from.
    print("\nevidence:")
    for evidence in state.evidence:
        where = evidence.path if evidence.line is None else f"{evidence.path}:{evidence.line}"
        print(f"  {where}  {evidence.fragment[:70]!r}")
    if not state.evidence:
        print("  none recorded")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1], " ".join(sys.argv[2:]) or "orders")
