# Devlog 163 — Tech-debt paydown pass: security audit, resolve-cost fix, F4/F7, requirements.txt analyzer

**Date:** 2026-09-04
**Branch:** `main` (10 commits, direct — local-tests-only + `[skip ci]`, per standing maintainer direction)
**Related:** follows directly from `devlog_162` (RFC 0135 Part B follow-up, same session)

---

## Summary

The user pasted an external "EKOS Technical Debt Paydown Plan" and asked to fix the whole thing.
Verification first (five parallel research forks) found the plan's own "Wave 1" — its highest
urgency item — was already fully shipped that same day under RFC 0135. The corrected plan's
remaining waves were then executed directly, autonomously, one real fix at a time: build → test →
clippy → fmt → commit for each. Ten real, independently-tested changes landed, plus one
investigated-but-correctly-reverted attempt (`oxc_parser` unpin) and two investigated-but-left-open
findings (F2, the `RustSymbol`/`Crate` self-naming collision). The single highest-priority item in
the original plan — a claimed live security gap — turned out to be stale: already closed, no code
change needed.

---

## PR — Planning pass: RFC-number collision fix + stale-item corrections

### Problem / motivation

The pasted plan's own "Wave −1" flagged `ArtifactId` computed from pre-redaction bytes and
redaction not applied at ~15 plugin sites as the single highest-priority item. Separately, the
plan's own numbering scheme was about to collide with a real, already-in-flight collision: RFC 0134
and RFC 0127 both still pointed their "next number" placeholders at "0135+"/"0128+" — both now
taken (0135 by the provenance RFC shipped that day, 0128 by Web Console Phase 0).

### What was built

- Investigated the security item directly against current source rather than trusting the plan's
  framing. Both halves are already closed: `build.rs:338-340` recomputes `ArtifactId` from
  post-redaction content (fixed 2026-08-25 for a staleness bug, closes this as a side effect), and
  `redact_json` (`redaction.rs:289-304`) recursively redacts every observer's artifact `data`
  unconditionally at `build.rs`'s single central choke point — by design (`devlog_43`), not a gap.
  No code change; corrected the TODO.md record instead of building a fix for a non-issue.
- Repointed the RFC-number collision: Web Console Phase 6 remainder + Phase 7 → **RFC 0136**; the
  long-displaced computed-staleness/doc-signature-drift RFC → **RFC 0137**. Updated both RFC files'
  own text and TODO.md's mirroring lines.
- Gave RFC 0112 (lock-free snapshot reads) and the `devlog_100` permission-denial incident their
  own dedicated TODO.md items — both previously only prose/passing mentions, which is how they'd
  gone untracked.

### Decisions

- **Verify before fixing, every time.** The plan's own "Wave 1" (RFC-relationship-determinism +
  ledger-provenance) was entirely obsolete before this session started. Treating every claim in an
  externally-sourced plan as needing direct re-verification against current source — not just the
  headline items — is what caught the stale security item before a wasted fix attempt.

---

## PR — Wave 0: CI coverage, clippy cleanup, Ollama model fix, git false-positive

### What was built

