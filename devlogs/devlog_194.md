# Devlog 194 — DAO treasury compliance (RFC 0032) + lock-free snapshot reads (RFC 0112)

**Date:** 2026-09-19 (live Snapshot verification appended 2026-09-22)
**PRs:** none (local `main`, `[skip ci]`)
**Branch:** main

---

## Summary
Two independent features, written the same week and held back together. RFC 0032 adds DAO treasury
compliance — "was this on-chain payment approved by governance?" — as two new connectors, two
recovery passes, a scorer and `ekos treasury scan`, reusing RFC 0029's reviewable-hypothesis shape
rather than inventing a second one. RFC 0112 makes a long-lived read-only `FactLedger` handle pick
up another process's commits without locking or a cold reopen, which is what lets `ekos mcp serve`
stay up across a rebuild. Written late: this devlog was referenced by `TODO.md` and both RFCs for
three days before it existed.

---

## RFC 0032 — DAO treasury compliance: payment ↔ approval matching

### Problem / motivation
A DAO multisig pays 50,000 USDC to an address. A contributor asks whether governance approved it.
Today that is answered by hand, cross-referencing a block explorer against a forum thread, and it
leaves no durable record. Architecturally this is RFC 0029's problem again: two independently
observed records that plausibly refer to the same fact, with no foreign key between them, where
getting the link wrong is worse than leaving it unresolved — so a match is written as an explicit,
reviewable hypothesis, never as an observed fact.

### What was built

| Component | Where |
|---|---|
| `TreasuryObserver`, `TreasuryClient`, `RealTreasuryClient` (Etherscan-family), `MockTreasuryClient` | `ekos/plugins/treasury` |
| `SnapshotObserver`, `GovernanceClient`, `SnapshotClient`, `MockGovernanceClient`, outcome derivation | `ekos/plugins/governance` |
| `TreasuryAnalyzerPass` → `Custom("TreasuryPayment")`, `GovernanceAnalyzerPass` → `Custom("GovernanceProposal")` | `crates/recovery/src/{treasury,governance}_analyzer.rs` |
| `find_treasury_approval_candidates`, `score_pair`, `payments_without_candidate` | `crates/identity/src/treasury.rs` (524 lines) |
| `ekos treasury scan` | `crates/cli/src/commands/treasury.rs` |
| `ekos_identity_review` accepts `AuthorizedBy` as well as `SameAs` | `crates/cli/src/commands/mcp.rs` |
| Custom-kind registry rows, both `structurally_keyed: true` | `crates/kir/src/custom_kinds.rs` |

Enabled by env var, soft-skipping when unset exactly like GitHub/Confluence:
`EKOS_TREASURY_ADDRESS` + `EKOS_TREASURY_CHAIN_ID` (+ optional `EKOS_TREASURY_EXPLORER_URL`,
`EKOS_TREASURY_API_KEY`), and `EKOS_SNAPSHOT_SPACE` (+ optional `EKOS_SNAPSHOT_HUB_URL`).

### Implementation details worth remembering
- **Each sub-transfer of a Safe multi-send is its own payment**, keyed `chain:<id>:tx:<hash>:<index>`.
  One batch transaction paying five recipients is five payments, because "was *this* payment
  approved" is a question about a recipient and an amount, not about a transaction hash.
- **Extraction refuses to guess.** `approved_recipient` / `approved_amount` are set only when the
  proposal body contains exactly one distinct address / one distinct `(amount, token)` pair. Two of
  either leaves the field unset — a wrong value would make the matcher assert something the proposal
  never said. `"$10k/mo"` (no token) is ignored.
- **A payment made before its approval is flagged and heavily penalised**, not silently scored on
  recipient and amount alone. A rejected proposal is never offered as a candidate at all.
- Decimal scaling is done past `f64`'s exact range — token values are not floats.

### Decisions
- **Reuse `AuthorizedBy` through `ekos_identity_review` rather than a new review tool.** The review
  UX, the `unconfirmed` status convention and the append-only re-write path already existed; a
  second tool would have been a second thing to keep honest.
- **Snapshot before Discourse, EVM explorers before Solana.** Snapshot's votes are structured;
  Discourse posts are free-text prose, which is a different (harder) extraction problem. Solana is
  deliberately v1-out despite EKOS's own token living there.

---

## RFC 0112 — Lock-free snapshot reads for `FactLedger`

### Problem / motivation
`ekos mcp serve` holds a read-only store open for the life of the process. Before this, its only
way to notice that a separate `build → … → commit` had run was RFC 0097's `walkdir` fingerprint of
the whole store on every call, followed by a full cold reopen on any change. That is the wrong cost
in both directions: expensive when nothing changed, and far more expensive than necessary when one
batch was appended.

### What was built

| Component | Where |
|---|---|
| `KnowledgeStore::refresh_snapshot` + `RefreshOutcome`, defaulting to `Unsupported` | `crates/ledger/src/lib.rs` |
| `SegmentStore::refresh_read_only` — `Unchanged` / new batches / "rebuild from cold" | `crates/ledger/src/segment/mod.rs` |
| Tail-only `provenance.jsonl` re-read tracking bytes already folded in | `crates/ledger/src/fact_ledger.rs` (+326) |
| Reader-side tantivy `reader.reload()` reporting whether the generation moved | `crates/ledger/src/search.rs` |
| `StoreCache` refreshes in place instead of fingerprinting | `crates/cli/src/commands/mcp.rs` |
| `ledger_refresh` Criterion bench | `benchmark/benches/ledger_refresh.rs` |

