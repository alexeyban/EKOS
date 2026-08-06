# RFC 0033 — Discord/Slack Chat Connector

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

Two independent needs converge on the same missing connector. First, DAO governance discussion
routinely happens in Discord/Slack threads before (or instead of) a formal forum post — RFC
0032's treasury-approval matching loses its strongest text-reference signal for any DAO whose
real deliberation lives in chat rather than a Discourse/Snapshot thread. Second, a
support/onboarding use case (a Claude Code agent answering a new community member's question by
citing the exact chat thread where that answer was actually established, instead of guessing or
citing stale docs) needs the same underlying data: accumulated, evidence-backed chat history.

Nothing in EKOS today observes a chat platform. Every existing connector observes either a file
tree (`file`, `git`), a structured API over discrete items (`github`'s issues/PRs, `confluence`'s
pages, `salesforce`'s objects), or a database. Chat is structurally closer to GitHub issues (many
independent, timestamped, threaded items with authors and bodies) than to a file tree, so this RFC
follows RFC 0020's shape closely rather than inventing a new connector pattern.

## Scope

- A connector observing message history from a Discord server (guild) and/or a Slack workspace,
  scoped to specific channels.
- Message content becomes searchable, evidence-citable `Object`s, with thread structure preserved
  as `Relationship`s.
- Free-text mention extraction (proposal ids, issue numbers, transaction hashes) reusing the
  existing keyword-scan pattern, so this connector's output directly strengthens RFC 0032's
  text-reference matching signal without RFC 0032 needing any chat-specific code of its own.

## Non-goals

- Real-time streaming ingestion (a gateway/websocket connection reacting to new messages as they
  arrive). Every existing `Observer::scan` is a pull-based, point-in-time scan; this connector
  follows that model — periodic re-`ekos build` picks up new messages via `since`-based
  incremental fetch, not a persistent connection. Real-time is a plausible future RFC, not this
  one.
