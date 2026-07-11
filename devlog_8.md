# Devlog 8 — Phase 12: Enterprise Knowledge Language (EKL)

**Date:** 2026-07-11
**PRs:** —
**Branch:** main

---

## Summary

Verified and closed out Phase 12 — the EKL crate, parser, interpreter, and `ekos ekl` CLI command
were already implemented on disk (RFC 0010, `crates/ekl/`, `crates/cli/src/commands/ekl.rs`) but
`TODO.md` still showed all three Phase 12 items unchecked and no devlog had been written for the
work. This session audited the implementation against RFC 0010 and the TODO validation criteria,
ran the full workspace test suite and clippy, and verified `ekos ekl` end-to-end against a freshly
built ledger (SQL schema → build → recover → resolve → compile → commit → ekl). Everything holds;
ticked the three Phase 12 checkboxes.

---

## Phase 12 — EKL RFC, parser, interpreter, CLI

### Problem / motivation

`ekos ask` (Phase 11) answers natural-language questions through a fuzzy FTS5 retrieval pipeline —
good for exploratory questions, not suitable for deterministic, scriptable queries that dashboards,
CI checks, or other tools need. EKL is the SQL-equivalent query language for the ledger: same query
text + same ledger state always produces the same result set, with no LLM in the loop.

### What was built (found already in place, verified this session)

| Component | Status |
|---|---|
| `docs/rfcs/0010-ekl.md` | Accepted — grammar (EBNF), scope-for-v0, non-goals |
| `crates/ekl/src/parser.rs` | `ekl_parse`, `EklAst`, `Entity`, `Predicate`, `Op`, `Literal`, `Order`, `ParseError` — 13 tests incl. fuzz |
| `crates/ekl/src/interpreter.rs` | `EklInterpreter`, `EklResult`, `EklError`, `default_returns` — 11 tests, one per RFC worked example |
| `crates/cli/src/commands/ekl.rs` | `ekos ekl "<query>"` — table or `--json` output, caret-pointer parse errors |
| `crates/cli/src/bin/ekos.rs` | `Commands::Ekl { query, json }` already wired |
| `TODO.md` | Ticked all 3 Phase 12 items this session |

### Implementation details worth remembering

**EKL v0 is deliberately narrower than the original TODO framing.** RFC 0010 scopes it to flat
predicate queries over `Object`/`Relationship` (`FIND … WHERE … FROM … RETURN … ORDER BY … LIMIT`),
optionally anchored to a named object's immediate neighbourhood via `FROM`. Multi-hop path
expressions (`orders -> customer_id -> customers`) from the original TODO wording are an explicit
non-goal for v0 — the RFC documents this as a narrowing so what shipped is fully specified and
testable rather than partially done.

**RFC numbered 0010, not 0009.** 0009 was already taken by the AI Runtime RFC (devlog 7) by the time
this RFC was written — noted directly in the RFC header to avoid future confusion, since the TODO.md
task list still says "0009 (0008 is taken...)" from when it was originally drafted.

**`EklInterpreter` compiles AST → `Runtime` calls only**, never touches the `Ledger` directly —
same consumer-facing boundary as `AiRuntime` (Phase 11) and `Runtime` itself (RFC 0005). `FROM`
anchoring resolves the named object via `Runtime::find_objects`/`load_neighborhood` and returns
`EklError` for an unknown anchor rather than an empty result set, so a query with a typo'd `FROM`
value fails loudly instead of silently returning zero rows.

**CLI parse errors use a caret pointer** (`eprintln!("{}^", " ".repeat(e.position))`) under the
offending query text — matches the "helpful parse errors (line, column, expected token)" TODO
validation criterion without needing a separate error-formatting crate.

### Decisions

No new architectural decisions this session — this was a verification/close-out pass, not new
design work. The interesting decisions (v0 scope narrowing, RFC numbering, no-tool-use boundary)
were made when the code now on disk was written; captured above from reading RFC 0010 and the source.

---

## Knowledge Captured

- **TODO.md checkbox state can drift behind actual implementation.** Phase 12's crate, RFC, and CLI
  command were fully implemented and tested, but the TODO items were still `[ ]` and no devlog
  existed — work had landed without the corresponding process close-out. Worth checking `git log`
  / crate contents directly against `TODO.md` rather than trusting checkboxes alone when resuming
  work, especially after a session boundary.
- **End-to-end verification command for the whole pipeline** (useful for any future phase's smoke
  test): `ekos init && ekos build && ekos recover && ekos resolve && ekos compile && ekos commit`
  against a directory containing `tests/fixtures/ecommerce.sql`, then query with `ekos ask` or
  `ekos ekl`. `ekos recover` warns and falls back to structural-only analysis when
  `ANTHROPIC_API_KEY` is unset — this is expected, not an error, and EKL/ask both still work off the
  structural KIR.
- **Pre-existing clippy warnings unrelated to Phase 12** noted in `ekos-recovery` (unused import in
  `cache.rs` test) and `ekos-compiler-core` (`nonminimal_bool`/`bool_comparison` in
  `diagnostics.rs` test). Not introduced this session; left as-is since they're out of scope, but
  worth a cheap follow-up cleanup pass.

---

## Files Changed

| File | Change summary |
|---|---|
| `TODO.md` | Ticked Phase 12's 3 items (RFC, Parser, Interpreter) — Phase 12 fully complete |
| `devlog_8.md` | This file |
