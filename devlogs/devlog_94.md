# Devlog 94 — Real project_key gap found testing RFC 0088 against a real single-non-dot-path workspace

**Date:** 2026-08-23
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Asked to regenerate a small piece of backend architecture again to test the changes just committed
(RFC 0088, the multi-alias fix, the non-Rust Component View/decomposition fixes). Scoped a real
test to a genuinely small real subsystem (`lib/plausible/auth`, 15 files, 116 symbols) in the
analytics project rather than its full 900-module backend, so a real local Ollama model could
complete the run in minutes. That real test — deliberately shaped as a single `[observe] paths`
entry that isn't `"."` — immediately surfaced a real, deeper bug than devlog_93's `real_file_path`
fix: `build.rs`'s own RFC 0079 `project_key` logic silently drops a real directory prefix whenever
a workspace has exactly one non-`"."` observe path, with no property left anywhere to reconstruct
it. Fixed at the actual source (`build.rs`'s `project_key` condition), not worked around in RFC
0088's own code a second time. Re-verified end-to-end: 104/116 real symbols and all 27 real
modules/subsystems correctly described, 0 errors.

## The bug

First real run against the small `lib/plausible/auth` scope reported `AI descriptions: 27
module(s), 0 symbol(s) described (0 cached, 116 skipped without a source span, 0 errors)` — despite
the compiled CKM genuinely having real `source_span` data on 104 of 116 real symbols (checked
directly). Root cause, one layer deeper than devlog_93's fix: `File.name` for this scope was
`"password.ex"`, not the real `"lib/plausible/auth/password.ex"` — `build.rs`'s own `project_key`
(RFC 0079, the mechanism that's supposed to carry a dropped directory prefix back as a real
`"project"` property) was empty, because its condition was `observe_paths.len() > 1`, and this
scope has exactly *one* `[observe] paths` entry. That condition conflates two genuinely different
cases: "the truly common `paths = ["."]` config, where the directory prefix is empty anyway" and "a
single scoped subdirectory (`paths = ["lib/plausible/auth"]`, or this repo's own test fixture's
`paths = ["src"]`), where a real, non-empty prefix exists but was never captured." The real analytics
backend-only config avoided this specific failure mode only because it lists *eight* separate
observe path entries — the exact single-entry, non-dot shape is what a smaller, more targeted scope
(exactly what this task asked for) exercises.

## The fix

`base != cwd` replaces `observe_paths.len() > 1` as the condition for writing `project_key`. This is
strictly more precise, not just different: for the truly common `paths = ["."]` case, `base == cwd`
always, so the condition still evaluates false and `project_key` stays empty — byte-identical ids,
no migration, exactly the guarantee the original code's own comment promised. For a single non-`"."`
entry, `base != cwd` now correctly evaluates true and captures the real prefix. `project_key` is the
single choke point every path-keyed recovery pass already reads back via `data.project` (RFC 0079),
so the fix propagates correctly to `elixir_analyzer.rs`/`rust_analyzer.rs`/`python_analyzer.rs`/
`git_analyzer.rs`/`local_docs_analyzer.rs` and to `llm_description.rs`'s own `real_file_path` with
no further changes needed anywhere else.

One existing test asserted the old behavior directly (`build_single_project_workspace_has_no_project_property`, using this repo's own `setup_workspace` fixture — `paths = ["src"]`, a single non-dot
entry, exactly the shape that was wrong). Its own doc comment claimed to test `paths = ["."]` but
never actually did — split into two correctly-scoped tests:
`build_single_dot_path_workspace_has_no_project_property` (a real inline `paths = ["."]` fixture,
confirms the still-correct no-migration guarantee) and
`build_single_non_dot_path_workspace_gets_a_real_project_property` (the existing `setup_workspace`
fixture, now asserting the corrected `project = "src"` behavior). Full workspace gate clean
afterward, including `tests/integration`.

## Live re-verification

Same real `lib/plausible/auth` scope, fresh ledger, same real local `llama3:latest`:
`AI descriptions: 27 module(s), 104 symbol(s) described (0 cached, 12 skipped without a source
span, 0 errors)` — the 12 real remaining skips are genuine (functions with no real block to span,
matching `elixir_analyzer.rs`'s own already-documented one-line `, do:` limitation), not a bug.
`Plausible.Auth.Password`'s real page now carries a real, accurate AI-Assisted Overview ("responsible
for performing password-related calculations and checks... hashing, and matching passwords") while
devlog_90's identity-resolution fix still holds (exactly 3 real `Contains` relationships, no phantom
edges). `hash`'s own symbol page correctly identifies "using the Bcrypt library" — read from the
real source text via the now-correctly-reconstructed file path, not guessed.

## Knowledge Captured

- **Counting `[observe] paths` entries is not the same test as "does this path need a prefix
  restored."** The two only coincide for the specific common case (`paths = ["."]` as the sole
  entry) the original code was written to protect; any other single-entry shape silently fails the
  same way multi-entry configs would have without RFC 0079 at all. Comparing the resolved path
  against `cwd` directly is the correct, general test.
- **A scoped-down test (exactly what "generate a small piece" asked for) found a real bug the full
  backend-only config's own 8-entry shape had never triggered** — matches this whole project's
  repeated live-verification lesson (devlog_90/91/92/93) from a new angle: smaller, more targeted
  real scopes exercise different real code paths than the biggest available one, not just faster
  ones.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/build.rs` | `project_key` condition fixed from `observe_paths.len() > 1` to `base != cwd` |
| `ekos/crates/cli/tests/skeleton.rs` | Split the one test asserting the old behavior into two correctly-scoped tests |
| `devlogs/devlog_94.md` | This file |
