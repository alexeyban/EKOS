# Devlog 299 — Step 1 in the console, and identity candidates that agree word for word (RFC 0223, 0224)

**Date:** 2026-10-25
**PRs:** none (direct to `main`)
**Branch:** `main`

---

## Summary

Steps 1–3 now run end to end in the console. A new **Sources** tab adds directories and Oracle databases, keeps a
database password in the server's keychain, tests the connection, and shows the database's census next to what was
compiled. A browser run started from an empty iDempiere project, analysed it in the console (32 min), and found
census = compiled for tables, views, routines and triggers. Identity candidates must now agree word for word: the
iDempiere identity review went from 25 candidates, 24 of them a table and its child or two sibling packages, to the 1
genuine one. Console fixes from the last demo: the review is cached at the end of Init & compile, the Conflicts list
gets a long enough time limit, the Review table no longer overflows.

---

## RFC 0223 — Sources page and database credentials

### Problem / motivation
Step 1's exit — empty project to compiled ledger without a terminal, numbers matching the census — failed on three
counts: a database could only be added by editing `ekos.toml`; a tenant's secret variables reached `ekos` only while
metered model calls were allowed, so a database password would vanish with that switch; nothing compared the census
with the ledger.

### What was built

| Component | What it does |
|---|---|
| `commands/database.rs` | `ekos database add|list|test|remove`; `toml_edit` edits checked by `check_sources`; `test` = connect + catalog (no profiling) + write probe + census vs the ledger's live objects (4 tests) |
| `plugins/oracle::test_connection` | the read `ekos build` does, minus profiling, returning counts only |
| `Tenant.source_env`, migration 4, `envs` | source credentials granted always; values filled from the keychain |
| `routes/sources.py`, `sources_write.py` | list, add directory (root named first), add database (password → keychain, token refused), replace password, test, remove (admin) (6 tests + 1 env test) |
| `pages/Sources.tsx` | the tab; step 1 on the bar links to it (3 tests) |
| `envs.BASE` | `LD_LIBRARY_PATH`, `TNS_ADMIN`, `ORACLE_HOME` — a production console otherwise stripped Instant Client's path |

### Decisions
- **A password only from a person.** Tokens may `operate`, but a route that takes a credential refuses them.
- **No keychain, no shortcut.** A container has none; the route answers with the `--password-env` route instead of
  writing the password anywhere else.
- **The root is named first.** A console-created project observes its root unnamed; naming any source makes EKOS read
  named sources only, so the page names the root too. The CLI keeps its explicit behaviour.

## RFC 0224 — Word-level identity candidates

### Problem / motivation
Jaro-Winkler rewards long shared prefixes: `ASP_PROCESS` / `ASP_PROCESS_PARA`, `react` / `react-dom`,
`@docusaurus/core` / `@docusaurus/utils`. RFC 0063 sends such pairs to review rather than merging them, which made
the identity sign-off 24 obvious rejections long.

### What was built
`similarity::tokens` / `token_compatible` (camelCase and separator split, `table`/`tbl`/`dim`/`fact` dropped, words
paired as spelling variants at Jaro-Winkler ≥ 0.9); a name score of 0 when words differ, `Person` exempt, identical
table rows exempt; `identity/current.json` at commit and the review filter; `SCORING_VERSION` folded into
`SemanticCompilerPass::version` with a test.

### Measured
25 → 12 → **1**. The 12 were DDL tables from the PostgreSQL migration scripts whose column lists scored as identical:
the first version let identical *columns* outweigh the words, so only identical *rows* do now.

---

## Knowledge Captured

- **Identical column sets are weak identity evidence** inside one application: the first cut of the exemption let 11
  table pairs through.
- **A secret's lifetime must not depend on a billing switch**: `model_env` was granted only with `allow_metered`; a
  database password needs its own grant.
- The console's production environment drops anything not in `envs.BASE`; a native library path counts.
- Oracle views go `INVALID` when a table they read is altered: after the RFC 0213 demo, 40 views (25 reading
  `C_BPARTNER` directly) showed as invalid; the census reports them.
- For a scratch console, `PYTHON_KEYRING_BACKEND` pointing at an in-process backend keeps a real keychain untouched.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/{database,review,commit,catalogue,mod}.rs`, `app.rs` | `ekos database`, identity current |
| `ekos/plugins/oracle/src/lib.rs` | `test_connection` |
| `ekos/crates/identity/src/{lib,similarity}.rs`, `ekos/crates/semantic/src/{lib,review}.rs` | RFC 0224 |
| `web/api/app/{models,envs,sources_write,commands,readproc,main}.py`, `routes/{sources,conflicts}.py`, tests | console |
| `web/ui/src/{pages/Sources*.tsx,StepBar.tsx,WorkspaceShell.tsx,main.tsx,api/types.ts,index.css}` | Sources tab |
| `ekos/docs/rfcs/0223-…`, `0224-…` | new RFCs with results |
| `evals/oracle-idempiere/part2-sources/` | browser run script and results |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/deployment.md`, `docs/generated/ekos-self-documentation.html` | docs |
