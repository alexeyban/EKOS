# EKOS Tokenomics 2.0

EKOS is open-source knowledge infrastructure for AI agents. The token exists to reward and
coordinate contributors. It is not a claim on the project and not a promise of price.

This document separates three things clearly: what is live today, what is proposed, and what is
not committed. Feedback received during the review may change the final version published on
ekos.dev. See `VISION.md` for the longer-term ecosystem roadmap (community rewards →
plugin/agent/knowledge marketplaces → governance) the token's utility is designed to grow into as
the platform is adopted.

Network: Solana

Contract (Mint) Address: `CwubepDFJndzSKFmAMAm9u8Xx3PrizAwSq8hcGimpump`

Pump.fun: https://pump.fun/coin/CwubepDFJndzSKFmAMAm9u8Xx3PrizAwSq8hcGimpump

## Principles

Six rules guide every decision in this framework.

- **Transparency.** Every allocation, payout and pool has a public, verifiable record.
- **Contribution over hype.** Rewards go to work with real impact: code, tests, documentation,
  integrations. Promotion and social posts are not rewarded.
- **No new issuance.** Existing pools are managed, not expanded.
- **No self-dealing.** The founder keeps no rewards from community pools. The founder may stake
  like anyone else, but returns all staking rewards to Community Rewards.
- **No custody without safeguards.** Community funds are not accepted until escrow exists.
- **Honest status.** Nothing proposed is described as live.

## What the token is for

The value of $EKOS is meant to come from using it, not from a share of income. It has three
roles; only the first is live today.

| Role | What it means | Status |
|---|---|---|
| Rewards for work | Contributors earn EKOS for testing, development and documentation. The token is earned by doing, not bought in the hope of income. | Live |
| Payment and discounts | Paying in EKOS for Knowledge Packs or hosted MCP access, at a discount. Demand comes from the product, not from a promise. | Planned |
| Access | Early access to features, priority support and a say in roadmap choices for people who hold or stake EKOS. | Planned |

Planned roles will be announced, with their exact terms, when they go live.

## Total Supply

**967,410,000 EKOS** (current).

Originally minted at 1,000,000,000 EKOS; **32,590,000 EKOS has since been burned**, permanently
removing it from supply. All percentages in this document are relative to the current
967,410,000 supply.

