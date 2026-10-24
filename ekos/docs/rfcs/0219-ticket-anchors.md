# RFC 0219 — Ticket Anchors: Commits, Change Sets and Code Joined by Ticket Key

**Status:** Accepted
**Date:** 2026-10-23
**Depends on:** RFC 0006 (git observer), RFC 0209 §8 (ticket anchors in Java comments), RFC 0210
(`SchemaChangeSet.tickets`)
**Plan:** `oracle-idempiere-plan.md` v3, Part I.8 (git history and ticket links). The docs repository
and the wiki snapshot of I.8 are not in this RFC.

---

## 1. Motivation

Legacy estates cite their issue tracker everywhere: iDempiere's migration scripts are named
`202409031411_IDEMPIERE-6203.sql`, its Java comments say `// IDEMPIERE-6203`, and 1,971 of its last
2,000 commit subjects start with the ticket they implement. The ticket is the one key that joins
*when* (the commit date), *why* (the commit subject, usually the ticket's title), *what changed in
the schema* (the change set) and *what changed in code* (the files and anchored methods).

Today each source records its tickets separately — `tickets` on 34,367 objects, the raw commit
message in each git event — and nothing joins them. A question like "when and why was
`T_ReportStatement.Account_ID` added" stops at the change set.

## 2. Goals and non-goals

**Goals**

1. One ticket parser, shared, with the keys configurable: `[tickets] keys = ["IDEMPIERE", …]`.
2. One `Custom("Ticket")` object per ticket key seen in commit messages, with its commits (sha, date,
   author, subject) and links to the files those commits touched.
3. At `ekos commit`, links from each ticket to every object whose `tickets` property names it.

**Non-goals**

- Fetching tickets from the tracker (titles, status): no network in core analysis.
- Fuzzy references ("see the invoice bug"): plan I.8 — anything fuzzier is an unconfirmed
  hypothesis, not a link.
- The docs repository and the wiki snapshot (separate work).

## 3. Design

### 3.1 Keys

`[tickets] keys` lists issue-tracker project keys. A ticket id is `KEY-<digits>` with a non-
alphanumeric character (or the start) before it; matching is exact on the key. A generic
`[A-Z]+-[0-9]+` pattern is refused by design: it matches `UTF-8`, `ISO-8859`, `SHA-256`. With no keys
configured nothing is linked, except RFC 0209's two built-in keys (`IDEMPIERE`, `ADEMPIERE`), kept
for Java anchors so existing ledgers do not change. `ekos init --detect` proposes keys: a key is
proposed when it names at least 20 file names (written with the counts as a comment). Commit
subjects are not counted — detection does not read git history.

`ekos_common::tickets::find(text, keys) -> Vec<String>` replaces the two copies in `java_analyzer`
and `schema_history` (the latter keeps its generic file-name reading, where the file-name convention
already proves the key).

### 3.2 Ticket objects (git analyzer, at `ekos recover`)

For each commit, the tickets in its message. Per ticket:

- `key`, `commit_count`, `first_commit_at`, `last_commit_at`;
- `commits`: up to 50, newest first — `{sha, date, author, subject}` (subject = first line);
- `title`: the subject of the earliest commit with the ticket key removed — the commit, not the
  tracker, says it (labelled `title_source: "first_commit_subject"`);
- `Touches` relationships to the `File` objects its commits changed (a commit changing more files
  than the coupling limit is counted, not linked).

The pass reads every commit, so its output sets `supersedes_earlier_runs` (RFC 0211): otherwise an
older run's ticket, with fewer commits, could win the merge at compile. The number of commits read
is `[observe] git-max-commits` (default 500, unchanged; it joins the build's fingerprint key only
when changed, so existing caches stay valid).

Evidence: the commits (`git:commit:<sha>` locations, as the git events already use).

### 3.3 Links (at `ekos commit`)

`semantic::tickets::link` adds `Custom("TicketFor")` from a `Ticket` to every `SchemaChangeSet`,
`JavaType` and `JavaMethod` whose `tickets` property contains its key (`SchemaChange`s are reached
through their set). Deterministic ids; only tickets that exist as objects are linked.

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | shared parser, `[tickets] keys`, `init --detect` proposal | unit tests; detect proposes `IDEMPIERE` on the testbed |
| 2 | `Ticket` objects + `Touches` | whole-tree counts: tickets, commits linked, files |
| 3 | `TicketFor` links | counts by target kind; the `T_ReportStatement` question answerable from the ledger |

## 5. Tests

Parser boundaries (`XIDEMPIERE-1` no, `(IDEMPIERE-1)` yes, `UTF-8` never without a key); ticket
aggregation over fixed commits (order, cap, title); link counts over a hand-built graph.

## 6. Results

iDempiere `release-13` @ `3355eb5fc9`, 16,260 commits, release build, scratch `EKOS_HOME`, no model
calls.

**Phase 1.** `init --detect` proposes `IDEMPIERE` (3,355 file names) and `TICKET` (50 — the
2012–2013 migration scripts `201211161720_TICKET-1001611.sql` name an older tracker). Nothing else
reaches 20.

**Phase 2.** With the default cap only 500 commits were read (363 tickets, 1,183 cited tickets
without a commit) — the cap became `[observe] git-max-commits`. With 20,000: 16,260 commits,
**4,597 tickets**, 25,045 `Touches` edges, of which 20,067 reach a `File` that exists today (the
rest name files since deleted or renamed — history, kept).

**Phase 3.** `TicketFor`: 3,074 change sets (of 3,356), 154 Java types, 272 Java methods; 34 cited
tickets are named by no commit.

**The plan's example question.** No migration adds `T_ReportStatement.Account_ID` — the column is in
the base schema, and every script touching the table modifies, constrains or indexes it. The chain
itself, on a column a migration did add: `C_Country.ISOCountryCodeAlpha3` → change set
`202412261358_IDEMPIERE-6349` → `Ticket IDEMPIERE-6349`: one commit by Carlos Ruiz on 2024-12-27,
"Add M49 country codes to the C_Country table (FHCA-6138)".

**Cost.** Reading the full history raised the build to 226 s (git log over 16,260 commits) and the
steady-state commit from about 165 s to 339 s, mostly the history's own edges (44,377
`CoupledWith`, 16,260 `OwnedBy`) rather than tickets (25,045 `Touches`).
