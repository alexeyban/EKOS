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
circulation: it is unlocked and spendable, and in the founder's personal wallet's case is mostly
tokens bought back on the open market.

_Burn transaction reference: to be recorded here (Solscan link + date)._

## Allocation

Every row is part of the 967,410,000 total supply — this table shows *where it sits*, not a
carve-out from a separate pool.

| Holder | Quantity | % of Supply | Status |
|---|---|---|---|
| Public / market holders | ~937,260,000 | ~96.9% | In circulation — Pump.fun bonding curve / open market |
| Founder — Vesting | 14,000,000 | ~1.45% | **Locked** — Streamflow vesting, linear monthly |
| Community Rewards | 5,000,000 | ~0.52% | In circulation — undistributed, separate Metamask wallet |
| Founder Lock | 4,000,000 | ~0.41% | **Locked** — Streamflow lock |
| Bounty Fund | 3,600,000 | ~0.37% | In circulation — community bounty wallet (see below) |
| Founder — Personal Wallet | 3,550,000 | ~0.37% | In circulation — mostly open-market buys |
| Treasury | 0 | — | Fully allocated to the Bounty Fund on 2026-08-27 |

**Locked total: 18,000,000 EKOS (~1.9%).** Everything else is in circulation.

Founder custody total: 30,150,000 EKOS (~3.1% of supply) — Founder Vesting + Founder Lock +
Community Rewards + Bounty Fund + personal wallet. This is a *transparency* figure for what the
founder can currently move, **not** locked or reserved supply: only the 18,000,000 EKOS above is
locked, the Bounty Fund and Community Rewards are earmarked for community distribution, and the
personal wallet is mostly open-market buys. It is up from the earlier ~2.8% figure partly from a
2,000,000 EKOS open-market purchase by the founder on 2026-08-27 (see Bounty Fund & Disbursements
below), and partly because the burn shrank the denominator.

The "Public / market holders" figure is total supply minus the named wallets above; the
circulating-supply figure is total supply minus only the locked tokens. Neither is a separate
reserved pool.

## Founder Vesting

14,000,000 EKOS of the founder allocation is locked under a Streamflow vesting contract:

- Wallet: `u2zUCiUHRoGp9jKRsyjMGQ8x9Z3UdtERm174aiXURZo`
- Total: 14,000,000 EKOS
- Unlock schedule: linear, monthly
- Unlock per month: 1,166,600 EKOS (14,000,000 ÷ 12)
- Next unlock: Sep 4, 2026, 1:31 AM GMT+2
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

Bounty Fund balance: 3,600,000 EKOS.

Separately, on 2026-08-27 the founder purchased 2,000,000 EKOS on the open market (Pump.fun). After
the 1,600,000 EKOS bounty contribution and this purchase, the Founder — Personal Wallet holds
3,550,000 EKOS — the ~90,000 EKOS below the arithmetic (3,240,000 − 1,600,000 + 2,000,000) is
trading fees and slippage on the purchase.

**"Make EKOS More Visible" bounty window:** published Sunday, August 23, 2026 — active through
Sunday, August 30, 2026.

## Distribution Principles

TBD — general principles governing distribution (e.g. eligibility, timing, caps) have not
yet been finalized. This section will be updated once decided.

## Disclaimer

Figures above are subject to change as the project evolves and will be kept up to date on
a best-effort basis. This document is informational only and does not constitute financial
advice.