**Circulating supply: ~949,410,000 EKOS (~98.1%)** — the total minus the 18,000,000 EKOS held in
Streamflow contracts (Founder Vesting 14M + Founder Lock 4M). Of those, ~15,666,700 EKOS is still
time-locked; the other 2.3333M EKOS of Founder Vesting is unlocked but not yet claimed (as of
2026-10-22), so it is still in the contract and not in circulation. Every other wallet listed below sits in
circulation: it is unlocked and spendable (the staking pool's tokens are held by the pool contract),
and in the founder's personal wallet's case is mostly tokens bought back on the open market.

_Burn transaction reference: to be recorded here (Solscan link + date)._

## Allocation

Every row is part of the 967,410,000 total supply — this table shows *where it sits*, not a
carve-out from a separate pool. About 97% is held by the market; founder and community allocations
together hold about 3%.

Snapshot as of **2026-10-22**; balances change on-chain.

| Holder | EKOS | % of Supply | Status |
|---|---|---|---|
| Public / market holders | ~938,360,000 | ~97.00% | Circulating — Pump.fun bonding curve / open market |
| Founder Vesting | 14,000,000 | 1.45% | **Vesting** — Streamflow, linear monthly; 2.3333M unlocked, not yet claimed |
| Founder Lock | 4,000,000 | 0.41% | **Locked until Jul 27, 2027** — Streamflow lock |
| Staking Pool | 4,000,000 | 0.41% | 3M reward budget + 1M founder stake ([StakePoint](https://stakepoint.app/embed/pool/cmvj8dje60000l308pgn2q79q?color=fb57ff&theme=dark)) |
| Founder Personal Wallet | 2,550,000 | 0.26% | Unlocked — mostly open-market buys |
| Bounty Fund | 2,500,000 | 0.26% | Bounty #2 expired, refund pending (see below) |
| Community Rewards | 2,000,000 | 0.21% | Held for distribution — separate Metamask wallet |
| Treasury | 0 | 0% | Fully allocated to the Bounty Fund on 2026-08-27 |
| **Total supply** | **967,410,000** | **100%** | |

**In Streamflow contracts: 18,000,000 EKOS (~1.9%)** — ~15,666,700 EKOS (~1.6%) still time-locked,
plus 2.3333M EKOS unlocked but unclaimed. Everything else is in circulation.

Founder custody total: 26,050,000 EKOS (~2.7% of supply) — Founder Vesting + Founder Lock +
personal wallet + the founder's 1,000,000 EKOS stake in the staking pool + Bounty Fund + Community
Rewards. This is a *transparency* figure, **not** locked or reserved supply: only the ~15,666,700
EKOS above is still time-locked, the Bounty Fund and Community Rewards are earmarked for community
distribution, and the personal wallet is mostly open-market buys. The staking pool's 3,000,000 EKOS
reward budget is not counted: it is paid out to stakers.

### How the pools changed since the first draft

- **Community Rewards, 5M → 2M:** 3M moved to fund the staking pool's reward budget.
- **Founder Personal Wallet, 3.55M → 2.55M:** 1M staked by the founder in the staking pool.
- **Bounty Fund:** bounty #1 (3.6M EKOS) was paid in full to 9 winners. The current 2.5M is
  bounty #2, funded from the founder's personal wallet; it expired and its refund returns to the
  Bounty Fund.

The "Public / market holders" figure is total supply minus the named wallets above; the
circulating-supply figure is total supply minus only the locked tokens. Neither is a separate
reserved pool.

## Founder Vesting

14,000,000 EKOS of the founder allocation is locked under a non-cancelable Streamflow vesting
contract:

- Wallet: `u2zUCiUHRoGp9jKRsyjMGQ8x9Z3UdtERm174aiXURZo`
- Total: 14,000,000 EKOS
- Unlock schedule: linear, monthly
- Unlock per month: 1.1666M EKOS (14,000,000 ÷ 12)
- Next unlock: Nov 3, 2026
- As of 2026-10-22: 2.3333M EKOS unlocked, 0 EKOS claimed (the unlocked tokens remain in the contract)

A further 4,000,000 EKOS is held in a separate Streamflow lock until Jul 27, 2027.

These figures are sourced from the live Streamflow contracts and may drift over time — the
contracts themselves are the source of truth.

## Community Rewards

The 2,000,000 EKOS Community Rewards pool pays for work that makes EKOS better, judged by evidence
of impact rather than activity counts.

| Track | Rewarded work | Evidence required |
|---|---|---|
| Testing | Running EKOS on a real or realistic system; reproducible bug reports; benchmark runs | GitHub issue with steps, logs or a test report |
| Development | Fixes, connectors, tooling and integrations outside formal bounties | Merged pull request |
| Documentation and education | Guides, tutorials, technical articles, walkthrough videos, translations | Published page or merged docs PR |
| Adoption | Introducing an architect, engineer, product owner or data leader who actively tests EKOS | The tester's confirmed feedback or report |

**How rewards are decided**

- **Task rewards:** published tasks with a fixed EKOS amount and clear acceptance criteria.
- **Recognition rewards:** for sustained, valuable work that fits no predefined task.
- The maintainer approves rewards. Community recommendations are welcome and are included in the
  monthly report.

**Rules**

- **Not rewarded:** promotion, social media posts, engagement campaigns, spam, AI-generated filler
  and duplicate reports.
- **Monthly cap:** at most _[X — TBD]_% of the remaining pool is paid out per month, so the pool
  shrinks gradually.
- **When the pool is empty,** rewards stop unless a new public proposal adds funding.

**Monthly report:** recipient (public name or handle, unless a justified privacy exception
applies), amount, reason, evidence link, transaction link, total paid in the month and remaining
balance.

## Bounties

Bounties pay for specific deliverables with published acceptance criteria. One bounty has been
paid in full; the second expired and its refund is pending.

| Bounty | Amount | Status | Result |
|---|---|---|---|
| #2 | 2,500,000 EKOS | Expired, refund pending | Refund returns to the Bounty Fund. [View](https://pump.fun/go/bb35a0d7-ac6d-43ab-b4d0-68a5518d57ec) |
| #1 "Make EKOS More Visible" | 3,600,000 EKOS + 0.25 SOL | Settled 2026-09-01 | ≈$1,256 paid to 9 winners. [View](https://pump.fun/go/d3beb920-3526-4394-9a19-13eee1500fd0) |

Bounty #1 was funded by the former Treasury (2,000,000 EKOS) and the founder's personal wallet
(1,600,000 EKOS). Bounty #2 was funded by the founder's personal wallet. Future bounties cover
development, testing, documentation and integrations; promotion is no longer a bounty category.

**Every bounty publishes, before it opens:**

1. The task and its objective.
2. Deliverables and acceptance criteria.
3. The amount in EKOS.
4. The deadline, normally two to three weeks.
5. How work is reviewed and approved.
6. What happens if it is not completed: extension, reassignment or cancellation with refund.

After review, the outcome and payment transaction are published. The maintainer gives final
approval.

**Community-funded bounties are not open yet.** EKOS will not accept community contributions to
bounties until an escrow mechanism with independent safeguards is live and publicly documented.

### Bounty Fund history

On 2026-08-27 the Treasury (2,000,000 EKOS) and 1,600,000 EKOS from the Founder — Personal Wallet
were consolidated into a dedicated **Bounty Fund** wallet, to finance the "Make EKOS More Visible"
activity and future community bounties.

| Date | Amount | From | To | Reference |
|---|---|---|---|---|
| 2026-08-27 | 2,000,000 EKOS | Treasury | Bounty Fund | https://pump.fun/go/d3beb920-3526-4394-9a19-13eee1500fd0 |
| 2026-08-27 | 1,600,000 EKOS | Founder — Personal Wallet | Bounty Fund | https://pump.fun/go/d3beb920-3526-4394-9a19-13eee1500fd0 |

Separately, on 2026-08-27 the founder purchased 2,000,000 EKOS on the open market (Pump.fun). After
the 1,600,000 EKOS bounty contribution and this purchase, the Founder — Personal Wallet held
3,550,000 EKOS — the ~90,000 EKOS below the arithmetic (3,240,000 − 1,600,000 + 2,000,000) is
trading fees and slippage on the purchase.

**Bounty #1 — "Make EKOS More Visible":** published Sunday, August 23, 2026 — active through
Sunday, August 30, 2026. The full 3,600,000 EKOS was paid out to 9 winners
([bounties](https://pump.fun/profile/EKOSBuilder?tab=bounties)) on 2026-09-01.

**Bounty #2:** 2,500,000 EKOS, funded from the Founder Personal Wallet and placed in the Pump.fun
bounty program ([bounty #2](https://pump.fun/go/bb35a0d7-ac6d-43ab-b4d0-68a5518d57ec)). It
expired; the refund is pending and returns to the Bounty Fund.

Bounty Fund balance (2026-10-22): 2,500,000 EKOS — bounty #2.

## Contributor vesting (proposed)

Larger rewards will be paid through non-cancelable vesting schedules, so contributors are rewarded
over time and commitments are visible on-chain.

- **Scope:** rewards and bounties above _[threshold — TBD]_ EKOS. Smaller rewards are paid directly.
- **Split:** _[30 — TBD]_% paid on approval, the rest vested.
- **Schedule:** six months, weekly unlocks, to the contributor's own wallet.
- **Non-cancelable:** once created, a schedule cannot be withdrawn.
- **Funding:** the existing Community Rewards and Bounty Fund pools. No new tokens are created.
- **Pilot:** the first qualifying contributor reward, published with its schedule link.

Voluntary community top-ups and a shared contributor treasury may follow later. They depend on
escrow being in place and on confirming what the vesting platform supports.

## Staking pool

A public $EKOS staking pool is live on StakePoint, created and funded on 2026-10-22, paying EKOS
rewards from a fixed 3,000,000 EKOS budget over 90 days. The founder also stakes 1,000,000 EKOS in
it as a regular participant.

| Parameter | Value |
|---|---|
| Reward budget | 3,000,000 EKOS, from Community Rewards |
| Founder stake | 1,000,000 EKOS from the founder's personal wallet, staked on the same terms as everyone |
| Pool duration | 90 days (from 2026-10-22) |
| Minimum lock | 14 days |
| Reward token | EKOS |

[Stake on StakePoint](https://stakepoint.app/pool/cmvj8dje60000l308pgn2q79q) — the pool is also
embedded on [ekos.dev/token](https://ekos.dev/token/#staking).

Rewards come only from this budget; no new tokens are created. The APR depends on the total amount
staked and is not guaranteed. When the 90 days end, the pool closes unless it is prolonged. The
founder may top up the reward budget from the personal wallet; top-ups are announced and reported.
Any extension or new pool will be announced as a separate proposal.

## Treasury and reporting

The treasury holds 0 EKOS, and EKOS has no revenue. Any future treasury will be announced publicly
with its address and rules.

Every report distinguishes completed activity from approved plans and from mechanisms still in
development. For each reward, bounty or pool movement it states:

- What happened and why.
- The amount.
- The recipient, unless a justified privacy exception applies.
- The supporting evidence.
- The on-chain transaction.
- The remaining balance of the pool.

Privacy exceptions are allowed, but never as a reason to leave the use of community funds
unexplained.

## Governance

The maintainer makes final decisions today; the community is consulted on every major economic
change before it happens.

- **Consultation now:** changes to allocations, reward rules, bounties, staking and treasury are
  published for discussion before they take effect, whenever practical.
- **Voting later:** a structured voting system will be designed in its own public RFC, covering
  eligibility, quorum, thresholds and platform.
- **Whale safeguards:** since about 97% of supply is held by the market, the voting design must
  prevent a few large wallets from deciding alone. Founder wallets will not vote on reward or
  bounty decisions.

Until voting is live, community feedback is advisory, not a binding vote.

## For product owners and data leaders

You don't need the token to use or test EKOS. It is open source, and we are looking for teams that
want to try it on real systems.

**The problem it addresses.** AI assistants answer questions about your systems without showing
where the answer came from. Before anyone acts on those answers, they need evidence and
traceability.

**What EKOS does.** It reads application code, SQL schemas, ETL pipelines, git history and
documentation, and compiles them into a versioned fact ledger. Every fact records its source file,
line, extractor and confidence. Agents and people query it through MCP tools, a CLI or a web
console.

**Questions it helps answer:**

- What breaks downstream if this table or column changes?
- Where does this metric come from, and which pipelines touch it?
- What do our legacy ETL jobs actually do, and what would a migration need to cover?

**Early testing.** Teams that test EKOS on their own systems get direct access to the maintainer
and a say in the roadmap. In return, we ask for honest feedback on what works and what doesn't.
Contact: _[email / link — TBD]_.

## Implementation status

Four items are live today; the rest are proposals.

| Item | Status |
|---|---|
| Founder vesting and lock (Streamflow, non-cancelable) | Live |
| Staking pool (3M EKOS rewards, 90 days) | Live |
| Bounty #1 payout to 9 winners | Completed |
| Community Rewards and Bounty Fund pools | Live |
| Monthly transparency reports | Proposed |
| Contributor vesting for larger rewards | Proposed |
| Escrow for community-funded bounties | Proposed — not built |
| Community voting system | Proposed — separate RFC |
| Buyback & Burn | Proposed — mechanism not yet designed |

Publishing this framework does not make any proposed item active. Each one will be announced when
it goes live.

## Disclaimer

$EKOS is a community token used to reward contributions to the EKOS open-source project.

- **A community token, not a share.** $EKOS works differently from company shares. It does not
  represent ownership of EKOS, and holding it does not come with a portion of the project's
  revenue or profits.
- **No price commitments.** Nothing here is a promise about the token's price, liquidity or future
  value. Token prices are volatile and can fall to zero.
- **Buyback & Burn is proposed but not designed.** No mechanism, schedule or amount has been
  decided. Any such mechanism would need revenue, legal review and a separate public proposal.
  Future investment agreements or enterprise contracts may restrict buybacks, or the public
  disclosure of revenue, contract values or investment amounts; such restrictions take precedence.
- **Proposals are not commitments.** Mechanisms marked proposed, planned or not committed are not
  active and may change or be dropped.
- **Figures change.** Balances above are snapshots, kept up to date on a best-effort basis; the
  on-chain contracts and wallets are the source of truth.
- **Not advice.** Nothing here is financial, investment, legal or tax advice. Check the rules in
  your jurisdiction before acquiring or holding crypto assets.
