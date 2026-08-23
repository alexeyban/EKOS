# Devlog 93 — RFC 0088 implemented: LLM-backed compile-time descriptions

**Date:** 2026-08-23
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Implemented RFC 0088 in full — modules, subsystems, symbols, and a project-level Purpose/
Architecture-style summary, all real, evidence-grounded, and persisted to the ledger at `commit`
time rather than regenerated at every `docs generate`. A design correction was found and applied
*before* writing any code (reading `semantic`/`ledger` source directly rather than assuming): this
had to be a post-`commit` step operating on `&dyn KnowledgeStore`, not a `CompilerPass`, because
this pipeline's ledger versions whole objects, not patches, and `merge_graphs`/`build_ckm` never
dedupe `KirObject`s sharing an id across passes. Live-verified end-to-end against a real local
Ollama model (`llama3:latest`, zero real API cost) after finding and fixing two further real bugs
the live run surfaced that no unit test had caught: a file-path resolution bug (found by the live
run producing "0 modules, 0 symbols described... 3 skipped without a source span" against real
data) and a pre-existing Ollama model-selection bug this session's own commit.rs copy inherited
from `docs.rs`/`marketing.rs`.

## What was built

| Component | What |
|---|---|
| `ekos/crates/recovery/src/llm_description.rs` (new) | `describe_objects` (module/symbol pass), `describe_project` (project-level Purpose/style), `estimate_call_counts` (cost gate), 17 tests |
| `ekos/crates/recovery/src/rust_analyzer.rs` | Real `source_span` per `RustSymbol` via `syn`'s joined `Spanned` span |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | Real `source_span` per `ElixirSymbol` via the existing block-depth stack's push/pop lines |
| `ekos/crates/compiler-core/src/config.rs` | `[llm-description]` config (`enabled`, `scope: modules\|symbols\|all`, defaults to the cheaper `modules`) |
| `ekos/crates/docs-gen/src/lib.rs` | "## AI-Assisted Overview" section, stale-comment callout on Definition, `Architecture.md` reads real Purpose/Architecture-style when compiled |
| `ekos/crates/cli/src/commands/commit.rs` | Cost-estimate + confirm gate, `ekos commit --yes`, wiring `describe_objects`/`describe_project` in after `commit_data_lineage` |
| `ekos/Cargo.toml`, `recovery/Cargo.toml` | `proc-macro2/span-locations` (real line numbers outside a real proc-macro context), `ekos-ledger` dependency (confirmed no cycle first) |

## Design correction found before writing any code

RFC 0088's original draft assumed a new `CompilerPass` writing `ai_overview`/etc. directly onto the
same `KirId` an existing module/symbol object already has. Reading `semantic/src/lib.rs` and
`ledger/src/fact_ledger.rs`/`lib.rs` directly (not assumed) before implementing found this would be
actively dangerous: `merge_graphs`/`build_ckm` never dedupe objects sharing an id across two
passes' artifacts, and `commit.rs` appends each one in iteration order; the ledger itself *is*
versioned by full content, but each version is a **complete snapshot, not a patch** — a bare
`KirObject { id: <same>, properties: {"ai_overview": ...} }` could become the new "current" version
and silently regress every real structural property (`kind`, `arity`, `visibility`, RFC 0087's
`description`) another pass already wrote. Fixed at the design stage: this step reads the real
current full object from `&dyn KnowledgeStore`, clones it, adds the new properties to *that* clone,
and re-appends the clone. Consequently it also can't be a `CompilerPass` inside `compile` at all —
it needs the fully-committed graph — so it runs as a post-`commit` step, the same architectural
slot `commit_rollups`/`commit_data_lineage` already occupy.

## Evidence model, simpler than `ask`'s

`ai.rs::AiRuntime::ask`'s citation validation exists to guard against a real risk: retrieval can
surface content the model wasn't meant to cite. Nothing here is retrieved — every prompt this
module builds is assembled entirely from real, already-compiled data (real neighbor names, real
`source_span`-sliced source text) handed to the model directly, so there's no "cited something it
wasn't shown" risk to guard against. One real `KirEvidence` is created per described object and
appended to that object's own native `evidence` field (not a new property) — it renders through the
same `## Evidence` section every other object already uses, a simpler and more idiomatic choice
than the RFC's original sketch of a separate `ai_evidence` property.

## Two real bugs the live run found (neither caught by 17 passing unit tests)

