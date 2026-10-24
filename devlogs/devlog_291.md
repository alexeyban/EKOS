# Devlog 291 — Ticket anchors: commits, change sets and code joined by ticket key (RFC 0219)

**Date:** 2026-10-23
**PRs:** none (local)
**Branch:** `main` (local, not pushed)

---

## Summary

Part I.8's git part. iDempiere names its tracker everywhere — migration files, code comments and
1,971 of its last 2,000 commit subjects — but nothing joined them. The git analyzer now makes one
`Ticket` per ticket id its commits name (for configured keys), with the commits, dates, authors,
a title from the first commit's subject and `Touches` edges to the files; `ekos commit` links each
ticket to the change sets and Java code that cite it. On the full history (16,260 commits): 4,597
tickets linked to 3,074 change sets, 154 Java types and 272 methods. A schema question now reaches
*when, why and who*: `C_Country.ISOCountryCodeAlpha3` → its change set → `IDEMPIERE-6349` → Carlos
Ruiz, 2024-12-27, "Add M49 country codes to the C_Country table".

---

## RFC 0219 — ticket anchors

### Problem / motivation
`tickets` sat on 34,367 objects and in every commit message, unjoined. And the git observer read
only the newest 500 commits — hard-coded — so most of a long-lived estate's history never arrived.

### What was built

| Component | What it does |
|---|---|
| `ekos_common::tickets::find` | exact-key ticket ids, boundary-checked; the Java pass now uses it |
| `[tickets] keys` | configured keys; no generic pattern (`UTF-8`, `SHA-256` look like tickets) |
| `detect.rs` | `init --detect` proposes keys naming ≥ 20 file names |
| `git_analyzer` | `Ticket` objects (commits newest first, capped at 50; title; first/last date; files touched; bulk commits) + `Touches`; output supersedes earlier runs |
| `semantic/src/tickets.rs` | `TicketFor` from a ticket to each `SchemaChangeSet` / `JavaType` / `JavaMethod` citing it |
| `[observe] git-max-commits` | the commit cap, default 500; in the fingerprint key only when changed |
| `ekos commit` | `Ticket links:` summary line |

### Measured (iDempiere `release-13`, 16,260 commits)

| | |
|---|---|
| detected keys | `IDEMPIERE` 3,355 file names, `TICKET` 50 |
| default cap (500 commits) | 363 tickets; 1,183 cited tickets without a commit |
| `git-max-commits = 20000` | 4,597 tickets; 25,045 `Touches` (20,067 to files that exist today) |
| `TicketFor` | 3,074 change sets of 3,356, 154 Java types, 272 Java methods; 34 cited tickets no commit names |
| cost | build 226 s; steady-state commit 165 s → 339 s, mostly the history's `CoupledWith` (44,377) and `OwnedBy` (16,260) |

### Implementation details worth remembering
- `File` objects are written by `build` straight to the ledger; they are not in the compiled model
  (`ckm/model.json.zst`). A dangling-edge check against the CKM reports every file edge as dangling
  — check against the ledger.
- A ticket's title is the first commit's subject minus the key, labelled
  `title_source: "first_commit_subject"` — the commit says it, not the tracker.
- A commit touching more files than the coupling limit (50) is counted on the ticket
  (`bulk_commits`), not linked to its files.

### Decisions
- **Exact keys only.** Generic `ABC-123` matching would link `UTF-8` and `ISO-8859`. Keys come from
  config, proposed by detection from file names (where the naming convention proves them).
- **Tickets from commits only.** A ticket object exists only when a commit names it; a citation
  without commits stays a property (34 of them).
- **The example question had a false premise.** The plan asked when `T_ReportStatement.Account_ID`
  was added; no migration adds it (base schema). The answer "since the base schema" is the honest
  one, and the eval should use questions whose premises hold.

---

## Knowledge Captured

- **The git observer's 500-commit default silently truncates history.** On a 16k-commit estate
  92% of commits were never read; any ticket, authorship or coupling number from the default is a
  sample of the newest work only.
- **Changing an observer setting needs a cache-key change**, or the build serves the old artifacts
  (the file fingerprint did not move). `observer_settings_key` adds the cap only when non-default,
  so existing workspaces keep their keys.
- **Every pass that reads "all of X" should supersede its earlier runs** (RFC 0211's flag) — the git
  pass now does; before, each new commit left an older output to merge.
- iDempiere 2012–2013 scripts use `TICKET-NNNNNNN` (an older tracker) next to `IDEMPIERE-NNNN`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0219-ticket-anchors.md` | new RFC with results |
| `ekos/crates/common/src/{tickets,lib}.rs` | new shared ticket finder |
| `ekos/crates/recovery/src/git_analyzer.rs` | `Ticket` objects, `Touches`, supersedes earlier runs, v4 |
| `ekos/crates/recovery/src/java_analyzer.rs` | ticket finder from `ekos-common` |
| `ekos/crates/semantic/src/{tickets,lib}.rs` | new: `TicketFor` links |
| `ekos/crates/compiler-core/src/{config,lib}.rs` | `[tickets] keys`, `[observe] git-max-commits` |
| `ekos/crates/cli/src/detect.rs` | ticket-key detection and `[tickets]` rendering |
| `ekos/crates/cli/src/commands/{build,commit,recover,config}.rs` | commit cap + cache key; ticket links at commit; keys to the git pass |
| `ekos/crates/kir/src/custom_kinds.rs` | `Ticket` (keyed) |
