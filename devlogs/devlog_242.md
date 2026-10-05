# Devlog 242 — LedgerSMB → LinkML demo, and a compile-order nondeterminism it exposed

**Date:** 2026-10-05
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`demo/ledgersmb-linkml/run.sh` goes from a clean checkout to a LinkML schema for LedgerSMB that LinkML's own tools accept. It clones LedgerSMB at a pinned commit, runs the full pipeline with `[semantics]`, reviews a small set of items as a stand-in reviewer (on the CLI and through `ekos import linkml`), exports, and checks the result with `linkml-lint`, `gen-json-schema`, `gen-pydantic` and `linkml-validate`. A record with `category: X` is rejected because the confirmed `CHECK` became the slot's enum range.

Building the demo showed that two runs on identical input gave different counts: 46 or 47 concepts, 219 or 222 coded values. The compile read knowledge artifacts in artifact-id order, and those ids hash a timestamp. The read order is now stable.

---

## The demo

| File | |
|---|---|
| `demo/ledgersmb-linkml/run.sh` | clone (pinned `544bcd947`) → pipeline → inspect → review (CLI + YAML import) → export → LinkML lint / generate / validate. `LEDGERSMB_SRC` for a local clone, `SKIP_REVIEW=1` to stop at hypotheses |
| `demo/ledgersmb-linkml/README.md` | the walkthrough, every review decision with the LedgerSMB text it rests on, and what the demo does not show |
| `demo/ledgersmb-linkml/output/` | one run's confirmed schema and transcript |

**Results on LedgerSMB:**
- Synthesis gives 47 concepts, 222 coded values (179 explained), 60 constraints, 15 gaps and 128 rationale links. 458 of 649 predicate sites resolved.
- After review, 21 items are confirmed: 8 concepts, 12 codes and 1 CHECK. The confirmed export has 14 classes and 3 enums; `--status all` has 127 classes and 61 enums.
- `linkml-lint` reports 0 errors and 83 warnings. 71 are slots without a description, because the SQL has no comment for them. 12 are permissible values named by their code.

The demo uses the release binary. `commit` on a fresh LedgerSMB ledger takes about 7 minutes in release and 8.5 in debug, nearly all of it writing about 11k evidence records, 11k objects and 8k relationships. Semantics synthesis takes about 30 s of that.

## The nondeterminism

LedgerSMB creates `user_preference` twice:
- `sql/Pg-database.sql` creates it with columns `language`, `stylesheet`, `dateformat` and so on.
- `sql/changes/1.9/transpose_user_prefs.sql` recreates it as `user_id`, `name`, `value`, with a `CHECK` and seed rows.

`sql_analyzer`'s `table_kir_id` derives an id from the table name, so both definitions get the same id and both reach the CKM. Whichever is written to the ledger last wins.

`SemanticCompilerPass` reads its knowledge artifacts in the order `dedup_knowledge_artifact_ids` returns, which was sorted by artifact id. A `KnowledgeArtifact`'s id hashes `meta.created_at`, so every `recover` shuffled that order, and with it the winner. With the newer definition, synthesis gains a concept (`UserPreferenceWithoutUserId`), 3 codes (`name = 'dateformat'`, …), a constraint and a gap. That is exactly the 46/47 difference.

**Fix:** the result is now sorted by the dedup key: source target (or input ids), then pass name, with artifact id only as a tie-break. On three fresh build → recover → compile runs, the `changes/1.9` definition came last every time. Two full demo runs then gave identical counts.

**Test:** `dedup_orders_by_source_target_not_by_artifact_id` builds 8 rounds with shifted timestamps and checks that the order follows the source path. It fails on the old id sort in round 1.

## Decisions

- **Fix the order, not the duplicate.** Two same-id objects still reach the CKM, and `SEM002` still reports them as duplicates. Deciding what two definitions of one table *mean* (base schema vs migration) is the open TODO item for applying `sql/changes/` on top of the base DDL. That needs its own design. A stable order makes the result reproducible now. "Later path wins" is arbitrary, but it is consistent, and on LedgerSMB it picks the newer schema.
- **The demo's reviewer is the script, and the README says so.** Confirming is human-only by design. The demo shows the mechanics; it does not claim an expert's verdict. Each decision cites the comment or seed row it rests on.
- **Release build in the demo.** Debug spends about 1.5 extra minutes in `commit`. A first-time user pays one release build instead.

---

## Knowledge Captured

- **An artifact id is not a stable sort key.** `KnowledgeArtifact` ids hash `created_at`, so anything ordered by artifact id is reshuffled on every `recover`. Any pass that folds artifacts where order matters (last write wins, first match wins) must sort by something from the source, such as the target path.
- **A determinism bug can look like natural variance.** Earlier RFC 0170 notes recorded LedgerSMB as "46–47 concepts, ~454–458 resolved sites", as if the range were measurement noise. A count that varies on identical input is a determinism bug; chase it before writing down a range.
- **`ekos … | head -n 1` panics** ("failed printing to stdout: Broken pipe"), because Rust's `println!` panics on EPIPE. Scripts should read the whole output (`awk 'NF && !p {print; p=1}'`). Fixing it in the CLI would mean resetting `SIGPIPE`, which needs `unsafe` (an RFC by this repo's rules).
- **`linkml-validate` needs every `required` slot.** Table classes mark `NOT NULL` columns `required`, including ones with a SQL `DEFAULT`, so a sample record must spell them out.
- **`linkml-lint --ignore-warnings --format tsv`** gives a parseable report and a non-zero exit only on errors. The TSV ends with a blank row.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/semantic/src/lib.rs` | `dedup_knowledge_artifact_ids` returns a source-ordered list; doc comment; regression test |
| `demo/ledgersmb-linkml/run.sh` | new: the demo |
| `demo/ledgersmb-linkml/README.md` | new: walkthrough |
| `demo/ledgersmb-linkml/output/ledgersmb.linkml.yaml`, `transcript.txt` | new: one run's schema and console output |
| `.gitignore` | `demo/ledgersmb-linkml/work/` |
| `README.md`, `TODO.md`, `ekos/docs/rfcs/0170-business-semantics-linkml.md` | demo pointer; TODO items; RFC numbers |
