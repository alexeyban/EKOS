# Devlog 67 — Storage default flipped to v3, and a stale memory record corrected

**Date:** 2026-08-21
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

A roadmap ask for "a storage v2→v3 deprecation timeline" turned into something better once
investigated: RFC 0016's fact-segment engine (v3) is not design-only — it's complete, tested, and
has been running on a real multi-project estate for a month. Flipped the default for brand-new
workspaces instead of writing a timeline for a migration that (per an outdated assumption) didn't
seem to exist yet to migrate to. Verified live, end to end, with a genuinely fresh workspace.

---

## The wrong starting assumption, and how it got caught

Going in, working memory (a persisted note from 2026-07-17) said "RFC 0016 engine still
design-only." Before planning anything around that, checked directly: `tantivy`/`memmap2` are real
workspace dependencies, `crates/ledger/src/fact_ledger.rs` is 1,445 lines with real tests,
`crates/ledger/src/segment/` is a real module, and `crates/cli/src/commands/store.rs` already has
a complete, working `KnowledgeStore`-backed auto-detection layer choosing between SQLite and the
fact engine per workspace. The memory record was simply stale — real progress had shipped after it
was written. Worth restating: **a memory record naming specific code is a claim about what existed
when it was written, not a fact about now** — this is exactly why it got verified against the
actual source before any plan was built on top of it, and why the plan changed shape entirely once
the real state was known.

## What RFC 0016 itself already said to do

RFC 0016's own dated acceptance section (`§7 measured outcome and gate status`, 2026-07-17) states
the exact condition for flipping the default: *"Fresh workspaces keep the SQLite default until the
engine has soaked on the live estate."* Its `## Phasing` section lists the remaining, not-yet-done
work as one line: *"6. Migration + flip: migrate --v3, acceptance gate, default switch."* Migration
and the acceptance gate were already done. Only the flip remained.

