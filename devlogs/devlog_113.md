# Devlog 113 — RFC 0096: EKL `AS OF` + `COUNT`/`GROUP BY`, first of a six-RFC gap-closure plan

**Date:** 2026-08-26
**PRs:** RFC 0096
**Branch:** main (direct)

---

## Summary

`docs/GAP_ANALYSIS.md` (devlog_112-adjacent, written earlier this session) surfaced seven real,
long-restated gaps under "Runtime/Retrieval." The user asked to fix them. Given the size (streaming,
multi-turn conversation history, embedding search, read caching, async conversion, EKL grammar
extensions, and a search-ranking boost — none trivial, several genuinely large), the work was
planned as a six-RFC sequence (smallest/most-grounded-first) via three parallel codebase-exploration
passes, with full async `KnowledgeStore` conversion explicitly excluded — not an oversight in the
original gap list, but RFC 0005's own deliberate, reasoned v0 rejection, re-confirmed still correct
rather than blindly redone. This entry covers RFC A of that plan: EKL's `AS OF <timestamp>` and
`COUNT`/`GROUP BY` clauses, the first and smallest-scoped item, chosen first because it reused an
already-shipped primitive (RFC 0047's point-in-time reads) rather than needing new infrastructure.

---

## RFC 0096 — EKL `AS OF`/`COUNT`/`GROUP BY`

### Problem / motivation

EKL (RFC 0010) could enumerate objects/relationships, filter, order, and project — but had no way
to ask "what did this look like at time T" in bulk, and no way to count or group results, despite
`Runtime::reconstruct_state_at` already answering the single-id version of the first question since
RFC 0047.

### What was built

| Component | Change |
|---|---|
| `KnowledgeStore` trait | New `all_objects_at`/`all_relationships_at`, both backends + macro delegation |
| SQLite `Ledger` | Correlated-subquery bulk point-in-time read (latest-per-id at-or-before `at`) |
| `FactLedger` | `all_current_payloads` generalized to `all_payloads_at(cut: Option<TxId>)` |
| `Runtime` | `list_objects_at`/`list_relationships_at` wrappers |
| `crates/ekl` parser | `AS OF '<rfc3339>'`, `COUNT`, `GROUP BY <field>` clauses; 2 new validations |
| `crates/ekl` interpreter | `AS OF` swaps the read path; `aggregate_count` post-filter step |
| `crates/cli/src/commands/ekl.rs` | Fixed tabular-output column selection for aggregate rows |

### Implementation details worth remembering

- The bulk "at time" read on `FactLedger` needed no new storage or scan shape — `all_current_payloads`
  already did one sequential EAVT-runs-plus-memtable pass folding each entity's history to "now"
  (`fold_state(entity, &entries, None)`); threading a real `cut: Option<TxId>` through that same fold
  call is the entire generalization. The SQLite side needed a genuinely new query (a correlated
  subquery selecting, per id, the row `object_at`'s own `ORDER BY written_at DESC, rowid DESC LIMIT 1`
  tie-break would have picked one at a time) — the two backends' bulk-read costs are structurally
  different (one pass vs. one correlated subquery) but produce identical results, verified by running
  the same 3 tests against both.
- Adding the two new trait methods without adding matching *inherent* methods on `FactLedger` first
  produced a real, caught-immediately `unconditional_recursion` compiler warning: `delegate_store!`'s
  generated `<$ty>::all_objects_at(self, at)` call resolves to the *trait* method itself (infinite
  recursion) when no inherent method of the same name exists to take priority in Rust's method
  resolution — every other delegate arm in this macro works because Rust's inherent-methods-beat-
  trait-methods rule silently does the right thing once both exist, not because of anything explicit
  in the macro. Worth remembering for any future `KnowledgeStore` method addition: the inherent method
  must exist on *both* concrete types before (or in the same change as) the trait+macro entry, or the
  compiler will say so immediately via this exact warning — which then meant `cargo clippy -D
  warnings` would have hard-failed the gate had it been missed.
- Aggregate output (`COUNT`/`GROUP BY`) turned out not to need a new `EklResult` variant, a
  simplification found while implementing rather than planned upfront: a grouped count is already
  expressible as an ordinary `Row` (`{"<field>": key, "count": N}`), so `ORDER BY`/`LIMIT` — both
  already generic over any `Row` — kept working on aggregate output with zero new code, and the MCP
  `ekos_ekl` tool needed no changes at all (it JSON-serializes `result.rows` directly regardless of
  shape). The CLI's plain-text renderer was the one place this *did* need a real fix — its column list
  for `RETURN`-less queries defaulted to `["id","name","kind"]`, which don't exist on an aggregate row
  at all; found and fixed before it reached a user, not after.

### Decisions (alternatives considered, why this choice)

- **A real `Object`↔`Relationship` `JOIN` was deliberately not attempted**, even though it was in the
  same gap-list paragraph as `AS OF`/`COUNT`. The parser's own header comment already documents the
  grammar as "six flat clause types with no recursive expression precedence" — a join is the one
  extension that actually breaks that shape, needing a combined row schema the current per-entity
  `object_row`/`relationship_row` split can't produce without a real redesign. Left for its own RFC
  once real usage shows it's needed, matching this project's just-in-time RFC convention.
- **`AS OF` + `FROM` is rejected outright, not silently degraded.** `load_neighborhood`/`trace_impact`
  have no time-aware equivalents; running them against current-state data under a query that reads as
  historical would produce plausible-looking wrong answers — a new `EklError::AsOfWithFromUnsupported`
  variant makes the gap loud instead of silent, matching this project's established "explicit failure
  over silently-wrong data" convention (RFC 0107's `SEM002` classification, the redaction bugs from
  devlog_112, etc.).
