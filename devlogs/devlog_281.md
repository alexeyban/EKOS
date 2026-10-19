# Devlog 281 — RFC 0205 bake-off closed out, `ekos marketing post`, Lakehouse docs on ekos.dev

**Date:** 2026-10-19
**PRs:** none (local `main`, pushed)
**Branch:** EKOS main — 4814eab (route), 35bd908 (marketing post), this commit; ekos-lakehouse
084bc75, 6bc0063, a4947ed; ekos-webpage 3ab8738
**Design:** RFC 0205 §Measured, RFC 0030 amendment (2026-10-19)

---

## Summary

devlog_280 left the RFC 0205 model bake-off "built, not run" (expired OpenCode key). It ran on
2026-10-18 after the key was renewed; this records the result and the loose ends a check turned
up: the raw numbers were only in a scratch directory, TODO still said "blocked", and the bug fix
the bench found had no test of its own. Also: the Pentaho verification call went out on X through
a new `ekos marketing post`, and ekos.dev gained two Lakehouse guides.

---

## RFC 0205 bake-off (ekos-lakehouse 084bc75, 6bc0063, a4947ed; EKOS 4814eab)

### Result
| model | A drafted & planned (32 gap steps) | B equal to deterministic (15) | $ per run |
|---|---|---|---|
| Claude Haiku 5.5 | 6 | 11 | 0.035 |
| GPT-6 Luna | 7 | 9 | 0.032 |
| MiniMax M3 | 4 | 7 | 0.124 |

$0.37 over two runs (cap $1.00, OpenCode Zen, the one existing key). `[llm.tasks.translate]` =
Haiku 5.5, Luna fallback. Both runs' tables are now committed in the lakehouse repo
(`crates/lakehouse-ext/examples/bench-results/2026-10-18.md`); the RFC table is run 2.

### The bug the bench found
Run 1's ground truth disagreed with all three models on the same rows — the translator was
wrong: Rhino reads a null Number as 0 in arithmetic and comparisons (`parseInt('42') + qty` = 42).
Fixed in 084bc75 (`kettle-expr/src/java.rs`), which only updated existing assertions; a4947ed
adds `a_null_number_is_zero_in_javascript_arithmetic_only` (JS coalesces, Java does not),
verified to fail with the fix reverted.

## `ekos marketing post` (35bd908, RFC 0030 amendment)

`publish` always drafts from a devlog through an LLM and caps at 280 characters; a hand-written
announcement needs neither. `post <file> [--confirm]` sends the text as written through the same
`TwitterPublisher` (trimmed, ≤25,000 characters for X Premium, preview without `--confirm`), logs
to `marketing/logs/marketing.log`, catalogue class `local`. `--confirm` is the per-run decision, so
the `[marketing.twitter] enabled = false` pause on automatic `publish` stays. First live use of the
OAuth 1.0a signer: tweet 2112099338776174825 (the Pentaho verification call).

## ekos.dev (ekos-webpage 3ab8738)

Guides `building-a-lakehouse` and `translating-pentaho-to-dbt`, a Pentaho section on the
Lakehouse landing page (status: not yet compared against real Pentaho runs), a cross-link from
the Databases integration page; deployed, all pages 200, no horizontal scroll at 390 px.

---

## Knowledge Captured

- **A bench's raw output belongs in the repo, not the scratchpad.** The RFC's numbers could only
  be traced because the session's scratch directory still existed.
- **When every model disagrees with the ground truth on the same rows, check the ground truth.**
- **A fix that only edits existing assertions is not a regression test** for the reported case.
- **X credentials live in `~/.bashrc`** (`TWITTER_*`); Claude Code's shell does not see them —
  run posting through `bash -ic`. The repo has no GitHub secrets for the marketing workflow.
- A docs claim drafted from a plan can be wrong: the plan said job order becomes dbt
  dependencies; the code only records it (`pentaho-dbt` `jobs`). Check the code before publishing.

---

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/cli/src/{app.rs,commands/marketing.rs,commands/catalogue.rs}` (35bd908) | `marketing post` |
| `ekos/docs/rfcs/0030-marketing-agent.md`, `CLAUDE.md` (35bd908) | amendment, command line |
| `TODO.md`, `docs/generated/ekos-self-documentation.html`, `devlogs/devlog_281.md` | this record |
| ekos-lakehouse `crates/kettle-expr/src/script.rs`, `examples/bench-results/`, `devlogs/devlog_12.md` | test, results, record |
| ekos-webpage `docs-src/04-guides.md`, `07-…md`, `site-src/pages.md`, `tools/build_docs.py` | guides, landing, nav |
