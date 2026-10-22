# Devlog 285 — Red Audit and CI fixed; $EKOS allocation snapshot and staking pool

**Date:** 2026-10-22 (CI work late on 2026-10-21)
**PRs:** none (local `main`, pushed): `7acc102`, `87e2cc5`, `3347f36`, `b95b351`, `29a2004`, `1d3fb78`;
site repo `alexeyban/ekos-webpage`: `1b561f9`, `19ffc47`, `9aaec6d` and two before them
**Branch:** EKOS main; ekos-webpage main (deployed to ekos.dev)

---

## Summary

Two GitHub Actions workflows went red without any change on our side. **Audit** failed because
RustSec published three certificate-trust advisories against `rustls-webpki` and the bundled
WebPKI roots. **CI**'s web console jobs failed for two reasons: the macOS runner image moved its
default Python to 3.14, and some RFC 0207 code that landed under `[skip ci]` had ruff errors. All
three are fixed. Both workflows are green again (CI run 39660242789, all six jobs).

The rest of the session was the `$EKOS` token: a StakePoint staking pool embedded on
ekos.dev/token, and the allocation table, its notes and `TOKENOMICS.md` brought to a dated
2026-10-22 snapshot. The founder supplied each figure and date.

---

## `7acc102` — RUSTSEC-2026-0381/0382/0383

### Problem / motivation
Audit run 39493515952 failed in all three workspaces. Certificate verification could accept
distrusted emSign intermediates, and the bundled root sets still trusted them:

| Advisory | Crate | Was | Now |
|---|---|---|---|
| RUSTSEC-2026-0382 | `rustls-webpki` | 0.103.15 | 0.103.16 |
| RUSTSEC-2026-0381 | `webpki-roots` | 1.0.8 / 1.0.9 | 1.0.10 |
| RUSTSEC-2026-0383 | `webpki-root-certs` | 1.0.9 | 1.0.10 (`ekos/` only) |

### What was built
`cargo update -p …` in `ekos/`, `tests/integration/` and `benchmark/`. These are semver-compatible
bumps, so only the lockfiles changed. `scripts/audit.sh` is clean and
`cargo check --workspace --all-targets` passes.

### Decisions
- Lockfile bumps only, no `Cargo.toml` changes. The fixed versions are inside the existing ranges.
- `webpki-root-certs` exists only in the `ekos/` graph. `cargo update -p` fails with "did not match
  any packages" in the other two workspaces, so each workspace needs its own crate list.

## `87e2cc5` — Console CI: Python 3.12 pin and ruff

### Problem / motivation
CI run 39655621887 failed in two jobs, for unrelated reasons:
1. **Web console API (macos-14):** `uv sync --frozen --no-build` refused `fa2-modified==0.4`
   ("marked as `--no-build` but has no binary distribution"). The runner now picks Homebrew
   CPython 3.14.7, and `fa2-modified` 0.4 has wheels for cp312/cp313 only.
2. **Web console (Linux), Ruff:** six E501 lines (the `app/routes/extensions.py` module docstring
   and five lines in `tests/test_extensions.py`), and one file that `ruff format` would rewrite.
   All came from RFC 0207 work committed with `[skip ci]`.

### What was built
| File | Change |
|---|---|
| `.github/workflows/ci.yml` | job-level `env: UV_PYTHON: "3.12"` on `web` and `web-os`, with a comment explaining it |
| `web/api/app/routes/extensions.py` | docstring rewrapped to 100 columns |
| `web/api/tests/test_extensions.py` | `ruff format` |

Locally: ruff check and format are clean, `tests/test_extensions.py` passes (15), and a
wheels-only 3.12 sync into a scratch venv succeeds.

### Decisions
- **Pin, not loosen.** The alternatives were dropping `--no-build` or replacing `fa2-modified`.
  Building from source in CI is what `--no-build` exists to prevent, and 3.12 is the console's
  stated CI Python (CLAUDE.md). `UV_PYTHON` works on every OS, and uv fetches a managed 3.12 if the
  runner lacks one.
- **Job-level env, not `setup-uv`'s `python-version` input.** The env var does not depend on which
  inputs the pinned `setup-uv` SHA supports.

---

## ekos.dev/token and `TOKENOMICS.md` — 2026-10-22 snapshot

