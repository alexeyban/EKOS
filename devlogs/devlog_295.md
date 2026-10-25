# Devlog 295 — Review and digest-bound sign-off (RFC 0213 Phase 1)

**Date:** 2026-10-25
**PRs:** none (direct to `main`)
**Branch:** `main`

---

## Summary

Part II of the Oracle/iDempiere plan starts with the gate every later step needs: a person agrees to the source
model, and that agreement stops counting the moment a fact under it changes. `ekos review` computes six subjects
from the ledger (scope, PII, conflicts, identity, write behaviour, glossary); `ekos review scope|pii|write-behaviour`
records item decisions; `ekos signoff approve` signs a subject on the digest the person was shown. Validity is
recomputed on every read, never written: on the iDempiere testbed, a column added in Oracle made the PII sign-off
stale, naming the column, while the scope sign-off stayed valid. Separately, the EKOS repository's own GitHub wiki was
recorded as RFC 0214's publish target.

---

## RFC 0213 Phase 1 — review subjects, decisions, sign-off

### Problem / motivation
Steps 4–7 (requirements, extraction, bronze/silver, marts) build on which tables are in scope, which columns are
personal data, how rows change and disappear, which code ↔ database conflicts were explained. All of that was compiled
but nobody had agreed to it, and a later compile could change it silently.

### What was built

| Component | What it does |
|---|---|
| `semantic/src/review.rs` | subjects → `Item`s (signed `facts`, shown-only `info`, `proposed`, `decision`, `outdated`, `blocking`); `ScopeRules` (first-match `*` globs); canonical JSON + item/subject digests; `decision_object`, `signoff_object`, `approve_check`, `signoff_state` (valid / stale with added, removed, changed / revoked / none) |
| `ReviewDecision`, `SignOff` | keyed custom kinds (registry rows) |
| `[review.scope]` | `classes` rules tried first, `replace-defaults`, `default-out` |
| `commands/review.rs` | `ekos review list|show|scope|pii|write-behaviour`, `ekos signoff show|approve|revoke` |
| catalogue, `review_guard` | read / review classes; five human-only entry points |

### Measured (iDempiere `idemdocs`)

| Subject | Items | Blocking |
|---|---|---|
| scope | 903 | 0 |
| pii | 195 (name-based) | 0 |
| conflicts | 429 | 195 open, unexplained |
| identity | 25 | 25 unconfirmed |
| write-behaviour | 850 | 0 |
| glossary | 0 | 0 |

Schema change → full pipeline (build 17 s, recover 263 s, resolve 12 s, compile 32 s, commit 495 s, 0 model calls) →
`pii` stale, 1 added; `scope` valid. Each command: ~25 s, 2.5 GB peak (reads the whole ledger).

### Implementation details worth remembering
- **Signed vs shown.** Row, null and distinct counts are `info`, outside the digest. Signing them would make every
  re-profile invalidate the sign-off, and an operator who sees "stale" every day stops reading it.
- **The operator passes the digest back.** `approve` compares it with a fresh computation, so a decision or a compile
  landing between `show` and `approve` refuses the sign-off instead of signing a state nobody saw.
- **Decisions remember their item's facts digest.** If the facts change, the decision is `outdated` and the item is
  undecided again, which also changes the subject digest.
- **Scope propagates.** PII, write behaviour and conflicts only include in-scope tables, so putting `T_REPORT` in scope
  added its write-behaviour item (849 → 850).
- Canonical JSON is hand-written (keys sorted at every level) instead of relying on `serde_json`'s map order, because
  feature unification can turn on `preserve_order` from any dependency.

### Decisions
- **Validity is computed, not stored.** Nothing at compile or commit has to invalidate anything, so nothing can forget.
- **Blocking = open and unexplained** for conflicts: RFC 0211's `explanation` (pending migration, translation table)
  already says why a disagreement is expected; the plan's exit counts *unexplained* conflicts.
- **Existing review paths are kept.** Conflicts, identity and glossary items are decided by their own commands;
  `ekos review` only adds the three subjects that had none.
- **Identity is the weak spot.** `SameAs` candidates are confirmed through `ekos_identity_review`, which an agent can
  call; the sign-off is still human. A CLI identity review is listed as a follow-up.

---

## RFC 0214 publish target

The maintainer named github.com/alexeyban/EKOS/wiki as the GitHub wiki target for Part II.2. It is public, so each
source's `licence` is checked before publishing (the iDempiere docs repository has none), and each publish is
confirmed first (TODO, 17bc47c).

---

## Knowledge Captured

- **A project's `[llm]` block runs during the pipeline too.** It was removed from the testbed project before re-running
  it here; with Zen configured, `recover` would have named every SQL file with the paid model.
- **`pkill -f <pattern>` matched the tool's own shell** in the previous session; kill by PID.
- `ora-live`'s P2 profile writes `profile.pii` only on columns whose values it suppressed (tables with rows); for every
  other column, apply `ekos_common::pii::classify_name` yourself, or the PII list misses empty tables.
- A `Table` recovered from a PDF (`localdocs` tables) is `ObjectKind::Table` too; filter DDL tables by the `dialect`
  property `SqlAnalyzerPass` stamps.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0213-review-and-sign-off.md` | new RFC with Phase 1 results |
| `ekos/crates/semantic/src/review.rs`, `lib.rs` | review subjects, digests, decisions, sign-off (16 tests) |
| `ekos/crates/kir/src/custom_kinds.rs` | `ReviewDecision`, `SignOff` rows |
| `ekos/crates/compiler-core/src/{config,lib}.rs` | `[review.scope]` |
| `ekos/crates/cli/src/commands/{review,mod,catalogue,review_guard,conflicts}.rs`, `app.rs` | commands, classes, human-only guard |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0213 |
