# RFC 0088 — LLM-Backed Compile-Time Descriptions (Modules, Subsystems, and Symbols)

**Status:** Proposed
**Author:** EKOS team
**Created:** 2026-08-23
**Revised:** 2026-08-23 — per-symbol (function/method-level) description generation folded into
this RFC's scope at the user's explicit request, no longer a deferred Phase 2. See "Revision
history" at the end.

---

## Motivation

Live-verified against the real analytics project's backend-only ledger (this session): a large
share of generated entity pages carry no real content at all beyond a name and a diagram — every
`Module`/`Symbol` object whose source genuinely has no `///`/`@moduledoc`/docstring/JSDoc (RFC
0087) renders "_Not documented in source._", "_No compiled properties._", and often no
relationships either. RFC 0087 only ever surfaces documentation that **already exists** in source —
real, but honest about doing nothing for the (common, in most real codebases) case where it
doesn't. The user requesting this RFC put it directly: entity pages need a real description/usage
section *regardless of whether the source file has a comment at all*, and — this revision's own
scope addition — **even a module/function that already has a real comment needs the same
treatment**, because a human-written comment can be stale, wrong, or incomplete; RFC 0087 can only
ever surface what a comment *claims*, never check it against what the code *actually does*. Neither
of EKOS's two existing LLM paths does this at the right layer: `--prose` (RFC 0035 Phase 5) runs at
*render* time, on-the-fly, every `docs generate --prose` call, never persisted to the ledger, so
it's outside the compiled knowledge model entirely and every regeneration re-spends the same
tokens; `architecture_reasoning.rs` (RFC 0065/0066) is Rust/`Crate`-specific role classification,
not a per-object description.

Separately, `Architecture.md`'s own `## Architecture Summary` section (RFC 0071) is honestly
explicit about four unfilled fields: **Purpose**, **Architecture style**, **Major risks**,
**Architecture confidence** — RFC 0068 §14's own standard names them, but nothing in this codebase
computes them today. Confirmed by direct code search before writing this: no `Risk` KIR kind
exists anywhere (`grep -rn '"Risk"' crates/` — zero real hits).

## Scope of this RFC

**In scope (this RFC, one implementation phase):** one new compile-time `CompilerPass` that asks an
LLM to write a real, evidence-grounded overview for:

1. Every **`Module`**-level object (`ElixirModule`, `RustModule`, `PythonModule`, `JsModule`, and
   `Crate`) and every **`Rollup`** (subsystem) object — as originally scoped.
