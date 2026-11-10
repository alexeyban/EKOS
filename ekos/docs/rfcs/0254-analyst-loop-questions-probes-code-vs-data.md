# RFC 0254 — The analyst loop: open questions, exploration probes, code-vs-data checks, the source passport

**Status:** Accepted (2026-11-10, owner's request to start work; built in order 0252 → 0253 → 0254)
**Date:** 2026-11-09
**Depends on:** RFC 0252 (profiling for source understanding), RFC 0253 (channels and testimony), RFC 0170
(`SemanticGap`, `EnumMeaning`, `RationaleLink`), RFC 0172 (`ConflictingEvidence`), RFC 0158 (inclusion verdicts),
RFC 0027/0028 (Transformation IR), RFC 0201/0202 (Kettle deep model and relational IR), RFC 0213 (review,
sign-off), RFC 0214 (source-estate docs), RFC 0219 (ticket anchors), RFC 0242/0243 (`Probe`, read-only checks),
RFC 0247 (annotations), RFC 0018 (impact), RFC 0180/0181 (people, read scope)

---

## 1. Motivation

RFC 0252 makes the data a trace and RFC 0253 makes people a source. Between them, the work of a systems analyst
preparing a source is a loop:

```text
traces (code, data, docs) ──► hypotheses ──► what nobody can explain ──► a question to the right person
        ▲                                                                         │
        └──────────── testimony (evidence) ◄── answer in Slack/Teams/Telegram ◄───┘
```

EKOS has the left half: hypotheses with evidence (RFC 0170), disagreements (RFC 0172), review and sign-off (RFC
0213). It does not have:

1. **Questions as objects.** `ekos semantics gaps` lists what is unexplained; nothing turns a gap into a
   question with context, a priority and an addressee, or tracks whether it was answered.
2. **A safe way for an agent to look at data** while investigating. `ekos sql check` probes (RFC 0242/0243) check
   a generated query; there is no aggregates-only exploration surface for an agent forming a hypothesis.
3. **Checks of recovered logic against the data.** RFC 0164 runs logic differentially; RFC 0228 reconciles
   built marts. Nothing asks, cheaply and before anything is built, *"this filter keeps 1, 2, 3 — what happens to
   the 4% of rows holding 5 and 9?"*
4. **One page per source table** that answers what an analyst is asked for: grain, keys, relationships, code
   meanings with their usage, open questions, who said what.

## 2. Goals and non-goals

**Goals**

1. `OpenQuestion`: derived deterministically at commit from what is unexplained or disputed; prioritized by
   impact; routed to people who are likely to know; delivered in batches through RFC 0253 channels; answered by
   testimony; status computed on read.
2. `ekos_probe`: an aggregates-only, read-only, budgeted exploration tool for agents, with small-cell suppression
   and PII rules; plus a `source-explorer` agent in the agent pack.
3. Code-vs-data checks: deterministic findings where recovered Transformation IR assumes something the data
   contradicts, as `ConflictingEvidence` of type `code_vs_data`.
4. The **source passport**: RFC 0214's per-table page extended with everything above, and a questionnaire
   export for workshops.

**Non-goals**

- A model asking people questions on its own initiative. Questions are derived deterministically; a model may
  rephrase one for readability (cite-or-drop), never invent one.
- Chasing people. Delivery is batched and rate-limited; there are no reminders beyond one digest.
- Free SQL for agents. `ekos_sql_generate` and `ekos sql check` stay what they are; `ekos_probe` is a closed set
  of aggregate templates.
- Replacing RFC 0164/0228. Code-vs-data checks are static and cheap; differential execution and reconciliation
  remain the proof.

## 3. Design

### 3.1 `OpenQuestion`

A keyed `Custom("OpenQuestion")` (registered, `structurally_keyed: true`; key = origin kind + origin id +
question type, so an unchanged gap yields the same question on every commit). Derived at `ekos commit` by
`ekos_semantic::questions`, pure, after RFC 0170 synthesis, RFC 0172 conflicts and RFC 0252 signals.

| Origin | Question type | Template (deterministic) |
|---|---|---|
| `SemanticGap` (code literal, no meaning) | `meaning_of_value` | What does `{column} = {literal}` mean? It is used in {n} places, e.g. {site}. |
| `SemanticGap` `data_only_values` (RFC 0252) | `unexplained_values` | {share}% of rows in `{column}` hold {k} values no code mentions. What are they? *(values revealed on read, §3.2)* |
| `EnumMeaning` with `dead: true` | `dead_code` | `{column} = {literal}` has not appeared since {last_seen}, but {n} code paths handle it. Can it be dropped? |
| `ConflictingEvidence` `open` without explanation | `which_is_right` | {source A} says {claim A}; {source B} says {claim B}. Which is right? |
| `FkCandidate` verdict `Unclear` | `is_this_a_reference` | {share}% of `{child}` values exist in `{parent}`. Is `{child}` a reference to `{parent}`? |
| `GrainCandidate` with no confirmed key | `what_is_a_row` | No unique key found in `{table}`. What does one row represent? |
| `code_vs_data` finding (§3.4) | `logic_vs_data` | `{transformation}` handles {literals}; data also holds {k} other values ({share}%). Where should they go? |
| RFC 0247 annotation `stale` | `still_true` | {author} noted "{text}" on `{entity}`, which has changed since. Is it still true? |

Properties: `question_type`, `subject` (entity id), `context` (the facts shown, with evidence links), `text`
(the template rendering), `priority`, `addressees`, `blocking` (true when the origin blocks an RFC 0213 subject).

**Status is computed on read, never written** (the RFC 0213/0247 rule):

| Status | When |
|---|---|
| `open` | no testimony answers it and the origin is current and undecided |
| `answered` | at least one active testimony has `answers = <id>`, origin not yet decided |
| `closed` | the origin item is decided, resolved or confirmed (by review, with or without the testimony) |
| `obsolete` | the origin is no longer current (`.ekos/semantics/current.json`, `.ekos/conflicts/current.json`, …) |

**Priority** is deterministic: `blocking` first; then impact — the number of dependents RFC 0018's
`ekos_impact` finds for the subject, the subject's row share for value questions, whether the subject is in RFC
0213 `scope`; ties broken by id. Out-of-scope tables produce no questions.

### 3.2 Revealing values on read

RFC 0252 never persists data values. A question about unexplained values is useless without them, so they are
**revealed on read**: when a person with read access to the column opens the question (console, channel card,
`ekos questions show`), EKOS runs one RFC 0252 `AggregateQuery` (group-by-count, top `[questions] reveal_top`
values excluding code literals) on the live source and shows the result. Rules:

- never for a column classified as PII (confirmed or unconfirmed): counts only;
- small-cell suppression: a value with fewer than `[probe] min_cell` rows (default 5) is shown as `<5 rows`
  without the value;
- the result is not persisted, not logged, not sent to a remote model unless the person sends it themselves in
  a message;
- the live query obeys RFC 0157 session safety and is attributed to the person in `application_name`.

### 3.3 Routing: who is likely to know

`addressees` is a ranked list of console users (only linked people are ever contacted), derived from evidence
EKOS already has:

| Signal | Source |
|---|---|
| configured owners | `[questions.owners]`: patterns over schema/table/column or functional area → users or groups (strongest) |
| authors of the code that uses the subject | `RationaleLink` (git blame, RFC 0170), commit authors via RFC 0219 |
| ticket participants | assignee and reporter of tickets anchored to the subject's change sets (RFC 0219) |
| document authors | Confluence page authors (RFC 0022), testimony authors on neighbouring entities |
| reviewers | the project's reviewers, as the fallback |

People are matched to console users by e-mail or by an explicit mapping in the console; an unmatched author is
shown to the operator as a hint ("probably knows: j.doe@…"), never contacted.

**Delivery.** `ekos questions digest` (and a schedule in the console, RFC 0183) sends each addressee at most
`[questions] per_person_per_day` (default 5) questions, highest priority first, as one message per channel with
one card per question (RFC 0253 rendering). A question goes to one person at a time; it moves to the next
addressee after `[questions] reassign_after_days` without an answer, or when the person presses "Not me". A
person can mute a functional area. Nothing is sent from a public deployment (RFC 0249).

**Answering.** A reply to a question card is an RFC 0253 statement with `answers` preset; the person still saves
it explicitly. In the console's Questions page an operator can record an answer received elsewhere (a meeting),
naming the person — recorded as testimony **by the operator**, with `on_behalf_of` text, never as the named
person's.

### 3.4 Code-vs-data checks

`ekos_semantic::code_vs_data`, pure, at commit, over Transformation IR (RFC 0027, Kettle deep model RFC 0201/0202,
PL/pgSQL IR RFC 0163) and RFC 0252 profiles. Each finding is a `ConflictingEvidence` with `conflict_type:
code_vs_data` and a `check` property, citing the code site and the profile run, so it is reviewed, explained and
signed like every other conflict:

| `check` | Condition | Why an analyst cares |
|---|---|---|
| `predicate_misses_values` | a filter, `CASE` without `ELSE`, Pentaho `FilterRows`/`SwitchCase` enumerates literals over a column; data holds other values with share ≥ `[code_vs_data] min_share` | those rows are dropped or become NULL silently — the new pipeline must decide on purpose |
| `dead_branch` | a branch's literal has 0 rows, or is `dead` (RFC 0252) | logic not worth migrating — or data not yet loaded |
| `lookup_not_unique` | a lookup / join step's key on the looked-up side has duplicate rate > 0 | fan-out: the legacy job multiplies rows, or the step keeps the first match nondeterministically |
| `null_not_handled` | an equality join or predicate on a column with null rate > 0 and no null handling in the IR | rows disappear from inner joins |
| `cast_will_fail` | the IR casts a text column to date/number; RFC 0157 pattern classes show a non-conforming share | the legacy tool may coerce silently; the target will error |
| `range_assumption` | a `BETWEEN`/comparison constant outside the column's observed min/max | stale constants (a year hard-coded in 2015) |

Every check reads only aggregates already in the ledger; none queries the source. Thresholds are settings, and a
finding records the measured value, so a reviewer sees "4.1% of rows", not "fails". The `explanation` mechanism
of RFC 0211 applies: a finding the source tree accounts for (an `ELSE` added in a later migration script) carries
an explanation and stays for a person to accept.

These findings feed §3.1 (`logic_vs_data` questions) and RFC 0228's reconciliation expectations: a
`predicate_misses_values` finding a person resolved as "drop on purpose" becomes the expected row-count delta, so
reconciliation does not flag it again.

### 3.5 `ekos_probe`: aggregates-only exploration for agents

A read-only MCP tool (in `SCOPED_TOOLS`; opt-in per project like `ekos_sql_generate`, RFC 0250; metered under
RFC 0183 when the project counts probe calls) with a closed set of templates, rendered through RFC 0252's
`AggregateQuery`:

| Template | Returns |
|---|---|
| `distribution(table, column, where?)` | top-k values with counts, null count, distinct estimate |
| `cooccurrence(table, x, y)` | counts per (x, y) pair, top-k |
| `null_by(table, column, by)` | null rate of `column` per value of `by` |
| `orphans(child, parent)` | the RFC 0158 inclusion counts for one pair |
| `over_time(table, column, time_column, grain)` | counts per value per month/year |
| `count(table, where?)` | a row count |

`where` is the RFC 0247 closed predicate vocabulary (column, operator, literal), type-checked against the
column — never SQL text. Every call: RFC 0157 session safety, `EXPLAIN` cost check against a per-call and
per-session budget, the same PII and small-cell rules as §3.2, results never persisted, call audited without its
results.

**Evidence from a probe.** A probe result is ephemeral. When a person saves a proposal that cited one (RFC
0253 §3.6), the Save action re-runs the cited probe and stores its **aggregate** result (counts, shares;
suppressed cells stay suppressed) as `KirEvidence` with the template, the parameters, the run id and the source
LSN/SCN. So a confirmed claim is backed by a measurement a reviewer can re-run, and an agent's word alone never
is.

**Agent pack.** A `source-explorer` agent definition joins the agent pack (RFC 0167 shape): it works the loop
*hypothesis → probe → refine*, uses `ekos_probe`, `ekos_profile`, `ekos_semantics_lookup`,
`ekos_transformation_explain`, and ends every investigation with proposals written **as text for a person**
(entity, claim, cited probes). It has no write tool. In the console chat (RFC 0187) the same instructions are
available as a chat mode.

### 3.6 The source passport and the questionnaire

RFC 0214's per-table page gains sections, all from the ledger, each fact with its status (hypothesis /
confirmed) and evidence link:

