# EKOS — Gaps, Trade-offs, and Not-in-Scope Items vs. RFCs

**As of:** 2026-08-27 (devlogs 1–127, RFCs 0001–0109). **Not re-synthesized since** — treat the
body below as a 2026-08-27 snapshot. Deltas known as of 2026-09-01 (devlogs 128–148, RFCs
0110–0126) are folded in as dated **UPDATE** notes at the top of each affected section; `TODO.md`
remains the always-current backlog.
**Author's method:** This is a synthesis, not a fresh re-derivation. EKOS already tracks this
continuously in `TODO.md`'s `## Ongoing / Cross-cutting` section — a 2026-08-21 full read-through
of every RFC's own Non-Goals section (58 of 95 RFCs carry one) promoted ~40 genuine deferred items
into tracked backlog, and every RFC/devlog since has kept that list current (most recently the
eight-item "gap-closure list" closed across devlogs 104–111, devlog_112's four bugs closed the same
session, and — since this document was first written — the entire "Runtime/Retrieval" backlog
closed via a six-RFC sequence, RFC 0068 §62's Architecture Diff/Human Review/MCP-exposure items, and
Storage Architecture Plan Phases 1–3, all across devlogs 113–127). This document reorganizes that
material by subsystem instead of chronologically, adds nothing invented, and calls out where the
tracking itself is stale.

---

## How to read this document

Three distinct categories, kept separate throughout:

- **Gap** — something an RFC or devlog describes as real functionality that doesn't exist yet, with
  no deliberate reason it shouldn't eventually exist. Tracked as `[ ]` in `TODO.md`.
- **Trade-off** — a deliberate design decision that *chose* a scope boundary, usually because the
  alternative was investigated and rejected (not merely unconsidered). Marked with the RFC that made
  the call and the reasoning given at the time.
- **Not-in-scope / Non-goal** — an RFC's own explicit boundary: work that RFC's author decided,
  in writing, does not belong in that RFC (and may or may not belong in a later one).

A fourth, smaller category closes the document: **stale tracking** — places where `TODO.md` itself
disagrees with the real state of the code, found while compiling this report.

---

## 1. Runtime / Retrieval

**UPDATE (2026-09-01):** the deferred "full ANN/vector-embedding implementation" below is now
built. RFC 0118 (umbrella) reframed retrieval as a compiled-knowledge query engine and shipped it
in eight phases, `devlog_143`–`devlog_148`: RFC 0119 the `KnowledgeStore::retrieve` seam · 0120
RRF fusion + `ExactName` · 0121 query understanding (`understand` → `QueryType`) · 0122 the QUERY
surface (`fact`/`facts_of` + named graph ops) · 0123 REASON (`QueryPlan` IR + rules planner +
typed `EvidenceSet`) · 0124 the surface (`ekos ask` compiled by default, MCP `ekos_query`/
`ekos_retrieve`, EKL `SEMANTIC`) · 0125 the vector arm (`EmbeddingProvider` + `VectorIndex`,
opt-in `[embeddings]`) · 0126 the CI-gated eval harness (`ekos_runtime::retrieval_eval`) +
per-arm timings. Still deferred: the `contextual_score` identity signal and the distributed
`VectorSearch` RPC (RFC 0125b). The EKL Object+Relationship join and async `KnowledgeStore` items
below remain open by the same deliberate choice.

**Closed (2026-08-26) — was the longest-standing gap in this document, restated across four RFCs
without closure until this session's six-RFC sequence** (`TODO.md`'s `## Ongoing / Cross-cutting`
has the full implementation detail per item): EKL now has `AS OF <timestamp>` point-in-time queries
and `COUNT`/`GROUP BY` aggregation (RFC 0096, on top of new bulk `all_objects_at`/
`all_relationships_at` primitives — `object_at`/`relationships_at` existed per-id already, RFC
0047). `ekos mcp serve` caches ledger reads via a real read-only `StoreCache` (RFC 0097 — a first
caching attempt was built, caught as unsafe by its own regression test, and reverted before
shipping: caching `FactLedger`'s writable open handle would have held tantivy's `IndexWriter` lock
indefinitely, starving any concurrent `ekos build`/`commit` in another process; RFC 0097 instead
built a genuine read-only open path that never acquires that lock). `ekos ask --stream` streams real
SSE/NDJSON responses from all three providers (RFC 0098). `ekos ask --session <name>` gives real
multi-turn conversation history (RFC 0099). Search structurally boosts `memory/`-path results 5×
(RFC 0101). Semantic/embedding search was **redesigned by explicit user direction, not built as
originally scoped**: rather than new vector-store infrastructure, RFC 0088's existing
`ai_overview`/`ai_usage` LLM prose is now indexed into search (RFC 0100) — zero new infrastructure,
and it found a real, separately-fixed bug along the way (`FactLedger::index_object` never indexed
`ocr_text`, silently breaking OCR'd-document search on every fact-engine workspace since RFC 0024).
A full ANN/vector-embedding implementation is not abandoned, just no longer attempted first — deferred
until real usage against this cheaper approach shows it's still needed.

**Still open, by deliberate choice, not oversight.** No async `KnowledgeStore`/`Runtime` methods —
RFC 0005's original sync-by-design decision (RFC 0001) was re-confirmed correct this session (100%
sync, both backends, 33 real call-site files), a trade-off, not a gap; revisit only if a concrete
future consumer (e.g. an async MCP transport) needs it. EKL still has no join across
Object+Relationship in one query — found live during RFC 0096 to be the one extension that actually
breaks EKL's flat-clause-type design (`parser.rs`'s own design comment), deferred as its own future
RFC rather than forced into the current grammar.

