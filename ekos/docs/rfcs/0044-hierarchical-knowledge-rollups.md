# RFC 0044 — Hierarchical Knowledge Rollups (+ a prerequisite object-identity fix)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-10

---

## Motivation

The user's framing: Claude can reverse-engineer a codebase into documentation, but hits its own
context-window ceiling on extra-huge projects and on many projects at once — how should EKOS help
close that gap, including building real relationships/documentation across many repos, docs,
databases, and pipelines?

Two research passes established what's real today versus what's a genuine gap, so this RFC builds
on fact, not assumption:

**Already real and working — retrieval-based context efficiency:**
- `AiRuntime::gather_context` (`runtime/src/ai.rs:130-158`) is a hard-capped retrieve→expand→ground
  pipeline: top-3 search matches (`config.max_matches`, default 3) each expanded by a 1-hop
  neighborhood walk (default depth 1) — a fixed seed fan-out, not a token budget.
- MCP tools cap result size individually: `ekos_search` at 50 hits (`ledger/src/fact_ledger.rs:408`),
  `ekos_diff` at 200 (`cli/src/commands/mcp.rs:404`, explicit comment: "cap the listing so a
  full-rebuild window stays consumable"), `ekos_impact` bounds hops (default 5). `ekos_ekl` has no
  implicit `LIMIT` — the one MCP surface without a built-in ceiling.
- Incremental re-scan works at two coarse levels: a whole-tree fingerprint per `[observe] paths`
  entry (`observation-sdk/src/lib.rs:90-101`) skips a connector's rescan entirely when nothing
  under it changed; per-pass manifest caching (`compiler-core/src/cache.rs`) skips a compiler pass
  when its content-addressed inputs are unchanged.
- Cross-*system* identity resolution (RFC 0029) links the same real-world entity observed by
  different connectors — proven, human-reviewable — but designed and tested for cross-connector
  matching within one ledger, not cross-*project* linking.

**This is genuinely context-efficient, but purely by retrieval limiting — fewer raw facts, never a
synthesized higher-level fact.** For a genuinely huge subsystem, or a whole project in a
many-project estate, an agent still has to personally compress dozens-to-hundreds of raw objects
into an understanding within its own context. Searched `identity`, `runtime`, `semantic` for
`summar`/`rollup`/`hierarch` — zero hits on ledger objects. **No rollup/summarization layer exists
anywhere.** Nothing in `TODO.md` or any existing RFC proposes this (RFC 0034, Draft, is the nearest
adjacent idea, but partitions ledger *storage* by connector-volume `source_scope` for throughput,
not objects by project/subsystem for navigability — orthogonal, not overlapping).

**A prerequisite correctness bug, found while investigating multi-project support for this RFC:**
`KirObject` carries no project/workspace provenance at all (`kir/src/lib.rs:188-199`), and
`plugins/file/src/lib.rs:67` computes each file's path relative to `ctx.workspace_root` (one
specific `[observe] paths` entry — `base` in `build.rs`'s per-path loop), which `build.rs:191`
(pre-fix) then hashed with **no project qualifier**
(`Uuid::new_v5(NAMESPACE_URL, rel_str.as_bytes())`). In a multi-project estate (the real
`~/PycharmProjects/ekos.toml`, ~40 `[observe] paths` entries, is a live example of exactly this
shape), two unrelated projects that each happen to have e.g. `src/main.rs` at the same relative
path silently collided into **one merged `KirObject`**. Confirmed by direct code reading that this
is specific to `Observer`-plugin-produced ids (file, and by the same `ScanContext.workspace_root`
convention, git/pentaho/python/rust/localdocs/github/confluence's own analyzer-side id derivation —
`github_analyzer.rs:file_kir_id`, `local_docs_analyzer.rs:document_kir_id`/`table_kir_id`/
`section_kir_id`, `rust_analyzer.rs:111`/`python_analyzer.rs:127` reading a `data.path` field the
observer itself embedded — all share the identical structural risk). This session's own new
recovery passes (`crate_topology_analyzer`, `cicd_analyzer`, the SQL/dependency-scan blocks in
`recover.rs`) were confirmed **not** affected — they derive paths relative to the overall `cwd`,
already implicitly project-qualified in a shared-estate setup.

## Scope

1. **Fixed in this pass**: `build.rs`'s File-object identity (the primary, universally-exercised
   case, and the one every rollup-grouping decision below traces back to — `RustSymbol`/
   `PythonSymbol`/etc. attribute to a rollup transitively through their containing `File`).
2. **Confirmed but explicitly deferred**: the identical risk in `github_analyzer.rs`,
   `local_docs_analyzer.rs`, `rust_analyzer.rs`/`python_analyzer.rs`'s artifact-embedded `data.path`
   fields, and git's `CoupledWith` file-pair ids. Fully closing this requires either (a) a second,
   central fix — qualifying `ObservationArtifact.content.target` itself (and any connector-embedded
   `data.path` mirror of it) at RFC 0043's existing redaction choke point in `build.rs`, before any
   analyzer ever reads it — or (b) touching each of the ~7 analyzer-owned id schemes individually.
   Neither was done this pass; tracked explicitly here rather than silently left unrecorded, same
   honesty standard as every prior RFC's Non-goals section in this project.
