# RFC 0142 — Telling the user what `ekos commit` is doing

**Status:** Proposed
**Author:** EKOS team
**Created:** 2026-09-09

---

## Motivation

`ekos commit` on a real workspace prints a wall of third-party index logs, then goes completely
silent for what can be hours, then prints a summary. Observed live on a 1,186-module workspace:

```
2026-09-09T14:26:53Z INFO tantivy::directory::managed_directory: Deleted "20b4…fieldnorm"
2026-09-09T14:26:53Z INFO tantivy::directory::managed_directory: Deleted "52274…pos"
…
LLM description requested for up to 1187 real call(s) … Proceed? [y/N]: y
                                    ← nothing, for hours
```

Two distinct problems, and they pull in opposite directions:

1. **The output is loud where nothing interesting is happening.** Every line above is tantivy's
   internal segment bookkeeping at `INFO`. It is not about the user's data and it is the *only*
   thing on screen.
2. **The output is silent where everything is happening.** `describe_objects` (RFC 0088) makes one
   sequential LLM call per module. At 1,186 modules against a local Ollama, that is hours with no
   indication of progress, no count, no ETA — indistinguishable from a hang. The user has already
   accepted a cost prompt at that point, so abandoning the run means paying it again.

Nothing here is a correctness bug; the run completes and the summary is accurate. It is a
usability defect severe enough that a user reasonably asks *"what is happening?"* — which is how
this RFC started.

---

## Design

### 1. Quiet the third-party index logs by default

`init_logging` builds its filter from `[workspace] log-level` alone (`"info"`), which enables
`INFO` for every dependency including tantivy's per-file garbage-collection lines.

Default the filter to `"<level>,tantivy=warn"`. `EKOS_LOG` continues to override the whole filter,
so `EKOS_LOG=info,tantivy=info` restores the old behaviour for anyone debugging the index.

Deliberately narrow: this silences one named dependency's routine bookkeeping, not warnings, not
errors, and nothing EKOS itself logs.

### 2. Report progress through the long phase

`describe_objects` is the only unbounded-duration phase, and it already knows exactly how many
objects it will visit before it starts.

**A progress callback, not a progress bar in the library.** `ekos-recovery` must not learn about
terminals, cursors, or TTY detection — that is the CLI's concern, and the crate is also used by
the MCP server and tests. The seam is a plain callback:

```rust
pub struct DescriptionProgress<'a> {
    pub done: usize,        // objects finished, including cache-skips
    pub total: usize,       // objects this scope will visit
    pub described: usize,   // real LLM calls that succeeded
    pub skipped_cached: usize,
    pub errors: usize,
    pub current: &'a str,   // the object just handled
}
```

**No public API break.** `describe_objects` keeps its exact signature and delegates to a new
`describe_objects_with_progress(..., progress: &dyn Fn(DescriptionProgress))`, passing a no-op.
Every existing caller and test is untouched.

### 3. Render it in the CLI, correctly for both destinations

The rendering has to serve two audiences that want opposite things:

- **A terminal**: a single line rewritten in place with `\r` — count, percentage, elapsed, ETA, and
  the current object name.
- **A pipe or log file** (the practical case here — these runs are long enough to be run under
  `nohup`): `\r` produces one unreadable mega-line. So when stdout is not a TTY, emit a normal
  newline-terminated line at intervals instead.

`std::io::IsTerminal` (stable since Rust 1.70) decides this. **No new dependency** — `indicatif`
would be the conventional choice, but the entire requirement here is one line of counter text, and
this workspace has a documented preference for not taking a dependency for something this small.

ETA is derived from mean elapsed-per-object so far. It will be wrong early in a run and settle;
that is honest and still far more useful than nothing, but it must be *labelled* an estimate.

Progress goes to **stderr**, not stdout: `commit`'s stdout is its report, and a progress line
interleaved into it would corrupt anything parsing that output.

---

## Non-goals

- **A progress bar for the whole `commit`.** The other phases (evidence, objects, relationships,
  rollups, lineage) are fast and already summarised. Only the LLM phase is unbounded.
- **Parallelising the LLM calls.** Real speedup, genuinely wanted, and entirely separate work —
  it changes ordering, error handling, and rate-limit behaviour. Progress reporting must not be
  bundled with it.
- **A general progress framework** for every long command. `recover` and `compile` are bounded and
  already report per-pass. If a second unbounded phase appears, generalise then.
- **Changing the cost-confirmation prompt.** It works and is a deliberate safety gate.

---

## Verification

- `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`.
- A unit test that the callback fires once per visited object, including cache-skips — a progress
  counter that silently stalls on the cached path would be worse than none, since a long cached
  stretch is exactly when the user suspects a hang.
- A test that the no-op delegation preserves `describe_objects`' existing behaviour.
- Manual check of both render paths: a TTY (single rewritten line) and a pipe (periodic lines),
  since the whole point of §3 is that these differ.
