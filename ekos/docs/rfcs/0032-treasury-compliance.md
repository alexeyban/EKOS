# RFC 0032 — DAO Treasury Compliance: Payment↔Approval Matching

**Status:** Accepted — implemented 2026-09-19 against mock clients. `SnapshotClient` **live-verified 2026-09-22** against `hub.snapshot.org` (found a silent-failure mode, now warned about); `RealTreasuryClient` still **not run live** — needs an explorer API key and a real treasury address (see "Implementation notes" and "Live verification")
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

Target scenario: a DAO multisig pays out 50,000 USDC to an address on-chain. A contributor or
auditor asks "was this payment approved by governance?" Today that question is answered by a
human manually cross-referencing a block explorer, a governance forum thread, and (often) a
spreadsheet someone maintains by hand — slow, error-prone, and produces no durable, checkable
record. The same failure mode repeats at every DAO: treasury transparency is a manual reconciliation
exercise, not something a system can answer on demand with a citation.

This is architecturally the same shape of problem RFC 0029 (Cross-System Identity Resolution)
solved for `cust_mstr` / `customers` / `gold.dim_customer` — two independently-observed records
that plausibly refer to the same real-world fact, with no direct foreign key linking them, where
getting the link wrong (or silently assuming it) is worse than leaving it unresolved. RFC 0029's
own framing applies verbatim here: "a candidate match must be recorded as an explicit, reviewable
hypothesis — never silently merged, never indistinguishable from a directly observed fact — until
a human or agent confirms it." A wrongly-auto-confirmed payment↔proposal match is a materially
worse failure than a wrongly-auto-merged table alias — it is a factual claim about whether real
money was authorized.

Nothing in EKOS today reads raw on-chain transaction history or governance-forum data. The
existing `crypto` connector (RFC 0017) reads a pre-processed Parquet export written by a separate
off-chain pipeline ("DeFi Sentinel") — it does not observe a chain directly, so it is not a fit to
extend for this.

## Scope

- A new connector observing a DAO treasury address's on-chain transaction history via a
  block-explorer REST API.
- A new connector (or extension of an existing text-source pattern) observing governance
  proposals from a forum or Snapshot.
- A new matching pass, structurally reusing RFC 0029's `CrossSystemScorer` shape, linking
  `TreasuryPayment` objects to `GovernanceProposal` objects with a confidence score and cited
  evidence.
- The MCP-visible outcome: an agent can query a payment and get back its approval status (or
  explicit lack thereof) with the exact transaction hash and proposal/thread it was checked
  against.

## Non-goals

- Reading directly from an RPC node / decoding arbitrary contract calldata (ABI decoding for
  non-standard multisig batch calls). v1 targets simple native-token transfers and standard
  ERC-20 `transfer`/`transferFrom` calls, which explorer APIs already decode into a stable JSON
  shape — a raw-RPC connector is a plausible future RFC, not this one.
