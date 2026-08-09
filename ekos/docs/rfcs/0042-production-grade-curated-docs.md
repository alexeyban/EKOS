# RFC 0042 — Production-Grade Curated Documentation (Program Entities, Crate Topology, CI/CD)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-09

---

## Motivation

RFC 0037's `--layout curated` (`README.md`/`Architecture.md`/`API.md`/`SequenceDiagrams.md`) was
run against EKOS's own repo and reviewed by the user. Verdict: not production-grade.
`Architecture.md` shows no real infrastructure, no program entities (functions/classes), and no
links between documents. Investigated directly against current source before writing this RFC
(two research passes, not assumed):

- `ekos/crates/recovery/src/rust_analyzer.rs` (RFC 0041) and `python_analyzer.rs` already emit
  real `KirObject(Custom("RustSymbol"))`/`Custom("PythonSymbol")`/`Custom("RustModule")`/
  `Custom("PythonModule"))` objects — each carries a `kind` property (function/struct/enum/trait,
  `rust_analyzer.rs:331-352`) and real `Contains` (module→symbol) and `Calls` (function→function,
  `rust_analyzer.rs:356-407`, 733 edges compiled in this repo today) relationships. Every one of
  these objects already gets a full detail page under `--layout objects`
  (`build_object_page_model`/`render_markdown_object_page`, `docs-gen/src/lib.rs:110-293`).
- The curated renderers never read any of this. `render_architecture`'s `## Components`
  (`docs-gen/src/lib.rs:852-949`) prints bare `count_by_kind` totals (`RustSymbol: 1301`) with no
  listing. `render_api` (`lib.rs:987-1031`) reads a stale text-scanned `File.symbols` string array
  from `build.rs` instead of the real `RustSymbol`/`PythonSymbol` objects. Relationship groups over
  20 edges (`Calls`, `Contains`, `CoupledWith`, `DependsOn` — all four fire in this repo) render as
  a one-line "diagram omitted" placeholder pointing at a CLI command, not a link. Nothing in the
  four curated files hyperlinks to a `--layout objects` page, even when one exists.
- `dependency_analyzer.rs` (RFC 0019) only pattern-matches ~25 hardcoded DB/infra
  connection-string literals in source text — it never parses `Cargo.toml`, so it has no view of
  EKOS's own crate dependency graph (internal, path-based) or its external crate dependencies
  (serde, tokio, syn, rusqlite, …). This is why `doc/Architecture.md`'s `## Technologies` section
  renders "_No technology dependencies compiled._" today: none of the 5 hardcoded substrings
  appear in EKOS's own Rust source.
- No analyzer or plugin anywhere reads `.github/workflows/*.yml` (`ci.yml`, `pages.yml` both
  exist at the repo root) or any other deployment/infra config. CI/CD is entirely unmodeled.
- `toml` is already a workspace dependency (`ekos/Cargo.toml:80`, used today only by
  `compiler-core/src/config.rs` to parse `ekos.toml` itself) — adding it to `recovery` is a
  one-line `Cargo.toml` change, not a new external dependency for the workspace as a whole. No
  YAML parser exists in the workspace yet; one is added for the CI/CD analyzer.

## Scope

