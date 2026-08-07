# RFC 0035 — Generated Documentation from the Compiled Ledger

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

EKOS already reads repos, GitHub, Pentaho, SQL, and more, and compiles them into an evidence-backed
KIR graph — the compiler already does the hard part. What's missing is rendering that compiled
knowledge as human-readable documentation: Markdown, static HTML, and diagrams a person (or an
onboarding contributor, or an auditor) can open without going through an MCP client. This is
explicitly a *rendering* problem, not an *extraction* one — nothing new needs to be recovered from
source, only presented.

Three pieces of the codebase already prove this pattern works, just not wired into a bulk
generation command:

- `Runtime::list_objects()` / `list_relationships()` (`ekos/crates/runtime/src/lib.rs:229,234`)
  already provide the "walk the whole graph" primitive a doc generator needs.
- `AiRuntime::ask` (`ekos/crates/runtime/src/ai.rs:98`) is already a grounded-generation pipeline:
  retrieve → expand neighborhood → reconstruct full state (object + relationships + evidence as
  JSON) → prompt an LLM → validate every citation against real evidence ids before returning it.
  Directly reusable for an LLM-prose upgrade tier with the same anti-hallucination guarantee.
- `ekos_transformation_explain` (RFC 0028) already generates a structured, evidence-backed
  human-readable explanation of one legacy pipeline — proof this pattern works, currently scoped
  to one object type and served as an MCP response rather than a file.

`KirObject`'s `ObjectKind` and `RelationshipKind` (`ekos/crates/kir/src/lib.rs:81,129` —
`ForeignKey`, `Calls`, `Extends`, `DependsOn`, `OwnedBy`, `Contains`, `References`, `CoupledWith`,
`Unknown`, `Custom(String)`) are the exact vocabulary diagram generation renders from — no new
data needed, purely a rendering pass over relationships already compiled. `docs/` in this repo is
itself a working example of the target output mode (static HTML site, dark neon/glass theme).

## Scope

- Markdown, static HTML, and Mermaid diagrams, generated from already-committed ledger objects and
  relationships, for any workspace EKOS has compiled — source-agnostic, since GitHub, Pentaho, SQL,
  etc. all already normalize into the same KIR graph this reads.
- A deterministic (zero-LLM) rendering tier that is always available, plus an opt-in LLM-prose
  upgrade tier.

## Non-goals

- A new extraction/recovery pass — this reads what's already compiled, extracts nothing new.
- Replacing `docs/`'s hand-written presentation decks — those are marketing artifacts for *this*
  project; generated docs are a feature *for users'* workspaces.
- LLM-prose as the default. It ships opt-in, after the deterministic tier, matching the project's
  "Proven, not promised" posture — a generated doc set must be useful with no API key at all.

## What already exists and is reused as-is

- `Runtime::list_objects()`/`list_relationships()` — the ledger-walk primitive.
- `AiRuntime::ask`'s grounding + citation-validation pipeline (`extract_citations` checked against
  `known_evidence`) — reused unchanged for the LLM-prose tier.
- `ekos_transformation_explain`'s pattern of structured, evidence-cited explanation — proof of
  concept for what a generated page's content should look like.
- `docs/assets/theme.css`'s visual pattern — reused for HTML output's look, not its content.
- The "post-commit CLI verb, not a `PassManager` stage" precedent from `ekos identity scan`
  (RFC 0029) and (planned) `ekos treasury scan` (RFC 0032) — same reasoning applies: this needs
  already-committed ledger state, not a pre-ledger `KirGraph`.
- `ekos_diff`'s "what changed since T" query — reused for incremental regeneration.

## Design

### New CLI command: `ekos docs generate`

`crates/cli/src/commands/docs.rs` (new), backed by a new `crates/docs-gen/` crate housing the
templating/rendering logic — kept separate from `cli` the same way `marketing` is its own crate.
Reads the committed ledger via `Runtime::list_objects()`/`list_relationships()`, the same
"post-commit CLI verb" shape as `ekos identity scan`.

### Deterministic tier (default, zero LLM, zero cost)

One page per "significant" object — module/file/table/pipeline-level, not per-`Column` — templated
directly from its compiled properties, relationships, and evidence. Pure rendering, no
interpretation, so it is always available with no API key, matching how repowise's
`init --no-prose` ships a complete, useful doc set with zero LLM spend.

### Diagrams

A `RelationshipKind → Mermaid edge` mapping renders three diagram families straight from existing
relationship data, no new extraction required:

| Diagram | Source relationships | Object kinds |
|---|---|---|
| Dependency graph | `References` / `DependsOn` / `CoupledWith` | any |
| ER diagram | `ForeignKey` | `Table` |
| Transformation DAG | Transformation IR chain (RFC 0027) | `Custom("TransformNode")` |

### HTML output

A static site per generation run, visually modeled on `docs/`'s existing dark neon/glass theme
(reusing the CSS pattern, not the specific marketing content) — an index page listing every
generated page, mirroring `docs/presentations.html`'s list pattern.

