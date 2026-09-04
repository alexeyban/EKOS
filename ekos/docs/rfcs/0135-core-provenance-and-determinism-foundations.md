# RFC 0135 — Core provenance & determinism foundations

**Status:** Accepted — all four parts implemented (`devlog_158` A, `devlog_159` D, `devlog_160` B,
`devlog_161` C)
**Author:** EKOS team
**Created:** 2026-09-04
**Builds on:** RFC 0004 (ledger design + the never-built audit trail), RFC 0043 (redaction),
RFC 0072/0076 (relationship-id determinism, piecemeal), RFC 0077 (fingerprint vs cleared ledger),
RFC 0060/0063 (identity merge threshold + review routing)
**Motivated by:** the "core standstill" review — `devlog_100` (a redaction fix couldn't re-scan
unchanged source, forcing a manual `.ekos` wipe), `devlog_112` (content-addressing invariant
violated by two code paths never kept in sync), and `TODO.md`'s three standing gaps:
per-entry `source_artifact_id`, the ~134 unaudited `KirRelationship::new()` call sites, and the
`DefaultResolver` kind-exclusion list that keeps being rediscovered live.

---

## Motivation

EKOS's positioning claim is "every conclusion carries the evidence it was derived from." That is
true today at the **evidence** level (`KirEvidence` → `SourceLocation`) but not at the **write**
level, and the graph it produces still accumulates real duplicate relationships and still has an
auto-merge path that can silently absorb a new object kind. Seven web-console increments shipped
while these foundations sat still. This RFC is four small, independently-mergeable parts that each
close one gap **and add a mechanical guard so the class of bug cannot silently return.**

| Part | Gap | Size | Ship |
|---|---|---|---|
| A | A redaction/analyzer logic change does not re-scan unchanged source | S | first |
| B | No `source_artifact_id` / `audit_trail()` — can't say which run wrote a ledger entry | M | second |
| C | ~134 `KirRelationship::new()` call sites unaudited for id determinism | M–L | third |
| D | `DefaultResolver`'s kind-exclusion list is enforced by memory, not code | S | any time |

**Not in this RFC:** `KnowledgeStore: Send` / RFC 0112 (its own RFC — noted as the real
concurrency story, out of scope here); resolving the RFC 0060 residual *fuzzy* mis-scores (no
confidence cutoff separates them — unchanged); retroactively de-duplicating rows already in a
committed ledger (no tombstone exists — render-time dedup stays).

---

## Part A — pipeline logic version in the build fingerprint

**Status: implemented 2026-09-04 (`devlog_158`).** Shipped slightly wider than proposed: the
fingerprint cache key folds in **both** `PIPELINE_LOGIC_VERSION` (code changes) **and an 8-hex
hash of the workspace's `RedactionConfig`** (per-workspace `[security]` changes), so a
`[security]` edit re-scans automatically with no constant bump. `build.rs`'s existing
`a_later_redaction_pattern_addition_actually_re_redacts_unchanged_source` test no longer needs its
`remove_dir_all(ledger)` — the key miss forces the rescan on its own.

### Problem

`crates/cli/src/commands/build.rs` skips a whole observe path when
`fingerprints.get(fp_key) == Some(&fp.0)`, where `source_fingerprint(&ctx)`
(`observation-sdk/src/lib.rs:143`) hashes only `(rel_path, size, mtime)` of each source file. A
fix to `ekos_common::redaction`, an analyzer, or any observe-path transform has **zero effect on
an unchanged file** — the stale (and possibly corrupted, per `devlog_100`) artifact is served
forever until a manual full `.ekos` reset. RFC 0077 fixed the adjacent "ledger cleared" case and
explicitly left this one.

### Design

A single `const PIPELINE_LOGIC_VERSION: u32` in `ekos_common` (or `observation-sdk`). It is mixed
into the **fingerprint cache key**, not the fingerprint value and not any user-facing output:

```rust
// build.rs
let fp_key = format!("{}@v{}", base.display(), ekos_common::PIPELINE_LOGIC_VERSION);
```

