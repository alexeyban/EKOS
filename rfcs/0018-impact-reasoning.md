# RFC 0018 — Multi-hop Dependency & Impact Reasoning

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-24
**Gating:** none (additive; builds on RFC 0005 Runtime, RFC 0010 EKL, RFC 0013 MCP)

---

## Motivation

EKOS today can only answer "what's directly connected to X" — `ekos_dependents`
(`ekos/crates/cli/src/commands/mcp.rs:250-281`) is a single `relationships_for(id)`
call, one hop, no traversal. `ekos_neighborhood`/`Runtime::load_neighborhood`
(`ekos/crates/runtime/src/lib.rs:62-104`) is the only real multi-hop mechanism, but
it is **undirected** (it cannot distinguish "depends on" from "depended on by") and
**relationship-kind-blind** (it cannot filter to just `DependsOn` edges). EKL itself
(`docs/rfcs/0010-ekl.md:164-176`) explicitly deferred multi-hop path expressions to
"a follow-up RFC" that was never written; `todo_v2.md:537` names the exact gap:
*"no multi-hop path expressions (`EXPLAIN WHY TABLE A DEPENDS ON TABLE B` needs
graph-path reasoning EKL v0 doesn't have)"*.

This is the load-bearing gap behind "if I replace X with Y, what breaks?" — a
question that requires walking a *directed*, *typed* dependency chain, not a
single hop. This RFC closes it, working immediately over facts the ledger
already has today (SQL `ForeignKey` chains, git `CoupledWith` edges) — no new
compiler pass is required for this RFC to be useful.

## Design

### `Runtime::trace_impact`