### LLM-prose tier (opt-in, v2)

Reuses `AiRuntime::ask`'s exact grounding+citation-validation pipeline: pass the deterministic
page's structure as context, ask for readable prose, validate every citation against real evidence
ids exactly as `ask` does today. A generated page can never carry a citation that doesn't trace to
real evidence. A token-cost estimate is shown and confirmed before any LLM spend, matching the same
transparency principle repowise's dashboard applies to its own LLM-upgrade path.

### Incremental regeneration

Reuses `ekos_diff`'s "what changed since T" query so `ekos docs generate` only re-renders pages for
objects that actually changed, not the whole doc set every run.

## Alternatives Considered

- **`PassManager` stage instead of a CLI command.** Rejected — needs already-committed ledger
  state, not a pre-ledger `KirGraph`, the same reasoning RFC 0029/0032 give for their own dedicated
  CLI verbs.
- **LLM-first generation.** Rejected as inconsistent with the project's evidence-first ethos; the
  deterministic tier ships first and stays free/always-available regardless of LLM configuration.
- **One page per symbol/column instead of per module/table.** Rejected as almost certainly too
  granular for human navigation at real workspace scale — mirrors repowise's own "one page per
  module and file," not per-symbol, choice.

## Open Questions

- [ ] Markdown templating approach (a lightweight crate like `tera` vs. hand-rolled) — an
      implementation detail, not an architectural one. (Phase 1/2 used hand-rolled `String`
      building — sufficient so far; revisit if page complexity grows in the HTML/diagram phases.)
- [x] **Resolved (Phase 2, devlog_34):** default object-kind granularity for "gets its own page"
      is every `ObjectKind` except `Column` — validated against a real recovered Northwind SQL
      schema (13 `Table` objects) plus real `File` objects from the same workspace's git
      observation, rendered together with a shared `index.md`. `Column` stays embedded in its
      parent `Table`'s properties rather than a page of its own; every other kind, including
      `Unknown`/`Custom(_)`, gets one, so no compiled fact is silently hidden by an allowlist.
- [ ] Output location — a user-specified output dir (proposed default) vs. writing into `.ekos/` —
      leaning user-specified, since generated docs are a deliverable *for* the user, not
      EKOS-internal state. (Phase 1/2 implemented the proposed default: `<workspace>/docs-generated`
      unless `--output` is given.)
- [ ] Mermaid diagram size/readability cap for graphs with hundreds of nodes — Phase 3 scoped the
      dependency-graph diagram to a 1-hop neighborhood per object specifically to sidestep this at
      the per-page level; the *whole-workspace* ER diagram has no such cap yet and would need one
      for a workspace with hundreds of `Table` objects — not solved by this RFC, deferred to a
      later phase once a real large workspace surfaces the actual threshold.

## Testing

- Golden-file tests for deterministic Markdown/HTML output against a fixture ledger.
- Mermaid output validated as syntactically parseable.
- LLM-tier citation-validation test: inject a fixture LLM response containing a fabricated
  evidence id, assert it's rejected/flagged — mirrors `ai.rs`'s existing citation-validation
  behavior for `ask`.
- Incremental-regen test: change one object, assert only its page (and pages of directly dependent
  objects, if diagrams are shared) is re-rendered.

## Acceptance Criteria

- [ ] All Open Questions resolved.
- [ ] At least one review completed.
- [ ] `ekos docs generate --no-prose` (deterministic tier) runs end-to-end against a fixture
      workspace with zero LLM calls and produces valid Markdown + HTML + Mermaid output.
- [ ] LLM-prose tier's citation-validation test passes.
- [ ] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants.

---

## Implementation Plan

**Phase 1 — Deterministic Markdown, one object kind. DONE (devlog_34).** `ekos docs generate`
renders one Markdown page per `Table` object (properties + relationships grouped by kind +
evidence citations) into a user-specified output dir (default `<workspace>/docs-generated`).
Files: `crates/cli/src/commands/docs.rs` (new), `crates/docs-gen/` (new). 8 unit tests +
3 CLI-command tests; verified end-to-end against a real recovered Northwind SQL schema (13 tables),
not just unit-seeded fixtures — which caught a real gap (relationship targets rendering as raw
ids) fixed in the same phase.

