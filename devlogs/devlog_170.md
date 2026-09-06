# Devlog 170 — evals/test-runs ledger self-contamination, found via the RFC 0138 baseline

**Date:** 2026-09-06
**PRs:** (uncommitted at time of writing — pending user review)
**Branch:** main (local)

---

## Summary

Continuing the open TODO from the previous session ("current agent answer quality is weak —
needs improvement, not just measurement"), ran the full 101-scenario `ekos-full` suite for the
first time (only the older 32-scenario version had ever been run end-to-end). While diagnosing
individual low-scoring answers, found a real, previously-undiscovered bug: `ekos.toml`'s
`[observe] ignore-patterns` was missing two directories — `evals/` (RFC 0138's own scenario
YAML + saved JSON reports) and `test-runs/` (a 2.3GB local, git-ignored leftover from the RFC
0111/0113 distributed-storage E2E soak test, devlog_144) — so `ekos build` had been ingesting
both as if they were EKOS's own project content. This is the same contamination class already
fixed once for `doc`/`doc-sa`/`doc-objects` (devlog_90). Fixed both, rebuilt the full pipeline
from scratch, and re-ran the eval suite for a clean comparison. The fix is real and confirmed
(a live repro is gone), but the eval score impact was smaller than expected — hallucination rate
and evidence groundedness improved modestly, while answer correctness and completeness stayed
flat, meaning `llama3:latest`'s own answer-generation quality remains the dominant open problem,
not ledger contamination.

---

## Finding — two `ekos.toml` ignore-pattern gaps causing ledger self-contamination

### Problem / motivation

The previous session's TODO called for (1) a full 101-scenario baseline run and (2) inspecting
low-scoring answers for a real failure-mode pattern before touching prompts/retrieval. Running
`ekos ask` directly for the failing `sec-001` scenario ("what module redacts secrets/PII?")
returned: *"the module responsible... is the one mentioned in `evals/datasets/security.yaml`"* —
citing the eval's own question file back at itself instead of naming `redaction.rs` directly,
even though the real file was in its evidence set. A search for `FooBarNonexistentAnalyzer` (an
intentionally-fake analyzer name that only exists in `evals/datasets/adversarial.yaml`, used to
test hallucination resistance) returned that file as a real ledger hit.

### What was found

- `evals/datasets/*.yaml` and `evals/reports/*.json` were both indexed into the ledger — every
  eval run's own report became future contamination for the next run, compounding over time.
- `test-runs/` — a git-ignored (`.gitignore` line 21) but **not** `ekos.toml`-ignored 2.3GB
  directory of leftover E2E test artifacts (a duplicated 95-partition Elixir workspace + MySQL/
  MSSQL DB-script fixtures from the RFC 0111/0113 soak test, devlog_144, 2026-08-31) — was also
  being fully re-observed on every `ekos build`. This was the larger of the two: it alone
  accounted for a 376-conflict `ekos resolve` hard-stop (generic names like `generate`/`validate`/
  `json`/`test` colliding across Rust/Python/Elixir/JS symbols from that test corpus) and roughly
  a third of the ledger's total object count (35,444 → 23,443 objects after exclusion).
- Being git-ignored does not exclude a directory from `ekos build`'s own observation walk —
  `ekos.toml`'s `[observe] ignore-patterns` is a separate, unrelated exclusion list. This is worth
  remembering as a general gotcha: any large local scratch/test-output directory needs its own
  entry here even if `.gitignore` already covers it for version control.

### What was built

Two new `ignore-patterns` entries in `ekos.toml`, each with an inline comment recording how it was
found (matching the existing `doc`/`doc-sa`/`doc-objects` entry's style):
```toml
"evals",
"test-runs",
```

### Rebuild + verification

Config changes to `[observe]` don't retroactively purge already-ingested objects from the
artifact store — the first incremental `ekos build` after the edit only reported "Files observed
(new): 1" (RFC 0135 Part A's fingerprint logic correctly avoided an unnecessary full wipe, but
that also meant it didn't re-scan already-observed files against the new exclusion list). A real
purge required moving `.ekos/` aside and running the full pipeline (`init → build → recover →
resolve --force → compile → commit`) from scratch.

Post-rebuild verification: `ekos query find "FooBarNonexistentAnalyzer" --mode lexical` now
returns zero hits (previously one, directly against `evals/datasets/adversarial.yaml`); a search
for `"ekos-full"` returns only real RFC/doc/code files, no `evals/` paths.

### Before/after — clean re-baseline (`ekos-full`, 101 scenarios, `ollama llama3:latest`)

| Metric | Contaminated (`20260906T132949Z`) | Clean (`20260906T182532Z`) | Δ |
|---|---|---|---|
| Passed | 47/101 | 48/101 | +1 |
| Answer correctness | 39.3% | 37.6% | −1.7pp |
| Evidence groundedness | 75.5% | 78.3% | +2.8pp |
| Completeness | 37.6% | 36.8% | −0.8pp |
| Recall@10 | 75.0% | 65.0% | **−10pp** |
| Hallucination rate | 12.9% | 9.9% | −3.0pp (better) |

Category pass counts (before → after): adv 7→10/18, arch 9→8/20, code 8→7/15, dep 11→11/12,
hist 2→2/12, lin 6→7/12, sec 4→3/12.

**Honest read, not spun**: this was a real, confirmed bug with a genuine live repro, and fixing it
produced a real improvement in hallucination rate and evidence groundedness. But overall pass rate
and answer correctness are essentially flat, and recall@10 actually dropped 10 points post-fix —
plausibly a retrieval-ranking shift from a ~34%-smaller corpus, or `ollama`'s own non-deterministic
sampling contributing noise at this scenario count, not yet root-caused. The takeaway: ledger
contamination was worth fixing on its own merits (correctness of what the system tells the truth
about), but it was **not** the dominant cause of the 45%-answer-correctness result flagged last
session. `llama3:latest`'s own answer-generation quality remains the primary open gap — the
original TODO's steps (1) A/B a stronger provider and (2) inspect remaining low scorers for a real
failure-mode pattern are still open and now more clearly the right next move.

---

## Incident — `ekos commit` killed by an OOM-style harness protection, ledger self-recovered

Mid-rebuild, the first `ekos commit` attempt (with `[llm-description]` still enabled) was killed
after ~11 minutes when the system dropped to 415MB free RAM — caused by unrelated Chrome/Firefox/
Edge browser processes on the same machine, not by `ekos` itself (which was only using ~320MB
RSS at the time). Re-running `ekos status` afterward completed a `tantivy` `prepared_commit`
finalize and garbage-collected the orphaned segment files left by the interrupted process,
reporting a consistent `118,283` fact entries / `76,965` objects with no corruption. Re-running
`ekos commit` from there completed cleanly (`401` objects written, `77,745` relationships,
`7,324` evidence records, `65` rollups — the rest were already-committed from the interrupted
run: `53,429` objects skipped).

This is real, live evidence that the RFC 0016 fact-ledger engine's append-only WAL design is
robust to an ungraceful interruption mid-commit — exactly the property the append-only invariant
(CLAUDE.md's "Key invariants" section) is meant to guarantee, now confirmed under a real failure
rather than just by design.

---

## Knowledge Captured

- **Git-ignored ≠ EKOS-ignored.** `.gitignore` and `ekos.toml`'s `[observe] ignore-patterns` are
  two independent exclusion lists. A large local scratch/test-output directory that's correctly
  git-ignored (so it never gets committed) can still be fully re-observed and ledgered by
  `ekos build` if nobody remembers to add the second entry. Check both whenever a new large local
  directory shows up in a workspace this repo self-observes.
- **`ekos.toml` config changes don't retroactively purge the artifact store.** Adding an
  ignore-pattern only affects what future `ekos build` scans discover; already-ingested objects
  matching the new pattern stay in `.ekos/artifacts`/the ledger until a full rebuild from a wiped
  `.ekos/`. RFC 0135 Part A's fingerprint logic correctly avoids an unnecessary full wipe on most
  config edits (that was the point of that RFC), but this specific case — narrowing observation
  scope — needs one anyway; there's no existing "prune objects matching an updated ignore list"
  path.
- **A full from-scratch pipeline rebuild in a debug binary is slow at this corpus size** — the
  final `ekos commit` (rollups + data lineage + fact-ledger/tantivy index writes over ~77k
  objects, `[llm-description]` disabled) took roughly 1.5-2 hours of wall time in an unoptimized
  `cargo build` binary. A release binary would very likely be dramatically faster for this kind
  of full-corpus operation; worth defaulting to `cargo build --release` before any future
  from-scratch rebuild at this scale rather than the debug binary used for day-to-day iteration.
- **`[llm-description] scope = "all"` is expensive to regenerate from a cold cache.** Wiping
  `.ekos/` also wipes `.ekos/llm-cache/`, forcing every description back to a fresh LLM call
  across thousands of Rust/JS/Python symbols. Restoring the old cache directory (content-addressed,
  so stale-but-matching entries are still valid hits) helps only if the underlying content is
  largely unchanged. For this rebuild, `[llm-description]` was temporarily disabled to keep the
  contamination-fix measurement fast and re-enabled afterward — the ledger's `ai_overview`/
  `ai_usage`/`ai_comment_check` properties are now stale/absent until the next real `ekos commit`
  regenerates them (tracked as a follow-up, not done in this session).
- **`ekos resolve --force` remains the right move for this repo's baseline conflict count.**
  Even after removing the `test-runs` contamination, ~223 identity conflicts remain (generic names
  like `diff`/`token`/`seed` colliding across this repo's own Rust/Python/JS plugin test fixtures)
  — pre-existing, expected, and unrelated to this session's fix (devlog_64 already documented
  the `--force` flag's purpose for exactly this class of conflict).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos.toml` | Added `evals` and `test-runs` to `[observe] ignore-patterns`, each with a dated comment recording the discovery |
| `evals/reports/20260906T132949Z-ekos-full-baseline.json` | New — contaminated 101-scenario baseline (47/101 passed) |
| `evals/reports/20260906T182532Z-ekos-full-clean.json` | New — clean 101-scenario baseline post-fix (48/101 passed) |
| `.ekos/` | Rebuilt from scratch (`init`/`build`/`recover`/`resolve --force`/`compile`/`commit`); not version-controlled |
