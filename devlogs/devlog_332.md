# Devlog 332 — v1.5.0 for every OS, Sonar rules before the push, RFC 0255–0258

**Date:** 2026-11-12
**PRs:** none — committed to `main` after local gates (EKOS 35393f8b … 14b61b90; ekos-lakehouse
1ec8caf → ba1ebf6)
**Branch:** main, with the merged local branches `rfc0255`, `rfc0256`, `rfc0257` (EKOS) and
`rfc0255`, `rfc0258`, `rfc0256` (ekos-lakehouse)

---

## Summary

EKOS v1.5.0 is released for Linux (glibc, musl, arm64), macOS and Windows. Each OS has a full package:
the CLI, the web console with its own Python, and the setup wizard. On Windows that package is now
installed, started and uninstalled inside the Release workflow. That test found two Windows-only hangs
(setup, uninstall), which are fixed. Windows no longer needs the Visual C++ runtime, and an MSIX package
for the Microsoft Store is built.

SonarCloud's gate rules now run locally before a push (`scripts/sonar-local.sh`).

Four RFCs were built:
- **RFC 0255:** an open source-engine seam for the lakehouse.
- **RFC 0256:** who reads and writes each table, declared by code and observed in PostgreSQL, through
  to a warning on the lakehouse extraction plan.
- **RFC 0257:** a recovered transformation as a chain of steps.
- **RFC 0258 phase 3:** health of the dashboards EKOS published.

---

## Release v1.5.0 — every OS, a full package, Windows tested in CI

### Problem / motivation
On Windows 11, a user's `ekos.exe` failed with "VCRUNTIME140.dll was not found". The release also had
no Windows full package that anyone had run.

### What was built
| Component | What |
|---|---|
| Static CRT | `+crt-static` for `x86_64-pc-windows-msvc`: `ekos.exe` needs no redistributable |
| `install-windows` job (`release.yml`) | unpacks the `-full.zip` on `windows-latest`, runs `ekos setup` with answers, checks `/api/health` and `console status`, runs `ekos uninstall`, checks the source tree was not written and that `bin\` is gone within 30 s |
| MSIX | the `store-msix-x64` artifact (identity `AlexeyBanaev.EKOS.DEV`), smoke-tested through its execution alias. It is not a release asset: the owner submits it in Partner Center |
| Local chat | `web/api/app/chat/providers.py::ollama_context` sets `num_ctx`: at least 8192 and at most 131072, from len(json)/3 + max_tokens, doubled |

### Implementation details worth remembering
- **Setup hang (28f564f2).**
  - `RealEffects::run_env` (`commands/setup.rs`) read the child's stdout and stderr through pipes until
    EOF.
  - On Windows, the detached `console run` and its Python inherit the pipe handles
    (`bInheritHandles`), so EOF never came and `ekos setup` hung after "start the console".
  - Output now goes to `tempfile::tempfile()` files, and setup waits on `child.wait()`.
- **Uninstall hang (54c903e1).**
  - The delayed removal of `bin\` was `cmd /c start "" /b cmd /c "timeout /t 3 … rmdir …"`.
  - Rust's `\"` escaping is misread by `cmd`, and `timeout` needs a console.
  - Now `Effects::remove_after_exit(dir)`; on Windows, `RealEffects` spawns
    `cmd` with `raw_arg("/d /c ping -n 4 127.0.0.1 >nul & rmdir /s /q \"<dir>\"")`, null stdio, and
    `DETACHED_PROCESS | CREATE_NO_WINDOW`.
  - The trait's default records a `cmd` run, so the fakes in the tests stay unchanged.
- **The workflow's setup step.**
  - It no longer pipes `| Tee-Object`, which holds the same inherited handle.
  - It runs `Start-Process` with `RedirectStandardOutput`/`Error` to files and `WaitForExit(600000)`,
    and the job has `timeout-minutes: 30`.
- **Chat and tools.**
  - About 29 offered tools are ~5,300 tokens. Ollama's default window is 4096 and truncates silently.
  - Measured with qwen2.5:1.5b: the default window gave a 4096-token prompt and no tool call; with
    16384 the prompt was 5328 tokens and `ekos_search` was called.
- **`deploy/e2e/package_browser.py`.**
  - The "saved" wait now looks for `p.ok-line:has-text('saved')`, because a hidden RFC 0253 help text
    "…saved testimony…" matched first.
  - The chat check waits for "not paid per call" before reading it.
  - `deploy/e2e/package.sh`: 19/19.

