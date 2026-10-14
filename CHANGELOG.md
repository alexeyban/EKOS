# Changelog

## Unreleased (on `main` since v1.0.3)

RFCs 0154–0183, devlogs 204–266. Not yet tagged; build from source or wait for the
next release.

**New capabilities**
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
