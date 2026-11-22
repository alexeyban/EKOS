# Devlog 354 — The first differential run of a Talend translation (RFC 0268 phase 3)

**Date:** 2026-11-22
**PRs:** none (direct to `main`); lakehouse devlog_30
**Branch:** `main`

---

## Summary

A Talend translation was checked against the project's own jobs for the first time.

**The sandbox.** The operator's Windows VM, set up with their approval:
- an isolated libvirt network, with outbound traffic also blocked in Windows;
- a clean snapshot;
- Java 17 and PostgreSQL 16, unpacked, with no installers.

**The run.** Fixture B's ten built jobs ran there, on a throwaway PostgreSQL loaded from the project's own scripts
and CSVs. `lakehouse talend verify` built the translated models' first version on a throwaway Trino, over the same
files, and compared table by table:

| Table | Rows | Result |
|---|---|---|
| `DIM_CALENDRIER` | 2,192 | equal on all 17 columns |
| `ODS_CATEGORIE` (snapshot) | 3 | equal |
| `ODS_TYPE_CLIENT` (snapshot) | 3 | equal |

- **Differences:** none. The comparer's planted difference was found.
- **Not verified:** 12 tables, reported with reasons: their models are disabled downstream of the two ODS jobs
  refused for defects.

---

## RFC 0268 phase 3 (EKOS docs; lakehouse devlog_30)

### What was built

| Part | What |
|---|---|
| RFC 0268 §Phase 3 | The sandbox host, the runner, the comparison; the result and amendments |
| Sandbox (operator's machine) | VM `win11` on the isolated network `ekos-sandbox` (no forwarding), Windows outbound blocked, snapshot `ekos-sandbox-clean`, verified unreachable from inside |
| Runner (lakehouse `scripts/talend-sandbox/`) | revert, payload, run every job in order, dump every table after each, bring back, revert |
| `lakehouse talend verify` | land, build, compare, control, clean up |

### Decisions

- **Every value the runner sets that the project does not state is a sandbox setting in the manifest**, with its
  reason: the run day, context rows, hard-coded paths, the JVM locale, libraries from the Studio, and a job packaged
  from its classes.
- **Only one run from empty tables is compared,** which is each model's first build. A second, incremental run is
  phase 3's next step.
- **The sandbox was set up with the operator's approval of each step:** tools, isolation, snapshot. One rule had to
  be widened: SSH on the Public profile, done during one minute on the old network, with outbound traffic already
  blocked.

---

## Knowledge Captured

- **Execution finds what no gate can.** Two translation defects passed Trino parse, anti-invention and scope, and
  would have shipped:
  - **one source name for six files:** every `tFileList` input shares its path expression;
  - **a file's name cut at `/` only:** a job on Windows stores `C:\…\file.csv`.
- **The JVM's locale changes outputs:** `January` against `Janvier`. A differential run must state it for both
  sides.
- **Windows sandbox gotchas** (also in the RFC):
  - EDB's PostgreSQL needs the VC++ runtime, which can be copied from the JRE;
  - never pipe a server's output over SSH;
  - a new network lands in the Public firewall profile;
  - ACPI shutdown is ignored; `shutdown /s` over SSH works;
  - `Start-Process` neither quotes arguments nor keeps exit codes without `$p.Handle`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0268-talend-to-dbt.md` | phase 3: sandbox, runner, comparison, result |
| `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | phase 3 ticked; devlog pointer; the differential run |
| ekos-lakehouse (devlog_30) | runner, `talend-dbt::verify`, `lakehouse talend verify`, two `talend-rel` fixes |