### Decisions
- **Static CRT rather than shipping `vcruntime140.dll` beside `ekos.exe`:** one file, nothing to
  install. The console's Python keeps its own DLLs in the full package.
- **The MSIX stays a workflow artifact:** Store submission is a manual step in Partner Center.

---

## `scripts/sonar-local.sh` — the gate's rules before the push

### Problem / motivation
The gate turned red after pushes on rules clippy and ruff do not check.

### What was built
- **Rust:** clippy plus Sonar's extra rules, and cognitive complexity.
- **TS/TSX:** `eslint-plugin-sonarjs`, `jsx-a11y` and `react` (in `scripts/sonar-local/`).
- **Python:** ruff `ASYNC`/`RUF029`, `pycogc.py` (complexity counted as Sonar counts it) and
  `pyrules.py` (S5850).
- **Shell:** `shcheck.py`.
- **Scope:** the lines the change touched, which is Sonar's "new code".

### Knowledge
- **Files come from git, never from the command line.** Sonar's taint analysis reports a path taken
  from argv as path traversal, a security rating, so the gate goes red. New files must be `git add`ed
  to be checked.
- **`pycogc.py` replaced `complexipy`.** It matched Sonar's numbers; `complexipy` scored functions up to
  half of Sonar's count.

---

## RFC 0255 — an open source-engine seam (phases 1–5)

### What was built
- **The engine is an open identifier.** `SourceEngine` is no longer an enum (`bronze/engine.rs`).
- **Registry.** `SourceProvider` and the `Engines` registry live in `lakehouse-ext/src/engines.rs`.
  An engine is added with `LakehouseExtension::new().with_engine(..)`.
- **Reconciliation dialects.** `CanonDialect` and `Dialect::Registered(&'static dyn CanonDialect)`,
  with a conformance kit in `migrate-validate`.
- **Capabilities.** Each source's `SourceCapabilities` (`kir/src/source_capabilities.rs`) is recorded
  at plan time and shown in the estate docs ("Source engines", and an `engine_limit` line).
- **Phase 5.** A fixture engine built outside the repo, and the guide for writing one.

### Decisions
- **Open question 1:** a fifth `Dialect` variant holding the dialect, rather than `&dyn` everywhere.
  `Dialect` is referenced 261 times in 16 files, passed by value and compared with `==`. The new
  variant keeps it `Copy + Eq`.

### Measured
- Lock files and reconciliation SQL are byte-identical before and after the seam:
  - `ekos-lakehouse-oracle/pipeline`: 849 tables, 6,402 digest lines;
  - `pipeline-sales`: 234 digest lines.
- `migrate-validate`'s SQL snapshots are unchanged.

### Open
- The live lakehouse suites fail before any engine code runs, on `CATALOG_NOT_FOUND`: the running Trino
  sandbox has no `pagila`, `m3` or `postgresql` catalogs.

---

## RFC 0256 — the interaction map, to the extraction plan

### What was built
- **EKOS, phases 1–4.**
  - `ekos interaction matrix|observe|findings`.
  - Findings, the `interaction` review subject, and questions.
  - The console's Interaction page and MCP `ekos_interaction`.
  - Details are in the RFC's §9.
- **ekos-lakehouse, ba1ebf6** (`writer_warnings.rs`, pure).
  - `lakehouse plan` keeps the current `unexplained_writer` / `unattributed_writes` findings and the
    row estimates in the proposal.
  - Every incremental table they name carries a warning in `extraction show` and on the console's
    Extraction page, with advice, reason and cost:
    - load in full when the table is within `full-refresh-max-rows`;
    - look for deletes every run when an outside delete meets audited deletes;
    - otherwise reconcile.

### Decisions
- **Open question 1, where the operator's instruction is stored:** in an RFC 0226 per-table change.
  - The change records `answers`, the evidence of the warnings on that table when it is made.
  - `--keep` is a change of kind `mode` that alters nothing, and is refused when nothing is open.
- **What counts as evidence:** finding, table, role and operations; never counts. More calls by the
  same role stay answered; a new role or a new operation reopens the warning.
- **Warnings are not part of the plan or its digest.** Keeping the mode leaves an approval valid, and
  an open warning does not block approval.

### Open
- The phase 4 exit (a browser run on the testbed).
- Oracle observed access.

---

## RFC 0257 — a transformation as a chain of steps (phases 1–2, EKOS side)

`ekos transformation graph|list`, MCP `ekos_transformation_graph`, the console's Transformations tab and
notes on a step, built from `semantic/src/transform_chain.rs`.

Recovery fixes found on LedgerSMB:
- `JOIN … ON (a = b)` lost its key pairs.
- A view's `Sink` now carries the view's output columns.

Measured on LedgerSMB: 576 views and routines rendered, 0 failures; 98% of view steps and 95% of
routine steps have a span in the source text.

Open: the mart side and phases 3–5.

---

## RFC 0258 phase 3 — dashboard health (ekos-lakehouse)

- **Commands:**
  - `lakehouse dashboards sync` reads Superset's REST API, GET only.
  - `status` gives a chart state (failing, unverified, late, trusted) with data-as-of and what was not
    evaluated.
  - `publish-health`, opt-in, writes a header and an "EKOS (pipeline health)" certification.
- **Decision:** a person's certification is never removed.
- **Merge:** rebased onto RFC 0255, so `dashboards` now loads its settings through the engine registry
  like every other command.

---

## Knowledge Captured

- **Windows handle inheritance.**
  - A detached child started while the parent holds pipes inherits those pipe handles, so reading the
    pipe until EOF hangs for as long as the child lives.
  - Read a grandchild-spawning command's output from files, or wait on the process, never on EOF.
  - PowerShell's `| Tee-Object` has the same problem.
- **Deferred deletion of a running program's directory from `cmd`.**
  - `timeout` fails without a console; use `ping -n N 127.0.0.1 >nul`.
  - Rust's argument quoting is wrong for `cmd /c`; pass the command line with `raw_arg`.
- **Ollama truncates a prompt over its context window without saying so.** Size `num_ctx` from the
  request, tools included.
- **GitHub runner capacity.** A macOS job "not acquired by Runner … capacity constraints" fails the
  attempt; `gh run rerun --failed` is enough.
  - The outage on 54c903e1 left ghost runs queued forever; dispatch fresh ones.
  - Polling `gh run list` every few seconds exhausted the API rate limit; poll every 3 minutes, or use
    `gh run watch --interval 180`.
- **Never share `CARGO_TARGET_DIR` between checkouts of different commits** (here, the worktrees per
  RFC branch): builds overwrite each other's artifacts.
- **A lakehouse branch built against a local EKOS branch** uses an uncommitted `.cargo/config.toml`
  `[patch]`.
  - Before merging, move it aside, restore `Cargo.lock`, and `cargo update -p ekos` against the pushed
    EKOS main.
  - Here the lakehouse lock jumped from EKOS 54dd67f to 793ab70b.
- **`ekos.toml` silently ignores a wrong key.** `pattern` for `path-glob` makes `from_file_or_default`
  fall back to defaults without a word. This is in TODO.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/setup.rs`, `ekos/crates/cli/Cargo.toml` | output through temp files; `remove_after_exit`; `tempfile` a dependency |
| `.github/workflows/release.yml` | `install-windows` job; MSIX; `Start-Process` with file redirection, `timeout-minutes` |
| `web/api/app/chat/providers.py`, `web/api/tests/test_chat.py` | `ollama_context` |
| `deploy/e2e/package_browser.py` | waits on the right "saved" line and the chat text |
| `scripts/sonar-local.sh`, `scripts/sonar-local/` | local Sonar rules |
| `ekos/crates/migrate-validate/src/{dialect,conformance}.rs`, `ekos/crates/kir/src/source_capabilities.rs`, `ekos/crates/docs-gen/src/estate/passport.rs` | RFC 0255 |
| `ekos/crates/kir/src/interaction.rs`, `ekos/crates/cli/src/commands/interaction.rs`, console Interaction page | RFC 0256 |
| `ekos/crates/semantic/src/transform_chain.rs`, `ekos/crates/cli/src/commands/transformation.rs`, console Transformations tab | RFC 0257 |
| ekos-lakehouse `crates/lakehouse-ext/src/writer_warnings.rs`, `extraction.rs`, `console.rs`, `plan_cmd.rs`, `lib.rs`; `crates/bronze/src/extraction.rs` | RFC 0256 plan warning, `Change::Keep`, `TableChange::answers` |
| ekos-lakehouse `crates/health/src/dashboard.rs`, `crates/lakehouse-ext/src/dashboards.rs` | RFC 0258 phase 3 |
| `ekos/docs/rfcs/0255…`, `0256…` | implementation logs; RFC 0256 open question 1 decided |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | v1.5.0 Windows notes; RFC 0255–0258 |
