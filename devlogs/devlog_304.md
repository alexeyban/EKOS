# Devlog 304 — Step 4: requirements to mart metadata (RFC 0215)

**Date:** 2026-10-26
**PRs:** none (direct to `main`); ekos-lakehouse devlog_18
**Branch:** `main`

---

## Summary

Step 4 of the operator workflow is built. An operator describes a mart in plain words on the console's Requirements
page (or `lakehouse requirements draft --text`). The model routed to the new `requirements` task drafts the mart's
header from an excerpt of the plan, and EKOS resolves every term against the deployed plan. What does not resolve is
a question for the operator, answered by editing the term, never by the model. Approval commits the description and
records a ledger fact; `gold draft` then builds only approved requirements. On iDempiere, the plan's own example was
drafted by local `llama3`, answered in nine edits and approved, and its gold mart was proposed with nothing invented.
The full 849-table Dagster run from RFC 0227 was dropped by the maintainer's decision.

---

## RFC 0215 — requirements to mart metadata

### Problem / motivation
RFC 0195 builds gold from a hand-written YAML header and asks back for what it cannot resolve. The plan's step 4 also
wants plain-language input, an approved spec before building, and a console page. iDempiere added two resolver
problems: upper-case Oracle names, and a dense key graph in which "the customer's name" has several meanings.

### What was built

| Component | Change |
|---|---|
| EKOS `ekos_recovery::Task::Requirements` | the `requirements` route (`[llm.tasks.requirements]`) |
| `gold` resolver | case-insensitive names; `… via t.c[, t2.c2]`; join paths ≤ 4 keys, searched only toward the target; one path per table; similar-name candidates; `= true` |
| `gold::requirements` | `render`, `apply` (edits), `rank`/`choose`/`excerpt`, `from_answer` |
| `lakehouse-ext::requirements` | state dir, draft (text/file), edit, show, approve, `require_approved` |
| `lakehouse requirements draft\|edit\|show\|approve` | classes external / review / read / review |
| `facts::record_requirement_approval` | `LakehouseApproval`, id `mart:<name>` |
| workflow | step 4 from the requirements' states |
| console | the Requirements page |

### Decisions
- **The model never answers a question.** Retrying with feedback is only for an answer that does not parse. Feeding
  the resolver's questions back would let the model pick among candidates, which is the meaning the operator owns.
- **`via` instead of "name a column on the path".** RFC 0195's only answer to "several paths" was to drop the attribute
  for the key column itself. On iDempiere that would make every region, partner or category dimension an id.
- **Coverage before score in the excerpt**, so every word of the request reaches its table.
- **Edits are class review**, like step 5's changes: the console sets the signed-in person only for review commands.

---

## Knowledge Captured

- **llama3 copies format placeholders literally** (`TABLE.C_REGION_ID`) and borrows names from a worked example (a
  dimension called `store` from the prompt's rental example). A concrete example in another domain still beat
  placeholders: one attempt instead of three.
- **Models drop the `---` lines** and list terms without names; reading a header from `mart:` on and naming listed
  terms from their definitions (a label only) avoids pointless retries.
- **A vague parse error makes a model repeat itself**: "expected a mapping of name: definition" three times over. Say
  which section and show an example.
- **iDempiere's key graph is dense**: from `C_ORDERLINE`, 36 key paths reach `C_REGION` within four hops and 19 reach
  `M_PRODUCT_CATEGORY`. Unbounded simple-path enumeration would not finish; pruning by reverse distance to the target
  makes it instant.
- **Some model mistakes resolve fine** (`issotrx is false` for "sales"): no deterministic check can catch them, which
  is why approval is a person's.
- **The console caches the command catalogue at start**: changing a command's class needs a console restart.
- **External commands need the command's name typed** in the console (`type lakehouse requirements draft to confirm`).
- The `requirements` route defaults to the project's `[llm]` provider: on the testbed that was metered Anthropic, and
  `draft --text` refused until `[llm.tasks.requirements] provider = "ollama"`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/llm.rs` | `Task::Requirements` |
| `ekos/crates/kir/src/custom_kinds.rs` | `LakehouseApproval` note: mart requirements |
| `ekos/docs/rfcs/0215-requirements-to-mart-metadata.md` | new RFC with results |
| `ekos/docs/rfcs/0227-silver-from-the-source-model.md`, `TODO.md` | the full run dropped |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | step 4 |