| Item | Change |
|---|---|
| CI | New `integration` job runs `tests/integration`'s own test/clippy/fmt gates (previously uncovered — broke silently on `main` at least once, `devlog_146`). Main job's clippy step gained `--all-targets`, the real fix for accumulated test-code lint noise (not toolchain drift, as first suspected). |
| Clippy | Every warning `--all-targets` surfaced across the workspace + `tests/integration` fixed: `field_reassign_with_default`, `unnecessary_get_then_check`, `needless_borrows_for_generic_args`, `bool_assert_comparison`/`bool_comparison`, `too_many_arguments` + `type_complexity` (bundled a test helper's 8 args into a `SeedItem` struct), `suspicious_open_options`, an unused import. |
| `docs.rs`/`marketing.rs` | `OllamaProvider::from_env()` → `from_env_with_model(...)`, matching the fix `recover.rs`/`commit.rs` already had — both silently ignored a configured `[llm] model`. |
| `llm_description.rs` | Structured-JSON call failures were silently discarded (`Err(_) => stats.llm_errors += 1`); now logged at `warn` with the real provider error string. |
| `plugins/git` | `is_git_repo()` used `git rev-parse --git-dir`, which walks up to *any* ancestor `.git` — a directory with no repo of its own could register as one if it merely sat inside a parent that has one. Now checks `.git` directly inside the given path. Regression test added. `collect_git_artifact_ids`'s remaining per-project scoping gap left open (touches ledger-bound git provenance, deserves its own tests-first pass). |
| `oxc_parser` | **Attempted, reverted.** The rustc-version blocker (needed 1.95, workspace had 1.93) is gone (now 1.98) — looked like a trivial bump. Bumping 0.133.0 → 0.148.0 (confirmed latest) revealed `oxc_ast::ExportNamedDeclaration` dropped its `declaration`/`source`/`with_clause` fields entirely between versions — a real AST restructuring, not a rename. Reverted rather than guess at the new export-representation shape under time pressure; this touches the export-detection logic behind a live-verified 99.3%/434-`JsModule` result. Documented as a real, properly-scoped follow-on, not a quick win. |

### Decisions

- **Toolchain-blocker-gone ≠ safe unpin**, for a fast-moving pre-1.0 dependency 15 minor versions
  behind. Checking "does it compile" isn't enough when the fix touches semantic AST shape a
  downstream consumer pattern-matches on directly — always diff the actual type definitions across
  the version gap before trusting a version bump is mechanical.

---

## PR — Wave 2: `KnowledgeStore: Send` audit

### Problem / motivation

`devlog_141` (RFC 0115, MCP-over-TCP) explicitly declined to add a `Send` bound to
`Box<dyn KnowledgeStore>` "for lack of a real audit," sidestepping via per-connection caches
instead — an accepted v1 shortcut the Web Console's async supervisor + job-runner architecture now
depends on more directly.

### What was built

Did the audit the compiler itself can answer authoritatively: `assert_send::<T>()` static-assertion
probes (scratch tests, deleted after use) against every real implementor — `Ledger`, `FactLedger`,
`partitioned::PartitionedLedger`, `ekos_distributed::DistributedLedger` — all already `Send`. A
separate probe against `dyn KnowledgeStore` itself failed with exactly one named cause: the trait's
own missing bound, no structural blocker anywhere. Added `pub trait KnowledgeStore: Send`. Full
workspace build/test/clippy/fmt clean with the bound in place — confirms the finding, not just
asserts it.

RFC 0112 itself (the actual lock-free snapshot-read feature the Send question was tangled up with
in TODO.md) is a separate, larger implementation effort this bound doesn't unblock or simplify —
corrected the record to stop conflating the two.

---

## PR — Wave 3: `resolve`'s pairwise-cost fix

### Problem / motivation

A real, previously-investigated-but-unfixed finding: `ekos resolve` measured 29.5M pairwise
identity-resolution comparisons over 10,178 candidates on a long-lived, repeatedly-`recover`'d
workspace, vs. 5,241 (~5,600× fewer) on a structurally identical fresh rebuild. Root cause was
named but not fixed: "accumulated `KnowledgeArtifact`s from many historical runs all still read as
current input by `compile`."

### What was built

Traced the exact mechanism: `SemanticCompilerPass::run` called `ctx.artifact_store.list()` and read
back *every* `artifact_type == "knowledge"` artifact ever written. Each is content-addressed by its
own hash, so a re-recovered file (new content → new hash) never overwrites its predecessor — it
just accumulates a new sibling forever.

`dedup_knowledge_artifact_ids` filters `compile`'s own candidate *read* to the newest
`KnowledgeArtifact` per logical target: `(pass_name, raw input's own target field)` for the
dominant one-artifact-per-file shape (resolved by reading the single input artifact back), falling
back to `(pass_name, exact input_ids)` for the few multi-input passes. **Nothing is deleted from
the artifact store** — a pure read-time filter, so it needed no relaxation of the ledger's
append-only guarantee. (Physical disk-space reclamation is a separate, later, admin-operated
concern — see Decisions below.)

4 new tests (same-target dedup, different-target independence, multi-input exact-rerun collapse,
end-to-end `SemanticCompilerPass::run` proving the stale object never reaches the CKM).
Live-verified against this repo's own real, long-lived `.ekos/` workspace: `ekos compile` still
runs clean (4783 objects, 7496 relationships).

### Decisions

- **User's explicit direction on retention, obtained mid-session:** "how much and how long to keep
  data is completely a user/consumer question" — retention/pruning must be admin/user-operated
  policy, never an automatic EKOS decision. This fix sidesteps needing that policy at all (it's a
  read-time filter, not a deletion), but the direction is recorded for whenever physical
  `KnowledgeArtifact` retention is eventually built.
- **Candidate-set filtering over physical deletion**, given the choice — lower risk, immediately
  effective, and orthogonal to the harder disk-space-reclamation design question.

---

## PR — Wave 4: F4 (`arm_timings`), F7 (stemming), lease acquire-retry

### F4 — `arm_timings` empty on partitioned + distributed stores

`PartitionedLedger::retrieve`'s own comment claimed per-arm timing was "already aggregated by the
fan-out" — no aggregation code actually existed; each partition's real `FactLedger::retrieve`
timing was computed and discarded. Fixed: sum `elapsed_ms`/`candidates` per `SignalSource` across
partitions (the fan-out loop is sequential, so summing is real total wall-clock).
`DistributedLedger`'s query-worker RPC doesn't carry a worker-internal arm breakdown over the wire,
so its fix measures at the gateway boundary instead (wall-clock around the fan-out round trip for
`Bm25`, local compute time for `ExactName`) — real measured data, coarser-grained than
`FactLedger`'s own, an honest scope difference not a shortcut. 2 new tests, one a real
2-query-worker distributed gateway (spawns an ephemeral coordinator + two workers, not mocked).

### F7 — inflected entity-mention resolution

"the customer table" (singular) missed the `Customers` object; "the Customers table" (plural,
exact) resolved fine. Root cause: BM25 used tantivy's plain `"default"` tokenizer (lowercase +
split, no stemming). Fixed: `name`/`content` fields now use `"en_stem"` — tantivy's own built-in
stemming tokenizer, pre-registered in every `TokenizerManager::default()`, nothing to hand-roll;
`kind` stays unstemmed (closed enum-like vocabulary).

