"""RFC 0136 §4 — server-side ForceAtlas2 layout. Pure function, no MCP/FastAPI involved."""

from __future__ import annotations

from app.layout import compute_layout


def test_every_node_gets_a_position() -> None:
    pos = compute_layout(["a", "b", "c", "d"], [("a", "b"), ("b", "c"), ("c", "a"), ("c", "d")])
    assert set(pos.keys()) == {"a", "b", "c", "d"}
    for x, y in pos.values():
        assert isinstance(x, float)
        assert isinstance(y, float)


def test_result_is_deterministic_regardless_of_input_order() -> None:
    a = compute_layout(["a", "b", "c", "d"], [("a", "b"), ("b", "c"), ("c", "a"), ("c", "d")])
    b = compute_layout(["d", "c", "b", "a"], [("c", "a"), ("a", "b"), ("c", "d"), ("b", "c")])
    assert a == b


def test_a_different_graph_produces_a_different_cache_entry() -> None:
    a = compute_layout(["a", "b"], [("a", "b")])
    b = compute_layout(["a", "b", "c"], [("a", "b"), ("b", "c")])
    assert set(a.keys()) != set(b.keys())


def test_no_edges_still_positions_every_isolated_node() -> None:
    pos = compute_layout(["a", "b", "c"], [])
    assert set(pos.keys()) == {"a", "b", "c"}


def test_empty_graph_returns_no_positions() -> None:
    assert compute_layout([], []) == {}


def test_single_node_is_placed_at_the_origin() -> None:
    assert compute_layout(["only"], []) == {"only": (0.0, 0.0)}


def test_an_edge_to_a_node_outside_the_requested_set_is_dropped_not_pulled_in() -> None:
    # "ghost" isn't in the node list (e.g. it was dropped by a min-degree filter upstream) --
    # it must not appear in the result just because an edge mentions it.
    pos = compute_layout(["a", "b"], [("a", "b"), ("b", "ghost")])
    assert set(pos.keys()) == {"a", "b"}


def test_a_self_loop_does_not_break_the_layout() -> None:
    pos = compute_layout(["a", "b"], [("a", "a"), ("a", "b")])
    assert set(pos.keys()) == {"a", "b"}