- Voice channel transcription, reactions-as-signal beyond a raw count, or any message content
  moderation/redaction beyond what a future redaction pass (referenced in `TODO.md`'s "Secrets
  management and sensitive-data policy" item) already covers project-wide.
- A unified Discord+Slack connector crate — see Alternatives Considered.

## Design

### `ChatMessage` — shared shape between Discord and Slack

```rust
pub struct ChatMessage {
    pub message_id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub author: String,
    pub body: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub reactions_count: u32,
}
```

### `DiscordObserver` / `DiscordClient` and `SlackObserver` / `SlackClient` — two crates, one shape

Each follows the `Observer` trait (`ekos/crates/observation-sdk/src/lib.rs`) and the
constructor-injected client-trait pattern exactly as `GitHubObserver`/`GitHubClient` and
`ConfluenceObserver`/`ConfluenceClient` do:

```rust
#[async_trait]
pub trait ChatClient: Send + Sync {
    async fn list_messages(
        &self, channel_id: &str, since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<ChatMessage>, ChatClientError>;
}
```

`RealDiscordClient` calls Discord's documented `GET /channels/{id}/messages` (bot-token auth);
`RealSlackClient` calls Slack's documented `conversations.history` (bot-token auth). Each ships a
`MockDiscordClient`/`MockSlackClient` exercising the real mapping logic with zero network
dependency — the same two-tier discipline `ekos/plugins/confluence/src/lib.rs`'s doc comment
states explicitly for every connector in the codebase.

One `ObservationArtifact` per message (matching "one issue/PR = one artifact" from RFC 0020's
`GitHubObserver`), not per thread — keeps individual messages independently evidence-citable
rather than bundling a whole thread into one opaque blob.

### Recovery pass

`Object { kind: Custom("ChatMessage"), properties: {channel_id, channel_name, author, body,
timestamp, reactions_count} }` per message. `Relationship { kind: Custom("Replies"), from: reply,
to: parent }` reconstructed from `reply_to`/`thread_id`, giving thread structure as first-class
graph edges rather than leaving it implicit in a `thread_id` string property.

**Mention extraction reuses `github_analyzer.rs`'s keyword-scan pattern directly**
(`ekos/crates/recovery/src/github_analyzer.rs`'s close-keyword scan is the template): scan
`body` for patterns matching a proposal id, an issue reference (`owner/repo#123`), or a
transaction-hash shape, emitting a `References` relationship to the matching object when one
exists in the ledger. This is what makes this connector valuable to RFC 0032 (a chat message
mentioning a tx hash strengthens that payment's text-reference signal) without RFC 0032 needing
any chat-specific logic — the `References` relationship shape is already generic.

Deterministic id: `Uuid::new_v5(NAMESPACE, "discord:{guild_id}:{channel_id}:{message_id}")` (or
`"slack:{workspace_id}:{channel_id}:{message_id}"`) — same determinism discipline as every other
connector, so re-running `ekos recover` converges rather than duplicating.

### Volume and noise — the actual design risk

Unlike a git repo's finite file tree or a bounded set of GitHub issues, a chat channel is
high-volume and grows without bound, and most messages are noise (small talk, single-emoji
reactions, off-topic chatter) relative to a git repo or Confluence space. Two mitigations designed
in from the start rather than retrofitted later:

1. **Per-channel opt-in, not organization-wide ingestion.** `ekos.toml`'s
   `[connectors.discord]`/`[connectors.slack]` config takes an explicit channel allowlist (e.g.
   only `#governance`, `#treasury`, `#support`), not "every channel the bot can see." This is a
   scope decision, not just a performance one — ingesting a general-chat channel into an
   append-only, evidence-backed ledger has real privacy implications for people who never expected
   their small talk to become permanent, queryable knowledge.
2. **Incremental sync via `since`.** The client trait's `since` parameter, combined with
   `ScanContext`'s existing fingerprint/skip-unchanged mechanism
   (`ekos/crates/observation-sdk/src/lib.rs`'s `source_fingerprint`, built for Phase 13's
   Optimizer), means a re-run of `ekos build` fetches only messages newer than the last successful
   scan per channel, not full history every time.

### Auth/config

Bot token via `password_env`-style config reference in `ekos.toml`
(`token_env = "DISCORD_BOT_TOKEN"` / `"SLACK_BOT_TOKEN"`), never a literal value — the same
secrets discipline `TODO.md`'s "Secrets management and sensitive-data policy" phase item
establishes project-wide (`ekos doctor` verifies the referenced env var exists).

## Alternatives Considered

- **One connector crate for both Discord and Slack, parameterized by platform.** Rejected in favor
  of two separate crates sharing the `ChatMessage` type, matching the existing
  `sql-dialect-mysql`/`sql-dialect-postgres` precedent (`ekos/plugins/`) — the two platforms'
  auth flows, pagination models, and rate-limit behavior differ enough that one crate would need
  internal branching everywhere, where two crates sharing a data type keep each implementation
  simple and let either ship independently.
- **Ingest full unrestricted channel history by default.** Rejected — see the volume/noise design
  section above; explicit per-channel opt-in is both a scale and a privacy decision, not a v2
  nice-to-have.
- **Real-time gateway/websocket ingestion instead of polling `scan`.** More timely, but breaks the
  "every `Observer::scan` is a point-in-time, side-effect-free, repeatable pull" contract every
  other connector satisfies, and would need its own long-running-process design (outside the
  `ekos build` CLI-invocation model entirely). Left as a non-goal for a future RFC if periodic
  polling proves too stale for the support-bot use case in practice.

## Open Questions

- [ ] Does Discord or Slack ship first? (Affects nothing architecturally — the shared
      `ChatMessage` shape means either order works — but affects which real API gets exercised
      first.)
- [ ] What's the right default `since` lookback window on first `ekos build` against a channel
      with years of history — all of it, or a bounded initial window (e.g. 90 days) with older
      history backfilled on request?
- [ ] Should reactions beyond a raw count (e.g. which emoji, from whom) be captured? Deferred here
      as likely low-value relative to the privacy cost of capturing more per-user detail than
      necessary.
- [ ] Interaction with a future redaction pass (`TODO.md`'s Phase item): chat is the connector
      most likely to contain PII/secrets pasted informally — should this connector be blocked from
      being marked "Accepted" until that redaction pass exists, rather than shipping ahead of it?

## Testing

- `MockDiscordClient`/`MockSlackClient`-driven tests exercising each `Observer::scan`'s mapping
  logic against a fixed message-history fixture, zero network dependency.
- A dedicated test asserting `Replies` relationships correctly reconstruct thread structure from a
  fixture with nested replies.
- A dedicated test asserting mention extraction emits `References` relationships for a fixture
  message body containing a known issue reference and a known transaction-hash-shaped string.
- A test asserting the per-channel allowlist in `ekos.toml` config is honored — a channel not in
  the allowlist produces zero artifacts even if the mock client would return messages for it.

## Acceptance Criteria

- [ ] All Open Questions resolved, including the redaction-pass sequencing question.
- [ ] At least one review completed.
- [ ] `DiscordObserver`/`DiscordClient` and `SlackObserver`/`SlackClient` each pass a
      `Mock*Client`-driven test suite with zero network dependency.
- [ ] Thread-structure and mention-extraction tests described above pass.
- [ ] Per-channel allowlist config is enforced and tested.
- [ ] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants.