**Trade-off.** `World` (RFC 0048) is a computed projection over `KnowledgeStore` queries, not a
persisted entity — deliberately rejecting the source planning document's literal "world is a stored
graph+state structure" framing to keep `Runtime` read-only and avoid a second storage model.

---

## 2. MCP / Connector infrastructure

**Gap.** MCP server (RFC 0013) is stdio-only — no HTTP/SSE transport, no auth, no multi-workspace
routing. MCP exposes tools only; resources/prompts capabilities are unbuilt. Every connector lacks
generic `ScanContext`/`ekos.toml [connectors.X]` config plumbing (confirmed missing project-wide,
not just for one connector, RFC 0017). Dynamic/runtime plugin loading (`.so`/WASM) is explicitly
named "a known limitation, not solved here" by RFC 0031 itself (also RFC 0006).

---

## 3. Connector-specific gaps

GitHub: still the older REST client, no GraphQL upgrade (RFC 0020); no secondary
abuse-detection rate-limit backoff — an accepted real risk after RFC 0062's live run hit it.
Confluence: no cross-space title resolution, no LLM topic/concept extraction, still on API v1 (RFC
0022). Local-docs: no cross-document `References` edges, no per-image `KirObject`s (RFC 0023).
ClickHouse: no cross-source joins in one query, no LLM-based business-meaning enrichment of
table/column names (RFC 0056). No live Databricks Jobs API / ADF management-plane connector (RFC
0038) — only the scaffolded proof-of-concept connectors exist for Salesforce/SAP/Oracle/Fabric/
Snowflake, never exercised against live accounts. Crypto/DAO: one raw-RPC treasury connector only,
no broader governance-platform coverage (RFC 0032). No real-time streaming ingestion for the chat
connector (RFC 0033).

**Real bug found and fixed live, not an RFC gap:** `GitObserver::is_git_repo()` walks up to *any*
ancestor `.git`, so a second `[observe] paths` entry with no `.git` of its own can be wrongly
detected as a repo if it sits inside a parent directory that happens to have one — compounded by
`collect_git_artifact_ids`'s whole-store, unscoped, last-one-wins repo metadata resolution, which
can nondeterministically surface the wrong commit history. **Still open** — needs two parts: a
direct (non-ancestor-walking) `.git` check, and the same per-project scoping RFC 0079 already gave
every other multi-project analyzer.

---

## 4. Analyzers

**Gap.** No interprocedural/cross-file call-chain tracing for either Python (RFC 0040) or Rust (RFC
0041) — same underlying limitation, named separately per language. Python: no `.ipynb` notebook
support, no `spark.sql(...)` argument-text parsing, incomplete `.agg(...)` coverage (RFC 0040).
Rust: no trait-dispatch resolution (RFC 0041). SQL dialects: no deep procedural-body parsing
(`IF`/`LOOP`/cursors) for MySQL/Postgres (RFC 0031) — confirmed live against a real Postgres trigger
function during RFC 0076's real-project testing: correctly reported `Unmapped` rather than
fabricated, since procedural control flow is genuinely outside the Transformation IR's
dataflow-only scope. No semantic/embedding-based synonym matching in identity resolution (`"orders"`
≈ `"purchases"`, no shared substring) — RFC 0007's original ask, still open; **narrower case since
solved**: fuzzy token-containment across kinds (`"sites"` the table vs. `lib/plausible/site` the
directory — same word reused, not a true synonym) shipped via `cross_system.rs`.

**Trade-off.** ORM/inheritance recognition (RFC 0091/0092) is Python/SQLAlchemy-only by deliberate
choice — Django and other ORMs, and JS/TS `class X extends Y`, are named as legitimate future
extensions not attempted without a real target project to verify against, not oversights.

