# RFC 0253 — People as a source: chat channels, attachments and testimony in the ledger

**Status:** Accepted (2026-11-10, owner's request to start work; built in order 0252 → 0253 → 0254)
**Date:** 2026-11-09
**Depends on:** RFC 0187 (console chat with EKOS tools), RFC 0180 (users, groups, roles), RFC 0181 (read scope),
RFC 0183 (metered calls), RFC 0023/0025 (local documents, OCR, HTML, e-mail), RFC 0022 (Confluence), RFC 0219
(ticket anchors), RFC 0221 (docs repository and wiki snapshot), RFC 0178 (`[[sources]]`), RFC 0043 (redaction),
RFC 0170 (`EnumMeaning`, cite-or-drop LLM text), RFC 0172 (conflicts), RFC 0213 (review, `HUMAN_ONLY`), RFC 0247
(annotations), RFC 0249 (public-demo safety, moderation)
**Related:** RFC 0252 (profiling), RFC 0254 (open questions — the main reason people are asked anything)
**Supersedes (in part):** RFC 0033's support use case; RFC 0033's history ingestion stays a separate connector

---

## 1. Motivation

RFC 0170's first hard limit is *no trace, no recovery*: meaning that exists only in people's heads cannot be
extracted. It can, however, be **asked for** — and today there is no good way to record the answer:

- RFC 0187's console chat answers questions; it cannot take an answer, an attachment or a link.
- RFC 0247 annotations take a person's knowledge, but only as a form on an entity page or a CLI command, from
  someone who already knows which entity to annotate.
- The people who know — a business owner, the developer who left the team, an accountant — live in Slack, Teams
  or Telegram, not in the EKOS console. They answer with a screenshot, a forwarded PDF, a link to a Confluence
  page or a Jira ticket, or one sentence.

This RFC makes a conversation a source like any other: what a person says and sends is stored with who said it,
where and when, compiled by the normal pipeline, and turned into **evidence** for the hypotheses EKOS already
has. It does not let a model decide anything.

## 2. Goals and non-goals

**Goals**

1. One conversation core, four channels: the console (extends RFC 0187), Telegram, Slack, Microsoft Teams.
2. Two things a person can do in a conversation: **ask** (RFC 0187's agent loop, with citations) and **tell**
   (a statement, an attachment, a link) which EKOS records.
3. Attachments — images, PDF, Word, Excel, PowerPoint, HTML, Markdown, text, e-mail — and links — Confluence,
   Jira, GitHub, any HTML page — become source artifacts compiled by the existing parsers. Images and scans are
   read by **local** OCR only.
4. A person's statement becomes a `Testimony` object; what it means for existing hypotheses becomes proposals
   the person confirms in the same conversation.
5. Every recorded fact carries provenance: person, channel, message link, time, attachment or snapshot.

**Non-goals**

- A model writing to the ledger. A model **proposes**; the write is the explicit action of an authenticated
  person (§3.6). Same rule as RFC 0213 and 0247.
- Ingesting channel history (RFC 0033). Only messages addressed to EKOS (a direct message, a mention, a reply in
  an EKOS thread, a file sent to the bot) are processed.
- A testimony **deciding** a review item by itself. Testimony is evidence; decisions stay in RFC 0213's review,
  with one configured exception (§3.7).
- Voice messages and calls (a later RFC; local speech-to-text would fit the same intake).
- Sending images or documents to a remote model. Recognition is local; a remote chat model sees extracted,
  redacted text only, and only when the project's chat model is remote (RFC 0187 metering applies).

## 3. Design

### 3.1 Conversation core

The core lives in the console (`web/api/app/chat`, extended) and is channel-agnostic:

```text
ChannelMessage { channel, channel_account, conversation_ref, message_ref, permalink?, text,
                 attachments: [Attachment], links: [Url], reply_to?, sent_at }
Attachment     { name, media_type, size, fetch: <adapter-specific handle> }
```

An adapter only translates transport: receive events, download files, post replies, render action buttons. The
agent loop, intake, testimony and proposals are shared. Conversations, messages and their state reuse RFC 0187's
`Conversation` / `ChatMessage` tables with a `channel` column.

### 3.2 Identity and binding

- **Accounts.** A channel account (Slack user id, Teams AAD object id, Telegram user id) is linked to a console
  user (RFC 0180) once: the person writes `/ekos link` to the bot, receives a one-time code, and enters it in
  the console while signed in. Teams accounts in the same tenant as the console's OIDC issuer may be linked
  automatically by AAD object id (setting, off by default).
- **An unlinked account can do nothing** except receive the link instructions. No reads, no intake.
- **Projects.** A Slack/Teams channel or a Telegram group is bound to one workspace/project by an owner in the
  console; a direct message uses the person's chosen default project (`/ekos project <name>`).
- **Permissions** are the linked user's: RFC 0181 read scope on every tool call, RFC 0180 role for every write.
  A person who may only read can ask, and can send material, but their material is held in the inbox as
  `pending` until a writer accepts it (§3.3).
- **Group chats.** In a channel with several people, answers to a question are rendered so that nobody sees
  what their own read scope would hide: a reply goes to the asking person (ephemeral message in Slack, a
  personal reply in Teams, a DM in Telegram) when the answer used anything outside the channel's **common**
  scope (the intersection of the linked members' scopes), else to the thread.

### 3.3 Intake: attachments and links become source artifacts

Everything a person sends is stored in a per-project **inbox source** and compiled by the normal pipeline,
never parsed ad hoc in the chat:

```text
<project>/inbox/<yyyy-mm>/<sha256-prefix>/<original-name>
<project>/inbox/<yyyy-mm>/<sha256-prefix>/meta.json   # who, channel, permalink, time, status, source url
```

The inbox is registered as `[[sources]] kind = "inbox"` (RFC 0178) and observed by the local-documents
connector. Identical files (same hash) are stored once and gain another `meta.json` entry.

| Input | Handling |
|---|---|
| PDF, DOCX, HTML, Markdown, text, e-mail | existing `localdocs` parsers (RFC 0023/0025) |
| PNG, JPEG, TIFF, WebP, scanned PDF | **new `ImageParser`** in `localdocs` over the existing `OcrEngine` (`TesseractOcr`); a standalone image becomes a `Document` with one `Section` per recognized block, with OCR confidence. Language packs from `[ocr] languages` (default `eng+rus+srp`) |
| XLSX, CSV | **new `SheetParser`**: each sheet a `Table`-like `Document` section with header and up to `[inbox] sheet_rows` rows as text; cells pass redaction |
| PPTX | **new `SlidesParser`**: slide text and notes; embedded images through OCR |
| Confluence page URL | fetched once through the RFC 0022 connector's credentials, stored as a snapshot (RFC 0221 snapshot format) |
| Jira issue URL | fetched once (summary, description, comments, status, assignee) and stored as a snapshot; the key also joins RFC 0219 ticket anchors |
| GitHub issue / PR / file URL | fetched through the RFC 0020 client, stored as a snapshot |
| Any other `https` URL | fetched as HTML (RFC 0025 parser), stored as a snapshot with the final URL and fetch time |

**Raw-content entry point.** The inbox is the fourth raw-content entry point. Like the other three (RFC 0157 §
Redaction), it calls `ekos_common::redaction::redact` on every extracted text before anything is held, hashed,
logged, indexed or persisted. The original file stays in the inbox, **outside** the ledger, so it can be deleted
(§3.9); the ledger only ever holds redacted extracted text.

**Limits and safety.** Allowed media types are a closed list; size caps per file and per day per account;
archives are not expanded; nothing is executed or rendered (no macros, no scripts, no remote images). URL
fetching refuses private, loopback and link-local addresses unless the host is on `[inbox] allow_hosts` (an
internal Confluence), follows at most 3 redirects, and never sends the person's channel credentials. In a public
deployment, RFC 0249 moderation runs on intake as well.

**Pending material.** Material from a person without the `write` role, or from any person when `[inbox]
require_accept = true`, is stored with `status: pending` and is not compiled until a writer accepts it in the
console's Inbox page (one click, recorded with who and when).

After a successful intake the bot replies with what it read: document title, pages/sheets, OCR confidence,
what was redacted (counts, not values), and that the material will be compiled on the next run (or now, if the
person may run `ekos build`).

### 3.4 Testimony

A statement a person makes to EKOS is recorded as a keyed `Custom("Testimony")` object (registered in
`custom_kinds::REGISTRY`, `structurally_keyed: true`; key = channel + message ref):

| Property | |
|---|---|
| `author` | console user (subject), never the raw channel id |
| `channel`, `permalink`, `sent_at` | provenance |
| `text` | redacted statement |
| `attachments` | inbox documents sent with it (`Mentions` edges to their `Document` objects) |
| `about` | entity ids the person confirmed the statement is about (§3.5) |
| `answers` | an RFC 0254 `OpenQuestion` id, when it is a reply to one |
| `status` | `active` / `withdrawn` (withdrawal is a new version; the ledger stays append-only) |

Testimony is evidence: a `KirEvidence` with `location = testimony://<id>` and the person's name in the citation.
Every answer that relies on testimony says so: *"According to Ivana Petrović (Slack, 2026-11-10): status 7 is a
customer cancellation."*

Not every message is testimony. A question to EKOS is not recorded. A message becomes testimony only through the
author's explicit action (§3.6).

### 3.5 From a statement to proposals

When a person tells EKOS something, the chat model (RFC 0187, read-only tools) does three things and **writes
nothing**:

1. **Resolve what it is about** with read tools (`ekos_search`, `ekos_semantics_lookup`, `ekos_profile`) —
   "status 7 in the orders table" → `orders.status`.
2. **Extract proposals**, cite-or-drop (the RFC 0170 `semantics_llm.rs` discipline: every proposal must quote
   the span of the statement or the attachment it comes from, or it is dropped):

| Proposal | Becomes, on confirmation |
|---|---|
| a code value's meaning | evidence on the `EnumMeaning` (new meaning source `testimony`), or a new `EnumMeaning` for a data-only value (RFC 0252) |
| a description of a table, column or concept | evidence on the entity's description (RFC 0234) |
| a business term or synonym | an RFC 0247 `synonym`, subject to its approval rule |
| a rule ("amounts are in cents", "fiscal year starts in April") | an RFC 0247 `instruction` in its closed rule vocabulary, subject to its approval rule |
| a remark | an RFC 0247 `comment` |
| an answer to an open question | `answers` on the testimony (RFC 0254) |

3. **Show them back** as a card with buttons, in the channel's own UI (Slack Block Kit, Teams Adaptive Card,
   Telegram inline keyboard, console buttons):

```text
I understood:
  • orders.status = 7  means  "cancelled by the customer"        [from your message]
  • orders.amount is stored in cents                              [from invoice-spec.pdf, p. 3]
[Save]  [Edit]  [Discard]
```

### 3.6 The write is the person's

`Save` is handled by a channel action endpoint that:

1. authenticates the action as coming from the **author** of the statement (the channel's signed request, the
   linked account) — nobody else's click counts;
2. writes the `Testimony` and the selected proposals with `author` = that person;
3. records the action in the audit log (RFC 0180) with the channel and message link.

The endpoint and its writer functions are listed in `review_guard::HUMAN_ONLY`; the chat agent's tool list never
contains them (RFC 0187's test that scans tool lists is extended to every channel), and no MCP tool reaches
them. `Edit` opens a form (a Slack modal, a Teams task module, a console dialog; in Telegram a reply with the
corrected text) — the edited text, not the model's, is what is saved.

A model can therefore never put words in a person's mouth: what is saved is either the person's own text or a
proposal the person saw and accepted, and both are attributed to the person, with the original message linked.

### 3.7 Testimony and review

Testimony feeds RFC 0213 review subjects as **evidence shown to the reviewer** (the `glossary` item for
`orders.status = 7` lists the testimony with its author and link). It does not decide the item.

One exception, configured per project: `[testimony] decides_for = ["reviewer"]`. When a person with that role
on the project saves a proposal that maps to a review item, the save is also recorded as that person's review
decision (`ReviewDecision` with `basis: testimony`). Sign-off still binds to the digest as RFC 0213 defines.

A testimony that contradicts other evidence (a different meaning for the same code, a unit that disagrees with
a column comment) produces an RFC 0172 `ConflictingEvidence` of type `testimony_mismatch` with both claims — it
never overwrites.

### 3.8 Channels

| | Telegram | Slack | Microsoft Teams | Console |
|---|---|---|---|---|
| Transport | Bot API, long polling (no inbound endpoint needed) or webhook | Socket Mode (no inbound endpoint) or Events API | Bot Framework / Azure Bot, inbound HTTPS required | RFC 0187 SSE |
| Addressed messages | DM; mention or reply in a bound group | DM; app mention; replies in an EKOS thread | personal chat; @mention in a bound channel | the chat page |
| Files | `getFile` | `files.info` + download with the bot token | attachment download URL with the bot token | upload |
| Buttons | inline keyboard + callback query | Block Kit + interactivity | Adaptive Card `Action.Execute` | buttons |
| Private replies | DM | ephemeral message | personal reply | always private |
| Secrets | `EKOS_TELEGRAM_TOKEN_ENV` | bot + app-level tokens | app id + secret (Key Vault in the Azure module) | — |

Secrets are referenced by environment variable name and stored in the keychain (RFC 0184), like the chat model's
key. Adapters run as one console background service (`ekos-channels`), started only when a channel is
configured; local mode works with Telegram and Slack without any public endpoint.

### 3.9 Retention and deletion

- Inbox files (originals) follow `[inbox] retention_days` (default: keep). `ekos inbox purge <id>` deletes the
  original and its snapshots; the compiled, **redacted** text in the ledger stays (append-only), and the
  `Document` is marked `source_deleted`.
- A person can withdraw their own testimony (`/ekos withdraw` on the message, or the console). Withdrawal is a
  new version; readers take the newest; answers stop citing it.
- Because the ledger cannot forget, redaction on entry (§3.3) is mandatory and tested, and a deployment that must
  honour erasure requests is told plainly (in the operator docs) that only the inbox originals are erasable.

### 3.10 Surfaces

- Console: Chat (attachments, links, proposal cards), **Inbox** page (pending material, accept/reject, purge),
  channel bindings and account links under Settings.
- CLI: `ekos inbox list|accept|purge`, `ekos testimony list|show|withdraw`, `ekos channels status`.
- MCP: read-only `ekos_testimony` (by entity, author or question; in `SCOPED_TOOLS`).
- Docs: RFC 0214 entity pages gain "What people said" (testimony citations), next to RFC 0247's "Notes from
  people".

## 4. Decisions

| Decision | Alternative rejected | Why |
|---|---|---|
| Attachments go through an inbox source and the normal compile | Parse in the chat handler and write facts directly | One parsing and redaction path; provenance and caching for free; nothing bypasses `recover` |
| Local OCR only | A vision model reads images | Screenshots of ERPs are full of personal data; RFC 0043's guarantee would end at the first upload |
| The author's explicit Save writes | The model writes high-confidence extractions | RFC 0213/0247 rule: decisions and statements are human; a model must not put words in a person's mouth |
| Testimony is evidence, not a decision | Any answer closes the item | A wrong answer from one person would silently become truth; review stays where accountability is |
| Unlinked accounts can do nothing | Read-only for anyone in a bound channel | Read scope is per person; an unlinked account has no scope |
| One core, thin adapters | A connector crate per platform with its own logic | Behaviour (safety, proposals, provenance) must be identical in every channel; only transport differs |

## 5. Tests (written first)

- Intake round trip per media type, including a standalone PNG through `ImageParser` with a fixture image of a
  known text; a seeded e-mail address and card number in an image are absent from the ledger after compile.
- URL fetch: private and loopback addresses refused; redirect cap; a Confluence and a Jira fixture snapshot.
- Pending: material from a reader is not compiled until accepted.
- Proposals: a statement with a quotable span yields a proposal with the span; a proposal without a span is
  dropped (scripted provider).
- Human-only: the Save endpoint refuses a click from anyone but the author; its writers are in `HUMAN_ONLY`;
  no channel's tool list contains a writer (scan, every adapter).
- Testimony mismatch: a meaning that disagrees with a column comment creates `ConflictingEvidence`
  (`testimony_mismatch`), and the original meaning is unchanged.
- Group scope: an answer using a fact outside the channel's common scope is delivered privately.
- Adapters: recorded request/response shapes for Telegram, Slack and Teams (events, file download, button
  callbacks, signature verification).
- Withdrawal: a withdrawn testimony disappears from answers and docs; the ledger has both versions.

## 6. Phases

1. Inbox source, `ImageParser`/`SheetParser`/`SlidesParser`, URL snapshots; console chat with attachments;
   `Testimony`, proposals, Save; `ekos_testimony`.
2. Telegram adapter (simplest transport, no public endpoint).
3. Slack adapter (Socket Mode first).
4. Teams adapter and the Azure module's Bot resource (RFC 0232).

## 7. Open questions

1. Should `ImageParser` use a layout-aware OCR mode for screenshots of tables (Tesseract PSM 6 vs 3), or detect
   tables first? Measure on real ERP screenshots.
2. Is one-time-code linking enough for Teams in enterprises, or is AAD auto-linking the default there?
3. Should a testimony be able to target a person (an owner) as "ask X" — i.e. forwarding — or is that RFC 0254's
   routing alone?
4. Rate limits per channel for proposals: one card per statement, or batch within a thread?

## 9. Implementation log

### 2026-11-10 — Phase 1: `ImageParser`, `SheetParser`, `SlidesParser`

**What.** Three new parsers in `ekos/plugins/localdocs` (§3.3), registered by
`LocalDocsObserver::with_settings`, which `ekos build` now uses:

| Parser | Files | Sections |
|---|---|---|
| `ImageParser` (`image.rs`) | `.png`, `.jpg`/`.jpeg`, `.tif`/`.tiff`, `.webp` | one per paragraph Tesseract recognises (TSV output grouped by page/block/paragraph), each with `ocr_confidence` = the mean word confidence (0–100, one decimal); the document carries the mean of its sections |
| `SheetParser` (`sheet.rs`, `calamine` 0.38 + `csv` 1.4) | `.xlsx`, `.csv` | each sheet (a CSV is one, unnamed) as a pipe table — header plus up to `[inbox] sheet-rows` rows — split at ~2,500 characters with the header repeated; also one `tables` entry per sheet |
| `SlidesParser` (`slides.rs`, `ooxml.rs`) | `.pptx` | each slide, in `p:sldIdLst` order: title (section heading), body text, then `Notes:` and the speaker notes; slide-number/date/footer placeholders dropped; raster pictures (PNG/JPEG/TIFF/WebP) go to the observer's existing OCR path, vector pictures (EMF/WMF) do not |

`OcrEngine` gained `recognize_blocks` (default: `recognize` split on blank lines, no confidence);
`TesseractOcr` overrides it with `tesseract … tsv`. `TesseractOcr::with_languages` checks
`[ocr] languages` (default `eng+rus+srp`) once against `tesseract --list-langs`, drops missing packs
with one warning and falls back to `eng`, then to any installed pack. No Tesseract: an image is
`ParseError::Skipped` — a warning, not counted as an error (the scanned-PDF path keeps its existing
soft-skip). Config: `[ocr] languages`, `[ocr] images` (default `true`), `[inbox] sheet-rows`
(default 200), all three in the build fingerprint key only when changed. `PIPELINE_LOGIC_VERSION`
5 → 6; `LocalDocAnalyzerPass` `v4` → `v5` (`ocr_confidence` on `Document` and `Section`).

**Redaction.** Every string the three parsers extract goes through
`ekos_common::redaction::redact` (built-in RFC 0043 patterns) and the new
`redact_personal_data` — e-mail addresses and Luhn-valid 13–19-digit card numbers (spaces and dashes
allowed) → `[REDACTED:email]` / `[REDACTED:payment-card]` — before it is held; OCR text of images
embedded in PDF/DOCX/PPTX gets the same. `ekos build` still redacts every artifact again with the
workspace's `[security]` patterns.

**Measured.** Fixture `plugins/localdocs/tests/fixtures/order-status-scan.png` (900×360, DejaVu Sans
28 px, rendered with PIL; two paragraphs: "Order status codes / Status 7 means cancelled by the
customer." and "Contact: jane.doe@example.com / Card: 4111 1111 1111 1111"), Tesseract 4.1.1 with `eng`
only (this machine has no `rus`/`srp`): block confidences **95.5** and **84.8**. Tesseract read the
card number as `4111 1111 11111111` (two groups run together) — still 16 Luhn-valid digits, still
redacted; both the address and the number are absent from the parser's output.

**Departures.**
- The RFC names the setting `[inbox] sheet_rows`; config is kebab-case throughout, so it is
  `sheet-rows`.
- E-mail/card redaction is a separate function, not part of `redact`: adding it to the build-wide
  baseline would rewrite commit authors and code everywhere. It is applied to what the new parsers
  extract and to OCR text only. PDF/DOCX/HTML/e-mail prose is unchanged.
- `[ocr] images` was added (not in the RFC) so a repository full of icons is not OCR'd on every
  build; it does not affect images embedded in documents.
- A slide's pictures are OCR'd into the document's `ocr_text` (the existing embedded-image path),
  not into the slide's own section.
- Not done here: the inbox source itself, URL snapshots, the intake reply (pages/sheets/confidence
  counts) — other Phase 1 items.

### 2026-11-10 — §3.3 inbox, §3.4–3.7 testimony (CLI and ledger)

| Part | What |
|---|---|
| `ekos inbox add <file\|url> --as <person>` | originals under `<state>/inbox/<pending\|accepted>/<yyyy-mm>/<sha256[..16]>/` with `meta.json` (senders, channel, permalink, source URL); identical content stored once, gaining a sender; a closed extension list; `[inbox] max-file-mb`; `--pending` or `[inbox] require-accept` holds it |
| links | fetched once as an HTML snapshot: http(s) only, ≤ 3 redirects each re-checked, refused when the host resolves to a loopback, private, link-local, CGNAT or ULA address unless listed in `[inbox] allow-hosts` |
| compile | `ekos build` observes `<state>/inbox/accepted` as the source `inbox` with the local-documents connector alone (images always through local OCR) and scrubs every string of every artifact with `redact_personal_data` before the usual redaction — **found live**: a Markdown note's e-mail address reached the ledger, because only the new image/sheet/slide parsers scrubbed personal data |
| `ekos inbox list \| accept \| purge` | accept and purge are human-only; purge deletes the original, logs it in `purged.jsonl`, and says plainly that compiled, redacted text stays |
| `ekos testimony add` | keyed `Testimony` (channel + message ref), text redacted, `about` (`Describes` links), `answers` (an RFC 0254 question), `meaning` (a code value's meaning), `on_behalf_of` (an operator recording a workshop answer), `--probe` (RFC 0254: re-run, counts stored as evidence); evidence `testimony://<id>` reading "According to <who> (<channel>, <date>): …". Human-only |
| meaning | a stated meaning is an `EnumMeaning` meaning source `testimony` at the next semantics step; when it differs from the label code gives, a `testimony_mismatch` conflict is recorded at once — the label is never overwritten |
| `ekos testimony list \| show \| withdraw` | withdrawal (author only) is a new version; the newest wins |
| MCP | `ekos_testimony`, read-only, scoped. No MCP tool records testimony |

**Measured** on the LedgerSMB scratch project: a Markdown note and the OCR fixture image went through
`inbox add` (the image pending until `accept`), `build` → `commit`: both searchable, the note's e-mail
address `[REDACTED:email]`. A link to `127.0.0.1` refused. A testimony from Slack answering an
`unexplained_values` question turned it `answered`; its meaning "owner capital" for `account.category
= 'Q'` (code says "Equity") became one `testimony_mismatch` conflict.

`[testimony] decides-for`: when the author is listed, a saved meaning is also their review decision —
an `EnumMeaning` label edit with `review_basis: testimony` (live: `account.category = 'L'` → confirmed,
"Liability"). **Not built here**: the conversation core, proposals, Save from a channel and the Telegram /
Slack / Teams adapters are the console's (a separate entry).
