# Devlog 259 — Named sources: several repositories and folders in one project (RFC 0178, Part 2)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

A project can now be made of several named inputs: repositories and directories anywhere on disk, compiled into one ledger. Every object records which source it came from, so a query can ask for one source, and the later access-control work has something to decide on.

Most of the mechanism already existed. `build` has scanned each configured path from its own root and kept their objects apart since RFC 0044. This work gives that key a name, puts it on objects, and removes the requirement that every path sit under one workspace root.

---

## What was built

| Component | Where |
|---|---|
| `[[sources]]` (`name`, `kind`, `path`), validation, `observe_bases` | `ekos/crates/compiler-core/src/config.rs` |
| Attribution rule, `source` / `sources` properties, per-kind counts | `ekos/crates/semantic/src/sources.rs` (new) |
| Stamp on `File` objects at build, on compiled objects at commit | `commands/build.rs`, `commands/commit.rs` |
| `ekos build --source <name>` | `commands/build.rs::run_selected` |
| `ekos source add / list / remove` | `commands/source.rs` (new) |
| `ekos project add --source <source>=<dir>`, repeatable | `commands/project.rs` |
| Freshness for sources outside the workspace | `freshness.rs` (`manifest_prefix`, `disk_path`) |

### How an object gets its source

1. Its `project` property, when the analyzer recorded the key of the directory it read. Exact.
2. Otherwise the sources holding a file one of its evidence records names. A path present in two sources attributes the object to both.
3. Otherwise, in a project with one source, that source.

What none of these reach has no source. `commit` prints the count by kind. On the test project (two repositories and a document folder) the only such kind is `Person`, the commit author.

### Implementation details worth remembering

- **One function returns the directories to observe.** `EkosConfig::observe_bases` replaced four copies of "`[observe] paths`, or the root when empty" (`build.rs`, `recover.rs`, `commands/config.rs`, `freshness.rs`). Seven helpers in `recover.rs` computed the key from `(base, cwd)`; they now take the key with the base.
- **Over-attribution is the safe direction.** When a path exists in two sources and the analyzer recorded no key, the object lists both. A reader restricted to one source must then not see it, which fails closed.
- **`build --source` keeps the other sources' manifest entries.** The source manifest is what the freshness check and the attribution both read. A partial build that rewrote it from only the observed source would have made every other source look deleted.
- **Config edits keep comments.** `source add` and `source remove` go through `toml_edit`, and the result must parse as a valid config before it is written.
- **A typed path is relative to where the command was typed.** With `--project` the process moves into the source root, so `main_with` records the starting directory first and `source add --path` resolves against that.

### Decisions

- **A compiled project cannot gain source names.** Ids were hashed without a key and the ledger is append-only. Naming sources later would leave every object beside a renamed copy of itself. `source add` refuses and says to create a new project with `--source <name>=<dir>`.
- **The property is `source`; `project` keeps its old meaning.** `project` is the RFC 0044 hashing key and existing ledgers depend on it. "Project" in RFC 0178 is the level above.
- **`kind = "git"` is a statement, not a switch.** The git observer runs on any directory with `.git`. Declaring a source `git` lets `build` warn when there is none.

---

## Not built

- `confluence` and `github` as source kinds; both are still one per project.
- `source` on objects written after the main commit loop: rollups, business semantics, label conflicts, LLM descriptions.
- A `--source` flag on `query find` / `ask`, and a `source` argument on MCP tools. EKL already filters: `FIND Object WHERE source = 'handbook'`.

---

## Knowledge Captured

- **Read the build loop before designing over it.** The first draft of the RFC said several repositories in one workspace yield no commits. `build` already scans each path from its own root. The true limitation was narrower: subdirectories of one repository, an unnamed key, and a key that only some objects carried.
- **`cargo fmt` between two scripted edits changes the text the second one matches.** A multi-line call collapsed to one line and the next replacement found nothing. Match after formatting, or format once at the end.
- **`ekl --json` prints an array of rows**, not an object with a `rows` field.
- **Assert on syntax or parsed values, not on a bare word.** A test checked that a config no longer contained "sources"; a comment in the fixture said "the sources", so it failed for the wrong reason. `[[sources]]` or the parsed list is what was meant.
- **Adding a dependency to `ekos/` makes the other two lockfiles stale** (`benchmark/`, `tests/integration/`); `cargo metadata` in each refreshes them, and `scripts/audit.sh` fails until then.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0178-ekos-home-projects-sources.md` | Part 2 rewritten as built |
| `ekos/crates/compiler-core/src/{config,lib}.rs` | `SourceConfig`, `SourceKind`, `ObserveBase`, `observe_bases`, `check_sources`, 2 tests |
| `ekos/crates/semantic/src/sources.rs`, `lib.rs` | New: `SourceIndex`, `stamp`, `SourceStats`, 5 tests |
| `ekos/crates/cli/src/commands/source.rs` | New: add, list, remove, 3 tests |
| `ekos/crates/cli/src/commands/{build,commit,recover,config,project}.rs`, `freshness.rs`, `app.rs` | Named bases, stamping, `--source`, commands |
| `ekos/crates/cli/tests/project_home.rs` | Multi-source end-to-end test |
| `ekos/Cargo.toml`, `ekos/crates/cli/Cargo.toml`, lockfiles | `toml_edit` |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |
