# Devlog 106 — the "`local_docs_analyzer.rs` id-collision" was never an id collision

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Third item on the gap-closure list, carried over from `devlog_102`'s open finding: "both real
`README.md` Document objects have zero path separators ... only one `Custom("Document")` object
named `"README.md"` survives in the ledger despite both files being real and processed — a real,
not-yet-diagnosed `local_docs_analyzer.rs` id-collision." Root-caused precisely: it was never an
id collision (`local_docs_analyzer.rs`'s RFC 0079-qualified UUIDv5 ids never actually collided,
confirmed by direct inspection) — it was the ninth occurrence of `DefaultResolver`'s missing
blanket kind-exclusion bug, the exact failure shape CLAUDE.md's own crate-map already names as an
explicit obligation ("New `ObjectKind::Custom(_)` variants ... must be added to `DefaultResolver`'s
blanket kind-exclusion list") and eight prior kinds already hit. `Custom("Document")` was simply
never added.

## Root cause

`crates/identity/src/lib.rs`'s `DefaultResolver` blocks candidates by `(kind, first-3-normalized-
chars)` and applies a same-kind structural-score fallback of 1.0 when there's no `columns`
property to differentiate on. A blanket exclusion list (`Section`, `TransformNode`, `RustSymbol`,
`RustModule`, `PythonSymbol`, `PythonModule`, `Crate`, `Claim`, `ArchitectureGap`, `ElixirModule`,
`ElixirSymbol`, `JsModule`, `JsSymbol`) already exists for kinds where each object is
self-identified by a structural key and no two distinct instances can legitimately be the same
real entity — `Document` fits that description exactly (self-identified by its own RFC
0079-qualified path) but was missing from the list.

`pdf-reader`'s real project root `README.md` and `frontend`'s unmodified Vite scaffold
`README.md` both compile to a `Document` object literally named `"README.md"` — not a shared
prefix, an *exact* name match — so the same-kind 1.0 structural fallback pushed them to confidence
1.00, which RFC 0063 auto-merges without review (only fuzzy, non-exact matches go to the review
queue). `ekos resolve` reported this as `'README.md' (Document) — 2 objects, confidence 1.00 —
auto-merge`, and `ekos compile` (which re-runs `DefaultResolver` internally to actually apply
exact-match merges, per RFC 0063) silently dropped one of the two real files from the compiled
CKM.

## The fix

Added `"Document"` to the blanket kind-exclusion list, with a comment following the same pattern
every prior instance in this list already uses (why the kind qualifies, how it was found). One new
regression test (`document_objects_are_never_merged_even_with_identical_names`, following the
established per-kind-exclusion test pattern). Two pre-existing tests had used `Custom("Document")`
as their example of a kind the exclusion *doesn't* apply to — both switched to `Custom("Page")`
(a kind genuinely outside every exclusion), since their actual point (a non-excluded kind still
merges normally / RFC 0060's exact-match flag still applies to a kind that *does* reach
comparison) no longer holds for `Document` once it's excluded.

## Live verification, including a real methodological trap caught along the way

First check: `ekos resolve --force` against `pdf-reader`'s already-built `.ekos/` (rebuilt for
`devlog_105`) went from 3 merge proposals (including the bad `README.md` auto-merge) to 2, with no
`README.md` proposal — before vs. after rebuilding only the `ekos` binary, no ledger rebuild.

That check alone turned out to be **necessary but not sufficient**: `ekos compile`'s
`semantic-compiler` pass caches on its declared `cache_inputs` (the upstream `recover`-stage
artifact ids), which hadn't changed, so it logged `skipping pass (cached)` and silently reused the
*old*, pre-fix compiled CKM — `ekos commit` then reported the same stale object count both before
and after the code fix, which would have been reported here as confirmed live verification had the
object count not been double-checked against what the fix should actually produce. `ekos clean`
(clears `.ekos/artifacts/`, the documented way to bust this cache) turned out to leave a *second*,
separate inconsistency: `build`'s own re-scan-skip fingerprint file isn't stored under
`artifact_dir` and survived the clean, so the next `build` trusted a stale "nothing changed"
fingerprint against a now-empty artifact store — `recover` then ran only 2 of 6 passes against an
effectively empty input set. The reliable fix was the full `rm -rf .ekos` + `init` cycle already
established earlier this session, not `ekos clean`.

Rebuilt `pdf-reader`'s `.ekos/` fully fresh (`init`/`build`/`recover`/`resolve --force`/`compile`/
`commit`): `ekos compile` now reports **148** objects, one more than the pre-fix 147 — the
previously-collapsed second `README.md`. `ekos query find "README.md"` returns 4 `README.md`-named
results (2 `Document` + 2 `File` — a `File` object from the observation layer and a `Document`
object from `local_docs_analyzer.rs` legitimately share a display name per real file, not a
duplicate bug), and `ekos query object` on each of the two `Document` ids confirms real, distinct,
correctly-attributed content: one is the project's actual `README.md` ("# PDF Reader..."), the
other is `frontend`'s real, untouched Vite scaffold text ("# React + TypeScript + Vite...").

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups, including the 2 updated + 1 new identity
test). `tests/integration` 3/3.

## Knowledge Captured

- **A pass-level result cache keyed on upstream content hashes, not on the compiling code's own
  version, will silently serve a stale result across a logic-only fix with no error or warning
  beyond one easy-to-miss `INFO skipping pass (cached)` log line.** Any live verification of a
  change to `compile`-stage logic (identity resolution, semantic compilation, anything gated by
  `crate::cache::should_recompute`) needs either a full ledger rebuild or an explicit check that
  the relevant pass actually re-ran — object/relationship counts alone can look identical between
  a stale and a fresh result if the fix's effect happens to net out to the same total, and did here
  (147 either way, until specifically cross-checked against what the fix should have produced).
- **`ekos clean` and a full `.ekos` rebuild are not interchangeable for verification purposes** —
  `clean` only clears `artifact_dir`, and `build`'s own separate re-scan fingerprint isn't stored
  there, so a `clean` followed by `build` can silently skip re-scanning against a now-empty
  artifact store, producing a badly truncated recover run with no error. A real, separate caching
  robustness gap worth someone's attention (not chased down further this session, out of scope for
  the identity fix), but the practical lesson for this session's own verification methodology is:
  when in doubt, `rm -rf .ekos` + `init` is the one reliably-consistent reset, already established
  and reused successfully throughout this session.
- **A `Custom(_)` kind that is "structurally self-identified" (deterministically keyed by some real
  property of the source, not by a name a person or LLM chose) almost always belongs in
  `DefaultResolver`'s blanket exclusion list** — this is the ninth kind to hit this exact failure
  shape, and CLAUDE.md already names the obligation explicitly. Worth treating as a standing
  checklist item for every future analyzer, not just a lesson to re-learn.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/identity/src/lib.rs` | `Custom("Document")` added to the blanket kind-exclusion list; 1 new regression test; 2 pre-existing tests updated to use a non-excluded example kind |
| `pdf-reader/.ekos/` (external project) | Rebuilt fully fresh against the fix |
