# RFC 0170 — Business semantics from technical traces, exported as LinkML

**Status:** Accepted — Phases 1–3 implemented (devlog_234, 235, 238); Phase 4 implemented except ontology mapping suggestions (devlog_236, 237); 2026-10-03
**Date:** 2026-10-03
**Related:** RFC 0135 (provenance + determinism — this RFC's Phase 0), RFC 0146 Phase 2 (`COMMENT ON`
descriptions), RFC 0163 (PL/pgSQL IR, `plpgsql_footprint`), RFC 0169 (views), RFC 0029/0063
(reviewable, never-auto-merged hypotheses), RFC 0138 (eval harness)
**Source plan:** `ekos-semantic-layer-linkml-plan.md` ("EKOS × LinkML: Recovering the Semantic Layer
from Technical Traces")

---

## Summary

Business meaning — "an active part", "an unapproved transaction", "entity class 2 is a customer" — is
rarely written down. It is left as **traces**: predicates in views and routines (`WHERE NOT
p.obsolete`), magic values (`entity_class = 2`) and the lookup rows, `CASE` branches and column
comments that explain them, and the commits that introduced each line.

EKOS recovers these traces deterministically, synthesizes **hypotheses** from them —
`BusinessConcept`, `EnumMeaning`, `ConstraintCandidate`, `SemanticGap`, `RationaleLink` — each with
the evidence it came from, and exports them as a **draft LinkML schema** a domain expert confirms or
corrects. LinkML tooling does everything downstream (JSON Schema, DDL, Pydantic, RDF/OWL/SHACL).

**EKOS feeds LinkML; it does not replace it.** Nothing here is a confirmed definition. Every exported
element carries `ekos_status: hypothesis` until a human decides (Phase 2).

## Hard limits (stated publicly, as the plan requires)

1. **No trace, no recovery.** Meaning that exists only in people's heads cannot be extracted. EKOS
   can only point at where it is missing — the gap report.
2. **Recovered meaning is a hypothesis.** Nothing becomes confirmed semantics without a human.
3. **Inference quality is the open problem.** The export is engineering; rebuilding correct business
   definitions from technical traces is not guaranteed, and Phase 1 deliberately uses **no LLM**.
4. **Downstream generation is out of scope** — that is LinkML's job.

## Phase 0 — prerequisites (already satisfied)

The plan names provenance and id determinism as prerequisites. In this repository both are RFC
0135: Part B (`WriteContext`, `source_artifact_ids` per object, `ekos ledger audit`) and Part C
(`KirRelationship::deterministic`, v5 ids everywhere). Every object this RFC adds has a v5 id derived
from its structural key, so it is stable across rebuilds and re-commits write nothing new.

## Design — Phase 1 (MVP)

### 1. Trace extraction (recovery, per SQL file, deterministic)

**`recovery/src/sql_predicates.rs`** walks a parsed `sqlparser` AST and records every
**column-vs-literal** predicate in a `WHERE`, `HAVING`, `JOIN … ON`, `CASE` branch or `CHECK`:

| Field | Meaning |
|---|---|
| `relation` | The relation owning the column, through the enclosing `FROM`'s aliases (or the `UPDATE`/`DELETE` target). Unqualified columns resolve only when one relation is in scope |
| `scope` | When `relation` is unknown: the relations in scope, resolved later against real table columns — never guessed |
| `column`, `op`, `values` | Normalized: `=` → `in`, `<>` → `not_in`, literal on the left flipped, `NOT x` on a boolean → `is_false`, `a = 1 OR a = 3` → `a IN (1, 3)`; values sorted and de-duplicated |
| `clause` | `where`, `having`, `join_on`, `case`, `check` |
| `top_level` | A top-level `AND` conjunct of its clause — part of the clause's definition, not a branch of an `OR` |
| `label` | For a `CASE` branch: its literal result — the code's candidate meaning |
| `line` | Absolute line in the file |

So `status IN (1,3)`, `status = 1 OR status = 3` and `3 = status OR status = 1` are one predicate
(the plan's normalization requirement). Comparisons against a column, a parameter or a variable are
plumbing, not business rules, and are dropped; PL/pgSQL routines additionally drop any "column" that
is one of the routine's own parameters or declared variables (`in_from_date IS NULL`), which on
LedgerSMB removed 190 of 623 raw sites.

Where the sites land:

| Carrier | Property |
|---|---|
| `View` (RFC 0169) | `predicates` — its query's |
| `ProcedureStatement` (RFC 0163) | `predicates` — its embedded SQL's (line = the statement's) |
| `Procedure`, `LANGUAGE sql` | `predicates` — its body's (it has no statements) |
| `Table` | `check_constraints: [{name, expression, line, predicates}]` |
| `Table` | `seed_rows: [{line, values: {column: literal}}]` — literal `INSERT … VALUES` rows into a table the same file creates (≤ 200 per table): a lookup table's seed is where a code's meaning lives |
| `Table.columns[]` | `not_null`, `primary_key`, `unique` (single-column table-level keys included) |

### 2. Synthesis (`ekos commit`, `[semantics] enabled = true`)

`semantic/src/business_semantics.rs`, a pure function over the committed graph (like RFC 0163's
`procedure_lineage`, and for the same reason: a predicate in one file names a table created in
another, and a lookup table's seed rows meet the column referencing it only once every file is
committed). It runs after `procedure_lineage`.

**Column resolution.** A site with a `relation` keeps it when that relation is a known `Table`,
`Dataset` or `View`. A site with only `scope` resolves when exactly one in-scope table declares that
column. Everything else is unresolved and contributes nothing.

**`BusinessConcept`** — a candidate named concept, from two sources:

- `view` — a `View` whose `WHERE` has resolved top-level conjuncts defines the concept "rows of the
  view": the conjunction, named after the view.
- `recurring` — the same canonical predicate (`where`/`having`/`join_on`) used by at least
  `min-sites` (default 2) distinct statements or views.

Excluded: predicates on a *row key* — a single-column primary key or unique column
(`defaults.setting_key = 'curr'`, `users.username = 'Migrator'` pick one row, they do not classify
rows), or a column compared against more than `max-enum-values` (12) distinct literals. Names are deterministic and
explicitly marked derived (`name_source: view | derived`) — e.g. `PartsNotObsolete`,
`AccountCategoryAOrE`. No LLM writes definition text in Phase 1: the definition **is** the predicate.

**`EnumMeaning`** — one per `(table, column, value)` for every coded column (compared by
`in`/`not_in` against non-boolean literals, or constrained by a `CHECK … IN`), with every meaning
source found, in this priority:

| Source | Example | Confidence |
|---|---|---|
| `column_comment` | `COMMENT ON COLUMN account.category IS 'A=asset,L=liability,…'` | 0.9 |
| `lookup_seed` | `entity_credit_account.entity_class` → FK → `entity_class` seeded `(2,'Customer')` | 0.8 |
| `lookup_seed` (own key) | `oe_class.id = 2` → `oe_class`'s own seeded row `(2,'Purchase Order')` | 0.8 |
| `case_label` | `CASE WHEN x.kind = 1 THEN 'AP'` | 0.5 |

A value the referenced lookup table seeds *without* any label column is still a gap, and the gap
says which table declares it.

A column referencing a seeded lookup table gets **every** seeded value, so the enum is complete
whether or not code compares against each one.

**`ConstraintCandidate`** — one per `CHECK` constraint: its expression, normalized predicates and a
`constraint_type` (`range`, `enum`, `pattern`, `not_null`, `other`).

**`SemanticGap`** — the gap report, as objects:

| `gap_type` | When |
|---|---|
| `unexplained_value` | A coded value used in logic with no meaning from any source |
| `undocumented_concept` | A concept with no author description (view comment) and no rationale commit |

**`RationaleLink`** (when the workspace is a git repository and `[semantics] rationale = true`) —
`git blame -w` on each concept's (and each unexplained value's) evidence lines → the commit that
last changed the line's content (whitespace-only commits ignored), its summary and date. "Last
changed", not "introduced": it is who to ask, not proof of intent. A `RationaleLink` per `(concept, commit)`, linked `ExplainedBy`. The blame source is a
trait (`RationaleSource`), so the pure synthesis never shells out; the CLI provides the git
implementation.

