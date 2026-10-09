# RFC 0172 — ConflictingEvidence: when sources disagree, keep both and say so

| | |
|---|---|
| **Status** | Implemented — Phases 1 and 2 (2026-10-09); Phase 3 (doc-vs-data in Migrate) not started |
| **Supersedes** | the "`ConflictingEvidence` diagnostic path" earlier drafts assumed (RFC 0148, 0154, 0158 and devlog_189/204 record that it never existed) |

## Problem

When two sources make different claims about the same thing, EKOS keeps one and silently drops the other:

| Where | Today | Real example (LedgerSMB) |
|---|---|---|
| Same-id duplicates in the CKM | Both objects reach the ledger, `SEM002` logs "duplicate object id", and the later path wins (devlog_242) | `user_preference`: `sql/Pg-database.sql` (`language`, `stylesheet`, …) vs `sql/changes/1.9/transpose_user_prefs.sql` (`user_id`, `name`, `value`) |
| Exact-name auto-merges (`apply_merges`) | The non-canonical objects are dropped with their properties | a table whose DDL, dbt model and ORM model disagree on a column type |
| Code meanings (RFC 0170 `meanings`) | The highest-confidence label wins; the others are kept but never compared | `account.category = 'A'`: the column comment says *asset*, and `FinStatements.sql:690` was read as *L* — that CASE is a sign flip, not a label |

Identity's `SameNameDifferentKind` and RFC 0170's `ConceptConflict` are different problems: one is about names across kinds, the other compares concepts with each other, not the evidence behind one fact.

## Model

A `Custom("ConflictingEvidence")` object per (subject, attribute):
- **Registry and id:** registered with `structurally_keyed: true`; its id is v5 of `conflicting-evidence:<subject id>:<attribute>`, so an unchanged disagreement appends nothing on re-commit.
- **Properties:**
  - `subject_id`, `subject_name`, `subject_kind`, `attribute`;
  - `conflict_type`: `duplicate_definition`, `merge_loss` or `label_mismatch`;
  - `claims`: one per source (`value`, `path`, `line`, `source`), sorted;
  - `chosen`: the value EKOS kept;
  - `status`: `open`, `resolved` or `dismissed`;
  - `signature`, plus the review fields.
- **Evidence and links:** one `KirEvidence` per claim, pointing at that claim's own file and line, and a `Disputes` relationship to the subject.

**Values are normalized before they are compared**, so equivalent spellings never count as conflicts:
- SQL type aliases are equal (`int`/`integer`/`int4`/`serial`, `bool`/`boolean`, `varchar(n)`/`character varying(n)`, `timestamptz`/`timestamp with time zone`, …);
- identifiers are compared case-insensitively;
- a fact one source does not state (say, `not_null` absent) is unknown, not "false". Only facts both sources state are compared;
- free-text prose (descriptions, comments) is never compared.

## Detection (deterministic, no LLM)

`ekos_semantic::conflicts`, pure functions:

1. **`duplicate_definitions(&mut KirGraph)`** runs in `SemanticCompilerPass` right after the artifacts are combined. It groups objects by id. Identical duplicates collapse silently. For differing ones, each disagreeing attribute becomes a conflict, and the group collapses to the last one read (path order, devlog_242). So the ledger stops getting two versions per commit, and `SEM002` stops firing for these.
2. **`merge_losses(&KirGraph, &[MergeProposal])`** runs before `apply_merges`, over each exact-name merge group.
3. **`label_mismatches(&[KirObject])`** runs at `ekos commit`, after business-semantics synthesis. It looks at the `EnumMeaning` items: two labels from *different* sources that share no normalized word are a conflict.

Compared attributes, for any object carrying them:
- `columns`, the set of names;
- per shared column, `data_type`, `not_null` and `primary_key`;
- `check_constraints`, the set of expressions;
- for `EnumMeaning`, `label`.

## Review (human-only)

`ekos conflicts resolve <id> --pick <n> | --both-valid --note …` writes a new version: `resolved` (with the picked claim) or `dismissed` (both valid, e.g. two facets of one code).

At commit, a fresh conflict keeps its review while its `signature` is unchanged. The signature covers the attribute plus each claim's value and path, not lines. When a claim changes, the conflict reopens as `open` with the old decision in `previous_review`.

Nothing in `commands/mcp.rs` may reach the resolve path, and a source-scan test enforces it. `.ekos/conflicts/current.json` lists the conflicts the latest commit derived. The ledger is append-only, so a conflict that disappeared is simply no longer current.

## Surfaces

| Surface | |
|---|---|
| `ekos conflicts list [--status] [--type] [--json]` / `show <id or name>` / `resolve` | CLI |
| MCP `ekos_conflicts` (read-only) | the current conflicts, filterable by subject |
| MCP `ekos_state` | an `open_conflicts` list for the object |
| `ekos status` / `--json` | count of open conflicts |
| `[conflicts] enabled = true` | default on; `false` restores the old silent behaviour |

## Not in this RFC

- **Phase 3, doc-vs-data:** in `migrate-dq`, a documented, checkable claim compared with profiled data (RFC 0158 `DQ.CONSIST.DOC`), emitted in the same shape.
- **Flags in answers:** a `disputed: true` flag on `ekos_query` / `ekos_retrieve` claims.
- **Console:** a conflicts view, and the RFC 0127 graph halo.

## Tests

- Each detector, in both directions, with the negative cases: type aliases, a fact only one side states, identical duplicates, two labels from the same source.
- Reversed input gives identical output.
- Ids are stable.
- Carry-forward keeps a review while the claims are unchanged and reopens it when they change.
- `resolve` validation.
- The MCP source-scan guard.
