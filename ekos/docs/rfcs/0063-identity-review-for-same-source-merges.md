# RFC 0063 — Extend Identity Review to Same-Source Merges

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-21

---

## Motivation

RFC 0060 (accepted 2026-08-20) raised `DefaultResolver`'s merge threshold from 0.85 to 0.90 using
real numbers from `analytics/` (Plausible Analytics) and found something more important than the
threshold value itself: **no single confidence threshold on the current scoring formula
(`0.7×name + 0.3×structural`) separates every known-correct merge from every known-wrong one.**
Known-good and known-bad real pairs interleave — `Build Private Images GHCR`/`Build Public Images
GHCR` (two different real CI pipelines) scores 0.9277, higher than the known-correct `Adam
Rutkowski`/`Adam` merge at 0.9000. RFC 0060 tried tuning the threshold higher (0.93+) and rejected
it: it would lose 3 known-good merges while still missing all 3 residual known-wrong pairs. Its own
Non-Goals section named the real fix and explicitly deferred it: *"extending RFC 0029's
cross-system `unconfirmed`-until-`ekos_identity_review`-reviewed flow to same-source
(`DefaultResolver`) merges ... is real follow-on work, not done here."* RFC 0062 found the
identical failure shape in GitHub items.

This matters more than "3 wrong merges out of many real ones." `DefaultResolver`'s merges are
applied via `apply_merges` (`crates/semantic/src/lib.rs`), which deletes the non-canonical objects
from the graph before the CKM is built and committed. The Semantic Knowledge Ledger is append-only
with no object-level delete/tombstone anywhere in the codebase — so a wrong `DefaultResolver` merge
is not cosmetic, it is unrecoverable data loss once committed. This is architecturally different
from `cross_system.rs`'s candidates (RFC 0029), which are written as `unconfirmed`
`Custom("SameAs")` relationships: both objects stay fully intact whether the candidate is confirmed
or rejected — `ekos_identity_review`'s handler only flips a `status` property and writes an Event,
never merging or deleting anything.

## Design

Given RFC 0060's own finding that no numeric confidence cutoff cleanly separates the two classes,
adding a second, higher "auto-merge" threshold would just relocate the same interleaving problem to
a different number, not fix it. The dividing line used here instead is **exact vs. fuzzy name
match**: every one of RFC 0060's 3 residual known-wrong pairs is a *fuzzy* match (different
strings that happen to score high) — but so is every one of its 3 known-*correct* merges (`Adam
Rutkowski`/`Adam`, `RobertJoonas`/`Robert`, `Vini Brasil`/`Vinicius Brasil` — none of these are
literal string-identical after normalization either). Fuzzy matching itself is the judgment call
RFC 0029's review flow exists for, regardless of which side of any threshold it lands on. An
**exact** normalized-name match within the same kind carries none of that ambiguity and stays safe
to auto-merge.

### `MergeProposal::exact_name_match`

`crates/identity/src/lib.rs`'s `DefaultResolver::resolve()` now computes one new bool per proposal:
`true` only if **every** member of the Union-Find group normalizes (via the existing,
qualifier-aware `name_for_similarity`) to the exact same string as the canonical. A group that rode
in on one exact and one fuzzy pairwise link — Union-Find is transitive, so `A↔B` exact and `B↔C`
fuzzy can still union `A`, `B`, `C` into one group even though `A↔C` alone might not score above
threshold — is conservatively treated as fuzzy as a whole. Scoring, blocking, and Union-Find
themselves are unchanged.

### `SemanticCompilerPass` splits on that flag

`crates/semantic/src/lib.rs`'s `SemanticCompilerPass::run` partitions `resolution.proposals` by
`exact_name_match` before applying anything:

- **Exact-match proposals** go to `apply_merges` exactly as before — no behavior change for the
  unambiguous case (the common case: literal duplicate objects from re-running `recover`).
