# Devlog 251 — Freshness: an opt-in content hash, and disputes in eval runs

**Date:** 2026-10-11
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Two small open items. Source freshness (RFC 0171) counted a file as changed whenever its modification time moved, so a fresh checkout or a `touch` made the whole tree read as changed. `[freshness] content-hash = true` now records a content hash at build and uses it to tell a touch from an edit. And `ekos eval run` passes the same dispute lookup as `ekos ask` (RFC 0173), so graded answers are produced the way real ones are.

---

## RFC 0171 amendment — content hash

### What was built

| Component | |
|---|---|
| `FreshnessConfig.content_hash` | `[freshness] content-hash`, default `false` |
| `Stamp.hash` | SHA-256 of the file, absent unless recorded |
| `freshness::add_hashes` | called by `ekos build` when the option is on; skips RFC 0043-excluded paths |
| `freshness::diff_with` | asks for a file's current hash only when the size is equal, the time moved and the older stamp has a hash |
| `RULE_HASHED` | the rule printed by `ekos freshness` and carried in `ekos_status` when the committed manifest has hashes |

### Implementation details worth remembering

- **A check stays cheap.** It walks metadata as before and reads only the files that are candidates for "touched only". On an unchanged tree it reads nothing.
- **The build fingerprint is untouched.** The hash is added to the manifest after `manifest_from`; `fingerprint_of` never sees it, so the test that pins the fingerprint still passes and no workspace rebuilds because of this.
- **Excluded files are never read.** `add_hashes` uses the same `is_excluded_path` as `ekos build`. Such a file keeps the size-and-time rule.
- **Turning the option on needs one `ekos build` … `ekos commit`.** Until then the committed manifest has no hashes and the old rule applies, which the printed rule shows.
- **`Stamp` is no longer `Copy`.**

### Verified on a real workspace

With the option on, after a full pipeline: `touch` on two observed files left `ekos freshness` at `fresh`; appending a line to one reported `1 changed` with the objects citing it.

A bug found doing this: `ekos freshness` printed the constant rule rather than the report's own, so it would have shown the wrong rule under hashing. Fixed.

### Decisions

- **Opt-in.** Hashing reads every observed file once more at build. RFC 0171's default promise, that freshness reads no file contents, stays true unless the user asks otherwise.
- **Full SHA-256 in the manifest, not in the ledger.** The manifest is a local sidecar under `.ekos/`, readable only by someone who can already read the files.

---

## RFC 0173 — eval runs

`commands/eval.rs` now calls `AiRuntime::with_disputes`, as `ask.rs` does. The eval corpora have no conflicts, so their prompts are unchanged; the point is that the graded path and the real path stay the same code.

---

## Knowledge Captured

- **`[skip ci]` also skips the Pages deploy.** A push whose head commit carries it does not redeploy `docs/generated/ekos-self-documentation.html`; run `gh workflow run pages.yml --ref main` after pushing.
- **Simulate a `touch` in a test by editing the recorded stamp**, not by rewriting the file and hoping the clock moved.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/compiler-core/src/config.rs` | `content_hash` |
| `ekos/crates/cli/src/freshness.rs` | `Stamp.hash`, `add_hashes`, `diff_with`, `RULE_HASHED`, 2 tests |
| `ekos/crates/cli/src/commands/{build,freshness,eval}.rs` | hash at build; print the report's rule; dispute lookup in eval |
| `ekos/docs/rfcs/0171-source-freshness.md` | amendment |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | documented; TODO items ticked |