**Query-side fix required too**, found live mid-implementation: query terms must be stemmed the
same way before becoming a `Term`, or a stemmed-at-index-time token never matches an
unstemmed-at-query-time one — missing this broke *every* search (14 test failures) before it was
added. Fixed by fetching the same `"en_stem"` analyzer via `searcher().index().tokenizers()` and
stemming `name`/`content` query terms, leaving `kind` on the plain lowercased term.

`RFC 0103`'s existing stale-schema self-heal (wipe + rebuild on next writable open) covers every
existing workspace automatically — no migration needed.

2 new direct tests + 1 existing test's expectations updated with reasoning (`order_items` now also
matches "orders" via the shared stem — a real recall improvement, ranked below the exact name
match, above content-only mentions).

**Retrieval eval baseline**: R@10/MRR/nDCG unchanged (confirmed byte-identical via
`retrieval_eval::tests::print_current`); `intent_accuracy` moved 0.85→0.83 — traced to exactly one
reference query ("how a customer first gets set up") whose entity-resolution confidence crossed
`classify_intent`'s dominant-entity bar for the first time, arguably a *more* correct classification
than its recorded label, not a quality regression. Baseline re-captured via the file's own
documented mechanism, not silently loosened.

### Lease acquire-retry loop

`ekos compile-worker run` previously failed immediately whenever the target shard's lease was
already held, with no way to wait it out. Added `--retry-lease-seconds <N>` (0 default, unchanged
fail-fast behavior — `crates/cluster/tests/harness.rs`'s "B must not get the held shard" test
relies on the original contract). Lives in `compile_worker_run` (the CLI layer), not
`CompileWorker::run_shard` itself. Retries *only* an `"already leased"` conflict — the one error
`lease_acquire` can produce before any work has started, so retrying it can never re-run a pipeline
that already started. Real end-to-end test: a live coordinator, a stub worker holding the shard,
`compile_worker_run` started concurrently with retries enabled, the shard released mid-wait, the
real pipeline still completes via retry.

