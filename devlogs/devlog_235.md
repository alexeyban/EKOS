# Devlog 235 — RFC 0170 Phase 2: the business-semantics review loop

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0170's hypotheses can now be reviewed. `ekos semantics confirm|reject|edit` is the only way an
item becomes `confirmed` or `rejected`, it is CLI-only, and a source-scan test keeps MCP from ever
reaching it. A decision holds only while what the item asserts and its evidence are unchanged:
otherwise `ekos commit` turns it into `needs_review`, and a confirmed item whose traces disappear
is flagged rather than dropped. `ConceptConflict` catches same-column threshold disagreements and
name collisions. The default `ekos export linkml --status confirmed` now produces a real schema.

The real LedgerSMB run found three bugs, all fixed: nondeterministic evidence selection rewrote
the busiest items on every commit, `IS NOT TRUE` was conflated with `IS FALSE`, and
`SqlAnalyzerPass` had no logic version.

---

## What was built

| Component | Change |
|---|---|
| `semantic/src/semantics_review.rs` | New: `signature`, `carry_forward`, `stale_version`, `apply_review`, `Decision`. 4 tests |
| `semantic/src/business_semantics.rs` | `ConceptConflict` + `ConflictsWith`; a `signature` on every item; sites sorted. 2 tests |
| `kir` | `ConceptConflict` registry row; `is_not_true`/`is_not_false` ops |
| `recovery/src/sql_predicates.rs` | `IS NOT TRUE`/`IS NOT FALSE` kept distinct (1 test); logic versions bumped |
| `recovery/src/sql_analyzer.rs` | `SqlAnalyzerPass::version()` = `v2` |
| `cli/src/commands/semantics.rs` | Commit step carries reviews forward and flags stale ones; `review` (confirm/reject/edit); `list --status`; gaps report shows conflicts and the `needs_review` queue; review-based eval metrics. 2 tests (MCP guard, commit-step lifecycle over a real `FactLedger`) |
| `cli/src/commands/export.rs` | Expert name/description/label; `ekos_conflict`, `ekos_reviewed_by`, `ekos_review_note` |
| `cli/src/app.rs` | `semantics confirm|reject|edit`, `list --status` |

---

## Implementation details worth remembering

- **Signature = kind + core fields + set of evidence `(path, fragment)`.** Line numbers are left
  out on purpose: an edit above a predicate shifts every line below it, and that should not reopen
  every review. A new usage *does* change the evidence set, and so reopens it — the plan's rule
  ("any evidence fact changes").
- **`carry_forward` copies review fields always, the status only while unchanged.** Expert edits and
  the decision trail survive a change; the old decision moves to `previous_review`. An item already
  in `needs_review` stays there until a human looks, even if the evidence changes back.
- **Stale items stay in `.ekos/semantics/current.json`**, so `list`/`gaps` keep showing them.
- **Reject needs a note; edit needs at least one of `--name/--description/--label`; `--label` only
  on a coded value; rationale links cannot be reviewed** (a git fact, not a hypothesis).

## Decisions

- **Narrow conflicts.** Differing `IN` sets on one column are usually different concepts
  (`account.category IN ('A','E')` vs `IN ('E','I')`), so only thresholds and names are flagged.
  LedgerSMB has none; the threshold case is covered by a unit test.
- **Console cards deferred.** The CLI is a complete review surface; RFC 0127 cards are a UI over the
  same `apply_review`.

---

## Knowledge Captured

- **`ledger.all_objects()` order is arbitrary** (the fact engine folds entities out of a `HashMap`).
  Anything that truncates (`take(12)`) or numbers by position (`evidence:{key}:{n}`) after iterating
  it is nondeterministic, which shows up only once an item has more sites than the cap. The symptom
  was "5 new ledger entries" on every re-commit for the five busiest items, never on small ones.
  Sort first; test with the input reversed.
- **`IS NOT TRUE` ≠ `IS FALSE`.** `NULL IS NOT TRUE` is true. LedgerSMB writes `IS NOT TRUE` on
  nullable booleans throughout, so the distinction yields real concepts ("not approved, or never
  set"). Found only because an edit to a confirmed concept's source failed to reopen it — a
  lifecycle test on real code checks the extractor as well as the lifecycle.
- **A pass whose `cache_inputs` hashes only its input text needs a `version()` bump whenever its
  output shape changes.** `SqlAnalyzerPass` had none; Phase 1 changed its `Table` output without one.

---

## Verification

- LedgerSMB clone, end to end: confirmed `TransactionsNotApproved`, edited
  `PartsWithInventoryAccnoId` → "Inventory part", rejected `UserPreferenceWithoutUserId` and one
  gap, relabelled `oe.oe_class_id = 4`; re-commit → reviews intact, 0 new entries. Editing a
  supporting line in `Drafts.sql` → the concept becomes stale `needs_review`; restoring it →
  re-derived, still `needs_review` until re-reviewed; next commit writes 0.
- Starter-set eval after the fixes: concept recall 0.68 (was 0.73; the starter set says `IS FALSE`
  where the code says `IS NOT TRUE`), label accuracy 0.95, evidence validity 1.0 (416/416).
- `ekos export linkml` (`confirmed` → 3 classes, 1 enum; `all` → 118 classes): `linkml-lint` 0
  errors, `gen-json-schema` succeeds for both.
- `cargo test --workspace` (157 test binaries), clippy `-D warnings`, `fmt --check`, LedgerSMB
  corpus ratchet.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/semantic/src/semantics_review.rs`, `business_semantics.rs`, `lib.rs` | Lifecycle, conflicts, signatures, deterministic sites |
| `ekos/crates/kir/src/predicates.rs`, `custom_kinds.rs` | New ops; `ConceptConflict` row |
| `ekos/crates/recovery/src/sql_predicates.rs`, `sql_analyzer.rs`, `plpgsql_analyzer.rs`, `view_analyzer.rs` | Three-valued ops; versions |
| `ekos/crates/cli/src/commands/semantics.rs`, `export.rs`, `app.rs` | Review commands, carry-forward, display, export |
| `ekos/docs/rfcs/0170-business-semantics-linkml.md` | Phase 2 section |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented |
