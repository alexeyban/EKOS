# Devlog 219 — RFC 0162: a report that cannot lie, and a determinism test that earned its keep

**Date:** 2026-09-26
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos-migrate-report` and `ekos migrate report`. The report is compiled from ledger facts rather than
written; every factual sentence carries the ids it rests on; the verifier resolves each one and checks
it is the *kind* of fact that can support the claim; and five mechanical preconditions decide whether
it can be signed.

Compiled against the live demo project, it renders with groundedness 1.000 and — correctly — refuses
to be signable, naming three blockers.

---

## A correction to RFC 0162

The RFC said the groundedness metric would be **reused** from `crates/evals`. That was too strong.
The evals evaluator is built around a `Scenario` — expected facts, refusal phrasings, an answer
produced by a model — and a compiled report has none of those. What is shared is the *shape* of the
metric (coverage × validity × grounded rate) and its reasoning, not the code.

Writing a wrapper to make "reuse" technically true would have been worse than saying so. The crate
doc says which it is.

---

## The determinism test found a real bug, twice

RFC 0162 requires that recompiling from the same snapshot produces byte-identical output — that is
what makes a signed report *re-derivable* rather than merely archived. The unit test asserts
`markdown(&r) == markdown(&r)`, which passes trivially and proves nothing about the compiler.

So I compiled the real report twice and diffed. It differed.

**First cause:** `store.all_objects()` returns no particular order, so findings rendered in a
different sequence each time. Sorted.

**Second cause, after sorting by name:** still different. Sorting by name is not a *total* order —
several findings share an object name, because one column can trip both `DQ.UNIQ.001` and an
inferred-FK rule. Ties broke arbitrarily. The key is now `(name, rule_id, id)`, with the id as the
tiebreak that guarantees totality and `rule_id` first because that is the order a human wants.

Four compilations, byte-identical. Neither bug is visible from a unit test; both are visible from
running the thing twice.

---

## Design notes

**The support check is structural, not semantic.** A claim about a row count may cite a
`MigrationValidationResult`, not a `MigrationTargetDesign`. That catches the common failure — a
plausible nearby id attached to a sentence — without putting a model in the verification loop, which
would reintroduce the problem one level up. Semantic entailment checking is the tempting next step
and is deliberately not taken.

**Narrative may explain; it may not state.** A prose sentence containing a digit is refused as
`NarrativeAssertsFact`. Deliberately blunt: a false positive costs an author one rewrite, and a false
negative is an uncited number in a document somebody signs.

**An empty report scores 0.0, not 1.0.** Every ratio is zero-denominator-safe *downward*. A report
with no factual claims is not perfectly grounded — it is empty, and scoring it 1.0 would let a report
that says nothing clear a gate a real one has to.

**An empty section says it is empty.** Omitting it reads as "inspected and clean", which is a
different statement from "nothing recorded".

**Blockers come before content.** A reader who stops after one screen must know whether this is
signable. The verdict is the third line.

**A failing report is still produced.** The person who has to close the gaps needs to see them, and
withholding the document makes that harder.

---

## Knowledge Captured

**`markdown(&r) == markdown(&r)` is not a determinism test.** It exercises one call on one value and
passes whatever the compiler does. The real test is compiling the *pipeline* twice from the same
ledger and diffing, and it found two bugs a unit test cannot reach. Any "deterministic output" claim
needs an end-to-end diff, not a self-comparison — the same lesson as devlog_209's self-comparison
near-miss, arriving in a different shape.

**Sorting by a non-unique key is not sorting.** `sort_by_key(|o| o.name)` looks like it establishes an
order and does not when names repeat. Anywhere output must be stable, the sort key has to be provably
total — usually by appending an id.

**The report must distinguish "not measured" from "zero".** A finding with no measurement renders as
*"not measured"*, never as *"0 rows affected"*. This is the same distinction the profiler needed when
an empty `TABLESAMPLE` read as precision 0, and the same one `affected_rows: Option<i64>` exists to
preserve — it survives all the way to the document a human signs, which is the only place it finally
matters.

**"No controls were run" is a missed control.** The preconditions list it explicitly rather than
leaving `controls_missed` empty, because an empty list reads as "every control passed". A tier that
ran none has not demonstrated it can see anything.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-report/src/citation.rs` | New — `Claim`, `ClaimKind`, structural support check, three-component groundedness, 8 tests |
| `ekos/crates/migrate-report/src/signoff.rs` | New — five preconditions, each independently blocking, 6 tests |
| `ekos/crates/migrate-report/src/render.rs` | New — deterministic Markdown, blockers first, 6 tests |
| `ekos/crates/cli/src/commands/migrate.rs` | `report` — compiles Scope / Findings / Approvals / Drift from facts, with a total sort order |
| `ekos/crates/cli/src/app.rs` | `ekos migrate report --threshold --out` |
| `TODO.md` | RFC 0162 core ticked; `signoff`, HTML and PDF remain |