1. A new `crate_topology_analyzer` pass: parses every `Cargo.toml` under the observed paths into
   `Custom("Crate")` objects, internal (path/workspace) `DependsOn` edges between them, and
   external-dependency `Custom("Technology")` objects (reusing RFC 0019's exact object kind) with
   `DependsOn` edges from the owning crate.
2. A new `cicd_analyzer` pass: parses `.github/workflows/*.yml` into `ObjectKind::Pipeline`
   objects (job/step structure as properties).
3. Rewrites of `render_architecture`, `render_api`, and `render_sequence_diagrams`
   (`ekos-docs-gen`) to surface real program-entity data, real crate/technology topology, and real
   CI/CD pipelines — with working links.
4. `--layout curated` starts writing one detail page per significant program-entity object
   (reusing the existing `--layout objects` page renderer, not a new one) so every link the
   curated files emit resolves to a real file from a single run.

## Non-goals

- No Docker/Kubernetes/Terraform/cloud-config parsing. Confirmed: EKOS's own repo has no
  Dockerfile, no k8s manifests, no IaC of any kind — only GitHub Actions workflows exist as real
  deployment/CI infrastructure to model. Adding parsers for infra formats this repo doesn't use
  would be speculative, unverifiable work; deferred until a workspace that has them needs it.
- Not a call-graph analyzer — `Calls` already exists (RFC 0041); this RFC only renders it, adding
  no new extraction logic for it.
- Not HTML output for curated (still deferred from RFC 0037's own Open Questions — unrelated to
  this RFC's scope).

## What already exists and is reused

- `ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind}`, `KirGraph` — same types
  every analyzer pass and every `docs-gen` renderer already uses.
- `DependencyAnalyzerPass`'s exact shape (`recovery/src/dependency_analyzer.rs:72-166`) — a
  `Vec<(rel_path, content)>` batched pass writing one `KnowledgeArtifact` — is the template both
  new passes follow, including its `Custom("Technology")` object kind (reused, not duplicated) and
  its `cache_inputs`/idempotent-append pattern.
- `recover.rs`'s existing file-collection idiom (`WalkDir` over `observe_paths` with the
  `ignore_patterns` filter, e.g. the `DEP_SCAN_EXTENSIONS` block at `recover.rs:141-199`) — both
  new passes are wired in the same way, filtering on filename (`Cargo.toml`) or path pattern
  (`.github/workflows/*.yml`) instead of extension.
- `docs-gen`'s `render_mermaid_graph` (generic over any object/relationship list, `lib.rs:453-503`)
  and `page_file_name`/`build_object_page_model`/`render_markdown_object_page`
  (`lib.rs:412-414`, `110-293`) — reused as-is for the new Architecture sections and for curated's
  new per-entity pages; no new diagram or page-rendering code is written.

## Design

### `crate_topology_analyzer.rs`

- `CrateTopologyAnalyzerPass::new(workspace_name, manifests: Vec<(String rel_path, String toml_content)>)`,
  mirroring `DependencyAnalyzerPass::new`'s signature exactly.
- Parses each manifest with `toml::from_str` into `[package]` (name, version, description) and
  `[dependencies]` (+ `[workspace.dependencies]` for the root manifest, since crate manifests use
  `dep.workspace = true` and the actual version lives in the root).
- One `Custom("Crate")` object per manifest with a `[package]` table; properties: `path`,
  `version`, `description` (empty string if absent — no invented text).
- For each dependency table entry: if it has a `path = "..."` key, resolve it relative to the
  manifest's directory and, if it matches another parsed crate's path, emit a `DependsOn` edge
  Crate→Crate; otherwise (no `path`, or path doesn't resolve to a known crate) emit/reuse a
  `Custom("Technology")` object for the dependency name (deduped by name across all manifests,
  same `technology_kir_id`-style deterministic id as `dependency_analyzer.rs:56-63`) with a
  `DependsOn` edge Crate→Technology, version as a property when a plain string version is given
  (skip recording a version for `{ workspace = true }` entries — the real version lives on the
  root manifest's `[workspace.dependencies]` entry, not worth resolving transitively for v1).
- Registered in `recover.rs` next to the `dep_pass`/`rust_pass` blocks: walk `observe_paths` for
  files literally named `Cargo.toml`, read content, pass the batch to the new pass.
- Tests mirror `dependency_analyzer.rs`'s: a 2-3 crate fixture (one root workspace manifest, one
  path dependency between two member crates, one external dependency) asserting the right
  `Crate`/`Technology` objects and `DependsOn` edges, plus an idempotency/dedup test for an
  external dependency declared in two crates.

### `cicd_analyzer.rs`

- `CicdAnalyzerPass::new(workspace_name, workflows: Vec<(String rel_path, String yaml_content)>)`.
- Parses each with the workspace's new YAML dependency (final crate choice — `serde_yaml` is
  unmaintained upstream; use `serde_yml` unless it's also unmaintained at implementation time, in
  which case fall back to `serde_norway`, its actively-maintained fork — pick whichever resolves
  cleanly with `cargo add` and note the actual choice in this RFC's Files Changed table once
  implemented).
- One `ObjectKind::Pipeline` object per workflow file: `name` from the YAML `name:` key (fallback
  to the file's stem if absent), properties = `{"triggers": [...on: keys...], "jobs": [{"name":
  ..., "steps": [...step names/run commands...]}]}` — flattened into JSON-serializable values the
  same way `RustSymbol`'s `kind` property is a plain string, not a nested typed struct. Evidence:
  one `SourceLocation::file(rel_path)` citing the workflow file itself.
- No relationships beyond what `Pipeline` already implies for this phase — a workflow-to-crate
  "builds" edge is future work, not blocking this RFC's stated gap (a CI/CD section existing at
  all, with real content, versus not existing).
- Registered in `recover.rs`: walk `observe_paths`, match files under a `.github/workflows/`
  path component with a `.yml`/`.yaml` extension.
- Tests: fixture workflow YAML (mirroring the shape of this repo's real `ci.yml`) → correct
  `Pipeline` object, job names, and step content; a workflow with no `name:` key falls back to the
  file stem; malformed YAML fails the single file's parse without aborting the whole pass (same
  per-item-resilience pattern `enrich_with_prose`, `docs.rs:228-243`, already uses for LLM calls).

### `render_architecture` rewrite (`docs-gen/src/lib.rs:852-949`)

- New `## Crate & Workspace Topology` section: `render_mermaid_graph` over `Custom("Crate")`
  objects and their `DependsOn` edges to other crates — the real internal-architecture graph
  CLAUDE.md's crate-map table currently only documents by hand.
