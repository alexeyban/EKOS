# Devlog 200 — First-run self-verification and a real release (RFC 0152, RFC 0153)

**Date:** 2026-09-23
**PRs:** none (local `main`, `[skip ci]`)
**Branch:** main

---

## Summary
Two pieces of work aimed at the same gap: the project has 199 devlogs, 151 RFCs, 34 crates and 32
presentation decks, and **16 stars, 5 forks, 0 releases**. Nothing converts a reader into a
running instance, and the first ten minutes of a run could silently produce nothing. RFC 0152
makes the first run self-verifying — `ekos init --detect` writes a config that matches the
repository, and `ekos coverage` reports which input kinds actually compiled into objects. RFC 0153
makes a release exist: a pinned toolchain, a release profile, a six-target build workflow, a
checksum-verifying installer, and a README that leads with a binary instead of `cargo install
--path`. Both were validated against real runs, and both had a design error that only a real run
exposed.

---

## RFC 0152 — First-run self-verification

### Problem / motivation
`ekos init` was 38 lines: four directories and a seven-line `ekos.toml` with no dialect, no
exclusions and no knowledge of the repository. `ekos doctor` checked the Rust toolchain, the LLM
key and whether the config existed — never whether the compiler **found anything**. The same
silent failure has now hit five real codebases:

| Incident | Symptom | Cause |
|---|---|---|
| `analytics` (devlog_177) | 0 `Table` objects for a repo with a schema | no dialect rule; whole-file parse failed, one buried `SQL001` |
| LedgerSMB (RFC 0146) | 0 of 103 tables on the **correct** dialect | `COMMENT ON … IS $$…$$` / `INHERITS` failed the all-or-nothing parse |
| git observer | 0 commits, silently | `paths` listed subdirectories; `GitObserver` only checks `root/.git` |
| first .NET app (devlog_190) | calls joined 0 of 36k | no per-format edge reporting |
| this repo's own config | 50.9% of objects described numpy/scipy | `.venv` unexcluded; `.gitignore` excludes nothing from the walk |

Every one exited `0` with a cheerful summary.

### What was built

| Component | Role |
|---|---|
| `cli/src/detect.rs` | `SourceKind` (13 kinds), `Classifier`, `detect_sources`/`detect_contaminants`/`guess_dialect` (all pure), `detect_workspace` (the only filesystem call), `render_config` |
| `cli/src/coverage.rs` | `compute_coverage` joining a `Detection` against a `CkModel`; `CoverageStatus`; text and compile-tail renderers |
| `cli/src/commands/coverage.rs` | `ekos coverage [--json] [--strict] [--all]` |
| `commands/init.rs` | `run_with_options` + `InitOptions { detect, dry_run, force }`; `run` unchanged |
| `commands/compile.rs` | one-line-per-finding tail, silent on a clean run |
| `commands/doctor.rs` | a `Coverage` check; fails only on a genuine zero |

### Implementation details worth remembering

- **One vocabulary, two directions.** `SourceKind::matches_path` is the single definition of "a
  Python file", used by detection to count inputs and by coverage to attribute an evidence path
  back to a kind. Without that, the two halves drift and the join is meaningless.
- **The dbt exception needs context, so it does not live in the path predicate.** A dbt model
  *is* a `.sql` file. `Classifier` holds the dbt roots and routes `.sql` under one to `Dbt`.
  Without it, a dbt-only workspace whose hand-written DDL recovery produced nothing would still
  show SQL as healthy — the exact false negative the feature exists to prevent.
- **Attribution is by evidence path, not `source_artifact_ids`.** The latter is more precise
  provenance but is empty for pre-RFC-0135 models and for objects synthesized in `compile`, and
  it names an artifact id rather than a path. Evidence paths are present on everything with any
  provenance, and RFC 0140 §1 guarantees they carry real locations.
- **Rows may sum above the object total**, because an object fused from DDL and a dbt model has
  evidence from both. The footer says so rather than picking an arbitrary primary kind.
- **Git evidence is a pseudo-path** (`git:commit:<sha>`, `git:contributors`), not a file, so the
  `Git` matcher keys on the `git:` prefix.
