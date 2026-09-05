"""Server-side ForceAtlas2 graph layout (RFC 0136 §4).

Pure graph-structure input/output, no MCP or FastAPI dependency — cheap to unit-test directly.
`networkx` is the graph representation; `fa2_modified` (a maintained fork of the original `fa2`
package) implements the algorithm and returns real `{node_id: (x, y)}` positions, confirmed against
a live run before this module was written (RFC 0127 §9.4 named "graphology + ForceAtlas2", but
graphology is a JavaScript library — not usable from this Python backend, so the real Python
ecosystem was checked instead of assuming a name-only match).
"""

from __future__ import annotations

from functools import lru_cache

import networkx as nx
from fa2_modified import ForceAtlas2

# RFC 0127 §9.4's stated threshold — client-side force simulation holds up below this, so the
# frontend only calls the layout endpoint above it. Not enforced here (the endpoint serves any
# request); kept as the one source of truth the frontend imports the equivalent constant from.
SERVER_LAYOUT_THRESHOLD = 2000

_ITERATIONS = 100


def _compute(
    node_ids: tuple[str, ...], edges: tuple[tuple[str, str], ...]
) -> dict[str, tuple[float, float]]:
    """Deterministic given `(node_ids, edges)` (fa2_modified re-seeds identically per call), so
    `lru_cache` below is a correct memoization, not a possible-staleness shortcut: the moment the
    real graph structure changes (a recompile, a filter change), the cache key changes with it.
    """
    g = nx.Graph()
    g.add_nodes_from(node_ids)
    g.add_edges_from(e for e in edges if e[0] in g and e[1] in g)
    fa2 = ForceAtlas2(verbose=False)
    if g.number_of_nodes() == 0:
        return {}
    if g.number_of_nodes() == 1:
        return {node_ids[0]: (0.0, 0.0)}
    return fa2.forceatlas2_networkx_layout(g, pos=None, iterations=_ITERATIONS)


@lru_cache(maxsize=32)
def _compute_cached(
    node_ids: tuple[str, ...], edges: tuple[tuple[str, str], ...]
) -> dict[str, tuple[float, float]]:
    return _compute(node_ids, edges)


def compute_layout(
    nodes: list[str], edges: list[tuple[str, str]]
) -> dict[str, tuple[float, float]]:
    """Positions for every id in `nodes`, using only edges whose *both* endpoints are in `nodes`
    (an edge to a node outside the requested set — e.g. one already dropped by a degree filter —
    would otherwise silently pull that node into the graph with degree 1 and skew its position).
    Cache key is order-independent (sorted), so `["a","b"]` and `["b","a"]` share one cache entry.
    """
    node_key = tuple(sorted(set(nodes)))
    node_set = set(node_key)
    edge_key = tuple(
        sorted(
            {
                (a, b) if a <= b else (b, a)
                for a, b in edges
                if a != b and a in node_set and b in node_set
            }
        )
    )
    return _compute_cached(node_key, edge_key)
