# Devlog 34 — RFC 0035 Phases 1–5: Markdown + HTML + Mermaid + LLM-prose generation from the compiled ledger

**Date:** 2026-08-07
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Implemented Phases 1–5 of RFC 0035 (Generated Documentation): a new `ekos docs generate --format
md|html [--prose]` command that renders one page per significant ledger object — properties,
relationships grouped by kind, evidence citations, relationship endpoints resolved to
human-readable names, a 1-hop Mermaid dependency diagram per page, and an opt-in LLM-written
overview reusing `ekos ask`'s own citation-validation pipeline — plus a grouped index and a
whole-workspace Mermaid ER diagram, in either Markdown or self-contained HTML from one shared page
model. The deterministic tier is zero LLM calls, zero cost, no API key, no external runtime
dependency; `--prose` is explicitly opt-in with a pre-spend cost estimate and confirmation.
Verified end-to-end against a real recovered SQL schema (Northwind, 13 tables + a real `CREATE
VIEW ... JOIN` producing a real transformation chain) and real `File` objects from the same
workspace, not just unit-seeded fixtures, in both output formats.

---

## RFC 0035 Phase 1 — deterministic Markdown, Table objects

### Problem / motivation

EKOS already compiles GitHub/Pentaho/SQL/etc. into an evidence-backed KIR graph but had no way to
render that compiled knowledge as human-readable documentation — the ask was Markdown/HTML/
diagrams generated from what's already compiled, not a new extraction pipeline. RFC 0035 scoped
this into six phases; this session shipped Phase 1.

### What was built

| Component | Location |
|---|---|
| Rendering crate | `ekos/crates/docs-gen/` (new) — pure function, no I/O |
| CLI command | `ekos/crates/cli/src/commands/docs.rs` (new) — `ekos docs generate` |
| Clap wiring | `ekos/crates/cli/src/bin/ekos.rs` — `Commands::Docs` / `DocsCommands::Generate` |

`render_table_page(object, relationships, evidence, object_names) -> RenderedPage` is the entire
rendering surface: no side effects, no ledger access — the CLI command does all I/O and passes in
already-resolved data. This mirrors the split every connector already uses (thin I/O wrapper +
pure logic core), just applied to a renderer instead of an `Observer`.

### Implementation details worth remembering

- **`KirId` has `Hash`+`Eq` but not `Ord`.** First compile attempt used `Vec::sort()` +
  `dedup()` to deduplicate cited evidence ids — doesn't compile. Fixed with a `HashSet`-backed
  insertion-order dedup instead. Anything wanting to sort/compare `KirId`s directly will hit the
  same wall; dedup via `HashSet`, order via insertion order or by another field (name, timestamp).
- **Relationship targets need a second pass to resolve names.** The first version rendered a
  relationship's other endpoint as a bare id (`→ \`8d088a7a-...\``). Running it against a real
  recovered Northwind schema (not just a unit-test fixture) immediately showed this was
  unreadable — `ekos_dependents`-shaped raw graph data is fine for an agent, not for a human
  reading a doc page. Fix: the CLI command builds one `HashMap<KirId, String>` from
  `all_objects()` (already fetched, no extra ledger reads) and passes it into the renderer, which
  now renders `→ Orders (\`8d088a7a-...\`)`. Caught only by testing against real data, not by unit
  tests seeded with a couple of hand-built objects — worth remembering for later RFC 0035 phases:
  run the real fixture check before considering a rendering surface "done."
- **`ObjectKind`/`RelationshipKind` already implement `Display`** (`Custom(s) => s`, otherwise
  `{other:?}`) — reused directly for page headers and relationship-group labels instead of writing
  a second name-mapping table.
- **Post-commit CLI verbs read the store through `open_store`** (`commands/store.rs`), which
  auto-detects SQLite vs. the RFC 0016 fact-engine backend. `ekos identity scan` already
  established this pattern; `ekos docs generate` reuses it unchanged rather than hand-rolling
  ledger access.