**Investigated, not attempted:** interrupt-in-flight on lease loss (the pipeline currently always
runs to completion even after the heartbeat detects a lost lease). Needs a cancellation signal
threaded through the whole `build→recover→resolve→compile→commit` pipeline — materially bigger and
riskier than any of the above; deliberately deferred.

**Investigated, not fixed:** F2 (`ekos diff` empty for a `--from` predating the ledger's first
write, on a partitioned workspace). Traced `FactLedger::diff`'s window computation, the sealed-
segment scan path, and `PartitionedLedger::diff`'s merge — all correct in two constructed
reproductions (2 new regression tests added). The original report's `Unchanged: 0` symptom (not
just `touched: 0`) points more at `self_counts`/`open_store` routing to an empty store than a
`diff` math bug — closer in shape to F5/F6 (both root-caused to partitioned-workspace config/state
not loading correctly) than to what was fixed here. Left open rather than guessed at.

**Investigated, not attempted:** `ekos ask`/EKL `SEMANTIC` still hardcode `RetrievalRequest::lexical`
despite `ekos-runtime` already depending on `ekos-recovery` (where `EmbeddingProvider` lives) — no
crate-layering blocker, but real design surface across three call sites (question-level vs.
per-node embedding reuse, EKL's separate wiring) that deserves its own pass, not a rushed swap.

---

## PR — Wave 5: `requirements.txt` analyzer, `github_analyzer` project-qualification fix

### A — `requirements_analyzer.rs`

Real gap found running the full pipeline against a real external project (`pdf-reader`: FastAPI
backend + React/TS frontend): every generated Technology Inventory / System Context view was blind
to all of a real FastAPI backend's declared pip dependencies, even though the Python source itself
was fully analyzed. New pass mirrors `package_json_analyzer.rs`'s exact shape (same `Technology` id
scheme, same `File`→`Technology` `DependsOn` edge, same RFC 0079 project qualification, same
manifest-collection pattern in `recover.rs`). Parses PEP 508's common subset — comments, blank
lines, `-r`/`-e`/`--flag` option lines, and VCS/URL requirements are skipped rather than fabricating
a `Technology` for something that isn't a plain declared package version.

8 new unit tests. **Live-verified end to end** against a fresh scratch workspace with a real
6-dependency `requirements.txt`: `ekos recover` finds all 6, `ekos query find` locates the real
object in the committed ledger, `ekos docs generate --layout curated` renders all 6 in
`Architecture.md`'s Technology Inventory.

### B — `github_analyzer.rs`'s `file_kir_id`

The last untouched piece of RFC 0079's project-qualification arc — and, on investigation, worse
than "untouched": *silently wrong*. `build.rs`'s central choke point already stamps a `"project"`
field onto every connector's artifact `data` (GitHub items included, same as git/rust/python);
`ItemData` just never had a field to read it back into, so it was silently dropped during
deserialization. `file_kir_id(path)` kept hashing the bare path, so a `References` edge pointed at
a `KirId` that no longer matched `build.rs`'s own project-qualified `File` object the moment a
workspace had more than one `[observe] paths` entry — dangling, not colliding.

Fixed: added `project: Option<String>` to `ItemData`, qualify `path` via
`ekos_common::project::project_qualify` before `file_kir_id`. Verified byte-identical to
`build.rs`'s own `id_key = format!("{project_key}:{rel_str}")` scheme via a direct id-equality
test, not just "it runs."

### Investigated, not attempted: Wave 5C (`RustSymbol`/`Crate` self-naming collision)

RFC 0078/`devlog_81` already fixed the `Technology`/`Crate` half; the `RustSymbol`/`Crate` half
needs a genuine structural link (a real relationship connecting a `Crate` to its own source
`File`/`RustModule`/`RustSymbol` tree) before identity resolution can distinguish legitimate
self-naming (a crate's own `mod pcre2` inside crate `pcre2`) from a real coincidental collision.
Unlike the JS/Technology precedent (`is_expected_technology_jsmodule_pair`, a safe *name-shape-only*
heuristic), a name-only heuristic here risks silently widening past real conflicts — Rust module
names like `utils`/`types`/`error` are common enough that "shares a name with some crate" alone
isn't a safe signal. Needs cross-analyzer coordination (`crate_topology_analyzer.rs` +
`rust_analyzer.rs` don't currently share object ids) and its own design pass — correctly left as
TODO.md already honestly scoped it, not attempted here.

---

## Knowledge Captured

- **Tantivy's default `TokenizerManager` already registers `"en_stem"`** (lowercase +
  `RemoveLongFilter` + English `Stemmer`) — no manual `index.tokenizers().register(...)` needed for
  the common case. Worth checking tantivy's built-ins before hand-rolling an analyzer.
