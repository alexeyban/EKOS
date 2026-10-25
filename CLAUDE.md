# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

EKOS has an implemented Rust (2024 edition) Cargo workspace — this is not a design-phase repo.
Read `devlogs/devlog_*.md` (numbered chronologically — the highest-numbered file is the latest,
`devlog_296.md` as of this writing) before starting non-trivial work: they are the project's
long-term memory and record what shipped, why, and what was learned. `TODO.md` tracks the
phase-by-phase roadmap; RFCs are split across two locations for historical reasons, not a
meaningful distinction — `docs/rfcs/` (repo root) has `0001`–`0024`,
`ekos/docs/rfcs/` has `0025`+ (highest: `0222`; 0213–0218 are reserved by the Oracle/iDempiere plan for Parts II–V). **Check both directories for the highest existing number before
picking one for a new RFC** — two RFCs have already collided on the same number once (0027) from
sessions that only checked one location.

**The Cargo workspace root is `ekos/`, not the repo root** — there is no top-level `Cargo.toml`.
`benchmark/` and `tests/integration/` are separate Cargo workspaces that depend on the `ekos/`
crates by path.

## Commands

```bash
# Main workspace (run from ekos/, or pass -p <crate>/--manifest-path from elsewhere)
cd ekos
cargo build --workspace
cargo test --workspace
cargo test -p ekos-ledger                    # one crate, e.g. ekos-ledger, ekos-plugin-git
cargo test -p ekos-ledger some_test_name     # one test
cargo clippy --workspace -- -D warnings      # CI fails on any warning
cargo fmt --check                            # CI checks formatting, doesn't fix it
../scripts/cognitive-complexity.sh           # functions over Sonar's complexity limit (S3776), no scan needed
../scripts/test-install.sh                   # install.sh against a local fake release (RFC 0179)
../deploy/e2e/package.sh                      # full package on Linux end to end: setup --answers, the console
                                             # (local sign-in, settings + restart, pipeline), the wizard in a
                                             # browser, uninstall — HOME is a scratch dir. RFC 0185
../scripts/package-release.sh                # offline install package (Linux x86_64, glibc + static musl,
                                             # SHA256SUMS, install.sh, INSTALL.md) → dist/; committed tree only
../deploy/e2e/run.sh                         # the production image end to end: Dex sign-in, two tenants,
                                             # roles, gateway, an MCP client, every page in a browser,
                                             # backup/restore. Needs `docker build -t ekos:e2e .`
                                             # first; nothing else covers real OIDC or the image. No model call
../scripts/audit.sh                          # RustSec advisories + stale-lockfile check, all 3
                                             # workspaces (CI: audit.yml, also weekly). Run it
                                             # locally too: `[skip ci]` commits bypass CI

# Integration tests (separate workspace, depends on ekos/ crates by path)
cd tests/integration && cargo test

# Benchmarks (separate workspace, Criterion)
cd benchmark && cargo bench
cargo bench --bench ledger_write             # one benchmark file

# CLI (binary crate name is `ekos`, package is crates/cli)
cargo run -p ekos -- init
cargo run -p ekos -- build && cargo run -p ekos -- recover && cargo run -p ekos -- resolve \
  && cargo run -p ekos -- compile && cargo run -p ekos -- commit   # full pipeline, in order
cargo run -p ekos -- doctor
cargo run -p ekos -- project add shop --source <dir>   # RFC 0178: config + state under ~/.ekos,
cargo run -p ekos -- --project shop build               # sources never written to
cargo run -p ekos -- home backup                        # RFC 0183: the home as one .tar.zst;
cargo run -p ekos -- home restore <file>                # replaces nothing without --force
cargo run -p ekos -- ask "<question>"        # compiled REASON answer (--classic / --explain)
cargo run -p ekos -- query find "<text>" --mode hybrid   # SEARCH: BM25 + vector, RRF-fused (RFC 0118/0125)
cargo run -p ekos -- ekl "FIND Object WHERE kind = 'Table'"  # Enterprise Knowledge Language
                                             # (entities are Object / Relationship only)
cargo run -p ekos -- identity scan          # cross-system candidate matches (RFC 0029)
cargo run -p ekos -- marketing publish      # devlog -> tweet -> approval -> X (RFC 0030)
cargo run -p ekos -- marketing post <file> --confirm   # a file's text to X as written (no LLM, no 280 cap)
cargo run -p ekos -- mcp serve --workspace <dir>
cargo run -p ekos -- docs generate --layout curated --output doc   # README/Architecture/API/
                                             # SequenceDiagrams + per-entity pages (RFC 0035/0037/0042)
cargo run -p ekos -- simulate <scenario.yaml>       # World Engine: load + run a scenario (RFC 0047-0055)
cargo run -p ekos -- replay <scenario.yaml>         # read back a previously recorded simulation, read-only
cargo run -p ekos -- semantics gaps                 # RFC 0170 (`[semantics] enabled = true`): business-semantics
cargo run -p ekos -- export linkml --status hypothesis --out model.yaml   # hypotheses → draft LinkML
```

CI (`.github/workflows/ci.yml`) runs build+test+clippy+fmt from `ekos/` and `cargo bench` from
`benchmark/` on every push/PR to `main`. Match these locally before pushing.

## High-Level Architecture

EKOS is a **compiler for enterprise knowledge**, not a database or document store. It observes
enterprise systems without interpreting them, compiles those observations through deterministic
passes into a Canonical Knowledge Model, and stores the result in an append-only ledger where
every conclusion carries the evidence it was derived from.

```
Enterprise Systems → Observation Layer → Knowledge Compiler → Canonical Knowledge Model (CKM)
                                                                          ↓
                              AI/Apps ← Knowledge Runtime (read-only) ← Semantic Knowledge Ledger
```

This maps directly onto the `ekos init/build/recover/resolve/compile/commit` CLI pipeline: each
verb is a compiler stage, run in that order, writing artifacts the next stage consumes.

### Crate map (`ekos/crates/`)

