# Part II.3 — review and sign-off in the console (RFC 0213 Phase 3)

**Date:** 2026-10-25 · **Project:** `idemdocs` (iDempiere code + docs + Oracle XE, testbed home) · **Console:** token
mode, release `ekos` built from this commit · **Model calls:** 0

`review_tour.py` drives the console in Chromium (Playwright): sign in, open **Review**, decide one scope item
(`I_PRODUCT` out of scope, as proposed), see the scope sign-off turn **stale** naming `I_PRODUCT`, sign scope, PII,
write behaviour and the glossary off by typing each subject's name, check each digest is shown, and check that
identity (25 unconfirmed matches) and conflicts (195 open, unexplained) cannot be signed. Result: **18 passed, 0
failed**.

| | |
|---|---|
| step bar after the tour | 1 done · 2 todo · 3 partial (4 of 6 signed) · 4–7 blocked |
| first review read after a compile | 24.6 s (whole ledger) |
| cached reads | 1.9–2.3 s |
| a decision or sign-off | ~25 s (reads the whole ledger, then rewrites the cache) |

**BLOCKER for a full sign-off of iDempiere:** 195 open, unexplained code ↔ database conflicts and 25 identity
candidates are a person's work. Many identity candidates are name-prefix pairs (`@docusaurus/core =
@docusaurus/utils`), which points at the fuzzy candidate rule rather than at the review.

Screenshots: `scope-signed.png`, `identity-blocked.png`.