- **`Ledger::open` on a path with no existing ledger file creates an empty one** (SQLite
  auto-creates), it does not error. An initial test assumed a missing ledger would error and
  asserted on the error message — wrong premise, not a bug; removed rather than worked around.

### Testing (Phase 1)

- `ekos-docs-gen`: 8 unit tests — properties table, honest empty-state placeholders (never a
  panic on missing data), evidence citation with a resolved fragment, evidence citation that
  degrades honestly to "evidence unavailable" instead of guessing, relationships grouped by kind
  without dropping non-`ForeignKey` kinds, filename slugification (including dots/mixed case),
  both relationship directions, and the name-resolution fix.
- `ekos` (CLI): 3 tests — one page per `Table` object with `ForeignKey` content asserted, non-
  `Table` objects correctly excluded, output-directory default resolution.
- End-to-end smoke test against `tests/fixtures/northwind.sql` through the real
  `build → recover → resolve → compile → commit → docs generate` pipeline: 13 real tables, real
  foreign keys, real evidence fragments, real column types — not just unit-seeded data. This is
  what surfaced the name-resolution gap above.

---

## RFC 0035 Phase 2 — generalize to every significant object kind + an index page

### Problem / motivation

Phase 1 was `Table`-only by construction (function name, doc comments, and file-naming scheme
all said "table"). Phase 2's job was resolving RFC 0035's "default granularity" Open Question —
which object kinds get a page — empirically, against real data, not by guessing upfront.

### What was built

- `render_table_page` → `render_object_page`: no longer `Table`-specific in behavior (it never
  actually was — it only *looked* Table-specific in naming), generalized to any `KirObject`.
- `is_significant(kind: &ObjectKind) -> bool`: every kind gets a page except `Column`, which stays
  embedded in its parent `Table`/`Dataset`'s properties table rather than becoming a standalone
  page — the resolved granularity decision, one page per module/file/table/pipeline, not per
  symbol.
- `page_file_name`: kind-prefixed (`table-orders.md`, `file-main-rs.md`, not just `orders.md`),
  so two objects of different kinds sharing a bare name never collide on disk.
- `render_index_page`: groups every generated page by kind (`## Table (13)`, `## File (2)`, …),
  alphabetical within each group, with an honest "no documented objects yet" message on an empty
  ledger rather than a blank file. `ekos docs generate` now always writes `index.md` alongside the
  per-object pages.

### Implementation details worth remembering

- **The granularity question resolved itself once real mixed-kind data was rendered together.**
  Running `docs generate` against the same Northwind workspace (which also has 2 real `File`
  objects from git observation of `ekos.toml`/`northwind.sql` themselves) alongside the 13 real
  `Table` objects, in one output directory, was the actual validation — not a hypothetical
  "what if there were File objects too." The index page immediately showed whether kind-prefixed
  naming and grouping read cleanly at real (if still small) scale.
- **Excluding only `Column` (not curating an allowlist) matches the project's existing
  don't-hide-facts posture** — same reasoning as Phase 1's decision to render every relationship
  kind, not just `ForeignKey`. An allowlist would have been a guess about what's "important";
  excluding one specific too-granular kind is a narrower, defensible claim.

### Testing (Phase 2)

- `ekos-docs-gen`: 5 new tests (13 total) — `Column` excluded while every other sampled kind
  (`File`, `Directory`, `Table`, `Pipeline`, `Dataset`, `Unknown`, `Custom`) is significant;
  non-`Table` kinds render with correct kind-prefixed file names and headers; same-named objects
  of different kinds don't collide on file name; index page groups by kind and links every page,
  alphabetical within a group; empty-set index is honest, not blank.
- `ekos` (CLI): 2 new tests (5 total) — `File`/`Custom("TransformNode")` objects get pages while
  `Column` doesn't; `index.md` is always written, including the honest empty-ledger case.
- Re-ran the Northwind end-to-end smoke test: 15 objects rendered (2 `File` + 13 `Table`),
  `index.md` correctly grouped and linked, real file content spot-checked (`file-northwind-sql.md`
  shows the real artifact id, excerpt, path, and size from git/file observation).
