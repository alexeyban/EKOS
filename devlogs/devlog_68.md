# Devlog 68 — Separating the technical pitch from token materials

**Date:** 2026-08-21
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Roadmap Priority 4 flagged a real positioning risk: README/pitch materials that could read as "a
crypto project wearing a dev-tools costume" to an enterprise buyer or technical reviewer evaluating
the compiler/ledger architecture on its own merits. Investigated the actual current state before
assuming anything needed inventing from scratch — `docs/index.html` (the live public site) already
had the right restrained pattern; `README.md` was the one place that didn't follow it.

---

## What was actually there

`docs/index.html`'s token section was already well-designed: its own CSS carries a
`/* token (deliberately minimal) */` comment, the section sits near the bottom of the page (after
Community, before Presentations), the copy is one restrained sentence ("a consequence of usage,
not a promise of price"), and it links out to `VISION.md`/`TOKENOMICS.md` with no raw contract
address or trading link on the page itself.

`README.md` didn't match that. Its bottom ran three separate `##` headers back to back —
`Official EKOS Token` (raw Solana contract address + a direct pump.fun trading link), `Official
Channels`, `Founder Vesting Wallet` (a raw wallet address) — with no restraint applied, right
before the closing `Versioning Roadmap`/`License` sections. Checked `TOKENOMICS.md` directly: the
contract address and pump.fun link were **already there**, verbatim. The README section was adding
real "crypto project" signal without adding any information `TOKENOMICS.md` didn't already have —
pure duplication, and duplication that can drift (two copies of a contract address is one more
place for a typo to create a real, dangerous mismatch). The one genuinely new fact — the founder
vesting wallet address — had nowhere else it belonged except stranded in the architecture README.

## What changed, and what deliberately didn't

Consolidated the three headers into one `## Token & Community` section, in the exact same position
(right before `Versioning Roadmap`) — not moved earlier, not made more prominent. No raw contract
address or pump.fun link inline anymore; a single link to `TOKENOMICS.md` covers verification
without repeating data that can go stale in two places. Moved the vesting wallet address into
`TOKENOMICS.md`'s existing `## Founder Vesting` section, next to the unlock schedule it already
documents. Reworded the one-sentence "About" mention to match the site's own "consequence of
usage, not a promise of price" framing, so the two properties say the same thing instead of two
independently-drifted paraphrases.

**Deliberately not done**: didn't remove the token/contract-address information from public
materials entirely. For a real on-chain token, the README/TOKENOMICS.md pairing also serves as the
anti-impersonation-scam reference point — the place someone checks to confirm a contract address
against a fake clone project. The fix is de-duplication and restraint in *placement*, not deleting
legitimate verification information. `docs/index.html`, `PIONEER_PROGRAM.md`, and `VISION.md` were
untouched — all three were already appropriately separate, reached only via deliberate links, never
mixed into the technical pitch.

---

## Knowledge Captured

- **Check what's actually there before assuming a rewrite is needed.** The site-level pattern
  (`docs/index.html`) already solved this problem correctly; the fix was bringing README.md up to
  an existing, already-validated standard, not inventing a new one.
- **Duplicated canonical facts (a contract address in two files) are a real drift risk, not just a
  style nit** — one link to a single source of truth is strictly safer than two copies that could
  quietly diverge.
- **"Remove the crypto costume" and "delete the crypto information" are different asks** — a real
  token's contract address is also the mechanism by which someone can verify they're not looking at
  a scam. Restraint in framing and placement is the right lever, not deletion.

---

## Files Changed

| File | Change summary |
|---|---|
| `README.md` | Three token/channel headers consolidated into one `## Token & Community` section; "About" section's token sentence reworded to match site framing |
| `TOKENOMICS.md` | Added the founder vesting wallet address to the existing `## Founder Vesting` section |
| `devlogs/devlog_68.md` | This file |