Bumping the constant invalidates every path's cached fingerprint at once, forcing exactly one
real re-scan; the re-scan then re-derives artifact ids from post-redaction content (RFC 0072
bug-1's fix already handles the rest), so genuinely-changed artifacts get persisted.

- **Bump discipline:** the constant's doc comment lists what counts — any change to
  `ekos_common::redaction`, an `Observer::scan` body, `walk_observed`, or the inline `File`-object
  construction in `build.rs`. A `CHANGELOG`-style comment block records each bump and why.
- **`ekos config preview-scan` is unaffected** — it answers "what would `build` observe" and must
  stay a pure function of the source tree and `[observe]` config. The logic version lives only in
  `build.rs`'s cache-key string.
- **Migration:** none. An old `fingerprints.json` keyed by the bare path simply misses on the
  first run after upgrade (one re-scan), then is rewritten with the versioned keys.

### Tests

- `a_logic_version_bump_forces_a_rescan_of_unchanged_source` — build, bump the constant in the
  test via a seam (`fp_key` builder takes the version as an arg), build again, assert the observer
  ran and a fresh artifact was written even though `source_fingerprint` is identical.
- `an_unchanged_logic_version_still_skips` — the RFC 0077 regression guard, unchanged.

---

## Part B — ledger entry provenance (`source_artifact_id` + `audit_trail`)

**Status: implemented 2026-09-04 (`devlog_160`).** `WriteContext` on the handle as designed
below; **SQLite** = 3 nullable `entries` columns via `ALTER TABLE … ADD COLUMN` on open (no
`user_version` bump — additive is enough); **FactLedger** = a `<root>/provenance.jsonl` sidecar
keyed by `tx` (no segment-format change — §6.2's cleaner option). `audit_trail(id)` on both;
`ekos ledger audit <id> [--json]` + the read-only `ekos_audit` MCP tool. `commit` stamps
`(run_id, "commit[:stage]", ckm-hash)`; `build` stamps `(run_id, "build", observation ArtifactId)`
per File object. MVP scope as written — run+stage everywhere, artifact-level for `build`'s File
objects; per-`KnowledgeArtifact` through `compile` still a follow-up.

### Problem

`LedgerEntry` carries `id / entry_type / payload / written_at` and nothing else
(`ledger/src/lib.rs:104`). "Which pipeline run — and ideally which `KnowledgeArtifact` — produced
this exact write" is unanswerable. RFC 0004 §"What the original plan called for that was never
built" documents this precisely.

### Design — a `WriteContext` on the handle, not a parameter on every `append_*`

Adding a param to the four `KnowledgeStore::append_*` methods means touching the trait, the
impl macro, both backends, and ~60 call sites. Instead:

```rust
pub struct WriteContext {
    /// A ULID minted once per `ekos build` / `ekos commit` invocation.
    pub run_id: String,
    /// "build" | "commit" | "commit:rollup" | "commit:lineage" | "review" | "identity" | …
    pub stage: &'static str,
    /// The originating artifact, where the caller knows it (build's File objects do;
    /// commit knows the CKM model hash; per-`KnowledgeArtifact` propagation is a follow-up).
    pub source_artifact_id: Option<ArtifactId>,
}

trait KnowledgeStore {
    /// Default no-op; both backends store it and stamp subsequent writes until cleared.
    fn set_write_context(&self, _ctx: Option<WriteContext>) {}
}
```

`append_inner` (FactLedger) / the `entries` insert (SQLite) records the active context next to
`written_at`. `build` sets a context with the real observation `ArtifactId` before the inline
`File`-object loop; `commit` sets one per stage with the CKM content hash as `source_artifact_id`.

**Storage:**
- SQLite: three new nullable columns on `entries` (`run_id TEXT`, `stage TEXT`,
  `source_artifact_id BLOB`). V2 schema, additive — a `user_version` bump with a one-way
  `ALTER TABLE ADD COLUMN` on open; old rows read back `NULL` (honest — provenance genuinely
  unknown for pre-0135 writes).
- FactLedger: the context is one synthetic fact per entry version under a reserved
  `__provenance` attribute path, or a sidecar column in the segment index — decided at
  implementation, whichever keeps `reconstruct` untouched.

**Reader:**

```rust
fn audit_trail(&self, id: &KirId) -> Result<Vec<AuditRecord>, LedgerError>;
// AuditRecord { entry_rowid, written_at, run_id: Option<String>, stage: Option<String>,
//               source_artifact_id: Option<ArtifactId>, content_changed: bool }
```

`content_changed` = did this write produce a new version (vs. a dedup no-op). Exposed as
`ekos ledger audit <object-id>` and, read-only, as the `ekos_audit` MCP tool.

### Scope line

MVP = **run + stage** provenance for every write, **artifact-level** for `build`'s `File` objects.
Per-`KnowledgeArtifact` provenance for `recover`/`compile`-derived objects needs CKM objects to
carry back-references through `compile` — a real follow-up, called out, not attempted here.

### Tests

- Write an object in two `commit` runs from changed input → `audit_trail` returns two records,
  distinct `run_id`, `content_changed: true` then depends.
- A dedup no-op re-commit → one record, `content_changed: false` on the second.
- `build` File object → `source_artifact_id` is the real observation artifact id.
- Both backends; a pre-0135 SQLite ledger opens, migrates, and reads old rows as `NULL`
  provenance without error.

---

## Part C — `KirRelationship::new()` determinism sweep

**Status: implemented 2026-09-04 (`devlog_161`).** `KirRelationship::deterministic(kind, from,
to, discriminator)` added to `ekos-kir`. Every **producer** call site swept in one pass (not
analyzer-by-analyzer — they were all the same `(from, to, kind)` shape once surveyed): ~24 bare
`::new` sites across `recovery/` + `semantic/` + `cli/commands/identity.rs` converted, all with
`discriminator = ""`. The ~7 sites that already assigned `rel.id = <helper>` (RFC 0072/0076/0092
— `crate_topology`, `sql_analyzer` FK, `python_analyzer` FK/Extends, `dbt_analyzer`,
`data_lineage`, `package_json`) are **left as-is** — converting them would change their ids and
rewrite every existing ledger. Guard: `no_bare_relationship_new_in_production_code` in both
`ekos-recovery` and `ekos-semantic` (strips `#[cfg(test)]` modules, then requires every
`KirRelationship::new(` be followed within 600 chars by `.id =`). The ~175 render/query/sim
sites were confirmed out of scope and untouched.

### Problem

`KirRelationship::new` assigns `KirId::new()` (random). RFC 0072 fixed the one observed case
(`crate_topology_analyzer` `DependsOn`); its own note says *"the other 134 call sites remain
exposed… each needs the same kind of case-by-case investigation, not a batch fix,"* with
`sql_analyzer`'s `ForeignKey` (two real FKs between the same tables via different columns share
`(from,to,kind)`) as the standing counter-example against a blanket rule.

### Design

**1. Scope is the producer set, not the grep count.** Of ~230 `KirRelationship::new(` call
sites, the ones that matter are those whose output is persisted via `append_relationship` on a
`recover`/`compile`/`commit` path:

| Bucket | Files | In scope? |
|---|---|---|
| Analyzer passes | `recovery/src/*_analyzer.rs`, `semantic/src/*` | **yes** (~55) |
| CLI write paths | `cli/src/commands/{recover,commit,identity,docs}.rs` | **yes** |
| Render-time | `docs-gen` (73), `dbt-gen`, `runtime/graph_export`, `ekl/interpreter`, `cli/commands/mcp` | **no** — throwaway objects, never appended; one sentence in the RFC records why |
| Test helpers / fixtures | `ledger`, `kir`, `*/tests` | **no** |
| Simulation | `simulation/src/*` | **separate** — its own append path, RFC 0047; audited but tracked distinctly |

**2. A real constructor, so the fix is one call not three lines.**

```rust
impl KirRelationship {
    /// Deterministic id from `(kind, from, to, discriminator)` — `discriminator` is `""` for the
    /// common "at most one edge of this kind between these two" case, or a real distinguishing
    /// key (a column name, an import alias) where more than one is legitimate.
    pub fn deterministic(kind: RelationshipKind, from: KirId, to: KirId, discriminator: &str)
        -> Self;
}
```

Mirrors the existing `Uuid::new_v5(NAMESPACE_URL, …)` pattern the analyzers hand-roll today
(`crate_topology_analyzer.rs:78`).

**3. Per-call-site decisions** land as a table in the RFC's appendix (one row per producer
call site: file:line, kind, chosen `discriminator`, rationale). The default is `""`; a
non-empty discriminator requires a named real counter-example.

