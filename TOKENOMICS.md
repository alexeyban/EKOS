# EKOS Tokenomics

See `VISION.md` for what the token is *for* — the phased ecosystem roadmap (community rewards →
plugin/agent/knowledge marketplaces → governance) the token's utility is designed to grow into as
the platform is adopted. This document covers supply, allocation, and vesting facts only.

Network: Solana

Contract (Mint) Address: `CwubepDFJndzSKFmAMAm9u8Xx3PrizAwSq8hcGimpump`

Pump.fun: https://pump.fun/coin/CwubepDFJndzSKFmAMAm9u8Xx3PrizAwSq8hcGimpump

## Total Supply

**967,410,000 EKOS** (current).

Originally minted at 1,000,000,000 EKOS; **32,590,000 EKOS has since been burned**, permanently
removing it from supply. All percentages in this document are relative to the current
967,410,000 supply.

**Circulating supply: ~949,410,000 EKOS (~98.1%)** — the total minus the only 18,000,000 EKOS that
is time-locked (Founder Vesting 14M + Founder Lock 4M). Every other wallet listed below sits in
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
| Founder Vesting | 14,000,000 | 1.45% | **Vesting** — Streamflow, linear monthly |
| Founder Lock | 4,000,000 | 0.41% | **Locked until Jul 27, 2027** — Streamflow lock |
| Staking Pool | 4,000,000 | 0.41% | 3M reward budget + 1M founder stake ([StakePoint](https://stakepoint.app/embed/pool/cmvj8dje60000l308pgn2q79q?color=fb57ff&theme=dark)) |
| Founder Personal Wallet | 2,550,000 | 0.26% | Unlocked — mostly open-market buys |
| Bounty Fund | 2,500,000 | 0.26% | Bounty #2 refund pending (see below) |
| Community Rewards | 2,000,000 | 0.21% | Held for distribution — separate Metamask wallet |
| Treasury | 0 | 0% | Fully allocated to the Bounty Fund on 2026-08-27 |
| **Total supply** | **967,410,000** | **100%** | |

**Locked total: 18,000,000 EKOS (~1.9%).** Everything else is in circulation.

Founder custody total: 26,050,000 EKOS (~2.7% of supply) — Founder Vesting + Founder Lock +
personal wallet + the founder's 1,000,000 EKOS stake in the staking pool + Bounty Fund + Community
Rewards. This is a *transparency* figure, **not** locked or reserved supply: only the 18,000,000
EKOS above is locked, the Bounty Fund and Community Rewards are earmarked for community
distribution, and the personal wallet is mostly open-market buys. The staking pool's 3,000,000 EKOS
reward budget is not counted: it is paid out to stakers.

### How the pools changed since the first draft

- **Community Rewards, 5M → 2M:** 3M moved to fund the staking pool's reward budget.
- **Founder Personal Wallet, 3.55M → 2.55M:** 1M staked by the founder in the staking pool.
- **Bounty Fund:** bounty #1 (3.6M EKOS) was paid in full to 9 winners. The current 2.5M is
  bounty #2, funded from the founder's personal wallet and now on hold in the Pump.fun bounty
  program; its expected refund returns to the Bounty Fund.

The "Public / market holders" figure is total supply minus the named wallets above; the
circulating-supply figure is total supply minus only the locked tokens. Neither is a separate
reserved pool.

## Founder Vesting

14,000,000 EKOS of the founder allocation is locked under a Streamflow vesting contract:

- Wallet: `u2zUCiUHRoGp9jKRsyjMGQ8x9Z3UdtERm174aiXURZo`
- Total: 14,000,000 EKOS
- Unlock schedule: linear, monthly
- Unlock per month: 1,166,600 EKOS (14,000,000 ÷ 12)
- Next unlock: Nov 3, 2026
- As of writing: 0 EKOS claimed, 0 EKOS unlocked

These figures are sourced from the live Streamflow vesting contract and may drift over
time — the contract itself is the source of truth.

## Bounty Fund & Disbursements

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
([bounties](https://pump.fun/profile/EKOSBuilder?tab=bounties)), in late August 2026 (around
August 24; the exact day was not recorded).

**Bounty #2:** 2,500,000 EKOS, funded from the Founder Personal Wallet. It sits in the Pump.fun
bounty program ([bounty #2](https://pump.fun/go/bb35a0d7-ac6d-43ab-b4d0-68a5518d57ec)), which is on
hold; a refund is expected and returns to the Bounty Fund.

Bounty Fund balance (2026-10-22): 2,500,000 EKOS — bounty #2.

## Staking Pool

4,000,000 EKOS sits in a [StakePoint staking pool](https://stakepoint.app/embed/pool/cmvj8dje60000l308pgn2q79q?color=fb57ff&theme=dark),
funded before the 2026-10-22 snapshot:

| Amount | From | Purpose |
|---|---|---|
| 3,000,000 EKOS | Community Rewards | Reward budget, paid out to stakers |
| 1,000,000 EKOS | Founder Personal Wallet | Founder's own stake |

The pool is also embedded on [ekos.dev/token](https://ekos.dev/token/#staking).

## Distribution Principles

TBD — general principles governing distribution (e.g. eligibility, timing, caps) have not
yet been finalized. This section will be updated once decided.

## Disclaimer

Figures above are subject to change as the project evolves and will be kept up to date on
a best-effort basis. This document is informational only and does not constitute financial
advice.