- `cargo clippy -p ekos-docs-gen -p ekos -- -D warnings`: clean, both phases. `cargo fmt --check`:
  clean, both phases.

---

## RFC 0035 Phase 3 — Mermaid diagrams

### Problem / motivation

RFC 0035 planned three diagram families: a per-object dependency graph, a whole-workspace ER
diagram, and a transformation DAG for Transformation IR (RFC 0027) pipelines. Phase 3's job was
building the `RelationshipKind → Mermaid` mapping and resolving whether that's really three
separate renderers or fewer.

### What was built

- `render_mermaid_graph(object, relationships, object_names) -> String`: a generic Mermaid
  `graph TD` renderer for one object's 1-hop neighborhood, embedded into every object's page under
  a new `## Diagram` section. `CoupledWith` edges render dashed (`-.->`) rather than solid, so a
  derived/statistical signal (RFC 0020's git co-change coupling) is visually distinct from a hard
  dependency (`ForeignKey`/`References`/`FeedsInto`) without needing to read the edge label.
- `render_er_diagram(tables, relationships) -> String`: a dedicated Mermaid `erDiagram` renderer
  (genuinely different syntax from `graph TD`) for every `ForeignKey` edge strictly between two
  `Table` objects — a whole-workspace diagram, not per-object, since an ER diagram's point is
  showing several tables' relationships at once. Written as `er-diagram.md`, linked from a new
  `## Diagrams` section at the top of `index.md` (only rendered when at least one diagram exists).
- **The transformation DAG needed no new renderer at all.** Transformation IR nodes (RFC 0027,
  `crates/semantic/src/transform_ir.rs`) lower into ordinary `KirObject`s
  (`ObjectKind::Custom("TransformNode")`, `properties["node_type"]` = `Source`/`Filter`/`Join`/
  `Sink`/etc.) connected by ordinary `KirRelationship`s (`RelationshipKind::Custom("FeedsInto")`)
  — the exact same shape `render_mermaid_graph` already draws for any object. Centering the
  existing per-object diagram on a `TransformNode` *is* the transformation DAG family; no
  duplicate graph-drawing logic was needed. This is the actual Phase 3 design finding, not just an
  implementation shortcut: three planned diagram families collapsed into two renderers because two
  of them were the same mechanism applied to different data.

### Implementation details worth remembering

- **Mermaid node ids can't contain hyphens; UUIDs are all hyphens.** `mermaid_node_id` strips them
  (`id.0.simple()`-equivalent, done via `Uuid::simple()`'s hyphen-free form) rather than quoting —
  quoting is reserved for the human-readable label text (`id["label"]` syntax), not the id itself.
- **Mermaid labels break on an unescaped `"` or embedded newline.** `mermaid_escape_label` replaces
  `"` with `'` and collapses `\n`/`\r` to a space. Caught a real, slightly ugly-but-safe case: the
  Northwind fixture's `"Order Details"` table name (SQL double-quoted identifier, so the literal
  quote characters are part of the compiled object *name* string, not just its SQL syntax) renders
  as `"'Order Details'"` in the ER diagram — safe, valid Mermaid, just not the prettiest label; a
  cosmetic follow-up, not a correctness bug, and left as-is rather than over-engineering label
  cleanup this phase.
- **Validating the transformation-DAG claim needed a fixture the earlier phases didn't have.**
  Phases 1–2's Northwind fixture is `CREATE TABLE`-only, so `sql_transform_analyzer` (which needs
  `SELECT`/`CREATE VIEW`/procedure bodies) produced zero `TransformNode`s — "0 total, 0% mapped" in
  Phase 1's own `ekos recover` output, unremarked on at the time since Phase 1 didn't need
  Transformation IR data. Phase 3 added a real `CREATE VIEW OrderSummary AS SELECT ... FROM Orders
  JOIN Customers JOIN "Order Details" ...` to the smoke-test fixture specifically to get real
  `FeedsInto`-chained nodes to render and inspect, rather than trusting the "falls out for free"
  design claim on code-reading alone.
