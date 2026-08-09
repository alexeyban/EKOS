# Devlog 42 — RFC 0042: production-grade curated docs (crate topology, CI/CD, real program entities) + a second identity-merge bug found dogfooding

**Date:** 2026-08-09
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

The user reviewed `ekos docs generate --layout curated`'s output for EKOS's own repo (generated
last session) and rejected it as not production-grade: `Architecture.md` had no real
infrastructure, no program entities (functions/classes), and no links between documents.
Investigation split the complaint into two very different kinds of gap: most of it was a pure
**rendering** gap — `rust_analyzer`/`python_analyzer` already compiled real `RustSymbol`/
`RustModule`/`PythonSymbol`/`PythonModule` objects with a real function-level `Calls` graph, but
the curated renderers never read any of it, falling back to a stale text-scan and bare counts. The
rest was a genuine **missing-data** gap: nothing parsed `Cargo.toml` (so "Technologies" was always
empty) or `.github/workflows/*.yml` (so CI/CD was unmodeled at all). RFC 0042 closed both: two new
analyzer passes (`crate_topology_analyzer`, `cicd_analyzer`) plus a rewrite of
`render_architecture`/`render_api`/`render_sequence_diagrams` and a change to `generate_curated` so
curated output writes real per-entity detail pages, not just the four fixed files.

Real-data testing against EKOS's own ~40-crate workspace found and fixed a second occurrence of
the identity-resolution over-merge bug devlog_41 fixed for `RustSymbol`/`RustModule`/
`PythonSymbol`/`PythonModule`: the new `Custom("Crate")` object kind hit the identical failure
shape — 39 real crates collapsed into 1 canonical object at `ekos compile` time, silently dropping
the entire crate/workspace dependency topology this RFC exists to surface. Fixed the same way,
by extending `DefaultResolver`'s blanket kind-exclusion list.

A separate, unrelated bug was also found and fixed while wiring up the curated per-entity pages:
the Dependency-Graph overflow section's "sample" links were pointing at `File`/`Person` endpoints
that curated never writes a page for — a real dangling-link bug caught by an automated
link-integrity check over the regenerated `doc/` output (0 missing out of 1396 links, post-fix).

---

## RFC 0042 — Production-Grade Curated Documentation

### Problem / motivation

`doc/Architecture.md`'s `## Components` section printed bare counts (`RustSymbol: 1301`) with no
listing; `## Technologies` was empty because `dependency_analyzer.rs` (RFC 0019) only pattern-
matches ~25 hardcoded DB/infra connection-string literals in source text, never `Cargo.toml`;
`## Dependency Graph` capped oversized relationship kinds (`Calls`, `Contains`, `CoupledWith`,
`DependsOn` — all four fire in this repo) with a one-line "diagram omitted" sentence pointing at a
different CLI invocation, not a link; `doc/API.md` read a stale `File.symbols` text-scan property
instead of the real `RustSymbol`/`PythonSymbol` objects; nothing anywhere modeled CI/CD. None of
the four curated files ever hyperlinked to a `--layout objects` page, even when one existed.

### What was built

| Component | Location |
|---|---|
| Crate/workspace topology analyzer | `ekos/crates/recovery/src/crate_topology_analyzer.rs` (new) |
| CI/CD workflow analyzer | `ekos/crates/recovery/src/cicd_analyzer.rs` (new) |
| RFC | `ekos/docs/rfcs/0042-production-grade-curated-docs.md` (new) |
| Curated renderer rewrite | `ekos/crates/docs-gen/src/lib.rs` (`render_architecture`, `render_api`, `render_sequence_diagrams`) |
| Self-contained curated output | `ekos/crates/cli/src/commands/docs.rs` (`generate_curated` now writes per-entity pages) |
| Identity-resolution fix | `ekos/crates/identity/src/lib.rs` (`Crate` added to the blanket-exclusion list) |

**`crate_topology_analyzer.rs`** parses every discovered `Cargo.toml` (walked the same way
`dependency_analyzer`'s file-scan block already does in `recover.rs`, matched by literal filename
instead of extension) into `Custom("Crate")` objects (name/path/version/description from the
manifest) plus `DependsOn` edges: crate→crate for path/`workspace = true` dependencies resolved
against the root `[workspace.dependencies]` table, crate→`Custom("Technology")` for everything
else — reusing `dependency_analyzer.rs`'s exact `Technology` object kind and id scheme, so both
analyzers' output lands in the same "Technologies" section without any renderer change needed
there. `resolve_dep_entry`/`normalize_rel_path` handle all three TOML shapes a dependency can take
(bare version string, `{ path = ... }`, `{ workspace = true }`/`dep.workspace = true` dotted-key
sugar) and lexically collapse `../` path segments with no filesystem access.

**`cicd_analyzer.rs`** parses `.github/workflows/*.yml` (`serde_yaml` — new workspace dependency;
`toml` was already present via `compiler-core`'s own config parsing, just newly added to
`recovery/Cargo.toml`) into `ObjectKind::Pipeline` objects: name from the `name:` key (falls back
to the file stem), triggers from `on:` (handles all three YAML shapes: bare string, list, mapping
— and the YAML-1.1 gotcha where an unquoted `on:` key parses as the boolean `true`, not the string
`"on"`), and job/step names as plain JSON properties. A malformed workflow file is skipped with a
warning, not an abort — matches the per-item-resilience pattern `enrich_with_prose` already used
for LLM calls.

**Renderer rewrite** (`docs-gen/src/lib.rs`): `render_architecture` gained `## Crate & Workspace
Topology` (a mermaid graph over the new `Crate` `DependsOn` edges, reusing the existing generic
`render_mermaid_graph`/`render_relationship_kind_graph` — no new diagram code) and `## CI/CD
Pipelines`; `## Components` now links each program-entity/crate/technology/pipeline kind's count
to where its real detail lives instead of dumping a thousand-line inline list; the `## Dependency
Graph` overflow case now prints a real linked sample (first 15 edges) instead of a bare sentence.
`render_api` was rewritten to read real `RustSymbol`/`PythonSymbol` objects (kind badge + link to
detail page) grouped by containing file, falling back to the old text-scan only when zero real
symbol objects exist. `render_sequence_diagrams` gained a `## Call Sequences` section from real
`Calls` edges (RFC 0037 had explicitly skipped this, correctly, since `Calls` didn't exist yet at
the time — it does now, 791 edges in this repo per RFC 0041).

**`generate_curated`** now also writes one detail page per entity-page-kind object (reusing
`build_object_page_model`/`render_markdown_object_page` verbatim — the exact same per-object
renderer `--layout objects` already uses), keyed through a new `unique_page_file_names` helper
that disambiguates same-name/same-kind collisions (routine at symbol scale — many different
modules each declare a `fn new`) with an 8-hex-char id suffix on every occurrence after the first.
A new `is_entity_page_kind` predicate in `docs-gen` is the single source of truth both the link-
generating renderers and the page-writing loop check, so a link is never emitted to a page that
wasn't written (or vice versa).

### Decisions (alternatives considered, why this choice)

