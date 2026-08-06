# RFC 0032 — DAO Treasury Compliance: Payment↔Approval Matching

**Status:** Draft
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

- [ ] Which chain/explorer API ships first — Ethereum mainnet via Etherscan, a specific L2, or
      Solana via Solscan (materially different response shapes and asset models; Solana is also
      where EKOS's own token lives, per `TOKENOMICS.md`, which may make it the more natural first
      target)?
- [ ] Which governance-forum flavor ships first — Discourse or Snapshot? They have different data
      models (Discourse: free-text forum posts; Snapshot: structured off-chain votes with an
      on-chain-anchored result) and may need different `approved_amount`/`approved_recipient`
      extraction logic.
- [ ] What confidence floor and per-signal weights hold up against real DAO data? RFC 0029's
      values are reused here as a starting point, not validated for this domain.
- [ ] Should `ekos treasury scan`'s "K payments with zero candidates" watchlist be a distinct MCP
      tool/query, or is `ekos_ekl` (the existing structured query tool) sufficient to express
      "find `TreasuryPayment` with no outgoing `AuthorizedBy` relationship" without new surface?

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

- [ ] All Open Questions resolved.
- [ ] At least one review completed.
- [ ] `TreasuryObserver`/`TreasuryClient` and the governance connector each pass a
      `Mock*Client`-driven test suite with zero network dependency.
- [ ] `find_treasury_approval_candidates` passes the ground-truth fixture test described above.
- [ ] `ekos treasury scan` runs end-to-end against fixture data and produces evidenced,
      `unconfirmed`-status `AuthorizedBy` relationships queryable via existing MCP read tools.
- [ ] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants
      (append-only ledger, evidence-backed conclusions, read-only Runtime, no silent merges).