### Problem / motivation
The founder created a StakePoint staking pool (2026-10-22, 90-day term, renewable). They funded it
with 3M EKOS from Community Rewards as the reward budget and 1M EKOS of their own as a stake.
Bounty #1 had been paid out and bounty #2 opened, so the published allocation was out of date.

### What was built
| Where | Change |
|---|---|
| `token/index.html` (site) | StakePoint pool iframe (`#staking`, the founder added it); allocation table rewritten with per-row status (Vesting (Streamflow), Locked until Jul 27 2027, 3M reward budget + 1M founder stake, Unlocked, Bounty #2 refund pending, Held for distribution), a Total row, snapshot date, "How the pools changed" list |
| `token/index.html` CSS | phone layout fix, see Knowledge Captured |
| `TOKENOMICS.md` | same table; custody 30.15M → 26.05M (~2.7%); bounty #1 paid in full to 9 winners on 2026-09-01; bounty #2 2.5M from the founder's wallet, on hold in Pump.fun's bounty program, refund expected; new Staking Pool section; vesting: next unlock Nov 3 2026, 1.1666M/month, 2.3333M unlocked and not yet claimed |

The final figures (967.41M total): public ~938.36M, Founder Vesting 14M, Founder Lock 4M,
Staking Pool 4M, personal wallet 2.55M, Bounty Fund 2.5M, Community Rewards 2M, Treasury 0.

### Decisions
- **18M is "held in Streamflow", not "time-locked".** 2.3333M of the vesting has unlocked but has
  not been claimed. Calling all 18M locked would be false. Counting the 2.3333M as circulating
  would also be false, because it is still in the contract. The page and the doc now split it:
  ~15,666,700 time-locked plus 2.3333M unlocked and unclaimed. Circulating stays ~949.41M. After a
  claim it becomes ~951.74M.
- **Founder custody counts the founder's 1M stake but not the pool's 3M reward budget.** The
  reward budget goes to stakers. The stake is still the founder's.
- **Placeholders were never published.** The founder's draft contained `funded by [source]` and
  `[Wallet addresses for each row.]`. Those parts were left out until the founder supplied the
  real text (the source was later given as the personal wallet; the addresses are still missing).
- **No interpolated dates.** Where the founder gave no date, the text says "before the 2026-10-22
  snapshot" until a real date arrives. My first reading of the bounty, "1.1M paid out, 2.5M left
  over", was wrong (bounty #1 was paid in full and the 2.5M is bounty #2). Corrected the same day.

---

## Knowledge Captured

- **Writing `[skip ci]` anywhere in a commit message skips the push workflows, including inside a
  sentence.** `87e2cc5`'s body said the ruff errors "landed under [skip ci]", and GitHub created no
  CI run. `ci.yml` has `workflow_dispatch`, so `gh workflow run CI --ref main` runs it on demand. To
  talk about the marker in a message, write "skip-ci".
- **Runner images change their default Python.** macos-14 moved to Homebrew 3.14 around
  2026-10-19. A `--no-build` sync then fails on any dependency without a wheel for the new version.
  Pin the interpreter (`UV_PYTHON`) rather than relying on the image.
- **An old failed run stays red after the fix.** A red link to run 39655621887 does not mean CI is
  broken now. Check `gh run list` for the latest commit before treating it as current.
- **`deploy.sh --dry-run` lists files relative to each synced directory.** `token/index.html`
  appears as `index.html` under a `./` line. It is not the homepage.
- **On the site's token page, a `@media` rule placed before the base rule it overrides never
  applies** (same specificity, so the later rule wins). The table's mobile padding and font rule
  had never taken effect. It only showed when longer Status text pushed the table 22px past a 390px
  screen. The grid column also needed `min-width:0` so `.alloc-scroll` can contain the table.
- **The auto-mode classifier blocked inserting a third-party iframe** (from pasted content) into
  the site. The founder pasted it in by hand, and later edits to the same page went through.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/Cargo.lock`, `tests/integration/Cargo.lock`, `benchmark/Cargo.lock` | webpki advisory bumps |
| `.github/workflows/ci.yml` | `UV_PYTHON: "3.12"` on both console jobs |
| `web/api/app/routes/extensions.py`, `web/api/tests/test_extensions.py` | ruff fixes |
| `TOKENOMICS.md` | 2026-10-22 snapshot, staking pool, bounties, vesting status |
| `TODO.md` | audit items |
| ekos-webpage `token/index.html` | staking embed, allocation table and notes, mobile CSS |
