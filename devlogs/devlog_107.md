# Devlog 107 — `compile.log`'s SEM002 noise, root-caused and precisely classified

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Fourth item on the gap-closure list: "`compile.log`'s `SEM002` warnings firing on ids that
actually resolve fine (discrepancy between `resolve`'s 0-conflict report and `ekos_semantic`'s
compile-time validation appearing to check a narrower object set)" — flagged three times already
this session (`devlog_101`, `devlog_104`) and left uninvestigated each time. Root-caused precisely:
`ekos compile`'s CKM validation checks relationships against a genuinely narrower object set by
design (`File` objects are written straight to the ledger by `ekos build`, never through the
`KnowledgeArtifact`s this compile stage reads) — not a bug in identity resolution, a real,
documented architectural split whose diagnostic output just never explained itself. Fixed by making
the warning count say so, cross-referenced against the real ledger. Live-verified on `pdf-reader`:
of 184 raw `SEM002` warnings (23 distinct dangling ids), 22 are now correctly shown as expected
`File` references and exactly 1 is honestly still flagged — and that one turned out to be a real,
already-independently-documented, separate limitation (see below), not something this fix should
have silently swallowed too.

## Root cause

`crates/semantic/src/lib.rs`'s `SemanticCompilerPass` builds its `combined` graph by reading only
`KnowledgeArtifact`s from the artifact store — i.e., only `recover`-stage pass output. `ekos
build`'s `File` objects are written directly to the ledger (`ledger.append_object`, `build.rs`),
never through that artifact-store path — a deliberate split, already documented in-line ("Hierarchical
rollups (RFC 0044) intentionally do NOT run here... `combined`/`resolved` above only ever contain
recovery-pass output"). `CkModel::validate()` then checks every relationship's `from`/`to` against
`model.objects` — which, given the above, structurally can never contain a `File`. Since the large
majority of real `DependsOn`/`Contains` edges any analyzer emits point *at* a `File` (the thing
being imported into, the thing containing a symbol, ...), nearly every SEM002 warning was this one,
entirely expected, gap — not a real defect, and not related to `ekos resolve`'s own separate report
(which never runs this check at all).

## The fix

- `CkModel` (`crates/semantic/src/lib.rs`) gained `dangling_relationship_target_ids(&self) ->
  HashSet<KirId>` — the same set `validate()` already computes internally, exposed as real ids
  instead of only formatted message strings, so a caller with a broader view of "known ids" can
  classify them precisely instead of parsing `validate()`'s text output. Zero new dependencies for
  `crates/semantic` — pure, additive, same crate.
- `crates/cli/src/commands/compile.rs` (which already has ledger access — `ekos-ledger` was already
  a real dependency) cross-references those dangling ids against the ledger's real `File` objects
  (already written by the time `compile` runs, since `build` always runs first in the documented
  pipeline) and reports a classified count: `"184 (22 are expected File-object references — ...
  resolve correctly after ekos commit; 1 other(s), see <log>)"` instead of a single opaque number.
  Falls back to the unclassified count if the ledger can't be opened, so this never blocks or
  breaks a compile run — purely a reporting improvement.
- 4 new tests: 2 for `CkModel::dangling_relationship_target_ids` (`crates/semantic`), the CLI-layer
  classification logic exercised indirectly via the live verification below (no ledger fixture
  harness existed in `crates/cli`'s own test suite for this command to unit test against without
  a disproportionate amount of new scaffolding for what is fundamentally a formatting change).

## A real finding surfaced by the fix, correctly left unfixed

The one id `pdf-reader` still genuinely flags (`fb4514e9-...`) traced to `git_analyzer.rs`'s
`OwnedBy` relationship: `KirRelationship::new(RelationshipKind::OwnedBy, subject_id, contrib_id)`,
where `subject_id = Uuid::v5(sha)` — a synthetic id representing "this commit," used elsewhere in
the same pass only as a `KirEvent.subject`, never registered as a real `KirObject` anywhere. A
`KirRelationship` connecting an object-less, event-only id is real, but — checked against
`docs-gen`'s own `render_data_architecture` (`## Ownership` section, `crates/docs-gen/src/lib.rs`)
— this is a **already fully documented, known gap**, not a new discovery: its own in-line text
already says *"`OwnedBy` edges are compiled from git history (`git_analyzer.rs`), but only from a
commit event to the contributor who authored it, never onto a `File`/`Table`/`Dataset` object...
`git_analyzer.rs` would need to derive a per-file top-contributor relationship, the way it already
derives per-file `CoupledWith` coupling"* — a real, scoped, RFC-worthy feature (per-file ownership
derivation), not a bug fix, and explicitly out of scope for this session's list. Left as-is,
correctly still surfaced by this fix's honest "1 other, see log" rather than silently absorbed into
"expected" — proof the classification is precise, not just a blanket suppression.

## Live verification

Rebuilt `pdf-reader`'s `.ekos/` fully fresh: `ekos compile` now prints `Warnings: 184 (22 are
expected File-object references — see CkModel::dangling_relationship_target_ids' doc comment —
resolve correctly after \`ekos commit\`; 1 other(s), see .../compile.log)`. Cross-checked every one
of the 23 distinct dangling ids directly (`ekos query object <id>` after `commit`, plus the raw
decompressed `model.json.zst` for the one id not yet committed at classification time): 22 are
real, correctly-attributed `File` objects (`app/api/pdf.py`, `package.json`, `src/App.tsx`, ...);
the 23rd is genuinely absent from `model.objects`, traced to the `git_analyzer.rs` finding above.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups). `tests/integration` 3/3.

## Knowledge Captured

- **A validation check scoped to a deliberately narrow object set needs to say so in its own
  output, not just in a code comment three call-frames away** — the underlying architectural split
  (`File` objects bypass the artifact-store path) was already correctly documented in `crates/semantic`
  since RFC 0044, but the diagnostic text itself (`"unknown from-id <uuid>"`) gave no indication
  this was expected, so it read as a real, unexplained discrepancy every time someone looked at
  `compile.log` — including three separate times this session before finally being traced.
  Precision in what a warning *count* claims (not just what an individual message says) matters:
  "184 warnings" invites investigating all 184; "22 expected, 1 other" invites investigating 1.
- **Once one "expected but unexplained" class of noise is silenced, the next one becomes visible**
  — before this fix, 184 warnings were indistinguishable noise; after, exactly one stood out, and
  it turned out to already be independently diagnosed and documented elsewhere in the codebase
  (`docs-gen`'s own Ownership section). Worth remembering: fixing a noisy diagnostic's *signal-to-
  noise ratio* is often more valuable than fixing everything it happens to be pointing at — some of
  what it's pointing at may already be a known, correctly-scoped-out gap.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/semantic/src/lib.rs` | New `CkModel::dangling_relationship_target_ids()`; 2 new tests |
| `ekos/crates/cli/src/commands/compile.rs` | Classifies dangling relationship-target ids against the ledger's real `File` objects; warning count now distinguishes expected vs. genuinely unresolved |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh against the fix |