**Gap found live against a real external project (`pdf-reader`, 2026-08-26).** No analyzer reads
`requirements.txt`/`pyproject.toml` at all — real `package.json` dependencies compile into
`Technology` objects with a `DependsOn` edge from the owning `File`, but the equivalent
`requirements.txt` `pkg==1.2.3`/`pkg>=1.2.3` line format has no analyzer, so a real Python project's
whole runtime dependency surface is invisible to the compiled ledger. Same real run also surfaced
two usage findings worth tracking alongside the RFC gaps above, not code bugs: small local Ollama
models (`qwen2.5:1.5b`) failed structured-JSON output for 111 of 119 real `[llm-description]` calls
— the parse-failure error string is discarded (`llm_description.rs::call_and_apply`, only
`stats.llm_errors` increments, no detail logged) — and `docs generate`'s LLM-based "microservices"
architecture label was not clearly earned by the actual code shape observed, worth a closer look
before trusting that classification generally.

---

## 5. Docs generation

**Gap.** No HTML output for the curated layout — punted by three separate RFCs in a row (0037,
reaffirmed 0042 and 0045) and still Markdown-only. No Docker/Kubernetes/Terraform/cloud-config
parsing feeding curated docs (RFC 0042). No LLM-based chapter/heading detection for document section
boundaries (RFC 0024). No Transformation IR semantic (business-meaning) diffing (RFC 0028).

**"Real Descriptions, Purpose, and Links" plan — Phase 3+ not scoped.** Phases 1–2 (real doc-comment
extraction + entity-page rendering, RFC 0087) and Phase 4 (LLM-backed compile-time descriptions, RFC
0088) shipped. Phase 3 ("Links" — cross-linking between related entities' prose, beyond the existing
relationship list) is explicitly deferred until real usage shows what's missing, per the project's
own just-in-time-RFC convention — not scoped at all yet, not even in draft.

**RFC 0088's own residual gaps, tracked not silently smoothed over:**
- Python and JS symbol-level AI overviews were deferred at launch (Rust/Elixir only), then Python's
  blocker (`python_analyzer.rs` never captured `source_span`) was closed the same week (`devlog_98`).
  **JS/TS symbol-level `source_span` capture is still open.**
- `docs.rs::select_llm_provider_for_prose` and `marketing.rs`'s equivalent still use `from_env`
  instead of `from_env_with_model` — a configured non-default Ollama model is silently ignored for
  `--prose` and `ekos marketing publish` today. `recover.rs` already has the fix; it was never
  ported to these two call sites. Flagged three separate times (devlog_93, 95, 100-era notes),
  still unfixed as of this document.
- `Risk` KIR kind + `## Major risks`/`## Architecture confidence` from a *real LLM judgment* (as
  opposed to the deterministic versions that did ship, RFC 0094/0095) — still open.

**RFC 0068 (Architecture Documentation Standard) — the big one.** A 67-section external spec
unifying ISO/IEC/IEEE 42010, arc42, C4, and ISO/IEC 25010. Explicit user instruction on file: build
the *whole* thing, only sequence it, never trim it. State as of this document:

- **Done (§61 MVP, all six view items):** System Context, Component View, Runtime View,
  Architecture Summary, Technology Inventory, Data Architecture, plus SVG diagram generation for
  System Context. (RFC 0069–0075, 0081–0086, devlogs 72–89.)
- **Explicitly still open in §61's own follow-on list:** `render_graph_svg` isn't wired into
  per-object neighborhood diagrams or the per-relationship-kind Dependency Graph yet; `erDiagram`/
  `sequenceDiagram` families would need their own node/edge extraction (different Mermaid syntax,
  not a reuse of the existing one); `layer_nodes` doesn't wrap wide layers within one row — a
  live-observed real problem (this repo's own System Context rendered as an unreadable
  8296×190px row before RFC 0084's fix; the wrap-within-a-row case, as opposed to across rows, is
  still unaddressed).
