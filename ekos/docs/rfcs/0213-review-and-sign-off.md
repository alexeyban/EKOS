# RFC 0213 — Operator Workflow: Step Bar, Review, Digest-Bound Sign-Off

**Status:** Accepted
**Date:** 2026-10-25
**Depends on:** RFC 0172/0174 (conflicts + console tab), RFC 0029/0063 (identity review), RFC 0170 (semantics
review), RFC 0161 (evidence-pinned approvals), RFC 0186 (catalogue classes), RFC 0198/0207 (extension pages),
RFC 0211/0212 (live catalog, profiles, write behaviour)
**Plan:** `oracle-idempiere-plan.md` v3, Part II — the step bar and II.3 *Review and sign-off*; decision D5
("digest-bound sign-off fact in the console for steps 3, 4, 5").

---

## 1. Motivation

Part II turns the compiled ledger into a lakehouse through seven steps an operator walks in the console. Steps 4–7
(requirements, extraction, bronze/silver, marts) build on a *source model*: which tables are in scope, which columns
hold personal data, how each table's rows change and disappear, which code ↔ database disagreements were explained,
which cross-system identities hold. Today those facts are compiled but nobody has agreed to them, and a later compile
can change them silently.

Two things are missing:

1. **Decisions per item** for subjects that have no review path yet — scope, PII, write behaviour (conflicts,
   identity and glossary already have one).
2. **A sign-off that means something**: approved on a set of facts, it stops being valid the moment one of those facts
   changes. RFC 0161 already does this for migration approvals (`EvidenceSnapshot`: a hash mismatch makes the request
   dead); the source model needs the same, per subject.

On the iDempiere testbed the subjects are not small: 903 live tables, 903 `WriteBehaviour` objects, 446 open
conflicts (165 `model_column_missing`, 153 `table_without_model`, 116 `mandatory_but_nullable`, …), PII candidates
from the P2 profile.

## 2. Goals and non-goals

**Goals**

1. Six review subjects — `scope`, `pii`, `conflicts`, `identity`, `write-behaviour`, `glossary` — each a
   deterministic list of items computed from the ledger, with the facts the operator is shown.
2. Item decisions for `scope`, `pii` and `write-behaviour` (the others keep their existing commands).
3. A **digest** per subject over exactly the facts shown; **sign-off** records who, when, the digest and the item
   ids. Valid only while the recomputed digest equals the signed one; otherwise *stale*, with the items that changed.
4. A **step bar**: `ekos workflow status --json` gives each of the seven steps a state; extensions add their steps'
   states.
5. Every decision and sign-off is human-only: catalogue class `review`, never reachable from MCP (`review_guard`),
   denied to API tokens by the console (RFC 0180). MCP can *read* sign-off state.

**Non-goals**