**4. A guard test** in `recovery` + `semantic`: greps their own `src/` for `KirRelationship::new(`
and fails with the list, directing new code to `::deterministic`. (`::new` stays public for the
render-time callers.)

### Tests

- Per fixed analyzer: three `recover` cycles against a disposable workspace produce the same
  relationship ids, not a growing count (the RFC 0072 live-verification shape, as a test).
- `sql_analyzer` FK-via-two-columns: both edges survive (discriminator = column pair).
- The guard test itself.

---

## Part D — make the identity kind-exclusion list a compile-time / test-time invariant

**Status: implemented 2026-09-04 (`devlog_159`).** `ekos_kir::custom_kinds::REGISTRY` is the
single source of truth (22 kinds, `structurally_keyed` per row); `DefaultResolver` derives its
exclusion set from `is_structurally_keyed()`; the `ekos-identity` test
`every_pipeline_custom_kind_is_registered` walks `crates/{recovery,semantic}/src` and fails CI on
any unregistered `Custom` kind. Shipped **4 latent over-merge fixes** as a side effect —
`Page` / `Risk` / `Rollup` / `ProjectSummary` were all structurally keyed but never excluded.

### Problem

`CLAUDE.md` spells it out: every new `ObjectKind::Custom(_)` that is self-identified by a
structural key (file path, manifest dir, source+index) **must** be added to `DefaultResolver`'s
blanket kind-exclusion list, and `Section`/`TransformNode`/`RustSymbol`/`RustModule`/`PythonSymbol`/
`PythonModule`/`Crate`/`ElixirModule`/`ElixirSymbol`/`JsModule`/`Document` have *all* hit the same
over-merge failure — several found live weeks after their analyzer shipped. The list is enforced
by reviewer memory.