- **§62 Phase 2 — partially closed this session (2026-08-26).** Architecture Diff (claim-level,
  distinct from raw `ekos diff`) shipped as `ekos architecture diff` — a real id-set comparison
  (technologies, crate role classifications, risks, open questions) between two points in time, not
  fuzzy matching, since every covered `KirId` is already deterministic (RFC 0108). Human Review
  workflow shipped as `ekos architecture review` — confirm/reject an LLM-classified crate role
  claim, following RFC 0029's `ekos_identity_review` pattern as intended; a real content-signature-
  versioning hazard (re-`commit`ing would silently reset a human's review decision, since the
  underlying claim is re-derived on every run) was found and designed around before implementation,
  not discovered after (RFC 0109). MCP exposure of architecture tools shipped —
  `ekos_architecture_evaluate`/`ekos_architecture_drift`/`ekos_architecture_diff`/
  `ekos_architecture_review` (RFC 0107/0108/0109). **Still open:** Terraform/Kubernetes/OpenAPI
  extractors don't exist (blocks Deployment Architecture entirely, since there's no compiled
  infrastructure data to render from). Security Architecture and Quality Architecture views are
  unbuilt. Architecture Drift as a *continuous*, scheduled check (vs. the MVP's one-shot
  `ekos architecture investigate` run) is unbuilt — `ekos_architecture_drift`'s MCP tool computes a
  one-shot comparison, not a background job. ADR generation is unbuilt.
- **§63 Phase 3 — entirely open:** runtime telemetry/logs/metrics/traces ingestion, continuous drift
  detection on a schedule, Architecture Q&A, Target/Migration Architecture (an *aspirational* future
  state compared against the observed one — genuinely new concept, no existing primitive), fitness
  checks, governance, evolution analysis across more than two baselines.
- **Cross-phase structural work — entirely open:** ISO 42010's Stakeholders/Concerns/Viewpoints
  framework has no EKOS concept model yet; Cross-View Consistency checking needs at least two real
  views to exist before it's even checkable; Correspondence, explicit Quality-to-Architecture and
  Architecture-to-Evidence traceability reports, a Glossary section, Appendices, a Documentation
  Quality Gate, and a packaged Machine-Readable Companion / Architecture Baseline (the last two are
  likely mostly-already-there — `.ekos/snapshots/*.json.zst` and the CKM/ledger JSON itself — just
  never packaged/documented as the deliverable) are all unbuilt.

**Trade-off.** Data Architecture's Ownership and Lifecycle views stay honestly blank for a concrete,
now-precisely-diagnosed reason (not vagueness): `git_analyzer.rs` only ever produces a
commit-event-level `OwnedBy` edge, never a per-`File` one, and no `Table`/`Dataset`↔`File` link
exists — RFC 0074's own text originally claimed the opposite and was corrected in place. Data
Quality was investigated and deliberately *not* faked from DDL `NOT NULL`/constraint metadata — a
structural constraint is a stated rule, not a measurement of real data, so it's genuinely blocked on
Phase 3 runtime telemetry, not merely unbuilt.

---

## 6. Multi-project / rollups (RFC 0044)

**Gap.** No per-sub-project curated docs generation — `ekos docs generate` always reads the whole
ledger; today, scoping to one project within a shared estate ledger requires N separate
`ekos.toml`/`.ekos` setups (confirmed against a real Databricks/ADF case). No opt-in LLM prose per
rollup (would mirror `--prose`'s existing cost-gated pattern exactly). No dedicated `ekos_summarize`
MCP tool (not urgent — rollups are ordinary `KirObject`s, already reachable via `ekos_search`/
`ekos_neighborhood`/EKL).

**Gap, explicitly not a "small fix."** Full remediation of *every* analyzer-owned id scheme for
multi-project collision safety (RFC 0044) is closed for four of five analyzer families (RFC 0079)
plus `crate_topology_analyzer`/`cicd_analyzer` (devlog_104) and `dependency_analyzer`/
`package_json_analyzer` (devlog_101) — but `github_analyzer.rs`'s `file_kir_id` (for `References`
edges parsed from PR/issue free text) remains a structurally different, harder problem: a path
parsed from prose has no single `[observe] paths` entry it naturally belongs to. Investigation
found this is now *silently wrong*, not just collision-risky, in a multi-project workspace — it
computes a bare-path id that no longer matches `build.rs`'s own project-qualified `File` id, so the
edge dangles instead of colliding. Still open.

**Gap, large and cross-cutting, deliberately deferred not fixed.** `KirRelationship`'s ids are
non-deterministic at 134 of 136 real `KirRelationship::new()` call sites across 32 files — only
`DependsOn` (RFC 0072, `crate_topology_analyzer.rs`) and SQL `ForeignKey`/`Table` (RFC 0076) got
deterministic ids so far. RFC 0072 deliberately rejected a blanket fix: `sql_analyzer.rs`'s real
`ForeignKey` edges proved that two distinct real relationships can legitimately share a
`(from, to, kind)` tuple (two FKs between the same tables via different columns), so a mechanical
global change would have silently collapsed real facts. Each of the other 134 sites needs its own
case-by-case judgment call, not a batch fix — this class of bug has recurred independently at least
three times already (Technology Inventory, Architecture Summary, and the id-staleness bug this
session's devlog_112 found in a *different* subsystem via the same root shape: an id computed
somewhere other than where the final content is fixed). **Also structurally permanent**: this class
of fix can never retroactively deduplicate rows already committed before the fix shipped — no
delete/tombstone mechanism exists anywhere in the codebase — so render-time dedup mitigations stay
in place indefinitely even after the root cause is fixed at any one call site.