- **The walk prunes and then counts.** `target/` here is 142GB; descending into it made
  `--detect` look hung. The walk prunes at every excluded directory, records what it refused to
  enter, then counts each subtree separately with a 20,000-file cap. 3.0s on this repository.

### Decisions (alternatives considered)

- **No threshold-based "partial coverage" verdict.** RFC 0060 settled this project's position on
  confidence thresholds when no cutoff could separate correct from incorrect identity merges.
  There is no defensible number at which "some tables missing" becomes a warning, and a wrong one
  trains people to ignore the report. Zero is the one non-arbitrary signal — and zero is what
  every real incident produced.
- **A dialect tie is not a guess.** Same reasoning: one postgres marker against one mysql marker
  yields `generic` plus a comment telling the reader what to check, not a coin flip. Guessing
  wrong costs the user their entire schema.
- **`compile` stays exit-0 on a finding.** A workspace can legitimately hold one `.sql` migration
  fragment that produces no tables. Loud by default; `--strict` is the fatal form, for CI.
- **Detection never enables a credentialed connector.** A detected GitHub remote becomes a
  commented-out suggestion. Turning on something that makes network calls stays a human act.
- **`ekos init` is unchanged.** Detection is opt-in via `--detect`, because a detected config
  makes real claims about a repository and a user must be able to get the inert one. `run()` kept
  its signature; `run_with_options` was added alongside — ten test call sites and no API break.

---

## RFC 0153 — Release engineering and binary distribution

### Problem / motivation
The install instruction was `git clone && cargo install --path crates/cli`: install a Rust
toolchain and compile 34 crates before seeing anything. Every deck, the token benchmark and every
future post terminated at that wall.

Two facts made it cheap: nothing in the dependency graph resists static linking (`rusqlite` is
`bundled`, `reqwest` is `rustls-tls` with `default-features = false`, no `*-sys` or `bindgen`
crates, and `tesseract` is shelled out to rather than linked), and the `distributed` feature is
already off by default. The one real gap was reproducibility — no `rust-toolchain.toml`, no
`rust-version`, so "reproducible builds" was enforced by nothing.

### What was built

| Component | Role |
|---|---|
| `rust-toolchain.toml` | pins 1.98.0 + rustfmt/clippy for CI, releases and developer machines alike |
| `[profile.release]` | `strip = "symbols"`, `lto = "thin"`, `panic = "unwind"` |
| `[workspace.package]` | `description`/`repository`/`homepage`/`keywords`/`categories`/`rust-version` |
| `.github/workflows/release.yml` | six targets on a `v*` tag → tarballs + one `SHA256SUMS` → `gh release create` → a `verify` job that installs what was just published |
| `install.sh` | POSIX `sh`; resolves the release, verifies the checksum **before** unpacking, installs to `~/.local/bin`, no `sudo` |
| `CHANGELOG.md` | v0.1.0 release notes, hand-written, including a real known-limitations section |
| `README.md` | `## Installation` leads with the binary; source build demoted to a subsection |

### Implementation details worth remembering

- **`panic = "abort"` is rejected on purpose.** It is a further size win, but RFC 0115's TCP
  transport spawns one OS thread per connection and RFC 0113's workers do the same. A panic
  serving one request must not kill a server serving others. The `Cargo.toml` comment says so, so
  nobody "optimizes" it later.
- **Workspace package metadata applies only where a member opts in.** Setting
  `[workspace.package]` fields changed nothing until `crates/cli/Cargo.toml` added
  `repository.workspace = true` and friends. `cargo metadata` is the way to check.
- **No workspace-level `readme`.** Its path resolves relative to each *member* crate, and a file
  outside a crate's directory is not included by `cargo package` — so one value is wrong for every
  member. It belongs per-crate, with the deferred crates.io work.
- **musl earns its matrix row.** glibc version skew is the most common "your binary doesn't run"
  report for Rust CLIs, and a static build costs one extra row.
- **crates.io is deferred, not forgotten.** `cargo install ekos` needs all 30 internal crates
  published in dependency order — 30 irreversible acts. RFC 0153 §6 records the ordering and the
  blockers. The name `ekos` was confirmed unclaimed on 2026-09-23.

