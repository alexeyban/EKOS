# RFC 0109 — Architecture Claim Human Review (RFC 0068 §62 Phase 2)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0068 references a "Human Review" workflow throughout its target package, and `TODO.md`'s own
tracking had already investigated and named the closest real precedent: RFC 0029's
`ekos_identity_review` — confirm/reject a candidate `SameAs` relationship, writing an Event to the
ledger, the one deliberate write-capable exception in an otherwise read-only MCP server.

The real gap this RFC closes: `ArchitectureReasoningPass` (RFC 0065 Phase 2) writes every role
`Claim` with `claim_type: "inference"` — an honest label that it's LLM-derived — but nothing
downstream ever distinguishes an inference nobody has looked at from one a human has actually
confirmed. `evaluate_architecture`, the rendered docs, and RFC 0108's own architecture diff all
treat every claim as equally trustworthy the moment it's compiled. Identity resolution solved the
identical problem for cross-system candidate matches (RFC 0029/0063) years earlier in this
project's history; architecture role claims never got the same treatment.

## Design

### `review_status` — read by absence, never written by the reasoning pass itself

A real correctness trap, found and designed around before writing any code: `ArchitectureReasoningPass`
is deliberately ledger-free (RFC 0068 §31's own established precedent — recovery passes only ever
produce KIR flowing forward through compile→commit, never read the ledger). If the pass stamped
every claim it writes with a literal `review_status: "unconfirmed"` property, that property would
be part of the object's content signature (RFC 0015) — and since the *same* deterministic claim id
gets re-derived on every `recover`/`commit` cycle, a claim a human had already confirmed would
compare as "different content" against the pass's freshly-derived (always-unconfirmed) version on
the very next re-run, silently reverting the human's decision back to unconfirmed. Avoided
entirely: the reasoning pass never writes `review_status` at all. A claim's status is read by
**absence** — no property means unconfirmed — the exact same convention already needed for claims
committed before this RFC existed, now the *only* convention, not a special case for old data.

### `commit.rs`: preserving a real review decision across re-commits

This alone isn't sufficient — without more, a *reviewed* claim (which *does* carry
`review_status`) would still lose that property on the next re-commit, for the identical reason
(the freshly-derived object never has it, so the content signature still differs). Fixed at the one
layer that already does real ledger-aware object enrichment before appending
(`commit.rs::commit_rollups`/`commit_data_lineage`, RFC 0044/0075's own precedent for "this needs a
ledger read the reasoning pass itself can't do"): before appending each freshly-compiled object,
`preserve_claim_review_status` checks whether a claim with the same id already exists in the ledger
with a `review_status`; if the role `value` is byte-identical to the existing version, it carries
`review_status`/`reviewed_at` forward onto the new object before `append_object` runs. If the role
`value` genuinely changed, review status is **not** carried forward — a changed assertion is a new
claim, and the old confirmation was never about this new one. When nothing about the claim actually
changed (same value, same carried-forward review metadata), `append_object`'s own content-signature
check finds no real difference and skips writing a new version at all — a reviewed, stable claim
stays completely untouched across repeated re-commits, not just eventually-consistent.

### `ekos_architecture_review`: the second write-capable MCP tool

Mirrors `ekos_identity_review`'s shape exactly, substituting a `Claim` object for a `SameAs`
relationship: `claim_id` + `decision` (`"confirmed"`/`"rejected"`). Verifies the target is really a
`Custom("Claim")` with `predicate == "has_role"` (the same kind-check discipline
`ekos_identity_review` already applies — this tool can't be used to "review" an arbitrary object).
Sets `review_status` + a real `reviewed_at` timestamp, then `append_object`s the claim — since the
content genuinely changed, this creates a real new version through the ledger's existing
content-signature versioning (RFC 0015), preserving the original unconfirmed version in
`object_history` rather than mutating it in place (the append-only invariant holds exactly the same
way it already does for `ekos_identity_review`'s relationship updates). Writes a `KirEvent`
(`EventKind::Modified`, subject = the claim's id, payload names the decision) recording that a
review happened, matching `ekos_identity_review`'s own audit-trail precedent.

Deliberately MCP-only, no new CLI subcommand — matches `ekos_identity_review`'s own precedent
exactly (confirmed by checking `IdentityCommands` in `bin/ekos.rs`: identity review has never had a
CLI equivalent, only the MCP tool). Reviewing is inherently an interactive, per-candidate decision;
MCP is where that decision actually gets made (by a human through an MCP client, or by an agent like
this project's own `identity-reviewer` pattern), not a batch CLI operation.

## Non-goals

- **Changing how `evaluate_architecture`/rendered docs/RFC 0108's architecture diff treat
  `review_status`.** They still read every claim the same way, confirmed or not — a real, separate
  follow-on (e.g. a lower completeness/confidence weight for unconfirmed claims, or a rendered
  "unreviewed" badge) that needs its own scoping, not bundled into "build the review mechanism"
  here. Matches RFC 0029's own v1 scope, which likewise didn't immediately change how confirmed
  `SameAs` relationships were consumed elsewhere.
- **A CLI-driven batch review command.** MCP-only, matching the established precedent — see Design.
- **Retroactively backfilling `review_status: "unconfirmed"` onto already-committed claims from
  before this RFC.** Read-as-unconfirmed-by-convention (absence of the property) already covers
  this correctly without needing a migration; a real backfill would touch every existing workspace's
  ledger for a property whose absence already means the right thing.

## Verification

New `ekos` (CLI) tests for `commit.rs::preserve_claim_review_status`: a reviewed claim
(`review_status: "confirmed"`) re-committed with an *unchanged* role value keeps its review status
and writes **no** new version (`append_object` returns `false`) — the core regression test this
RFC exists to guarantee; a reviewed claim re-committed with a *changed* role value does **not**
carry the old review status forward, and does write a new (unconfirmed) version; a claim with no
prior ledger version at all (first-ever commit) is untouched, matching current behavior exactly.
New MCP tests: `ekos_architecture_review` confirms a claim (verified via a direct `get_object`
re-fetch showing `review_status: "confirmed"` and a new version in `object_history`, the original
unconfirmed version still present); rejects a claim the same way; refuses to review a
non-`Claim`/non-role object with a clear tool error, matching `ekos_identity_review`'s own refusal
test for a non-`SameAs` relationship; a review event is recorded and independently retrievable.
Full workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`,
`test --workspace`), `tests/integration` 3/3.

Live-verified: ran the real `ekos mcp serve` binary against a real workspace with a real
LLM-classified role claim (confirmed no `review_status` property on the freshly compiled claim —
correctly read as unconfirmed by absence), sent a real `ekos_architecture_review` `tools/call`
confirming it, confirmed the claim's current version now reads `review_status: "confirmed"` while
its original, unreviewed version remains intact in `object_history`, then ran the real
`recover`/`compile`/`commit` pipeline again with no source changes — confirmed the claim stayed at
exactly two versions (no silent third "reverted to unconfirmed" version appeared) and its current
`review_status` was still `"confirmed"`.