---

## 7. Security / Secrets (RFC 0043)

**Gap.** No env-var-only enforcement for connector secrets yet (Postgres/Salesforce/SAP passwords
can still be literal config values; `ekos doctor` doesn't yet verify referenced env vars exist).
**No data retention/erasure story** — RFC 0043 explicitly did not resolve the tension between
GDPR-style right-to-erasure and the append-only ledger guarantee; redaction stops *new* secrets from
being stored, it provides no way to erase something already committed before the RFC shipped. This
is a structural property of the architecture (no object-level delete/tombstone exists anywhere),
not a missing feature that can be bolted on later without a real design decision.

**Gap, flagged as security-relevant not routine.** `ArtifactId` was, until this session, computed
from pre-redaction bytes while the persisted data was post-redaction — meaning a redaction-engine
fix could never retroactively apply to already-observed content. **Fixed this session** (devlog_112,
unreleased as an RFC at time of writing) for the central `build.rs` choke point. The
project's own pre-existing tracking flagged a second, adjacent gap not covered by this session's
fix: redaction isn't applied at each of roughly 15 individual plugin `data`-construction call sites
directly — only at the central `build.rs`/`recover.rs` entry points. Worth auditing whether any
plugin path bypasses the central choke point entirely.

**Trade-off.** RFC 0043's redaction covers credential-*shaped* secrets and PII in structured
connector data (e.g. commit author email) is intentionally exempt — general PII in free-running
prose (a person's name or email mentioned in a document body, not a config value) is explicitly
out of scope, by design, not oversight.

---

## 8. Demo server (RFC 0045/0046)

**Not-in-scope, by explicit RFC design.** General multi-tenant/self-serve ingestion — the demo is a
fixed, pre-baked two-repo catalog (EKOS-self + `sharkdp/fd`) on purpose, stated in the RFC's own
Non-Goals. No no-LLM/ledger-only `/ask` fallback mode exists.

**Gap.** The 5–10 minute demo script has never been rehearsed against a real, unfamiliar person —
the one remaining step before this is genuinely "done," and it needs a live human, not more code.

---

## 9. World Engine (RFC 0047–0055)

Every RFC in this crate is additive on existing KIR/ledger primitives by design (no new storage
class introduced anywhere in the crate) — itself a trade-off worth naming: the whole engine chose to
extend `Custom()` escape hatches and `properties` conventions rather than invent new first-class
types, even where the source planning document suggested otherwise (e.g. `World` as a stored graph
vs. a computed projection, RFC 0048).

**Gap, all independently reconfirmed still-open by the 2026-08-21 survey:**
- A claim-review MCP tool and `valid_from`/`valid_until` query surface on `KirObject` itself (RFC
  0047 only put temporal validity on `KirRelationship`).
- A memory-type taxonomy (short-term/long-term, tracked incorrect beliefs) — RFC 0049.
- Phase 8 (parallel agent execution — though the current ordering already produces the same
  order-independent semantics Phase 8 wanted), an LLM-backed `DecisionEngine` (only two
  deterministic reference engines exist: `AlwaysDoNothing`, `RuleBasedAgent`), per-kind action
  effects beyond the one worked `FormAlliance` example, and Phases 14–16 (Metrics, Turning-Point
  Detection, Report Generation, Monte Carlo, Counterfactuals, Web UI, Video Generation) — RFC 0050,
  reaffirmed RFC 0054.
- Per-agent decision-engine selection in scenario YAML (blocked on the LLM-backed engine above),
  scenario linting beyond structural/reference errors, a scenario ledger cleanup command — RFC 0051.
- Per-action-kind differentiated resource costs, YAML-authorable resource costs, richer domain
  conflict rules beyond the one worked example — RFC 0052.
- `Like`/`Follow`/`Share`/`Reply` as their own action kinds (currently folded into the closed
  12-action vocabulary some other way) and a nested-thread reconstruction helper — RFC 0053.
- An interactive replay stepping session and video/report rendering of a replay — RFC 0054.
- A `DocumentSemanticsAnalyzerPass` for world sources, `[security]` pattern extension applied to
  scenario ingestion, incremental/cached re-ingestion — RFC 0055.

**Trade-off, explicit and closed-ended by design.** `ActionKind` has no `Custom()` escape hatch — a
deliberate scope decision (a closed 12-action vocabulary), not an oversight the way every other KIR
enum's `Custom()` pattern would suggest.

---

## 10. Architecture Knowledge Model (RFC 0065–0067) — known gaps left by its own authors

Distinct from RFC 0068's build-out above — this is the smaller MVP reasoning layer that came first.

- The full 3-iteration reasoning loop (chunking fix included) has only been re-verified via a fast
  `recover`-only check, never a complete `ekos architecture investigate` run with the fix in place —
  both full runs that did complete predate the fix.
- 5 of 44 real crates in this repo's own workspace still don't get classified even post-fix — not
  yet root-caused as model-capability vs. a smaller second instance of the same chunking bug.
- `document_semantics_analyzer.rs::collect_sections` has the identical latent duplicate-artifact bug
  `architecture_reasoning.rs::collect_crates` was found to have and fixed — noted live, never fixed
  for this second location.
- Role classifications (`Claim` objects with `has_role`) are real, evidence-backed, and queryable —
  but never rendered into generated docs. A human reading `Architecture.md` today can't see what the
  LLM concluded about a crate's role without querying the ledger directly.
- `evaluate_architecture` computes 2 of the RFC's own listed scoring dimensions
  (`completeness`, `evidence_coverage`) only — `consistency`, `cross_view_consistency`,
  `traceability`, and the rest have no real signal to compute yet and are deliberately left unscored
  rather than faked.
- Persistent checkpointing/resume, concurrency-safety infrastructure, a CI/CD exit-code matrix +
  PR-comment workflow, and `Assumption`/`Contradiction` claim types are all named as deliberately
  not started — each is real RFC-sized work on its own, not begun speculatively ahead of an actual
  need. (A human-review workflow and further MCP tool additions, both named here as not started when
  this document was first written, shipped this session as `ekos architecture review`/
  `ekos_architecture_review` — RFC 0109 — and `ekos_architecture_evaluate`/`ekos_architecture_diff`
  — RFC 0107/0108; see §5 above. No human-review *UI* beyond the CLI/MCP tool exists.)

---

## 11. Storage / Ledger (RFC 0080 — Storage Architecture Plan)

**UPDATE (2026-09-01):** the horizontal-distribution phases are also done now. RFC 0111 Phase A
(single-machine partitioned storage — `ledger/src/partitioned/`) and RFC 0113 Phase B (distributed
— the `cluster` coordinator + `compile-worker` lease/heartbeat protocol, the `distributed`
query-worker RPC layer + `DistributedLedger` gateway with per-shard IDF merge → RRF) shipped
`devlog_130`–`devlog_144`, feature-complete at v1 on 2026-08-30. The object-store read path
(`segment-backend`) is real and verified live against MinIO + a 95-partition Elixir workspace;
two end-to-end soak runs found and fixed 8 bugs (`devlog_144`, branch merged). RFC 0125 adds the
vector index to the same `publish_aux`/`fetch_aux` object-store channel.

Real, live evidence-driven, six-phase plan. **Phases 1–3 of 6 shipped 2026-08-26; Phases 4–6
(horizontal distribution) shipped 2026-08-27 – 08-30 — see the UPDATE above:**

1. **Concurrency — shipped, RFC 0104.** Both real gaps closed: SQLite `Ledger::append`/
   `append_object`/`append_relationship` now run inside real `BEGIN IMMEDIATE`/`COMMIT`
   transactions — the likely real mechanism behind a corrupted FTS5 table found live in a real
   external project's ledger. `FactLedger` gets a real, designed cross-process `write.lock` file
   (`fs4`, promoted from transitive to direct dependency) acquired before any segment/index touch —
   a second writable process now fails fast with `LedgerError::Locked` instead of an eventual
   tantivy-internal error. RFC 0016's "the manifest lock enforces it" text (incorrect — no such lock
   existed in the code) has been corrected. The concurrent-read visibility spec turned out to be a
   real, previously-unverified gap in its own right: a `FactLedger` handle's view is frozen as of its
   own `open()` call, not auto-refreshed by a separate process's writes — now proven by a dedicated
   regression test, not just documented as an inherited claim.
2. **WAL recognition + repair tool — shipped, RFC 0105.** No new WAL needed building — `FactLedger`'s
   existing segment format (checksummed frames, atomic manifest writes) already provided real
   ledger-level durability; the real gap was that nothing surfaced it. New `ekos ledger repair`
   opens the ledger (triggering its existing free self-heals), then reports a precise per-segment
   diagnostic — which segment, which transaction range — for the one case (genuine bit-rot in a
   sealed segment) with no synthesizable automatic fix.
3. **Snapshot + compaction — shipped as version-chain checkpoints, RFC 0106.** Periodic per-entity
   checkpoints (`checkpoints.jsonl`, one every 20 versions) bound how far back a point-in-time read
   has to fold, provably equivalent to full replay by construction (a missing/corrupt checkpoint only
   costs speed, never correctness). Deliberately purely additive — built to *not* need Phase 4's
   retention question resolved first.
4. **Retention/pruning policy — blocked on a real invariant conflict found by Phase 3, not just a
   sequencing dependency.** Phase 4 as originally named means discarding old delta history, directly
   conflicting with `CLAUDE.md`'s own Key Invariant that the ledger is append-only with no
   object-level delete/tombstone mechanism anywhere (confirmed: none exists in the codebase). Needs
   an explicit decision before any Phase 4 design starts: relax the invariant (a real, load-bearing
   architectural change), or re-scope Phase 4 to something that doesn't require it (e.g. archival to
   a separate location rather than in-place deletion — not yet investigated). **Asked directly this
   session; user chose to stop the plan here rather than decide the invariant question yet.**
