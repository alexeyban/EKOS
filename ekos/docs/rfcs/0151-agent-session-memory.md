# RFC 0151 — Agent Session Memory

**Status:** Accepted, all phases implemented 2026-09-21 (devlog_197). Phase 5 GO is on a deterministic proxy and is conditional on a live-model run (see Results).
**Plan:** `todo-agent-session-memory.md` (phases P0–P8). **Findings:** `docs/spikes/session-memory-findings.md`.

## Problem

An agent session learns things (a dead end, a constraint, a decision and why) that vanish at
`/compact` or session end. EKOS already compiles code and documents into an evidence-backed ledger;
it has nowhere to put what an *agent* concluded about that code, and no way to say "this conclusion
was about `orders`, and `orders` has since changed".

## Design (summary)

1. **Inbox, not ledger.** Notes are written to a capped, redacted, append-only JSONL inbox
   (`.ekos/session/inbox/<session_id>.jsonl`). The write path never opens a ledger handle.
2. **Ledger only through the pipeline.** A `SessionObserver` emits content-addressed artifacts from
   sealed inbox segments; a deterministic pass maps them to `Custom("Session")`,
   `Custom("Claim")` (`claim_type: "session_note"`) and `Custom("DeadEnd")`. No new KIR primitive.
3. **Trust tiers.** Every session claim enters as unconfirmed (`T0`). Promotion to `T1` is human-only
   (`ekos session review`, CLI, no MCP twin).
4. **Anchors and staleness.** A note may anchor to real objects by exact-match; a per-kind fingerprint
   recorded at pin time lets read paths report `fresh / changed / orphaned / unanchored`.
5. **Isolation.** Default answer paths (`ekos_query`, `ekos_retrieve`, `ekos ask`, the RFC 0126
   ranking gate) are unaffected by session claims; they surface only through `ekos_session_*`.

## Phase 1 scope (this RFC's first implementation)

- `ekos-session` crate: entry schema (`schema: 1`), content-hash `entry_id`, per-session JSONL,
  `O_APPEND`, size/count caps with dropped-entry counting, redaction at the single write choke point
  (entry dropped, nothing written, if redaction fails or leaves nothing), workspace-boundary and
  session-id validation, user-only file modes.
- Config `[session-memory]` (kebab-case like every other section), `enabled = false` by default.
- CLI `ekos session note` and `ekos session status`.

## Non-goals (Phase 1)

Any ledger write, any MCP tool, any hook, transcript capture, LLM extraction.

## Security

`ekos_session_note` (Phase 6) will be the only agent-writable surface and is inbox-only. `SECURITY.md`
and `CLAUDE.md` invariants are amended in the same PR that ships it, not before.

## Open questions

3. Fingerprint noise on a real ledger (measured in Phase 4).
6. Retry/backoff policy under writer-lock contention (policy chosen in the spike; verified in Phase 2).
Questions 1 and 2 are resolved in the spike (ledger, per-workspace).

## Deviations from the first draft (as built)

| Draft | As built | Why |
|---|---|---|
| Notes are `Custom("Claim")` with `claim_type: "session_note"` | New kind `Custom("SessionClaim")` (registered, structurally keyed) | `architecture_evaluator`, identity and `ekos_architecture_review` all read `Custom("Claim")`; sharing the kind would change architecture-confidence scores. |
| `DeadEnd` as an object kind | A `SessionClaim` with `note_kind: dead_end` plus an `EventKind::Custom("DeadEnd")` event | one identity per note; the event keeps the dead end queryable as an event. |
| `SessionObserver` implements `Observer` | A pure `observe()` step inside `ekos session commit` | the standard `build` walk has no notion of an inbox; the step still re-redacts and content-hashes the batch (an RFC 0043 entry point). |
| `Runtime::session_claims` | Free functions over `&dyn KnowledgeStore` in `ekos-session::read` | keeps `Runtime` unmodified; `Runtime::find_objects`/`retrieve` do gain a filter that hides session kinds. |
| `ClaimStatusChanged` reducer over events | Status stored on the re-appended claim + a `ClaimStatusChanged` event | the trait has no "all events" read; this is the `architecture_review` pattern. |
| Hybrid (BM25 + vector) recall | Deterministic lexical overlap with stopwords + light stemming | no embedding dependency for a small per-workspace corpus; recall quality is measured only by the proxy eval. |
| `brief` includes "changed since last session" | One line: new notes since the previous brief + notes whose anchors are *currently* changed | the ledger does not record when an anchor moved relative to a note, so it is a state count, not a diff. |

## Threat model (Phase 8)

Assets: the append-only ledger, the redaction guarantee, the reader agent's trust.

| Threat | Control | Test |
|---|---|---|
| Secret in a note reaches the ledger | redact at write, redact again at observation, drop on redactor failure | `secrets_are_redacted_before_the_write`, `hand_edited_inbox_secret_is_redacted_again_at_observation`, `failing_redactor_writes_nothing` |
| Prompt injection in a note steers a later agent | `T0` label, untrusted envelope, no MCP promotion, recall never says "closest" | eval `poisoned_leak_rate`, `session_tools_are_listed_only_when_enabled_and_there_is_no_mcp_promotion_tool`, `no_mcp_code_can_reach_the_lifecycle_module` |
| Agent write path becomes a ledger write path | `ekos_session_note` opens no ledger handle | `session_note_tool_writes_only_the_inbox_and_opens_no_ledger` |
| Flooding | per-note, per-session count and byte caps, dropped counter | `entry_count_cap_drops_and_counts`, `oversize_note_and_byte_cap_are_dropped`, MCP cap test |
| Path traversal / symlink escape | canonicalised inbox and anchors, session-id charset | traversal + symlink tests |
| Crash mid-append | torn last line skipped, next append starts a new line | `truncated_last_line_is_tolerated` |
| Concurrent writers | per-session `create_new` lock file around check-then-append (stale after 10s), `O_APPEND` single write; no torn lines, exact caps/dedupe | `concurrent_sessions_and_writers_never_tear_lines` |
| Session claims skew default answers | kinds hidden in `Runtime::find_objects`/`retrieve` | `session_memory_is_invisible_to_default_retrieval` |

**Known residual risks.** Redaction is pattern-based, so an unrecognised secret shape is still
committed and cannot be removed. A crashed writer's lock file blocks that session's writers for up to 10 seconds
before it is reclaimed. (An earlier version without the lock tore lines under 4 concurrent writers —
found by the concurrency test, not by inspection.) Default
retrieval isolation is by kind filter; BM25 corpus statistics still include session objects, so scores
of real hits can shift slightly even though the returned set is unchanged.

## Results

`docs/evals/session-continuity-2026-09-21.md`. Proxy GO: correctness 1.00 vs 0.62 (modelled compaction
baseline), stale-fact-served 0.00 vs 0.50. **Not established:** any live-model comparison.
