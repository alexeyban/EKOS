# Devlog 109 — RFC 0093: `Technology`/`JsModule` cross-kind conflict false positive

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Sixth item on the gap-closure list: cross-kind identity conflicts (`react`/`vite` etc. as both
`Technology` and `JsModule`), previously left as "a broader identity-design question for a future
RFC" (`devlog_102`). Filed and implemented RFC 0093: a narrow, precise conflict-detection exclusion
for exactly this pair, not a merge and not a blanket suppression. Live-verified against
`pdf-reader`'s real ledger: `ekos resolve` (no `--force`) now succeeds cleanly for the first time
all session on this project — conflict count dropped from 5 to 0.

## The design question, resolved

`package_json_analyzer.rs` compiles a `Technology` object per declared `package.json` dependency;
`javascript_analyzer.rs` compiles a `JsModule` object per real `import` specifier. Both are real,
but `DefaultResolver`'s cross-kind conflict detector flags any exact same-name match across
different kinds as a `[CONFLICT]` — and `ekos resolve` (no `--force`) refuses to proceed at all
when any conflict exists. Since a real dependency that's both declared *and* imported is the
expected shape for every real JS/TS project, this fires on effectively every one of them — not an
edge case, systemic noise that trains users to reach for `--force` reflexively.

Considered and rejected: merging `Technology`/`JsModule` by name (RFC 0026 `Concept`-style).
Rejected because `JsModule` isn't exclusively "external npm package" — `handle_import` creates one
for *every* import specifier equally, including real relative/local imports (`./api/client`) with
no `Technology` counterpart and no real "same entity" relationship to one. Merging by name alone
would risk conflating a local file with an unrelated npm package sharing its bare name.

The fix instead narrows exactly what stops being *flagged* (not merged): a name group whose kinds
are **exactly** `{Technology, JsModule}` — a third kind mixed in still conflicts — **and** every
`JsModule` in the group looks like a real bare package specifier (doesn't start with `.`, `..`, or
`/`, the same syntactic rule Node's own module resolution already uses). `react`/`@vitejs/plugin-react`
pass; `./utils` does not, and still correctly conflicts against an unrelated same-named `Technology`
— the collision this exclusion must not silently hide. Both objects stay real, distinct, unmerged;
this only stops them from being *reported* as an ambiguity.

Also considered: routing these through `cross_system.rs`'s reviewable-candidate mechanism (RFC
0029/0063) instead of excluding them outright. Rejected — that pattern fits genuine uncertainty
worth a judgment call; this isn't uncertain, it's the definitionally-expected shape for any real
dependency both declared and imported. Routing all of them through review would just move the noise
rather than remove it.

## Implementation

`crates/identity/src/lib.rs`: new `is_expected_technology_jsmodule_pair`, checked before a
same-name-different-kind group is turned into a `ConflictReport`. 3 new tests: the real `react`
shape no longer conflicts; a third mixed-in kind still conflicts; a relative-specifier `JsModule`
sharing a `Technology`'s name still conflicts.

## Live verification

Against `pdf-reader`'s real whole-project ledger: `ekos resolve` (no `--force`) now reports
`Conflicts detected: 0` (was 5 — `react`/`vite`/`react-router-dom`/`pdfjs-dist`/
`@vitejs/plugin-react`) and exits 0 without requiring `--force`, for the first time all session on
this project. `compile`/`commit` unaffected in object/relationship count (148/192, unchanged) —
this fix only changes what gets *reported* as a conflict, not what gets merged or compiled.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups, 70/70 in `ekos-identity` specifically).
`tests/integration` 3/3.

## Knowledge Captured

- **A safety check that fires on the expected, common case trains users away from paying attention
  to it** — the concrete, measurable cost this session: 5 identity conflicts, on a completely
  ordinary JS/TS project, that had nothing wrong with them, requiring `--force` on every single
  `resolve` run all session. A conflict-detection mechanism is only as useful as its actual
  precision at distinguishing "genuinely worth a human's attention" from "structurally, always
  going to happen for any project shaped this way" — the fix here is not "make the check less
  strict" but "make it check the one specific thing that actually distinguishes the two cases"
  (bare-specifier vs. relative-path `JsModule` name), which is a real, mechanical, zero-judgment-call
  distinction already available in the data.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0093-technology-jsmodule-conflict-exclusion.md` | New RFC, Accepted |
| `ekos/crates/identity/src/lib.rs` | New `is_expected_technology_jsmodule_pair` conflict-detection exclusion; 3 new tests |
| `pdf-reader/.ekos/` (external project) | Re-resolved/compiled/committed against the fix (no ledger content change — only conflict reporting changed) |
