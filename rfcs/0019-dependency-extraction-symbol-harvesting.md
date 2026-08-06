# RFC 0019 — Dependency-Fact Extraction + Symbol Harvesting

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-24
**Gating:** none (additive; builds on RFC 0018's `trace_impact`, RFC 0014's
content-excerpt FTS indexing)

---

## Motivation

RFC 0018 built the reasoning *engine* (`Runtime::trace_impact`, `ekos_impact`,
EKL `VIA`), but it can only trace edges the ledger already has.
`RelationshipKind::DependsOn` and `Calls` (`ekos/crates/kir/src/lib.rs`) have
**zero real construction sites** anywhere outside test fixtures today — no
compiler pass has ever emitted one. So "if I replace Postgres with Cosmos DB,
what breaks?" has an engine but no facts to walk: nothing has ever told the
ledger that a given file or service depends on PostgreSQL.

Similarly, "where is authentication implemented?" has no fact to hit beyond a
filename or a 600-char file-opening excerpt (`ekos/plugins/file/src/lib.rs`,
`EXCERPT_MAX_CHARS`) — there is no code-level indexing finer than a file, so a
`fn authenticate_user(...)` buried past the first 600 characters is invisible
to `ekos_search`.

This RFC closes both gaps with the cheapest correct mechanism for each —
explicitly **not** building a full AST parser or call-graph engine (confirmed
absent from the tree today; no `syn`/`tree-sitter` dependency anywhere, and
this RFC adds none).

## Design

### 1. Dependency-fact extraction — new `DependencyAnalyzerPass`

New recovery pass, `ekos/crates/recovery/src/dependency_analyzer.rs`, following
the existing `CompilerPass` pattern (`SqlAnalyzerPass`, `GitAnalyzerPass`,
`CryptoAnalyzerPass`): given a batch of `(file_path, content)` pairs, scans
each file's full content for known technology signatures — connection-string
prefixes (`postgres://`, `mongodb://`, `redis://`, …) and well-known
import/require statements per language (`psycopg2`, `require('pg')`,
`org.postgresql`, …) — and emits, for each match:

- A synthetic **Technology** object: `ObjectKind::Custom("Technology")`,
  named after the detected technology (e.g. `"PostgreSQL"`), **deduplicated
  by name** via a deterministic id (`Uuid::new_v5` on `"technology:{name}"`)
  so the same technology detected across many files — or across repeated
  `ekos recover` runs — always resolves to the same object.
- A `RelationshipKind::DependsOn` edge from the file object (using the same
  deterministic id scheme `build.rs` already uses for file objects,
  `Uuid::new_v5(NAMESPACE_URL, rel_path)`) to the Technology object.
- `Evidence`: the actual matched line, with a `SourceLocation::file(path)` —
  every dependency claim traceable, per EKOS's evidence invariant.

Pattern matching is **plain substring matching, case-insensitive where
appropriate** — not a new regex dependency. The patterns are literal strings
(`"postgres://"`, `"psycopg2"`, …), so `str::contains` after lowercasing is
sufficient and keeps this RFC's dependency footprint at zero new crates.
Explicitly **not** a general import-statement parser: it will miss aliased or
obfuscated imports, and that's an acceptable, documented v1 limitation — it
answers "what obviously depends on X" cheaply, not "prove no dependency
exists."

Initial pattern table covers PostgreSQL, MySQL, MongoDB, Redis, and Kafka,
across Python/JavaScript/Java-style import idioms — enough to make the
Postgres→Cosmos DB example real, and enough breadth to prove the pattern
generalizes to more technologies later without an engine change (adding a
technology is adding a row to a table, not new code).