### Design

- **A single registry.** Each `Custom(_)` kind an analyzer emits is declared once, with a
  `structurally_keyed: bool`. `DefaultResolver` derives its exclusion set from
  `structurally_keyed == true` instead of a hand-maintained literal list.
- **A test** that enumerates every `ObjectKind::Custom` string produced anywhere in `recovery/`
  (grep or a small inventory fn) and asserts each is present in the registry — a new kind that
  skips the registry fails CI, not a generated entity page weeks later.
- No behaviour change for the kinds already on the list; this is the guard the list always
  needed.

### Tests

- The inventory/registry-coverage test.
- A regression test per historically-failed kind (some already exist — consolidate them).

---

## Appendix — Part C per-call-site decisions

**Converted to `::deterministic(_, _, _, "")`** — one edge of this kind per ordered pair:

| Analyzer / module | Edge | Endpoints (both already deterministic) |
|---|---|---|
| `dependency_analyzer` | `DependsOn` | file → Technology |
| `crypto_analyzer` | `Custom(kind)` | sentinel entity → entity |
| `confluence_analyzer` | `Contains`, `References` | page → page |
| `document_semantics_analyzer` | `References`, `Custom(kind)` | section → concept, concept → concept |
| `git_analyzer` | `OwnedBy`, `CoupledWith` | commit → contributor, file → file |
| `github_analyzer` | `References` ×3 | item → file / item |
| `elixir_analyzer` | `Contains` ×2, `DependsOn` ×2 | file → module, module → symbol / Technology / module |
| `rust_analyzer` | `Calls`, `DependsOn`, `Contains` | symbol → symbol, file → module |
| `javascript_analyzer` | `DependsOn`, `Contains` | file → module / symbol |
| `python_analyzer` | `DependsOn` ×2, `Contains` | file → module / symbol / ORM Table |
| `local_docs_analyzer` | `Contains` ×2 | document → table / section |
| `semantic::rollup` | `Contains` | rollup → member |
| `semantic::lib` | `Custom("SameAs")`, `References` | canonical ↔ member, Risk → target |
| `semantic::transform_ir` | `Custom("FeedsInto")` | node → node |
| `cli::commands::identity` | `Custom("SameAs")` | cross-system candidate a ↔ b |

**Left as-is** (already `rel.id = <helper>`, changing it rewrites ledgers): `crate_topology_analyzer`
(`depends_on_kir_id`), `sql_analyzer` (`foreign_key_kir_id`, with `fk_desc` — the standing
counter-example), `python_analyzer` (`orm_foreign_key_kir_id`, `extends_kir_id`), `dbt_analyzer`
(`dbt_depends_on_kir_id`), `semantic::data_lineage` (`reads_writes_kir_id`), `package_json_analyzer`
(`depends_on_kir_id`).

**Out of scope** (throwaway objects, never `append_relationship`'d): `docs-gen` (73),
`runtime::graph_export` (8), `dbt-gen`, `ekl::interpreter`, `cli::commands::mcp`, all of
`simulation/` (its own identity model — sibling RFC if ever needed), every `#[cfg(test)]` module.

---

## Rollout

A → merge (unblocks confident redaction/analyzer fixes immediately).
D → merge (small, pure guard).
B → merge (schema-additive, both backends).
C → land analyzer-by-analyzer behind the guard test; each analyzer's fix is its own small PR
citing this RFC.

Each part: local `fmt` / `clippy --workspace` / `test --workspace` / `tests/integration` /
`web/api` pytest, `[skip ci]`, `--no-ff` to `main` — per the maintainer's standing instruction.
A devlog per part (or one per batch of C).

---

## Open questions

1. **`PIPELINE_LOGIC_VERSION` — manual bump vs. derived.** A hash of the relevant source files at
   build time would never be forgotten but makes every dev-build a cache miss. Manual `u32` with a
   documented bump rule is the proposal; revisit if it's missed once.
2. **FactLedger provenance storage** — synthetic `__provenance` fact vs. segment-index sidecar.
   Implementation-time call; the reader contract (`audit_trail`) is fixed regardless.
3. **Part C discriminator for LLM-derived edges** (`document_semantics_analyzer`,
   `llm_description`) — an LLM pass is non-deterministic by nature; these may need a content-hash
   discriminator or an explicit "not deterministic, dedup at render" carve-out, same honesty as
   RFC 0088's own status note.
4. **Simulation** (`simulation/src/*`, ~20 call sites) — its writes go through `&dyn KnowledgeStore`
   like `commit` does. Fold into Part C or a sibling RFC? Proposal: sibling, since its identity
   model (agent actions, forum events) is genuinely different from recovered structural facts.
