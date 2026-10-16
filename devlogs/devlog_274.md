# Devlog 274 — EKOS Lakehouse starts: RFC 0190, extension seam v2, a Trino dialect

**Date:** 2026-10-16
**PRs:** none (local branch `feat/lakehouse-m0-core`, merged to main locally)
**Branch:** feat/lakehouse-m0-core → main

---

## Summary

EKOS Lakehouse — generated, maintained PostgreSQL → Iceberg medallion pipelines (Trino,
Polaris, MinIO, dbt-trino, Dagster) — is designed (RFC 0190) and its core-side groundwork is in.
It will live in its own public repository as an extension, so the extension seam had to grow:
RFC 0191 lets an extension add whole command groups with catalogue classes and serve its MCP
tools to restricted readers. Core also learned Trino SQL (a dialect plugin the dbt analyzer
falls back to) and the canonical-hash expressions for Trino in `migrate-validate`. Nothing
Lakehouse-specific runs yet; the Trino hash expressions are reviewed, not verified on a live
engine.

---

## RFC 0190 — EKOS Lakehouse (design)

### Problem / motivation
The spec of 2026-10-15 asks for a product that keeps a lakehouse in step with an OLTP source:
bronze and silver generated deterministically from compiled source facts, gold drafted from
stakeholders' written descriptions under the anti-invention check, schema changes classified
and either applied or turned into pull requests with their impact, and every check a ledger
fact with its query. About half of it exists in Migrate (live catalog, profiling, DQ rules,
V1–V3 reconciliation, approvals, LinkML semantics).

### What was decided
| Question | Decision |
|---|---|
| Where the product lives | Separate **public** repo `alexeyban/ekos-lakehouse` (MIT), an extension built on `main_with`; binary `ekos-lakehouse` (contains all of core) |
| What core gets | What reads and understands: seam v2, Trino dialect, `Dialect::Trino`, registry rows, serde on drift shapes, `classify_with` |
| Migrate | Continues alongside; shared crates change additively only |
| Open questions of the spec | PostgreSQL only in v1; polling by default; Markdown + YAML header for marts; gold approved by stakeholder and data engineer |

### Decisions (alternatives considered)
- **In-tree next to Migrate** was offered and declined: Lakehouse stays a side project with
  EKOS as its core, releasing on its own schedule.
- **Private (open-core) like the binary decompiler** was the first draft of the plan; changed
  to public the same day.

---

## RFC 0191 — Extension seam v2

### Problem / motivation
RFC 0149's seam covers observers, passes, a post-commit step and MCP tools. A product needs
commands: `Cli`/`Commands` were private and `main_with` called `Cli::parse()` itself, the
command catalogue (RFC 0186) only knew its const table, and under a restricted read scope
(RFC 0181) every extension MCP tool was refused because `SCOPED_TOOLS` is a const.

### What was built
| Component | Change |
|---|---|
| `EkosExtension` | `commands()`, `command_classes()`, `run_command(name, matches, ctx)`, `mcp_tool_is_scoped(name)`; all default to today's behaviour |
| `CommandContext` | config (with `--project`/`--tenant` applied), cwd, config path, where the command was typed |
| `Extensions::extend_cli` | adds extension commands to the tree; refuses a taken name (built-in or another extension) and a leaf without a class |
| `app.rs` | `parse_cli` parses the extended tree once; an extension command becomes the `#[command(skip)]` `Commands::Extension { name, matches }` and goes through the normal project/config/logging path; `main_with_args` for tests |
| `catalogue_with` | `ekos commands --json` lists extension commands; the built-in table is consulted first, so an extension cannot reclassify a built-in |
| `mcp.rs` | the restricted-reader check also accepts an extension tool its extension marks scoped — never a built-in name |
| `EkosConfig::extensions` | `[extensions.<name>]` tables kept as given; `extension_settings::<T>(name)` parses one, naming the table on error |

### Implementation details worth remembering
- Clap: the derive can't parse a subcommand it doesn't know, so `parse_cli` checks the matched
  subcommand name first and builds `Cli` by hand from the global flags' ids (`config`, `home`,
  `project`, `tenant`, `read_scope` — the field names).
- `--read-scope` stays refused for extension commands (same rule as every command but
  `mcp serve`/`ekl`/`query`).
- `EkosConfig` is `deny_unknown_fields`, and `from_file_or_default` turns a parse error into the
  **defaults with only a warning** — a `[lakehouse]` section would silently have reset a whole
  project's config. Hence the one sanctioned place for extension settings.
- The console's MCP gateway keeps its own `SCOPED_TOOLS` and still refuses extension tools to
  restricted readers — the safe direction, recorded in the RFC as a known limit.

---

## Trino in core

