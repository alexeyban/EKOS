# Devlog 227 — SonarCloud from D: SHA-256 validator hashing, sandbox secret, UI promises, smells

**Date:** 2026-09-29
**PRs:** none (local commit to `main`, `[skip ci]`)
**Branch:** main

---

## Summary

SonarCloud rated EKOS **D on security** and failed the quality gate on `new_security_rating`.
Reliability was C (6 bugs) and there were 146 code smells. The cause was two vulnerabilities: MD5 in
the EKOS Migrate validator, and a literal password in the ClickHouse sandbox config. Both are fixed.
MD5 was replaced rather than suppressed, after checking that every engine in the matrix has SHA-256
natively. The 6 floating-promise bugs and the cheap smells (redundant closures, wildcard imports,
misplaced test modules, undocumented FastAPI error responses, duplicated literals, installer
positional parameters and a few more) are fixed too. The 35 Rust cognitive-complexity findings are
**not** addressed; see *Knowledge Captured*.

---

## Vulnerabilities

### `rust:S4790` — MD5 in `migrate-validate/src/hash.rs` (CRITICAL)

#### Problem / motivation
RFC 0155 hashed canonical rows with MD5 "for availability" and explicitly rejected SHA-256 for
"weaker portability across the engine matrix". That premise was wrong. PostgreSQL 11+ (`sha256`),
ClickHouse (`SHA256`) and Spark SQL (`sha2(…, 256)`) all have SHA-256 natively.

#### What was built

| Component | Change |
|---|---|
| `hash.rs` | `row_hash` uses `sha2::Sha256`; `md-5` dropped from the workspace |
| `dialect.rs` | New `hash_hex_expr(d, text)`, the single source for engine-side hashing. PG `encode(sha256(convert_to(x,'UTF8')),'hex')`, CH `lower(hex(SHA256(x)))`. Replaces three hand-written md5 sites |
| `tests/golden.rs` | 35 golden literals regenerated via `print_golden` |
| `tests/snapshots/*.sql.txt` | Regenerated (`UPDATE_SNAPSHOTS=1`) |
| `tests/live_engines.rs` | Prefix test goes through `hash_hex_expr`; new `both_engines_hash_non_ascii_text_identically` |
| RFC 0155 | Formulas updated; the alternatives list reversed; dated *Amendment 2026-09-29* |

#### Implementation details worth remembering
- PostgreSQL's `sha256` takes `bytea`, so text must go through `convert_to(…, 'UTF8')`. ClickHouse
  hashes a `String`'s bytes as stored. The new non-ASCII live test (`Ünïcödé 🦀`) exists to catch an
  encoding slip there, which would only ever show up on accented or emoji data.
- `prefix_60` (the first 15 hex characters), the `(count, sum)` pair and the endianness handling are
  unchanged.

### `xml:S2068` — password in `docker/clickhouse/named-collections.xml` (MAJOR)
The sandbox's named collection now reads `<password from_env="EKOS_MIGRATE_SOURCE_PASSWORD"/>`.
`docker-compose.migrate.yml` sets that variable, defaulting to the local-only sandbox password.
Verified by recreating the ClickHouse container and reading through the collection: 501 rows from
`pg_catalog.pg_class`.

---

## Bugs — `typescript:S9383` floating promises (6)
- `qc.invalidateQueries(...)` in statement position in `Config`, `RunDetail`, `Schedules` and
  `Workspaces` is now prefixed with `void`: fire-and-forget on purpose. Invalidations *returned*
  from `onSuccess` were left alone, because react-query awaits those.
- `Graph.tsx`: the clipboard copy gained a `.catch`. Clipboard access can be denied (permissions,
  insecure context), and the UI should then not claim the link was copied.

---

## Code smells fixed