**Bug 1 — file-path resolution ignored the real `project` property.** First live run against a
real (if tiny) fixture reported "0 modules, 0 symbols described... 3 skipped without a source
span" — despite the compiled CKM genuinely having real `source_span` properties on all three
symbols (checked directly, not assumed). Root cause: a `File` object's own `name` is relative to
whichever single `[observe] paths` entry it was walked from (`build.rs`'s `WalkDir::new(base)`),
not to the workspace root — `build.rs` already writes the dropped directory prefix back as a real
`"project"` property (RFC 0079) whenever more than one `[observe] paths` entry exists, but this
module's `read_symbol_source` joined `workspace_root` with the bare `file.name` directly, so it
tried to open e.g. `<root>/repo.ex` instead of the real `<root>/lib/plausible/repo.ex` — every real
symbol in any multi-path workspace (the real analytics project's own backend-only config: 8 real
`[observe] paths` entries) would have silently failed this exact way. Fixed with a new
`real_file_path` helper (`project` + `/` + `name` when `project` is present); new regression test
(`a_multi_path_workspaces_file_prefix_is_reconstructed_from_its_real_project_property`) reproduces
the exact real shape. All 17 existing unit tests had passed regardless — every one of them built its
own `File` fixture with the full real path already baked into `.name` directly, none exercised the
real multi-path `project`-property shape.

**Bug 2 — Ollama model selection ignored `[llm].model`, inherited from a pre-existing sibling bug.**
Second live run showed a clear error: `model 'llama3.1:8b' not found` — despite `ekos.toml` naming
`qwen2.5:1.5b`. `select_llm_provider_for_description` was copied from `docs.rs`'s
`select_llm_provider_for_prose`, which calls `OllamaProvider::from_env()` (always the hardcoded
`llama3.1:8b` default) rather than `from_env_with_model(config.llm.model.as_deref())`. Grepping
every call site found `recover.rs` already carries the correct fix, but `docs.rs`/`marketing.rs`
still have the same bug this session's copy inherited — a real, pre-existing gap in two files this
session did not touch, flagged here rather than silently fixed out-of-scope. Fixed only in
`commit.rs`'s own new call site.

**Separately (not a bug):** `qwen2.5:1.5b` itself doesn't reliably follow this module's JSON-output
instructions — confirmed directly via a raw `curl` to the Ollama API (free-text prose came back,
not JSON), matching `architecture_reasoning.rs`'s own already-documented caveat about this exact
model's structured-output compliance at this size. This module's `serde_json::from_str` failure
path handled it exactly as designed: `llm_errors` incremented, no crash, no garbage written. Full,
clean, zero-error success came from `llama3:latest` (8B) instead, also free/local via Ollama.

## Live verification (real, local, zero real API cost)

A small real Rust fixture (2 files, 3 functions, one with a real `///` doc comment) run through the
full pipeline with `[llm-description] enabled = true, scope = "all"` and `[llm] provider = "ollama",
model = "llama3:latest"`:

- `ekos commit --yes` output: `AI descriptions: 2 module(s), 3 symbol(s) described (0 cached, 0
  skipped without a source span, 0 errors)`.
- `get_user`'s real generated page: real `## Definition` (RFC 0087's real doc comment, unchanged),
  a real, accurate, non-verbatim `## AI-Assisted Overview` ("This function fetches a user record by
  ID, retrying once in case of a transient connection error before giving up") with real cited
  evidence.
- `try_fetch` (genuinely undocumented): honest "_Not documented in source._" Definition, but a real,
  correct AI-Assisted Overview independently describing its actual `id == 42` special-case logic —
  proving the pass grounds in real code, not just an existing comment.
- `Architecture.md`'s `## Architecture Summary`: real `**Architecture style:** modular monolith
  _(LLM-assisted, RFC 0088 ...)_` — and, correctly, `**Purpose:** _not yet computed_` stayed
  unfilled since the fixture had no real README to ground a purpose statement from — the honest
  "no fabrication" behavior working exactly as designed, not a bug.

Full workspace gate (`build`/`test`/`clippy -D warnings`/`fmt --check` from `ekos/`, plus `cd
tests/integration && cargo test`) clean throughout, including after both live-found-bug fixes.

## Knowledge Captured

- **`File.name` is relative to its own `[observe] paths` entry, not the workspace root, whenever
  more than one entry is configured** — any new code reading a real file from disk based on a
  compiled `File` object must reconstruct the real path via the object's own `"project"` property
  (RFC 0079), never assume `.name` alone is already workspace-root-relative. This project's own
  multi-path real projects (the analytics backend-only config: 8 entries) are exactly where this
  bites, and it's silent — no error, just a failed read that degrades into "skipped."
- **A small local model's JSON-output non-compliance is a real, load-bearing constraint, not a
  theoretical one** — confirmed directly via a raw API call before concluding it was the cause,
  not assumed from the error message alone. `architecture_reasoning.rs` had already documented this
  for `qwen2.5:1.5b` specifically; this session reconfirms it independently for a *different* new
  LLM-backed pass, suggesting it's a property of the model, not of any one pass's prompt design.
- **Copying a working pattern (`select_llm_provider_for_prose`) can copy its bugs too** — the
  `from_env` vs. `from_env_with_model` gap already existed in two other files before this session;
  copying without re-verifying against the one call site that *had* already been fixed
  (`recover.rs`) silently propagated it into new code. Worth grepping every existing call site of a
  pattern being reused, not just the nearest/most obvious one.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/llm_description.rs` (new) | `describe_objects`, `describe_project`, `estimate_call_counts`, `real_file_path`; 17 tests |
| `ekos/crates/recovery/src/rust_analyzer.rs` | `source_span` via `syn`'s joined `Spanned`; 4 tests |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `source_span` via the existing block-depth stack; 6 tests |
| `ekos/crates/recovery/Cargo.toml` | New `ekos-ledger`, `proc-macro2` dependencies |
| `ekos/crates/compiler-core/src/config.rs` | `LlmDescriptionConfig`/`DescriptionScope`; 2 tests |
| `ekos/crates/docs-gen/src/lib.rs` | AI-Assisted Overview section, stale-comment callout, `ProjectSummary` read in Architecture Summary; 6 tests |
| `ekos/crates/cli/src/commands/commit.rs` | Cost gate, `--yes` flag, `describe_objects`/`describe_project` wiring (async `run`) |
| `ekos/crates/cli/src/commands/architecture.rs`, `src/bin/ekos.rs`, `cli/tests/*.rs`, `tests/integration/tests/integration.rs` | Updated for `commit::run`'s new async signature + `yes` parameter |
| `ekos/Cargo.toml` | `proc-macro2` with `span-locations` |
| `ekos/docs/rfcs/0088-llm-backed-compile-time-descriptions.md` | Revised with the post-`commit` design correction and both live-found bugs |
| `devlogs/devlog_93.md` | This file |
