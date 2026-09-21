# Devlog 195 — compiled app to Python: a screenshot-per-step demo

**Date:** 2026-09-21
**PRs:** (working tree, not committed)
**Branch:** `main` (local)

---

## Summary

A reproducible demo and deck: take a real open-source compiled .NET library (MarkdownSharp 2.0.5, MIT), recover it
with EKOS, write a Python rewrite from the recovered statements, and show the rewrite gives byte-identical output.
`demo/binary-demo/run_demo.sh` runs 17 stages of real commands, `capture.py` renders each to a PNG (frames are
GIF-ready), and `tools/build_deck.py` builds `docs/presentations/compiled-app-to-python.html`. Java was the first
idea, but JVM bodies are `structural` only (RFC 0150 non-goal), so the demo is .NET.

## Results (all from the frames, seed and inputs fixed)

| Check | Result |
|---|---|
| Recovery | 101 method bodies, 99 `statements`, 2 `control_flow` (98%) |
| `ekos-characterize check` | 16/16 |
| Fixtures, original (wine-mono) vs port | 15/15 sha256-identical |
| Differential fuzz | 3,000/3,000 identical (seeds 20260921, 7, 11, 5) |
| Planted bug (one regex flag) | caught: 44 of 300 differ |
| Static `ekos_binary_migration_check` | 42 match, 22 differ, 14 missing — all explained (constants moved, properties are attributes) |

## Added: "how EKOS helps" section

Two slides (EKOS-vs-without table; why it is harder for a plain LLM) plus stage 17, `tools/generic_baseline.py`:
Python-Markdown and markdown-it-py against the original on the 15 fixtures. Byte-exact 1/15 for both; ignoring
whitespace 10/15 and 8/15; the spec-derived port is 15/15. It is a proxy, not an LLM run: no head-to-head was done,
and the deck says so. Stages are now 18 (scoreboard is 18).

## Knowledge Captured

- The port matched on the first attempt; nothing was staged. The two `control_flow` methods (`Normalize`,
  `FormParagraphs`) needed reading against the IL, not the pseudo-code.
- .NET to Python regex traps: `\z` is Python `\Z`; .NET `\Z` is `(?=\n?\Z)`; variable-width lookbehind
  `(?<=\n\n|\A)` must be split; atomic groups need Python 3.11+ (3.10 fails); `RegexOptions` appear as ints in IL
  (42 = Multiline|IgnorePatternWhitespace|Compiled).
- `EncodeEmailAddress` uses `System.Random`: the original is non-deterministic there, so compare after entity decoding.
- A fuzzer must be validated with a planted mutant: the first fuzz bank missed a removed `Singleline` flag until
  multi-line emphasis inputs were added.
- `ekos-characterize` Python side only constructs a class when `python.ctor` is present (`"ctor": []` for default).
- A static migration check with closures for evaluators reports them `missing`; named methods mirroring the original fix that.
- Decompiler stays private (RFC 0149); the deck shows output only.

## Files Changed

| File | Change |
|---|---|
| `demo/binary-demo/` | `run_demo.sh`, `capture.py`, `app/*.cs` harnesses, `inputs/`, `python-port/mdport`, `tools/` |
| `docs/presentations/compiled-app-to-python.html` + `assets/binary-demo/*.png` | the deck and its 18 screenshots |
| `docs/presentations.html` | index entry |
