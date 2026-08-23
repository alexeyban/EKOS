# Devlog 74 — RFC 0068 Increment 3: Architecture Summary + Runtime View

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Third increment of continuous, automatic build-out against RFC 0068. Architecture Summary
(Executive Overview, §14) and Basic Runtime View (§20) — the last two of RFC 0068 §61's MVP view
items. With this increment, all six §61 MVP views are shipped except SVG diagram generation. Hit
the same relationship-duplication bug RFC 0070 found, in a second independent location — confirms
it's a real, recurring pattern worth promoting, not a one-off already fully handled.

---

## RFC 0071 — Architecture Summary + Runtime View

### What was built

| Component | What it does |
|---|---|
| `render_architecture_summary` (`docs-gen`) | Real component/crate counts, top-5 technologies by real dependent count, open-questions count — with explicit "not yet computed" text for the four fields RFC 0068's template names but nothing here can back |
| `## Runtime View` | Links to the already-generated `SequenceDiagrams.md` rather than duplicating its content |

### Implementation details worth remembering

**"Not yet computed" is data, not an apology.** RFC 0068 §14's template has seven fields; four of
them (`Purpose`, `Architecture style`, `Major risks`, `Architecture confidence`) have no real EKOS
source today, for three genuinely different reasons: two need reasoning this deterministic renderer
doesn't do, one needs a KIR kind (`Risk`) that doesn't exist yet, and one needs a computation
(`evaluate_architecture`) that exists but isn't wired into this code path. Writing all three
reasons out explicitly, per field, rather than one generic "coming soon," is what makes the gap
followable later — the next person (or increment) reading this doesn't have to re-derive why each
field is empty.

**Runtime View resisted the temptation to duplicate data for its own sake.** `SequenceDiagrams.md`
already has every real compiled call/data-flow sequence. RFC 0068 §20's own examples ("Create
Order", "Process Payment") are about *named business scenarios*, not an exhaustive per-symbol
dump — and picking which sequences deserve a name is a judgment call this project has no
deterministic source for. Linking through, with the scenario-naming gap stated plainly, is more
honest than either re-rendering the same data under a new heading (implying it's a different,
curated view when it isn't) or inventing scenario names.

**The relationship-duplication bug is now confirmed recurring, not a one-off.** Live-verifying
Architecture Summary found the exact same symptom RFC 0070 diagnosed two increments ago — raw
dependent counts for `serde_json` read 132 instead of the real ~33-34. Applied the identical fix
(dedupe by `(from, to)` before counting) with its own regression test, rather than assume RFC
0070's fix in one view covered every other place the same root cause could surface — it didn't, and
now there's real evidence (two independent occurrences, not a hypothesis) that a third is
plausible. Promoted the root-cause TODO.md item's framing accordingly: "worth promoting ahead of
further §62 work" rather than "eventually."

---

## Live verification — no new pipeline run needed

Third increment running on the same pattern: this repo's own real, already-committed ledger had
everything needed. `ekos docs generate --layout curated` rendered a real Architecture Summary (44
crates, real top-5 technologies — `serde_json` correctly at 34 dependents after the fix, not 132 —
0 open questions) and a real Runtime View linking to the real `SequenceDiagrams.md`.

---

## Knowledge Captured

- **A bug found once in one view is a hypothesis about every other similar view until it's
  actually checked, not a closed issue.** RFC 0070's fix stayed scoped to the one view it touched,
  deliberately, rather than trying to preemptively patch every other relationship-reading code
  path without evidence any of them were actually affected. This increment's own live verification
  *was* that evidence for a second path — cheaper to find real occurrences one at a time via real
  verification than to guess at all of them up front.
- **An honest empty field is more useful to a reader than a plausible-sounding fabricated one** —
  restated concretely again this increment (four fields, three different real reasons), because
  it's the same principle every prior increment has already applied, and consistency in *how* a
  gap is reported is itself part of what makes the reporting trustworthy.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0071-architecture-summary-and-runtime-view.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Increment 3 status note |
| `ekos/crates/docs-gen/src/lib.rs` | New `## Architecture Summary` + `render_architecture_summary`; new `## Runtime View`; dedup fix; 4 new tests |
| `TODO.md` | RFC 0068 §61 MVP items ticked off (all done except SVG generation); relationship-id bug promoted; next increment scoped |
| `devlogs/devlog_74.md` | This file |