| Crate | Role |
|---|---|
| `compiler-core` | `Compiler`, `PassManager`, `Scheduler`, `Diagnostics`, `EkosConfig` — the pipeline that drives every pass over a `PassContext` |
| `compiler-sdk` | Public traits for extending the compiler |
| `observation-sdk` | `Observer` trait — the contract every connector implements, returning an `ObservationPackage` of content-addressable `ObservationArtifact`s |
| `artifact` | Artifact types + `ArtifactStore` (loose JSON, or packed segments post RFC 0015) |
| `kir` | Knowledge Intermediate Representation — the typed output of knowledge-recovery passes, input to the semantic compiler |
| `recovery` | Knowledge-recovery passes: one analyzer per source kind (`sql_analyzer` — DDL; RFC 0176: when the whole file fails to parse, each statement is tried alone, a failing one is blanked out (line breaks kept, so line numbers stay the file's own) and reported as `SQL005`, and `SQL001` names the dialect when a file states tables and yields none; `ALTER TABLE … ADD … FOREIGN KEY` becomes an edge, `sql_transform_analyzer` — SELECT/VIEW/procedures into the Transformation IR, `plpgsql_analyzer` — RFC 0163, every PL/pgSQL routine in a `postgres`-dialect file into `Custom("Procedure")` (computed fidelity label) + per-statement `Custom("ProcedureStatement")` objects with `Contains` edges carrying the branch, each recording the `reads`/`writes`/`calls` its embedded SQL names (`plpgsql_footprint.rs`, a real `sqlparser` AST walk — needs sqlparser's `visitor` feature), `view_analyzer` — RFC 0169, every `CREATE [MATERIALIZED] VIEW` into a `Custom("View")` keyed by (file, name) with its query's reads/calls, parsed one definition at a time, `procedure_lineage`/`data_lineage` treat views as link targets, `trigger_analyzer` — RFC 0163 triggers, `Custom("Trigger")` keyed (file, table, name), classified at compile by `semantic::triggers` from the function's IR facts (Validation / DerivedColumn / Audit / Cascade / Mixed / Unknown, always with reasons); `identity` narrows `{View, Procedure, Trigger}` same-name groups (separate SQL namespaces); both kinds hang off their `File` (`Contains`) with a `source_span`, take `COMMENT ON FUNCTION|PROCEDURE|VIEW` as `description` (`sql_comments::extract_object_comments`, kept apart from the table extractor so a view comment never lands on a same-named table), and are in docs-gen's entity-page/symbol/doc-bearing lists, `doc_links` and `llm_description`, `pentaho_analyzer`, `git_analyzer`, `github_analyzer`, `confluence_analyzer`, `local_docs_analyzer`, `document_semantics_analyzer`, `crypto_analyzer`, `dependency_analyzer`, `python_analyzer` — RFC 0038/0040, real AST + `Calls`-adjacent structural recovery: SQLAlchemy ORM models promoted to real `Table` objects (`__tablename__` present, same-file-only `ForeignKey` resolution, RFC 0091) and real class inheritance as `RelationshipKind::Extends` edges (same-file-only base resolution, RFC 0092), `rust_analyzer` — real AST + `Calls` graph, RFC 0041, `crate_topology_analyzer`/`cicd_analyzer` — `Cargo.toml`/GitHub Actions structural parsing, RFC 0042, `dbt_analyzer` — static extraction of `Table` objects from a dbt project's own checked-in `.sql` model files (Jinja `ref()`/`source()` macro calls) plus `schema.yml`/`sources.yml`-shaped YAML, never `manifest.json`/live warehouse introspection, RFC 0117), `elixir_analyzer`/`javascript_analyzer` (real AST + module/symbol/`Calls` recovery), `architecture_evaluator.rs` (`evaluate_architecture`, RFC 0065 Phase 3 — deterministic completeness/evidence-coverage scoring, no LLM; consumed by both `ekos architecture investigate` and `docs generate`'s Executive Summary, RFC 0095), `architecture_reasoning.rs` (RFC 0065 Phase 2 — LLM crate-role classification), plus LLM provider glue (`anthropic.rs`, `ollama.rs`, `llm.rs`), `embed.rs` (RFC 0125 — `EmbeddingProvider` trait + Mock/Ollama/OpenAI/Cached impls + the opt-in post-`commit` `embed_objects` vector pass) and `llm_description.rs` (RFC 0088 — a post-`commit` step, not a `CompilerPass`, that persists real evidence-grounded `ai_overview`/`ai_usage`/`ai_comment_check` properties onto `Module`/`Rollup`/`Symbol` objects directly through `&dyn KnowledgeStore`; runs after `commit_rollups`/`commit_data_lineage`, opt-in via `[llm-description]`) |
| `identity` | Identity Resolution — `DefaultResolver` merges same-source-kind duplicates before the CKM exists (RFC 0007), but only when a group's normalized names match *exactly*; fuzzy matches (no confidence threshold reliably separates real correct fuzzy merges from real incorrect ones — RFC 0060) instead become reviewable `unconfirmed` `SameAs` relationships, same as `cross_system.rs`'s cross-system candidates (RFC 0029/0063), never a silent irreversible merge. `cross_system.rs` separately scores cross-system candidate matches (RFC 0029), written as reviewable `unconfirmed` relationships, never auto-merged. **New `ObjectKind::Custom(_)` variants an analyzer emits must be declared in `ekos_kir::custom_kinds::REGISTRY`** with `structurally_keyed` set — `true` for a kind self-identified by a structural key (file path, manifest dir, source+index): `DefaultResolver` derives its merge-exclusion set from that flag (RFC 0135 Part D), so a keyed kind is never a merge candidate. `Section`/`TransformNode`/`RustSymbol`/`RustModule`/`PythonSymbol`/`PythonModule`/`Crate`/`ElixirModule`/`ElixirSymbol`/`JsModule`/`JsSymbol`/`Document`/`Page`/`Risk`/`Rollup`/`ProjectSummary`/`Claim`/`ArchitectureGap` all hit (or would hit) the same over-merge failure — name-prefix or exact-name similarity + the same-kind structural-score fallback of 1.0. The `ekos-identity` test `every_pipeline_custom_kind_is_registered` now fails CI if a new `recover`/`compile` `Custom` kind skips the registry, so this is enforced, not remembered — but still set the flag correctly when you add the row. `is_expected_cross_namespace_group` (RFC 0147 amendment b) treats a same-name group spread over separate namespaces — the database (`Table`/`View`/`Procedure`/`Trigger`) and each language's code (Perl, JS, Python, Rust, Elixir module/symbol kinds) — as expected when each namespace's share is unremarkable alone. Separately, `is_expected_technology_jsmodule_pair` (RFC 0093) narrows — never widens — the cross-kind *conflict* detector (not the merge path above) for the one specific, mechanically-expected co-existence a `Technology` (declared dependency) and a bare-specifier `JsModule` (real import) sharing a name represents; a relative-specifier `JsModule` still conflicts. |
| `semantic` | Semantic compiler: KIR + resolved identities → CKM; `transform_ir.rs` is the shared Transformation IR (RFC 0027) every legacy-format parser (Pentaho, SQL) compiles into. `concentration_risks` (RFC 0094) derives real, persisted `Custom("Risk")` objects — `risk_type: "observed"` only, no LLM/inferred severity — for any object with 3+ real compiled `DependsOn` dependents, computed inside `SemanticCompilerPass::run()` (needs the whole-graph `DependsOn` view only available post-resolution, but — unlike RFC 0044's rollups — no `File`-object dependency to defer to `ekos commit` for). `procedure_lineage::link_procedures` (RFC 0163) runs at `ekos commit` after RFC 0075's `data_lineage`: routine → table `DependsOn` + statement `ReadsFrom`/`WritesTo` + routine → routine `Calls`, by **unique** name match only, one edge per id. `business_semantics::synthesize` (RFC 0170, opt-in `[semantics]`) runs at `ekos commit` right after it: the `predicates` recovery left on `View`/`ProcedureStatement`/`LANGUAGE sql` `Procedure` objects (schema: `ekos_kir::predicates::PredicateSite`, produced by `recovery/src/sql_predicates.rs`) plus each `Table`'s `check_constraints`/`seed_rows`/column comments → `BusinessConcept` (the **built-in** `ObjectKind::BusinessConcept`, never `Custom("BusinessConcept")`, which serde would silently turn into the built-in variant on read) and `Custom` `EnumMeaning`/`ConstraintCandidate`/`SemanticGap`/`RationaleLink`, all `status: hypothesis`; git blame comes in through the `RationaleSource` trait (CLI: `commands/semantics.rs::GitBlame`). `.ekos/semantics/current.json` lists the ids the latest run derived — the ledger is append-only, so read commands filter by it. Phase 3 carriers also include Pentaho `FilterRows` and standalone-`SELECT` `TransformNode`s and PL/pgSQL conditions (`$new`/`$old` → the trigger's table); dbt column tests feed domains/FKs/constraints; opt-in cite-or-drop LLM text is `recovery/src/semantics_llm.rs`; dbt model SQL (Jinja-rendered) makes a model a view-like carrier, Perl `use constant` groups and marked glossaries (`recovery/src/glossary.rs`) are meaning sources, `semantic/src/ontology.rs` makes vocabulary suggestions. Every predicate-recording pass folds `sql_predicates::PREDICATES_VERSION` into its cache key (bump it when the extractor's output changes; a test checks literal pass versions). Synthesis must only read inputs from source kinds, never from its own items (the glossary reader once re-imported its own output). Phase 2 review lifecycle is `semantics_review.rs` (`carry_forward` at commit, `apply_review` **human-only** — `commands/mcp.rs` must never name it, a test enforces it; every human-only entry point is listed in `commands/review_guard.rs::HUMAN_ONLY`, which parses `mcp.rs` and resolves imports/aliases/globs, so add a new one there); synthesis must not depend on ledger iteration order (sites are sorted — a reversed-input test enforces it). `review.rs` (RFC 0213): review subjects → items with signed `facts` and shown-only `info`, `ReviewDecision` / `SignOff` kinds; a sign-off's validity is recomputed from the digest on every read, never written by a compile; `decision_object`/`signoff_object` and `commands/review.rs::{decide,approve,revoke}` are **human-only** (`review_guard::HUMAN_ONLY`). `workflow.rs` (RFC 0213 Phase 2): the seven-step bar (`assemble` blocks steps 4–7 until step 3 is done) and `gate`; CLI `commands/workflow.rs` (`ekos workflow status`, `require_signoff[_in]` for an extension's step command), steps 2/4–7 from `EkosExtension::workflow_steps`; `commands/review.rs::subject_statuses` caches the summary in `<state>/review/summary.json` keyed by ledger counts + inputs — `approve` never reads it. `conflicts.rs` (RFC 0172, `[conflicts]`, on by default): `duplicate_definitions` (same-id objects collapse to the last one read and each disagreeing attribute becomes a `Custom("ConflictingEvidence")` item) and `merge_losses` (before `apply_merges`) run inside `SemanticCompilerPass`; `label_mismatches` runs at commit over the current `EnumMeaning`s (`commands/conflicts.rs::commit_step`, which also writes `.ekos/conflicts/current.json` with counts). Only stated facts are compared, SQL types normalized; `resolve` is **human-only** (`commands/mcp.rs` must not name it — a test scans) |
| `ledger` | The store API is two traits (issue #13): `KnowledgeReader` (every read) and `KnowledgeStore: KnowledgeReader` (the `append_*` writes, `set_write_context`, `vacuum_into`); `Runtime` and `runtime::graph_export` take `&dyn KnowledgeReader`, so they cannot write, and a `&dyn KnowledgeStore` coerces to it. `scope.rs` (RFC 0181): `ReadScope` (allow / deny rules by source, path pattern, object id; deny wins; unknown source or path counts as denied) and `ScopedStore`, a `KnowledgeStore` wrapper that filters **every** read — this is where access inside a project is enforced, because `Runtime`, EKL, graph export and commands all take a `&dyn KnowledgeStore`. A new method on either trait needs a decision there (`every_store_method_has_a_decision_here` reads both traits' source, and each wrapper body must consult the scope — `is_restricted`/`admit`/`visible`/`refuse`/`keep_` — or be listed in its `PASSTHROUGH` with a reason). `commands/store.rs` wraps every store the CLI opens when `--read-scope` is given (`EkosConfig::read_scope`, runtime-only), and only `mcp serve`, `ekl` and `query` accept the flag — `ask`, `docs generate` and the rest refuse it (`app.rs::with_read_scope`); `commands/mcp.rs` takes a per-call scope from `params._meta["ekos/readScope"]`, keys its result cache by scope, and under a restricting scope serves only `SCOPED_TOOLS` — a new MCP tool must be added to `SCOPED_TOOLS` (reads only through the store) or `SCOPE_REFUSED_TOOLS` (with the reason), a test enforces it, and an unlisted tool is refused. `crates/cli/tests/read_scope.rs` is the leak suite: it must keep failing when a filter in `ScopedStore` is disabled. Append-only Semantic Knowledge Ledger — `fact.rs`/`fact_ledger.rs` (facts), `index.rs`, `search.rs` (RFC 0177: the partial-match band is ordered by BM25 rank fused with term-coverage rank; `CONTENT_VERSION` must be bumped whenever `KirObject::indexed_content` changes — a writable open under another version wipes and replays the index, a read-only open only warns), `vector.rs` (RFC 0125 brute-force cosine vector index, opt-in), `retrieval.rs` (RFC 0119 the `retrieve` seam + `rrf_fuse`), `partitioned/` (RFC 0111 single-machine partitioning). RFC 0016's fact-segment engine (tantivy + mmap) is the default for **new** workspaces as of 2026-08-21 (`crates/cli/src/commands/store.rs::open_store`, real month-long soak evidence in the RFC's own dated section); any **pre-existing** SQLite-backed workspace keeps serving from SQLite unaffected unless explicitly migrated via `ekos ledger migrate --v3` |
| `segment-backend`, `cluster`, `distributed` | RFC 0111 Phase A (partitioned storage) + RFC 0113 Phase B (distributed, v1 feature-complete 2026-08-30). `segment-backend` is the storage-backend seam (`SegmentBackend` trait: local FS or object store); `cluster` is the coordinator + `compile-worker` lease/heartbeat protocol; `distributed` is the query-worker RPC layer + `DistributedLedger` gateway (per-shard IDF merge → RRF). Object-store read path verified live against MinIO + a 95-partition Elixir workspace (`devlog_144`). |
| `runtime` | Read-only state reconstruction and context projection. `ai.rs` is the `ekos ask` grounding+citation pipeline; `retrieval.rs` is RFC 0121 query understanding (`understand` → `QueryType`); `reason.rs` is RFC 0123's Query Plan IR + rules planner + typed `EvidenceSet`; `retrieval_eval.rs` is RFC 0126's CI-gated retrieval quality harness. RFC 0177: a search hit's claim shows the window of its text that matched the query (`reason.rs::matched_window`), a symbol's source text falls back to its own evidence fragment (a commit records a `ka:` knowledge-artifact id, which holds no file text), a requested dependents traversal from `M::self` includes the dependents of every `M::…` import-path object, and `Runtime::excluding_from_search` exists for `ekos eval run` only. |
| `ekl` | Enterprise Knowledge Language — `parser.rs` + `interpreter.rs` for the `ekos ekl` query command. A field that is not a built-in column is read from `properties` (only the fields a query names are copied into its rows; `a.b` is a nested path), and a `WHERE`/`ORDER BY`/`GROUP BY` field no candidate has is `EklError::UnknownField` (RFC 0175). Supports `AS OF <ts>` / `COUNT` / `GROUP BY` (RFC 0096) and `SEMANTIC 'text' [LIMIT k]` retrieval candidate sets (RFC 0124). |
| `docs-gen` | Deterministic Markdown/HTML rendering from the compiled ledger (RFC 0035) — `render_object_page` (`--layout objects`, one page per significant object) and `render_readme`/`render_architecture`/`render_api`/`render_sequence_diagrams` (`--layout curated`, RFC 0037/0042) — zero LLM calls; `render_dependency_risk_report`/`render_onboarding_guide`/`render_findings_memo` (`--layout solution-architect`, RFC 0090) for a team-handoff bundle; `--prose` (opt-in) is the one path that layers an LLM overview on top, via `ekos ask`'s own grounding+citation pipeline. `render_architecture_summary`'s Executive Summary renders real `Custom("Risk")` objects (RFC 0094) and an `ArchitectureConfidence` score (RFC 0095, a small local struct mirroring `ekos_recovery::EvaluationReport` rather than a real cross-crate dependency — `cli` does the translation) when the caller has real signal, an honest placeholder otherwise — never a fabricated 100%. |
| `dbt-gen` | Renders the Transformation IR (RFC 0027) into executable dbt SQL models with `ref()` semantics |
| `marketing` | Devlog → tweet → human approval → X publish (RFC 0030) — auxiliary tooling outside the compiler pipeline, not a `CompilerPass`/`Observer` |
| `simulation` | World Engine (RFC 0047-0055) — auxiliary, opt-in, deliberately separate from the compiler pipeline above: `action.rs`/`decision.rs`/`simulation.rs` (a closed 12-action vocabulary, a provider-independent `DecisionEngine` trait, a deterministic round-based loop with seeded priority/resource conflict resolution), `scenario.rs` (YAML scenario/agent definitions, `ekos simulate`'s loader), `forum.rs` (channels/replies/likes/follows/shares), `replay.rs` (a durable per-ledger event log + point-in-time reconstruction, `ekos replay`), `ingest.rs` (`world.sources`: real documents via the actual `localdocs` connector + `LocalDocAnalyzerPass`, no LLM). Writes through `&dyn KnowledgeStore` directly, the same access level `commit.rs` has — `Runtime` stays read-only throughout, unmodified by this crate. Every RFC in this crate builds additively on existing KIR/ledger primitives (`Custom()` escape hatches, `properties` conventions) rather than inventing new storage |
| `session` | RFC 0151 agent session memory: `inbox.rs` (capped, redacted, append-only JSONL inbox — the only thing an agent can write, never a ledger handle), `map.rs`/`commit.rs` (deterministic inbox → `Session`/`SessionClaim` KIR + `ekos session commit`), `anchor.rs` (exact-match anchors + per-kind staleness fingerprints), `read.rs` (`recall`/`brief`, staleness verdicts), `lifecycle.rs` (**human-only** confirm/reject/supersede — `mcp.rs` must never call it, a test enforces it), `capture.rs` (redacted transcript slices + strict-JSON LLM extraction that is only ever a `T0` proposal), `eval.rs` (deterministic proxy eval). Every `Runtime` read path — not just `find_objects`/`retrieve` — hides `Session`/`SessionClaim`, and `runtime::graph_export` filters them separately because it reads the store directly; a guard test (`every_object_returning_read_path_has_an_audited_session_decision`) fails CI on a new unaudited `pub fn`. An edge counts as session memory only when an *endpoint* does — `AnchoredTo`/`ObservedIn` are a cheap pre-filter, never the decision |
| `migrate`, `migrate-validate`, `migrate-dq`, `migrate-target-clickhouse`, `migrate-approval`, `migrate-report`, `migrate-logic`, `pg-live`, `plpgsql` | EKOS Migrate (RFC 0154-0167), `ekos migrate *`, opt-in via `[migrate] enabled = true`, PostgreSQL → ClickHouse. `migrate` = project/unit model + append-only state machine; `pg-live` = the **third raw-content entry point** (live PG catalog + profiling, runs its own redaction, sync `postgres` driver on a dedicated thread — never inside the tokio runtime); `migrate-validate` = RFC 0155 canonical serialization + RFC 0156 tiers V1-V3 with planted controls; `migrate-dq` = rules measuring affected rows, plus `doc_claims` (RFC 0172 Phase 3: checkable claims in column comments, and in Markdown/Confluence sentences naming one `table.column` — recorded at recover as `column_mentions` on `Section`/`Page` by `recovery/src/column_mentions.rs` — → `DQ.CONSIST.DOC` findings and `doc_vs_data` conflicts at `migrate assess`, kept in `.ekos/conflicts/migrate.json` so a commit never drops them); `migrate-target-clickhouse` = type map, design, statement classifier and the guarded executor; `migrate-approval` = computed risk + evidence-pinned, **human-only** approvals (`commands/mcp.rs` must never reach it — a source-scan test enforces it); `migrate-report` = compiled, citation-verified report; `plpgsql` = PL/pgSQL parser + procedural IR (RFC 0163), consumed by `recovery`'s `plpgsql_analyzer`; not yet lowered into the Transformation IR (RFC 0164). `migrate-logic` = RFC 0164 Phase 1, pure (no LLM, no connection): `lower.rs` is a **whitelist walker** — every `match` ends in an arm that blocks by name, so add a rule there rather than passing a construct through — that turns a PostgreSQL `CREATE VIEW` into a ClickHouse one and must read back identically through the ClickHouse grammar; `invention.rs` reduces statements to atoms (tables, columns, literals, functions, comparisons) and rejects a candidate with an atom its source lacks; `gate.rs` is the fidelity gate; `triggers.rs` the per-class proposals. CLI: `commands/migrate_logic.rs` (`ekos migrate lower` / `check-candidate`), which re-reads definitions from source files by recorded span and therefore applies RFC 0043 exclusion + redaction itself. The engine differences the rewrites assume are measured in `crates/migrate-logic/tests/clickhouse_semantics.sql` (live suite). `v5.rs` is differential execution of a lowered view behind a `Sandbox` trait (fixture from the view's own comparisons, compared columns crossed, branch coverage measured on the source engine, `not_run` is never a pass); the engines are `pg-live/src/sandbox.rs::PgSandbox` — the **only writable PostgreSQL session**, a separate type from the read-only `PgSource`, one rolled-back transaction per run — and a ClickHouse database that is always dropped; `ekos migrate v5` refuses a connection without `sandbox = true` or one that is a project's source. `reconstruct.rs` builds the prompt and judges a model's answer (gate before the call, anti-invention after); the call is in the CLI and needs `--allow-metered` unless the provider is Ollama. Proposals are `Custom("MigrationProposal")`, keyed by source object, always written `status: proposed` — nothing writes another status, and `commands/mcp.rs` does not reach any of this. Live suites: `tests/v5_live.rs` (planted defects) and `tests/clickhouse_semantics.rs`. Not built: V5 for routines, approval of proposals. Migrate kinds are built from constants, which `every_pipeline_custom_kind_is_registered` cannot see — `every_migrate_kind_has_a_registry_row` covers them. Live suites: `docker-compose.migrate.yml` + `EKOS_MIGRATE_LIVE=1` |
| `cli` | `commands/` — one file per CLI subcommand, dispatched from `crates/cli/src/app.rs` (`main_with`; `bin/ekos.rs` is a shim); `home.rs` + `commands/project.rs` (RFC 0178): a project in the EKOS home (`<home>/data/<tenant>/<project>/{ekos.toml,state/}`), selected with the global `--project`/`--tenant`; `main_with` then loads that config strictly, sets `EkosConfig::location` and changes into `[workspace] root`. `EkosConfig::ekos_dir` is the only function that decides the state directory — never build a state path from `cwd` (the session inbox and default output directories did; see `session::inbox_at`, `EkosConfig::output_base`), and a project run must leave the source tree byte-identical (`tests/project_home.rs`). Part 2: `[[sources]]` names a project's inputs; `EkosConfig::observe_bases` is the **only** function that turns `[[sources]]` or `[observe] paths` into directories and keys (a source's key is its name — the RFC 0044/0079 `project` field), `semantic/src/sources.rs` attributes each object (`source`/`sources`, stamped on `File` objects at build and on compiled objects in `commit.rs::write_objects`; an object with neither must be treated as not visible by a source-restricted reader), `commands/source.rs` edits `ekos.toml` through `toml_edit`; also hosts the MCP server (`commands/mcp.rs`) and `extension.rs`, the RFC 0149 extension seam (`EkosExtension`: extra observers, recover passes, a post-commit step, MCP tools) that out-of-tree builds plug into. RFC 0191 widened it: `commands()` + `command_classes()` + `run_command()` add whole command groups (`main_with` parses the built-in tree plus `Extensions::extend_cli`, which refuses a taken name or a leaf without a catalogue class; an extension command becomes the skipped `Commands::Extension` variant and goes through the normal project/config setup; `main_with_args` exists for tests and installs the global logger, so call it once per test binary), and `mcp_tool_is_scoped()` lets an extension's own tool reach a restricted reader (built-ins are decided by `SCOPED_TOOLS`/`SCOPE_REFUSED_TOOLS` whatever an extension says). EKOS Lakehouse (RFC 0190) is the first such extension, in its own public repo `alexeyban/ekos-lakehouse`; its kinds are the `Lakehouse*` rows in `custom_kinds::REGISTRY`. RFC 0198: `console_pages()` declares console tabs as data (`ConsolePage`/`Panel`/`View`/`Action` in `extension.rs`); `commands/extension_pages.rs` checks them for `ekos extensions --json` (a panel runs only a **read** catalogue command with its own flags, an action needs no argument and is not local), and the console checks again (`web/api/app/extensions.py`, `routes/extensions.py`, `pages/ExtensionPage.tsx` — `readproc.read_checked_json` is only for argv `extensions.panel_argv` accepted). RFC 0207 (pages v3): `ConsolePage.params` are selections filled from a source panel's table rows; `Panel.params` append them as `--name=value` to a **value** option of the panel's read command (checked on both sides, ≤512 chars, no NUL); `Action.binds`/`flags`/`confirm`; views `code`/`graph`/`markdown` (the UI's own renderer, `pages/markdown.tsx` — never HTML strings); the page types derive `Default`, so construct with `..Default::default()`. `POST /cli` refuses `--allow-metered` unless the tenant allows metered calls. RFC 0200: `EkosExtension::java_decompiler()` / `Extensions::java_decompiler()` (the private ekos-binary build offers a `JavaDecompiler`; the public build none), `CommandContext`/`RecoverContext` carry `extensions`; `Task::Translate` (`[llm.tasks.translate]`); the Pentaho translator itself lives in ekos-lakehouse (crates `kettle`, `kettle-expr`, `kettle-rel`, `pentaho-dbt`), its kinds are the `Pentaho*` registry rows. RFC 0199: `setup::Machine::program` is the running binary's name, so an extension binary installs and starts the console as itself; `scripts/package-full.sh` takes `EKOS_PACKAGE_PROGRAM`/`EKOS_PACKAGE_INSTALL_MD`; `migrate::ConnectionRef::secret_file` (pg-live refuses a file others can read) for jobs that do not inherit a shell's variables. `EkosExtension::binary()` gives an extension binary its own name and version (`--version` prints `<name> <version> (EKOS <x>)`). The console registers home projects at start-up and on every project list (`web/api/app/home_projects.py`), so one created while it runs appears without a restart. RFC 0170: `commands/semantics.rs` (`ekos semantics list|show|gaps|eval|confirm|reject|edit` + the commit step) and `commands/export.rs` (`ekos export linkml`, default `--status confirmed`), `commands/import.rs` (`ekos import linkml` — YAML edits → review decisions by `ekos_id`, human-only like `semantics confirm`); the web console's Semantics tab (`web/api/app/routes/semantics.py` + `semantics_write.py`, `web/ui/src/pages/Semantics*.tsx`/`Linkml.tsx`) runs those CLI commands under the write role, never MCP; its Conflicts tab (RFC 0174, `routes/conflicts.py` + `conflicts_write.py`, `pages/Conflicts.tsx`) does the same for `ekos conflicts resolve`. RFC 0179: a registered workspace whose path is `<EKOS_HOME>/data/<tenant>/<project>` is a home project, and `web/api/app/home.py` is the **only** place that builds an `ekos` command line for a workspace (`command`, `mcp_command`) or names its state directory — a new subprocess call in the console must go through it, or it runs against the wrong directory for a project. RFC 0180 (console access control): `web/api/app/auth.py` only turns a request into a `Principal` (OIDC user, API token, or development static token); `web/api/app/authz.py::authorize` is the **only** place a role is compared, and every route reaches it through `authz.authenticated` / `allow(action)` / `allow_tenant(action)` / `allow_platform` — `tests/test_authz_routes.py` walks `main.ROUTERS` and fails on a route that declares none, or on a new route that decides its action in the handler without being added to the test's `DYNAMIC` list. A token principal is denied `review`, `admin` and `platform` inside `authorize` whatever its bindings say (review stays human-only at this layer too). A workspace or tenant the caller has no role in is 404, never 403. RFC 0181 in the console: a role binding may carry a rule; `authz.scope_for` turns a principal's bindings into the scope, `deps.mcp_for_workspace` returns a client that attaches it to every MCP call (the only way a route gets one), and `allow(action, scoped=True)` marks the few routes safe for someone who sees part of a workspace — every other workspace route answers them 403, and `test_only_the_listed_routes_are_open_to_a_restricted_reader` pins the list. RFC 0182: `routes/mcp_gateway.py` is the MCP endpoint for agents (`POST /mcp/<tenant>/<project>`, mounted outside `/api` — `main.MOUNTS`); it builds each forwarded call from the tool name and arguments only (a client's `_meta` never reaches the server), decides the needed right from `mcp_policy.action_for`, and audits every call. `mcp_policy.SCOPED_TOOLS` must equal `SCOPED_TOOLS` in `commands/mcp.rs` (a test reads the Rust source). The supervisor starts a project's server on first use (`acquire`), stops idle ones and caps how many run. RFC 0183: `commands/home_backup.rs` is `ekos home backup|restore` (a project is copied under `fact_ledger::hold_write_lock`; restore unpacks to a staging directory, accepts plain files and directories at checked paths only, and moves anything it replaces to `trash/`, never deleting). In the console every `ekos` subprocess gets its tenant's environment from `envs.for_path` / `envs.for_tenant` (a base list plus the variable names an owner gave that tenant, while `allow_metered` is on) — `_proc.spawn` / `run_streaming` must always be passed `env=`, and `test_every_process_the_console_starts_is_given_an_environment` scans for a call without it. RFC 0184: console settings also come from `<home>/control/console.toml` (precedence: environment, keychain for secret fields, file, default — `Settings.settings_customise_sources`); every `Settings` field needs a row in `console_file.FIELDS` (a test compares the sets); `routes/platform.py` (owner only) edits the file with `tomlkit` and stores secrets through `app/keychain.py` (`keyring`; entries `env:<NAME>` / `console:<field>` under service `ekos`), which never returns a value; tests get an in-memory keychain from an autouse fixture and must never touch the real one. `envs.for_tenant` fills a granted variable from the keychain. RFC 0185: `commands/console.rs` (launcher; stop/restart are files `control/stop-requested`/`restart-requested` that `app/lifecycle.py` watches when supervised — no cross-process signals), `commands/setup.rs` (answers → pure `plan` → `apply` through the `Effects` trait, so tests never start services; `control/install.json` is what `uninstall` undoes), `setup_server.rs` + `setup.html` (the wizard, token on every request). RFC 0188: `uninstall_server.rs` + `uninstall.html` (`ekos uninstall --ui`; the steps shown come from the pure `setup::uninstall_plan`, and a test holds them equal to what `uninstall` does — change both together); the console's `POST /api/platform/uninstall` (local mode, `install.json` present) runs `uninstall --ui --detach`, which under the systemd login service goes through `systemd-run --user` because stopping the unit kills every process in its cgroup. RFC 0189: a chained command's stage argv is `Command.stage_args`, its stages per workspace `Command.stages_for` (home projects skip `init`), each stage limited by the setting `run_stage_timeout_seconds` (single commands keep `Command.timeout`). The console's CI runs on Python 3.12 (Linux), macOS and Windows: tests must not assume `/bin/...`, POSIX permissions, `#!` fake binaries or `/` in paths — skip on Windows with the reason when they must. Never build a path for display or comparison with Rust's `canonicalize` (Windows `\\?\`): use `home::canonical_dir`. RFC 0186: **a new CLI subcommand needs a row in `commands/catalogue.rs::CLASSES`** (read / write / review / admin / external / local — who may run it from the web; a test fails without one); the console runs catalogue commands only through `routes/cli.py` + `app/catalogue.py::build_argv` (never a shell, never an unlisted argument, reviewer forced to the signed-in person); config sections derive `schemars::JsonSchema` for `ekos config schema --json`. RFC 0187: `app/chat/` (providers over `httpx`, `agent.turn`) + `routes/chat.py`; the model is offered only tools with `mcp_policy.action_for(tool) == "read"` that `mcp_gateway._Caller.may` allows, through `mcp_for_workspace` (the scoped client); **the chat code must never name a review tool** (a test scans it); a metered provider (`providers.is_metered`) needs the tenant's `allow_metered`; tests use a scripted provider, the package check a local Ollama model — never a paid call. Local mode (`mode = "local"`): `LoopbackOnly` middleware, one-time codes in `control/local-signin/` (`local_signin.py`), principal kind `local` (an owner). `docs/deployment.md` is the operator's guide; keep it in step with these RFCs. Schema changes to the console database go in `models.MIGRATIONS` (append a function; `create_all` never alters a table). The production image is the root `Dockerfile` (no BuildKit-only syntax: it must build with the classic builder) + `deploy/docker-compose.yml`; `Dockerfile.dev` and `web/docker-compose.yml` are development files. `freshness.rs` (RFC 0171): `ekos build` records a per-file (path, size, mtime) manifest in the same walk as its fingerprint (`observation_sdk::source_manifest` / `fingerprint_of` — the hash must stay byte-identical, a test pins it), a successful `ekos commit` promotes it to `.ekos/committed-manifest.json`, and `check` diffs it against the disk for `ekos status`/`doctor`/`freshness`, `FRESH001` at compile/commit and MCP (`ekos_status` block + a note on read results, memoised in `StoreCache`). Metadata only — never a raw-content entry point (the opt-in `[freshness] content-hash = true` stores a SHA-256 per file in the manifest so a `touch` is not a change: `add_hashes` at build, `diff_with` hashes only same-size/moved-time files at check, RFC 0043-excluded paths are never read; nothing from it reaches the artifact store or ledger); EKOS's own state dir is always excluded |
| `common` | Shared utilities — `ContentHash`, zstd compression, and `redaction` (RFC 0043: built-in, non-disable-able secrets/PII pattern table + excluded-file globs, the single module both `build.rs` and `recover.rs`'s direct-file-read blocks call before anything reaches the artifact store or ledger; a reader that re-reads a workspace file after `build` for a model or the ledger goes through `redaction::read_redacted` — exclusion refusal, then the whole file redacted — and `git blame` (`commands/semantics.rs::GitBlame`) redacts each commit summary, RFC 0043 amendment) |
| `scheduler`, `sql-dialect-sdk` | Pass scheduling primitives; the `SqlDialectParser` trait every `plugins/sql-dialect-*` crate implements (RFC 0031) |

### Connectors (`ekos/plugins/`)

Each plugin implements `Observer` from `observation-sdk` and is registered independently in
`ekos/Cargo.toml`'s workspace members. Real/tested: `file`, `git`, `github`, `confluence`,
`localdocs` (PDF/DOCX/text/Markdown/HTML/email), `pentaho` (`.ktr`/`.kjb`, RFC 0027), `crypto`,
`python` (real AST + PySpark DataFrame chains into the Transformation IR, RFC 0038/0040), `rust`
(real AST + the first real `Calls` function-call graph, RFC 0041), `java` (RFC 0209: hand-written
declaration parser in `recovery/src/java_syntax.rs`, `JavaType`/`JavaMethod`; `app_model.rs` turns
iDempiere-family generated `I_`/`X_` classes into `ModelTable`s — a code claim, never merged into a
database `Table`; `java_sql.rs` folds SQL built in method bodies; generated files get no `JavaMethod`
objects; the kinds are built from constants, so `every_java_kind_has_a_keyed_registry_row` guards them),
`elixir`/`javascript` (real
AST + `Calls` recovery), `clickhouse` (real HTTP client, schema metadata + the opt-in live
NL-to-SQL query engine, RFC 0056). `sql-dialect-{postgres,mysql,mssql,snowflake,databricks,clickhouse,trino,oracle}`
implement the `SqlDialectParser` trait (RFC 0031; `trino` is RFC 0190 — `GenericDialect` plus a
`preprocess` that blanks time travel, `ALTER TABLE … EXECUTE` and `TABLESAMPLE`; the dbt analyzer
falls back to it; `oracle` is RFC 0210 — `GenericDialect` plus a line-preserving `preprocess` that
blanks SQL*Plus commands and whole PL/SQL units, and rewrites Oracle-only clauses, listed in the
crate docs). Oracle scripts are split by `crates/plsql`'s `script` module (Oracle quoting: `''`
escapes, backslash is plain, `q'[…]'`; units end at a `/` line; block comments do **not** nest —
PostgreSQL's do, so never mask Oracle text with `ekos-plpgsql`'s scanner). `crates/plsql` also
reads PL/SQL routines, triggers and packages into RFC 0163's `ProcedureIr`, handing each
`BEGIN … END` to `ekos_plpgsql::parse_body` with `Origin { flavor: Flavor::PlSql }` (PL/SQL-only
statement forms are recognised only under that flavor, so PL/pgSQL parsing never changes);
`recovery/src/plsql_analyzer.rs` writes them through `plpgsql_analyzer::recover_parsed_routines`, so
both languages produce identical `Procedure`/`ProcedureStatement` objects, and an Oracle trigger's
inline body is a `Procedure` named after it plus a `Trigger` whose `function` names it.
`recovery/src/schema_history.rs` (RFC 0210) turns every `<yyyyMMddHHmm[ss]>_<description>.sql`
migration script into a keyed `SchemaChangeSet` + one `SchemaChange` per DDL operation, classified
from tokens (sqlparser 0.53 reads Oracle `MODIFY c NOT NULL` as a column of type `NOT`), including
iDempiere's PostgreSQL `INSERT INTO t_alter_column` rows; `semantic/src/schema_history.rs` links
changes to tables at commit (`Changes`, unique name only). RFC 0211: `crates/ora-live` is the
**fourth raw-content entry point** (live Oracle catalog; driver `oracle` behind the crate feature
`oracle`, off by default; Instant Client on `LD_LIBRARY_PATH` at run time) — reads `DBA_*` (through
`ALL_*` a read-only user sees 0 of 48 functions and the counts still reconcile), inside `SET
TRANSACTION READ ONLY`, reconciled against `*_OBJECTS`. `[[databases]]` in `ekos.toml` (the password's
variable **name**, never its value) → `plugins/oracle` observer at build → `OracleCatalogPass` at
recover → objects with `origin: live`, which identity never merges and name lookups
(`procedure_lineage::NameIndex::add_object`, `data_lineage`) use only for names no file defines.
`semantic/src/live_conflicts.rs` compares `ModelTable`s / `db/oracle` routines with them at commit
(RFC 0172 conflicts with an `explanation`: `pending_migration` by time **and** release horizon,
`translation_table`; `mandatory_but_nullable` carries a `measured` null count from the profile).
Profiles: `ora-live::profile` (`[[databases]] profile = p0|p1|p2`) — a text column is never a
`MIN`/`MAX` argument, PII rules are the shared `ekos_common::pii`. **Live sources run outside the
build fingerprint** (`build.rs::read_databases`, once per build, failure fatal); the catalog pass's
output sets `KnowledgeContent::supersedes_earlier_runs`, so `compile` keeps only its newest read
(multi-input passes otherwise accumulate every past output and merge them), and each catalog
artifact lists the objects it read so a dropped table leaves. A pass that calls another analyzer's
functions must fold that analyzer's `LOGIC_VERSION` into its cache key (two stale-cache bugs).
RFC 0212: `semantic/src/write_behaviour.rs` derives one keyed `Custom("WriteBehaviour")` per table at
commit (update class, delete classes, change log, `citations` — not `evidence`, which EKL reserves) from
footprint `update_sets` / `delete_shapes`, `recovery/src/java_po.rs` `PO` delete receivers (method-local
declarations only; test code counted apart), live cascading FKs and the opt-in `[[databases]] dictionary`
(`ora-live/src/dictionary.rs`, `AD_Table` flags); statements in migration scripts are ignored.
RFC 0219: `ekos_common::tickets::find` (exact `[tickets] keys` only — never a generic `ABC-123`); the git
analyzer emits keyed `Custom("Ticket")` objects + `Touches` and sets `supersedes_earlier_runs` (any pass that
reads "all of X" should); `semantic/src/tickets.rs` adds `TicketFor` at commit. `[observe] git-max-commits`
(default 500) joins the build fingerprint key only when changed (`build.rs::observer_settings_key`) — an
observer setting that changes output must invalidate the cache. `File` objects live in the ledger, not the CKM.
RFC 0220 run logs: `ekos_common::run_log::emit(stage, msg, &data)` is a run-log line (target `ekos_run_log`);
`crates/cli/src/run_log.rs` is the layer — the pipeline commands write JSON lines to `EKOS_RUN_LOG` or
`<state>/runs/<stamp>-<cmd>.jsonl` (warnings copied redacted), ending with `llm` (model calls, counted from
`ekos_model_call` events that `MeteredLlmProvider`, the LLM cache and the embedding providers emit — **a new
model-call path must emit one**) and `run finished`. The terminal filter turns both targets off. The console sets
`EKOS_RUN_LOG=<runs>/<id>.jsonl` (`runner.with_run_log`) and serves `/api/runs/<id>/events`. RFC 0221: localdocs lifts Markdown front matter into a `front_matter` field (Document properties; `title`/`source_url`/
`source_revision`/`licence` also on sections) — a change to an observer's output needs `PIPELINE_LOGIC_VERSION` bumped;
sections record `tickets` (`with_ticket_keys`) and `semantic::tickets` links `Section`s; `scripts/wiki-snapshot.py`
(MediaWiki export/API → Markdown with provenance; tests in `scripts/tests`). Testbed: `docker-compose.oracle.yml` +
`deploy/fixtures/idempiere-oracle.sh`; live suite `EKOS_ORACLE_LIVE=1`. `SqlAnalyzerPass` stamps `dialect` on
every `Table`, and `conflicts` compares claims only within one dialect (`generic` = unknown, compared
with all) — engine twins of one script are two databases, not a dispute. `init --detect` writes one
`[[recover.sql.dialect-rules]]` per engine-named directory (`oracle/`, `postgresql/`, …).
**Not in this repo:** RFC 0148's compiled .NET/JVM binary decompiler (readers, observer, analyzer pass,
`[binary-reconstruction]`, `ekos_binary_explain`) lives in the private `alexeyban/ekos-binary` workspace as
an RFC 0149 extension with its own `ekos` binary. Public EKOS keeps only the `[binary-reconstruction]`
config schema, the `Binary*` custom-kind registry rows and identity's binary narrowing, so a ledger a
private build wrote still resolves and queries here. Public crates must never depend on it.

Scaffolded proof-of-concept only (mock API shapes, not exercised against live accounts):
`salesforce`, `sap`, `fabric`, `snowflake` (`oracle` is live since RFC 0211).

`crate_topology_analyzer`/`cicd_analyzer` (RFC 0042, in `recovery`, not `plugins/`) and the
dependency-scan block in `recover.rs` are a second, separate raw-content entry point — they walk
`observe_paths` and call `std::fs::read_to_string` directly rather than going through an
`Observer`/`ArtifactStore` round-trip. Both entry points (the `Observer`-based one and this direct
one) independently run RFC 0043's redaction pass before any content is persisted — see `common`'s
crate-map entry.

### Key invariants (enforced by review, not just convention)

- The ledger is **append-only** — knowledge is never modified in place.
- Every semantic conclusion must be traceable to **Evidence** (one of the four primitives:
  Object, Relationship, Event, Evidence).
- The Runtime is **read-only** — it reconstructs state, never modifies it.
- AI systems consume knowledge through the Runtime only (in practice, through `ekos mcp serve`,
  RFC 0013) — they never touch raw enterprise systems directly.
- Compiler passes must be **deterministic** and **side-effect-free**.
- Artifacts are **content-addressable** (id + checksum + metadata + dependencies + version).
- **Secrets/PII are never observed or stored** (RFC 0043) — a built-in redaction baseline
  (`ekos_common::redaction`) runs at every raw-content entry point; `ekos.toml`'s `[security]`
  section can only extend it, never disable it. Because the ledger is append-only, this has to be
  a prevention control, not a cleanup step — there is no way to un-commit something already
  ledgered (confirmed: no object-level delete/tombstone exists anywhere in the codebase).

### AI agent access (MCP)

`ekos mcp serve --workspace <dir>` exposes the Runtime read-only over stdio via newline-delimited
JSON-RPC 2.0 (also `--tcp <addr>` for multiple clients on one server, RFC 0115). Tools:
`ekos_search` (`mode` `lexical`/`vector`/`hybrid`, RFC 0124/0125), `ekos_query` / `ekos_retrieve`
(compiled fact+graph answer and the inspectable query plan / evidence set, no LLM — RFC 0124),
`ekos_ekl`, `ekos_neighborhood`, `ekos_state`, `ekos_dependents`, `ekos_impact` (multi-hop,
RFC 0018), `ekos_diff`, `ekos_status`, `ekos_transformation_explain`/`ekos_transformation_diff`
(Transformation IR, RFC 0028), `ekos_architecture_evaluate`/`ekos_architecture_drift`/`ekos_architecture_diff`
(RFC 0065/0107-0109), the gated `ekos_clickhouse_query` (live NL-to-SQL, off by default, RFC 0056),
the opt-in `ekos_session_note`/`ekos_session_recall`/`ekos_session_brief` (RFC 0151, `[session-memory]`; the note tool writes only the inbox file, never the ledger), the read-only `ekos_signoff_status` (RFC 0213, the workflow bar and sign-off states), the read-only `ekos_conflicts` (RFC 0172, where sources disagree; `ekos_state` also carries `open_conflicts`, and an `ekos_query`/`ekos_retrieve` claim about a disputed attribute carries `disputed: true` — RFC 0173, `EvidenceSet::mark_disputed` in `runtime/src/reason.rs` with the lookup supplied by `commands/conflicts.rs::flag_disputed`, since the Runtime cannot know which conflicts are current; `ekos ask` gets the same lookup through `AiRuntime::with_disputes`, and a label conflict carries a second `Disputes` link to its column's table whose `attribute` property — `columns.<col>.meaning` — is what the lookup uses for that table), the opt-in read-only `ekos_semantics_lookup`/`ekos_semantics_gaps` (RFC 0170, `[semantics]`; every result carries its review status in words — there is no MCP tool that confirms a definition), and the two write-capable review tools `ekos_identity_review` (confirm/reject a candidate identity
match — cross-system, RFC 0029, or same-source fuzzy, RFC 0063; the tool doesn't distinguish
origin, only `Custom("SameAs")`; an RFC 0032 treasury `AuthorizedBy` is **human-only**,
`ekos treasury confirm|reject` → `commands/treasury.rs::review_authorization`, issue #16) and
`ekos_architecture_review` (confirm/reject an LLM-classified crate role, RFC 0109). Both record
the decision as an event citing evidence with the optional `reviewer`/`note` arguments (issue
#16). The gated `ekos_clickhouse_query` also writes: one evidence + `ClickHouseQueryExecuted`
event per live query (RFC 0056 audit; id derived from database, SQL and result hash, issue #11).
Every other tool is read-only, going through `Runtime` only; `mcp.rs`'s
`only_listed_functions_can_write_the_ledger` pins the set of functions that can write. This repo's own `.claude/skills/ekos-knowledge` and `.claude/skills/memory` skills consume
this server. Runnable demos live under `demo/` (`ledgersmb-linkml/`, `session-memory/`,
`binary-demo/`); the old twelve-act walkthrough was removed in the 2026-10-10 cleanup (it is in git
history before that date).

### LLM-backed passes

`recovery`'s analyzer passes that call an LLM (document semantics, some SQL/Git recovery) go
through the `LlmProvider` trait (`recovery/src/llm.rs`), with `anthropic.rs` and `ollama.rs`
implementations selected via `[llm]` in `ekos.toml`. Treat this as a Claude-relevant provider
boundary — check `recovery/src/llm.rs` before assuming request/response shapes.

## Mandatory Development Workflow

Every task follows this sequence — do not skip steps:

1. **Design** — write an RFC in `docs/rfcs/NNNN-<topic>.md` before any code
2. **Architecture Review** — validate against the compiler model above
3. **Interfaces** — define public traits and types first
4. **Tests** — write tests before implementation
5. **Implementation**
6. **Refactoring**
7. **Documentation** — every public API must be documented
8. **Integration**
9. **Benchmark** — required for performance-relevant changes (see `benchmark/`)
10. **Merge**

No feature is implemented until its RFC is accepted. RFCs are authored **just-in-time**, before
the phase that needs them, not all up front (see `TODO.md` Phase -1 for the rationale).

## PR Checklist

Every PR must satisfy: tests passing, documentation, benchmarks (performance-relevant changes
only), no public API break, compiler diagnostics, logging, examples.

## Coding Rules

- Rust 2024 edition
- Zero `unsafe` unless formally justified in an RFC
- No global mutable state
- Dependency injection through traits (`Observer`, `LlmProvider`, `CompilerPass`, `ArtifactStore`, …)
- Every artifact must be serializable
- Pure functions wherever possible
- Reproducible builds

## Devlog Rule

**`devlogs/devlog_N.md` files are the project's long-term memory.** They are the primary source of
truth for project history, architecture decisions, production incidents, and non-obvious knowledge.
Treat them as the first thing to read, not the last thing to write.

**After any session with significant changes, generate a new `devlogs/devlog_N.md`.**

Significant = any of: new feature shipped, bug found and fixed, architecture decision made,
non-obvious knowledge captured, production incident, or a set of PRs merged in one session.
Minor chores (typo fixes, dependency bumps) alone do not warrant a devlog entry — fold them
into the next substantive one.

### Filename

Increment from the highest existing `devlogs/devlog_N.md`: `devlog_43.md` → `devlog_44.md`, etc.

### Required sections

```markdown
# Devlog N — <short title>

**Date:** YYYY-MM-DD
**PRs:** #N, #N+1, …
**Branch:** <branch> → <target> (merged / squash-merged)

---

## Summary
<2–5 sentence overview: what changed and why it mattered>

---

## PR #N — <title>

### Problem / motivation
### What was built  (table of components if >3 items)
### Implementation details worth remembering
### Decisions (alternatives considered, why this choice)

(repeat for each PR)

---

## Knowledge Captured
<Non-obvious facts, gotchas, SDK quirks, production behaviour, or patterns
 that should not be re-discovered from scratch. Each item should answer:
 "What would a future developer need to know to avoid the same mistake?">

---

## Files Changed
| File | Change summary |
```

### What belongs in "Knowledge Captured"

- SDK/library quirks that aren't in the docs
- Production-only behaviour
- Decisions with non-obvious rationale
- Cost/latency benchmarks discovered in practice
- Debugging techniques that were hard to find

### After writing the devlog

1. Also update `TODO.md` to tick off completed items for the day's work
2. Update `README.md` if any user-facing behaviour changed
3. **Update `docs/generated/ekos-self-documentation.html`** — the human-readable, comprehensive
   capabilities reference published at
   [alexeyban.github.io/EKOS/generated/ekos-self-documentation.html](https://alexeyban.github.io/EKOS/generated/ekos-self-documentation.html).
   If the devlog shipped a new capability, a new CLI command/flag, a new MCP tool, or changed how
   an existing one behaves, add or update that capability's own section there — this page is meant
   to stay a complete, current, human-readable description of the whole project and every aspect of
   it, not a point-in-time snapshot. A devlog that's purely an internal fix/refactor with no
   user-visible capability change doesn't need an update here (same bar as the README update in
   step 2). Pushing this file (it lives under `docs/`) redeploys the live page automatically via
   `.github/workflows/pages.yml`.
4. Commit everything in one PR: `chore: devlog_N, README + TODO + capabilities doc update for <topic>`
