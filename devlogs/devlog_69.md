# Devlog 69 — Same-source identity merges now go through review, not a new threshold

**Date:** 2026-08-21
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

RFC 0060's own honest finding — no single confidence threshold cleanly separates real known-good
merges from real known-wrong ones — meant tuning `DefaultResolver`'s threshold further was never
going to close its documented residual (3 of 17 known-wrong real pairs, restated for GitHub items
in RFC 0062). Closed it structurally instead, per RFC 0060's own Non-Goals: extended RFC 0029's
`unconfirmed`-until-`ekos_identity_review` review flow to same-source merges. The dividing line is
exact-vs-fuzzy normalized name match, not a second threshold. Verified live against a disposable
copy of the real `analytics/` estate.

---

## RFC 0063 — Extend identity review to same-source merges

### Problem / motivation

`DefaultResolver`'s merges are applied via `apply_merges`, which deletes non-canonical objects from
the graph before the CKM is committed. The ledger is append-only with no object-level
delete/tombstone anywhere in the codebase — confirmed while reading `SECURITY.md` earlier this
session — so a wrong `DefaultResolver` merge is unrecoverable data loss, not a cosmetic bug. RFC
0029's cross-system candidates never have this risk: they're written as `unconfirmed`
`Custom("SameAs")` relationships, and confirming/rejecting one only flips a `status` property and
writes an Event (confirmed by reading `ekos_identity_review`'s handler in `crates/cli/src/commands/
mcp.rs`) — both objects always stay intact. RFC 0060 explicitly named extending that same flow to
same-source merges as the real fix, not done in that RFC's own scope.

### What was built

| Component | What it does |
|---|---|
| `MergeProposal.exact_name_match` (`crates/identity/src/lib.rs`) | New field on `DefaultResolver::resolve()`'s output: `true` only if every group member normalizes to the exact same name as the canonical |
| `partition_proposals` / `review_candidates_for` (`crates/semantic/src/lib.rs`) | Split proposals by that flag; exact ones still auto-merge via `apply_merges`; fuzzy ones become `unconfirmed` `SameAs` relationships + evidence instead |
| `resolve.rs` preview output | Labels each proposal `auto-merge` or `sent to review`, matching what `ekos compile` will actually do |

### Implementation details worth remembering

**Why exact-vs-fuzzy, not a second numeric threshold.** RFC 0060's own data made this the only
defensible split: `Build Private Images GHCR`/`Build Public Images GHCR` (wrong) scores 0.9277,
*higher* than `Adam Rutkowski`/`Adam` (correct) at 0.9000. Tuning up to 0.93 loses `Adam
Rutkowski`/`Adam` and `Vini Brasil`/`Vinicius Brasil` while still missing all 3 residual wrong
pairs — RFC 0060 tried this and rejected it. But every one of the 3 known-wrong residual pairs is a
*fuzzy* name match, and — this is the part that makes exact-vs-fuzzy work where a threshold
can't — so is every one of the 3 known-*correct* merges. Fuzzy matching itself is the ambiguous
case, independent of where it happens to land on the confidence axis. An exact normalized-name
match (case/whitespace variants of the literal same string) carries none of that ambiguity in any
of the real data checked.

**Union-Find transitivity means a group's exactness isn't just "the winning pair."** `A↔B` exact
and `B↔C` fuzzy can still union all three into one group even though `A↔C` alone might score below
threshold. `exact_name_match` is `true` only if *every* member matches the canonical exactly —
tested directly (`exact_name_match_false_for_a_transitively_chained_mixed_group`) — so a group
can't sneak an unsafe fuzzy member in by riding along with a safe one.

**Zero changes needed to `ekos_identity_review` itself.** Its handler only checks
`RelationshipKind::Custom("SameAs")`, never the relationship's origin — so same-source review
candidates (written from `crates/semantic`, at compile time, pre-ledger) and cross-system ones
(written from `crates/cli/src/commands/identity.rs::scan`, post-commit, against the live ledger)
are indistinguishable to the reviewer and to the tool. Confirmed live: drove the real MCP tool over
stdio JSON-RPC against a same-source candidate and it worked with the code exactly as RFC 0029 left
it.

**Confirming a same-source candidate does not merge objects — deliberately.** This matches
cross-system's real semantics ("these are related") rather than inventing a new "confirm and
retroactively merge" pathway, which would have to solve remapping already-committed relationship
IDs in an append-only store — a materially bigger and riskier change than this RFC's actual problem
(stopping *wrong, silent, irreversible* merges) required.

**Two different code paths, one relationship shape.** `identity.rs::scan` writes cross-system
candidates directly against an already-open `&dyn KnowledgeStore` (`ledger.append_relationship`),
post-commit. This RFC's same-source candidates are synthesized inside `SemanticCompilerPass::run`,
pre-CKM, and flow to the ledger through the ordinary `build_ckm` → `commit.rs::ckm_rel_to_kir`
path — confirmed by reading `commit.rs` directly that `CkmRelationship.properties` round-trips
verbatim, so `status: "unconfirmed"` set at compile time survives unchanged into the ledger. Same
end shape (`Custom("SameAs")`, `status`/`confidence` properties), two legitimately different
origins for it, by design.

### Decisions (alternatives considered, why this choice)

- **A second, higher "auto-merge" confidence threshold** — rejected. RFC 0060 already showed no
  threshold on the current formula separates the two classes; a second threshold just relocates the
  same interleaving problem.
- **Actually merging objects on confirmation** — rejected as materially larger scope for no real
  benefit here: the goal was stopping silent, wrong, irreversible deletes, not building a new
  "confirm-then-merge" remapping pathway. Matching cross-system's existing "related, not merged"
  contract was both simpler and more conservative given the append-only/no-tombstone constraint.
- **Per-pairwise-edge review relationships instead of star topology** — rejected as unnecessary
  complexity: `MergeProposal` already summarizes a group by canonical + confidence (the group-max
  score, an existing simplification `apply_merges` also relies on), and canonical→member star
  relationships are the natural mirror of that existing shape.

---

## Live verification against real data

Ran the actual pipeline against a **disposable copy** of the real `analytics/` (Plausible
Analytics) `.ekos/` state — never against the live workspace itself, since `compile`/`commit` write
real, permanent records. `ekos resolve` on the real corpus: 1439 proposals, split 1321 auto-merge /
118 sent to review. All 3 RFC 0060 residual pairs (`Build Private Images GHCR`/`Build Public Images
GHCR`, `Tracker CI`/`Tracker script update`, `ua_inspector.readme.md`/`ref_inspector.readme.md`)
now route to review. Read the compiled CKM directly (`unzstd` + `python3 json`): both `Build
Private Images GHCR` and `Build Public Images GHCR` survive as 5 separate real objects each,
connected by 5 real `unconfirmed` `SameAs` relationships carrying the expected evidence fragment
(`"same-source merge candidate for 'Build Private Images GHCR' (Pipeline), confidence=1.00"`).
Committed to the scratch ledger, then drove the real `ekos mcp serve` binary over stdio JSON-RPC
with a real `ekos_identity_review` call against one of the new same-source candidates — `status`
flipped to `confirmed`, no code changes needed there, exactly as designed.

**One workaround worth remembering**: `ekos compile`'s Phase 13 pass-cache
(`.ekos/artifacts/pass-manifests/`) keys on `{cache_inputs, config_hash}`, not on the pass's own
code — so re-running `compile` against unchanged recover output silently reused the *old* CKM
(from before this fix) until the manifest directory was moved aside. Found existing
`pass-manifests.bak-pre-identity-fix`-style directories already in the real `analytics/`
`.ekos/artifacts/`, confirming this exact workaround was already established practice from RFC
0060's own verification — not a new discovery.

---

## Knowledge Captured

- **When a metric genuinely can't be separated by any threshold on real data, don't keep tuning the
  threshold — change what kind of decision is being made.** RFC 0060 already proved this
  numerically; the fix was to stop trying to classify by score and instead classify by a structural
  property (exact vs. fuzzy string match) that happens to align with the real risk.
- **"Doesn't merge objects on confirm" is not a limitation of the review flow — it's the load-bearing
  safety property.** In an append-only ledger with no tombstone, "confirm this relationship" and
  "confirm this delete" have very different blast radii; keeping same-source review on the
  non-destructive contract cross-system already uses was a deliberate, conservative choice, not an
  oversight to fix later.
- **A compile pass's cache key can outlive the code that produced what it's caching.** Verifying a
  behavior change against real committed data requires checking whether the pass will actually
  re-run, not just that the binary was rebuilt — `.ekos/artifacts/pass-manifests/` is the thing to
  clear (on a disposable copy, never the live workspace) when in doubt.
- **Never run compile/commit-style live verification against a real, currently-in-use workspace.**
  Copying `.ekos/` to a scratch directory first cost nothing and meant a wrong assumption during
  verification couldn't have written bad data into a real, append-only ledger.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0063-identity-review-for-same-source-merges.md` | New RFC |
| `ekos/crates/identity/src/lib.rs` | `MergeProposal.exact_name_match`; computed in `resolve()`; 5 new tests |
| `ekos/crates/semantic/src/lib.rs` | `partition_proposals`, `review_candidates_for`; `SemanticCompilerPass::run` wired to both; 4 new tests |
| `ekos/crates/cli/src/commands/resolve.rs` | Preview output now labels each proposal's real disposition |
| `TODO.md` | "Identity resolution" backlog item ticked off |
| `devlogs/devlog_69.md` | This file |
