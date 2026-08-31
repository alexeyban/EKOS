# Devlog 140 — `docs generate --prose` silently ignored for `--layout curated`

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Demonstrating real `ekos docs generate` output against EKOS's own real ledger (for a benchmark
report's documentation-generation examples) surfaced a genuine bug: `--prose` combined with
`--layout curated` was silently accepted and silently did nothing — byte-identical output with or
without it, no warning printed. Fixed by rejecting the combination with a clear error instead.
Also live-reproduced (not fixed — deliberately out of scope, already tracked) the pre-existing
`select_llm_provider_for_prose` hardcoded-model bug, which failed with a real 404 against this
session's local Ollama setup.

---

## PR — reject `--prose --layout curated` instead of silently ignoring it

### Problem / motivation

Running `ekos docs generate --layout curated --output doc-prose --prose --yes` against a real,
freshly-committed ledger (5,757 objects) produced output byte-for-byte identical to the same
command without `--prose` — confirmed via `diff`. No warning, no token-cost estimate (which
`--prose` is supposed to print before running), no confirmation prompt, nothing.

Root cause: `generate()`'s dispatch on `Layout::Curated` returns early —
`return generate_curated(config, cwd, output);` — and `generate_curated`'s signature never took
`prose`/`yes` parameters at all. Only `Layout::Objects` (the default path) and
`Layout::SolutionArchitect` (which explicitly threads `prose, yes` into
`generate_solution_architect`) ever wired the flag through.

### Fix

`generate()` now checks `prose` before dispatching to curated and rejects it with:

```
--prose is not yet supported for --layout curated — use --layout objects or
--layout solution-architect, both of which do support it
```

This matches the codebase's own already-stated design principle for this feature —
`select_llm_provider_for_prose`'s doc comment: "a user who asked for it wants real output or an
honest failure, not silent placeholder prose." A silently-ignored flag violates that principle
just as much as a silently-degraded output would.

**Not attempted**: actually building curated `--prose` support. Curated pages are project-wide
roll-ups (README/Architecture/API/SequenceDiagrams), not single objects — the existing per-object
prose pipeline (`enrich_with_prose`, grounded via `AiRuntime::ask` on one object at a time) doesn't
directly apply. That's a real, separately-scoped feature, not a fix to make here.

### Testing

New `generate_curated_with_prose_errors_clearly_instead_of_silently_ignoring_the_flag`: calls
`generate(..., Layout::Curated, prose: true, yes: true)` and asserts the error message names the
unsupported combination, and that no output directory (or partial output) is left behind.

---

## Also found, not fixed: the pre-existing hardcoded-Ollama-model bug, live

TODO.md already flagged `docs.rs::select_llm_provider_for_prose` (and `marketing.rs`'s twin) for
calling `OllamaProvider::from_env()` instead of `from_env_with_model(config.llm.model.as_deref())`
— meaning a configured `[llm] model` is silently ignored for `--prose`, always falling back to the
hardcoded default `llama3.1:8b`. That entry says explicitly: "out of RFC 0088's own scope to fix;
tracked here so it isn't rediscovered from scratch."

Running `ekos docs generate --layout solution-architect --prose --yes` for real, against this
session's local Ollama (which has `llama3:latest` pulled, matching this workspace's `ekos.toml`,
but not `llama3.1:8b`) reproduced it exactly:

```
warning: findings memo executive summary generation failed — keeping deterministic findings
list only: api error 404: {"error":"model 'llama3.1:8b' not found"}
```

Handled gracefully — a warning, the deterministic findings list still written, not a crash —
which is itself worth noting as correct behavior distinct from the curated bug above (this one
degrades safely; that one didn't fail at all when it should have surfaced *something*).
Setting `OLLAMA_MODEL=llama3:latest` (the environment-variable override `from_env` does still
consult, one level below the ignored `[llm].model` config) worked around it and produced a real,
correctly-grounded executive summary for the same demo. Left as TODO.md already decided —
appended the live reproduction so a future session that hits this again recognizes it instead of
re-diagnosing from scratch.

---

## Knowledge Captured

- `--prose`'s per-layout wiring is easy to miss when a new layout is added, because `generate()`'s
  early-return dispatch pattern (`if layout == X { return generate_x(...) }`) makes it simple to
  write a new layout's function signature without the `prose`/`yes` parameters at all — the
  compiler never complains, since the caller's `prose: bool` variable just goes unused past that
  branch. Any future layout needs an explicit decision (support it, or reject it clearly) checked
  at review time, not assumed from "it compiled."
- A flag that's accepted but does nothing is a worse failure mode than a flag that errors clearly
  — the error is discoverable immediately; the silent no-op requires a `diff` against a
  known-good run (exactly how this one was found) to notice at all.
- Demonstrating a feature against real data is itself a good way to catch this class of bug —
  static reading of `generate_curated`'s call site alone wouldn't have made the missing parameters
  obviously wrong; running the actual command and comparing real output did.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/docs.rs` | `--prose` now rejected clearly for `--layout curated`; new regression test |
| `TODO.md` | Documented the fix and the live reproduction of the adjacent, already-tracked hardcoded-model bug |
