# Devlog 64 — Fixing what a live Claude+EKOS session actually hit

**Date:** 2026-08-20
**PRs:** (uncommitted at time of writing — see Files Changed)
**Branch:** main (direct)

---

## Summary

A separate Claude Code session ran EKOS live against `plausible/analytics` (the same repo this
whole devlog arc has used) and its transcript
(`/home/legion/PycharmProjects/ekos-session-transcript-2026-08-20.txt`, 5,234 lines) was analyzed
for real problems. Two of three candidate findings were real and got fixed; the third, on closer
verification, turned out not to be a real gap at all — worth recording exactly why, since it's a
direct instance of this project's own "verify claims with real commands, not guesses" discipline
catching a wrong guess before it became wasted work.

---

## Finding 1 — UTF-8 char-boundary panic in `statement_repair.rs`

### Problem / motivation

`starts_with_keyword()` in `ekos/crates/recovery/src/statement_repair.rs` sliced
`&trimmed[..kw.len()]` by byte length with no char-boundary check. Real SQL containing a
multi-byte character (e.g. an em dash in a comment) within a keyword's byte length of the start of
a line panics. The transcript's own session hit this live against real `analytics/` SQL and
already applied the fix in the working tree, uncommitted — just missing a regression test.

### What was built

- Kept the existing one-line fix as-is: `&& trimmed.is_char_boundary(kw.len())` added before the
  slice.
- Added two regression tests: a direct unit test on `starts_with_keyword()` using a crafted
  3-byte UTF-8 character (U+2014 EM DASH) positioned so a keyword-length byte offset falls inside
  it — reproduces the exact panic shape the old code hit; and an end-to-end test through
  `ensure_statement_separators()` with the same character inside a SQL comment mid-statement, to
  prove the fix holds through the actual caller path, not just the isolated helper.

### Decisions

No RFC. This is a one-line defensive bounds check on an existing private helper — not a new
capability, same class as prior no-RFC corrections. A devlog entry (this one) is enough.

---

## Finding 2 — `ekos resolve` hard-stopped on any identity conflict, no override

### Problem / motivation

`ekos/crates/cli/src/commands/resolve.rs` printed proposals/conflicts/stats, then unconditionally
`anyhow::bail!`'d if any conflicts existed — blocking `compile`/`commit` even when conflicts are
diagnostic information a user might reasonably want to see and revisit later, not a mandatory
blocker every time. In the transcript, the analyzed session ran `ekos resolve` against the
**global**, multi-project shared workspace (`/home/legion/PycharmProjects/.ekos/`, many unrelated
repos in one `[observe] paths` list) and hit 230 conflicts — mostly generic short symbol names
(`parser`, `tokenizer`, `op`, `ratelimiter`) colliding across unrelated projects — which hard-
blocked the rest of the pipeline for a large chunk of the transcript.

### What was built

- Added a `--force` flag to `ekos resolve` (`Commands::Resolve { force: bool }` in
  `crates/cli/src/bin/ekos.rs`, matching the existing `Recover { parallel: bool }` pattern).
- `resolve::run(config, cwd, force)` now delegates the bail-or-continue decision to a small, pure
  `check_conflicts(conflict_count, force) -> Result<()>` helper: without `--force`, behavior is
  byte-for-byte unchanged (still bails); with it, conflicts are printed (unchanged) but `run()`
  returns `Ok(())` instead of erroring.
- Extracting `check_conflicts` as a pure function made it trivially unit-testable without needing
  to fake a whole artifact-store round trip — three tests: no-conflicts always `Ok` regardless of
  the flag, conflicts without `--force` bail (and the error text names both the count and
  `--force`), conflicts with `--force` succeed.
- Updated the two existing call sites that predate the signature change
  (`crates/cli/tests/transformation_benchmark.rs`, `tests/integration/tests/integration.rs`) to
  pass `force: false`, preserving their existing behavior exactly.

### Real verification

Ran the freshly built release binary against the **real** global workspace
(`ekos resolve --config /home/legion/PycharmProjects/ekos.toml`, from `/home/legion/PycharmProjects`)
— read-only, `resolve` never writes to disk. Reproduced the exact real conflict set from the
transcript: 230 conflicts, same symbol collisions (`parser`, `tokenizer`, `ratelimiter`, `op`, …).
Without `--force`: exit non-zero, `Error: 230 identity conflict(s) detected...`. With `--force`:
exit 0, conflicts still printed, followed by `230 identity conflict(s) detected — continuing
anyway (--force)`.

### Decisions

Default (no flag) behavior is unchanged — a single-project workspace hitting a real conflict
should still stop and get looked at; `--force` is opt-in for exactly the "generic names collide
across a shared multi-project space, and I want to keep going anyway" case the transcript hit.

---

## Finding 3 — Postgres `CREATE TYPE ... AS ENUM (...)`: investigated, not a real gap

### What was suspected

