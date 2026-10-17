# RFC 0195 — EKOS Lakehouse M4: gold marts from written descriptions

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-17
**Builds on:** RFC 0190 (EKOS Lakehouse), RFC 0193 (silver), RFC 0194 (proposals),
RFC 0164 (anti-invention), RFC 0170 (business semantics)
**Code:** `ekos-lakehouse`, crate `gold` (pure) and `lakehouse-ext`

## Goal

A stakeholder describes a mart in words; EKOS turns it into a tested dbt model over silver only
when every term in the description resolves to something that exists, and asks back otherwise.
Exit (RFC 0190): three marts on LedgerSMB, every term resolved or asked back, anti-invention at
100%.

## Design

### 1. The description

`marts/<name>.md` in the pipeline repository: Markdown prose for people, and a YAML header the
generator reads.

```yaml
---
mart: receivables_by_customer_month
grain: [customer, month]
dimensions:
  customer: ar.entity_credit_account
  month: month of transactions.transdate
measures:
  invoiced: sum of ar.amount_bc
  invoices: count of ar
  average_invoice: the invoiced total divided by the number of invoices
filters:
  - transactions.approved is true
---
```

A value is either a **reference** (`table.column`, `table`), a **pattern** over references
(`sum|count|average|min|max of …`, `count distinct …`, `day|week|month|quarter|year of …`,
`<ref> is true|false|null|not null`, `<ref> = | <> | < | <= | > | >= <literal>`), or **prose**.

### 2. Every term resolves, or is asked

A reference resolves when the planned silver model and column exist. A bare word resolves when
exactly one silver column, model, or confirmed RFC 0170 concept is named by it; zero or several
candidates make a **question**, listing the candidates. A mart with any question generates no
SQL — the questions are written to `marts/<name>.questions.md` for the author. "That stop is the
product working, not failing" (RFC 0190).

### 3. SQL: compiled where the header says enough, drafted only for prose

References and patterns compile deterministically: the measures' table is the base; every other
table joins to it along declared single-column foreign keys (a `LEFT JOIN`, so a measure's total
is never cut by a missing dimension row); no path is a question. Only a prose measure or filter
goes to a model (`LlmProvider`; local Ollama in the sandbox, a metered provider only with
consent), which sees the resolved tables, columns and their descriptions, and returns one SQL
expression.

### 4. Anti-invention on the whole statement

The **source atoms** are the resolved facts written as SQL — the tables, the columns (including
join keys), the literals and predicates the description itself states, and the functions its
patterns name. The finished mart, compiled and drafted parts together, is checked with RFC 0164's
`check_candidate_with` (Trino grammar): one unmapped atom — a filter, a constant, a table, a
function nobody wrote — rejects the draft, and the prose term becomes a question naming what was
invented.

### 5. Tests

Grain uniqueness (`ekos_unique_combination` over the dimension columns); per additive measure
(`sum`, `count`), a reconciliation test: the mart's total equals the same aggregate over the base
silver model under the same filters.

### 6. Approval

Every gold mart is proposed (RFC 0190: "every gold mart, and every change to one"): a branch
`ekos/gold/<mart>/<id>` with the model, its YAML, the reconciliation tests and
`proposals/gold-<mart>.md` — the term-resolution table (term → what it resolved to → how),
the anti-invention verdict, and the SQL. The stakeholder and a data engineer approve by merging.

## Found while building it (2026-10-17)

- **Anti-invention is necessary, not sufficient**: a draft `SUM(ar.amount_bc) / COUNT("ar".*)`
  stated nothing the description did not, and Trino refused it (`label.*` only in row-pattern
  counts). Every draft, and the finished mart, is now planned with `EXPLAIN` on the engine; a
  refusal goes back to the model as feedback, like an unmapped atom.
- **The loop reflects**: each rejected draft returns its unmapped atoms or the engine's message
  to the model, up to three attempts; then the term is asked back with the last rejection. With
  `llama3` (local Ollama) `average_invoice` failed three times on `count`; one fact in the
  instructions — "count rows with count(*)" — made the first draft pass. `revenue` passed first time.
- **A description must be committed before drafting**: an uncommitted `marts/<name>.md` in the
  working tree collided with the proposal branch's copy on merge. Drafting now refuses one, and
  the branch carries a description only when it lives outside the repository.
- Group by the dimension expressions, not ordinals: ordinals are literals the description never
  stated. Source atoms name functions as `f(*)`, which states no literal.
- Join conditions on their own lines and reconciliation CTEs indented: the generated gold passes
  the same `sqlfluff` rules as silver.

## Out of scope

Confluence intake, a console form, semantic layers, incremental gold models.