New method on `Runtime` (`ekos/crates/runtime/src/lib.rs`), reusing
`load_neighborhood`'s proven cycle-safe BFS shape (`visited: HashSet<KirId>`,
`VecDeque` queue) with two differences: it is **directional** and
**relationship-kind-filterable**.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactDirection {
    /// What depends on this object — follow edges where `rel.to == current`.
    Dependents,
    /// What this object depends on — follow edges where `rel.from == current`.
    Dependencies,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactHop {
    pub hop: u32,
    pub object: KirObject,
    pub via: KirRelationship,
}

pub fn trace_impact(
    &self,
    id: &KirId,
    direction: ImpactDirection,
    kinds: &[RelationshipKind],
    max_hops: u32,
) -> Result<Vec<ImpactHop>, RuntimeError>
```

- `kinds` empty ⇒ no kind filter (all relationship kinds); non-empty ⇒ only
  expand through edges whose kind is in the list.
- `max_hops` bounds runaway traversal (the MCP tool defaults this to 5,
  matching the spirit of `ekos_search`'s existing 50-row cap).
- **Object-level dedup, same simplification as `load_neighborhood`**: a
  neighbour is recorded (and its edge kept) the first time it's reached: if
  a second, different relationship also reaches an already-visited object,
  that edge is not separately reported. This is a deliberate v1 scope
  choice, not an oversight — showing every parallel edge is a documented
  future refinement if a concrete use case needs it.
- The root itself is never included in the output (mirrors
  `ekos_dependents`'s existing "target is separate from dependents/
  dependencies" shape).

### `ekos_impact` MCP tool

New tool alongside `ekos_dependents` in
`ekos/crates/cli/src/commands/mcp.rs`:

```json
{
  "name": "ekos_impact",
  "description": "Transitive impact analysis: follows dependency edges multiple hops (default 5), directionally and optionally filtered to specific relationship kinds — 'what breaks N levels deep if I change this', not just direct edges.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "id": { "type": "string" },
      "direction": { "type": "string", "description": "\"dependents\" (default) or \"dependencies\"" },
      "kinds": { "type": "array", "items": { "type": "string" }, "description": "Relationship kind names to follow (default: all)" },
      "max_hops": { "type": "integer", "description": "Hop bound (default 5)" }
    },
    "required": ["id"]
  }
}
```

Response shape: a level-by-level tree — hops grouped by `hop`, each entry
carrying the object, the relationship kind that led to it, and (unlike
`ekos_dependents`) evidence is available via a follow-up `ekos_state` call
per node, keeping the response itself lean for a potentially wide/deep trace.

### `RelationshipKind: FromStr`

Both the MCP tool's `kinds` array and EKL's new `VIA` clause need to parse a
plain string into a `RelationshipKind`. Added once, shared:
`impl FromStr for RelationshipKind` in `ekos/crates/kir/src/lib.rs`, parsing
the same names `Display` renders (case-insensitive), falling back to
`Custom(s)` for anything unrecognized — infallible, matching the taxonomy's
own escape hatch.

### EKL `VIA <kind> DEPTH <n>`

The follow-up RFC 0010 flagged and never wrote. Grammar addition in
`ekos/crates/ekl/src/parser.rs`'s existing "FROM/RETURN/ORDER BY/LIMIT may
appear in any order" loop:

```
FIND Object VIA DependsOn FROM 'orders' DEPTH 3
```

- `VIA <ident>` — a relationship kind name, parsed via the new `FromStr`.
- `DEPTH <num>` — hop count; without `VIA`, `DEPTH` still generalizes the
  existing hardcoded `load_neighborhood(anchor, 1)` call to
  `load_neighborhood(anchor, depth.unwrap_or(1))` — the small, backward-compatible
  extension RFC 0010 itself predicted.
- With `VIA` present, the interpreter delegates to `trace_impact` instead of
  `load_neighborhood`, using **`ImpactDirection::Dependencies`** — this
  matches RFC 0010's own illustrating example (`orders -> customer_id ->
  customers`, tracing *outward* along a named kind) and keeps EKL's `FROM`
  semantics symmetric with today's (expand outward from the anchor).
  Tracing *dependents* transitively remains an MCP-tool-only capability
  (`ekos_impact` with `direction: "dependents"`) — EKL's anchor-expansion
  model doesn't have a natural "incoming" framing the way a single-target
  impact query does.
- Fully backward compatible: existing queries with neither clause behave
  exactly as before.

## Alternatives Considered

- **Generalize `load_neighborhood` itself to take a kind filter** — rejected;
  it's inherently undirected (used for "what's connected" exploration), and
  conflating direction into it would change its contract for every existing
  caller (EKL's no-`VIA` path, the `ekos_neighborhood` MCP tool, `ekos ask`'s
  pipeline). A new, additive method is safer and keeps `load_neighborhood`'s
  existing tests untouched.
- **Report every parallel edge to an already-visited node** — deferred; adds
  complexity with no concrete use case yet (documented as v1 scope, see
  Design above).
- **A total-node cap in addition to `max_hops`** — not added; `max_hops`
  alone is the same bounding lever `load_neighborhood` already relies on
  (depth, not a node count), and adding an undiscussed second cap is scope
  creep the RFC doesn't need yet.

## Testing

- Directed traversal doesn't leak the wrong direction (dependents vs
  dependencies on the same fixture return disjoint results).
- Kind filtering excludes non-matching edges (a `CoupledWith` edge is
  invisible when filtering to `[ForeignKey]`).
- Cycle safety at `max_hops` greater than the cycle length (mirrors
  `load_neighborhood_handles_cycles`).
- `max_hops` actually bounds output on a synthetic 5-node chain.
- EKL: `VIA ... DEPTH ...` parses; without `VIA`, `DEPTH` alone generalizes
  the existing anchor expansion; existing FROM-only queries are unaffected.
- Live verification: `ekos_impact` over the real estate's SQL fixture chain
  (`orders → order_items → …`), confirming a real multi-hop, evidence-
  traceable result with zero new compiler-pass work.

## Acceptance Criteria

- [ ] `Runtime::trace_impact` is directional, kind-filterable, cycle-safe,
      and bounded by `max_hops`.
- [ ] `ekos_impact` MCP tool registered and dispatched.
- [ ] `RelationshipKind: FromStr` shared by both the tool and EKL.
- [ ] EKL `VIA`/`DEPTH` grammar lands with zero regression to existing
      queries (full existing EKL test suite green).
