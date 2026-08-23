# Devlog 92 — Multi-alias phantom-module bug, Component View + Layer Breakdown for non-Rust workspaces, RFC 0088 filed

**Date:** 2026-08-23
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Asked to clean up and analyze the analytics project's backend, and produce a small architecture
document to test recently-shipped features. Scoped a new backend-only `ekos.toml` for that project
(`lib`/`extra/lib`/`priv`/`config`/`mix.exs`/`rel`, excluding `assets`/`tracker`/`e2e`/`fixture`),
ran the full pipeline, and generated `Architecture.md`. Reading the output surfaced a real,
previously-unnoticed `elixir_analyzer.rs` bug (a multi-target `alias X.{A, B}` form fabricating a
phantom module object for the shared prefix instead of real edges to each real leaf module) — fixed,
tested, live-verified. Separately, the user pointed out several real, honest gaps in the generated
`Architecture.md` (empty entity pages, no Purpose/Style/Risk/Confidence, no crate-based Component
View for a non-Rust project, decomposition too coarse) — two of those (Component View, decomposition
detail) were deterministic and fixed this session; the larger ask (LLM-generated descriptions
persisted to the ledger at compile time) is scoped into a new RFC 0088, not yet implemented pending
review.

## Bug — multi-alias `X.{A, B}` created a phantom shared-prefix module instead of real per-leaf edges

Live-verified against the real analytics backend: `plausibleweb-customersupport-team-components.md`
(an `ElixirModule` entity page) had no properties, no file, and almost no relationships — a
completely contentless page. Root cause: `PlausibleWeb.CustomerSupport.Team.Components` was never
`defmodule`'d anywhere in the real source — it only ever appears as a multi-target alias prefix
(`alias PlausibleWeb.CustomerSupport.Team.Components.{Sites, Billing, SSO, ...}`, wrapped one leaf
per line by `mix format`, a real, common shape — 74 real occurrences of this pattern across the
codebase). `extract_dependency_target`'s per-line scan (no lookahead) could only ever see the
shared prefix on the `alias X.{` line itself, so it created one `DependsOn` edge to a phantom `X`
object instead of real edges to the individual real leaf modules — exactly matching this file's own
doc comment, which already (accurately, honestly) documented this as a deliberate scope limitation,
not a bug, at RFC 0081's original design time.

Fixed with a new pre-scan (`prescan_multi_alias_targets`, same lookahead pattern
`extract_doc_comments` already established for multi-line heredocs): follows a multi-target alias
block — single-line or wrapped across several real lines — to its real closing `}`, expanding it
into one real `DependsOn` edge per real leaf (`X.A`, `X.B`, `X.C`) instead of one edge to the bare
shared prefix. The phantom prefix object is never created again; the real leaf modules (which
already exist via their own `defmodule`) now correctly gain the incoming dependency edge instead.
2 new tests (single-line and multi-line-wrapped forms), both asserting the phantom object is never
created. Live re-verified: `PlausibleWeb.Live.CustomerSupport.Team`'s entity page now lists all 7
real dependent modules by name; the phantom `.Components` page no longer exists in a fresh
regeneration.

## Component View + Layer Breakdown — real gaps for non-Rust workspaces, both fixed

The user flagged `## Component View` always saying "No crate directory matched a compiled
subsystem rollup" for the analytics project — technically honest (Elixir has no `Cargo.toml`, so
zero `Crate` objects ever compile) but useless, since RFC 0044's `Rollup` objects already give this
project real Container-level structure. `render_component_view` now falls back to listing real
compiled `Rollup`s directly, clearly labeled as a fallback ("showing each real compiled `Rollup`
... since 'crate' doesn't apply outside a Rust workspace"), whenever zero `Crate` objects exist but
real `Rollup`s do — live-verified: the analytics backend's Component View now lists all 5 real
rollups (`config`, `extra/lib`, `lib`, `priv`, `rel`) with real member-file counts and links.

Also added a new `### Layer Breakdown` subsection under `## System Decomposition` — the "detailed
view" the user asked for: which real `Rollup` contributes how many files to each Backend/Frontend/
Database layer, computed from each `Rollup`'s own real `Contains` edges cross-referenced against
each member's already-computed layer. A rollup with members in more than one real layer is honestly
listed under every layer it actually touches, not forced into one — live-verified this is a real
case, not just a hypothetical: `priv` (291 real backend files) also contains one real compiled
frontend asset (`priv/tracker/js/p.js`, a checked-in tracker build artifact), correctly listed under
both `**Backend:**` and `**Frontend:**`.

## RFC 0088 filed, not yet implemented — LLM-backed compile-time descriptions

The user's larger ask — every module (and eventually every function) gets a real LLM-generated
description/usage/links section, "despite exist or not exist comments in file," extracted at
compile time and persisted to the ledger rather than regenerated at every `docs generate` — is a
genuinely new compiler capability with real cost implications (≈914 real LLM calls for a first full
run against the analytics backend: 908 modules + 5 rollups + 1 project-level summary call). Per
CLAUDE.md's mandatory RFC-first workflow, drafted RFC 0088 rather than building directly; confirmed
with the user before writing it. Scope decided with the user: Phase 1 = module/subsystem-level only,
not every symbol (~914 calls, not ~5,000); the user's own further framing — that even an *existing*
doc comment can be stale/incorrect/incomplete, so a future phase needs the LLM checking every
function against real current code, not just filling gaps RFC 0087 left absent — is captured
verbatim in the RFC as an explicitly deferred Phase 2, not lost. Not implemented this session;
awaiting review.

## Knowledge Captured

- **A per-line scanner with no lookahead will silently give up real information on any multi-line
  source construct**, even when its own doc comment already, correctly, calls this out as a
  deliberate scope choice rather than an oversight — "deliberately narrow" and "actually the best
  fix is now cheap given the same lookahead pattern already exists elsewhere in this file" are not
  mutually exclusive; worth revisiting a documented scope limitation once a similar problem has
  already been solved once in the same file (here: `extract_doc_comments`'s heredoc lookahead).
- **A file-classification convention (`classify_path`) that looks complete can still have real
  gaps invisible until live data crosses them** — `priv/tracker/js/p.js` inside an otherwise-backend
  `priv/` directory is exactly the "mixed rollup" case the new Layer Breakdown was built to surface
  honestly rather than average away.
- Confirms an existing lesson rather than a new one: reading one real generated page directly (the
  empty `.Components` entity page) found a real bug six existing tests plus a full workspace gate
  had never caught, since every existing multi-alias test used a single-line form — the exact
  "live verification finds what fixtures can't" pattern devlog_90 and devlog_91 both already
  recorded this same session.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `prescan_multi_alias_targets` (new); multi-alias `DependsOn` expansion; 2 new tests |
| `ekos/crates/docs-gen/src/lib.rs` | `render_component_view` Rollup fallback for non-Rust workspaces; new `render_system_decomposition_detail` (`### Layer Breakdown`); 3 new tests, 2 existing tests updated |
| `ekos/docs/rfcs/0088-llm-backed-compile-time-descriptions.md` | New RFC — LLM-backed compile-time module/subsystem descriptions, not yet implemented |
| `/home/legion/PycharmProjects/analytics/ekos.toml` | New backend-only scoped workspace config (`lib`/`extra/lib`/`priv`/`config`/`mix.exs`/`rel`), `doc`/`docs-generated` in `ignore-patterns` |
| `devlogs/devlog_92.md` | This file |