`recover.rs` gains a new file walk (alongside the existing SQL-file walk)
collecting source files by extension (`.py`, `.js`, `.ts`, `.java`, `.go`,
`.rb`) and their full content, registering one `DependencyAnalyzerPass` per
`ekos recover` invocation over the whole batch (mirrors `GitAnalyzerPass`
and `CryptoAnalyzerPass`'s "one pass, many inputs" shape, not
`SqlAnalyzerPass`'s "one pass per file" shape — needed here specifically so
Technology-object dedup happens within a single pass's local map before
`append_object`'s content-addressed idempotency takes over across runs).

### 2. Symbol harvesting — extends the existing excerpt mechanism

No new pass. `ekos/plugins/file/src/lib.rs`'s `FileObserver` already reads a
file's full byte content before truncating it to a 600-char excerpt
(`text_excerpt`). It gains a second, parallel scan of that same full content:
`harvest_symbols`, a plain-text scanner (again, no regex dependency) that
looks for lines starting with a small set of declaration keywords (`fn `,
`def `, `class `, `func `, `interface `) after trimming leading whitespace,
and extracts the identifier that follows — capped at 50 symbols per file to
bound output size. The harvested names ride on the observation artifact as
`data["symbols"]`, alongside the existing `excerpt`.

`build.rs`'s file-to-`KirObject` promotion picks up `symbols` (if present)
into a new `"symbols"` property, the same way it already does for
`"excerpt"`.

**Both ledger backends' FTS indexing (RFC 0014's `object_fts` in `Ledger`,
and `FactLedger`'s tantivy `search.upsert`) are extended to feed `excerpt`
**and** `symbols` (joined by spaces) into the indexed content column** — this
is the one small, necessary touch to existing indexing code (two spots in
`ledger/src/lib.rs`, one in `ledger/src/fact_ledger.rs`), each a one-line
change concatenating an additional property lookup. This is the direct,
low-effort answer to "where is authentication implemented": `ekos_search
"authenticate"` now hits real declaration sites via the existing tantivy/FTS5
search, no new engine work. Explicitly **not** a call graph — a symbol name
appearing in the index says nothing about who calls it or how; that's
documented future scope if this proves insufficient.

## Alternatives Considered

- **Full AST parsing (`syn`, `tree-sitter`) for both dependency extraction and
  symbol harvesting** — rejected for this RFC. Confirmed absent from the tree
  today; adding either is a large, multi-language undertaking disproportionate
  to what a pattern/regex-based v1 needs to prove ("the fact-generation gap
  can be closed without an engine change"). Worth revisiting in a future RFC
  if pattern-matching's false-negative rate proves too high in practice.
- **A general import-statement parser instead of a fixed pattern table** —
  rejected; a fixed, documented table of (pattern → technology) rows is
  transparent, easy to extend, and matches this RFC's "cheap, honest v1"
  framing better than a parser that still can't handle every language's
  import syntax anyway.
- **Storing full file content instead of an excerpt + symbol list** — rejected
  (unchanged from RFC 0014's original decision); bounding indexed content size
  per object still matters at estate scale, and symbol *names* — not full
  bodies — are what search needs.

## Testing

- Dependency extraction: known import/connection-string patterns across
  Python, JavaScript, and Java-style syntax each produce the expected
  `DependsOn` edge and (deduplicated) Technology object; a file with no
  recognized pattern produces neither; the same technology detected in two
  different files resolves to the same Technology object id.
- Symbol harvesting: a file with `fn authenticate_user(...)` (and similarly
  `def`, `class`, `func`, `interface` declarations) yields the expected
  symbol name; a file with no declarations yields an empty list; output is
  capped at the documented maximum.
- FTS: an object whose `symbols` property contains `"authenticate_user"` (but
  whose `excerpt` does not mention it) is findable via `ekos_search
  "authenticate"` in both ledger backends.

## Acceptance Criteria

- [ ] `DependencyAnalyzerPass` detects known technology signatures across at
      least 3 language idioms and emits deduplicated `DependsOn` edges with
      evidence.
- [ ] `FileObserver` harvests declaration-line symbols alongside the existing
      excerpt, bounded and additive (no change to existing excerpt behavior).
- [ ] Both ledger backends index `symbols` into searchable content.
- [ ] Zero new crate dependencies.