- Automatically flagging or blocking "unapproved" payments. EKOS's Runtime is read-only and never
  interprets business meaning beyond what it observed (see `CLAUDE.md`'s key invariants) — this
  RFC surfaces the evidence for a human/agent to judge, it does not render a verdict.
- Supporting every DAO governance platform. v1 targets one forum flavor (Discourse) or Snapshot,
  not both, and not e.g. Tally, Aragon, or Colony (see Open Questions).

_Both the raw-RPC connector and broader DAO governance platform support are tracked as backlog:
see `TODO.md` → "Promoted from RFC Non-Goals" → "Connector-specific gaps"._

## Design

### `TreasuryObserver` (new crate, e.g. `ekos/plugins/treasury`)

Follows the `Observer` trait (`ekos/crates/observation-sdk/src/lib.rs`) exactly as every existing
connector does: `fn name()`, `async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage,
ObserveError>`, `scan` side-effect-free and idempotent.

Split into a thin `TreasuryObserver` wrapper and a `TreasuryClient` trait, mirroring
`GitHubClient`/`ConfluenceClient` (`ekos/plugins/github/src/lib.rs`,
`ekos/plugins/confluence/src/lib.rs`):

```rust
#[async_trait]
pub trait TreasuryClient: Send + Sync {
    /// Every transaction touching `address`, block-explorer-decoded.
    async fn list_transactions(
        &self, address: &str,
    ) -> Result<Vec<OnChainTx>, TreasuryClientError>;
}

pub struct OnChainTx {
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    pub value: String,       // decimal string, avoids float precision loss
    pub token: Option<String>,  // None = native asset
    pub memo: Option<String>,   // decoded input-data text, when present
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub block_number: u64,
}
```

A `RealTreasuryClient` calls a block-explorer REST API (Etherscan-family `/api?module=account&
action=txlist`, or the chain-appropriate equivalent — see Open Questions); a `MockTreasuryClient`
exercises the real mapping logic with zero network dependency, the same two-tier testing
discipline every existing connector uses (`ekos/plugins/confluence/src/lib.rs`'s doc comment
states this pattern explicitly: "`MockConfluenceClient` exercises the real mapping logic ...
without any network dependency").

One `ObservationArtifact` per transaction. Recovery pass emits `Object { kind:
Custom("TreasuryPayment"), properties: {tx_hash, from, to, value, token, memo, timestamp,
block_number} }`, deterministic id via `Uuid::new_v5(NAMESPACE, "chain:{chain_id}:tx:{tx_hash}")`
— same determinism discipline as `github:{owner}/{repo}#{n}` (RFC 0020) and every other
connector, so re-running `ekos recover` converges instead of duplicating.

**Multisig batching**: a Gnosis-Safe-style multi-send transaction batches several logical payouts
into one on-chain transaction. v1 decodes each sub-transfer (when the explorer API's "internal
transactions" / logs endpoint exposes them) as its own `TreasuryPayment` object rather than
treating the batch as a single opaque payment — otherwise a batch containing one approved and one
unapproved payout would incorrectly read as fully approved.

### Governance proposal connector

Either a `GovernanceForumObserver` (Discourse REST API — same pattern as `ConfluenceObserver`,
one artifact per proposal topic with its current status) or a `SnapshotObserver` (public GraphQL
API, one artifact per proposal with vote tally and outcome) — v1 picks one (Open Questions).
Recovery pass emits `Object { kind: Custom("GovernanceProposal"), properties: {proposal_id, title,
status, approved_amount: Option<String>, approved_recipient: Option<String>, body_excerpt} }`.
`approved_amount`/`approved_recipient` are `Option` because many real proposals are prose ("pay
the marketing team ~$10k/mo") rather than a structured payout line — this is stated as an honest
limitation below, not hidden.

### The matching layer: `find_treasury_approval_candidates`

New function in `ekos/crates/identity/`, structurally parallel to
`find_cross_system_candidates` (`ekos/crates/identity/src/cross_system.rs`) but scored on a
different, treasury-specific signal set — this is a **new function, not a parameterization of
the existing one**, for the same reason RFC 0029 itself is a new resolver rather than a
`DefaultResolver` config change: the comparison semantics and signal set are different enough
that forcing them into one generic function would make both harder to reason about.

```rust
pub struct ApprovalCandidate {
    pub payment: KirId,
    pub proposal: KirId,
    pub confidence: f32,
    pub signals: ApprovalSignals,  // {recipient_match, amount_match, text_reference, temporal}
}

pub fn find_treasury_approval_candidates(
    payments: &[KirObject],
    proposals: &[KirObject],
) -> Vec<ApprovalCandidate>
```

Four signals, each `Option<f32>` — excluded from the weighted average, not scored as 0, when its
input is unavailable, exactly RFC 0029's "degrades gracefully" rule:

1. **Recipient-address match** — exact-match `payment.to` against `proposal.approved_recipient`
   when the proposal specifies one. Highest weight when present, since an exact address match is
   near-conclusive.
2. **Amount match** — `payment.value` (+ `token`) against `proposal.approved_amount`, exact or
   within a small configurable tolerance (handles gas deduction, rounding, or a partial-tranche
   payment against a total-approved amount). `None` when the proposal didn't specify an amount.
3. **Text-reference match** — reuses `github_analyzer.rs`'s keyword-scan pattern
   (`ekos/crates/recovery/src/github_analyzer.rs`'s `find_references`-style scan) directly: does
   `payment.memo` mention the proposal's id/number, or does the proposal thread's body mention the
   transaction hash? Either direction counts. This is the one signal that survives even when a
   proposal is pure prose with no structured amount/address.
4. **Temporal proximity** — `payment.timestamp` after the proposal's recorded approval timestamp,
   within a configurable window (e.g. 90 days). A payment **before** its claimed approval is
   scored as a strong *negative* signal (not simply "no evidence") — this is the one case where
   this resolver deliberately diverges from RFC 0029's purely-additive scoring, because a
   payment predating its approval is itself informative, not neutral.

`confidence` is the weighted average of available signals, weights `{recipient: 0.35, amount:
0.25, text_reference: 0.3, temporal: 0.1}` renormalized over whichever signals are actually
available — mirroring RFC 0029's renormalization approach exactly. A floor
(`MIN_APPROVAL_CONFIDENCE`, proposed `0.3`, matching RFC 0029's own floor value as a starting
point pending real-data tuning) excludes obvious non-matches from being written at all; everything
at or above the floor is written and kept — including low-confidence candidates, so a reviewer
sees "0.35, weak evidence" rather than the system silently deciding.

### Storage — identical shape to RFC 0029's `SameAs`, new relationship kind

```rust
KirRelationship {
    kind: RelationshipKind::Custom("AuthorizedBy".to_string()),
    from: payment_id,
    to: proposal_id,
    properties: {
        "status": "unconfirmed",   // "unconfirmed" | "confirmed" | "rejected"
        "confidence": candidate.confidence,
        "recipient_match": ..., "amount_match": ..., "text_reference": ..., "temporal": ...,
    },
    evidence: [ev_id],  // cites the specific tx hash / proposal excerpt / signal values
    ..
}
```

Never consumed by `DefaultResolver`/`apply_merges`, exactly as RFC 0029's `SameAs` is not — only
an explicit read (by an agent via MCP, or a human via a review command) does anything with it.

### New CLI entry point: `ekos treasury scan`

Cannot live in `ekos resolve` for the same reason RFC 0029's cross-system scan couldn't: it needs
to read **already-committed** ledger objects (`TreasuryPayment` and `GovernanceProposal`, written
by the normal `build → recover → resolve → compile → commit` pipeline) and write new relationships
back. New command, `crates/cli/src/commands/treasury.rs`, `ekos treasury scan`:
1. `ledger.all_objects()`, filter to `Custom("TreasuryPayment")` and `Custom("GovernanceProposal")`.
2. Run `find_treasury_approval_candidates`, write each candidate ≥ floor as an `unconfirmed`
   `AuthorizedBy` relationship.
3. Report a summary: N payments, M with at least one candidate ≥ floor, K with zero (the
   "unapproved spend" watchlist).

### MCP surface

Reuses `ekos_identity_review` (RFC 0029's one write-capable tool) to confirm/reject an
`AuthorizedBy` candidate, rather than inventing a parallel review tool for a relationship that is
structurally identical to `SameAs` — extend its accepted `kind` parameter rather than branch the
tool. Reading is entirely existing surface: `ekos_state`/`ekos_neighborhood` on a
`TreasuryPayment` object returns its `AuthorizedBy` relationship(s) with status and evidence. A
payment with **zero** relationships above the floor is the interesting case, and is answerable
today with no new read tool: "no `AuthorizedBy` relationship found" is itself a citable, evidenced
answer (which proposals were checked and why each scored below the floor, from the `ekos treasury
scan` run's log).

## Alternatives Considered

- **Extend `find_cross_system_candidates` with a new object-kind branch instead of a new
  function.** Rejected: the signal set (address/amount/temporal) is domain-specific to financial
  matching and shares almost nothing with RFC 0029's column-overlap/naming-pattern signals; a
  shared function would need enough branching to lose the clarity RFC 0029 itself argued for in
  rejecting a `DefaultResolver` config change.
- **Read raw on-chain data via RPC + local ABI decoding instead of a block-explorer API.** More
  complete (works for any contract, not just explorer-decoded standard transfers) but
  significantly more implementation surface for v1, and loses the "documented API, mockable
  client, no network dependency in tests" property every existing connector has. Deferred to a
  future RFC if explorer-API coverage proves insufficient in practice.
- **Auto-confirm high-confidence matches (e.g. ≥ 0.9) instead of always requiring review.**
  Rejected for the same reason RFC 0029 rejected it: this RFC's entire value proposition is that a
  human/agent can trust the "approved" answer precisely because nothing was auto-decided on their
  behalf. Auto-confirming defeats the point for the exact cases (money movement) where it matters
  most.

## Open Questions

Resolved during implementation (2026-09-19). Where the choice was a judgement call, the reasoning is
given so it can be revisited.

- [x] **Which chain/explorer first** — **EVM, via the Etherscan-family `module=account` API** (default
      endpoint: Etherscan's multichain v2, which takes a `chainid`). Reason: it is the one explorer
      API whose response shape also decodes ERC-20 transfers and internal transactions, which is what
      a Safe multi-send needs. Solana (where EKOS's own token lives) has a materially different asset
      and account model and is left to a follow-up connector; nothing here precludes it — the
      `TreasuryClient` trait is the seam.
- [x] **Which governance platform first** — **Snapshot** (public GraphQL hub, no key). Reason: a
      proposal there has a structured outcome (choices, per-choice scores, close time), so "approved,
      and when" is data. Discourse would have made the approval timestamp a guess from forum prose.
      What a proposal approves (amount, recipient) is still free text in its body either way.
- [x] **Confidence floor and weights** — kept at RFC 0029's values (`0.3`; recipient 0.35, amount
      0.25, text reference 0.30, temporal 0.10), **still unvalidated against real DAO data**. Two
      changes from the design text, both found by the ground-truth tests:
      (a) a *partial-tranche* amount scores `0.25`, not `0.5` — at `0.5`, any payment smaller than an
      approved total counted as half a match, which let a wrong-recipient, 18%-of-the-amount payment
      clear the floor; (b) the floor is applied to the score *before* the before-approval penalty, so
      a full structured match that was paid early still surfaces (flagged, low confidence) instead of
      vanishing.
- [x] **Watchlist as a distinct MCP tool?** — **No.** `ekos treasury scan` prints it, and "no
      `AuthorizedBy` relationship found" is answerable through `ekos_state` / `ekos_neighborhood`. EKL
      has no negation ("no outgoing relationship"), so a first-class tool remains a reasonable
      follow-up if that query becomes common.

## Deviations from the design text

- `OnChainTx` gained `index` (position within its transaction). The RFC's id
  `chain:{id}:tx:{hash}` cannot tell the sub-transfers of one multi-send apart; ids are
  `chain:{id}:tx:{hash}:{index}`.
- `find_treasury_approval_candidates` takes `&[KirObject]` (it filters kinds itself) rather than two
  slices, so `ekos treasury scan` mirrors `ekos identity scan`.
- Proposals whose recorded outcome is `rejected` are never candidates: offering a payment as
  "authorized by" a vote that failed would be misleading. `unknown` outcomes are candidates, but
  carry no `approved_at`, so the temporal signal is unavailable rather than assumed.
- Confirming or rejecting an `AuthorizedBy` candidate records `AuthorizationConfirmed` /
  `AuthorizationRejected` events, not `Merged`.

## Testing

- `MockTreasuryClient`-driven tests exercising `TreasuryObserver::scan`'s mapping logic with a
  fixed transaction fixture, zero network dependency — matching every existing connector's test
  shape.
- A fixture-based test for `find_treasury_approval_candidates` with known ground-truth
  payment↔proposal pairs, including at least one deliberate non-match and one deliberate
  before-approval-timestamp negative case, asserting the scorer ranks true pairs above the floor
  and the non-match/negative case below it — mirrors how RFC 0029's `cross_system.rs` is tested.
- An integration test through `ekos build → recover → resolve → compile → commit → treasury scan`
  against the fixtures above, asserting the resulting `AuthorizedBy` relationships and their
  `status: unconfirmed` are queryable via `ekos_state`.

## Acceptance Criteria

- [x] All Open Questions resolved.
- [x] At least one review completed (self-review against CLAUDE.md invariants; no external reviewer).
- [x] `TreasuryObserver`/`TreasuryClient` and the governance connector each pass a
      `Mock*Client`-driven test suite with zero network dependency.
- [x] `find_treasury_approval_candidates` passes the ground-truth fixture test described above.
- [x] `ekos treasury scan` runs end-to-end against fixture data and produces evidenced,
      `unconfirmed`-status `AuthorizedBy` relationships queryable via existing MCP read tools.
- [x] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants
      (append-only ledger, evidence-backed conclusions, read-only Runtime, no silent merges).

## Implementation notes (2026-09-19)

| Piece | Where |
|---|---|
| `TreasuryObserver`, `TreasuryClient`, `RealTreasuryClient` (Etherscan-family), `MockTreasuryClient` | `ekos/plugins/treasury` |
| `SnapshotObserver`, `GovernanceClient`, `SnapshotClient`, `MockGovernanceClient`, outcome derivation | `ekos/plugins/governance` |
| `TreasuryAnalyzerPass` → `Custom("TreasuryPayment")`; `GovernanceAnalyzerPass` → `Custom("GovernanceProposal")` | `ekos/crates/recovery/src/{treasury,governance}_analyzer.rs` |
| `find_treasury_approval_candidates`, `score_pair`, `payments_without_candidate` | `ekos/crates/identity/src/treasury.rs` |
| `ekos treasury scan` | `ekos/crates/cli/src/commands/treasury.rs` |
| `ekos_identity_review` accepts `AuthorizedBy` | `ekos/crates/cli/src/commands/mcp.rs` |
| Custom-kind registry rows (both `structurally_keyed: true`) | `ekos/crates/kir/src/custom_kinds.rs` |

**Enabling the connectors** (both soft-skip when unset, like GitHub/Confluence):
`EKOS_TREASURY_ADDRESS` + `EKOS_TREASURY_CHAIN_ID` (+ optional `EKOS_TREASURY_EXPLORER_URL`,
`EKOS_TREASURY_API_KEY`); `EKOS_SNAPSHOT_SPACE` (+ optional `EKOS_SNAPSHOT_HUB_URL`).

**Extraction refuses to guess.** A proposal body is prose. `approved_recipient` and
`approved_amount` are set only when the body contains exactly one distinct address / one distinct
`(amount, token)` pair; two of either leave the field unset, because a wrong value would make the
matcher assert something the proposal never said. `"$10k/mo"` (no token) is ignored.

**Verified.** Mock-client tests for both observers (mapping, determinism, outflow filtering, batch
sub-transfer ids, decimal scaling past `f64`'s exact range); scorer ground-truth tests including a
deliberate non-match, a before-approval case, a rejected proposal, a wrong-token case and a partial
tranche; analyzer tests for the property contract; and `crates/cli/tests/treasury_pipeline.rs`, which
runs the real observers (mock clients) through `build → recover → resolve → compile → commit →
treasury scan`, then asserts the `unconfirmed` `AuthorizedBy` relationships, the absence of one for
the unmatched payment, `ekos_state` showing the candidate, an `ekos_identity_review` confirmation, and
that a re-scan preserves it.

### Live verification (2026-09-22)

`SnapshotClient` has now been run against the real public hub. The tests are committed as
`ekos/plugins/governance/tests/live_snapshot.rs`, `#[ignore]`d so CI stays offline and
deterministic; the hub needs no API key, so they are reproducible by anyone:

```bash
cargo test -p ekos-plugin-governance --test live_snapshot -- --ignored --nocapture
```

What they confirmed, and what they found:

| | result |
|---|---|
| every field `parse_proposal` reads exists on a real node, with the assumed types (`created`/`end` integer unix seconds, `scores` an array of numbers) | ✅ confirmed on `ens.eth` |
| paging across a real page boundary — no repeats, `created ASC` ordering survives | ✅ confirmed on `uniswapgovernance.eth` (>100 proposals, 100 then 99) |
| a short page really is the end of results | ✅ confirmed — `ens.eth` returns 98 for `first: 100`, and `first: 1000` also returns 98 |
| **a wrong space id is not an error** | ❌ **found a silent failure** |

`aave.eth` (a real DAO whose space has since been renamed to `aavedao.eth`), a space id that never
existed, and an empty string all return **HTTP 200** with `{"data":{"proposals":[]}}` and no
GraphQL `errors` key — indistinguishable from a space that genuinely has no proposals. Left silent
this is worse than a missing connector: with zero proposals observed, `ekos treasury scan` reports
*every* payment as lacking governance approval, which is an actively wrong compliance answer rather
than an absent one.

`SnapshotObserver` now emits a `tracing::warn!` naming the space whenever the result is empty.
`TreasuryObserver` got the symmetric warning: `parse_explorer_result` correctly reads Etherscan's
`status: "0"` / "No transactions found" as an empty history rather than an error, so a valid-but-wrong
`EKOS_TREASURY_ADDRESS` or chain id looks exactly like a treasury that has never paid anyone.

**Not verified — read before relying on this.**
- `RealTreasuryClient` has **never been run against a live explorer**. Its parsing is unit-tested
  against the documented response shapes, but rate limits, pagination past the explorer's 10,000-row
  cap, chain-specific quirks and explorer casing differences are untested. This needs an explorer API
  key and a real treasury address.
- `SnapshotClient`'s live coverage is read-path only: field shapes, paging and the wrong-space case.
  Hub rate limiting under a long crawl, and the `max_pages` × 100 ceiling on a space larger than
  5,000 proposals, remain untested.
- The scoring weights and floor are untuned: there is no real DAO ground truth in this repository.
- Multi-send decoding depends on the explorer exposing internal transactions / token transfers; a batch
  the explorer does not decode appears as one opaque payment.
