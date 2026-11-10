# Changelog

## Unreleased

**Features**
- **People as a source in the console** (RFC 0253/0254). An **Inbox** tab (pending material from
  people without the reviewer role, accept, purge with the id typed), a **Questions** tab (filter,
  reveal values live, record an answer heard elsewhere as the operator's testimony, reassign, "not
  me" by the addressee), "What people said" on entity panels, and in the Chat tab a *Tell* mode: the
  model proposes a card whose every line quotes the person's words (cite-or-drop), and only the
  author's **Save** records their own or edited text. Files and lone links go to the inbox.
- **Chat channels** (RFC 0253 §3.8). Telegram (long polling), Slack (Socket Mode or the signed
  Events API) and Microsoft Teams (Bot Framework, JWT checked) over one core: an unlinked account
  gets only the link instructions (`/ekos link` → a one-time code entered on the **Channels** page),
  owners bind channels to projects, answers use the person's read scope and go privately in a group
  unless every member sees the whole project, buttons count only from the account they were sent
  to. A questions digest (`channels_digest_cron`) sends each person their questions with
  **Answer** / **Not me**; nothing is sent from a public deployment. The service starts only when a
  channel's secrets (by variable name, environment or keychain) are present.
- **What the data says about the code** (RFC 0252 §3.2). `ekos profile run` measures, on every
  `[[databases]]` entry with profile p1/p2: how many rows hold each value code names and when each
  was first and last seen (`dead` after 24 months without a row), how many rows hold values *no*
  code names (counts only — a data value never leaves the database: the query returns a code literal
  or `NULL`), `column = v ⇒ other IS [NOT] NULL` implications, validity periods and soft-delete
  flags, and each table's grain (declared key, or a levelwise key search). `EnumMeaning`s gain
  `data_usage` and `implies`, columns with unexplained values get a `data_only_values` gap,
  `WriteBehaviour` gains `data_history`, every table a keyed `GrainCandidate`. Re-profiling never
  stales a review. `ekos profile show [TABLE]`; MCP `ekos_profile` (read-only, scoped).
- **References found in the data** (RFC 0252 §3.3–3.4). `ekos relationships discover
  [--cross-database]`: a salted MinHash signature of every comparable column, computed by the
  database itself, proposes references to unique columns; every code join is added; each pair is
  checked exactly by RFC 0158's inclusion query. Keyed `FkCandidate`s carry `seed: code | data |
  both` and the verdict; key overlap across databases becomes `value_overlap` on `SameAs`
  candidates (never a merge). A new optional review subject `relationships` (`ekos review
  relationships --confirm | --reject | --confirm-with-orphans`, human-only); MCP
  `ekos_relationships` (read-only, scoped).
- **Images, spreadsheets and slides as documents** (RFC 0253 Phase 1). The local-documents connector
  reads PNG, JPEG, TIFF and WebP through local Tesseract OCR (one section per recognised block, each
  with its `ocr_confidence`), XLSX and CSV (each sheet a section: header plus up to `[inbox] sheet-rows`
  rows, default 200) and PPTX (per slide: title, text and speaker notes; pictures through OCR).
  `[ocr] languages` (default `eng+rus+srp`) falls back to the packs actually installed with a warning;
  without Tesseract, images are skipped with a warning. E-mail addresses and payment card numbers in
  recognised text, cells and slides are redacted before anything is held
  (`ekos_common::redaction::redact_personal_data`). Pipeline logic version 6: the next `ekos build`
  re-reads every source once. Standalone image files in a source tree are read only with
  `[ocr] images = true`.
- **Profiles for every live database** (RFC 0252 §3.1). `[[databases]] profile = "p1" | "p2"` now works
  for PostgreSQL and ClickHouse as well as Oracle: P1 counts each table's rows, P2 scans each table once
  for exact null counts, number and date bounds and the longest text — never a text value, never a
  bound of a column whose name says personal data. PostgreSQL also gets P0 (the planner's row and
  column estimates, from `pg_class` and `pg_stats` counts only); ClickHouse P0 is `system.tables`'
  row total, and only engines that store their own rows are scanned (no `SELECT` from a Kafka,
  RabbitMQ, S3Queue, MySQL, S3 … table). One planner (`ekos_observation_sdk::profile`) renders the
  queries for all three engines. The console's Sources form offers the tier for every kind.
- **SQL Server as a live database source** (RFC 0252 §3.5, Phase 3). `[[databases]] kind = "sqlserver"`
  (`connect = "host[:port]/database"`, optional `?trust-server-certificate=true`; `owners` are schemas)
  reads the `sys.*` catalog through `tiberius` (rustls): tables with keys, unique indexes, checks and
  foreign keys, `MS_Description` comments, views, procedures, functions and triggers from
  `sys.sql_modules`, reconciled against `sys.objects`, compiled to the same `origin: live` objects as
  the other kinds (T-SQL routines at signature fidelity, with read/write footprints). Read-only without
  a session flag: `ApplicationIntent=ReadOnly`, every statement in an always-rolled-back transaction,
  lock and command timeouts, and a permission-based write check. Profiles P0 (`sys.partitions`
  estimates), P1 and P2; `ekos database add|test`, `ekos sql check --database` and the console's
  Sources form accept it. Testbed: `docker-compose.sqlserver.yml`.
- **Informix as a live database source** (RFC 0252 §3.5, Phase 4). `[[databases]] kind = "informix"`
  (`connect = "host[:port]/database@server"` for the IBM Informix Client SDK over SQLI, or
  `drda://host[:port]/database` for the IBM Data Server Driver over DRDA; `owners` are table owners)
  reads the system catalog over ODBC (`odbc-api`, behind the `ekos` build feature `informix`, off by
  default): tables with columns decoded from `coltype`/`collength`, defaults, primary and unique keys,
  unique indexes, checks, foreign keys, view text from `sysviews`, SPL routine text and `DOCUMENT`
  clauses from `sysprocbody`, trigger text from `systrigbody`, reconciled against the catalog's own
  counts. SPL routines compile as `Procedure` objects **without IR** (no Informix SPL reader yet;
  warning `IFXLIVE003` and the recover summary say how many) with read/write/call footprints from
  their SQL statements. Read-only: autocommit off and every statement rolled back, `SET TRANSACTION
  READ ONLY`, `COMMITTED READ LAST COMMITTED`, a lock wait and a query timeout, and a rights-based write
  check (`systabauth`, `syscolauth`, roles, `sysusers`). Profiles P0 (`systables.nrows`), P1 and P2;
  `ekos database add|test`, `ekos sql check --database`, `ekos profile run` and the console's Sources
  form accept it. Informix has no MD5, so its columns get no overlap signature (RFC 0252 §3.4).
  Testbed: `docker-compose.informix.yml` + `deploy/fixtures/informix/load.sh`.
- RFCs 0252 (profiling for source understanding), 0253 (people as a source) and 0254 (the analyst
  loop) accepted.

**Security**
- **`rusqlite` 0.32 → 0.41** (bundled SQLite via `libsqlite3-sys` 0.30 → 0.39) for
  RUSTSEC-2026-0459; `cargo audit` is clean again in all three workspaces. rusqlite now needs
  `&mut Connection` to install a progress handler, so the benchmark's `Db` installs its query
  deadline once at open and each query sets the deadline (`Db::with_deadline`).

## v1.4.6 — 2026-11-09

**Features**
- **Console: a read-only showcase** (`read_only`, `EKOS_CONSOLE_READ_ONLY`; RFC 0251 amendment 1).
  Everyone, owners and tokens included, may only read: runs, schedules, reviews, ekos.toml, Admin
  and Settings are refused, and so are Commands and Chat, which act with read rights alone. The UI
  hides them; MCP reads keep working for BI tools.

## v1.4.5 — 2026-11-09

**Fixes**
- **`ekos_sql_generate` never returns SQL that writes** (RFC 0250 amendment 2). Asked to "delete
  all invoices of Joe Block", the agent wrote a DELETE and the tool returned it with an open finding;
  a client without its own guard could have run it. A write — parsed, or recognised by its verbs when
  the dialect's parser cannot read it — is now answered `status: "refused"` with a reason and no SQL.

## v1.4.4 — 2026-11-09

**Features**
- **`ekos_sql_generate` takes `tables`** (RFC 0250 amendment 1): the tables a caller lets its users
  query, up to 500. The agent sees only those tables and the joins among them; names EKOS has not
  compiled come back in `tables_unknown`. A BI tool's allow-list no longer turns a good question into
  a refused query over some other table.

## v1.4.3 — 2026-11-08

**Fixes**
- **`ekos_ossie_export` builds the whole model once.** The MCP server keeps it per store state,
  status, database and read scope; `match`, `max_datasets` and the new `names` only narrow it.
  A BI tool that asks for a few tables at a time used to walk the whole store on every ask
  (minutes each on a network file share); on the 1,233-table iDempiere project the first call
  takes 40 s and every later one milliseconds.
- **`names`**: `ekos_ossie_export` takes exact table names (case-insensitive, up to 500), so a known
  set of tables comes back in one call.

## v1.4.2 — 2026-11-08

**Fixes**
- **The MCP server keeps the SQL schema between calls.** `ekos_sql_check` and `ekos_sql_generate`
  rebuilt it from the whole store on every call (20–25 s on a 140,000-object project); it is now
  built once per store state and read scope and dropped when the store changes (0.003 s for a
  repeat check). `ekos_ossie_export` is classified expensive, so a repeated export comes from the
  result cache (37 s → 0.025 s).
- **Console:** `mcp_call_timeout_seconds` (default 30) — how long the console waits for one reply
  from a project's MCP server; a large project's first schema read can need more.

## v1.4.1 — 2026-11-08

RFCs 0250–0251, devlog 329.

**New capabilities**
- **`ekos_sql_generate`** (RFC 0250): EKOS's own text-to-SQL agent (`ekos sql generate`, the one
  measured on BIRD) as an MCP tool, with read-only probes on the project's live database. The first
  MCP tool that calls a model, so it is off unless `[sql-generate] mcp = true`; a provider other than
  a local Ollama model also needs `allow-metered = true`; a daily cap per project (default 50 calls,
  200 000 tokens) is counted before the model is called. Through the console gateway the tenant must
  allow metered calls and each caller has a daily allowance (`mcp_metered_per_person_per_day`,
  default 5; `mcp_metered_per_token_per_day`, default 50). The console chat never offers it.