Earlier investigation (reading `sqlparser` 0.53.0's `parse_create_type()` source directly) seemed
to show no `ENUM` branch — only a composite-type `(` case after `AS`. `analytics/priv/repo/structure.sql`
has 3 real `CREATE TYPE ... AS ENUM (...)` statements (`billing_interval`, `oban_job_state`,
`site_membership_role`), which the transcript's `ekos recover` output showed failing with
`Expected: end of statement, found: ENUM at Line: 37`.

### What real verification found

Before writing an RFC or touching `plugins/sql-dialect-postgres`, added a throwaway probe test
calling `sqlparser::parser::Parser::parse_sql(&PostgreSqlDialect{}, ...)` directly on the real
`billing_interval` statement. **It parsed correctly** — `sqlparser` 0.53.0 does have real ENUM
support (`Statement::CreateType` with a `DataTypeRepresentation::Enum` variant), just not at the
code path the earlier read landed on. The existing `postgres_dialect_parses_the_real_analytics_structure_sql_after_preprocessing`
test — which uses the *entire* real `structure.sql` fixture, ENUM statements included — was
already green before any of this session's changes, which should have been the first thing
checked. Confirmed live end-to-end too: a fresh `ekos recover` against the real `analytics/`
Postgres schema (after clearing the compiler-pass cache) shows `sql-analyzer complete ...
objects=42 relationships=0` for `priv/repo/structure.sql` with no parser warning at all.

### Why the transcript saw the error anyway

Almost certainly the same stale-globally-installed-binary pattern already identified for a CODEC
parser error earlier this session (RFC 0057/0058 fixed that one before this whole session even
started) — the transcript session was very likely running an out-of-date `~/.cargo/bin/ekos`
binary for at least part of its run, not hitting a real gap in the current source.

### Decision

No code change. Reverted the probe test after confirming the result. This is exactly the kind of
thing this whole session's discipline exists to catch — a plausible-sounding, source-read-based
claim that turns out wrong the moment it's actually run.

---

## The dual-ledger confusion — reported, not fixed this pass

The transcript session did real work in `analytics/`'s local `.ekos/` workspace, but the connected
MCP tools were actually querying a *different*, global shared workspace
(`/home/legion/PycharmProjects/.ekos/`, config'd via `/home/legion/PycharmProjects/ekos.toml`'s
`[observe] paths`) — wasting a full local pipeline run, and `ekos_search`/`ekos_ekl` silently
returned empty results with no diagnostic when the project wasn't yet listed in the global config.
The transcript's session self-resolved its immediate blocker by hand-editing the shared config
(`analytics` is now present in `paths` — confirmed, second entry after `memory`), but no code
changed as a result. Scoping a real fix (where does a "not indexed here" hint belong — CLI, MCP
server, or both, and how do you avoid false-hinting on legitimately-empty results?) is a bigger
design question than the two fixes above. Deferred, not chased, this pass.

Also explicitly out of scope, reported only: dbt/Jinja-templated SQL parsing (too large, would
need general templating-engine awareness); the stale globally-installed-binary drift itself (an
operational/deployment gap — reinstall after `cargo build`, not a code bug); `ekos_search` ranking
and `ekos_state`/`ekos_neighborhood` excerpt truncation (already documented in existing decks, not
new).

---

## Knowledge Captured

- **A source-read "confirmed" gap is not confirmed until it's actually run.** The ENUM finding was
  written up with page-and-line citations from reading `sqlparser`'s source directly, and it was
  still wrong — the real fix path exists elsewhere in the crate than where the read landed. Always
  run the exact real input through the exact real, currently-pinned dependency before writing an
  RFC around a "gap," even when the source-reading feels thorough.
- **`check_conflicts`-style extraction pays for itself immediately.** Pulling the bail-or-continue
  branch out of `resolve::run` into a pure `fn(usize, bool) -> Result<()>` made it testable in
  three lines each, with zero need to fake an artifact store — worth doing whenever a CLI command's
  core decision logic is buried inside I/O-heavy setup code.
- **`resolve` is read-only against the artifact store**, confirmed by re-reading the full file:
  it's safe to run directly against the real, shared global multi-project workspace to reproduce a
  reported problem exactly, without any risk of mutating it.
- **The compiler-pass cache-invalidation gap (noted in `devlog_63`) matters for verification, not
  just development** — had to move `analytics/.ekos/artifacts/pass-manifests/` aside again before
  re-running `recover` here, to be sure the "42 objects, no warning" result reflected the current
  binary and not a stale cached pass result.
- **The dual-ledger gap is real and already self-healing in an unsatisfying way**: a human/agent
  hits the "no results" wall, digs, finds the global config, edits it by hand — this works, but it
  means every new project onboarded to the global workspace has to independently rediscover this,
  with a silent empty result the only symptom.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/statement_repair.rs` | Regression tests for the UTF-8 char-boundary fix (fix itself already present, from a different session) |
| `ekos/crates/cli/src/bin/ekos.rs` | `Resolve { force: bool }` CLI flag |
| `ekos/crates/cli/src/commands/resolve.rs` | `run()` takes `force`; `check_conflicts()` pure helper + 3 tests |
| `ekos/crates/cli/tests/transformation_benchmark.rs` | Updated call site for new `resolve::run` signature |
| `tests/integration/tests/integration.rs` | Updated call site for new `resolve::run` signature |
| `devlog_64.md` | This file |
