# Agent session memory — user guide (RFC 0151)

Session memory lets an agent leave notes for its future self — a decision and its *why*, a dead end,
a constraint — tied to real objects in your compiled ledger, and tells the next session when the
thing a note describes has since changed.

It is **opt-in** and **never a source of truth**: every note is an unconfirmed agent claim (`T0`).
It is separate from the default answer paths — `ekos ask`, `ekos query`, `ekos_search`, `ekos_retrieve`
and EKL never return session notes.

## Turn it on

```toml
[session-memory]
enabled = true
# optional: max-note-chars = 2000, max-entries-per-session = 200, max-bytes-per-session = 262144
#           capture-retention-days = 14, extraction = false, inbox-dir = ".ekos/session/inbox"
```

## The loop

```bash
ekos session note "orders.total is stored in cents" --kind decision \
    --rationale "finance reports dollars" --anchor orders      # -> inbox file only, no ledger
ekos session status                                            # counts, dropped, pending
ekos session commit                                            # inbox -> unconfirmed, anchored claims
ekos session recall "how is orders total stored"               # tier + staleness verdict per hit
ekos session brief --scope orders --budget 800                 # token-budgeted, untrusted envelope
ekos session review <claim-id> confirm                         # HUMAN ONLY: T0 -> T1
```

Kinds: `finding | decision | dead_end | constraint | todo`. Anchors must be exact object names or
workspace-relative paths; an ambiguous or unknown anchor is recorded as such and never guessed.

`--scope` (and `--scope-from-git`) is what makes a brief relevant rather than generic: scope
overlap outranks staleness and recency in the ranking, so a note about the thing you are editing
comes first. A scope entry matches an anchor exactly or on the final path segment, so a git path
still lines up with an anchor written as a bare file name.

## Staleness

Each anchor records a fingerprint of the narrow slice of state a note is about (a table's columns, a
symbol's signature, a section's text). At read time a note is `fresh`, `changed` (with what changed),
`orphaned` (the anchor left the ledger) or `unanchored`. A confirmed (`T1`) note whose anchor changed
still shows `changed` — confirmation does not freeze the code.

In the **brief**, only a label that calls for action is printed: a line carries `[CHANGED]` or
`[ORPHANED]` when the code it describes has moved, and `[HUMAN-CONFIRMED]` once a person has
promoted it. Everything else — the common `T0` / `fresh` / `unanchored` case — is stated once by
the preamble and left off the line. Printing every label on every line is what made the marker
invisible in the first live eval (0 of 24 answers flagged it). If the budget hides some notes, the
truncation line says how many of the hidden ones were changed.

The preamble sits **above** the `<session-memory untrusted="true">` envelope, not inside it: the
envelope's whole point is that its contents are inert data, so instructions belong outside it.

## Safety properties (each has a test)

- Secrets are redacted at write **and** again at commit; if redaction fails the note is dropped.
- Inbox files are `0600`, the directory `0700`; anchors or an inbox path that leave the workspace are refused.
- The agent-facing write tool (`ekos_session_note`) opens no ledger handle.
- No MCP tool can confirm, reject or supersede a note.
- Nothing is deleted from the ledger; superseded/rejected notes just leave default ranking.
- Recalled text is wrapped as untrusted data. **Residual risk:** redaction is pattern-based, and a
  committed note cannot be un-committed (the ledger is append-only). `ekos session purge` deletes
  inbox files and slices; ledger claims remain and their evidence reads "source purged".

## Optional: transcript capture and LLM extraction

`ekos session capture` stores redacted transcript slices outside the ledger (retention:
`capture-retention-days`). `ekos session extract` (needs `extraction = true`) asks the `[llm]`
provider for claim proposals; each must quote a span that exists in the slice or it is dropped, and
results are cached by slice checksum. With a cloud provider this is a metered call.

## What is not built / not verified

`SessionStart` context injection is verified live; hook timeout/failure semantics and the `SessionEnd` commit hook are not (`docs/integrations/claude-code-session-memory.md`).
A live comparison against a model-written compaction summary showed **no correctness advantage** and an
inconclusive staleness difference at 7 notes (`docs/evals/session-continuity-2026-09-21.md`, live
section); whether session memory helps at larger volumes is untested.