- **Sign-in by the platform** (RFC 0251): `platform_auth = "aca"` lets Azure Container Apps'
  authentication (GitHub, Google, …) sign people in to the console; the person is `<provider>:<name>`
  and API tokens keep working. `signed_in_group` puts everyone who signs in in one group, so a
  `viewer` binding opens a project to a public audience.

## v1.4.0 — 2026-11-08

RFCs 0245–0249, devlogs 324–328.

**New capabilities**
- **Apache Ossie interchange** (RFC 0245/0246): `ekos export ossie` / `ekos import ossie`
  (import is human-only), Ossie files ingested as knowledge, and the resulting synonyms, joins and
  metrics used by text-to-SQL, dbt-semantic export, business semantics and search.
- **Annotations on any entity** (RFC 0247): comments, instructions and synonyms attached to a table,
  column, mart or anything else in the ledger. Validity is computed from the entity's digest, so a
  note goes stale when its target changes. `ekos annotate add|list|show|withdraw|approve|confirm`;
  writes are human-only, the MCP tool `ekos_annotations` is read-only. `ekos annotate resolve --rule`
  decides conflicts in bulk (dry run unless `--apply`; a rationale and a matcher are mandatory).
  Console: an Annotations tab and a Notes panel on every object.
- **Public-demo safety** (RFC 0249, console backend, off unless `moderation_enabled`): deterministic
  injection / secret / hidden-text / forbidden-schema / abuse signals, standing (ban, temporary block),
  daily limits, per-IP rate limit, an optional model intent check that can only add refusals and fails
  closed, and an owner log with unban.
