# RFC 0232 — Business descriptions in the estate docs

**Status:** Proposed
**Date:** 2026-10-27
**Repos:** EKOS (`ora-live`, `plugins/oracle`, `recovery`, `docs-gen`, `cli`, console step 2)
**Depends on:** RFC 0214 (estate docs), RFC 0211/0212 (live Oracle catalog, application dictionary), RFC 0209
(application model from `I_`/`X_` classes), RFC 0163 (procedural IR), RFC 0088 (LLM descriptions), RFC 0170
Phase 3 (cite-or-drop text), RFC 0230 (step 2 in the console)

## 1. Problem

The estate docs (RFC 0214) describe structure well and meaning hardly at all.

- **Tables.** On the iDempiere testbed (`idemsales`), **41 of 876** table pages open with a description. The
  other 835 start at the area/class line and go straight to the column table: names, types, links, counts. A
  description is shown only when the database has a `COMMENT ON` or the application model's class carries one,
  and iDempiere's Oracle schema has almost none of the first.
- **Logic.** A routine page (`acctBalance()`) lists where it is defined, its arguments, what it reads and a
  statement table with conditions such as `(v_AccountSign='N')` and `(v_AccountType IN ('A','E'))`. Nothing says
  what the routine is *for*, or what those branches mean to the business. RFC 0088 already writes an
  `ai_overview` for routines and views, but the estate pages do not show it, and it is a code overview, not a
  business explanation, with no line-level citations.

A reader of step 2's documentation — an analyst deciding scope in step 3, a business owner reading the wiki —
needs the meaning, and today has to read PL/SQL to get it.

## 2. Goals and non-goals

**Goals**

1. Show the description the application's own authors wrote wherever one exists, before any model is asked.
2. Where none exists, add a model-written description to table pages, and a business meaning plus a
   business-terms walkthrough to routine, view and trigger pages.
3. Make every model-written sentence traceable: it cites the facts or source lines it rests on, and a sentence
   that cites nothing valid, or names something its subject does not contain, is dropped.
4. Never let model text pass for author text: it is labelled on the page, ranked last, and stored apart from the
   subject.

**Non-goals**

