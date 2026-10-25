# Part II.1 — step 1 in the console (EKOS RFC 0223)

**Date:** 2026-10-25 · **Console:** token mode, scratch EKOS home, an in-process keychain (the desktop's keychain was
not touched) · **Model calls:** 0

`sources_tour.py` drives the console in Chromium. Phase A: create a project from the iDempiere checkout, add the
documentation repository on the **Sources** page (the project's root is named as a source first), add the Oracle
database with its password, **Test connection**, click **Init & compile**. Phase B, after the run: test again and
compare.

| | |
|---|---|
| phase A | 7 passed, 0 failed — the password field is cleared once stored; the test connects read-only; nothing compiled yet |
| analysis (Init & compile, in the console) | 32 min 33 s: build, recover, resolve, compile, commit, coverage — all succeeded |
| phase B | 4 passed, 0 failed |
| census vs compiled | tables 903 = 903 · views 160 = 160 · routines 62 = 62 · triggers 0 = 0 |
| step bar | 1 Analyse sources **done** |

The database's password went from the browser to the console's keychain under `EKOS_DB_DEFAULT_IDEMCONSOLE_IDEMPIERE_DB`
and reached `ekos build` / `ekos database test` only through the tenant's environment; `ekos.toml` holds the
variable's name.

The test reported **40 invalid objects** where the Part 0 census found none. All 40 are views; 25 read `C_BPARTNER`
directly (`USER_DEPENDENCIES`), the table RFC 0213's stale demo altered (a column added, then dropped) — Oracle marks
a view `INVALID` when a table it reads changes, and recompiles it on first use. The page reports them as the database
does.

CLI equivalent: `ekos database test idempiere-db` — connect + catalog 6 s, census vs the ledger 13 s.