- New `## CI/CD Pipelines` section: one sub-heading per `Pipeline` object with its jobs/steps as a
  nested bullet list — same prose style `render_readme`'s `## Contributors` already uses for a
  flat list of real objects.
- `## Components`: for `RustModule`/`RustSymbol`/`PythonModule`/`PythonSymbol`/`Crate` (module/
  symbol/crate kinds), replace the bare count with a count **plus** a markdown link to a new
  per-kind listing (grouped by containing module/crate, using existing `Contains` edges to
  establish grouping) — every entity name links to its individual detail page via
  `page_file_name`.
- Relationship sections over `MAX_GRAPH_EDGES` (`lib.rs:931`): keep the existing cap (a >700-edge
  Mermaid graph is genuinely unreadable) but replace the plain-sentence placeholder with a real
  markdown link to a new per-kind appendix page listing every edge as `[from](link) -> [to](link)`,
  generated the same way (reusing `page_file_name`) rather than telling the reader to run a
  different CLI invocation themselves.

### `render_api` rewrite (`docs-gen/src/lib.rs:987-1031`)

- Reads `Custom("RustSymbol")`/`Custom("PythonSymbol")` objects directly (their `kind` property
  and their `Contains` parent module) instead of `File.symbols`. Grouped by containing module,
  modules grouped by crate (via the new `Crate` `Contains`/path-prefix association). Each symbol
  line: `` `kind` `name` `` linked to its detail page. `File.symbols`-based rendering is removed —
  it was RFC 0037's explicit "closest real data available at the time" stopgap, now superseded by
  real `RustSymbol`/`PythonSymbol` data per the research above.
- Honest empty-state placeholder retained for workspaces with no compiled symbols (e.g. a
  non-Rust/Python workspace), matching RFC 0037's existing ethos.

### `render_sequence_diagrams` addition (`docs-gen/src/lib.rs:1053-1145`)

- New `## Call Sequences` section, built from real `RelationshipKind::Calls` edges grouped by
  caller's containing module — one small Mermaid `sequenceDiagram` per module with >0 calls,
  capped the same way `## Dependency Graph`'s per-kind cap works, to avoid a 733-edge unreadable
  diagram. RFC 0037's stated reason for skipping `Calls` ("never constructed anywhere... every hit
  is inside a test fixture") is stale — confirmed 733 real edges exist in this repo today via
  `rust_analyzer.rs`'s `CallVisitor`. The existing `FeedsInto`/`TransformNode` section (pipeline
  data-flow order) is unchanged and stays clearly labeled as distinct from this new code
  call-sequence section.

### Curated output becomes self-contained (`cli/src/commands/docs.rs::generate_curated`, `docs.rs:350-381`)