Every object carries `status: hypothesis`, `confidence`, and `evidence` records with path + line.
Relationships: concept/enum/constraint → `Table` (`Custom("Describes")`), concept → the `View` or
statement it was seen in (`Custom("EvidencedBy")`), gap → its subject (`Custom("Describes")`),
concept → `RationaleLink` (`Custom("ExplainedBy")`). Concepts **link to** technical entities and are
never merged into them. Concepts use the built-in `ObjectKind::BusinessConcept`; the four `Custom` kinds are structurally keyed in the identity registry. All are created at `commit`, after identity resolution, so none is ever a merge candidate.

### 3. Read surface (Phase 1)

- `ekos semantics list [--kind concept|enum|constraint|gap|rationale]`
- `ekos semantics show <name-or-id>` — the item, its evidence and its links
- `ekos semantics gaps` — the gap report, for a human to review
- `ekos export linkml [--status hypothesis|confirmed|all] [--out model.yaml]`

### 4. LinkML export

| EKOS | LinkML |
|---|---|
| `Table` touched by any item | `class` with `attributes` (range from SQL type, `required` from NOT NULL, `identifier` from a single-column PK) |
| `BusinessConcept` | `class` with `is_a: <Table class>`, `description` = the predicate, `annotations.ekos_*` |
| `EnumMeaning` per column | `enum` `<Table><Column>`, `permissible_values` keyed by the code, `description` = best label |
| `ConstraintCandidate` | `slot_usage` on the table class: `minimum_value`/`maximum_value`, `pattern` (from `LIKE`), or the enum as `range` annotation |
| status / confidence / evidence | `annotations.ekos_status`, `ekos_confidence`, `ekos_evidence` (compact `path:line` refs; commit refs when rationale exists) |
| `SemanticGap` | `annotations.ekos_gap` on the value or class, plus `comments` |