| Rule | Where | Fix |
|---|---|---|
| `rust:S1612` (42) | workspace-wide | `cargo clippy --fix` with only `redundant_closure_for_method_calls` enabled |
| `rust:S2208` (3) | `plpgsql/parse.rs`, `migrate-dq/catalog.rs`, `ledger/partitioned/knowledge_store.rs` | Explicit imports (clippy `wildcard_imports` suggestions) |
| `rust:S9045` (2) | `semantic/transform_ir.rs`, `demo-server/main.rs` | Test module moved to the end of the file |
| `rust:S8863` | `common/redaction.rs` | Redundant `'static` in a `static` item's type removed |
| `python:S8415` (39) | `web/api/app/routes/*` | Every route's decorator declares the statuses it raises via `responses=`, including those raised in helpers the route calls. Descriptions live in the new `routes/_responses.py` |
| `python:S1192` (2) | `runs.py`, `schedules.py` | `NO_SUCH_RUN` / `NO_SUCH_SCHEDULE` constants |
| `python:S3776` (2) | `commands.py`, `runs.py` | `_param_argv` and `_log_events` extracted |
| `python:S7503` (4) | `deps.py`, `auth.py`, `runner.py`, `scheduler.py` | **Kept async**, marked `NOSONAR` with the reason: FastAPI runs async dependencies on the event loop, and the others are awaited interfaces |
| `typescript:S3776` | `Graph.tsx` | `graphParams`, `exportToGraph`, `pinPositions` extracted to module level |
| `typescript:S7723/S7758/S7740` | `graph-export.ts`, `mockDownload.ts` | `new Array<number>()`, `String.fromCodePoint`; the mock reads the anchor from vitest's `mock.contexts` instead of aliasing `this` |
| `css:S1874` | `index.css` | `word-break: break-word` → `overflow-wrap: anywhere` |
| `shelldre:S7679` (8) | `install.sh` | Positional parameters assigned to function-prefixed variables (POSIX `sh` has no `local`) |
| `docker:S1135/S7031` | `Dockerfile.dev` | "TODO.md" wording tripped the TODO rule; `rustup` merged into the `apt` `RUN` |

---

## Knowledge Captured

- **"Chosen for availability" deserves a re-check before it is defended against a scanner.** The
  RFC's comment "do not upgrade it to SHA-256 and lose an engine" was never tested. No engine was
  lost. Check the engine docs before suppressing a finding on portability grounds.
- **A hash change can expose tests that only passed by luck.** `mask_key("7")` must not contain `7`,
  but the mask ends in 8 hex characters of the hash, and about 40% of such strings contain any given
  digit. MD5's value happened not to. Assertions of the form "the output does not contain X" over
  hex output must use X outside `[0-9a-f]`.
- **ClickHouse config supports `from_env="VAR"` on any element**, including named-collection
  passwords. This is the way to keep a credential out of a checked-in `config.d` file.
- **`web/ui/node_modules/.vite` is root-owned on this machine.** `vitest run` passes all tests, then
  fails to write its results cache with `EACCES`. That is not a test failure, but it makes the exit
  code non-zero. Fix with `sudo chown -R $USER web/ui/node_modules/.vite`.
- **Not done: 35 `rust:S3776` cognitive-complexity findings** (recover.rs `run` at 245, docs-gen,
  both PL/pgSQL lexer and parser, SQL dialect parsers, …). They do not affect any rating
  (maintainability is A). Most are lexers and parsers whose branching is inherent, and refactoring
  them blind would risk regressions for no rating gain. Pick them up one at a time, each with its own
  tests.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-validate/src/{hash,dialect,bisect}.rs` | SHA-256, `hash_hex_expr`, flaky mask test fixed |
| `ekos/crates/migrate-validate/tests/{golden,live_engines}.rs`, `tests/snapshots/*` | Regenerated literals and snapshots; non-ASCII live test |
| `ekos/Cargo.toml`, `ekos/crates/migrate-validate/Cargo.toml`, `ekos/Cargo.lock` | `md-5` → `sha2` |
| `ekos/docs/rfcs/0155-canonical-serialization-and-checksums.md` | Amendment 2026-09-29 |
| `docker/clickhouse/named-collections.xml`, `docker-compose.migrate.yml` | Password via `from_env` |
| ~50 files under `ekos/crates`, `ekos/plugins` | Redundant closures, wildcard imports, test-module placement, `'static` |
| `web/ui/src/**` | Floating promises, Graph refactor, TS/CSS smells |
| `web/api/app/**`, `web/api/app/routes/_responses.py` (new) | Documented error responses, constants, extractions, `NOSONAR` rationale |
| `install.sh`, `Dockerfile.dev` | Shell and Docker smells |