### Implementation details worth remembering
- **The default is `RefreshOutcome::Unsupported`, not "no change".** SQLite readers get WAL
  isolation natively, and partitioned/distributed stores keep RFC 0097's fingerprint. A default of
  "nothing changed" would have silently pinned those readers forever.
- **Only whole newline-terminated `provenance.jsonl` lines are consumed.** A line a writer is
  mid-way through appending is left for the next call rather than half-parsed and lost — the same
  torn-line discipline the session inbox needed.
- **A seal, a manifest/dictionary change or a shrunken file is not a refresh**, it is a cold
  rebuild. The cheap path is only for the pure active-segment append.
- `reader.reload()` is reader-side: it re-reads `meta.json` and never takes the writer lock, so it
  cannot contend with a concurrent writer.

### Benchmark (5k objects, re-run 2026-09-22)

| | before | after |
|---|---|---|
| nothing changed | 103.9 µs (`walkdir` fingerprint) | **27.7 µs** (one `HEAD` read + one `stat`) |
| one appended batch | 832 µs (full cold reopen) | **109.5 µs** (decode the appended tail) |

Live-verified with a long-lived `ekos mcp serve` against a separate `build → … → commit` process.

---

## Live verification of `SnapshotClient` (2026-09-22)

The RFC shipped saying both real clients had never been run against a live endpoint. The Snapshot
hub is public and needs no API key, so that half is now closed — three `#[ignore]`d tests in
`plugins/governance/tests/live_snapshot.rs`, reproducible with
`cargo test -p ekos-plugin-governance --test live_snapshot -- --ignored`.

Confirmed: every field `parse_proposal` reads exists on a real node with the assumed types
(`created`/`end` integer unix seconds, `scores` an array of numbers); paging across a real boundary
repeats nothing and preserves `created ASC` (`uniswapgovernance.eth`, 100 then 99); a short page
really is the end (`ens.eth` returns 98 for both `first: 100` and `first: 1000`).

Found: **a wrong space id is not an error.** `aave.eth` — a real DAO whose space has since been
renamed to `aavedao.eth` — plus a never-existent id and an empty string all return HTTP 200 with
`{"data":{"proposals":[]}}` and no GraphQL `errors` key. Both observers now warn on an empty result.

---

## Knowledge Captured

- **Zero observed records is the dangerous answer for a compliance feature, not the safe one.** A
  wrong `EKOS_SNAPSHOT_SPACE` yields no proposals, and with no proposals `ekos treasury scan`
  reports *every* payment as lacking governance approval. That is an actively wrong answer wearing
  the clothes of a cautious one. The same shape has now bitten this repo three times — `observe`
  paths silently killing the git observer, a wrong SQL dialect silently producing 0 tables, and now
  this. Any connector that can return an empty set should say so out loud.
- **Snapshot's hub reports a renamed or misspelled space as an empty list, never an error.** There
  is no "unknown space" response to detect. The only signal available is the emptiness itself.
- **A short page from the Snapshot hub genuinely is the end.** `ens.eth` returns 98 rows for
  `first: 100` and also 98 for `first: 1000`, so the client's `batch.len() < PAGE` stop condition is
  correct — worth recording because 98-for-100 looks exactly like a bug until you check.
- **ENS governance mostly is not on Snapshot.** `ens.eth` has ~98 Snapshot proposals total; the bulk
  runs on-chain through Tally. Do not use proposal count as a proxy for how active a DAO is.
- **`RefreshOutcome::Unsupported` beats a `false` default.** A trait method that answers a freshness
  question must distinguish "I checked, nothing changed" from "I cannot answer this" — collapsing
  the two pins every backend that did not implement it.
- **A devlog written three days late is a devlog three files of context poorer.** `TODO.md` and both
  RFCs referenced `devlog_194` before it existed, so anything following those references hit a dead
  end. The RFC status lines carried the load in the meantime, which is why they were worth keeping
  precise.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/plugins/treasury/` | new connector — observer, client trait, Etherscan-family + mock clients |
| `ekos/plugins/governance/` | new connector — observer, client trait, Snapshot + mock clients, outcome derivation |
| `ekos/plugins/governance/tests/live_snapshot.rs` | new — 3 `#[ignore]`d live hub tests |
| `ekos/crates/recovery/src/{treasury,governance}_analyzer.rs` | new recovery passes |
| `ekos/crates/identity/src/treasury.rs` | new — candidate scoring, `payments_without_candidate` |
| `ekos/crates/cli/src/commands/treasury.rs` | new — `ekos treasury scan` |
| `ekos/crates/cli/tests/treasury_pipeline.rs` | new — full-pipeline fixture test |
| `ekos/crates/cli/src/{app.rs,commands/mod.rs,commands/build.rs,commands/recover.rs}` | connector + subcommand wiring, env-var gating |
| `ekos/crates/ledger/src/{lib.rs,segment/mod.rs,fact_ledger.rs,search.rs}` | RFC 0112 refresh seam, tail re-read, reader reload |
| `ekos/crates/cli/src/commands/mcp.rs` | `StoreCache` refreshes in place; `ekos_identity_review` accepts `AuthorizedBy` |
| `benchmark/benches/ledger_refresh.rs` | new — RFC 0112 cost benchmark |
| `ekos/docs/rfcs/0032-…`, `0112-…` | status + implementation notes; RFC 0032 gains "Live verification" |
| 36 other `ekos/docs/rfcs/*.md` | mechanical `Status: Draft` → `Accepted — implemented (verified against the code 2026-09-19)` |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | treasury capability, RFC 0112 entry, status |