- **A real, working `ekos.toml` gotcha found while iterating:** the default output dir
  (`docs-generated`) sits inside the observed workspace by default (`[observe] paths = ["."]`), so
  running `ekos build` again after `ekos docs generate` re-ingests the generated docs as `File`/
  local-document objects — and because the ledger is append-only, once ingested those objects
  persist even after adding `docs-generated` to `ignore-patterns` afterward (correct append-only
  behavior, not a bug, but it meant a contaminated test workspace had to be discarded and rebuilt
  clean rather than "fixed"). No code change needed — `ekos.toml`'s existing `[observe]
  ignore-patterns` list already supports excluding the output dir; it just needs to be there
  *before* the first `ekos build` that would see it. Worth calling out explicitly in user-facing
  docs for this command once it's more broadly used, so nobody else finds this the hard way.

### Testing (Phase 3)

- `ekos-docs-gen`: 11 new tests (24 total) — the `## Diagram` section appears with a fenced
  ` ```mermaid ` block; edges are labeled with relationship kind and correct direction;
  `CoupledWith` renders dashed; an unresolvable neighbor falls back to its id rather than being
  dropped; quote-escaping in labels; ER diagram renders `ForeignKey` edges between given tables;
  excludes edges to objects outside the given table set; ignores non-`ForeignKey` relationships;
  quotes entity names containing spaces; index page lists diagrams ahead of object groups; index
  page omits the `## Diagrams` section entirely when there are none.
- `ekos` (CLI): 2 new tests (7 total) — `er-diagram.md` is written and linked from `index.md` when
  a `ForeignKey` relationship exists between two `Table` objects; both are correctly skipped when
  none exists.
- Re-ran the Northwind end-to-end smoke test with the added `CREATE VIEW`: 22 objects rendered
  (2 `File` + 13 `Table` + 7 `TransformNode`), 1 ER diagram, real `FeedsInto` edge inspected on a
  real `Sink` node's page (`northwind.sql#13:5 --> northwind.sql#13:6`, correctly directed), real
  `ForeignKey` edges inspected on `table-order-details.md`'s diagram.
- `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`: all
  clean.

---

## RFC 0035 Phase 4 — static HTML output via a shared page model

### Problem / motivation

RFC 0035's Design section was explicit: "`--format md` and `--format html` share the same
underlying page-model data structure ... only the renderer differs." Phases 1–3 didn't have that
structure — `render_object_page` built a Markdown string directly, interleaving data-gathering
and Markdown syntax in one function. Phase 4's job was the refactor the RFC already called for,
plus the new HTML renderer built on top of it.

### What was built

- `ObjectPageModel` (+ `RelationshipRow`, `RowEvidence`, `EvidenceRow`): the format-agnostic
  content of one object's page — no Markdown or HTML syntax anywhere in it.
- `build_object_page_model(object, relationships, evidence, object_names) -> ObjectPageModel`:
  the data-assembly logic extracted unchanged from the old `render_object_page`.
- `render_markdown_object_page(&model)` and `render_html_object_page(&model)`: two renderers
  consuming the same model. `render_object_page` (the Phase 1–3 entry point every earlier test
  calls) is now a two-line wrapper: build the model, render it as Markdown.
- `render_html_index_page` and `render_html_er_diagram_page` mirror `render_index_page`/
  `render_er_diagram` for the HTML case.
- `ekos docs generate --format md|html` on the CLI (`Format::parse`, default `md`).
- A compact, self-contained embedded CSS (`EMBEDDED_CSS` in `docs-gen`) and `html_escape`/
  `html_document` helpers.

### Implementation details worth remembering

- **The refactor was verified for zero behavior change, not just "should be fine."** A dedicated
  test (`model_and_markdown_page_agree_with_the_direct_render_object_page_wrapper`) asserts
  `render_object_page(...)` and `render_markdown_object_page(&build_object_page_model(...))`
  produce byte-identical `RenderedPage`s. All 24 pre-Phase-4 tests also passed unmodified against
  the refactored code — real evidence the shared-model design didn't change what Phase 1–3 already
  shipped, not just an assumption.