- **Changing a field's indexed tokenizer is a schema change tantivy's `SchemaError` mismatch
  detects** (not just field additions/removals) — `RFC 0103`'s existing stale-schema self-heal
  covered this migration for free, confirmed live via the existing `writable_open_self_heals_a_
  stale_on_disk_schema` test passing unmodified.
- **A hand-constructed `Term::from_field_text` bypasses whatever tokenizer the schema declares for
  that field** — `SearchIndex::query_scored` builds terms directly from lowercased query text, so
  switching a field's indexing tokenizer to something non-trivial (stemming) *requires* mirroring
  that transform on the query side, or every search on that field silently returns zero results.
  Found this the hard way (14 test failures on the first attempt) before fixing the query side too.
- **`self_counts` computing the whole-ledger total, not just the requested window,** means a
  `LedgerDiff`'s `unchanged` count is a red herring if `self_counts` itself is wrong (e.g., wrong
  store opened) — `unchanged: 0` alongside `touched: 0` is a stronger signal of "opened the wrong
  store" than of "the window math is broken," worth remembering when triaging a similar report.
- **`ObservationArtifact`/`KnowledgeArtifact`'s `#[serde(flatten)]` content wrapper** means new
  fields on the flattened struct silently vanish from `serde_json::from_value` if the *consuming*
  struct (e.g. `github_analyzer.rs`'s `ItemData`) doesn't also declare them — `build.rs` stamping a
  field onto raw JSON is necessary but not sufficient; every consumer needs its own matching field.

---

## Files Changed

| File | Change summary |
|---|---|
| `.github/workflows/ci.yml` | new `integration` job; `--all-targets` on main clippy step |
| `TODO.md` | corrected/closed ~12 items across this pass, RFC-number repointing |
| `ekos/docs/rfcs/{0127,0134}-*.md` | repointed stale "next number" placeholders to 0136/0137 |
| `ekos/crates/cli/src/commands/{ask,docs,marketing,recover}.rs` | Ollama model fix, `field_reassign_with_default` fixes, `requirements.txt` manifest collection |
| `ekos/crates/recovery/src/llm_description.rs` | log real LLM error string instead of discarding it |
| `ekos/crates/recovery/src/{cache,confluence_analyzer,dbt_analyzer,elixir_analyzer,github_analyzer}.rs` | clippy fixes; `github_analyzer.rs` project-qualification fix |
| `ekos/crates/recovery/src/requirements_analyzer.rs` | new — `requirements.txt` dependency analyzer |
| `ekos/plugins/git/src/lib.rs` | `is_git_repo` ancestor-`.git` false-positive fix |
| `ekos/crates/compiler-core/src/diagnostics.rs`, `ekos/crates/distributed/tests/gateway.rs`, `ekos/crates/docs-gen/src/lib.rs`, `ekos/crates/ledger/src/fact_ledger.rs`, `tests/integration/tests/integration.rs` | clippy fixes |
| `ekos/crates/ledger/src/lib.rs` | `KnowledgeStore: Send` bound |
| `ekos/crates/semantic/src/lib.rs` | `dedup_knowledge_artifact_ids` (resolve-cost fix) |
| `ekos/crates/ledger/src/partitioned/mod.rs`, `ekos/crates/distributed/src/gateway.rs` | F4 `arm_timings` fix |
| `ekos/crates/ledger/src/search.rs` | F7 `en_stem` tokenizer, index + query sides |
| `ekos/crates/runtime/src/retrieval_eval.rs` | baseline re-capture with justification |
| `ekos/crates/cli/src/bin/ekos.rs`, `ekos/crates/cli/src/commands/cluster.rs` | `--retry-lease-seconds` |