- **Azure module**: `allowed_cidrs` ingress restriction and `external_secrets` (Key Vault secrets a
  person adds, referenced by managed identity, never held by Terraform).
- Release workflow: build provenance attestation and a glibc install check.

**Fixes**
- The SQL statement splitter honours quotes, comments and dollar-quoting instead of splitting on `;`.
- Duplicate memory-path boost removed from search; truncation detection for Ollama uses the prompt
  token count.

## v1.3.0 — 2026-11-05

RFCs 0242–0244, devlogs 319–320.

**New capabilities**
- **Writing and checking SQL against compiled knowledge** (RFC 0242/0243):
  - `ekos sql check [FILE] [--database NAME | --sqlite FILE]` lints a query against the ledger's
    tables, columns, keys and foreign keys and the dialect's rules. With a database it also runs
    read-only probes: the result, value spellings, rows repeated by joins, ties.
  - `ekos sql generate "<question>"` is a tool-using agent whose every draft is checked before
    it is final.
  - The read-only MCP tool `ekos_sql_check` gives a dialect's writing rules, or checks a drafted
    query.
  - Every SQL dialect plugin states its engine's facts and generation notes.
  - The PostgreSQL, ClickHouse and Oracle plugins run probes behind a single-SELECT guard and a
    read-only session. PostgreSQL also refuses a role that can write.
  - A `text-to-sql` skill for Claude Code.
- **Text-to-SQL agent on BIRD** (RFC 0242):
  - The cells `strategy = "loop" | "agent"` add `verify`; `ekos eval bench checks` replays the
    checks over a report.
  - BIRD Mini-Dev hold-out, Claude Haiku 5.5: 76.2% with the agent, against 53.3% on raw DDL.
  - On 100 tune questions, Claude Sonnet 5.5 with the agent scored 87.0%.
- **BIRD-Test submission support** (RFC 0244):
  - suite `bird-dev` (BIRD New Dev 2025-11-06);
  - test-set questions without gold SQL are predicted, not scored;
  - `eval bench run` checkpoints every answer, and `--resume` continues an interrupted run;
  - profile queries have a deadline, for giant databases;
  - `eval bench run --questions <ids>`.
- **SQLite dialect plugin** (`plugins/sql-dialect-sqlite`).

**Changes**
- SonarCloud analysis runs in CI on every push to `main` and weekly, with the quality gate
  enforced (`.github/workflows/sonar.yml`).

## v1.2.0 — 2026-11-04

RFCs 0190–0241, devlogs 273–318. Upgrading: the first `ekos commit` after upgrading writes one
round of new object versions (evidence ids are now derived from content), and `ekos recover`
re-runs passes whose versions changed.

**New capabilities**
- **Benchmarks** (RFC 0240 / 0241): `ekos eval bench list|prepare|run|compare|export`. BIRD
  Mini-Dev 1024 is checked in as a suite. `prepare` verifies the pinned archives, builds one
  project per database and scans every ledger for leaked questions, gold SQL and evidence. `run`
  scores by execution like the official scorer (parity checked on 704 predictions), with bootstrap
  intervals, outcome categories, and billed vs cached tokens. `compare` is a paired comparison
  (bootstrap + McNemar); `export` writes BIRD's prediction format; `context` prints what a cell
  shows the model. With Claude Haiku 5.5 on the hold-out split, the EKOS cell scored 63.9%
  against 53.3% for raw DDL; with empty-string profiles, 66.9%.
- **Live PostgreSQL and ClickHouse as project databases** (RFC 0235), next to Oracle (RFC 0211):
  `[[databases]] kind = …`, `ekos database add|list|test|remove`, a read-only catalog reconciled
  against the database's own counts.
- **Oracle / iDempiere estates** (RFCs 0209–0228): a Java analyzer and application model, Oracle
  SQL and PL/SQL, schema history, a live Oracle catalog with profiles, write behaviour per table,
  tickets. Also the seven-step workflow: review and digest-bound sign-off (`ekos review`,
  `ekos workflow status`, `ekos_signoff_status`), source-estate documentation
  (`docs generate --layout estate`), run logs for every changing command, and an iDempiere exit
  eval.
- **Cross-reference integrity** (RFC 0233): `ekos check refs` (broken references, cycles,
  reachability, SARIF), `ekos check explain`, the read-only `ekos_xref_check` MCP tool.
