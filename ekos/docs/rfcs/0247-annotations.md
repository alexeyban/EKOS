# RFC 0247 — Annotations: comments, instructions and synonyms on any ledger entity

**Status:** Accepted (2026-11-07)
**Date:** 2026-11-07
**Depends on:** RFC 0213 (digest-bound sign-off), RFC 0172 (conflicts), RFC 0170 (business semantics), RFC 0186
(catalogue classes), RFC 0181 (read scope)
**Plan:** `ekos-demo-public/docs/PLAN.md`, "Annotations in EKOS core"

---

## 1. Motivation

People know things the sources do not say: "our fiscal year starts in April", "this column is in cents", "use
`c_bpartner` joined through `c_invoice`, never `c_order`". Today there is nowhere to put that. RFC 0170 captures
*derived* meanings for review, RFC 0172 records disagreements, and a conflict decision has no place for its rationale
except a free-text note on the decision. The public demo (plan, step 8) needs the same thing for any entity: a user's
comment, a rule that changes later answers, a business synonym.

## 2. Goals and non-goals

**Goals**

1. Attach a record to **any ledger entity** (table, column, code symbol, transformation, mart, term, dashboard,
   chart) or to the project.
2. Three kinds: `comment` (for people), `instruction` (a machine-applicable rule, or free text passed to a model
   as *data*), `synonym` (business word → entity).
3. Bind each record to the **digest of the entity it describes**; when the digest changes the record is `stale` and
   needs confirmation (the RFC 0213 mechanism).
4. Used by: term resolution, documentation generation, an MCP read tool, an annotations panel on entity pages.
5. Bulk resolution of review items **by rule**, recorded as an instruction (needed to close step 3: 195 conflicts).

**Non-goals**

- A model never writes an annotation. Only an explicit human action does (same rule as `review_guard::HUMAN_ONLY`).
- No free-form edits of compiled facts; an annotation sits beside them and never changes what was observed.
- Not a wiki. No rich text, no attachments.

## 3. Design

### 3.1 Record

A keyed `Custom("Annotation")` object (registered in `custom_kinds::REGISTRY`, `structurally_keyed: true`), key =
(target id, kind, author, text hash). Properties: `kind`, `target` (object id or `project`), `text`, `rule`
(structured, instructions only), `author`, `created`, `target_digest`, `scope` (`personal` | `shared`), `status`
(`active` | `stale` | `withdrawn`), `approved_by`/`approved_at` for shared instructions and synonyms.

Withdrawal is a new fact with `status: withdrawn` (the ledger stays append-only); readers take the newest per key.

### 3.2 Instruction rules

Machine-applicable rules are a closed vocabulary, each checked against the entity it targets: `default_filter`
(column predicate), `preferred_join` (an ordered path of declared joins), `unit` (a column's unit/scale),
`fiscal_year_start` (month), `exclude` (rows/values to leave out). A rule that does not type-check against its
target is refused at write time. Free-text instructions are never applied as logic: they reach a model only inside
a delimited data block and are shown to the user as such.

### 3.3 Visibility and approval

`comment`s are visible to everyone with read access at once. `instruction`s and `synonym`s apply to their author's
own requests at once and to everyone only after approval (an owner/reviewer action, RFC 0213 style: recorded with
who and the target digest). Read scope (RFC 0181) applies: an annotation on an entity a reader cannot see is not
returned.

### 3.4 Staleness

On every read the record's `target_digest` is compared with the entity's current digest (the digest function RFC 0213
uses per subject; for a single entity, a hash of its compiled properties). A mismatch yields `stale`; a stale
instruction is not applied until confirmed again. This is computed on read, never written by a compile.

### 3.5 Surfaces

- CLI: `ekos annotate add|list|show|withdraw|approve|confirm` (catalogue classes: `add`/`withdraw` = write,
  `approve`/`confirm` = review, `list`/`show` = read). `ekos annotate resolve --rule <file>` bulk-decides review
  items by rule and writes the rule as an instruction.
- MCP: read-only `ekos_annotations` (in `SCOPED_TOOLS`). No write tool exists; `review_guard::HUMAN_ONLY` lists the
  write entry points and a test scans `commands/mcp.rs` for them.
- Console: an annotations panel on every entity page; the Review page uses `resolve`.
- Term resolution and docs generation read through one function, `annotations::applicable(entity, requester)`, so
  the "which rule applied" explanation shown to a user is the same data that changed the answer.

## 4. Decisions

| Decision | Alternative rejected | Why |
|---|---|---|
| Annotation is its own object kind | Properties on the target | Targets are immutable compiled facts; an annotation has an author, a lifecycle and a different digest |
| Closed rule vocabulary | Arbitrary SQL fragments | Rules must be checkable and must not reopen SQL injection (the demo's "users never write SQL") |
| Staleness computed on read | Written by compile | Matches RFC 0213: validity is never stored |
| Withdrawal as a new fact | Delete | The ledger is append-only |

## 5. Tests

Round trip per kind; a rule that fails to type-check is refused; digest change → stale, not applied; withdrawn is
hidden; read scope hides annotations on hidden entities; `HUMAN_ONLY` scan (no MCP write); a reversed-input test
(results independent of ledger order); bulk `resolve` decides exactly the items the rule matches and no others.

## 6. Open questions

1. Granularity of the entity digest for a column (the column alone, or the table's definition)?
2. Should a `synonym` shared by approval be able to override a built-in term, or only add?
3. Retention of personal annotations of a banned user in a public deployment (see RFC 0249).
