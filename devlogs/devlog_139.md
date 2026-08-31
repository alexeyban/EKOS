# Devlog 139 — CI flake: FactLedger's write.lock needed a bounded retry

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

The previous commit's CI run failed on `build_redacts_a_fake_secret_from_the_observed_excerpt`
with `LedgerError::Locked` — "another writable process already holds the ledger's write lock" —
even though the test's own two ledger opens happen strictly sequentially, on the same thread, with
no other code able to run in between. Root-caused live (not by inspection) to a genuine kernel-level
flock scheduling artifact under heavy concurrent load: `acquire_write_lock`'s single, un-retried
`try_lock_exclusive` attempt could occasionally see the lock as still held for a few milliseconds
after the previous holder had already closed its file descriptor. Fixed with a short bounded retry.

---

## Investigation and fix

### Problem / motivation

CI failed on a test unrelated to the just-landed change (the evidence-path fix, commit `469c76a`)
— a `git diff` of that commit touched nothing near locking. The panic:

```
called `Result::unwrap()` on an `Err` value: cannot open fact ledger at /tmp/.tmpczzKpX/.ekos/ledger/facts:
cannot write: another writable process already holds the ledger's write lock at
/tmp/.tmpczzKpX/.ekos/ledger/facts/write.lock — only one writable ekos process ... may run against
this workspace at a time
```

from a test that does exactly `build::run(...).await.unwrap(); open_store(...).unwrap();` — two
sequential, non-overlapping opens of the *same* ledger, in the *same* async test function, on a
single-threaded `#[tokio::test]` runtime. No other task should be able to run between the first
handle dropping and the second opening.

### Reproducing it live, not guessing from the stack trace

Ran the identical test locally — passed. Ran it 8 times under `--test-threads=4` (closer to CI's
runner core count than my 12-core dev machine) — 2 of 8 failed, each time a *different* test in
`skeleton.rs` following the same `build → open` pattern. Isolated a minimal repro: one async
function doing 200 `build → open` iterations in a tight loop, run alone (200/200 passed), then run
**four-wide** as four parallel `#[tokio::test]` functions under `--test-threads=4` (failed within
tens of iterations, repeatedly, on entirely independent tempdirs per test — ruling out any
cross-test resource sharing).

Instrumented `acquire_write_lock` and `FactLedger`'s write-lock field with acquire/release
timestamps and thread ids (temporary, not shipped). The failing sequence, same thread throughout:

```
ACQUIRED  .../write.lock at t=...983935063   (build's own ledger)
RELEASING .../write.lock at t=...028162839   (build's ledger drops — 44.8ms later)
FAILED to acquire .../write.lock at t=...028211518   (open_store's attempt — 49µs later!)
```

The second attempt failed **49 microseconds** after the first handle's file closed, on the same
thread, with no other code able to interleave. This rules out a leaked handle or an async yield
point in this codebase's own logic — it's the kernel's flock bookkeeping itself occasionally not
having caught up by the time a new, non-blocking `try_lock_exclusive` on a *different* open file
description checks it, specifically under heavy multi-thread contention on the same host.

### Fix

`acquire_write_lock` now retries up to 20 times, 5ms apart (≤100ms worst case), before returning
`LedgerError::Locked`. A genuine second writer is still correctly rejected — the change only adds
up to ~100ms of latency to that rejection, imperceptible for a CLI command. Verified the retry is
what actually closes the gap, not just noise: 19/19 stress runs (the original 4-wide repro,
`--test-threads=4`) came back clean after the fix, versus a reliable ~25% failure rate before it
(2/8, 2/6, 2/8 across three separate batches beforehand).

### Testing

- `a_second_writable_open_fails_with_a_clear_locked_error_once_retries_are_exhausted` (renamed
  from `..._fails_fast_...` — still correct behavior, just no longer literally "fast") — unchanged
  assertions, still passes since it holds the lock for the whole test.
- New `a_writable_open_retries_through_a_lock_held_only_briefly`: holds the real `write.lock` file
  on a background thread for 30ms (longer than one retry interval, well under the 100ms budget),
  then releases it; asserts the main thread's `FactLedger::open` — started while the background
  thread still holds the lock — succeeds only *after* the background thread's release flag is set.
  This is deterministic by construction (flock is exclusive; `open` can only succeed post-release),
  not a timing-dependent flake itself, and it would fail against the pre-fix code (no retry means
  the main thread's single attempt, made before the 30ms window closes, would return `Locked`
  immediately).

---

## Knowledge Captured

- A test failing in CI but passing locally, with a stack trace pointing at otherwise-correct
  sequential code, is worth reproducing under **constrained parallelism** (`--test-threads=N` at
  or below the CI runner's actual core count) before assuming it's caused by the just-landed
  change. My 12-core dev machine masked this entirely at default parallelism; CI's smaller runner
  didn't.
- `flock()`-based advisory locks, while normally released synchronously on `close()`, can present a
  same-process, same-thread, sequential `try_lock_exclusive` failure immediately after the prior
  holder's fd closed, under enough concurrent load elsewhere on the host. Don't assume a `Drop`
  completing means a lock is instantly visible-as-released to the very next non-blocking trylock —
  a short bounded retry is the standard, low-cost mitigation, not a sign of a deeper bug once
  proven (via holding a lock on a background thread for a known window) that the retry genuinely
  bridges a real gap rather than papering over a leak.
- To prove a retry loop actually retries (rather than the test just not needing to), hold the
  contended resource on a background thread for a *known* window inside the retry budget and
  assert the main thread's success is causally *after* that window closes — the same "construct a
  scenario that can only pass if the mechanism under test is real" pattern used for the RFC
  0113/0114 pruning and caching tests earlier this session.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/fact_ledger.rs` | `acquire_write_lock` retries up to 20× (5ms apart) before failing; renamed and added a test |
| `TODO.md` | RFC 0104 Phase 1 entry updated with the CI-flake root cause and fix |