5. Materialized views alongside the EAV fact engine — least-scoped of the six; needs a pass over
   real EKL/MCP query logs to find what's actually worth materializing before design starts.
6. Horizontal distribution — blocked on RFC 0034 (status: Draft, not yet implemented) shipping a
   real foundation first.

**Live-recurring symptom of the same untreated root cause** (candidate-set inflation from repeatedly
re-`recover`'d long-lived workspaces, most likely uncapped `KnowledgeArtifact` accumulation read as
current input by every `compile`): `ekos resolve` took ~5 minutes against a real long-lived
workspace (29.5M pairwise comparisons over 10,178 candidates) vs. 5,241 pairs on a fresh rebuild of
the structurally identical data — a ≈5,600× difference. Recurred a second and third time in later
sessions (`SEM002` warning counts spiking 3379→3879→6331 across consecutive cycles against the same
workspace, resolving correctly after `commit`'s content-addressed dedup each time). **Deliberately
not patched with a guessed fix** — the real fix is either an artifact-store lifecycle change
(prune/supersede old `KnowledgeArtifact`s per pass) or a blocking-key improvement, both larger
changes with genuine risk of dropping evidence a narrower fix hasn't been tested against. This is
exactly Storage Architecture Phase 4's territory (retention/pruning) — `ekos ledger` still has no
real prune tool, and Phase 4 is the item explicitly paused on the append-only-invariant question
above, not merely unscheduled.

---

## 12. This session's own findings (devlog_112, not yet an RFC)

Four real bugs found running EKOS's pipeline against its own repository for the first time this
session — see `devlogs/devlog_112.md` for full detail:

1. Artifact ids were computed from pre-redaction content while the persisted data was
   post-redaction, permanently locking in whatever the redaction engine produced the *first* time
   any file was observed. **Fixed.**
2. 10 of 11 `recover.rs` artifact-id collectors never deduplicated by target at all (only
   `collect_crypto_artifact_ids` had ever been fixed — the same "fixed once, never generalized"
   pattern seen elsewhere in this log, e.g. §12's `from_env`/`from_env_with_model` gap). **Fixed.**
3. Three independent bugs in the `generic-assigned-secret` redaction pattern: asymmetric quote
   consumption, no word-boundary guard on compound identifiers, whole-match (not value-only)
   replacement deleting required struct-literal syntax. **Fixed.**
4. A one-time legacy data-corruption residue from before fix #1 existed, in two specific files,
   requiring a full `.ekos/` reset rather than a further code change — content-addressing's
   same-id-implies-same-content invariant, once violated for a specific id, cannot self-heal by any
   later code fix.

**Not yet investigated, left open from this session:** an `observeerror` identity conflict
(`RustSymbol`/`RustModule`) surfaced during `resolve --force`, and 157 of the 5,035 `compile`
warnings classified only as "other" (non-`File`-object) `SEM002`s — proceeded past both with
`--force`/acknowledgment rather than deep-diving, given the size of this repo's own real corpus and
that they were non-blocking. Worth checking against `devlog_107`'s `dangling_relationship_target_ids()`
classifier (`CkModel`, `crates/semantic/src/lib.rs`) — the "other" bucket is exactly what that
classifier already isolates for investigation, just not yet investigated for *this specific* run.
(Corrected here from an earlier "RFC 0107" mislabel in this document — that devlog number
predates, and is unrelated to, RFC 0107 / MCP architecture tools, which now really exists as of
this session; see §5/§10.)

---

## 13. Deliberate design trade-offs worth naming on their own (cut across every RFC above)

- **No delete/tombstone mechanism anywhere in the codebase.** Confirmed, not assumed — checked
  directly during RFC 0043's own retention-tension discussion. This is the single structural
  decision behind: the erasure/GDPR gap (§7), the permanence of the id-duplication bug's already-
  committed rows (§6), and the id/content-mismatch residue found this session (§12.4). It is also
  *why* World Engine simulations write to a dedicated, separate `.ekos/simulations/<id>/ledger.db`
  by default rather than the real workspace ledger (§9) — a fictional entity, once committed to the
  real ledger, could never be removed.
- **One canonical evidence model** (`KirEvidence`), never a second parallel evidence type, even
  where a new KIR extension (Claims, temporal validity, World, Agents) might have tempted one.
- **Sync end-to-end, decided once at RFC 0001** (`Observer::scan`, `LlmProvider::complete`, and
  parallel scheduling all share one model) — re-confirmed correct this session rather than
  revisited blindly (100% sync, both ledger backends, 33 real call-site files), so this stays a
  named trade-off, not a project-wide gap (§1).
- **Fuzzy identity matches are never silently merged**, only ever surfaced as reviewable
  `unconfirmed` relationships (RFC 0060/0063) — a decision made explicitly *because* no confidence
  threshold reliably separated known-good from known-wrong real pairs, not a caution taken for its
  own sake.
- **Every new self-identified `Custom(_)` KIR kind must be added to `DefaultResolver`'s blanket
  kind-exclusion list by hand** — a known, named, still-live failure mode. Nine real kinds have now
  hit this exact bug at their own launch (`Section`, `TransformNode`, `RustSymbol`, `RustModule`,
  `PythonSymbol`, `PythonModule`, `Crate`, `ElixirModule`, `ElixirSymbol`, `JsModule`, `JsSymbol`,
  `Document`, `Claim`, `ArchitectureGap` — several found live, weeks after their own RFC shipped, by
  reading a real generated page rather than by inspection). CLAUDE.md now names this obligation
  explicitly for every future analyzer; it remains a manual step, not an enforced one.

---

## 14. Stale tracking found while compiling this report

`TODO.md`'s Phase -1 through Phase 14 checkboxes (lines 1–1745, the original pre-code roadmap) are
still marked `[ ]` in every single case — including for functionality that has been shipped,
documented in the crate map, and exercised in dozens of devlogs since (`ekos build`, `ekos commit`,
`SqlAnalyzer`, `GitAnalyzer`, the LLM provider trait, etc.). The actively-maintained tracking lives
entirely in the `## Ongoing / Cross-cutting` section (line 1745 onward) — the original phase
checklist was never updated once real work superseded it. Not a functional gap, but worth a cleanup
pass so a future reader doesn't mistake `TODO.md`'s top half for current status.

---

## Summary — what would most change reality if picked up next

Ordered by real leverage, not RFC number:

1. **Storage Architecture Phase 4 — the append-only-vs-retention decision (§11)** — the top item on
   the previous version of this list (concurrency, RFC 0080 Phase 1) shipped this session (RFC
   0104); Phases 2–3 (RFC 0105/0106) shipped alongside it. Phase 4 is now the live blocker: a real
   architectural choice — relax the append-only invariant, or re-scope pruning as archival — needed
   before the plan can resume, already surfaced to the user once and deliberately left undecided.
2. **The `KirRelationship` non-deterministic-id gap at 134 remaining call sites (§6)** — this exact
   bug shape (id computed independently of, and out of sync with, the content actually persisted)
   has now caused real duplicate/dangling data at least four separate times across four different
   subsystems (Technology Inventory, Architecture Summary, `sql_analyzer.rs`, and a prior session's
   artifact-id bug). A recurring root cause, not four unrelated bugs.
2b. **`GitObserver::is_git_repo()`'s ancestor-`.git` false positive (§3)** — can nondeterministically
   surface the *wrong repository's* commit history in a multi-project workspace; small, scoped fix.
3. **RFC 0068's remaining §62/§63 work (§5)** — Architecture Diff, Human Review, and MCP exposure
   closed this session; Terraform/Kubernetes/OpenAPI extractors, Security/Quality Architecture views,
   continuous drift, and ADR generation are what's left of the largest single body of
   explicitly-scoped work in the project, still under a standing instruction not to trim it, only
   sequence it.
4. **`requirements.txt`/`pyproject.toml` Python dependency analysis (§4)** — a real, live-found gap:
   a Python project's whole runtime dependency surface is currently invisible to the compiled ledger,
   the one gap class (declared dependencies) that already has a working analogue for `package.json`.
5. **Env-var-only connector secrets + `ekos doctor` verification (§7)** — small, well-scoped, and
   directly closes a real credential-hygiene gap for every non-file connector.