1. **What a row is** — grain (confirmed key, or `GrainCandidate`s), row count, growth.
2. **Keys and references** — declared, code-seeded and data-seeded (RFC 0252), with verdicts and review status.
3. **Codes** — per coded column, every value with meaning, source (code, comment, data, testimony), share,
   first/last seen, dead/new.
4. **How it changes** — RFC 0212 write behaviour with RFC 0252 history evidence.
5. **What the code assumes** — `code_vs_data` findings and their decisions.
6. **What people said** — testimony (RFC 0253) and annotations (RFC 0247).
7. **Open questions** — with status and addressee.

`ekos questions export --format docx|md|xlsx [--area <area>] [--addressee <user>]` produces a questionnaire for
a workshop: the questions grouped by table, each with its context and space for the answer. Answers recorded in
the workshop go back through the Questions page (§3.3, as operator-recorded testimony).

### 3.7 Surfaces

- CLI: `ekos questions list|show|digest|export|reassign|mute`, `ekos check code-vs-data` (recompute and print,
  like `ekos check refs`).
- MCP: read-only `ekos_questions` (filter by status, subject, addressee, type; in `SCOPED_TOOLS`), `ekos_probe`.
- Console: Questions page (filter, open, reveal values, record an answer, reassign), question badges on entity
  pages, the extended table page; a "questions" item in the step bar's step 3 (RFC 0213): blocking questions
  open.