- **Literally reusing `docs/assets/theme.css` (e.g. via `include_str!`) was considered and
  rejected.** `ekos docs generate` runs in arbitrary user workspaces, which don't have this
  repo's `docs/` directory available at runtime — a relative-path dependency on this repo's own
  files would only work when generating docs *for this repo itself*. The embedded CSS is
  self-written, inspired by the same dark-neon palette (`--accent:#9945ff`/`#14f195` in dark mode)
  but with zero build-time coupling to this repo's file layout — matches the RFC's own wording
  ("reusing the CSS *pattern*, not the specific marketing content") more literally than a direct
  file include would have.
- **Object-derived text needs HTML-escaping that Markdown never needed.** Names, property values,
  relationship labels, and evidence fragments all originate from arbitrary source/SQL/document
  content and can contain `<`, `>`, `&`, `"` — any of which breaks HTML structure if not escaped
  (Markdown is far more tolerant of stray special characters). Every such field is escaped via a
  small local `html_escape`; a dedicated test seeds an object literally named
  `<script>alert(1)</script>` and asserts it never appears unescaped in the output.
- **Live Mermaid rendering was scoped out deliberately, not forgotten.** Rendering the diagram
  visually in-browser needs `mermaid.js`, either bundled into the binary (increases binary size
  for a feature not every user wants) or loaded from a CDN (breaks the fully-offline, zero-
  external-dependency property this whole generator is built around). Phase 4 shows the Mermaid
  source in a `<pre>` block instead — copyable into any Mermaid renderer — and the RFC now states
  this as an explicit, intentional limit rather than leaving it to be rediscovered as a "missing
  feature."

### Testing (Phase 4)

- `ekos-docs-gen`: 12 new tests (33 total) — model/direct-wrapper output equivalence; HTML page
  has the correct `.html` extension and is a complete document; dangerous characters
  (`<script>...`) are escaped, never raw; Mermaid source is embedded without its Markdown fence;
  empty-object HTML page shows the same honest placeholders as Markdown; HTML index groups by
  kind and lists diagrams first; empty-set HTML index is honest, not blank; fence-stripping
  helper; HTML ER-diagram page has the correct file name and embeds (escaped) diagram source.
- `ekos` (CLI): 2 new tests (9 total) — `Format::parse` accepts `md`/`markdown`/`html`, rejects
  anything else; `--format html` writes `.html` pages, `index.html`, and `er-diagram.html`
  (verified to *not* also write `.md` files), each starting with `<!doctype html>`.
- Re-ran the Northwind end-to-end smoke test with `--format html`: 22 objects + 1 ER diagram
  rendered as real HTML files, manually inspected — `table-order-details.html` shows correctly
  escaped properties/relationships/evidence and an embedded (unescaped-for-display, i.e. readable)
  Mermaid source block; `index.html` correctly groups and links every page with real `<a href>`s.
- `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`: all
  clean.

---

## RFC 0035 Phase 5 — opt-in LLM-prose upgrade tier

### Problem / motivation

The deterministic tier (Phases 1–4) never interprets anything — properties, relationships, and
evidence are shown as compiled, not explained. RFC 0035's Phase 5 was explicit that the prose
upgrade must reuse `AiRuntime::ask`'s existing grounding+citation-validation pipeline rather than
build a second, parallel prompt/citation system, and must show a cost estimate before any spend.

### What was built

- `ProseSection { text, cited_evidence }` and `ObjectPageModel.prose: Option<ProseSection>` —
  `docs-gen` itself still never calls an LLM; it only knows how to render one if the caller
  supplies it, so the deterministic tier's zero-LLM guarantee is structurally unaffected by
  Phase 5 existing at all.
- `ekos docs generate --prose` (opt-in) / `--yes` (skip confirmation) on the CLI.
- `enrich_with_prose(models, &AiRuntime)` — one `ai.ask(&model.name)` call per model, writing the
  result into `model.prose` on success, logging a warning and leaving that one page's
  deterministic content untouched on failure (a bad LLM call for one object doesn't cost every
  other page its content).