- Steps 4–7 themselves (RFC 0215/0216, ekos-lakehouse). This RFC gives them the gate to check.
- Multi-person approval rules (RFC 0161's policy engine); one operator signs a subject.
- Changing how conflicts, identity or glossary items are decided.

## 3. Design

### 3.1 Subjects and items (`ekos-semantic::review`, pure)

A subject is a function `ledger view → Vec<Item>`; an item is `{ id, label, facts, info, proposed, decision,
outdated, blocking }`. `facts` are **signed** (they enter the digest); `info` is **shown only** — measurements such as
row counts, null counts and distinct counts change with every profile, and a sign-off that went stale on each
re-profile would teach the operator to ignore staleness. Neither ever holds sample values (RFC 0043; profiles already
carry none for PII columns).

| Subject | Items | Facts shown | Decided by | Blocking until decided |
|---|---|---|---|---|
| `scope` | every `Table` (live first, else DDL), keyed by schema + name | class, row count, FK in/out counts, `ModelTable` present | `ekos review scope` in / out | no — default by class |
| `pii` | every column with a PII classification (name or profile) | table, column, category, basis (`name`/`values`), null %, distinct | `ekos review pii` confirm / reject, mask method | no |
| `conflicts` | current `ConflictingEvidence` (`.ekos/conflicts/current.json`) | type, claims, explanation | `ekos conflicts resolve` (RFC 0172) | **yes**, while `open` |
| `identity` | `SameAs` hypotheses | both sides, status; score shown | `ekos_identity_review` (RFC 0029) ¹ | **yes**, while `unconfirmed` |
| `write-behaviour` | `WriteBehaviour` per in-scope table | update class, delete classes, change log | `ekos review write-behaviour` confirm / override + reason | no |
| `glossary` | current `BusinessConcept`s (`[semantics]` on), else none | term, columns, status | `ekos semantics confirm|reject|edit` (RFC 0170) | no |

¹ Identity candidates are decided through the MCP tool today, which an agent can call; the sign-off is still a
person's, on the decisions as shown. A CLI path for identity review is a follow-up.

Tables are the live ones when the project has a database, else the DDL `Table`s that carry a `dialect` (a `Table`
recovered from a PDF has none); one per lower-case name. A conflict about a table out of scope leaves the
`conflicts` subject; one about anything else stays. A conflict is blocking while `open` **and** without an RFC 0211
`explanation` (`pending_migration`, `translation_table`) — the plan's exit counts *unexplained* conflicts.

Table **class** for `scope` comes from ordered name rules, first match wins, configurable:

Built-in rules: `T_*` transient, `I_*` import, `*_Trl` translation, `*Log*` log, `AD_*` config; transient, import
and log are proposed out of scope. A project's own rules are tried first:

```toml
[review.scope]
default-out = ["transient", "import", "log"]   # unset keeps the built-in list
replace-defaults = false                       # true: only the rules below

[[review.scope.classes]]
pattern = "HR_*"
class = "personnel"
```

A class is a proposal shown with its rule; the operator's in/out decision is what counts. Write behaviour items are
limited to tables in scope after decisions, so scoping out a table drops its write-behaviour item — and changes that
subject's digest, as it should.

### 3.2 Decisions (`Custom("ReviewDecision")`)

One keyed object per (subject, item id): `subject`, `item`, `decision` (`in`/`out`, `confirm`/`reject`,
`override`), `mask` (PII: `hash`/`redact`/`none`), `reason`, `reviewed_by`, `reviewed_at`, and `facts_digest` — the
item's own facts digest when it was decided. A new decision is a new version (append-only, last wins). A decision
whose item's facts have since changed is shown as **outdated** and the item counts as undecided again: the operator
agreed to something that no longer exists.

The registry row is `structurally_keyed = true`.

### 3.3 Digest

`item_digest = SHA-256(canonical JSON of {id, facts, decision-without-who/when})`; `subject_digest = SHA-256` over the
sorted `(id, item_digest)` pairs plus the subject name and a `REVIEW_VERSION`. Canonical JSON: keys sorted, no
whitespace, numbers as written by `serde_json`. Who and when are excluded so re-recording the same decision does not
change the digest. Input order never matters (items sorted; a reversed-input test pins it).

### 3.4 Sign-off (`Custom("SignOff")`)

`ekos signoff approve <subject> --digest <hex> [--note]` — the operator passes the digest they were shown (from
`ekos signoff show`), so a change between showing and approving refuses the approval instead of silently signing the
new state. Refused also while any blocking item is open. Writes one keyed object per subject (`subject`, `digest`,
`items` count, `item_ids` digest, `signed_by`, `signed_at`, `note`, `review_version`); a new sign-off is a new
version. `ekos signoff revoke <subject> --reason` writes a revoked version.

**Validity is computed, never stored**: `show`/`status` recompute the digest; equal → `valid`, else `stale` with the
items added, removed or changed since (the signed object keeps the per-item digests, compressed, to diff against).
Nothing runs at compile to invalidate anything, so a compile can never forget to.

### 3.5 Step bar (`ekos workflow status --json`)

```json
{"steps":[
 {"n":1,"id":"analyse","title":"Analyse sources","state":"done","detail":"committed 2026-10-24, fresh"},
 {"n":2,"id":"docs","title":"Documentation","state":"todo","detail":"…"},
 {"n":3,"id":"review","title":"Review and sign-off","state":"partial","detail":"4 of 6 subjects signed, 1 stale",
  "subjects":[{"id":"conflicts","state":"stale","signed_by":"…","changed":3}, …]},
 {"n":4,"id":"requirements", …}, …]}
```

States: `todo`, `partial`, `done`, `stale`, `blocked` (a prerequisite is not done). Step 1 from the committed
manifest and freshness (RFC 0171); step 3 from §3.4. Steps 2 and 4–7 come from `EkosExtension::workflow_steps()` (new,
default none), which returns the same structure; without an extension they show `todo` with "needs ekos-lakehouse".
Steps 4–7 are `blocked` while step 3 is not `done`, and their commands check `ekos_semantic::review::gate` before
writing anything.

### 3.6 Commands

| Command | Class | |
|---|---|---|
| `ekos review list [--json]` | read | subjects with item, decided, blocking counts |
| `ekos review show <subject> [--json] [--undecided]` | read | items, decisions, digest |
| `ekos review scope <table…> --in|--out [--reason]` | review | |
| `ekos review pii <table.column…> --confirm|--reject [--mask hash|redact|none]` | review | |
| `ekos review write-behaviour <table> --confirm | --override <field>=<value> --reason` | review | |
| `ekos signoff show <subject> [--json]` | read | digest, state, changes since |
| `ekos signoff approve <subject> --digest <hex> [--note]` | review | |
| `ekos signoff revoke <subject> --reason` | review | |
| `ekos workflow status [--json]` | read | the step bar |

Every `review`-class entry point is added to `review_guard::HUMAN_ONLY`. MCP gains `ekos_signoff_status` (read-only,
in `SCOPED_TOOLS`? no — it summarises the whole project, so `SCOPE_REFUSED_TOOLS`).

### 3.7 Console

- **Step bar** across the top of a home project's pages, from `workflow status --json`; each step links to its page.
- **Review** page: a tab per subject (§3.1 table), decisions in place through the catalogue (class `review`, the
  reviewer forced to the signed-in person, RFC 0186), a **Sign off** button showing the digest and asking for typed
  confirmation. The Conflicts tab (RFC 0174) is reused for `conflicts`.
- In ekos-lakehouse: `workflow_steps()` reports steps 2, 4–7; its step commands refuse to run with a stale or
  missing sign-off and name the subject.

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | `review` module: subjects, items, digests; `ReviewDecision`/`SignOff` kinds; CLI `review`/`signoff`; human-only guard rows | on the iDempiere testbed: all six subjects listed with counts; a sign-off; a schema change → `stale` with the changed items |
| 2 | `workflow status`, `EkosExtension::workflow_steps()`, `ekos_signoff_status` MCP tool, the gate function | step bar JSON on the testbed; lakehouse step refuses without sign-off |
| 3 | console step bar + Review page | a full review in a browser, sign-off recorded, digest visible |

## 5. Tests

Digest: order-independent, who/when excluded, a fact change changes it. Subjects: scope classes by rule order,
default-out, scoped-out table drops its write-behaviour item; PII items carry no values; conflicts block while open.
Decisions: last wins, outdated when facts change. Sign-off: wrong digest refused, blocking items refuse, valid after
approve, stale after a fact change with the item named, revoke. Guards: every new `review` command in `HUMAN_ONLY`
and `CLASSES`; MCP cannot reach them (existing scanning tests extended).

## 6. Results

**Phase 1** (2026-10-25, iDempiere testbed `idemdocs`: code + docs + Oracle XE, release build, no model call):

| Subject | Items | Blocking |
|---|---|---|
| scope | 903 tables (classes by the built-in rules) | 0 |
| pii | 195 candidates, all by column name (the P2 profile attaches its own classification to the 32 it suppressed) | 0 |
| conflicts | 429 current (17 about out-of-scope tables left the subject) | 195 open and unexplained |
| identity | 25 `SameAs` candidates | 25 unconfirmed |
| write-behaviour | 849 → 850 after `T_REPORT` was put in scope | 0 |
| glossary | 0 (`[semantics]` off) | 0 |

Walk-through: a PII decision (`AD_ORGINFO.EMAIL`, mask `hash`) and a scope decision (`T_REPORT` in) were recorded;
approving `pii` with the digest printed before the decision was refused ("changed since it was shown"); `scope` and
`pii` were signed off on their current digests; `conflicts` cannot be (195 blocking). Then `ALTER TABLE C_BPartner ADD
(EKOS_Contact_EMail VARCHAR2(60))` in Oracle and a full pipeline (build 17 s, recover 263 s, resolve 12 s, compile
32 s, commit 495 s, 0 model calls): `pii` became **stale — 1 added: `C_BPARTNER.EKOS_CONTACT_EMAIL`**, while `scope`
stayed **valid** (a column count is shown, not signed). The column was dropped afterwards.

Each `review`/`signoff` command reads every object and relationship of the ledger: about 25 s and 2.5 GB peak on this
project (144k objects, 218k relationships). Fine for an operator's command; the console will want a cached summary
(Phase 2).

**Phase 2** (2026-10-25, same testbed, release build):

- `ekos workflow status --json` → 1 `done` (fresh, committed 09:49 UTC), 2 `todo`, 3 `stale` ("1 of 6 subjects signed,
  1 stale, 220 blocking item(s)"; `scope` valid, `pii` stale since the Phase 1 column), 4–7 `blocked`.
- The subject summary is cached in `<state>/review/summary.json` under a key over the ledger's entry, object and
  relationship counts (append-only: every write moves the entry count), the current conflict and semantics ids and
  `[review]`: 24.5 s / 2.5 GB to compute, 1.8 s / 333 MB cached (`review list`, `signoff show`, `workflow status`,
  `ekos_signoff_status`). `approve`, `decide` and `revoke` never read it.
- `ekos_signoff_status` over `mcp serve`: `signed_off: false`, the refusal naming five subjects, seven steps. In
  `SCOPE_REFUSED_TOOLS` (it summarises the whole project).
- ekos-lakehouse (`27ed7c0`) reports steps 2 and 4–7 from its ledger facts (marts drafted, tables synced, latest run per
  table and layer); with `[extensions.lakehouse] require-signoff = true`, `lakehouse sync` refused before reading the
  source: "the source model is not signed off — pii: sign-off is stale (its facts changed); conflicts: not signed
  off; …". Its counts are cached the same way (12.1 s → 2.2 s).
- The gate is off by default in the extension: existing PostgreSQL projects have never been reviewed.

Step 1 is source **file** freshness: the Oracle column dropped after Phase 1 does not make it stale, because a live
database is read every build and is not in the manifest (RFC 0211). The `pii` sign-off still names the column because
the ledger has not been rebuilt since.

**Phase 3** (2026-10-25, same testbed, console in token mode, `evals/oracle-idempiere/part2-review/`):

- Console: the step bar on every project page (`GET /workspaces/{id}/workflow`), and a **Review** tab
  (`GET /review`, `GET /review/{subject}`, `POST /review/{subject}/decide|signoff|revoke`). Reads need `read`;
  writes need `review`, which no API token holds, run the human-only CLI commands (targets after `--`), and record
  the signed-in person. Sign-off posts the digest the page showed and needs the subject's name typed.
- `ekos review identity <match…> --confirm|--reject [--note]` (human-only, `HUMAN_ONLY`): a new version of the
  `SameAs` relationship with `status`, `reviewed_by`, `reviewed_at`, plus an event citing evidence — what
  `ekos_identity_review` writes, made by a person.
- The cache now holds every subject's items (`<state>/review/state.json`, ~1 MB on iDempiere), so `review show` is
  cached too, and every review write recomputes it from the objects it loaded plus what it wrote; a test holds that
  equal to a fresh read, including a scope decision that changes other subjects.
- Browser walk-through, 18/18: a scope decision turned the scope sign-off stale naming the item; scope, PII, write
  behaviour and glossary signed; identity and conflicts refused while items block. The console's read and write
  time limits for these routes are 300 s (the first read after a compile takes ~25 s).

Not done: a full iDempiere sign-off (195 conflicts and 25 identity candidates for a person to decide); the review page
in `deploy/e2e/s8_browser.py`.
