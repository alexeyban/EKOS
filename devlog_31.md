# Devlog 31 — Fixing the SQL↔Pentaho identity gap found during real-world testing

**Date:** 2026-08-05
**PRs:** none yet — uncommitted, pending review
**Branch:** main (working tree)

---

## Summary

A prior session cloned two real GitHub Pentaho/SQL projects (`PIH/pih-pentaho`,
`joseph-higaki/etl_adventureworks_sales_purchases_datamart`) and ran them cold through the full
EKOS pipeline to build a presentation deck. While cross-checking `ekos_impact`/`ekos_dependents`
against a table it had already proven a Pentaho job writes to, it found `dependents_count: 0` —
no relationship existed between the SQL-recovered `Table` object and the Pentaho `Sink` node
that actually writes it, even though both carry the literal same name
(`fact_patient_coded_value`). This session root-caused and fixed it. Two bugs, not one: cross-system
identity resolution (RFC 0029) was silently skipping the single most confident case it could ever
see, and separately, `ekos_impact`/`ekos_dependents`/`ekos_neighborhood` had no concept of
"unconfirmed" at all — once `ekos identity scan` is actually run, they treated thousands of
unreviewed noise candidates exactly like observed facts.

---

## Bug #1 — exact cross-kind name matches were the one case guaranteed to never link

### Problem / motivation

`cross_system.rs::find_cross_system_candidates` skipped any pair whose names matched exactly
(case-insensitively), on the stated assumption that "same-name dedup is `DefaultResolver`'s job"
(RFC 0007). But `DefaultResolver::structural_score` returns `0.0` immediately when
`a.kind != b.kind` — it **only ever compares objects of the same `ObjectKind`**. A SQL `Table`
and a Pentaho `TransformNode` (`Custom("TransformNode")`) are different kinds by construction, so
an exact-name match between them fell into a gap neither resolver covered. Confirmed against
`pih-pentaho`: the `Sink` node in `transforms/load-fact-coded-values.ktr` has
`object_name: "fact_patient_coded_value"` — identical, character for character, to the `Table`
object's `name`. It was the most confident possible match, and the one case never proposed.

### What was built

