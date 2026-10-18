# RFC 0205 — Model-drafted SQL for Pentaho code steps the translator cannot follow

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0200 (umbrella), RFC 0203 (code steps), RFC 0204 (translation), RFC 0197
(LLM task routing, metering, budget), RFC 0207 (`--allow-metered` needs the tenant's permission)
**Code:** `ekos-lakehouse` — `pentaho-dbt/src/draft.rs`, `kettle-rel` (`Expr::Sql`, drafts),
`lakehouse-ext` (`pentaho translate --allow-metered`)

## Goal

When a code step (JavaScript, Java class, Java expression or filter, a decompiled plugin) is a gap,
let a model propose SQL for it — and accept only what can be checked.

## Which steps

A step is offered to the model only when the deterministic translator left it a gap, it is a code
step (or a plugin with decompiled code), it has one input whose fields and types are known, and the
outputs it must produce are declared (the step's own field list). Everything else stays a gap:
a model cannot turn a file reader or a loop over row metadata into a row-local expression, and is
not asked to.

## The exchange

Task `translate` of the RFC 0197 router (`[llm.tasks.translate]`), so the route, fallbacks, cache,
usage log and budget are the router's. The prompt gives the step's original text, its input fields
with types, the outputs it must produce, Kettle's semantic rules (RFC 0202) and asks for strict
JSON:

```json
{"columns": {"<output>": "<Trino SQL expression over the input columns>"},
 "keep_when": "<Trino SQL predicate, or null when every row is kept>",
 "cannot": "<why it cannot be expressed per row, or null>"}
```

## The checks (all must pass)

1. Every expression and the predicate parse as a Trino expression.
2. **Anti-invention:** every identifier is an input field; every function is on the translator's
   list of Trino functions; every string literal, and every number other than 0, 1, 100 and 1000,
   appears in the step's original text.
3. Every declared output has an expression; nothing else is set.

A failed check is fed back to the model with the findings, up to three attempts (RFC 0195's
loop). What survives becomes a Project (plus a Filter for `keep_when`) labelled **llm-drafted**,
with the model's name in the notes; the whole model is then planned by Trino like any other
(RFC 0204), and refused models are disabled. A `cannot` answer leaves the gap and adds the model's
reason to it.

## Cost and consent

An Ollama route runs without asking. Any other route needs `--allow-metered`; without it the
command says how many steps a model could draft, through which model, and drafts none. From the
console, `--allow-metered` is refused unless the tenant allows metered calls (RFC 0207). Every
call is metered and logged by the router; the budget (`[llm] budget-usd`) stops the run.

## Non-goals

Drafting set-level logic (joins, aggregates), whole transformations, or anything the model would
have to invent from outside the step.