- **Cargo.toml parsing over pattern-matching connection strings** — rejected extending
  `dependency_analyzer.rs`'s existing table; `Cargo.toml` is structured TOML, and pattern-matching
  it as free text would also miss the crate-to-crate internal topology entirely (the actual "real
  infrastructure" the user asked for), not just external deps.
- **No Docker/Kubernetes/Terraform analyzer** — confirmed via direct investigation that EKOS's own
  repo has none of those; only `.github/workflows/*.yml` exists as real deployment/CI config.
  Building parsers for formats this repo doesn't use would be speculative, unverifiable code.
- **Curated writes its own per-entity pages rather than linking to a separately-run `--layout
  objects` pass** — a single `ekos docs generate --layout curated` invocation must produce a fully
  self-consistent, fully-linked doc set; requiring two commands kept in sync by the user was
  rejected as fragile.

---

## Bug: `Custom("Crate")` hit the same identity over-merge devlog_41 already fixed once

### Problem

Real-data testing (running the full `build → recover → resolve → compile → commit → docs
generate` pipeline against this repo's own ~40-crate workspace) found `doc/Architecture.md`'s
`## Crate & Workspace Topology` rendering a **single self-looped node** (`ekos-benchmark` →
`ekos-benchmark`) instead of the expected 39-crate dependency graph. `crate_topology_analyzer`'s
own trace log confirmed 39 real `Crate` objects + 368 `DependsOn` edges were emitted at pass time;
querying the compiled CKM directly (`unzstd` the `model.json.zst`, inspect with `python3 -c
"json.load(...)"`) showed only 1 `Crate` object survived `ekos compile`.

### Root cause

`DefaultResolver` (`ekos-identity`) already has a documented, previously-fixed failure mode
(devlog_39/40/41: `Section`, `TransformNode`, `RustSymbol`/`RustModule`,
`PythonSymbol`/`PythonModule`) where objects of the same kind sharing a long name prefix/suffix
score above the 0.85 merge threshold on Jaro-Winkler name similarity alone, because
`structural_score`'s same-kind fallback (no `columns` property to differentiate on) adds a flat
+0.3 on top. `Custom("Crate")` objects (`ekos-cli`, `ekos-compiler-core`, `ekos-common`, …) share
the workspace's `ekos-` prefix and an identical property shape (`path`/`description`/`version`,
no `columns`) — the exact same shape of failure, just never hit before because `Crate` didn't
exist as an object kind until this session.

### Fix

Added `"Crate"` to the existing blanket kind-exclusion list in `DefaultResolver::resolve`
(`ekos/crates/identity/src/lib.rs`) — each `Crate` is already deterministically identified by its
manifest directory, so no two distinct crates can legitimately be the same real-world entity; this
is a blanket exclusion, not a threshold/name-length guard, matching the existing precedent exactly.
Added a regression test (`crate_objects_are_never_merged_even_with_shared_name_prefix`) mirroring
the existing `rust_symbol_objects_are_never_merged_even_with_shared_name_suffix` test shape. Post-
fix, `ekos compile` correctly retains all 39 `Crate` objects and 2 `Pipeline` objects.

---

## Bug: dangling links from the Dependency-Graph overflow sample

### Problem

While spot-checking the regenerated `doc/` output with an automated link-integrity script (every
`](...)` link in the four curated files must resolve to a file `generate_curated` actually wrote),
75 of 1471 links were dangling — all pointing at `File`/`Person` detail pages that curated never
writes (only the 6 entity-page kinds get one).

### Fix

`render_architecture`'s Dependency-Graph overflow sample now gates each endpoint's link on
`is_entity_page_kind` before emitting a markdown link, falling back to plain text for any endpoint
kind curated doesn't write a page for. Regression test added asserting a `File`/`Section` overflow
sample renders `- doc.pdf → section-0` as plain text, never `[doc.pdf](...)`. Post-fix: 0 missing
out of 1396 links across the regenerated `doc/` output.

---

## Knowledge Captured

- **`Custom("RustModule")`/`Custom("PythonModule")` are `use`/import targets, not containers.**
  The real `Contains` edge into a `RustSymbol`/`PythonSymbol` comes from the defining **`File`**
  object, not from the `RustModule`/`PythonModule` object with a similar-looking name — those two
  kinds instead get a `DependsOn` edge *from* the file (an import reference). Grouping "API
  entities by containing module" by filtering on `RustModule`/`PythonModule` kind is a plausible-
  looking but wrong assumption; the first implementation of `render_api`/`render_sequence_diagrams`
  made exactly this mistake and it only surfaced as "every symbol groups into one
  '(containing module not compiled)' bucket" when run against real data — not caught by unit tests,
  which used a fixture object literally named "module" without checking what kind it actually
  needs to be.
- **New `ObjectKind::Custom(_)` variants must be added to `DefaultResolver`'s blanket-exclusion
  list on introduction, not discovered after the fact.** This is now the third occurrence of the
  identical failure shape (`Section`/`TransformNode` → `RustSymbol`/`RustModule`/`PythonSymbol`/
  `PythonModule` → `Crate`). Any new deterministically-self-identified object kind (already unique
  by some structural key — file path, manifest directory, source+index) with no distinguishing
  `columns`-like property is a candidate; the pattern is name-prefix/suffix similarity plus the
  same-kind structural-score fallback of 1.0 compounding past the 0.85 threshold.
- **Pass-level cache in `.ekos/artifacts/pass-manifests/` hashes input *artifacts*, not code.** A
  logic-only fix (the `DefaultResolver` change above) doesn't invalidate `ekos compile`'s cache on
  its own — `ekos compile` reported "skipping pass (cached)" and reused the pre-fix CKM until the
  cache directory was cleared by hand. No CLI flag exists yet to force recomputation of one pass.
- **`unzstd -c model.json.zst | python3 -c "json.load(...)"` is the fastest way to inspect the CKM
  directly** when debugging a compile-time discrepancy — faster than reasoning about the ledger's
  SQLite schema (payloads aren't stored as queryable JSON text in all backends; `sqlite3`'s bundled
  FTS5 build on this machine also doesn't support `contentless_delete`, so ad hoc `object_fts`
  queries fail outright) or writing a throwaway EKL query for every hypothesis.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0042-production-grade-curated-docs.md` | New RFC |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | New: `Cargo.toml` → `Crate`/`Technology` objects + `DependsOn` edges |
| `ekos/crates/recovery/src/cicd_analyzer.rs` | New: `.github/workflows/*.yml` → `Pipeline` objects |
| `ekos/crates/recovery/Cargo.toml`, `ekos/Cargo.toml` | `+toml`, `+serde_yaml` dependencies |
| `ekos/crates/cli/src/commands/recover.rs` | Register both new passes (file/YAML collection blocks mirroring the existing dependency-scan block) |
| `ekos/crates/docs-gen/src/lib.rs` | Rewrite `render_architecture`/`render_api`; extend `render_sequence_diagrams`; `+unique_page_file_names`, `+is_entity_page_kind` |
| `ekos/crates/cli/src/commands/docs.rs` | `generate_curated` writes per-entity detail pages; `render_api` call site updated |
| `ekos/crates/identity/src/lib.rs` | `+"Crate"` to `DefaultResolver`'s blanket kind-exclusion list, `+` regression test |
| `doc/*.md`, `doc/*.md` (1855 new entity pages) | Regenerated against this repo's own ledger |
