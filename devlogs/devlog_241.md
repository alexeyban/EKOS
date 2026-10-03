# Devlog 241 — `ekos resolve` no longer stops on cross-namespace homonyms

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Observing LedgerSMB's Perl (`lib/`) beside its SQL made `ekos resolve` refuse to proceed with **16
identity conflicts, every one Perl beside SQL** — subs wrapping the stored procedure they call
(`asset__save`), subs and packages named after tables (`payment`, `workflow`). With `UI/` observed,
JavaScript beside Perl (`initialize`) joins them; TODO.md had recorded the problem with `--force`
(which hides every conflict) as the only way through. Identity now treats a same-name group spread
over separate namespaces as expected: 16 → **0** conflicts with `sql/`, `lib/` and `UI/` observed,
and the same 193 merge proposals.

---

## What was built

| Component | Change |
|---|---|
| `identity/src/lib.rs` | `is_expected_cross_namespace_group`, wired into conflict detection. 3 tests; 3 existing tests now use `Technology` as their unrelated kind |
| `ekos/docs/rfcs/0147-perl-connector.md` | Amendment (b) |

## The rule

Namespaces: the **database** (`Table`, `View`, `Procedure`, `Trigger`) and each language's **code**
(Perl, JavaScript, Python, Rust, Elixir module/symbol kinds). A group is expected when it spans at
least two namespaces and each namespace's share is unremarkable on its own — the database side only
tables or only views/routines/triggers (so `Table` beside `View` still conflicts), the Perl side any
mix of package and sub, any other language a single kind. A kind outside these namespaces keeps the
group a conflict.

## Decisions

- **Generalised past what was observed** (JS beside Perl was observed, Python/Rust/Elixir were not):
  the namespace argument is the same for every language, and narrowing language by language would
  repeat this fix each time a polyglot repo is observed. The within-namespace checks keep it from
  hiding anything a single namespace would flag.
- **One earlier decision reversed**: a `PerlPackage` beside a same-named `Table` used to conflict.
  It is the same code-versus-database co-existence (`workflow`), so it no longer does.

---

## Knowledge Captured

- **A conflict detector that fails by default trains users to pass `--force`**, which hides real
  conflicts with the false ones. Every narrowing so far (RFC 0093, 0147, 0148, 0169, 0163) and this
  one was found the same way: on a real repository, the first time a new language or object kind
  was observed beside the others.

---

## Verification

- LedgerSMB clone, `sql/` + `lib/` + `UI/` (118 JS files) observed: `resolve` 0 conflicts (was 16
  without `UI/`); `initialize` present as both `JsSymbol` and `PerlSymbol`; compile/commit proceed.
- `cargo test -p ekos-identity` (95), workspace tests, clippy, fmt.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/identity/src/lib.rs` | Cross-namespace narrowing + tests |
| `ekos/docs/rfcs/0147-perl-connector.md` | Amendment (b) |
| `TODO.md`, `CLAUDE.md` | Item closed; identity note |
