# Devlog 297 — The Review page and the step bar in the console (RFC 0213 Phase 3)

**Date:** 2026-10-25
**PRs:** none (direct to `main`)
**Branch:** `main`

---

## Summary

The source-model review now works without a terminal. Every project page in the console shows the seven-step bar,
and a **Review** tab lists each subject's items, records decisions for selected ones, and signs a subject off on the
digest it showed, after the subject's name is typed. Identity matches gained a CLI review (`ekos review identity`), so
nothing in the review needs the agent-callable MCP tool any more. A browser walk-through on the iDempiere testbed passed
18/18; a full sign-off there still needs a person to settle 195 conflicts and 25 identity candidates.

---

## RFC 0213 Phase 3

### Problem / motivation
Plan exit for II.3: "a full review of the iDempiere scope in the console, sign-off recorded, its digest visible".
The console's subprocess limits (20 s read, 30 s write) were below what a review read of the whole ledger takes
(~25 s), and only the summary was cached, so `review show` was always slow.

### What was built

| Component | What it does |
|---|---|
| `commands/review.rs::review_state` | caches every subject's items + statuses (`state.json`); `show`, `list`, `signoff show`, the bar and MCP read it |
| `refresh_after_write` / `refresh_with` | after `decide`, `decide_identity`, `approve`, `revoke`: the loaded objects/relationships with the written versions in place → recomputed → cached under the new key |
| `ekos review identity` | `SameAs` confirm/reject by id or `A = B` label, note required to reject, evidence + event; `HUMAN_ONLY`, class review |
| `web/api/app/routes/review.py`, `review_write.py` | workflow/review reads; decide/signoff/revoke; argv with targets after `--`; 300 s limits |
| `readproc` | `review list`, `workflow status`, `review show <subject> --json [--undecided] [--limit n]` shapes; per-call `time_limit` |
| `web/ui/src/StepBar.tsx`, `pages/Review.tsx`, `review-shared.ts` | the bar on every project page; subject tabs, filterable table, select-all, decide box, sign-off box (typed confirmation, digest, stale changes, revoke) |
| `evals/oracle-idempiere/part2-review/` | `review_tour.py` (Playwright), results, two screenshots |

### Implementation details worth remembering
- **The post-write cache is exact by construction**: `all_objects`/`all_relationships` return the latest version per
  id, so replacing by id in the loaded vectors gives what a fresh read would; the subjects are order-independent.
  `the_state_cached_after_a_write_equals_a_fresh_read` and the identity test compare key, statuses and items.
- **Targets go after `--`**: a table name can start with `-`; clap reads everything after `--` as positionals.
- One reviewer field per page (token mode has no identity), shared by the decide and sign-off boxes.
- The UI shows UUID fact values shortened to 8 characters; the signed facts are unchanged.

### Decisions
- **Sign-off is offered again when already valid** (same digest): harmless, and hiding it would hide the digest's
  meaning behind state.
- **Conflicts and the glossary keep their pages**; the Review tab links there and signs the subject off.
- **No cache warm-up at commit**: it would cost every project ~25 s per commit on large ledgers whether anyone reviews
  or not. The first console read after a compile takes that time instead (noted in TODO).

---

## Knowledge Captured

- **Playwright `wait_for_load_state("networkidle")` returns at once after the page's first load** — it is per
  navigation, so it does not wait for fetches a click starts. Wait for the element's state (`expect(...).to_have_text`).
- A success line left on the page by an earlier action can satisfy a later wait: scope each result to its own box
  (`aria-label`), or a test reads "accepted" while the request is still running.
- `pgrep -f <pattern>` / `pkill -f` match the shell that runs them when the pattern is in the command line (exit 144
  again); keep the PID when starting a background server.
- `web/ui/node_modules/.vite` is root-owned on this machine: run vitest with a scratch config that sets `cacheDir`.
- ruff `ASYNC109` rejects an async function parameter named `timeout`; the console uses `time_limit`.
- Identity candidates on iDempiere include `@docusaurus/core = @docusaurus/<x>` pairs from the docs site's package
  names — the fuzzy same-source rule is worth a look before anyone reviews 25 by hand.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/{review,review_guard,catalogue,workflow}.rs`, `app.rs` | state cache, refresh after writes, `review identity` (2 tests) |
| `ekos/crates/semantic/src/review.rs` | `Item: Deserialize` |
| `web/api/app/{routes/review,review_write,readproc,semantics_write,main}.py`, `tests/test_review.py` | routes (5 tests) |
| `web/ui/src/{StepBar,WorkspaceShell,main}.tsx`, `pages/{Review,review-shared}.ts(x)`, `Review.test.tsx`, `api/types.ts`, `index.css` | step bar + Review page (7 tests) |
| `evals/oracle-idempiere/part2-review/` | walk-through script, results, screenshots |
| `ekos/docs/rfcs/0213-review-and-sign-off.md` | Phase 3 results |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/deployment.md`, `docs/generated/ekos-self-documentation.html` | RFC 0213 Phase 3 |
