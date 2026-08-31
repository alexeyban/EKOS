# Devlog 138 — Evidence citations leaked absolute filesystem paths

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

A real, previously-unrooted bug from TODO.md: rehearsing the RFC 0045 demo end-to-end, `ekos ask`
citations for `fd` (a real external Rust project baked into a demo workspace) rendered a full
absolute scratch path (`/tmp/claude-.../demo-repo-spike/fd/./src/error.rs`) instead of a clean
repo-relative one (`src/error.rs`), while EKOS-self's own citations looked fine. This session
root-caused it, fixed it, and reproduced the exact before/after behavior live against a real
workspace rather than trusting the fix by inspection alone.

---

## PR — fix absolute-path leakage in File-object evidence

### Problem / motivation

TODO.md had this flagged as "not yet root-caused — only confirmed as a real, visible, reproducible
symptom," with a theory but no located call site. The theory (`KirEvidence`/`SourceLocation.path`
populated from an absolute path somewhere) was directionally right but needed a real repro to
confirm rather than more reading.

### What was found

`crates/cli/src/commands/build.rs`'s per-file `File`-object construction builds one `KirEvidence`
per observed file. It computed:

```rust
let abs_path = base.join(rel_str);
let mut ev = KirEvidence::new(
    SourceLocation::file(abs_path.to_string_lossy().as_ref()),
    format!("file: {rel_str} ({size} bytes)"),
);
```

`abs_path` — an absolute filesystem path — went straight into the evidence's `SourceLocation`,
while every other evidence-producing analyzer in the codebase (`local_docs_analyzer.rs`,
`llm_description.rs`'s symbol-description pass, etc.) already used the plain relative path. The
comment directly above this code even said the relative path "stays the plain within-project path
... for `content.target`/display" — the code just didn't follow its own stated intent for this one
field.

**Why EKOS-self's citations looked clean while `fd`'s didn't, despite the bug applying equally to
both:** `local_docs_analyzer.rs` reprocesses Markdown/PDF/DOCX/HTML/email files and produces its
*own* evidence with the correct relative path. EKOS-self's rehearsal happened to cite Markdown
files (`TODO.md`), which are covered by that separate, always-correct pass. `fd` is a Rust
codebase; a `.rs` source file has no equivalent reprocessing pass, so the *only* evidence it had
was the base observer's absolute-path one — the bug was never masked there.

### Live reproduction, not just static reading

Built a tiny two-file Rust project in a deeply-nested scratch tempdir (the real repro shape — a
workspace that isn't the process's own repo root), ran the real `build`/`recover`/`resolve`/
`compile`/`commit` pipeline through the actual `ekos` binary, then queried the File object's
evidence through a real `ekos mcp serve` session (`ekos_state`). Before the fix:

```json
"location": { "path": ".../scratchpad/demo-repo-spike/tinyproj/./src/error.rs" }
```

— reproducing the reported bug down to the literal `/./` (from `base` itself carrying a `.`
path component). After the one-line fix, the identical setup produced:

```json
"location": { "path": "src/error.rs" }
```

### Fix

One field: `SourceLocation::file(rel_str.as_str())` instead of `SourceLocation::file(abs_path...)`.
`abs_path` had no other use in that function — removed entirely, not left as dead code.

### Testing

New `file_object_evidence_location_is_relative_not_absolute` (`build.rs`) builds a workspace in a
deeply-nested `tempdir()` (so a trivial "cwd == repo root" case can't accidentally pass) and
asserts the resulting File object's evidence location equals the plain relative path, doesn't start
with `/`, and doesn't contain the workspace's own absolute root as a substring — the same shape of
assertion that would have caught this bug before it shipped.

---

## Knowledge Captured

- When one bug's symptom is masked in one common case (EKOS-self's mostly-Markdown citations) and
  visible in another (a plain Rust codebase), check whether a *different* code path happens to
  cover the masked case correctly, rather than concluding the two cases differ in root cause. Here
  `local_docs_analyzer`'s separate, correct evidence for Markdown/PDF/etc. was the entire reason
  the bug looked workspace-dependent when it was actually universal.
- A comment describing the *intent* of a variable ("stays the plain relative path for X, Y, Z
  below") is not proof the code following it actually does that — this bug was exactly a case
  where the code diverged from its own adjacent comment. Worth treating such comments as a
  claim to verify against the actual line, not a substitute for reading the line.
- Reproducing a reported bug live, in a real nested-tempdir workspace through the real binary and a
  real `ekos mcp serve` session, found the *exact* reported artifact (the literal `/./` in the
  path) — a much stronger confirmation than fixing from static reading alone, and cheap to do here
  since the repro was three files and five CLI invocations.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/build.rs` | `File`-object evidence now uses the relative path, not `base.join(rel_str)`; new regression test |
| `TODO.md` | Marked the citation-path item fixed, with root cause and verification recorded |