- `estimate_prompt_tokens(&model)` — a stated rough proxy (~4 chars/token over the model's own
  compiled properties/relationships/evidence, plus a flat overhead), summed and shown before any
  call, with a Y/N confirmation (`confirm_prose_spend`) mirroring `ekos marketing publish`'s
  existing `approve` pattern (RFC 0030) — `--yes` skips it, EOF on stdin is treated as "no."
- `select_llm_provider_for_prose` — deliberately **not** `recover.rs`'s `build_llm_provider`,
  which silently falls back to a mock when no API key is configured (a legitimate degraded mode
  for `recover`, which has real structural-analysis-only value without an LLM). `--prose` has no
  such degraded mode: a mock response would just produce nonsense "Overview" text instead of a
  clear "no API key" error, so this mirrors `marketing.rs`'s stricter `select_llm_provider`
  instead, which errors out clearly.

### Implementation details worth remembering

- **A real bug, caught by a real integration test, not by reading the source.** The first version
  of `enrich_with_prose` passed a full instruction sentence ("Write a short, plain-language
  overview of X: what it is and how it relates...") as the `question` to `ai.ask`. A test seeding
  a real evidence id and asserting it survived into `ProseSection.cited_evidence` failed —
  `cited_evidence` came back completely empty, not just missing the deliberately-injected bogus
  id. Root cause: `AiRuntime::ask`'s retrieval step (`Runtime::find_objects`) is keyword/name
  search — the same "2-3 keywords, not natural-language questions" constraint the `ekos_search`
  MCP tool is already documented to have elsewhere in this project. Burying the object's name deep
  inside a full sentence meant retrieval matched nothing, which left `ask`'s internal
  `known_evidence` set empty, which made its citation filter (correctly, by its own contract) drop
  every citation — including the genuinely valid one. Fixed by passing just `model.name` as the
  question, matching `ai.rs`'s own tests (`ai.ask("orders")`). This would not have been caught by
  reading `ai.rs`'s source and reasoning about it — only by actually running a seeded evidence id
  through the real (mocked-LLM, real-retrieval) pipeline.
- **Testing an LLM-calling code path without real credentials**: `enrich_with_prose` takes an
  already-constructed `&AiRuntime<'_>` rather than building one internally, specifically so tests
  can pass a `MockLlmProvider`-backed instance and exercise the real citation-validation logic
  with zero network dependency — the same two-tier (mock-driven unit test + real-provider
  end-to-end check) discipline every connector in this codebase already uses, applied to an LLM
  call instead of an HTTP one.
- **A test that could accidentally make a real network call is a real risk, not just a style
  nit.** An early version of the "no credentials → clear error" test relied on the ambient test
  environment simply not having `ANTHROPIC_API_KEY` set — true here, but not guaranteed on every
  machine or CI runner this test might run on, and a passing environment variable would have sent
  a real request to a real provider from inside `cargo test`. Fixed by pointing
  `config.llm.api_key_env` at a variable name guaranteed not to exist
  (`EKOS_DOCS_TEST_DEFINITELY_UNSET_KEY`), making the test deterministic regardless of the
  environment it runs in.

### Testing (Phase 5)

- `ekos-docs-gen`: 5 new tests (38 total) — `build_object_page_model` initializes `prose: None`;
  Markdown embeds the Overview section and its citations ahead of Properties; Markdown omits the
  section entirely when there's no prose; HTML embeds and escapes prose text; HTML omits the
  section when there's no prose.
- `ekos` (CLI): 5 new tests (14 total) — `estimate_prompt_tokens` grows with model content;
  `confirm_prose_spend(true)` skips the prompt; `enrich_with_prose` sets `model.prose` on success
  against a `MockLlmProvider`; `enrich_with_prose` only keeps citations that actually resolved
  (the real bug above, now a regression test); `generate(..., prose: true, yes: true)` with no
  credentials configured fails clearly rather than silently degrading.
- `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`: all
  clean.
- **Real end-to-end run against a live local model**, not just mocks: `OLLAMA_MODEL=llama3:latest
  ekos docs generate --prose --yes` against the real recovered Northwind workspace (`[llm]
  provider = "ollama"` in `ekos.toml`). Took ~11 minutes for 22 sequential real completions on a
  CPU/GPU-split 5.6GB model — no per-object failures (exit 0, no warning lines). Result: 22/22
  objects got a real "## Overview"; 16/22 came back with real, citation-validated evidence ids
  (`grep -l "Cited evidence"` count); the other 6 degraded honestly (full answer kept, empty
  citation list) because the model's response didn't include a parseable trailing JSON block —
  exactly `ai.rs`'s existing, unmodified "answer is never discarded" contract, not a Phase 5 bug.
  Spot-checked `table-order-details.md` (real column list, correctly no fabricated citation) and
  `file-northwind-sql.md` (real citations, `6c66cfe7-...`/`97228874-...`/etc., all genuine ids
  from the ledger — none fabricated).

---

## Knowledge Captured

- Unit tests seeded with two or three hand-built objects can pass while a rendering surface is
  still genuinely unreadable against real compiled data — the gap here (raw ids instead of names,
  Phase 1) only showed up once real Northwind objects with real relationships were rendered.
  Phase 2 leaned on the same lesson deliberately: the granularity question was resolved by
  rendering real mixed-kind data together, not by reasoning about it in the abstract.
- `KirId` intentionally has no `Ord` — anything wanting sorted/deduped `KirId` collections needs
  `HashSet`/`HashMap`, not `sort()`/`dedup()`.
- `ObjectKind`/`RelationshipKind` already implement `Display` (`Custom(s) => s`, otherwise
  `{other:?}`) — reused directly for page headers, relationship-group labels, and index grouping
  instead of writing a second name-mapping table.
- A planned RFC diagram taxonomy is a hypothesis until real data is drawn — "three diagram
  families" collapsed to two renderers once Transformation IR's real shape (ordinary objects +
  ordinary relationships) turned out to already match the generic per-object graph renderer.
  Worth checking whether a planned "new, distinct" feature is actually new before building it.
