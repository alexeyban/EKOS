# Devlog 6 — Phase 10 close-out: Runtime text search

**Date:** 2026-07-08
**PRs:** —
**Branch:** main

---

## Summary

Closed out the last open item in Phase 10 (Runtime): exposed the ledger's FTS5 object index
through `Runtime::find_objects`, so Phase 11's AI ask-pipeline has a retrieval path that goes
through the Runtime rather than the Ledger directly. While wiring it up, found and fixed a
real bug in `Ledger::find_objects`: any query containing FTS5 syntax characters (notably `-`)
threw a SQLite error instead of returning results — including the exact query
(`"zzz-nonexistent"`) named in Phase 10's own validation spec in `TODO.md`. Phase 10 is now
fully complete; all TODO items checked.

---

## PR — Runtime::find_objects + FTS5 query-escaping fix

### Problem / motivation

`Ledger::find_objects` (with FTS5 backing) already existed from a prior session, and the CLI's
`ekos query find` called it directly. But the TODO spec for Phase 10 required `Runtime::find_objects`
specifically — Phase 11's AI Runtime is speced to call `Runtime::find_objects`, not the ledger,
keeping the Runtime as the single consumer-facing API (RFC 0005). Without it, Phase 11 has no
legal way to do retrieval.

### What was built

| Component | Change |
|---|---|
| `crates/runtime/src/lib.rs` | Added `Runtime::find_objects(query) -> Vec<(KirId, String)>`, thin delegate to `Ledger::find_objects` |
| `crates/ledger/src/lib.rs` | Fixed `find_objects` to escape queries containing FTS5 operator characters into a literal quoted phrase |
| `crates/cli/src/commands/query.rs` | `ekos query find` now goes through `Runtime::find_objects` instead of `Ledger::find_objects` directly |
| Tests | 2 new runtime tests, 1 new ledger regression test |

### Implementation details worth remembering

`Ledger::find_objects` builds the FTS5 `MATCH` expression conditionally: if the query is only
alphanumerics, spaces, or `*` (a normal prefix query like `order*`), it's passed through
unescaped so prefix search keeps working. Otherwise the whole query is wrapped in double quotes
(with internal `"` doubled) to force FTS5 to treat it as a literal phrase instead of parsing `-`,
`:`, etc. as query syntax.

### Decisions

Considered catching the SQLite error and returning an empty result instead of pre-escaping the
query. Rejected: that would silently swallow genuinely malformed queries too, and it wouldn't let
literal phrase searches (e.g. searching for a hyphenated object name) actually match — escaping
is strictly more correct and keeps `find_objects` behaving like a real text search.

---

## Knowledge Captured

- **FTS5 treats `-` as a NOT operator, `:`as a column filter, and `"` as phrase delimiters.**
  Any free-text search surface backed by SQLite FTS5 needs to either escape user input into a
  quoted phrase or explicitly document that raw MATCH syntax is exposed to callers. This bit us
  immediately: the exact validation example in `TODO.md` Phase 10 (`find_objects("zzz-nonexistent")`
  returns an empty list) would have thrown a SQL error in production had it shipped as originally
  written, because nobody had exercised a query with a hyphen in it.
- **Phase 10 is now fully closed.** All ledger/runtime/CLI query surfaces (`object`, `find`,
  `neighbourhood`) go through `Runtime`, not `Ledger`, from the CLI layer down — this is the
  contract Phase 11's `AiRuntime` will build on.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/runtime/src/lib.rs` | Added `find_objects()` + 2 tests |
| `crates/ledger/src/lib.rs` | Fixed FTS5 query escaping + 1 regression test |
| `crates/cli/src/commands/query.rs` | `find` now calls `Runtime::find_objects` |
| `TODO.md` | Ticked final Phase 10 item; Phase 10 fully complete |