Changed the skip condition in `find_cross_system_candidates` from "same name" to "same name **and
same kind**" — same-kind exact matches still defer to `DefaultResolver` as before; cross-kind
exact matches now flow through normal scoring, where `name_pattern` resolves to `1.0` and pushes
`confidence` to ~0.95–1.0 (still gated behind `ekos_identity_review`, never auto-confirmed — RFC
0029's non-negotiable).

### Implementation details worth remembering

- `ekos/crates/identity/src/cross_system.rs`: one `if` condition, plus a doc comment explaining
  *why* (so nobody "fixes" it back by re-adding a blanket name-equality skip).
- Two new tests: `exact_name_match_across_kinds_is_proposed_at_max_confidence` (the fix) and
  `exact_name_match_same_kind_is_still_skipped` (guards against regressing `DefaultResolver`'s
  territory).

### Decisions

- Did not auto-confirm exact matches, even at `confidence ~1.0`. RFC 0029 already settled this:
  an unconfirmed candidate must stay reviewable, no threshold-based shortcut — a wrong silent
  merge here would corrupt every downstream `ekos_impact`/`ekos_dependents` answer for that
  object, silently.

---

## Bug #2 — unconfirmed identity candidates were being traversed as if they were facts

### Problem / motivation

Running `ekos identity scan` against `pih-pentaho` (472 objects) produced **7,751 candidate
`SameAs` relationships** — most of them noise (star-schema tables share generic surrogate-key
column names like `patient_id`/`date_id`/`location_id`, which inflates `column_overlap_score` for
genuinely unrelated pairs). All were correctly written with `status: "unconfirmed"`. But
`Runtime::relationships_for` (backing `ekos_dependents`), `Runtime::load_neighborhood` (backing
`ekos_neighborhood` and EKL's `FROM` anchor), and `Runtime::trace_impact` (backing `ekos_impact`)
had no status filter at all — every unconfirmed hypothesis was walked exactly like a real
`ForeignKey`/`Custom("FeedsInto")` edge. Querying `ekos_dependents` on `fact_patient_coded_value`
after a scan returned **108 "dependents"**, none of which was the correct one (that one hadn't
been proposed yet — see Bug #1). This directly violates RFC 0029's own stated invariant: an
unconfirmed candidate must stay "structurally distinguishable from an observed fact, never
indistinguishable."

### What was built

| Component | Change |
|---|---|
| `ekos-kir` | `KirRelationship::is_pending_review()` — true for any `Custom("SameAs")` relationship whose `status` property isn't `"confirmed"` |
| `ekos-runtime` | `load_neighborhood` and `trace_impact` both skip edges where `is_pending_review()` is true |
| `ekos-cli` (`mcp.rs`) | `ekos_dependents` handler skips the same |

### Implementation details worth remembering

- Placed the predicate on `KirRelationship` itself (not duplicated per call site) so the three
  independent traversal paths (`ekos_dependents`, `ekos_impact`, `ekos_neighborhood`/EKL) can't
  drift out of sync on what "pending review" means.
- Rejected relationships (`status: "rejected"`) are also excluded — only `"confirmed"` passes.
  A relationship with no `status` property at all (every non-`SameAs` kind, i.e. every real
  compiler-derived fact) is unaffected; the check short-circuits on `kind` first.
- New tests: `trace_impact_excludes_unconfirmed_same_as_but_keeps_confirmed` and
  `load_neighborhood_excludes_unconfirmed_same_as_but_keeps_confirmed`, both asserting the
  unconfirmed case is dropped and the confirmed case still traverses.

### Decisions

- Did not add an explicit opt-in ("include unconfirmed candidates via a flag") to `ekos_impact`'s
  existing `kinds` filter parameter. No current caller needs to inspect unreviewed hypotheses
  through the impact/neighborhood path — `ekos identity scan`'s own output and
  `ekos_identity_review` are the intended surface for that. Revisit if a real use case appears.

---

## Verified end to end (not just unit tests)

Against the live `pih-pentaho` ledger (not a fixture):
1. Before the fix: `ekos_dependents(fact_patient_coded_value)` → `dependents_count: 0`.
2. Ran `ekos identity scan` again (append-only) with the fix in place → 53 new candidates written
   that were previously silently dropped, including the exact `Table`↔`Sink` match.
3. `ekos_dependents` on the `Table` still returned `0` immediately after — correctly excluded
   while unconfirmed.
4. Called `ekos_identity_review` with `decision: "confirmed"` on that one relationship.
5. `ekos_dependents` now returns exactly one dependent: `transforms/load-fact-coded-values.ktr:1`,
   `confidence: 0.95`, `status: "confirmed"` — the real Pentaho step that writes the table.

`cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` (clean
except one pre-existing, unrelated diff in `marketing/src/prompt.rs` from other in-progress work),
and `cargo test --workspace` all pass.

---

## Knowledge Captured

- **`DefaultResolver` (RFC 0007) is same-kind-only, unconditionally.** Any future cross-kind
  linking work (this bug, and likely others like it) cannot lean on it — `cross_system.rs` or a
  successor is the only place same-name-different-kind matching happens today.
- **Star-schema column-name overlap is a weak signal at scale.** Shared surrogate-key column
  names (`*_id`) across dozens of dimension/fact tables inflate `column_overlap_score` for
  unrelated pairs — this is *why* a real workspace produced 7,751 candidates from 472 objects.
  The noise itself wasn't fixed this session (deliberately out of scope — `MIN_CANDIDATE_CONFIDENCE`
  tuning is a judgment call, not a correctness bug); only the fact that noise was being treated as
  ground truth by traversal was.
- **Test with a real, uncontrolled dataset before trusting a heuristic.** Every existing
  `cross_system.rs` test used small, hand-built fixtures (2–3 objects) where the exact-match skip
  never mattered because no test constructed a `Table`/`TransformNode` pair with identical names.
  The bug was invisible in unit tests and only surfaced once a real 472-object ledger was queried
  end to end.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/identity/src/cross_system.rs` | Exact-name skip now scoped to same-kind pairs only; 2 new tests |
| `ekos/crates/kir/src/lib.rs` | Added `KirRelationship::is_pending_review()` |
| `ekos/crates/runtime/src/lib.rs` | `load_neighborhood`/`trace_impact` skip pending-review relationships; 2 new tests + 2 test helpers |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_dependents` skips pending-review relationships |
| `devlog_31.md` | This file |