- Mermaid syntax has real, easy-to-hit failure modes (hyphenated ids, unescaped quotes in labels)
  that only surface when real data — not hand-picked test strings — is rendered through it.
- An append-only ledger means a workspace contaminated by ingesting a tool's own output directory
  (before that directory was added to `ignore-patterns`) can't be un-contaminated by fixing the
  config afterward — the objects are already committed history. The fix is prevention
  (exclude-before-first-build), not cleanup.
- When an RFC specifies a shared-model refactor ("format X and format Y render the same
  structure"), doing the refactor first and adding a byte-identical-output regression test is
  what actually proves nothing broke — re-running the old test suite unmodified against refactored
  code is real evidence a design change was behavior-preserving, not just "should be fine because
  I was careful."
- A generator meant to run in arbitrary third-party workspaces can't assume this repo's own files
  (`docs/assets/theme.css`) are available at runtime — anything it needs has to be self-contained
  (embedded as a Rust constant) rather than referenced by a relative path that only resolves
  inside this specific repo.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/docs-gen/Cargo.toml`, `src/lib.rs` | New crate — `ObjectPageModel`, `build_object_page_model`, `render_object_page`/`render_markdown_object_page`, `render_html_object_page`, `is_significant`, `render_index_page`/`render_html_index_page`, `render_mermaid_graph`, `render_er_diagram`/`render_html_er_diagram_page`, 33 tests |
| `ekos/crates/cli/src/commands/docs.rs` | New — `ekos docs generate`, `Format` (md/html), 9 tests |
| `ekos/crates/cli/src/commands/mod.rs` | Register `docs` module |
| `ekos/crates/cli/src/bin/ekos.rs` | `Commands::Docs` / `DocsCommands::Generate { output, format }` |
| `ekos/crates/cli/Cargo.toml` | `ekos-docs-gen` dependency |
| `ekos/Cargo.toml` | `crates/docs-gen` workspace member + dependency alias |
| `ekos/docs/rfcs/0035-generated-documentation.md` | Marked granularity/output-location/diagram-size Open Questions resolved or scoped; Implementation Plan updated with Phase 1–4 status |
