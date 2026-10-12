# Devlog 257 — SonarCloud report after the refactors: gate green, five smells open

**Date:** 2026-10-12
**PRs:** none (pushed to `main`, `[skip ci]`)
**Branch:** main

---

## Summary

Two scans ran today. The first, on `165c818`, confirmed devlog_256's complexity work and failed the quality gate on Reliability. The second, on `7d321e2`, passes every condition. This devlog records the report as it stands: zero bugs, five open code smells, and one condition close to its threshold.

---

## The three scans

| Scan | Revision | Gate | Bugs | Code smells | Open issues |
|---|---|---|---|---|---|
| 2026-09-29 | `d1adf75` | — | 0 | 59 | 59 |
| 2026-10-12 14:44 UTC | `165c818` | Failed | 6 | 18 | 24 |
| 2026-10-12 16:12 UTC | `7d321e2` | Passed | 0 | 5 | 5 |

The six bugs in the middle scan were not caused by the refactors. They were in the console's Semantics and Conflicts tabs and the semantics commit step, which that scan saw for the first time. Their fixes are in devlog_256's follow-up section.

---

## The report on `7d321e2`

### Gate conditions (new code)

| Condition | Value | Required |
|---|---|---|
| Reliability rating | A | A |
| Security rating | A | A |
| Maintainability rating | A | A |
| Coverage | 81.4% | ≥ 80% |
| Duplicated lines | 1.3% | ≤ 3% |
| Security hotspots reviewed | 100% | 100% |

### Overall

| Measure | Value |
|---|---|
| Lines of code | 148,900 |
| Bugs / vulnerabilities / hotspots | 0 / 0 / 0 |
| Code smells | 5 |
| Coverage | 86.7% |
| Duplicated lines | 1.6% |
| Reliability / security / maintainability | A / A / A |

### Open issues

| Rule | Severity | Where | Finding |
|---|---|---|---|
| `rust:S3776` | Critical | `ekos/crates/semantic/src/business_semantics.rs:899` (`synthesize`) | Cognitive complexity 241, limit 30 |
| `typescript:S3776` | Critical | `web/ui/src/pages/line-diff.ts:8` | Cognitive complexity 33, limit 15 |
| `typescript:S3776` | Critical | `web/ui/src/pages/line-diff.ts:83` | Cognitive complexity 27, limit 15 |
| `python:S8415` | Major | `web/api/app/routes/semantics.py:229` | A 409 `HTTPException` is not in the route's `responses` |
| `plsql:OrderByExplicitAscCheck` | Minor | `ekos/crates/migrate-logic/tests/clickhouse_semantics.sql:20` | `ORDER BY` without `ASC` |

None affects the gate. `synthesize` needs a designed decomposition (devlog_256). The other four are small.

---

## Knowledge Captured

- **The local checker's prediction held.** `scripts/cognitive-complexity.sh` said one Rust function would remain over the limit and named it with the number Sonar reports (241).
- **The checker covers Rust only.** The two `line-diff.ts` findings were not visible locally.
- **New-code coverage is 81.4% against 80%.** A moderately sized untested change fails the gate. Check `new_coverage` before pushing a large change to the console or the CLI.
- **Every figure here came from the public API, no token:** `api/project_analyses/search`, `api/qualitygates/project_status`, `api/measures/component`, `api/issues/search?resolved=false`. Compare the analysis revision with `HEAD` first.
- **Running a scan still needs `SONAR_TOKEN`,** which this environment does not have. The maintainer ran both of today's scans.

---

## Files Changed

| File | Change summary |
|---|---|
| `devlogs/devlog_257.md` | New: this report |
| `CLAUDE.md`, `TODO.md` | Devlog number; scan item ticked, four open findings listed |