- After writing the four fixed files, `generate_curated` now also builds and writes one detail
  page per significant program-entity object (`Crate`, `RustModule`, `RustSymbol`,
  `PythonModule`, `PythonSymbol`, `Technology`, `Pipeline`) — calling the exact same
  `build_object_page_model`/`render_markdown_object_page`/`page_file_name` functions
  `generate()`'s `Layout::Objects` branch already calls (`docs.rs:118-139`). No new page-rendering
  code; this is the same per-object loop, scoped to the kinds this RFC's new Architecture/API
  sections link to, so a single `ekos docs generate --layout curated` run is enough to make every
  link resolve — a user should never need to also run `--layout objects` to follow a link.

## Alternatives Considered

- **Extend `dependency_analyzer.rs`'s pattern table with Cargo.toml-shaped literals** — rejected;
  Cargo.toml is structured TOML, not free text to pattern-match, and doing so would miss the
  crate-to-crate internal topology entirely (the actual "real infrastructure" the user asked for),
  not just external deps.
- **Full Docker/Kubernetes/Terraform infra analyzer now** — rejected per Non-goals: no such config
  exists in this repo to verify against; would be unfalsifiable, speculative code.
- **Generate curated per-entity pages via a link to `--layout objects` output instead of writing
  them from `generate_curated` directly** — rejected; would require the user to remember to run
  two commands and keep both outputs in sync, contradicting "a single command produces complete,
  self-consistent documentation."

## Open Questions

- [ ] Final YAML crate choice (`serde_yml` vs `serde_norway`) — resolved at implementation time
  based on which compiles cleanly; not a design-blocking decision either way.
- [ ] Whether `Crate`→`Crate` internal topology should also feed a future `Service`/`Api` object
  kind once a real API-surface analyzer exists — out of scope here, no such analyzer exists today.

## Testing

- Unit tests for both new analyzer passes (`crate_topology_analyzer.rs`, `cicd_analyzer.rs`),
  following `dependency_analyzer.rs`'s existing test shape exactly (fixture input → assert exact
  `KirGraph` objects/relationships).
- `ekos-docs-gen` golden-file-style tests for the rewritten `render_architecture`/`render_api`/
  `render_sequence_diagrams`, each covering real-data rendering and the honest empty-state
  placeholder when the relevant source data doesn't exist (same pattern RFC 0037's own tests use).
- CLI-level `docs.rs` tests: `--layout curated` writes the four fixed files **plus** the expected
  per-entity pages for a fixture ledger containing `Crate`/`RustSymbol`/`Pipeline` objects, and
  every link emitted in `Architecture.md`/`API.md` resolves to a file that was actually written in
  the same run.

## Acceptance Criteria

- [ ] All Open Questions resolved or explicitly deferred with rationale.
- [ ] `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D
      warnings && cargo fmt --check` clean from `ekos/`.
- [ ] `ekos docs generate --layout curated` run against EKOS's own committed ledger produces an
      `Architecture.md` with a real crate-dependency mermaid graph, a populated `Technologies`
      list, a populated `CI/CD Pipelines` section, and working relative links into individual
      entity pages; `API.md` lists real functions/types grouped by module with kind badges and
      links.
- [ ] `--layout objects` (unaffected default) and all pre-existing `docs.rs`/`docs-gen` tests
      still pass unmodified.

## Files Changed (planned)

| File | Change |
|---|---|
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | New pass: `Crate`/`Technology` objects + `DependsOn` edges from parsed `Cargo.toml` files |
| `ekos/crates/recovery/src/cicd_analyzer.rs` | New pass: `Pipeline` objects from parsed `.github/workflows/*.yml` |
| `ekos/crates/recovery/Cargo.toml` | `+toml.workspace = true`, `+<yaml crate>.workspace = true` |
| `ekos/Cargo.toml` | `+<yaml crate>` in `[workspace.dependencies]` |
| `ekos/crates/cli/src/commands/recover.rs` | Register both new passes, mirroring the `dep_pass`/`rust_pass` wiring |
| `ekos/crates/docs-gen/src/lib.rs` | Rewrite `render_architecture`, `render_api`; extend `render_sequence_diagrams` |
| `ekos/crates/cli/src/commands/docs.rs` | `generate_curated` writes per-entity pages for the kinds the curated docs now link to |