- **Fuzzy-match proposals** are not merged. For each group, one `unconfirmed`
  `Custom("SameAs")` `KirRelationship` per non-canonical member is synthesized (canonical → member,
  star topology, `properties["status"] = "unconfirmed"`, `properties["confidence"]` from the
  proposal), in the same shape `crates/cli/src/commands/identity.rs::scan` already writes for
  cross-system candidates — plus one `KirEvidence` per relationship recording the match. Both are
  pushed into the resolved graph before `build_ckm` runs, so they flow through the existing,
  unmodified `commit.rs::ckm_rel_to_kir` path into the ledger exactly like any other relationship.

Because `ekos_identity_review`'s handler (`crates/cli/src/commands/mcp.rs`) only checks
`RelationshipKind::Custom("SameAs")`, not the relationship's origin, it handles these same-source
candidates with **zero code changes** — confirming or rejecting a same-source candidate has the
same, deliberately non-destructive contract as a cross-system one: a status flip and an Event, never
an object merge. Given the append-only/no-tombstone ledger, this is the conservative choice —
consistent with cross-system semantics ("these are related") rather than inventing a new "confirm
this and retroactively merge/delete" pathway.

`crates/cli/src/commands/resolve.rs` (a preview-only command — it never writes to disk; `ekos
compile` recomputes resolution independently) now labels each printed proposal with its real
disposition (`auto-merge` vs. `sent to review (fuzzy match, RFC 0063)`), so its output stops
implying every listed proposal becomes one merged object.

## Non-goals

- Not a new confidence threshold — RFC 0060 already showed this doesn't cleanly separate the
  known-good/known-bad cases on real data.
- Not touching `cross_system.rs`, `identity.rs::scan`, or `ekos_identity_review`'s handler — reused
  as-is, not rebuilt.
- Not retroactively merging objects on confirmation — kept on the same non-destructive contract
  cross-system candidates already use.
- Not touching `structural_score`, blocking, or Union-Find.

## Testing

- `crates/identity/src/lib.rs`: `exact_name_match` true for identical-after-normalization pairs;
  false for RFC 0060's known-good fuzzy merges (still correctly proposed, just flagged fuzzy);
  false for all 3 RFC 0060 residual known-wrong pairs (still proposed — this RFC doesn't change
  whether a pair is proposed, only how it's handled); false for a group formed by a transitively
  chained exact+fuzzy pair.
- `crates/semantic/src/lib.rs`: `partition_proposals` splits correctly; `review_candidates_for`
  produces the expected `unconfirmed` `SameAs` shape; an end-to-end test confirms a fuzzy proposal
  leaves both objects in the resolved graph and survives through `build_ckm` as a relationship.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test` (separate workspace, per devlog_67's lesson that `ekos/`'s
  `cargo test --workspace` alone is not the full gate).
- **Live, real data**: ran `resolve`/`compile`/`commit` against a disposable copy of the real
  `analytics/` `.ekos/` state (never against the live workspace itself). `ekos resolve` reported
  1439 proposals — 1321 auto-merge, 118 sent to review. All 3 RFC 0060 residual pairs (`Build
  Private Images GHCR`/`Build Public Images GHCR`, `Tracker CI`/`Tracker script update`,
  `ua_inspector.readme.md`/`ref_inspector.readme.md`) now route to review instead of silently
  merging; reading the compiled CKM directly confirmed both sides of `Build Private/Public Images
  GHCR` survive as separate objects (5 each), connected by 5 real `unconfirmed` `SameAs`
  relationships with the expected evidence fragment. Drove the real `ekos_identity_review` MCP tool
  over stdio JSON-RPC against one of the newly created same-source candidates and confirmed it —
  `status` flipped to `confirmed`, no code changes needed in the tool itself.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0063-identity-review-for-same-source-merges.md` | This RFC |
| `ekos/crates/identity/src/lib.rs` | `MergeProposal.exact_name_match`; computed in `resolve()`; 5 new tests |
| `ekos/crates/semantic/src/lib.rs` | `partition_proposals`, `review_candidates_for`; `SemanticCompilerPass::run` wired to both; 4 new tests |
| `ekos/crates/cli/src/commands/resolve.rs` | Preview output labels each proposal's real disposition |