---

## Knowledge Captured

- **A reporting feature can commit the exact error it exists to prevent, and only a real run
  shows it.** Two of them here, both caught by running the thing rather than by the 35 unit
  tests that passed:
  1. `NoEdges` fired on kinds with a *single* object — a repo with one commit and one Python
     class produced two "0 relationships" findings. An edge needs two endpoints; the rule is now
     `objects >= 2`, which is structural, not a tuned threshold.
  2. The `NoEdges` finding printed the *zero-coverage* explanation. Next to "1 object compiled",
     the git hint read "yields zero commits with no error at all" — describing a failure that had
     not happened. A report that misdescribes what it found is worse than no report.
- **A capped count must never print as a total.** The first version reported `target: 60000
  file(s)` — three `target/` directories each stopping at the 20,000 cap. Nothing had counted
  60,000 anything. Capped counts now render as `20000+`.
- **An asset name in a `SHA256SUMS` lookup must be regex-escaped.** `ekos-0.1.0-….tar.gz` is
  full of `.`, which matches any byte — so a line naming a *different* file could satisfy the
  check. Verified by feeding the installer a line for `ekos-0X1Y0-….tar.gz` and confirming it is
  refused. In a verification step this is the whole point, not a nicety.
- **Test every refusal path of an installer, not just the happy path.** Against a locally served
  fake release: clean install succeeds; tampered archive, asset missing from `SHA256SUMS`, 404,
  unsupported OS and unsupported architecture each exit non-zero and install nothing. Same
  lesson as the `headless.sh` act-filter bug — a filter only ever tested against inputs it should
  accept will pass everything.
- **A hint that names a file must name the real one.** The zero-coverage hints pointed at
  `.ekos/diagnostics/recover-*.log`; `write_diagnostics_log` writes `recover.log`. That is RFC
  0076's "(check logs)" bug returning. A guard test now asserts every hint mentioning a
  diagnostics path names the filename the writer actually produces.
- **`ekos init --detect` honours the *existing* config's `ignore-patterns`**, so running it on
  this repository reports no SQL inputs — `fixtures/` is excluded here. Correct, and surprising
  the first time.
- **The acceptance criteria in a draft RFC are guesses until a real run.** RFC 0152's criterion 8
  asserted this repository would show Perl and dbt inputs. It has neither. Corrected in the RFC
  with a note saying it was wrong, rather than quietly deleted.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0152-first-run-self-verification.md` | new — detection + coverage design, with the §6a corrections the first real run forced |
| `ekos/docs/rfcs/0153-release-engineering-and-binary-distribution.md` | new — targets, profile, pipeline, installer, deferred crates.io plan |
| `ekos/crates/cli/src/detect.rs` | new — `SourceKind`, `Classifier`, detection, dialect guessing, config rendering (+19 tests) |
| `ekos/crates/cli/src/coverage.rs` | new — the inputs-vs-objects join and its renderers (+13 tests) |
| `ekos/crates/cli/src/commands/coverage.rs` | new — `ekos coverage` (+2 tests) |
| `ekos/crates/cli/src/commands/init.rs` | `--detect`/`--dry-run`/`--force`; `run()` unchanged (+6 tests) |
| `ekos/crates/cli/src/commands/compile.rs` | coverage tail, silent when clean |
| `ekos/crates/cli/src/commands/doctor.rs` | `Coverage` check |
| `ekos/crates/cli/src/app.rs`, `commands/mod.rs`, `lib.rs` | clap surface + module registration |
| `ekos/Cargo.toml`, `ekos/crates/cli/Cargo.toml` | release metadata, `[profile.release]`, member opt-in |
| `rust-toolchain.toml` | new — pinned toolchain |
| `.github/workflows/release.yml` | new — six-target build, release, and install verification |
| `install.sh` | new — checksum-verifying POSIX installer |
| `CHANGELOG.md` | new — v0.1.0 notes |
| `README.md` | Installation leads with the binary |
| `docs/generated/ekos-self-documentation.html` | new §00 "Install & first run" |
| `TODO.md` | RFC 0152/0153 entries ticked |