3. **New in this pass**: a deterministic, zero-LLM hierarchical rollup pass (`ekos_semantic::rollup`),
   plus the fix above.

## Non-goals (this pass)

- Full remediation of every analyzer-owned id scheme listed under "confirmed but deferred" above.
- A dedicated `ekos_summarize` MCP tool — rollups are ordinary `KirObject`s, so `ekos_search`/
  `ekos_neighborhood`/EKL already surface them for free; a tool that jumps straight to the nearest
  enclosing rollup for a given object is a natural, non-blocking follow-up.
- Per-sub-project curated docs generation (`ekos docs generate` scoped to one project within a
  shared estate ledger) — a real, separately-confirmed gap, but not this pass's priority; the new
  `"project"` `KirObject` property this RFC adds is exactly what a future pass would key off of.
- Opt-in LLM-written natural-language synthesis per rollup (mirroring `docs-gen`'s `--prose`,
  RFC 0035 Phase 5) — structural rollups (counts, boundary relationships, real `Contains` links)
  ship first; prose-per-rollup is designed the same reuse-not-reinvent way but implemented as a
  follow-up once the structural layer is proven against real multi-crate/multi-project data.

## Design

### Phase 1 — project-qualified object identity (`build.rs`)

Per `base` in the observe-paths loop, compute `project_key`: `base`'s own path relative to `cwd`,
**empty for the single-path case** (`paths = ["."]`, the overwhelmingly common setup) — existing
single-project ledgers keep byte-identical ids, no migration. When non-empty, `project_key` is
folded into the id-hash input for every id `build.rs` derives from `rel_str` (`{project_key}:{rel_str}`,
never mutating `rel_str` itself — `content.target`/`abs_path`/the object's own display name and
`"path"` property stay the plain within-project path), and stamped as a new `"project"` property on
the `KirObject` — the property Phase 2's project-grouping key reads.

### Phase 2 — hierarchical rollups (`ekos_semantic::rollup`)

One new deterministic function, `synthesize_rollups(graph: &mut KirGraph, depth: usize)`
(`ekos_semantic::rollup`) — but **not** wired into `SemanticCompilerPass` (`ekos compile`) as
originally designed, corrected after a real-pipeline run surfaced the reason why: `File` objects,
the only kind rollups group by directly, are written straight to the ledger by `ekos build`
(`cli/src/commands/build.rs:188-260`), never through a `KnowledgeArtifact` in the artifact store —
so `SemanticCompilerPass`'s `combined`/`resolved` graph (built purely from `KnowledgeArtifact`s)
never contains a single `File` object, and rollup synthesis there would silently do nothing on
every real workspace. (Caught by running the full pipeline against this repo's own ~4,500-object
ledger post-implementation and finding zero `Rollup` objects in the output — not caught by the
unit tests, which construct `File`-bearing fixture graphs by hand and never exercise this real
data-flow boundary.) Rollup synthesis instead runs in **`ekos commit`**
(`cli/src/commands/commit.rs::commit_rollups`) — the first point in the pipeline where `File`
objects (written earlier by `ekos build`) and CKM-derived objects (just written by this same
`commit` invocation) coexist in one place: read the ledger's full current object/relationship set,
run `synthesize_rollups` against an in-memory `KirGraph` built from it, then append only the
newly-produced `Rollup` objects/relationships/evidence (a re-run against unchanged input is a
no-op, since `append_object` on an already-known deterministic id does nothing). Rollups are
still just ordinary `KirObject`s by the time anything downstream sees them — no new query surface
needed.

- **Grouping key** (`group_key_for`): `"project:<value>"` when an object carries the Phase 1
  `"project"` property (multi-project estates — the coarser, more meaningful boundary when
  present); otherwise `"dir:<first N path components>"` from its `"path"` property (every `File`
  object has one) — `N` defaults to 3 (`DEFAULT_DIRECTORY_DEPTH`), tuned for a Cargo workspace
  (`ekos/crates/kir/src/lib.rs` groups at crate granularity, not the far coarser `ekos` or
  `ekos/crates`) but genuinely project-structure-dependent, so callers may override it.
- **Only `File` objects are direct rollup members.** Everything else (`RustSymbol`, `PythonSymbol`,
  …) is reachable transitively through the `File`→symbol `Contains` edges recovery passes already
  emit — not double-counted as a direct member, keeping the rollup's own `Contains` edge count
  linear in file count, not total object count.