- **Full async `KnowledgeStore` conversion excluded from this whole plan, not just this RFC.** RFC
  0005 evaluated and rejected it for v0 in writing (`tokio::spawn_blocking` overhead not justified for
  a short-lived CLI process); the exploration pass confirmed that reasoning still holds (33 files, 18
  trait methods, both backends, still 100% sync) and found no new concrete reason to revisit it.
  Redoing an already-reasoned rejection without a new driving need would be scope creep, not gap
  closure.

---

## Knowledge Captured

- **A macro-generated trait delegation (`delegate_store!`) silently produces infinite recursion, not
  a compile error, if a new trait method is added without its matching inherent method on both
  concrete types** — Rust's inherent-beats-trait method resolution is what makes every *existing*
  arm of this macro correct, invisibly; a new arm added carelessly degrades to self-recursion instead
  of a "method not found" error, and only surfaces via `unconditional_recursion`, a warning (which
  this gate promotes to a hard failure via `-D warnings`, but would be silent otherwise).
- **Not every "needs a new output shape" item in a plan actually does once you're implementing it.**
  The original exploration report predicted `EklResult` would need a second variant for aggregate
  output; reusing the existing flat `Vec<Row>` shape turned out to work cleanly and for free across
  `ORDER BY`/`LIMIT`/MCP serialization. Worth re-checking a plan's stated design against the real
  existing contract once implementation starts, rather than building the more complex thing the plan
  assumed was necessary.
- **A tabular CLI renderer that infers display columns from a query's *entity type* breaks silently
  for any result shape that isn't a plain entity row** (here: aggregate rows). This is a real, generic
  risk for any future EKL extension that changes what a row can look like — worth checking
  `ekl.rs`'s column-selection logic specifically whenever `EklResult`'s row shape gains a new case.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/lib.rs` | New `all_objects_at`/`all_relationships_at` on `KnowledgeStore` + SQLite `Ledger`; macro delegation; 3 new tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | `all_current_payloads` generalized to `all_payloads_at`; new `FactLedger::all_objects_at`/`all_relationships_at`; 3 new tests |
| `ekos/crates/runtime/src/lib.rs` | New `Runtime::list_objects_at`/`list_relationships_at` |
| `ekos/crates/ekl/src/parser.rs` | `AS OF`/`COUNT`/`GROUP BY` grammar + validation; 8 new tests + 6 fuzz seeds |
| `ekos/crates/ekl/src/interpreter.rs` | `AS OF` read-path swap; `aggregate_count`; new `EklError::AsOfWithFromUnsupported`; 7 new tests |
| `ekos/crates/ekl/Cargo.toml` | Added `chrono` dependency |
| `ekos/crates/cli/src/commands/ekl.rs` | Fixed tabular-output column selection for aggregate (`COUNT`) rows |
| `ekos/docs/rfcs/0096-ekl-as-of-count-group-by.md` | New RFC |
