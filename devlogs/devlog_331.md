# Devlog 331 — The analyst loop: data signals, references, questions, testimony (RFC 0252–0254)

**Date:** 2026-11-10
**PRs:** none — committed to `main` after local gates (a6ddff19 … 60e2632c and the console merge)
**Branch:** main (with merged local branches `rfc0253-parsers`, `rfc0252-sqlserver`,
`rfc0252-informix`, `rfc0254-passport`, `rfc0253-console`)

---

## Summary

RFC 0252–0254 were accepted and built in one session. EKOS now measures live databases for what an
analyst asks first (which code values the data actually holds and since when, what else it holds,
implied nulls, keys, history), finds references the data shows but no code joins, asks the people
likely to know about what nobody explained, and records their answers as testimony — all without a
data value reaching the ledger. Five live database kinds (SQL Server and Informix are new), an inbox
for files and links people send, code-against-data checks, budgeted aggregate probes for agents and a
per-table "source passport" complete it. Measured on LedgerSMB against PostgreSQL and its ClickHouse
copy.

---

## RFC 0252 — Profiling for source understanding

### Problem / motivation
EnumMeanings said what code *mentions*; nothing said what data *holds*. Relationship inference was
seeded only by code joins, so a database without code (Informix in the target estate) yielded nothing.

### What was built
| Component | What |
|---|---|
| `observation-sdk::signals` | coverage (`CASE WHEN text(c) IN (literals) THEN text(c) END` + counts, first/last month), unexplained distinct, implications, key search, history patterns, reveal |
| `observation-sdk::overlap` | in-database MinHash (salted MD5, 16 minimums + span), Jaccard/containment, LSH bands, text inclusion SQL, name affinity |
| `ekos profile run\|show` | `<state>/profile/current.json`, `GrainCandidate`, history → `WriteBehaviour.data_history` |
| `semantic::data_usage` | `EnumMeaning.data_usage` / `implies`, `data_only_values` gaps |
| `ekos relationships discover\|list` | keyed `FkCandidate` (seed, verdict), cross-database `value_overlap` |
| review | optional subject `relationships` |
| SQL Server, Informix | live sources (subagents), same catalog/profile/probe surface |

### Implementation details worth remembering
- Coverage returns only literals or `NULL`, so the "unexplained" bucket is a count by construction.
- Signatures are computed by the database (MD5 is identical on PG, Oracle 12c+, ClickHouse, SQL Server
  over `varchar`), so no value is read into the process and cross-database comparison works.
- `data_usage`, `implies` are not signed; the data-only gap's evidence carries no counts. A test holds
  signatures equal across re-profiles with different counts.
- The `relationships` subject is optional (`Subject::is_optional`): not listed and not gate-required
  without items — adding it to `ALL` otherwise broke every project's workflow gate.

### Decisions
- Names stay a tie-breaker, but are *required* for key→key pairs and for parents that are dense id
  sequences. Measured: without it, LedgerSMB gave ~260 data candidates like `oe.id → payment.id`; with
  it, 9.
- No `LiveProfiler` trait (see the RFC log): a shared pure plan, each engine runs it its own way.

---

## RFC 0254 — Questions, code vs data, probes, passport

### What was built
| Component | What |
|---|---|
| `semantic::questions` | derive (8 types), priority, routing, status on read |
| `semantic::code_vs_data` | predicate_misses_values, dead_branch, range_assumption, lookup_not_unique |
| `ekos questions …`, `ekos check code-vs-data` | list/show --reveal/export/digest/reassign/decline/mute |
| `ekos probe`, `ekos_probe` | closed aggregate templates, budget, PII + small cells, audit |
| `.claude/agents/source-explorer.md` | hypothesis → probe → proposals; no write tool (tested) |
| `docs-gen::estate::passport` | 7 per-table sections (subagent) |

### Decisions
- Predicate sites now record `case_else` (`predicates/4`). Measured: every LedgerSMB `CASE` over
  `account.category` has an `ELSE` (sign flips) — the first version's 15 findings were all false.
- Routing state is a file, not the ledger: who a question is with is operational.

---

## RFC 0253 — People as a source

### What was built
Parsers for images (local Tesseract), XLSX/CSV, PPTX (subagent); `ekos inbox add|list|accept|purge`
(files and safe link snapshots, pending until accepted, compiled as source `inbox`); `ekos testimony
add|list|show|withdraw` (human-only; meaning source `testimony`; `testimony_mismatch` conflicts);
console pages, chat intake/proposals and Telegram/Slack/Teams adapters (subagent — see its RFC log).

### Decisions
- Standalone image OCR in a source tree is opt-in (`[ocr] images = false`); the inbox always reads
  images.

---

## Knowledge Captured

- **Personal-data scrubbing must be at the inbox boundary, not per parser.** Found live: a Markdown note
  in the inbox kept `cfo@example.com` because only the new image/sheet/slide parsers called
  `redact_personal_data`. `build.rs` now scrubs every inbox artifact (`redact_personal_json`).
- **A probe session refuses a writable role** (`ekos sql check`'s rule). The test fixture user `ekos`
  can write; create a read-only role (`ekos_ro`) for live probes rather than passing
  `--allow-writable-role`.
- **`BTreeMap` index panics**: `o.properties["x"]` on a `KirObject` panics when the key is missing
  (it is a `BTreeMap`, not a `serde_json::Value`). Use `.get()`. Crashed `relationships discover` once.
- **Sonar S7493** counts as a reliability *bug*: a blocking `std::fs` call inside an `async fn` turned
  the gate red (rating C). Use `tokio::fs` there.
- **Sonar's PL/SQL analyzer reads T-SQL fixtures** and reports `CHAR → VARCHAR2`; `deploy/fixtures/**`
  is excluded with the reason.
- Small integer surrogate keys make containment meaningless; density (`distinct ≥ 0.8 × span`) is the
  cheap test.
- Running `ekos build` in a chained shell command without the database password variable exported
  fails the whole chain at the live source — check before concluding a change "did not apply".

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/observation-sdk/src/{signals,overlap,probe}.rs` | new pure SQL builders |
| `ekos/crates/semantic/src/{data_usage,code_vs_data,questions}.rs` | new; fold, checks, questions |
| `ekos/crates/semantic/src/{business_semantics,review,workflow}.rs` | data fold, testimony meanings, `relationships` subject |
| `ekos/crates/cli/src/commands/{profile,relationships,questions,inbox,testimony,probe}.rs` | new commands |
| `ekos/crates/cli/src/commands/{commit,build,semantics,conflicts,review,mcp,catalogue,review_guard,app}.rs` | wiring, MCP tools, guards |
| `ekos/crates/recovery/src/sql_predicates.rs`, `ekos/crates/kir/src/predicates.rs` | `case_else`, `predicates/4` |
| `ekos/crates/compiler-core/src/config.rs` | `[questions]`, `[code-vs-data]`, `[testimony]`, `[probe]`, `[inbox]` fields |
| `ekos/plugins/{sqlserver,informix}/`, `ekos/plugins/localdocs/src/{image,sheet,slides,ooxml}.rs` | subagents |
| `ekos/crates/docs-gen/src/estate/passport.rs` | subagent |
| `web/api/app/…`, `web/ui/src/…` | console and channels (subagent), `mcp_policy.SCOPED_TOOLS` |
| `.claude/agents/source-explorer.md` | new agent |
| RFC 0252/0253/0254 §9, README, CHANGELOG, CLAUDE.md, TODO.md, capabilities page | docs |