- **A group must have ≥2 members to become a rollup** (a lone file is just that file — no summary
  value), **and there must be ≥2 groups overall** (if the whole graph is one group, "rolling it up"
  doesn't distinguish it from the graph itself).
- Each `Rollup` (`ObjectKind::Custom("Rollup")`) carries: `group_key`, `member_count`, `components`
  (member count broken down by `ObjectKind`, same idiom as `docs-gen::count_by_kind`), and
  `boundary_relationships` (relationship-kind counts for edges whose *other* endpoint sits outside
  the group — the real "what does this subsystem depend on / who depends on it" signal, computed
  once per rollup by scanning the graph's relationships for each member).
- Linked to every member via the **existing** `RelationshipKind::Contains` — no new relationship
  kind, so `ekos_search`/`ekos_neighborhood`/EKL understand rollups with zero new code.
- Deterministic id: `Uuid::new_v5(NAMESPACE_URL, "rollup:{group_key}")` — stable across
  recompiles, same convention every other analyzer in this codebase uses.

## Alternatives Considered

- **LLM-driven summarization as the primary mechanism** — rejected for v1; matches this project's
  consistent "structural first, LLM opt-in on top" pattern (RFC 0019's pattern table,
  `docs-gen`'s `--prose`). A rollup's counts/boundary-relationships are real, zero-cost, and
  immediately useful; prose synthesis can layer on top later without redesigning the structural
  layer.
- **A brand-new relationship kind for rollup membership** (e.g. `SummarizedBy`/`PartOf`) — rejected;
  `Contains` already means exactly this ("X contains Y") everywhere else in the codebase
  (File→RustSymbol, and now Rollup→File), and reusing it means every existing query surface
  understands rollups immediately.
- **Fixing every analyzer's id-collision risk in this same pass** — rejected as scope creep beyond
  what's needed to make Phase 2 sound; File-object identity (fixed) is what every rollup-grouping
  decision actually traces back to. Recorded explicitly as deferred, not silently dropped.

## Testing

- `ekos_semantic::rollup` unit tests (`semantic/src/rollup.rs`): directory-prefix grouping at a
  given depth; a single group covering the whole graph produces no rollup; `"project"` property
  takes priority over directory grouping; a rollup links to every member via `Contains` while a
  single-member group produces neither a rollup nor a `Contains` edge; `boundary_relationships`
  correctly counts edges crossing the group boundary.
- `cli/tests/skeleton.rs`: Phase 1 regression — a 2-project fixture with a same-relatively-named
  file in each produces two distinct `KirObject`s (not one merged object) after `ekos build`, each
  carrying the correct `"project"` property; the common single-path case gains no `"project"`
  property at all (no behavior change for the overwhelmingly common setup).
- Full pipeline against this repo's own ledger (`ekos build && recover && resolve && compile &&
  commit`): confirmed the pre-fix design (rollups computed in `SemanticCompilerPass`) produced
  zero `Rollup` objects against ~4,500 real committed objects — the bug described in Design above
  — and confirmed the corrected `commit.rs`-based design produces real rollups (e.g. one per
  `ekos/crates/<name>` directory) against the same ledger.
- Full workspace: `cargo build/test/clippy/fmt` clean, matching every prior RFC this session.

## Acceptance Criteria

- [x] Phase 1: single-path workspaces produce byte-identical ids to before this RFC (verified by
      `build_single_project_workspace_has_no_project_property`).
- [x] Phase 1: a 2-project fixture with a name-colliding file no longer merges into one object
      (verified by `build_keeps_same_named_files_in_different_projects_distinct`).
- [x] Phase 2: rollups appear in the CKM/ledger for real multi-file, multi-directory data with
      correct counts and boundary-relationship data — verified against this repo's own ~4,500-object
      ledger after correcting the integration point from `ekos compile` to `ekos commit` (see
      Design); the original placement was unit-tested but never real-pipeline-verified, which is
      exactly how it went unnoticed until this RFC's own acceptance check caught it.
- [ ] Deferred items (analyzer-owned id-collision risk beyond `File`, per-sub-project curated docs,
      opt-in rollup prose, `ekos_summarize` MCP tool) tracked in `TODO.md`, not silently dropped.

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/cli/src/commands/build.rs` | Phase 1: `project_key` computation + id-hash qualification + new `"project"` property |
| `ekos/crates/cli/tests/skeleton.rs` | Phase 1 regression tests |
| `ekos/crates/semantic/src/rollup.rs` | New: `synthesize_rollups`, `group_key_for`, tests |
| `ekos/crates/semantic/src/lib.rs` | `+pub mod rollup;` (re-exported; **not** called from `SemanticCompilerPass` — see Design) |
| `ekos/crates/cli/src/commands/commit.rs` | `+commit_rollups`, called after CKM objects/relationships are written, before the summary print |
| `ekos/crates/docs-gen/src/lib.rs` | `is_entity_page_kind` includes `"Rollup"`; `render_architecture` gains a `## Subsystems` section |