2. **Every `Symbol`-level object** (`ElixirSymbol`, `RustSymbol`, `PythonSymbol`, `JsSymbol`) — folded
   in at the user's request. This is a materially different job from (1), not just a smaller version
   of it: a module's real "usage" can be inferred from its already-compiled `DependsOn`/`Contains`
   graph, but a function's real behavior can't — the LLM needs the **real function body source
   text**, which nothing in the compiled KIR graph carries today (see "New requirement: symbol
   source spans" below). And per the user's own framing, kept verbatim since it sets this whole
   sub-scope's actual bar: *"comments may be inactual or improper, incomplete, so that LLM need on
   every function, procedure and class, module — to save real code state"* — i.e. this pass must run
   against real current code and be willing to say a comment is stale, not just fill in comments
   that are absent.

Both regardless of whether RFC 0087 already found a real doc comment for that object — a doc
comment being present is real input to the prompt (see Design), not a skip condition.

**Still out of scope, explicitly deferred to a later RFC:** the `Risk` KIR kind + `## Major risks`
(needs a new object kind and a real source of risk signal — likely the evaluator/drift findings
RFC 0065/0069 already compute, not fresh LLM invention); `## Architecture confidence` from a real
LLM judgment (the existing deterministic evaluator only has two narrow dimensions today). `##
Purpose`/`## Architecture style` **are** in scope — see below, one project-level call reusing the
same shape.

## Implementation-time design correction: not a `CompilerPass`, runs post-`commit`

Found before writing any code, by reading `semantic/src/lib.rs` and `ledger/src/fact_ledger.rs`/
`lib.rs` directly rather than assuming: `merge_graphs`/`build_ckm` never dedupe or merge
`KirObject`s that share an id across two different passes' artifacts — each stays a fully separate
entry, and `commit.rs` calls `ledger.append_object()` once per entry in iteration order. The ledger
itself *is* versioned by full content (`append_object` records a new version and repoints
`current_objects` whenever the payload differs) — but each version is a complete snapshot, not a
patch. So a naive `LlmDescriptionPass` that emitted a bare `KirObject { id: <same as the real
module>, properties: {"ai_overview": ...} }` (missing `name`/`kind`/every real structural property
RFC 0087 and the analyzers already wrote) would, if it ever got appended after the real object in
the same `commit` run, become the new "current" version — silently regressing the real, already-
compiled object down to just the new property. `architecture_reasoning.rs` never hits this because
it deliberately creates a *separate* `Custom("Claim")` object referencing its subject by id, rather
than writing onto the subject directly — the same reason applies here, but this RFC's design
explicitly wants the properties **on** the real object, not a separate Claim, so the fix is at the
write site instead: **this pass reads the real current full object from the ledger
(`store.all_objects()`), clones it, adds the new `ai_*` properties to that clone, and re-appends the
clone** — never a bare partial object. This also settles this RFC's own "exact CLI trigger" open
question: since it needs the fully-committed ledger (not just the in-memory CKM `compile` builds),
it can't run as a `CompilerPass` inside `compile` at all — it runs as a **post-`commit` step**, the
same architectural slot `commit_rollups`/`commit_data_lineage` (`commit.rs`) already occupy for
"needs the full committed graph, appends new versions/objects directly through `&dyn
KnowledgeStore`." `ekos-recovery` gains a new `ekos-ledger` dependency for this (checked first: no
existing dependency runs the other direction, so this doesn't create a cycle).

## New requirement: symbol source spans

Every analyzer (`rust_analyzer.rs`, `python_analyzer.rs`, `elixir_analyzer.rs`,
`javascript_analyzer.rs`) must additionally record a real `source_span` property
(`{"start_line": N, "end_line": M}`, 1-indexed, matching each file's own existing line-numbering
convention) on every `Symbol` object at extraction time — confirmed by direct code search before
writing this RFC: no analyzer persists this today (`line_idx` exists only as a transient loop
variable in `elixir_analyzer.rs`, never written onto an object). Without it, `LlmDescriptionPass`
has no way to slice the real function body out of the artifact store's already-stored raw source
text for symbol-level prompts — module-level prompts don't need this (their real "usage" comes from
the already-compiled dependency graph), which is why RFC 0087 never needed it either.

Per-language real source: `syn`'s own `Spanned` trait already gives real line numbers for every
parsed item (Rust) and real `rustpython-parser` AST nodes carry a real line range (Python) — both
free, no new parsing. `elixir_analyzer.rs`'s existing block-depth stack already tracks exactly when
a `def`/`defp`'s enclosing block opens and closes — recording the line index at push and pop for
that stack entry is a small, bounded addition, not a redesign. `oxc_parser`'s spans are byte
offsets, not line numbers — `javascript_analyzer.rs` needs a real byte-offset-to-line-number
conversion (a small, one-time, deterministic pass over the source, same shape RFC 0087's own
`doc_anchor` byte-offset handling already established for this exact parser).

## Design

### New object-level properties

Reuses the existing `KirObject.properties` map — no schema migration, no new `ObjectKind`. Written
only by this pass, distinct from RFC 0087's `description` (real, extracted from a comment,
possibly absent) so the two are never confused:

- `ai_overview` (string) — for a `Module`/`Rollup`: 2-4 sentence summary grounded in its real
  compiled structure (symbols it contains, what it depends on/is depended on by). For a `Symbol`:
  2-4 sentences grounded in the **real function body text** sliced via `source_span` — what the
  function actually does, not what its name or an existing comment merely claims.
- `ai_usage` (string) — how the object is actually used, grounded in its real incoming
  `DependsOn`/`Calls`/`Contains` edges (RFC 0041's real `Calls` graph gives this real signal for
  Rust symbols specifically; other languages fall back to module-level `DependsOn`, honestly, since
  no cross-language call graph exists yet).
- `ai_comment_check` (string, symbols only, present only when RFC 0087's `description` property
  exists on the same object) — one of `"consistent"` (the real code matches what the existing
  comment claims), `"stale"` (the LLM found a real, specific discrepancy — cited in `ai_overview`),
  or `"incomplete"` (the comment is accurate as far as it goes but omits real behavior the code
  has). This is the concrete, queryable answer to the user's "comments may be inactual or improper"
  concern — never silently trusting an existing comment, and never silently overwriting/deleting
  it either: RFC 0087's `description` property is never modified by this pass, only supplemented.
- `ai_evidence` (array of KirId strings) — which real compiled objects/relationships (and, for a
  symbol, its own real `source_span`-sliced source) the LLM was shown and is expected to ground its
  answer in — the same citation discipline `ekos ask`'s grounding pipeline and `--prose`'s
  `ProseSection.cited_evidence` already enforce. A response citing an id it wasn't shown, or citing
  nothing at all, is rejected and the object keeps no `ai_*` properties at all rather than an
  ungrounded one.

`docs-gen` renders `ai_overview`/`ai_usage`/`ai_comment_check` in a new "## AI-Assisted Overview"
subsection, after the real `## Definition`/`## Properties` sections, never merged into or replacing
them — a reader must always be able to tell compiled-real-evidence text from LLM-synthesized text
at a glance. A non-`"consistent"` `ai_comment_check` renders as a visible callout (e.g. "⚠ possibly
stale — see AI-Assisted Overview") right on the `## Definition` section itself, since that's exactly
the moment a reader is about to trust the wrong thing.

### New `CompilerPass`: `LlmDescriptionPass`

Runs after `semantic` (needs the fully-linked CKM for real cross-file `DependsOn`/`Calls` edges),
config-gated:

```toml
[llm_description]
enabled = false          # off by default — real LLM spend, same as [architecture_reasoning]
scope = "modules"        # "modules" | "symbols" | "all" — see Cost below for why the default
                          # stays the cheaper option even though "symbols"/"all" are now fully
                          # in scope and implemented, not rejected
```

Iterates every in-scope object already in the compiled graph, skips any that already carry an
`ai_overview` whose input evidence is unchanged (cache key = the object's own id + a stable hash of
its real neighbor ids **and**, for a symbol, its own `source_span`-sliced source text — so an edit
to a function's body correctly invalidates its cached `ai_overview` even if nothing structural
changed), and calls the configured `LlmProvider` (wrapped in `CachedLlmProvider`,
`.ekos/llm-cache/`, exactly like every other LLM-backed pass in this repo) once per remaining
object.

### Project-level Purpose / Architecture style

Unchanged from this RFC's original draft: one additional, single LLM call per `commit` run (not
per-object) — real input: the workspace's own README (RFC 0023), the top 10 largest `Rollup`s, and
the Technology Inventory. Writes `purpose`/`architecture_style` (each with real `evidence`) onto one
synthetic `Custom("ProjectSummary")` object. `Architecture.md`'s `## Architecture Summary` reads it
when present, keeps today's honest "_not yet computed_" text otherwise.

### Cost gating — revised numbers now that symbols are in scope

The real analytics backend-only ledger (this session's own compiled numbers): 908 modules + 5
rollups + 1 project-level call = **914 calls** at `scope = "modules"` (unchanged default); adding
4,250 real symbols brings `scope = "all"` to **5,164 calls** on this one project alone — a ~5.6x
jump, with larger per-call prompts too (real function body text, not just structural metadata).
`ekos compile` (exact CLI trigger still an Open Question, see below) must show this real estimate,
broken down by scope tier, and require confirm-or-`--yes` before the first call — `[architecture_reasoning]`/`[document_semantics]` are already `enabled = false` by default in this
codebase for the equivalent reason; `[llm_description]`'s own default of `scope = "modules"` (never
defaulting straight to `"all"`) exists specifically so opting in once doesn't silently commit a
user to the 5.6x-larger real spend without a second, explicit `scope = "all"` choice.

## Alternatives considered

- **A single flat scope with no "modules"/"symbols" tiers** — rejected: the ~5.6x real cost gap
  between them is too large to collapse into one on/off switch; a user who wants module-level
  overviews (the architecturally highest-value signal per real dollar spent) shouldn't be forced
  into the symbol-level spend to get them.
- **Running at `docs generate --prose` time instead of `commit`** — rejected: the user's explicit
  ask was ledger persistence ("save to ledger"), and a compiled property is queryable through `ekos
  ekl`/`ekos ask`/MCP the same as any other compiled knowledge, which a render-time-only
  `ProseSection` never is.
- **Letting the LLM rewrite/replace a stale `description` property directly** — rejected: RFC
  0087's `description` is real extracted-from-source text: rewriting it with LLM output would erase
  a real fact (the comment's own literal words) in favor of a judgment call, and would make a
  future "recheck against source" audit impossible (nothing left to compare against). `ai_overview`
  + `ai_comment_check` supplement, never overwrite.

## Testing (planned)

- `LlmDescriptionPass` unit tests via `MockLlmProvider`: a module with real neighbors gets a
  grounded `ai_overview`/`ai_usage`/`ai_evidence`; a symbol's prompt contains its real
  `source_span`-sliced body text, not just its name/kind; a response citing an id never shown is
  rejected; an unchanged neighbor-hash (module) / unchanged source-slice-hash (symbol) skips the
  LLM call on a second run (mock call-count assertion); a symbol with an existing RFC 0087
  `description` gets a real `ai_comment_check` value, one with none doesn't get that property at
  all (present-only-when-applicable, matching this whole codebase's absence convention).
- Per-analyzer `source_span` tests (one per language): a real multi-line function's `start_line`/
  `end_line` matches the real source; a one-line function has `start_line == end_line`.
- `docs-gen` render tests: the new subsection and the stale-comment callout, each present/absent
  correctly.
- Cost-gate test mirroring `confirm_prose_spend_auto_skips_the_prompt`, extended to show the
  modules-vs-all cost delta.
- Live verification (once implemented): re-run against the real analytics backend-only ledger at
  `scope = "modules"` first (real cost bounded, ~914 calls), confirm grounded output on at least one
  previously-empty module page; a small, deliberately chosen `scope = "all"` subset run (not the
  full 4,250 symbols — a `--only-dirs`-style narrowing, matching `ArchitectureReasoningPass::
  with_only_dirs`'s already-existing pattern) to confirm real symbol-level grounding and at least
  one real `ai_comment_check: "stale"` or `"incomplete"` finding against real, messy source.

## Open Questions

- Whether `ai_overview` should also flow into `--prose`'s existing "## Overview" section or stay a
  visually distinct "## AI-Assisted Overview" — this RFC assumes the latter.
- Symbol-level `ai_usage` quality for non-Rust languages, honestly: RFC 0041's `Calls` graph is
  Rust-only today, so a Python/Elixir/JS symbol's `ai_usage` can only ever be grounded in
  module-level `DependsOn` edges, not a real per-function call graph — a real, accepted quality gap
  for this RFC's scope, not silently smoothed over; `ai_usage`'s own prompt should say so explicitly
  when falling back, the same "explicit about what it can't back with evidence" discipline RFC
  0068's Architecture Summary already established.

## Revision history

- **2026-08-23 (original):** Phase 1 = modules/subsystems only; per-symbol generation explicitly
  deferred to "a later RFC" as Phase 2.
- **2026-08-23 (this revision):** per-symbol scope folded into this RFC's own implementation scope
  at the user's explicit request — no longer deferred. Added the `source_span` requirement (a real
  prerequisite this fold-in exposes: symbol-level grounding needs real source text, module-level
  didn't), the `ai_comment_check` property (the concrete answer to "comments may be inactual"), and
  revised cost numbers (~914 calls at the still-default `scope = "modules"`, ~5,164 at
  `scope = "all"` against the real analytics backend).

## Files Changed (planned, not yet implemented)

| File | Change |
|---|---|
| `ekos/docs/rfcs/0088-llm-backed-compile-time-descriptions.md` | This RFC |
| `ekos/crates/recovery/src/llm_description.rs` (new) | `describe_objects` (post-`commit` step, not a `CompilerPass`), module/rollup/symbol prompt construction, evidence-citation validation, `ai_comment_check` logic — reads full objects from `&dyn KnowledgeStore`, clones + extends, re-appends |
| `ekos/crates/recovery/Cargo.toml` | New `ekos-ledger` dependency (confirmed no cycle) |
| `ekos/crates/recovery/src/rust_analyzer.rs` | `source_span` property on every `RustSymbol` via `syn`'s `Spanned` (needs `proc-macro2/span-locations`) |
| `ekos/crates/recovery/src/python_analyzer.rs` | `source_span` property via the AST's own line range |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `source_span` property via the existing block-depth stack's push/pop lines |
| `ekos/crates/recovery/src/javascript_analyzer.rs` | `source_span` property via a byte-offset→line-number conversion over `oxc_parser`'s spans |
| `ekos/crates/compiler-core/src/config.rs` | `LlmDescriptionConfig { enabled: bool, scope: Scope }` (`Scope::Modules \| Symbols \| All`) |
| `ekos/crates/docs-gen/src/lib.rs` | New "## AI-Assisted Overview" subsection; stale-comment callout; `Architecture.md`'s `## Architecture Summary` reads `ProjectSummary.purpose`/`architecture_style` when present |
| `ekos/crates/cli/src/commands/commit.rs` | Cost-estimate (modules-vs-all breakdown) + confirm gate, calling `describe_objects` after `commit_data_lineage` when `[llm_description].enabled` |
