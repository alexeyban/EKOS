# Devlog 167 — SonarCloud scan: `sonar.sources` regression found and fixed

**Date:** 2026-09-05
**PRs:** (commit `7942fe2`, local `main`, `[skip ci]`)
**Branch:** main (direct)

---

## Summary

Asked to run a fresh SonarCloud scan and confirm every metric is green. The scan "succeeded" and
reported a clean Quality Gate on the first attempt — but investigating why several core measures
(`ncloc`, `coverage`, `lines`, `files`) were completely absent from the API response uncovered a
real regression: the previous session's `sonar.tests` fix (commit `3756a9b`) had silently dropped
the entire codebase from analysis except the 24 files matching `sonar.test.inclusions`. Fixed by
pinning `sonar.sources=.` explicitly. Also closed the resulting real `new_coverage` gap (73.0% →
84.4%) with a genuine missing test, not a suppression.

---

## The bug

`sonar-project.properties` set `sonar.tests=web/api/tests,web/ui/src` and `sonar.test.inclusions=…`
but never set `sonar.sources`. With `sonar.tests` explicitly configured, the SonarScanner CLI does
**not** fall back to `.` for `sonar.sources` on its own — confirmed by re-running with `-X` and
reading the indexing log directly: `Input files for indexing: [web/api/tests/*.py, …]`, `24 files
indexed (done)` as the **entire project total**. Zero of 237 tracked `.rs` files, zero of 26
non-test `web/api/app/*.py` files, zero of 22 non-test `web/ui/src/**/*.tsx` files were ever
indexed, sourced or tested. The Rust plugin (`rustenterprise`) wasn't even loaded — it only
activates once `.rs` files are detected during preprocessing, and none were.

The scan still finished, uploaded a report, and processed successfully — a project with ~0
analyzed source obviously has ~0 issues, so the Quality Gate came back green. This is the
dangerous shape of this class of bug: nothing errors, nothing warns loudly, the dashboard just
quietly stops meaning anything. The tell was querying `api/measures/component` for `ncloc`/
`coverage`/`lines`/`files` and getting an **empty measures array** — those metrics aren't
"failing", they're simply never computed when the file set is this small.

**Fix**: pin `sonar.sources=.` explicitly alongside the existing `sonar.tests`/
`sonar.test.inclusions`/`sonar.exclusions` — the standard, well-supported combination. Re-running
jumped indexing from 24 files to 2,941; `ncloc` appeared for the first time at 81,320 (later 81,392
after this session's own commits), `files` at 294, real coverage at 86%+, and the Rust Enterprise
sensor ran for real (24.5s).

## The real (not fabricated) new_coverage gap this surfaced

Once analysis was genuine, the Quality Gate's `new_coverage` condition failed for real: 73.0%
against an 80% threshold. Six files accounted for 205 of the 251 total new-code uncovered lines,
four of them inside [[rfc-0138-eval-harness|RFC 0138]]'s own new `ekos-evals` crate —
`runners::agent_runner` (29 lines, 0%), `runners::retrieval_runner` (15 lines, 0%), `lib::run_all`
(14 lines, 0%), `evaluators::evaluate` (60 lines, 0%). RFC 0138's own Verification section had
promised "`agent_runner` tested against `MockLlmProvider`" — that test was never actually written.
Added it for real: one end-to-end test seeding a `Ledger`, building a real `Runtime`/`AiRuntime`
over a `MockLlmProvider`, and driving all three `Scenario::Mode`s (`Ask`/`Reason`/`Retrieval`)
through `run_all` in one pass. `new_coverage` went to 84.4%. Also fixed two trivial
closure-vs-method-reference code smells in `schema.rs` found by the same now-working scan
(`.filter_map(|e| e.ok())` → `.filter_map(Result::ok)`, same pattern for `.to_str()`).

**Left alone, deliberately**: `ekos/crates/cli/src/bin/ekos.rs`'s `main()` dispatch function
crossed Cognitive Complexity 42 (>15 allowed) once the new `Commands::Eval` arm was added — a
pre-existing, accepted category of debt in this codebase (`recover.rs` sits at 232, `build.rs` at
68, `mcp.rs` at 51, all still open) for CLI dispatch/parsing functions specifically, not something
this session's scope was to refactor.

---

## Knowledge Captured

- **SonarScanner CLI does not default `sonar.sources` to `.` once `sonar.tests` is explicitly set.**
  Pin both explicitly whenever a project needs `sonar.tests` for test-file classification — never
  rely on the "sources defaults to project root" behavior once tests are configured by hand.
- **A SonarCloud scan reporting a clean Quality Gate is not proof the scan actually analyzed
  anything.** Cross-check `ncloc`/`files`/`lines` via `api/measures/component` (or just read the
  scanner's own `-X` indexing log) before trusting a green gate on a codebase this size — an empty
  measures array for basic size metrics is the tell, not an error message.
- **`rustenterprise` (SonarCloud's Rust plugin) loads lazily**, only once `.rs` files are detected
  during the scanner's file-preprocessing pass — it silently doesn't appear in "Plugins loaded" at
  all when zero files match, which looks identical to "Rust isn't supported on this plan" unless
  you know to check file counts first.
- Confirmed live: this session's SONAR_TOKEN + local `sonar-scanner` 8.1.0.6389 CLI (found at
  `~/Downloads/sonar-scanner-8.1.0.6389-linux-x64/bin/`) is the actual mechanism this project uses
  to scan — there is no CI wiring (`.github/workflows/` has no Sonar step), it's a deliberate
  manual/on-demand step, consistent with [[feedback-local-tests-skip-ci]].

---

## Files Changed

| File | Change summary |
|---|---|
| `sonar-project.properties` | Add `sonar.sources=.` (the actual fix) |
| `ekos/crates/evals/src/lib.rs` | Real `agent_runner`/`retrieval_runner`/`run_all` test against `MockLlmProvider`, all three `Mode`s |
| `ekos/crates/evals/src/schema.rs` | Two closure→method-reference cleanups |