### What was built
| Component | Change |
|---|---|
| `plugins/sql-dialect-trino` | `GenericDialect` + `preprocess` that blanks (keeping line breaks) time travel `FOR VERSION|TIMESTAMP AS OF <expr>`, `ALTER TABLE … EXECUTE …` statements and `TABLESAMPLE BERNOULLI|SYSTEM (n)`; registered as `trino` |
| `dbt_analyzer::parse_model` | falls back to the Trino dialect; pass version `v5` |
| `migrate-validate` `Dialect::Trino` | every expression: `CAST … AS varchar`, `to_iso8601(CAST(… AS timestamp(6)))`, `lower(to_hex(sha256(to_utf8(…))))`, `from_base(substr(h,1,15),16)`, `sum(CAST(… AS decimal(38,0)))`, `GROUP BY 1`; SQL snapshot `trino.sql.txt` |
| `migrate-logic` V5 | `run` returns NotRun for any engine pair other than PostgreSQL → ClickHouse |
| `migrate-target-clickhouse` | `classify_with(sql, &dyn Dialect)`; `classify` delegates |
| `migrate::drift` | `TableRef`/`TableShape` serialize, so a deployed snapshot can be stored and diffed |
| `custom_kinds::REGISTRY` | `LakehousePipeline/Table/Run/Change/Metric/Mart/Approval`, all keyed |

### Decisions
- Base dialect chosen by measurement, not by name: 20 Trino statements against generic, ansi,
  hive, postgres and duckdb. Generic parsed everything except three constructs that no dialect
  parsed; ansi and hive also failed `INSERT … current_timestamp` and window functions.
- The classifier must **not** run the dialect's `preprocess`: it blanks `ALTER TABLE …
  EXECUTE`, and a statement the classifier never sees could not be refused. `classify_with`
  parses the text as given; unparseable is refused.
- `Target::Iceberg` and a Trino `EngineKind` were not added: nothing uses them yet. They come
  with the M1 type map.

---

## Verification
- `cargo test --workspace --no-fail-fast`: 2,910 passed, 0 failed, 8 ignored (169 suites),
  then `compiler-core` + `cli` again after the settings table: 507 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`,
  `scripts/audit.sh` (three workspaces; only the existing allowed warnings).
- Not run: any live engine. The Trino canonical-hash expressions have not met a Trino server.

---

## Knowledge Captured
- **Trino can't `GROUP BY` an output alias**, and `sum(bigint)` **errors on overflow** rather
  than wrapping. The bucket queries group by position and sum a `decimal(38,0)`.
- **`format_datetime` is Joda-based and stops at milliseconds**; `date_format`'s `%f` prints
  milliseconds × 1000. `to_iso8601` of a `timestamp(6)` keeps microseconds.
- Trino string literals have no backslash escapes, so the null sentinel is `'\N'` as on
  PostgreSQL, not ClickHouse's `'\\N'`.
- **An exhaustive `match` on a public enum makes "add a variant" a cross-crate change**:
  `Dialect::Trino` broke four matches in `migrate-logic`'s V5 that no grep for `dialect.rs`
  would have found. Clippy over the whole workspace found them.
- `main_with_args` installs the global tracing subscriber (`.init()` panics on a second call),
  so a test binary may call it only once — the command test has its own file.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0190-lakehouse.md` | new: the Lakehouse design, where each part lives, resolved questions |
| `ekos/docs/rfcs/0191-extension-seam-v2.md` | new: extension commands, classes, scoped MCP tools |
| `ekos/crates/cli/src/extension.rs` | seam v2 hooks, `CommandContext`, `extend_cli`, owner lookup |
| `ekos/crates/cli/src/app.rs` | `parse_cli`, `main_with_args`, `Commands::Extension`, catalogue with extension classes |
| `ekos/crates/cli/src/commands/catalogue.rs` | `catalogue_with` |
| `ekos/crates/cli/src/commands/mcp.rs` | scoped extension tools for restricted readers |
| `ekos/crates/cli/tests/extension_commands.rs` | new: parse/dispatch, catalogue, collisions, missing class, no reclassifying |
| `ekos/crates/cli/tests/extension_seam.rs` | restricted reader: scoped tool served (and filtered), others refused |
| `ekos/plugins/sql-dialect-trino/` | new crate |
| `ekos/crates/recovery/src/{sql_dialect_registry,dbt_analyzer}.rs` | `trino` registered; dbt fallback + test; pass v5 |
| `ekos/crates/migrate-validate/src/dialect.rs`, `tests/sql_snapshot.rs`, `tests/snapshots/trino.sql.txt` | `Dialect::Trino` |
| `ekos/crates/migrate-logic/src/v5.rs` | Trino arms; NotRun without a lowering |
| `ekos/crates/migrate-target-clickhouse/src/classify.rs` | `classify_with` |
| `ekos/crates/migrate/src/drift.rs` | serde on `TableRef`/`TableShape` |
| `ekos/crates/kir/src/custom_kinds.rs` | `Lakehouse*` rows |
| `ekos/crates/compiler-core/src/config.rs` | `extensions` table + `extension_settings` + test |
| `ekos/Cargo.toml`, lockfiles of all three workspaces, `scripts/publish-crates.sh` | the new crate |
| `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | seam v2, Trino, Lakehouse M0 checklist |