Default filter is `--status confirmed`, so hypotheses are never exported as facts by accident; Phase 1
has nothing confirmed, so `--status all` (or `hypothesis`) is what produces a draft — and the command
says so when the filter leaves the schema empty. Output validates with `linkml-lint` and
`gen-json-schema`.

### 5. Evaluation

`ekos semantics eval --gold <gold.yaml>` scores the recovered semantics against a gold set:
enum coverage (share of coded values with ≥1 meaning hypothesis), meaning precision against gold
labels, concept recall/precision on canonical predicates, and gap usefulness when the gold set
lists known unknowns. The gold set must be written by a domain expert **before** looking at EKOS's
output; the repository ships the format and a small starter set derived from LedgerSMB's own
documentation, clearly marked as not expert-authored.

## Phase 2 — review loop (implemented, devlog_235)

**Lifecycle.** `hypothesis → confirmed | rejected`; `confirmed`/`rejected → needs_review` when what
the item asserts or its evidence changes; a reviewed item whose traces vanish gets a stale
`needs_review` version (never silently dropped). Rejected items whose traces vanish are left alone.

**Signature.** Every item carries `signature` = hash of its kind, its *core* fields (concept:
definition + table; enum: value, label, meanings' label/source/path; constraint: expression; gap:
type + subject; conflict: definitions) and the set of its evidence `(path, fragment)` pairs.
**Line numbers are excluded**, so an edit above a predicate does not reopen every review. A review
records `reviewed_signature`; at `commit`, `carry_forward` keeps the decision only while the fresh
signature equals it — confirmations never carry over a changed assertion (the identity-review
principle). The old decision is kept in `previous_review`, and expert edits survive.

**Human-only.** `ekos semantics confirm|reject|edit <name-or-id> [--as who] [--note …]` (reject
requires a note; `edit` takes `--name`, `--description`, `--label` for a coded value and confirms
with corrections). The pure transition is `ekos_semantic::semantics_review::apply_review`; the CLI is
its only caller, and a source-scanning test fails the build if `commands/mcp.rs` ever names it.
Writes go through the ledger with stage `semantics-review`, so `ekos ledger audit` shows who decided.

