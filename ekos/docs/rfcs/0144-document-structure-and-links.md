# RFC 0144 — Document structure, section attributes, and deterministic doc links

**Status:** Accepted (per user direction, 2026-09-15 — "make EKOS smarter: decompose source quality, smarter relations, new attributes")
**Author:** EKOS team
**Created:** 2026-09-15
**Builds on:** RFC 0024 (Section objects), RFC 0025 (plain-text/Markdown parsing), RFC 0135 Part C (`KirRelationship::deterministic`), RFC 0094 (post-resolution whole-graph derivations in `SemanticCompilerPass`)
**Measured against:** RFC 0138 eval baseline `devlog_183` (53/101)

---

## Motivation

Sorting the 48 failing scenarios of the devlog_183 eval run by their own `attribution` field gives
**24 retrieval failures** and only 7 generation failures. The retrieval failures are overwhelmingly
questions about *documentation*, not code:

- a specific RFC's Motivation section (hist-011), and which RFC builds on another (hist-012)
- rules stated in CLAUDE.md (sec-002/010, lin-001, lin-011, arch-010, lin-002, arch-020)
- which module owns a named concept (lin-003) — answered in CLAUDE.md prose, with the code object
  named in backticks

(Scenario ids only, never the question text: a design doc quoting eval questions verbatim is
ingested into the very ledger the eval grades — `hist-012` was measured answering from an earlier
draft of this RFC that did exactly that.)

Three concrete, code-confirmed root causes:

1. **Markdown is chunked blindly.** `ekos-plugin-localdocs`'s `TextParser` treats `.md` as plain text
   and cuts it into fixed 2,500-char chunks (`chunk_text`), ignoring headings. A Section object is
   named `docs/rfcs/0001-compiler-core.md: section 1` — nothing in its name says "Motivation", and a
   chunk boundary routinely splits one heading's content across two sections.
2. **More than half of every chunk is unsearchable.** `LocalDocAnalyzerPass` truncates each
   Section's `excerpt` — the only property `KirObject::indexed_content` reads for a Section — to
   `SECTION_EXCERPT_MAX_CHARS = 1200`, while the chunk itself is 2,500 chars. ~1,300 chars of every
   Markdown chunk never reach the BM25 index at all. Evidence points at the whole file, no line range.
3. **Docs and code are disconnected graphs.** A doc sentence naming `` `build_llm_provider` `` or
   "RFC 0015" produces no relationship. Retrieval that lands on the prose can't hop to the code, and
   vice versa.

---

## Design

### 1. Heading-aware Markdown sections (`ekos-plugin-localdocs`)

`TextParser` for `md` (only) uses a new `chunk_markdown(text, budget)`:

- Splits at ATX headings (`#` … `######` followed by a space). Lines inside fenced code blocks
  (```` ``` ```` / `~~~`) are never headings — a Rust `#[derive]` or a shell comment in a code fence
  must not start a section.
- Each section records its **heading** and its **heading path** — the stack of enclosing headings,
  e.g. `["RFC 0016 — Fact segments", "Motivation"]` — plus real 1-indexed `line_start`/`line_end`.
- Text before the first heading becomes a heading-less section.
- A heading's body longer than the budget is sub-chunked by the existing line-boundary logic; every
  sub-chunk keeps the same heading/heading-path, with its own line range.
- `SECTIONS_MAX` still caps the section count.

`DocumentSection` gains `heading: Option<String>`, `heading_path: Vec<String>`,
`line_start: Option<u32>`, `line_end: Option<u32>`. PDF/DOCX/HTML/email/plain-text leave them empty.
The artifact JSON carries them only when present, and the analyzer reads them with
`#[serde(default)]`, so every pre-existing artifact still deserializes unchanged.

Still no Markdown AST (RFC 0025's Alternatives Considered stands): ATX headings + code fences are
the only structure needed, and both are line-local.

### 2. Section attributes (`LocalDocAnalyzerPass`)

- **Name:** `"{path} § {heading_path joined ' › '}"` when a heading path exists (a sub-chunk appends
  ` (part N)`), else unchanged. The name is BM25-indexed separately from content, so
  "RFC 0001 Motivation" now matches by name.
- **Properties:** `heading`, `heading_path`, `line_start`, `line_end`, and on both the Document and its
  Sections: `doc_type` (`rfc` | `devlog` | `readme` | `claude_md` | `doc`, from the path) and, for an
  RFC, `rfc_number` (zero-padded 4-digit string), `rfc_title`, `rfc_status` — parsed from the first
  `# RFC NNNN — Title` line and either the `**Status:** X` or the `| **Status** | X |` header form
  (both exist in this repo).
- **Excerpt cap** raised from 1,200 to 3,000 chars (the observer's own `SECTION_TEXT_MAX_CHARS`), so the
  whole chunk is indexed.
- **Evidence** location becomes `SourceLocation::at(path, line_start)` and the evidence text carries
  `path:line_start-line_end`, so a citation points at the real heading, not just the file.

**Id stability.** A Section id stays `section_kir_id(path, index)`. Because heading-aware splitting
changes how many sections a Markdown file has, index N of a Markdown document will usually describe
different text after this RFC than before. The ledger is append-only and `commit` writes a new
fact version per id, so this is an ordinary content change, not corruption; historical `AS OF`
queries keep the old text. PDF/DOCX/plain text are unaffected.

### 3. Deterministic doc links (`ekos-semantic`, post-resolution)

A new pure function `doc_links(&KirGraph) -> Vec<KirRelationship>` runs inside
`SemanticCompilerPass::run()` right after `concentration_risks` — the same placement and
reasoning as RFC 0094: it needs the whole resolved graph (docs *and* code objects from every
analyzer) and nothing that only `commit` has. Every edge is
`KirRelationship::deterministic(RelationshipKind::References, section, target, "")`, carries the
section's own evidence, and a `link_type` property.

- **RFC → RFC** (`link_type: "rfc"`). Every `RFC NNNN` mention in a Section's excerpt resolves to the
  Document whose `rfc_number` matches. Self-references (a section of RFC 0016 mentioning "RFC 0016")
  are skipped. When the mention sits on a line containing `builds on`, `depends on`, `requires`,
  `supersedes` or `closes`, the edge's `relation` property records it (`depends_on` / `supersedes`);
  otherwise `mentions`. An unknown RFC number produces no edge.
- **Doc → code** (`link_type: "code"`). Every backticked span in a Section's excerpt that is a plain
  identifier or `::`-path (`build_llm_provider`, `ekos_common::redaction`, `ObservationArtifact`) is
  matched by **exact name** against code-kind objects — `RustSymbol`, `RustModule`, `PythonSymbol`,
  `PythonModule`, `JsModule`, `JsSymbol`, `ElixirModule`, `ElixirSymbol`, `Crate`. It links **only
  when exactly one** such object has that name. Ambiguous names (`new`, `run`, `parse`) produce no
  edge — never a guess (RFC 0060). Spans shorter than 4 chars are ignored.
- Edges are de-duplicated per (section, target), sorted for determinism, and capped per section (50)
  so a reference table can't explode the graph.

No new `ObjectKind::Custom` is introduced, so `custom_kinds::REGISTRY` is unchanged.

### 4. Retrieval

No retrieval-code change is required for the core win: the Section *name* now carries the heading
path (name-indexed), and the *excerpt* is no longer truncated. The REASON planner's `Neighborhood` expansion is
relationship-kind-agnostic, so a matched Section reaches its linked code object (and an RFC its
referenced RFCs) through the new `References` edges. The measurement run additionally enables `[embeddings]` (local `nomic-embed-text`) so
descriptive questions get the vector arm RFC 0125 already shipped.

---

## Non-goals

- Doc → **file-path** links (`` `transform_ir.rs` ``). `File` objects are written by `ekos build`
  straight to the ledger and are not in `SemanticCompilerPass`'s graph; linking them would mean a
  post-`commit` step like rollups. Deferred until measured to matter.
- Setext headings (`Title\n=====`) — not used anywhere in this repo.
- LLM-inferred doc relationships. Everything here is deterministic.

## Alternatives considered

- **A full Markdown AST (`markdown-rs`/`comrak`).** More correct for exotic inputs, but a new
  dependency for two line-local rules; RFC 0025 already rejected it for the same reason.
- **Raising only the excerpt cap.** Fixes cause 2 but not 1 or 3; section names would still be
  meaningless and chunk boundaries would still split one heading's content.
- **Fuzzy doc → code matching** (substring, case-insensitive). Rejected: RFC 0060 showed no threshold
  separates correct from incorrect fuzzy matches; an unconfirmed wrong edge poisons impact analysis.

## Testing

- `localdocs`: headings split; `#` inside a code fence is not a heading; heading path nests and pops
  correctly; long bodies sub-chunk with inherited heading; line ranges are exact; `.txt` is unchanged.
- `local_docs_analyzer`: name/property shape; RFC header parsing in both formats; text past 1,200
  chars is searchable via `indexed_content`; old artifacts without the new fields still parse.
- `semantic::doc_links`: RFC→RFC edge and `relation`; self-reference skipped; unique backticked symbol
  links; ambiguous name doesn't; deterministic across runs; per-section cap.