## 4. Decisions

| Decision | Alternative rejected | Why |
|---|---|---|
| Questions derived deterministically from existing items | A model decides what to ask | Reproducible, explainable, no invented questions; a model may only rephrase |
| Status computed on read | Status written on answer | RFC 0213/0247: validity is never stored; the origin item's decision is the truth |
| Values revealed live, never stored | Store unexplained values for the question | The append-only ledger cannot forget; a question can show values to an authorized person without keeping them |
| Code-vs-data as `ConflictingEvidence` | A new finding kind with its own review | One review path, one console view, one sign-off digest (RFC 0172/0174) |
| Closed probe templates | Read-only free SQL for agents | Aggregates-only and small-cell suppression become properties of the tool, not of the agent's discipline |
| Probe evidence re-measured on Save | Trusting the agent's reported numbers | A confirmed claim must rest on a measurement EKOS made, not on text a model produced |
| One person per question at a time, batched digests | Broadcast to everyone who might know | People stop answering a bot that spams; ownership must be clear |

## 5. Tests (written first)

- Derivation: each origin in §3.1 yields exactly one question with a stable id across two commits; a resolved
  origin makes the question `closed`, a vanished origin `obsolete` — computed, nothing written.
- Priority: a blocking question outranks a higher-impact non-blocking one; out-of-scope tables yield none;
  reversed ledger order gives the same ranking.