- **Clouds** (RFC 0232): `ekos home backup --to` / `restore --from` (S3, Azure Blob, GCS),
  Terraform modules for Azure, Google Cloud and AWS (VM, containers, Kubernetes), a Helm chart, a
  console instance lease, scheduled backups, git-synced sources.
- **LLM routing** (RFC 0197): routes per task, OpenCode Zen wire formats, a usage log,
  `[llm] budget-usd`, a data policy.
- **Extensions** (RFCs 0190–0207): command groups and console pages from extensions, extension
  binaries under their own name. EKOS Lakehouse and the Pentaho translator live in their own
  repository.

**Fixes and changes since v1.1.0**
- **Fixed:** `COMMENT ON TABLE|COLUMN` never reached a table or column declared with a quoted
  identifier (`CREATE TABLE "orders" ("status" …)`): the descriptions were dropped without a
  diagnostic. Takes effect at the next `ekos recover` (`SqlAnalyzerPass` v6).

- **Security / external review (issues #10–#20, devlog_283):**
  - **Fixed:** redaction gaps. Crate `//!` doc comments were sent to the LLM unredacted, `[llm-description]` re-read files excluded by `[security]`, and `git blame` commit subjects reached the ledger unredacted. Every re-reader now goes through `ekos_common::redaction::read_redacted` (exclusion, then redaction), and blame summaries are redacted.
  - **Fixed:** evidence ids were random, so any pass re-run on a cache miss wrote a new version of every object it touched. They are now derived from content. Expect one round of new versions at the first commit after upgrading.
  - **Fixed:** several compiled edges (`FeedsInto`, `Disputes`, risk `References`, business-semantics links) and `Rollup` objects carried no evidence.
  - **Fixed:** `ekos_dependents`, `ekos_impact` and `ekos_neighborhood` walked onto unconfirmed hypotheses and migration proposals as if they were facts. They skip them until a person confirms, and `ekos_dependents` shows the other end's `status`.
  - **Changed:** confirming a treasury payment authorization (`AuthorizedBy`) is human-only. Use `ekos treasury confirm|reject <id>`; `ekos_identity_review` now reviews `SameAs` only.
  - **New:** `ekos_identity_review` / `ekos_architecture_review` take optional `reviewer` and `note`, recorded on evidence that the decision event cites. The ClickHouse audit event has a deterministic id.
  - **Changed (API):** `KnowledgeStore` is split into `KnowledgeReader` + `KnowledgeStore: KnowledgeReader`, and `Runtime::new`/`over` and `export_graph` take a reader. Code that calls reads on a concrete store type, or writes `KnowledgeStore::get_object(…)` in full, needs `KnowledgeReader` in scope.
  - **New:** a pipeline test fails if any committed object or edge kind has no evidence (`evidence_everywhere.rs`). Its first run found and fixed Rust `Contains`/`Calls`/import edges, `File → View` edges, `Crate`, `PythonModule`, `RustModule` and `Technology`. Takes effect at the next `ekos recover` (pass versions bumped).
  - **Changed:** `only_listed_functions_can_write_the_ledger` now parses `mcp.rs` (aliases, method calls and macro arguments resolved) instead of matching text. RFC 0056 states that a repeated identical ClickHouse query is one event id with a new version per run.
  - Docs: RFC 0007 amended to describe the shipped identity algorithm; amendments to RFCs 0032, 0043, 0056, 0135 and 0149.
- **Fixed:** a chained run's stages (pipeline, Init & compile) had the 30-minute limit of a single command, so a large project's commit was killed (exit 124). They now take `run_stage_timeout_seconds` (default 6 hours, Settings → Runs).
- **Fixed:** Python import / contains / extends edges and git ownership / co-change edges now carry evidence (file and line, or the commits). `ekos coverage` reported "Python: 0 relationships" and "Git history: 0 relationships" for projects with thousands of edges, and none could be cited. Takes effect at the next `ekos recover` (analyzer versions bumped).
- **Fixed:** LLM descriptions use `[llm] max-tokens` (default 4096) instead of a hard-coded 1024; a reasoning model ran out of tokens before finishing its answer.
- **Fixed:** `install.sh` replaces a running `ekos` (it failed with "Text file busy" while a pipeline stage or an MCP server ran); `install.ps1` no longer deletes half the program when EKOS is running.

## v1.1.0 — 2026-10-14

RFCs 0154–0189, devlogs 204–272. The first release with **full packages**: the `ekos` command,
the web console and a pinned Python for Linux (x86_64, aarch64), macOS on Apple silicon and
Windows 11 (Intel Macs get the command only: `cryptography` has no Intel macOS wheels), installed by `install.sh` / `install.ps1` with a setup wizard in the browser
(`ekos-<version>-<target>-full`). The command-only archives are published as before, and the
console image as `ghcr.io/alexeyban/ekos`. Programs are checksummed, not code-signed.

**New capabilities**
- **Uninstall in the browser** (RFC 0188): `ekos uninstall --ui` serves a page (loopback, one-time token) listing each step and the projects in the home with their size; the person chooses whether to keep the projects and types `uninstall` (or `delete everything`) to confirm. The console's Settings page has **Uninstall EKOS** in local mode for an install made by `ekos setup`; it starts the page in its own process (`--detach`, through `systemd-run --user` under the login service), so the page outlives the console it removes.
- **Init & compile** (RFC 0189): one button in a project's bar runs init (in-place workspaces), build, recover, resolve (identity conflicts reported, not stopping), compile, commit and coverage as one run. Chained commands now carry their stage arguments (`stage_args`).
- **Chat with a project** (RFC 0187): a Chat tab answering with a configured model (Ollama, OpenAI-compatible, Anthropic) that uses the project's reading EKOS tools with the person's rights and read scope, every call shown and audited; a hosted model only in a tenant with metered calls on; conversations per person; a Chat step in `ekos setup` that checks a model without a paid call. New console settings `chat_*`.
- **Every command and setting from the console** (RFC 0186): `ekos commands --json` (every command, its arguments and a class deciding who may run it from the web) and `ekos config schema --json` (the JSON schema of `ekos.toml`). The console's new Commands tab runs any command but servers and the installer, as a validated argv; the config page gets a Settings form for every section that keeps the file's comments.
- **Setup wizard, console launcher, full packages** (RFC 0185): `ekos setup` (a wizard in the browser, or `--answers` without one), `ekos console start|stop|restart|status|open|logs`, `ekos uninstall`; full packages with the console and a pinned Python for Linux, Windows 11 and macOS (`scripts/package-full.sh`, a `package` job in the release workflow); `install.sh` installs the full package when a release has one and opens the wizard, `install.ps1` does so on Windows. **Changed:** with no session secret set, a console with a home keeps a generated one in `control/session.key`, so a restart no longer signs everyone out (production mode accepts it).
- **Console settings from the console, local mode, OS keychain** (RFC 0184): a platform owner edits the console's settings on a new Settings page, saved in `<home>/control/console.toml` (an environment variable still wins and shows read-only); secrets and model keys typed there go to the OS keychain and reach only the commands of a tenant granted the name. New local mode for one person on one machine: loopback only, `Host` checked, sign-in by a one-time code written into the home. The console's child processes now get the variables Windows needs; the UI directory is a setting (`EKOS_CONSOLE_UI_DIR`). **Changed:** the image sets `EKOS_CONSOLE_KEYCHAIN=off` and `EKOS_CONSOLE_SUPERVISED=true`.
- **Offline install package** (`scripts/package-release.sh`): builds `dist/ekos-<version>-release.tar.gz` for Linux x86_64 from the committed tree, with the glibc and the static musl binary packed as a release packs them, `SHA256SUMS`, `install.sh` and an `INSTALL.md`, and checks that it installs. Installed with `EKOS_VERSION=v<version> EKOS_BASE_URL="file://$PWD" sh install.sh`. The deployment guide adds loading the console image on a server without internet access.
- **Projects in an EKOS home** (RFC 0178): `ekos project add <name> --source <dir>` creates a project whose config and compiled data live under `~/.ekos/data/<tenant>/<project>/` (`EKOS_HOME`, `--home`), and `--project <name>` runs any command against it from any directory. The sources are never written to. Each project has its own ledger; `--tenant` separates tenants. A workspace used in place is unchanged.
- **Console fixes from a browser, a group claim and an MCP client** (`deploy/e2e/run.sh`, now 143 checks): opening or reloading the page of a workspace in a tenant (`/w/<tenant>.<project>`) was a 404, because the dot was read as a file extension; new setting `EKOS_CONSOLE_OIDC_SCOPES` (default `openid email profile`) so providers that send group membership only for the `groups` scope can feed claim-mapped groups; a limited viewer's graph page no longer requests the statistics and conflicts it is refused.
- **Console sign-in fixes** (found by the end-to-end deployment check, `deploy/e2e/run.sh`): sign-in now answers 502 with the cause when the identity provider cannot be reached and 400 when a sign-in is cancelled or finished in a browser that did not start it (each was a 500); the session cookie is marked `Secure` when the redirect URI is `https://`. The deployment guide's restore steps now work on a new volume (`--force`).
- **Backup, restore and per-tenant model credentials** (RFC 0183): `ekos home backup` / `ekos home restore` (one zstd tar; projects copied under their write lock, the console database as a consistent snapshot; restore replaces nothing without `--force` and then keeps the old in the trash). In the console each tenant's commands receive only the model-key variables an owner named for it, and only while its metered switch is on. **Changed:** every `ekos` process the console starts no longer inherits the console's own `EKOS_CONSOLE_*` settings; outside production mode the `default` tenant otherwise keeps its environment. A deployment guide is at `docs/deployment.md`.
- **MCP gateway on the console** (RFC 0182): `POST /mcp/<tenant>/<project>` with an API token, MCP Streamable HTTP. Tools are filtered by role (no token reaches the review tools), every call carries the caller's read scope and is recorded in the audit log, and a scope sent by the client is dropped. **Changed:** a project's MCP server now starts on first use, stops after 15 idle minutes and at most 8 run at once (`EKOS_CONSOLE_MCP_IDLE_SECONDS`, `EKOS_CONSOLE_MCP_MAX_SERVERS`); the workspace list shows `idle` for a stopped one.
- **Access inside a project** (RFC 0181): a read scope of allow / deny rules by source, path pattern or object, enforced where the ledger is read, so every read path is covered. `--read-scope <file>` on `ekos mcp serve`, `ekos ekl` and `ekos query`; per call over MCP in `_meta["ekos/readScope"]`. In the console a viewer can be limited to part of one workspace or have part of it hidden; they get the graph and search, filtered, and 403 on the pages that show more than the ledger. Under a restricting scope the MCP server refuses every tool that does not read solely through the ledger. Objects with no recorded source are hidden from a restricted reader.
- **Tenants, users, groups and roles in the console** (RFC 0180): four roles (`viewer`, `reviewer`, `operator`, `admin`) granted per tenant or per project, platform owners from `EKOS_CONSOLE_OWNERS`, groups (by membership or by an OIDC claim value), API tokens for agents (viewer or operator only, stored as a hash, shown once), a per-tenant audit log, and per-tenant source roots. A workspace or tenant the caller has no role in answers 404. `EKOS_CONSOLE_PRODUCTION=true` refuses development credentials. Outside production mode the `default` tenant behaves as before. **Changed:** creating or removing a workspace now needs `admin` (the static write token still can); review endpoints need `reviewer`, which an API token cannot hold.
- **Container image and installer into the EKOS home** (RFC 0179): one image with the `ekos` binary, the console API and the console UI (`Dockerfile`, `deploy/docker-compose.yml`), state in one volume, sources mounted read-only. The console creates and runs projects kept in the home, and limits them to the mounted source directory. `install.sh` now installs to `~/.ekos/bin` (`EKOS_HOME`); an earlier install in `~/.local/bin` is upgraded where it is, and `EKOS_INSTALL_DIR` still wins. Tagged releases will publish `ghcr.io/alexeyban/ekos`.
- **Named sources** (RFC 0178): a project can be several repositories and directories (`--source <name>=<dir>`, `ekos source add|list|remove`), compiled into one ledger. Every object records its `source`; EKL filters on it; `ekos build --source <name>` re-observes one. A source declared `git` with no `.git` is reported instead of silently yielding no commits.
- **EKOS Migrate** (RFC 0154–0162, opt-in `[migrate]`): PostgreSQL → ClickHouse.
  - **Assess:** live catalog and profiling (PII suppressed, read-only sessions), drift against the repository's DDL, data-quality and compatibility rules that measure affected rows, inferred foreign keys from real code joins and `pg_stat_statements`.
  - **Load:** type mapping with lossiness, a ClickHouse design and generated DDL, a statement classifier with guarded execution, human-only approvals pinned to evidence.
  - **Validate and report:** V1–V3 validation, a compiled and citation-verified report. Verified live on a real table.
- **Logic lowering** (RFC 0164), for PostgreSQL views → ClickHouse:
  - `ekos migrate lower` lowers a view by rule and lists each rewrite, or blocks it by a named rule. The output must parse on the target, read back identically, and state nothing the source does not.
  - `ekos migrate v5` runs each lowered view against its source on a PostgreSQL and a ClickHouse sandbox, over a fixture built from the view's own comparisons, and reports branch coverage with the verdict. Both connections must be marked `sandbox = true`.
  - `ekos migrate reconstruct` sends one fully recovered PL/pgSQL routine to a model and keeps the answer only if it invents nothing. A model that is not local needs `--allow-metered` per run.
  - `ekos migrate check-candidate` runs the anti-invention check on a statement you supply.
  - Triggers get a proposed replacement per class; `--record` saves proposals as `MigrationProposal`, status `proposed`.
  - Not built: V5 for routines, approval of proposals.
- **Stored procedures, views and triggers** (RFC 0163, 0169): every PL/pgSQL routine is parsed (LedgerSMB 212/212) and linked to the tables it reads and writes and the routines it calls. Views become ledger objects. Triggers are classified by what their function does. All three are documented like code.
- **Business semantics → LinkML** (RFC 0170, opt-in `[semantics]`):
  - concepts, code meanings, constraints and gaps recovered from SQL, dbt, Perl, Python and glossaries;
  - a human-only review loop;
  - `ekos export linkml` / `ekos import linkml`;
  - MCP `ekos_semantics_lookup` / `ekos_semantics_gaps`;
  - a web console Semantics tab;
  - a reproducible LedgerSMB demo (`demo/ledgersmb-linkml/`).
- **Source freshness** (RFC 0171, on by default): `ekos status` / `doctor` / `freshness` say whether the ledger still matches the source, and which compiled objects cite changed files. `FRESH001` at compile/commit; MCP `ekos_status` carries the same report. Opt-in `[freshness] content-hash = true` stops a `touch`ed file from counting as changed.
- **ConflictingEvidence** (RFC 0172, on by default): when sources disagree, both claims are kept, never the last one silently. The detectors cover:
  - duplicate definitions;
  - merge losses;
  - label mismatches;
  - documentation vs data in `ekos migrate assess` (`DQ.CONSIST.DOC`): claims in column comments, and in Markdown or Confluence sentences that name one `table.column`.

  Resolving is human-only through `ekos conflicts list|show|resolve`; MCP has a read-only `ekos_conflicts`.
  An `ekos_query` / `ekos_retrieve` claim about an attribute with an open conflict carries `disputed: true` and the conflict ids, and `ekos ask` tells the model the same (RFC 0173). A label conflict is also linked to its column's table. The web console has a Conflicts tab and rings disputed objects in the graph (RFC 0174).

**Fixed: results that were wrong without saying so** (RFC 0175, 0176)
- **EKL** filters, sorts and groups on properties (`WHERE status = 'confirmed'`, nested `a.b`). A field no candidate has is an `unknown field` error; it used to return 0 rows.
- **SQL schemas:** one statement the dialect cannot parse no longer discards every table in the file. `SQL005` lists what was skipped; `SQL001` names the dialect and the error when a file states tables and yields none. `ALTER TABLE … ADD … FOREIGN KEY` becomes a foreign-key edge. LedgerSMB: 184 → 197 tables, 218 → 229 foreign keys.
- **Migrate:** `DQ.UNIQ.001` no longer blocks on foreign-key columns. Inferred keys attribute each join column to the table its qualifier names (it proposed `country.country_id → entity.id`).
- **Treasury:** explorer reads are paged; a history past 10,000 rows used to be cut short silently. One that cannot be read whole is now an error.
- **Evals:** `ekos eval run --agent ollama` works under a cloud `[llm]` config.

**Search and answers** (RFC 0177)
- **A symbol's doc comment and signature are searchable.** They were not indexed, so a question that described a function found every document quoting its comment and not the function. Existing workspaces re-index on the next command that writes to the ledger (`ekos ledger repair` does it at once).
- **Partial matches** are ranked by how many of the question's words they hold as well as by BM25.
- **A search hit shows the passage that matched,** not the opening of its text.
- **`ekos ask` shows a symbol's source.** The lookup behind "full source text" found nothing on a real ledger; it now falls back to the source recorded with the symbol.
- **"What depends on module M"** includes what depends on M's items.
- **A file is found by its name** without directory or extension.
- **A question built on a false premise** is answered `Premise not supported.` and a cited correction. A question whose answer is not in the evidence is answered `Insufficient evidence.`, followed by what the evidence does show.
- **Rust struct and enum symbols record their fields** (after the next `ekos recover`).
- **Evals:** `ekos eval run --evidence-only` measures retrieval without calling a model. A run with an errored scenario fails. Refusals are counted apart from uncited answers. Recall@10 counts an expected name as found when any object bearing it is ranked.

**Fixes**
- Console: a project's MCP server no longer takes a port another program already listens on. Two consoles on one machine both counted up from 7400, and the second's server "never became ready".
- Console on Windows (found by the first Windows CI run): reloading a project page answered 404 and an unknown `/api/...` path got the app (paths arrive with `\`); a run log showed an empty line after every line (`\r\n`); a project's sources were stored as `\\?\C:\...` and then refused as outside the allowed root.
- Web console: rows in the Semantics and Conflicts lists can be opened from the keyboard. Applying an edited LinkML schema no longer blocks other requests while the file is written.
- `ekos resolve` no longer stops on same-name groups across the database and code namespaces (LedgerSMB 16 → 0 conflicts).
- Compile output is now deterministic when two files define the same object: artifacts are read in source order, not timestamp-hashed id order.
- The MCP server refuses argument names a tool does not declare. Plus audit fixes: a forgeable console session, bounded MCP traversal, panic isolation, a rustls advisory.
- Business semantics no longer reads a `CASE` that rewrites a code (`'A'` → `'L'`, a sign flip) as that code's label; a branch's result is a label only when it depends on the code alone (LedgerSMB 5 → 3 conflicts).
- 14 EKOS Migrate defects found running it on LedgerSMB, plus 3 cross-engine validation defects found against live engines.

## v1.0.3 — 2026-09-24

**No functional change to the `ekos` binary.** Not one `.rs` file differs from v1.0.2, so these
binaries behave identically. If v1.0.2 works for you there is nothing here to upgrade for.

This release exists to exercise the release pipeline itself. The `release` job only runs on a
tag, so two changes made to it — per-job least-privilege permissions, and a rewritten checksum
step that validates every archive contributed a `SHA256SUMS` line — had never actually executed.
Proving them on a release that cannot regress anything is cheaper than discovering a broken
release job during one that matters.

Everything else since v1.0.2 is the web console and repository tooling, none of which ships in
the binary:

- Web console: three security fixes (absolute paths disclosed in API errors, a non-atomic
  symlink-following config write, an unbounded read/parse) and several reliability fixes — most
  importantly a job runner whose worker task could die permanently on one transient database
  error, silently stalling every future run on that workspace. Also removed all blocking file I/O
  from the event loop, which could stall the whole API on a slow disk.
- CI workflow now declares `contents: read` rather than inheriting repository defaults.
- SonarCloud analysis excludes demo material.

## v1.0.2 — 2026-09-24

A packaging release. **No code changes since v1.0.1** — not one `.rs` file differs — so the
binaries are functionally identical and there is no reason to upgrade for behaviour.

What it does contain is the metadata that makes the workspace publishable to crates.io: every
crate now carries `license`, `repository`, `homepage` and `rust-version`; every internal
dependency carries a version; and the eight crates that should not be on the registry are marked
`publish = false`.

It exists as its own version so that the registry, the git tag and the release binaries all
describe the same tree. Publishing the crates under `1.0.1` would have put content on crates.io
that did not match the `v1.0.1` tag, and a published version can never be replaced.

## v1.0.1 — 2026-09-23

A packaging fix. No code changes to EKOS itself; if `ekos --version` already works for you, there
is nothing here you need.

**The Linux x86_64 binary in v1.0.0 would not start on most machines.** It was built on Ubuntu
24.04 and therefore required `GLIBC_2.39`, so it failed on Ubuntu 22.04, Debian 12 and RHEL 9 with:

```
/lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

Two fixes, because either alone leaves a gap:

- The glibc builds are now produced on the **oldest supported runner** (Ubuntu 22.04, glibc 2.35)
  rather than the newest, for both x86_64 and aarch64. A glibc binary runs on that version or
  newer, so building on the newest image is exactly backwards.
- `install.sh` now **runs the binary it downloaded** before installing it, and on x86_64 Linux
  falls back to the fully static musl build if it will not start. Checking empirically covers
  every reason a build might not run on a machine, not only the glibc one.

If you installed v1.0.0 on Linux and it did not run, re-running the install command is enough.

## v1.0.0 — 2026-09-23

The first tagged release of EKOS, and the first one you can install without a Rust toolchain.

EKOS is a **compiler for enterprise knowledge**. It observes the systems you already have —
source code, Git history, SQL schemas, GitHub issues, Confluence, local documents, compiled
binaries — compiles them through deterministic passes into a canonical knowledge model, and
serves that model to AI agents over MCP, with the evidence attached to every conclusion.

This is the `v1.0` row of the versioning roadmap in the README: the Enterprise Knowledge
Compiler, end to end.

### Install

```bash
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh
```

Then, in any repository:

```bash
ekos init --detect
ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
ekos coverage
ekos ask "what does this system do?"
```

Prebuilt binaries: Linux x86_64 (glibc and static musl), Linux aarch64, macOS on Apple Silicon
and Intel, Windows x86_64. Every asset is listed in this release's `SHA256SUMS`, and the
installer verifies the download against it before unpacking.

### What 1.0.0 commits to

Version numbers are a promise, so here is the exact one being made.

**Stable, and a breaking change means 2.0.0:**

- The pipeline verbs and their order — `init → build → recover → resolve → compile → commit` —
  and their existing flags.
- The MCP tool names and their argument shapes. An agent configured against 1.0.0 keeps working.
- The on-disk ledger format. A ledger written by 1.x is readable by every later 1.x. The legacy
  SQLite backend keeps serving existing workspaces and is never switched implicitly.
- The four semantic primitives — Object, Relationship, Event, Evidence — and the rule that every
  conclusion carries its evidence.
- The `ekos.toml` schema: existing keys keep their meaning, and new ones are additive with
  defaults that preserve current behaviour.

**Explicitly not covered by that promise:**

- The Rust crate APIs. The crates are not published, and `compiler-sdk` / `observation-sdk` are
  still moving as connectors are added. Out-of-tree builds use the RFC 0149 extension seam, which
  *is* covered.
- Output text and formatting of human-readable reports. `--json` surfaces carry a
  `schema_version` and are the stable form for machines.
- LLM-generated content. Different models and prompts produce different prose; only the grounding
  and citation contract is stable.
- Anything gated behind an opt-in flag that this file calls scaffolded or experimental.

### What's in it

- **The compiler pipeline** — `init → build → recover → resolve → compile → commit`, each stage
  writing artifacts the next consumes, into an append-only ledger where every conclusion carries
  the evidence it came from.
- **Connectors** for files, Git, GitHub, Confluence, local documents (PDF/DOCX/HTML/email),
  Pentaho, dbt, ClickHouse, and source analysis for Rust, Python, JavaScript/TypeScript, Elixir
  and Perl — real ASTs, not regex.
- **MCP server** (`ekos mcp serve`) over stdio, TCP or HTTP, exposing the ledger read-only to
  Claude Code, VS Code, Visual Studio and any other MCP client.
- **Query surfaces** — `ekos ask` (grounded, cited answers), `ekos query find` (BM25 + vector,
  RRF-fused), `ekos ekl` (a real query language), `ekos impact` (multi-hop blast radius).
- **Documentation generation** — `ekos docs generate` renders README/architecture/API/sequence
  diagrams deterministically from the compiled ledger, with no LLM in the path.
- **First-run self-verification** — `ekos init --detect` writes a config that matches your
  repository, and `ekos coverage` reports which input kinds actually compiled into objects, so a
  pipeline that silently produced nothing says so.
- **Storage that scales down and up** — a single-file fact-segment engine by default, opt-in
  partitioned storage, and an opt-in distributed tier with object-store backends.

### Known limitations

These are real and shipped as-is; 1.0.0 is a stability promise, not a claim of completeness.

- Identity resolution merges only exact normalized-name matches; fuzzy candidates become
  reviewable `unconfirmed` relationships rather than silent merges. This is deliberate
  (RFC 0060), and it means some real duplicates stay unmerged until a human confirms them.
- SQL DDL parsing is whole-file all-or-nothing per dialect: an unsupported construct anywhere in
  a file loses the whole file. `ekos coverage` now reports this rather than hiding it, but the
  parser behaviour itself is unchanged.
- The Salesforce, SAP, Oracle, Fabric and Snowflake connectors are scaffolded proofs of concept
  against mock API shapes — they have not been exercised against live accounts.
- Call-graph recovery is intraprocedural and within-file for most languages; cross-file call-chain
  tracing is not implemented. Perl has no call graph at all (dispatch is fully dynamic).
- crates.io: the workspace is publish-ready as of 1.0.2 but the crates are published separately
  (44 of them, rate-limited by crates.io to roughly one new crate per ten minutes). Until that
  completes, `cargo install ekos` does not work — use the prebuilt binary above.
- Releases are checksummed (`SHA256SUMS`) but not signed. No Sigstore/cosign, no SLSA provenance.
- There is no static musl build for aarch64 Linux, so `install.sh` has no fallback there: an
  aarch64 machine older than glibc 2.35 must build from source. x86_64 Linux has the fallback.
- The compiled .NET/JVM binary decompiler is a separately licensed extension build, not part of
  this release (RFC 0148/0149).

Full engineering history is in `devlogs/` (201 entries) and the design record in `docs/rfcs/` and
`ekos/docs/rfcs/` (153 RFCs).