- Column-level model descriptions (business names from the application model cover most columns; revisit
  after Phase 2's measurements).
- Changing what a routine is *recovered* as: the IR, footprint and fidelity labels are untouched.
- Any model call at `build`/`recover`/`compile`/`commit`. Description is its own opt-in command.
- Translating logic into another engine (RFC 0164) — this is explanation for a reader, not a candidate program.

## 3. Phase 1 — author text from the application dictionary (no model)

iDempiere keeps its authors' text in the dictionary: `AD_Table.Description` / `Help`, `AD_Column.Description` /
`Help`. RFC 0212's `[[databases]] dictionary = "idempiere"` reads only `AD_Table`'s flags today
(`ora-live/src/dictionary.rs`).

- **Read.** With the same opt-in, the dictionary query also reads `AD_Table.Description, Help` and a second
  query reads `AD_Column` (`AD_Table_ID` joined to its table name, `ColumnName`, `Description`, `Help`), both
  inside the existing read-only transaction, named queries so recorded sources answer them. No other dictionary
  table is read. List values and their labels are **not** read here: the generated `X_` classes already carry
  them (`app_model.rs::read_list_values`).
- **Redact.** `ora-live` is a raw-content entry point: the text goes through `ekos_common::redaction` before it
  reaches the catalog artifact, like every other string it reads.
- **Store.** `OracleCatalogPass` puts the text on the live `Table` as `dictionary_description` /
  `dictionary_help`, and on its columns as `columns[].dictionary_description` / `dictionary_help`. The catalog
  pass version and `PIPELINE_LOGIC_VERSION` are bumped.
- **Licence.** Dictionary text belongs to the database source for RFC 0214's licence gate (`[docs-estate]
  licences.<database>`). iDempiere's dictionary content is shipped by its GPL migration scripts; the operator
  states that licence, EKOS does not assume it.
- **Render.** The description a table page shows is the first of:
  1. the database comment (`description` on the `Table`),
  2. the dictionary description, followed by its help text when that adds a sentence,
  3. the application model's description,
  4. the model-written description (Phase 2).

  The same order applies to the area pages' description column and to columns (comment → dictionary → model
  class Javadoc). `ESTATE_VERSION` is bumped.

**Measure:** table pages with a description, before and after, on `idemsales`.

## 4. Phase 2 — model-written table descriptions

### 4.1 Command

`ekos docs describe tables [--scope in|all] [--only <TABLE>…] [--allow-metered] [--dry-run]`

- Default scope `in`: the tables in scope after the step 3 review (all tables when there is no review).
- Skips a table with author text (Phase 1 order 1–3) and counts it `skipped_documented`.
- `--dry-run` prints the number of calls it would make, from the cache state, and calls nothing.
- Model calls go through `LlmProvider` with a new `Task::Describe` (`[llm.tasks.describe]`), wrapped in
  `MeteredLlmProvider` so each call emits `ekos_model_call` for the run log (RFC 0220). A metered provider needs
  `--allow-metered`; Ollama does not.
- Catalogue class **write**, model flag set (`commands/catalogue.rs::CLASSES`), so the console can run it under
  the tenant's `allow_metered` rule.

### 4.2 What the model is given

Numbered facts `F1…Fn`, all from the ledger, all structural or licence-cleared text:

- the table name, its area, its application model class and access level;
- each column: name, type, key/not-null, business name, and its description *only from a source whose licence
  allows text* (the same `licence_allows_text` check the pages use, applied before the prompt is built);
- foreign keys out and in (table and column names);
- list columns with their code → label pairs from the model class;
- the routines, views and triggers that read or write it (names only), and its `WriteBehaviour` classes
  (RFC 0212).

Never: profile values (`min`/`max`), seed rows, row samples. Counts are not needed and are not sent.

### 4.3 What comes back

Strict JSON: `{"sentences": [{"text": "…", "cites": ["F3", "F7"]}]}`, at most three sentences, the first saying
what one row records. A sentence is **dropped** when it cites no fact it was given, uses a hedge (`likely`,
`probably`, `may`, `possibly`), or names a table, column or routine in backticks or upper case that is not among
its facts. Dropped sentences are counted; a table whose every sentence is dropped gets no description and is
counted `empty`.

## 5. Phase 3 — business meaning of routines, views and triggers

### 5.1 Command

`ekos docs describe logic [--scope in|all] [--only <name>…] [--allow-metered] [--dry-run]` — scope `in` means
logic that reads or writes an in-scope table. Same task, catalogue class, metering and dry run as Phase 2.

### 5.2 What the model is given

- **The source**, numbered by the line numbers the page's Statements table shows: for a live routine, the
  catalog's `*_SOURCE` text; for a file routine, the defining file read by recorded span through
  `redaction::read_redacted` (exclusion refusal, then redaction). A view gets its query text the same way.
- **The statement IR**: kind, condition, reads and writes per line (what the page already lists).
- **Its tables**: for each table it reads or writes, the table's description (author text first, else the
  Phase 2 text, marked as model-written), and the business names and list labels of the columns its conditions
  and assignments name.
- **Its callees**: the business meaning of each routine it calls, so logic is described **callees first**
  (topological order over `Calls`; a cycle is broken by name order and the routine is described without the
  cycle's meanings).
- **What fires it**: the triggers whose `function` names it, with their table and timing.

A routine whose prompt exceeds the token budget is **skipped and counted `too_large`**, never truncated: half a
routine described as if it were whole is worse than no description.

### 5.3 What comes back

```json
{"meaning": [{"text": "Returns a GL account's balance, signed the way that account type is read.", "lines": [38, 45]}],
 "steps":   [{"text": "Looks up the account's sign and type in the chart of accounts.", "lines": [39]},
             {"text": "For an asset or expense account with natural sign, the balance is debit minus credit.", "lines": [45, 46, 47]}],
 "rules":   [{"text": "…", "lines": […]}],
 "changes": [{"text": "…", "lines": […]}]}
```

- `meaning`: one or two sentences — what the routine is for.
- `steps`: how it works, in order, in business terms (accounts, orders, invoices — not variables and cursors).
- `rules`: business rules its conditions enforce.
- `changes`: what it changes in the data, in business terms (empty for a pure function or view).

The same cite-or-drop applies, on lines instead of facts: a sentence must cite at least one line that exists in
the routine; a name it uses must be in the routine's source, its footprint, its tables' columns or its callees;
a code value it quotes must appear in the source; hedges drop it. A routine whose `meaning` is entirely dropped
gets no section.

### 5.4 Rendering

On a routine/view/trigger page, after the definition list and before **Statements**:

```markdown
## Business meaning

> Generated by <model> from this routine's source and the compiled model. Not reviewed.

Returns a GL account's balance, signed the way that account type is read. ([38](#l38), [45](#l45))

## How it works

1. Looks up the account's sign and type in the chart of accounts. ([39](#l39))
2. …

**Business rules:** … **Changes:** none.
```

Line references link to the row of the Statements table (rows gain `l<line>` anchors). When the routine has an
author's `COMMENT ON` text, that text is shown first as the description, and the generated section follows under
its label — the generated text never replaces it. Table pages show the Phase 2 description under the same
label. `ESTATE_VERSION` is bumped again.

## 6. Storage

A description is **not** a property of its subject. It is its own object:

- `Custom("BusinessDescription")`, **structurally keyed** by (subject id, part — `table` or `logic`), registered
  in `ekos_kir::custom_kinds::REGISTRY` with `structurally_keyed = true`, so identity never merges two.
- Properties: `subject`, `subject_kind`, `part`, the sentences (`text` + `cites` / `lines`), `dropped`
  (counts by reason), `model`, `prompt_version`, `basis_hash`, `text_sources` (the sources whose text went into
  the prompt, for the licence gate at render), `status: generated`.
- A `Custom("Describes")` relationship to its subject.
- `<state>/descriptions/current.json` lists the ids the latest run of each part wrote; the ledger is append-only,
  so the estate renderer and any reader filter by it (the RFC 0170 `semantics/current.json` pattern).

**Why not RFC 0088's `ai_overview` property on the subject?** Writing a new version of a compiled `Table` or
`Procedure` to add text makes model output part of the subject's own history and of every later read of it; a
separate object keeps the compiled fact untouched, can be listed, dropped from `current.json`, and in Phase 4
reviewed, without touching the subject. RFC 0088's pass stays as it is for modules and symbols.

**Cache.** `basis_hash` covers the prompt version, model, the subject's facts or source and IR, and the
`basis_hash` of every table and callee description the prompt included. A run calls the model only for subjects
whose hash changed; a changed callee re-describes its callers. A second run on an unchanged ledger makes 0 calls.

## 7. Visibility to agents

Descriptions are ordinary ledger objects: searchable, readable through `ekos_state`/EKL, carrying `status:
generated` and the model name in words. `ekos_query`/`ekos_retrieve` claims built from them carry
`generated: true`, so an agent can tell model text from compiled fact. No MCP tool writes or confirms one.

## 8. Phase 4 (sketch, not in this RFC's scope to build)

A step 3 review subject "descriptions": a person confirms, edits or rejects a description; the decision is a
`ReviewDecision` (RFC 0213), human-only. A confirmed description's label becomes "Confirmed by <name>"; an edited
one becomes author text and ranks with Phase 1's. Its own RFC.

## 9. Console

RFC 0230's step 2 page gains two actions, **Describe tables** and **Describe logic**, running the catalogue
commands with a `--dry-run` count shown in the confirmation ("33 calls, local model"). A metered provider is
refused unless the tenant allows metered calls (`POST /cli` already enforces it).

## 10. Tests

- Phase 1: a recorded dictionary source returns descriptions; they land on the live table and columns, redacted
  (a planted secret in `Help` never reaches the artifact); render order comment → dictionary → model → generated;
  a database with licence `unknown` shows none of it.
- Phase 2/3, against a scripted provider:
  - an uncited sentence, a sentence citing `F99`, a hedge, a sentence naming a table the subject does not touch,
    a quoted code value absent from the source — each dropped and counted;
  - text from a source with an unknown licence is never in the prompt;
  - a table with author text makes no call;
  - a second run makes 0 calls; changing a callee's source re-describes it and its caller, nothing else;
  - callees are described before callers; a two-routine cycle terminates;
  - an oversized routine is `too_large`, no call;
  - `current.json` drops a description whose subject left scope;
  - the page shows the label, the author text first, and line links that resolve to Statements anchors.
- The registry guard (`every_pipeline_custom_kind_is_registered`) covers `BusinessDescription`; the catalogue
  test covers `docs describe tables|logic`.
- No test calls a paid model. The live check uses local Ollama.

## 11. Evaluation and exit

On `idemsales` (33-table slice) and its full in-scope set, with local llama3:

| Measure | Reported |
|---|---|
| Table pages with any description, before / Phase 1 / Phase 2 | counts |
| Logic pages with a business meaning | count of 62 routines + 160 views + triggers in scope |
| Sentences dropped, by reason | counts per phase |
| Calls, wall time, second-run calls | must be 0 on the second run |
| Hand check of 20 routine and 20 table descriptions by the maintainer | correct / partly / wrong, each with a note |

Exit: second-run calls 0; no description shown without its label; the hand check's "wrong" rate reported as
measured, not gated — this RFC adds a labelled reading aid, and the number decides whether Phase 4 is needed
before descriptions are shown by default in the console.

## 12. Alternatives considered

- **Generate only, skip the dictionary.** Rejected: the authors' own text is free, correct by definition and
  would be overwritten in readers' minds by model text.
- **Let the model read the whole routine and the whole schema.** Rejected: unbounded prompts, and no way to check
  a sentence against what the model was given.
- **Truncate long routines.** Rejected (§5.2).
- **Show generated text without a label once Phase 4 exists.** Deferred to Phase 4; until a person confirms a
  description, it is labelled.