- Routing: configured owner first; git-blame author next; an unmatched author is never contacted; "Not me"
  moves the question to the next addressee.
- Digest: per-person cap holds; nothing sent in a public deployment.
- Reveal: values shown for a non-PII column, counts only for a PII column, small cells suppressed; a ledger and
  log scan afterwards finds none of the revealed values.
- Code vs data, one fixture per check: Kettle `FilterRows` over `1,2,3` with data `1..5`; a `CASE` without
  `ELSE`; a lookup on a non-unique key; an equality join on a nullable column; a text→date cast over a column
  with 2% non-dates; a hard-coded year outside the data's range. A later migration script adding `ELSE`
  yields an explanation.
- Probe: a `where` that does not type-check is refused; a template over budget is refused before running; the
  scoped-store leak suite covers `ekos_probe` and `ekos_questions`; Save re-measures and stores aggregates only.
- Agent pack: `source-explorer` has no write tool (scan).
- Export: a questionnaire for one area round-trips its question ids.

## 6. Phases

1. `OpenQuestion` from existing origins (`SemanticGap`, `ConflictingEvidence`, inclusion `Unclear`, stale
   annotations); `ekos questions list|show|export`; Questions page; `ekos_questions`.
2. Code-vs-data checks over Kettle and SQL IR; `code_vs_data` questions; RFC 0228 expectation hand-off.
3. Routing and digests through RFC 0253 channels; reveal on read.
4. `ekos_probe`, probe evidence on Save, `source-explorer` agent; the source passport sections.

