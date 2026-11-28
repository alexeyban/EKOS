"""What would a change to one table, function or job reach?

    python impact_report.py <workspace> <name> [hops]

Prints everything that depends on <name>, hop by hop, with the relationship each step follows.
Use it before renaming or dropping something.
"""

from __future__ import annotations

import sys
from collections import defaultdict

import ekos


def find(kb: ekos.Knowledge, name: str) -> ekos.Hit | None:
    """The object called exactly `name`, or the best search match."""
    hits = kb.search(name, limit=10)
    exact = [hit for hit in hits if hit.name == name]
    candidates = exact or hits
    return candidates[0] if candidates else None


def main(workspace: str, name: str, hops: int) -> None:
    kb = ekos.open(workspace)
    target = find(kb, name)
    if target is None:
        sys.exit(f"nothing called {name!r} in this ledger")

    reached = kb.impact(target.id, direction="dependents", hops=hops)
    print(f"{target.name}: {len(reached)} dependent object(s) within {hops} hop(s)")

    by_hop: dict[int, list[ekos.ImpactHop]] = defaultdict(list)
    for step in reached:
        by_hop[step.hop].append(step)
    for hop in sorted(by_hop):
        print(f"\nhop {hop}")
        for step in sorted(by_hop[hop], key=lambda s: (s.kind, s.name)):
            print(f"  {step.kind:<12} {step.name}   via {step.via}")

    # The other direction: what the object itself relies on.
    needs = kb.dependencies(target.id)
    if needs:
        print(f"\n{target.name} itself depends on:")
        for neighbor in needs:
            print(f"  {neighbor.kind or '?':<12} {neighbor.name}   via {neighbor.relationship}")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 3)