**`ConceptConflict`** (deliberately narrow): *threshold* — one column, the same comparison direction,
different literals (the plan's "90 days vs 60 days"); *name* — one name for different definitions.
Differing `IN` sets on one column are **not** flagged: on LedgerSMB they are different concepts
(`category IN ('A','E')` vs `IN ('E','I')`), not disagreements. LedgerSMB has 0 conflicts.

**Read side.** `list --status`, gaps report shows open conflicts and the `needs_review` queue with
the reason; rejected gaps/conflicts drop out of it. Export uses `expert_name`/`expert_description`/
`expert_label`, annotates `ekos_reviewed_by`/`ekos_review_note`/`ekos_conflict`; the default
`--status confirmed` now yields a real schema once anything is confirmed. `eval` adds the plan's
review-based metrics: *definition precision* (reviewed concepts accepted without edits) and *gap
usefulness* (reviewed gaps confirmed as real unknowns).

**Console (devlog_236).** The RFC 0127 web console's *Semantics* tab is the review UI: a queue
with evidence and confirm/reject/edit, the gap report, and a LinkML viewer + YAML editor. It runs
the same human-only CLI commands under the write role, recording the OIDC identity or, in token
mode, a typed name as `token:<name>` — a shared token is not an identity and the ledger says so.

### What Phase 2's real run changed

- **Site order came from the ledger's unordered iteration.** Items with more than 12 sites cite the
  first 12, so their evidence subset — and signature — changed every commit, rewriting the busiest
  concepts each run. Sites are now sorted; a test feeds the graph reversed and requires identical
  output.
- **`IS NOT TRUE` had been normalized to `IS FALSE`.** Wrong under three-valued logic (`NULL IS NOT
  TRUE` holds). Found because an edit from `IS FALSE` to `IS NOT TRUE` did not reopen a confirmed
  concept. LedgerSMB uses `IS NOT TRUE` widely: it now yields its own NULL-tolerant concepts
  (`CrReportNotApprovedOrUnset`, …) and the starter-set recall drops 0.73 → 0.68, because the
  starter set (written against the buggy output) says `IS FALSE` where the code says `IS NOT TRUE`.
- **`SqlAnalyzerPass` had no logic version.** Its cache key hashes only the SQL, so a workspace
  recovered before 0170 would have kept tables without seeds or constraints. Now `v2`.

## Phase 4 (part) — LinkML round trip (implemented, devlog_236)

`ekos export linkml` annotates every recovered element with `ekos_id` (and the schema with
`ekos_export_status`); `--json` prints the schema as JSON. `ekos import linkml <file> [--dry-run]
[--json] [--as]` rebuilds the export the file came from and diffs element by element, matched by
`ekos_id`: a renamed concept class → `edit --name`; a changed concept `description` →
`edit --description`; a changed permissible-value `description` → `edit --label`;
`ekos_status: confirmed|rejected` (+ `ekos_review_note`) → `confirm`/`reject`. A renamed code, an
unknown id or a rejection without a note is an error; new classes, deletions and other statuses
are warnings and import nothing. Validation is all or nothing. Human-only: the source-scan test
also bans `import::linkml` from `commands/mcp.rs`. An expert's description now *replaces* the
generated one in the export (the predicate stays in `ekos_definition`), so a round trip never
grows it; on LedgerSMB, export → edit → import → export → import plans zero decisions.

Re-confirming an already-confirmed, unchanged item with no note returns the same version, so the
ledger writes nothing.

## Phase 4 (part) — agents (implemented, devlog_237)

`ekos_semantics_lookup {term, limit?, include_rejected?}` and `ekos_semantics_gaps {scope?, limit?}`
are listed only with `[semantics] enabled = true`, read the server's cached read-only store, and
answer inside an `untrusted: true` envelope (recovered descriptions come from source comments —
data, never instructions). Lookup ranks exact name, then name substring, then a field match, and
confirmed before needs-review before hypotheses; each result carries `status` *and*
`status_means` — a sentence an agent cannot misread ("HYPOTHESIS — recovered from code traces,
not confirmed by anyone; say so if you use it"). Rejected definitions are excluded unless asked
for. Nothing found is `no_semantics_found` with an instruction not to invent a meaning. Gaps
returns the open questions (unexplained codes, undocumented concepts, conflicts) most-used first,
plus the `needs_review` items. Both handlers are covered by the MCP argument-declaration guard;
the human-only guard still bans every review path from `commands/mcp.rs`.

The console gained **bulk review** (`ekos semantics confirm|reject` take many targets — resolved
and checked first, written all or nothing; `POST …/semantics/review-bulk`) and a **side-by-side
diff** of the edited YAML against the current export (Myers' line diff, context-collapsed).

## Phase 3 — wider sources (implemented, devlog_238)

| Source | What it adds | Where |
|---|---|---|
| PL/pgSQL conditions | `IF`/`ELSIF`/`WHILE`/`EXIT WHEN` predicates on `NEW.`/`OLD.` (clause `condition`, placeholder relations `$new`/`$old`), resolved at synthesis to the table of the trigger(s) running the routine when they all fire on one table; unqualified names in a condition are variables and are ignored | `sql_predicates::condition_predicates`, `plpgsql_analyzer`, `business_semantics::collect_sites` |
| Pentaho `FilterRows` | Predicates read from the structured `<condition>` tree (field, function, typed value; nested `AND` stays top-level, `OR` makes branches; `IN LIST`, `STARTS WITH`… normalized; field-vs-field skipped), on the step's upstream `TableInput` table, with the XML line | `pentaho_analyzer::filter_predicates` |
| Standalone `SELECT`s | Analyst/report queries carry predicates on their Transformation-IR `Filter` node. Views and routines do not (their own analyzers already carry them — no double counting) | `sql_transform_analyzer::top_level_query_predicates` |
| dbt `schema.yml` | Column descriptions (→ legends), `not_null`/`unique` (→ column facts, row keys), `accepted_values` (→ a declared domain), `relationships` (→ a foreign key into lookup meanings); each test becomes a `ConstraintCandidate` with `source: dbt_test`. `tests:` and `data_tests:`, arguments inline or under `arguments:` | `dbt_analyzer::column_json` |
| Seed rows | Each `VALUES` row cites its own line, found in the text (sqlparser 0.53 gives literals no span) | `sql_analyzer::seed_rows` |
| Legends | Also `A asset, L liability, Q equity` — only when every part is a short upper-case code and a word | `business_semantics::comment_legend` |
| LLM text (opt-in) | `[semantics] llm-definitions = true`: ≤2 sentences per undocumented concept from the `[llm]` provider. The model sees only the concept's evidence plus the known meanings of its codes, numbered; a sentence citing nothing valid, or hedging ("likely", "probably", …), is dropped. Stored as `llm_definition` + per-sentence `llm_citations` — never the definition, never status/confidence/signature; exported as `ekos_ai_summary`. A cloud provider asks before spending (like `[llm-description]`); a cached provider re-asks nothing | `recovery::semantics_llm`, `semantics::describe_with_llm` |

Measured: LedgerSMB 522 → 534 predicate sites (373 resolved), 40 concepts, re-commit writes 0. The
analytics demo's dbt project (no SQL bodies EKOS parses): 91 dbt constraint candidates, 27 coded
values in 7 columns with all five `account_category` codes explained by the dbt description. LLM
text with local `llama3` on 8 LedgerSMB concepts: 8 described; the first version called `Q`
accounts "likely Quality accounts", which is why code meanings became evidence and hedged sentences
are dropped — the second run says "equity accounts".

## Phases 4 remainder (proposed)

- **Phase 3 — not covered.** Confluence glossaries and application constants (Perl/Python) are not
  read; neither are predicates inside dbt model SQL (Jinja is not parsed).
- **Phase 4 — remainder.** Optional ontology mapping suggestions (hypotheses only).

## Results — LedgerSMB, Phase 1 (2026-10-03)

SQL + git history only (UI/Perl/docs ignored), no LLM, on a local clone of the 19,542-commit repo.

| | |
|---|---|
| Predicate sites recorded / resolved to a table | 522 / 368 |
| `BusinessConcept` (1 from a view, 38 recurring) | 39 |
| `EnumMeaning` — coded values in 56 columns / with a meaning | 209 / 173 |
| `ConstraintCandidate` | 60 |
| `SemanticGap` (unexplained values) | 8 |
| `RationaleLink` | 97 |
| Re-commit on unchanged sources | 0 new ledger entries |
| `linkml-lint` | 0 errors; warnings only `recommended` (undocumented columns) and `standard_naming` (numeric codes are the stored values) |
| `gen-json-schema`, `gen-pydantic` | succeed |

Against the **starter** gold set `0170-ledgersmb-starter-gold.yaml` — written by the implementing
session after the first run, not by an accounting expert, so these are smoke-test numbers, not a
quality claim:

| Metric | Value |
|---|---|
| Concept recall (exact predicate) | 0.73 (16 / 22) |
| Concept precision vs gold (lower bound) | 0.41 (16 / 39) |
| Enum coverage on gold codes | 1.0 (22 / 22) |
| Enum label accuracy | 0.95 (21 / 22; `RFQ` vs "Request for quotation") |
| Gap recall | 1.0 (1 / 1) |
| Evidence validity (cited line exists and names the column or meaning) | 1.0 (420 / 420) |

The six missed concepts are four the extractor cannot see by construction (overdue, unpaid — column
vs column or date; on hold, reversed — no literal use in SQL) and two that occur in only one
place (`acc_trans.cleared IS FALSE`, `account.contra IS TRUE`), below `min-sites`.

### What the first real run changed

1. **`Custom("BusinessConcept")` silently vanished on read.** `ObjectKind` already has a built-in
   `BusinessConcept`; the `Custom` variant is `#[serde(untagged)]`, so it serialized to the same
   string and came back as the built-in variant — every concept was in the ledger and invisible to
   a `Custom` match. Concepts now use the built-in kind, and a test round-trips every kind.
2. **PL/pgSQL parameters parsed as columns.** `in_from_date IS NULL` resolved to whatever single
   table was in scope: 190 of 623 raw sites. The routine's parameters and variables are filtered.
3. **Row keys dominated the gap report** (`defaults.setting_key = 'earn_id'`, 49 sites). Primary-key
   and unique columns are row lookups, not codes: 19 gaps → 8.
4. **Evidence validity first measured 0.82** — the checker, not the evidence: a lookup seed row
   names the label, not the referencing column. Fixed in the checker.

## Alternatives considered

- **Synthesis at `compile`** (like RFC 0163 triggers). Rejected: rationale needs the workspace's git
  repository, which `SemanticCompilerPass` deliberately has no access to, and a single place for all
  of it keeps the concept ids and their rationale in one deterministic step.
- **Per-site ledger objects (`PredicateSite`)**. Rejected: thousands of objects for what is a
  property of an existing object (the view or statement it is in); sites ride on their carriers.
- **`Custom("ConstraintCandidate")` vs the built-in `ObjectKind::BusinessRule`.** `BusinessRule`
  is "a constraint or policy" — but a `CHECK` is a database-enforced fact, and the item is a
  *candidate* rule for the semantic layer; a distinct kind keeps the two apart.
- **LLM naming/definitions in the MVP.** Rejected by the plan: Phase 1 must show what deterministic
  traces alone recover before a model is allowed to summarize them.

## Acceptance criteria (Phase 1)

- [x] Predicate, enum-usage, constraint and seed extraction from SQL, normalized, with lines
- [x] Concept / enum / constraint / gap synthesis with evidence; deterministic ids; re-commit writes nothing
- [x] Rationale linker (git blame → commit) behind a trait
- [x] `ekos semantics list|show|gaps` and `ekos export linkml`
- [x] Exported LinkML validates with standard LinkML tooling
- [x] LedgerSMB: ≥ 10 concepts with evidence, gap report produced, first eval numbers published