## 7. Open questions

1. Should a question answered by two people who disagree become a `testimony_mismatch` conflict automatically
   (RFC 0253 §3.7), or wait for the reviewer? Proposed: automatically — it is exactly what a reviewer must see.
2. Is ownership by functional area (RFC 0214's areas) a better default for `[questions.owners]` than table
   patterns?
3. Should `dead_branch` findings feed RFC 0164's lowering coverage as "intentionally not lowered" once confirmed?
4. Do we measure the loop? Proposed metric for the devlog on a real estate: open → closed questions per week and
   median time to answer, before and after channel delivery.

## 8. Acceptance criteria

- [ ] On LedgerSMB and iDempiere, `ekos questions list` produces a ranked list whose top 20 an operator judges
      worth asking (recorded in the devlog with the judgement).
- [ ] At least one `predicate_misses_values` and one `lookup_not_unique` finding on the Pentaho fixture estate,
      each confirmed by hand against the data.
- [ ] A question answered in Telegram closes after the reviewer decides its origin, with the testimony cited in
      the review item and on the table's passport page.
- [ ] Ledger scan after reveals and probes: zero source values beyond code literals.

## 9. Implementation log

### 2026-11-10 — Console Questions page and digest delivery (Phases 1 and 3, console side)

**What.** Over `ekos questions list|show|digest|reassign|decline|mute`:

- `web/api/app/routes/questions.py`: list (filters status, type, table, addressee, limit), show,
  `GET …/{id}/reveal` (`questions show --reveal`; who looked is audited, the values are not),
  `POST …/{id}/answer` (an answer heard elsewhere: `testimony add --answers <id> --on-behalf-of
  <who said it> --as <the operator>`, `review` right), reassign (`operate`), decline ("not me":
  only the person the question is with, as themselves) and mute (for oneself). Reads go through
  the `readproc` allowlist (`questions show` takes `--json [--reveal] <hex id>` and nothing else).
- `web/ui/src/pages/Questions.tsx`: the tab, with the panel's context, answers, *Reveal values*,
  *Record an answer*, *Reassign* and *Not me*.
- Delivery (§3.3): `ChannelService.digest` (`web/api/app/channels/service.py`) runs `questions
  digest --json` per project and sends each person, through the channel they last wrote to
  directly, a header and one card per question with **Answer** (`/ekos answer <id> <text>`, which
  makes an RFC 0253 card with `answers` preset) and **Not me** (`questions decline --as <them>`).
  Scheduled by `channels_digest_cron`; `POST /api/channels/digest` sends now (owner). People are
  matched to linked accounts by the e-mail the CLI names them with. Nothing is sent when
  `Settings.public` (RFC 0249 checks on, or a read-only showcase).

**Tests.** `web/api/tests/test_people.py` (the argv of every route, reveal audited without
values, not-me refused for anyone else), `test_channels.py` (digest delivery with Not me; none
from a public deployment), `web/ui/src/pages/People.test.tsx`.

**Not done.** Question badges on entity pages and the step bar's "blocking questions" item; the
console does not decide who an unmatched author is (the CLI's routing does); the per-person cap
is the CLI's `[questions] per-person-per-day`, applied by `questions digest`.
