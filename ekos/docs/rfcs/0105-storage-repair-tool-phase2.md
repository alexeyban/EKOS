# RFC 0105 — Storage Architecture Phase 2: WAL Recognition + Repair Tool

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0080 scoped Phase 2 as "a WAL + repair tool," but its own investigation already found there's
no new WAL to build: `FactLedger`'s existing segment format (checksummed frames, atomic manifest
writes, automatic torn-tail truncation and index self-heal on open) already *is* a real,
ledger-level write-ahead log — durable, crash-safe, and independent of either backend's own
internal storage engine. The real, concrete gap RFC 0080 identified is narrower and more
actionable: **no tool surfaces any of this today.** `TODO.md` had characterized ledger recovery as
"the only option is a full migration rollback" — investigated here and confirmed still accurate:
`SegmentStore::verify_sealed` exists, is unit-tested, and is never called from any real command.

## Design

### The existing segment format *is* the WAL — recognized, not rebuilt

`segment/mod.rs`'s own doc comment already states the durability properties a WAL needs to provide:
fsync-then-publish ordering, crash recovery that scans forward and truncates a torn tail, and
whole-segment integrity via SHA-256. Nothing here changes that format. This RFC's "WAL" half is
documentation, not code: recognizing in `RFC 0080`/`TODO.md` that this gap was already closed by
RFC 0016's own design, and that Phase 2's real deliverable is the tool half.

### `verify_sealed_report`: every segment checked, not just the first failure

`SegmentStore::verify_sealed` (existing, tested) stops at the first corrupt sealed segment — a
reasonable fail-fast contract for an internal caller, but useless for a repair report a human needs
to act on (which segment? how many? what tx range does each affect?). Added
`SealedSegmentCheck { seq, ok, len, sha256_ok, detail }` and `verify_sealed_report(&self) ->
Vec<SealedSegmentCheck>`, which checks every sealed segment unconditionally and returns one row
per segment. `verify_sealed` is refactored to use the same per-segment check internally (returning
the first failure, unchanged behavior/error text shape, existing tests untouched) — one source of
truth for what "a segment fails verification" means, not two independently-maintained checks.

### `ekos ledger repair`: the actual surfaced tool

A new subcommand alongside the existing `ekos ledger status`/`migrate` (same file,
`crates/cli/src/commands/ledger.rs`), `FactLedger`-only (matches this whole phase's own scoping —
the SQLite backend has no segment/manifest concept to repair; `PRAGMA integrity_check` already
exists for it and is out of this RFC's scope). Opens the ledger **writable** — the same open path
every other write-capable command uses, which already performs its two *existing*, previously
un-surfaced self-heals for free (torn active-segment tail truncation; stale/unreadable index-runs
rebuild via the memtable path) — then runs `verify_sealed_report()` and prints one line per sealed
segment (`OK` or a named failure with its `tx_min..tx_max` range), plus a summary count.

**What this tool does *not* pretend to do**: a sealed segment whose hash genuinely doesn't match
(real disk-level bit rot, a truncated backup, etc.) has no synthesizable fix — there's no
redundancy anywhere in this format to reconstruct lost bytes from. The tool's real, honest value is
turning an opaque "run migrate and hope" workflow into a precise report: exactly which segment(s),
exactly which transaction range, so a human can make an informed call (restore that one file from a
backup if one exists, or knowingly accept the loss for that range) — replacing `TODO.md`'s
previously-accurate "the only recovery option is a full migration rollback" with a real diagnostic
path that doesn't require throwing away the whole ledger to find out what's wrong.

## Non-goals

- **Automatic repair of a genuinely corrupt sealed segment.** Not possible without redundancy this
  format doesn't have (see above) — reporting precisely, not fixing the unfixable, is this RFC's
  real scope.
- **A SQLite-backend equivalent.** `PRAGMA integrity_check` already exists and does the analogous
  job for that backend; RFC 0104 Phase 1 already fixed the concrete corruption mechanism found live
  for SQLite. Matches every prior phase's precedent of not doubling scope onto the backend already
  being phased out.
- **A `--fix`/auto-quarantine flag that discards a bad segment automatically.** A real, more
  invasive follow-on if real usage shows it's wanted; not attempted here without a live need, and
  discarding a segment is exactly the kind of destructive action this project's own conventions
  require a human decision for, not a default CLI flag.

## Verification

New `ekos-ledger` tests: `verify_sealed_report` returns one `OK` row per real sealed segment on a
healthy store; flipping a byte inside one sealed segment (mirroring the existing
`verify_sealed`-corruption test's own technique) produces exactly one failing row naming that
segment's `seq`/`tx` range, with every other segment still reported `OK` (proving the report
doesn't stop early); `verify_sealed`'s own existing pass/fail tests still pass unmodified (the
refactor didn't change its contract). New `ekos` (CLI) tests: `ekos ledger repair` against a
healthy real workspace reports every segment `OK`; against a workspace with one byte-flipped sealed
segment, reports exactly that segment as failing with its tx range, exit code reflecting a real
problem was found (non-zero) rather than a clean run. Full workspace gate clean (`cargo fmt`,
`build --workspace`, `clippy --workspace -D warnings`, `test --workspace`), `tests/integration`
3/3.

Live-verified through the real `ekos` binary: `ekos ledger repair` against a real scratch
workspace produced by the full `init`/`build`/`recover`/`resolve`/`compile`/`commit` pipeline opens
cleanly and reports the honest "no sealed segments yet" case — the default 8MB seal threshold means
a small scratch fixture never actually rolls a segment, so this is the real, common case for most
workspaces this size, exercised through the real CLI end to end, not a mock. The corruption-report
path itself (a byte-flipped sealed segment correctly named with its `tx_min..tx_max` range, every
other segment still reported `OK`) is verified with real segment files on real disk, real
`fs::write` corruption, and the real `repair()` CLI function — the `ekos-cli` tests use a tiny
1-byte seal threshold (the same technique `ekos-ledger`'s own segment tests already use) to force
real sealed segments quickly rather than writing megabytes through the full pipeline; the segment
format and repair code path exercised are identical either way, only the trigger for "how a segment
became sealed" differs.