**Phase 2 — Generalize to every significant object kind + an index page. DONE (devlog_34).**
`render_table_page` → `render_object_page`, generic over `ObjectKind` via `is_significant`
(every kind except `Column`); kind-prefixed file names (`table-orders.md`, `file-main-rs.md`)
so same-named objects of different kinds never collide; new `render_index_page` groups every
generated page by kind, written as `index.md` on every run (including an honest empty-state
message when the ledger has nothing yet). Resolved the "default granularity" Open Question
empirically: rendered real `File` objects (from this same workspace's git observation) alongside
the 13 real `Table` objects together, not just Tables in isolation. 13 unit tests + 5 CLI-command
tests.

**Phase 3 — Diagrams. DONE (devlog_34).** One generic `render_mermaid_graph` (`graph TD`) covers
two of the three planned diagram families: each object's page now embeds a 1-hop dependency
diagram under a new `## Diagram` section, and — because Transformation IR nodes are `KirObject`s
connected by `Custom("FeedsInto")` relationships like any other object — the transformation DAG
falls out of the exact same renderer for free when centered on a `TransformNode`, no duplicate
diagram logic needed. Only the ER diagram genuinely needed different Mermaid syntax
(`erDiagram`), so `render_er_diagram` is a second, dedicated function — a whole-workspace diagram
of every `ForeignKey` edge between `Table` objects, written as `er-diagram.md` and linked from a
new `## Diagrams` section at the top of `index.md`. `CoupledWith` edges render dashed (`-.->`) to
visually distinguish a derived/statistical signal from a hard dependency. Validated against real
data: the Northwind fixture was extended with a real `CREATE VIEW ... JOIN ... JOIN` to produce
real `FeedsInto`-chained `TransformNode`s (not available from the `CREATE TABLE`-only schema
Phases 1–2 used), confirming the transformation-DAG-for-free claim against an actual recovered
join chain, not a hypothetical one. 11 new unit tests (24 total) + 3 new CLI-command tests
(7 total).

**Phase 4 — Static HTML site output. DONE (devlog_34).** Refactored page assembly into a
format-agnostic `ObjectPageModel` (`build_object_page_model`), with `render_markdown_object_page`
and the new `render_html_object_page` each rendering the same model — exactly the shared-model
design this RFC specified, verified by a test asserting `render_object_page`'s Markdown output is
byte-identical whether produced directly or via the model. HTML pages are fully self-contained
(embedded CSS inspired by, not built-time-coupled to, `docs/assets/theme.css` — `ekos docs
generate` runs in arbitrary user workspaces without this repo's files available, so a literal
`include_str!` of this repo's theme file was rejected as a fragile dependency). `--format md`
(default) / `--format html` on `ekos docs generate`; `render_html_index_page` and
`render_html_er_diagram_page` mirror the Markdown index/ER-diagram renderers. All object-derived
text is HTML-escaped (name, property values, relationship labels, evidence fragments) since it
originates from arbitrary source/SQL/document content. The Mermaid diagram is shown as its raw
source in a `<pre>` block rather than live-rendered — rendering would need bundling or CDN-loading
`mermaid.js`, which conflicts with this generator's zero-external-dependency, fully-offline design
goal; stated as an honest limit, not fixed silently. 12 new unit tests (33 total, one library-side
regression test proving Markdown output didn't change) + 2 new CLI-command tests (9 total).

**Phase 5 — LLM-prose upgrade tier. DONE (devlog_34).** `ekos docs generate --prose` (opt-in,
`--yes` skips confirmation) layers an LLM-written "## Overview" onto each `ObjectPageModel` by
calling `AiRuntime::ask` — the exact same pipeline `ekos ask` uses, not a reimplementation, so a
fabricated citation is structurally impossible (proven by a real test: a bogus evidence id mixed
into a mock LLM response never survives into `ProseSection.cited_evidence`). A rough per-page
token-cost estimate (from the model's own compiled content, ~4 chars/token) is shown and must be
confirmed before any spend, mirroring `ekos marketing publish`'s existing Y/N approval pattern
(RFC 0030). No API key configured → a clear error, not a silent degraded mode — `--prose` is
explicitly opt-in, so a user who asked for it gets real output or an honest failure, never mock
placeholder text. One real, load-bearing finding from actually running this against a live LLM
(not assumed from reading `ai.rs`'s source): `ask`'s retrieval step is keyword/name search, so a
full instruction sentence as the "question" buries the object's name deep enough that retrieval
can return zero matches — which empties `ask`'s citation-validation set and silently drops *every*
citation, including genuinely valid ones. Fixed by passing just the object's name as the question,
matching `ask`'s own tested usage elsewhere in the codebase. 5 new unit tests (38 total) + 5 new
CLI-command tests (14 total, including one exercising a real `MockLlmProvider`-backed `AiRuntime`
end to end). Verified against a real local Ollama model (`llama3:latest`, not just mocks)
generating real prose for the real recovered Northwind workspace: 22/22 objects got a real
Overview, 16/22 with real, citation-validated evidence ids; the other 6 degraded honestly — full
answer kept, empty citation list — exactly per `ask`'s existing "answer is never discarded"
contract, when the model's response didn't include a parseable citation block. Small-model
citation compliance is real-world variable, not a defect in this RFC's validation logic — the
validation itself (never letting through a fabricated id) held on every one of the 22 real calls.

**Phase 6 — Incremental regeneration.** `ekos_diff`-based "only re-render what changed" — last,
since it's a pure optimization over an already-correct full-regenerate path from Phases 1–5.

Each phase ships with its own tests before the next starts, matching `CLAUDE.md`'s mandatory
Tests-before-Implementation workflow discipline.
