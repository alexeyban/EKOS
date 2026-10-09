# Devlog 244 — ConflictingEvidence: when sources disagree, keep both (RFC 0172)

**Date:** 2026-10-09
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Four earlier drafts (RFC 0148, 0154, 0158, devlog_189/204) assumed a "`ConflictingEvidence` diagnostic path" that never existed. When two sources disagreed, EKOS kept one and dropped the other silently, in three places: same-id duplicates (later path wins, with only a `SEM002` log line), exact-name merges (non-canonical properties discarded), and RFC 0170 code meanings (highest confidence wins). RFC 0172 turns each disagreement into a `ConflictingEvidence` item: every claim with its file and line, what EKOS kept, and a `Disputes` link. A person resolves it on the CLI; agents read it over MCP. On LedgerSMB it finds exactly five, and two of them expose a real extractor bug.

---

## What was built

| Component | Change |
|---|---|
| `semantic/src/conflicts.rs` (new) | `duplicate_definitions`, `merge_losses`, `label_mismatches`; `normalize_type`; `carry_forward`, `resolve` (`Pick(n)` / `BothValid`); 10 tests |
| `semantic/src/lib.rs` | `SemanticCompilerPass`: detector 1 right after combining artifacts, detector 2 before `apply_merges`; `CONF001` diagnostic |
| `kir/src/custom_kinds.rs` | `ConflictingEvidence` row (`structurally_keyed: true`) |
| `cli/src/commands/conflicts.rs` (new) | `carry_review`, `commit_step` (labels + `.ekos/conflicts/current.json` with counts), `items_in`, `open_for`, `agent_list`, `list`/`show`/`resolve`; commit-step test; MCP source-scan guard |
| `cli/src/commands/commit.rs` | carry review on compiled conflicts; conflicts step after business semantics; summary line |
| `cli/src/app.rs` | `ekos conflicts list|show|resolve` |
| `cli/src/commands/mcp.rs` | `ekos_conflicts` (listed unless disabled), `open_conflicts` on `ekos_state`; listing test |
| `cli/src/commands/ledger.rs` | `Conflicts :` line and `conflicts` object in `status --json`, read from the manifest |
| `compiler-core/src/config.rs` | `[conflicts] enabled = true` |
| `ekos/docs/rfcs/0172-conflicting-evidence.md` | the RFC |

## Measured

- **Small workspace.** Two files define `prefs` (different columns) and `orders.total` (`INTEGER` vs `BIGINT`, while `INT`/`INTEGER` and `SERIAL`/`INT` stay equal).
  - Detection: 2 conflicts.
  - Review: `resolve --pick 2` and `--both-valid --note` worked, and `--both-valid` without a note was refused.
  - Re-commit: the decisions were kept.
  - Change: switching `orders_v2.sql` to `NUMERIC(12,2)` reopened that conflict, with "was resolved by ann" kept.
  - MCP: `ekos_state` on `orders` listed it, and `ekos_conflicts` returned both.
- **LedgerSMB** (pipeline 245 s): 5 conflicts, 0 merge losses. "duplicate object id" `SEM002` warnings went from 3 to 0 (the other two duplicates were identical and collapsed silently).
  - `user_preference.columns`: `Pg-database.sql` vs `changes/1.9/transpose_user_prefs.sql`, a real schema change.
  - `account.category = 'A'.label`: *asset* (comment, `Pg-database.sql:72`) vs *L* (`FinStatements.sql:690`). Same for `'L'`. Both are **extractor misreadings**: a `CASE … WHEN category = 'A' THEN 'L'` is a sign-flip rewrite, not a label.
  - `oe.oe_class_id = 1/2.label`: *Sales/Purchase Order* vs *customer/vendor*. Not wrong: that is the counterparty, a different facet. A reviewer marks it `--both-valid`.

## Decisions

- **Collapse duplicates to the same winner as before.** Detection must not change which definition EKOS keeps (devlog_242's path order). It only stops the loss being silent, and it stops the ledger receiving two alternating versions per commit.
- **Unstated is not false.** A DDL column without `NOT NULL` and a dbt column with no test say different amounts. Comparing absence would flood the list, so only facts both sides state are compared.
- **Label detector at commit, outside `business_semantics`.** It reads the current `EnumMeaning`s after synthesis, so RFC 0170's code is untouched and the detector can be switched off on its own.
- **Its own small review lifecycle** (`open` / `resolved` / `dismissed`), not `semantics_review`. Conflicts are not hypotheses about meaning, and "both valid" has no semantics equivalent. The signature rule (claims' values and paths, no lines) is the same.
- **Counts in the manifest.** `ekos status` stays instant: `commit` and every `resolve` rewrite `{open, resolved, dismissed}`.
- **`ekos_state` finds conflicts through incoming `Disputes` links**, not by scanning all objects.

---

## Knowledge Captured

- **A conflict detector doubles as an extractor test.** The two LedgerSMB label conflicts are the first evidence that `case_label` mistakes a code rewrite for a label (TODO). Nothing else surfaced it: the wrong label had lower confidence and simply lost.
- **Same-id duplicates used to write two versions on every commit.** `append_object` compares against the *current* version, so A then B always looked new. Collapsing before the CKM fixes that as a side effect.
- **`SEM002` mixes two things:** duplicate ids (gone now) and relationships to `File` objects that `compile` never sees (2,635 on LedgerSMB, expected, RFC 0072). Don't read the count as an error rate.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/semantic/src/conflicts.rs`, `lib.rs` | new module, wired into `SemanticCompilerPass` |
| `ekos/crates/kir/src/custom_kinds.rs` | registry row |
| `ekos/crates/cli/src/commands/conflicts.rs`, `commit.rs`, `ledger.rs`, `mcp.rs`, `mod.rs`, `app.rs` | CLI, commit step, status, MCP |
| `ekos/crates/compiler-core/src/config.rs` | `[conflicts]` |
| `ekos/docs/rfcs/0172-conflicting-evidence.md` | new RFC |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | docs |