**Real soak evidence, not a guess**: `/home/legion/PycharmProjects/.ekos/` — a real, actively-used
multi-project estate — has been running on the fact engine since 2026-07-17 (the same date as the
RFC's own measurement), with 16 sealed segments spanning roughly 350,000 real transactions, and its
`HEAD` marker was still being touched as recently as 2026-08-20, four days before this work. A full
month of continuous real use. That's the condition the RFC itself set, satisfied.

## The actual code change: small, because the hard part was already done

`crates/cli/src/commands/store.rs`'s `open_store()` is the one call site every command opens the
knowledge store through. Before: if a workspace had no `facts/manifest.json`, it always opened
SQLite. Now: it opens SQLite only if a SQLite `ledger.db` *already exists* — a genuinely fresh
workspace (neither file exists yet) opens (creates) a `FactLedger` instead, since
`FactLedger::open`'s own doc comment already promises "open (or create)," the same semantics
`Ledger::open` has for SQLite. Any pre-existing SQLite workspace — this repo's own `.ekos/`,
`analytics/`, or anyone else's — is completely unaffected: it keeps serving from SQLite forever
unless explicitly migrated via `ekos ledger migrate --v3` (unchanged, still the only migration
path, still reversible).

One real subtlety found while testing: `facts/manifest.json` is written **lazily** by the fact
engine (confirmed by reading `segment/mod.rs::load_manifest` — it returns an in-memory default
without touching disk when the file is absent), while `segments/` is created immediately on open.
A test asserting `uses_fact_engine()` becomes true right after one `open_store()` call with nothing
written failed on the first attempt — correctly, since nothing had actually been written yet. Fixed
the test to check for the `segments/` directory instead, which `SegmentStore::open` does create
immediately, regardless of whether anything's been written.

## Verified live, end to end

Not just unit tests. Created a genuinely fresh, disposable workspace (`/tmp/.../v3-default-test`,
two real small `.md` files, nothing borrowed from this repo or `analytics/`), ran the real
`build → recover → resolve → compile → commit` pipeline against it with the new binary, and
confirmed: `.ekos/ledger/` contains only `facts/`, no `ledger.db` was ever created; `ekos query
find "README"` returns real, correct results against the fact-engine-backed ledger. Then
re-confirmed both pre-existing workspaces (this repo's own `.ekos/`, `analytics/`) are untouched —
still SQLite-only, exactly as before.

## A real regression caught by running the actual full gate, not a narrower one

First verification pass ran `cargo test -p ekos --lib` (fast, targeted) and declared victory —
wrong. The mandatory full gate is `cargo test --workspace`, which also runs the `ekos` package's
own **integration test binaries** (a separate compilation unit from `--lib`). Running that turned
up 7 real failures in `crates/cli/tests/skeleton.rs`: every one of them called
`ekos_ledger::Ledger::open(&config.ledger_path(dir))` directly after running the real pipeline,
bypassing `open_store`'s auto-detection entirely — so after this change, the real data went to a
now-default `FactLedger`, and the test's hardcoded `Ledger::open` opened a *new, empty* SQLite
ledger instead of reading back what was actually written. Fixed by switching all 8 occurrences to
`ekos::commands::store::open_store` — the same call every real CLI command already goes through,
so the tests now exercise realistic behavior instead of an implementation detail that just changed.

That fix surfaced a second, genuinely interesting failure: `build_is_idempotent` held one
`open_store` handle open across a *second* `build::run()` call (which opens its own handle
internally) and hit `LockBusy` — tantivy's `IndexWriter` takes a real, enforced exclusive lock per
process/directory. This isn't a bug; it's the fact engine actually enforcing the single-writer
invariant CLAUDE.md already states as a project-wide rule, in a way SQLite's more permissive
locking never surfaced in this exact test shape. Fixed by scoping each store handle to just the
one read it needed and dropping it before the next `build::run()` call — which is also just how
every real CLI invocation already behaves (one process, one command, exits, releases the lock).

A further check (`grep -rl "Ledger::open"` across every test directory, not just the one that
failed) found the **same** hardcoded-`Ledger::open` pattern in the *separate*
`tests/integration/tests/integration.rs` workspace (`cd tests/integration && cargo test` — its own
Cargo workspace per `CLAUDE.md`, not covered by `ekos/`'s `cargo test --workspace` at all). Fixed
the same way, plus switching `Runtime::new(&ledger)` (generic over `KnowledgeStore`, but expects a
concrete `&S`) to `Runtime::over(&*store)` (the trait-object-friendly constructor, matching the
exact pattern `crates/cli/src/commands/ask.rs` already uses). All 3 tests there pass. Confirmed the
4 `benchmark/` Criterion benchmarks construct `Ledger`/`FactLedger` directly for isolated
micro-benchmarking and never call `open_store` — genuinely unaffected, not just unchecked.

**Lesson for next time a default like this changes**: `grep` for the concrete type being replaced
(`Ledger::open`, `Ledger::new`, whatever it is) across *every* test directory in *every* Cargo
workspace in the repo before considering the change verified — "I ran the tests" and "I ran all
the tests that exercise this" are different claims, and this repo has three separate workspaces
(`ekos/`, `tests/integration/`, `benchmark/`) that don't share one `cargo test` invocation.

---

## Knowledge Captured

- **Verify a memory record against the actual code before planning around it — even (especially)
  when the record sounds authoritative.** This one specific check turned "write a documentation
  timeline" into "ship the actual feature the timeline would have been apologizing for not having
  shipped yet."
- **A doc comment promising "open (or create)" is a real, checkable contract** — `FactLedger::open`
  and `Ledger::open` share that exact semantic, which is what made the fresh-workspace branch a
  one-line addition instead of new plumbing.
- **Lazy-write internals can make a test's naive assumption wrong even when the code under test is
  right.** `uses_fact_engine()`'s manifest-existence check is correct for its actual job (detecting
  an established fact store); it just isn't the right signal to assert on immediately after a
  bare `open()` with no writes. Check what a component's own initialization code actually touches
  on disk before writing an assertion against it.
- **This repo has three separate `cargo test` surfaces** (`ekos/`'s workspace, `tests/integration/`'s
  own workspace, `benchmark/`'s own workspace, per `CLAUDE.md`'s own Commands section) — a change
  affecting a shared default needs a targeted `grep` across all of them, not just the one
  `cargo test --workspace` invocation that happens to be top of mind.
- **Tantivy's `IndexWriter` lockfile is a real, working single-writer enforcement mechanism** —
  hitting `LockBusy` from a test holding two handles open wasn't a bug to route around, it was the
  fact engine correctly refusing a second concurrent writer, exactly matching CLAUDE.md's "no
  global mutable state / single writer" invariant, just enforced at the OS/file level here instead
  of only by convention.
- **RFC 0016's own dated sections (from its 2026-07-17 acceptance) already contained the entire
  decision framework for this — the "gate amendment," the soak-period condition, the phasing list.**
  Re-reading an already-accepted RFC in full before assuming new design work is needed found the
  answer already written down.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/store.rs` | `open_store`/`store_display` now default fresh workspaces to the fact engine; pre-existing SQLite workspaces unaffected; 3 new tests |
| `ekos/crates/cli/tests/skeleton.rs` | 8 call sites switched from hardcoded `Ledger::open` to `open_store`; `build_is_idempotent` fixed to not hold a store handle across a second `build::run()` call |
| `tests/integration/tests/integration.rs` | Same fix in the separate integration-test workspace; `Runtime::new` → `Runtime::over` for the trait-object store handle |
| `docs/rfcs/0016-fact-segment-engine.md` | New dated `## Default switch (2026-08-21)` section |
| `CLAUDE.md` | `ledger` crate-map entry updated to state the new default and backward-compat guarantee |
| `README.md` | "Fact-segment engine" section rewritten from "experimental opt-in" to "the default for new workspaces" |
| `~/.claude/.../memory/ekos-storage-state.md`, `MEMORY.md` | Corrected the stale "RFC 0016 implementation not started" description |
| `devlogs/devlog_67.md` | This file |
