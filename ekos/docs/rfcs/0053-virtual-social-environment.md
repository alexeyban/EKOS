# RFC 0053 — Virtual Social Environment

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Seventh RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
RFC 0052 (Conflict Resolution), and now — at the user's explicit request ("go next phase 11") —
Phase 11 of `EKOS_World_Engine_Development_Plan.md` (§16): a `VirtualForum` giving scenarios a
richer social-interaction surface (channels, replies, likes, follows, shares) than raw actions
alone, explicitly *not* integrating any real platform ("Do not integrate X or Reddit initially" —
the source document's own words).

Checked before designing, same discipline as the prior six RFCs:

- **`ObjectKind::Custom("Channel")`** (RFC 0048) and **`ActionKind::PostMessage`** targeting one,
  with capacity consumption (RFC 0052), already implement two of the source document's seven
  capabilities (`create_channel`, `publish_message`) almost completely — confirmed by re-reading
  `execute_action`'s existing `PostMessage` branch before assuming anything needed to be built from
  scratch.
- **`ActionKind` has no escape hatch, by explicit design** (RFC 0050: "a scope decision, not a
  taxonomy" — the vocabulary is a fixed 12). `reply`/`like`/`follow`/`share` don't fit any of the
  12 as written. Before deciding whether to reopen that design decision, checked what each verb
  structurally *is*: a reply is a message with a parent pointer (an additive field on `Action`, not
  a new kind); `like`/`follow`/`share` are social-graph facts about existing entities (relationship-
  shaped, not action-shaped) — none of them actually need a 13th `ActionKind` variant. RFC 0050's
  closed vocabulary stands unmodified.
- **`KnowledgeStore` has no bulk "every event" query** — confirmed by re-reading the trait
  (`ledger/src/lib.rs`): `all_objects`/`all_relationships` exist, there is no `all_events`, only
  `get_event(id)` by known id. `read_messages(channel)` therefore can't be a scan; it needs an
  index. The fix reuses an existing mechanism rather than adding one: a relationship can point
  *from* an event just as validly as from an object (confirmed by how RFC 0049's `Knows` edges
  already point *at* events) — so every published message gets a `Custom("PostedIn")` relationship
  from the message event to its channel, and `read_messages` is just `relationships_for(channel)`
  filtered and dereferenced. No new `KnowledgeStore` method.

## Scope

1. **`Action.reply_to: Option<KirId>`** — additive field (not a new `ActionKind`); when set on a
   `PostMessage`, the resulting event's payload carries `"reply_to"`, giving the source document's
   own "Alice posts → Bob observes → Bob decides → Bob replies" loop a real, round-based path with
   zero vocabulary changes.
2. **`VirtualForum`** (`crates/simulation/src/forum.rs`) — a thin, *direct* (non-round) API over
   `&dyn KnowledgeStore`, for scenario setup/seeding and for capabilities the closed `ActionKind`
   vocabulary deliberately doesn't cover:
   - `create_channel` — a `Custom("Channel")` object (existing convention, just a named helper).
   - `publish_message` — same effect as `execute_action`'s `PostMessage` branch (capacity check,
     `Custom("ActionExecuted")` event, `PostedIn` indexing relationship), callable outside a round.
   - `like`/`follow`/`share` — `Custom("Likes")`/`Custom("Follows")`/`Custom("Shares")`
     relationships, confirmed facts (no `status` property — an agent either did or didn't, the same
     posture RFC 0049 gave `Knows`), idempotent (repeating one is a no-op, matching `append_knows`'s
     existing dedup pattern).
   - `read_messages` — every message posted to a channel, oldest first, via the `PostedIn` index.
3. **The `PostedIn` index also gets appended by the existing round-based `PostMessage` path** — so
   `read_messages` sees messages posted through a normal `Simulation::run_round` decision loop, not
   only ones posted through the direct `VirtualForum` API. The one piece of genuinely shared logic
   between the two call sites.

## Non-goals

- **No round-based `Like`/`Follow`/`Share`/`Reply`-as-its-own-kind actions.** `ActionKind` stays
  closed at 12 (RFC 0050's own design decision, reaffirmed here, not reopened). An agent replies by
  choosing `PostMessage` with `reply_to` set; liking/following/sharing are direct-API/scenario-setup
  capabilities in this RFC, not something a `DecisionEngine` can choose mid-round. Real, deferred
  work if a future scenario genuinely needs agents to like/follow each other as part of their own
  decision-making — would mean reopening the closed-vocabulary decision deliberately, not
  incidentally, as its own scope call.
- **No X/Reddit (or any real platform) integration** — the source document's own instruction.
- **No nested-thread reconstruction helper.** `reply_to` is a flat pointer to one parent message;
  walking a full reply tree is left to the caller (real, deferred work only if a scenario needs it).

_Both round-based Like/Follow/Share/Reply actions and the nested-thread reconstruction helper are
tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "World Engine"._
- **No new `KnowledgeStore` trait methods** — `PostedIn` reuses `append_relationship`/
  `relationships_for`, already there.
- **No moderation, rate-limiting, or spam-detection semantics** — out of scope for a deterministic
  simulation substrate.

## Design

### `Action.reply_to` (`crates/simulation/src/action.rs`)

```rust
pub struct Action {
    pub kind: ActionKind,
    pub target: Option<KirId>,
    pub content: Option<String>,
    pub reply_to: Option<KirId>,
}
```

Meaningful only for `PostMessage` (any other kind carrying it is simply ignored by
`execute_action`, not an error — the field describes *what this post is a reply to*, not a
generic property every action needs). `Action::new` defaults it to `None`; a new
`.with_reply_to(id)` builder method sets it, mirroring `.with_target`/`.with_content`.

### `VirtualForum` (`crates/simulation/src/forum.rs`)

```rust
pub struct VirtualForum<'a> {
    store: &'a dyn KnowledgeStore,
}

impl<'a> VirtualForum<'a> {
    pub fn create_channel(&self, name: impl Into<String>, capacity: Option<f64>) -> Result<KirObject, ForumError>;
    pub fn publish_message(&self, actor: &KirId, channel: &KirId, content: impl Into<String>, reply_to: Option<KirId>) -> Result<KirEvent, ForumError>;
    pub fn like(&self, actor: &KirId, message: &KirId) -> Result<(), ForumError>;
    pub fn follow(&self, follower: &KirId, target: &KirId) -> Result<(), ForumError>;
    pub fn share(&self, actor: &KirId, message: &KirId) -> Result<(), ForumError>;
    pub fn read_messages(&self, channel: &KirId) -> Result<Vec<KirEvent>, ForumError>;
}
```

`publish_message` reuses RFC 0052's `try_consume_resource` (made `pub(crate)`, not duplicated) for
the same capacity check `execute_action`'s round-based `PostMessage` path already performs, then
appends the message event plus a `Custom("PostedIn")` relationship (message → channel) — the index
`read_messages` queries. `execute_action`'s own `PostMessage` branch gains the same `PostedIn`
append (one new call, no restructuring), so both paths populate the same index.

`like`/`follow`/`share` all follow `append_knows`'s existing idempotency pattern (RFC 0050): check
for an existing matching relationship first, only append if absent — repeated calls are safe, not
error-prone or duplicate-accumulating.

`read_messages(channel)` = `relationships_for(channel)` filtered to `Custom("PostedIn")` edges
whose `to == channel`, each `from` resolved via `get_event`, sorted by `occurred_at` — a pure read,
no new storage, working for messages posted through either `VirtualForum::publish_message` or a
normal simulation round.

## Alternatives Considered

- **Adding `Like`/`Follow`/`Share`/`Reply` as new `ActionKind` variants** — rejected; would reopen
  RFC 0050's closed-vocabulary decision without a concrete round-based scenario forcing the
  question. `reply` doesn't need a new kind at all (an additive `Action` field suffices); `like`/
  `follow`/`share` are relationship-shaped facts, not verbs an agent "decides" mid-round in this
  RFC's scope.
- **A dedicated `Message` KIR type** (distinct from the existing `Custom("ActionExecuted")` event
  shape) — rejected; the source document's own instruction ("messages become graph events") is
  already exactly what `execute_action`'s `PostMessage` path produces. A parallel type would
  duplicate what already exists for the same purpose.
- **Scanning all events for `read_messages`** — not available; `KnowledgeStore` has no bulk event
  query, confirmed before designing. The `PostedIn` relationship index is the direct consequence of
  that constraint, not a preference.
- **A shared `publish_message_event` helper fully unifying the round-based and direct-API paths**
  — rejected; the two call sites have genuinely different available context (a `Decision` with
  `reasoning_summary`/`confidence` vs. a bare direct call), so their event payloads differ
  legitimately. Only the capacity check and the new `PostedIn` index append are shared — forcing
  full unification would blur two different call shapes for a cosmetic reduction in line count.

## Testing

- `simulation` unit tests (`forum.rs`): `create_channel` round-trips; `publish_message` respects
  capacity (reusing the same `try_consume_resource` behavior RFC 0052 already tests) and appends a
  `PostedIn` index entry; `like`/`follow`/`share` are idempotent (calling twice leaves exactly one
  relationship); `read_messages` returns messages in `occurred_at` order and only for the queried
  channel.
- `simulation` integration test: the source document's own worked loop (§16) — Alice
  `publish_message`s to a channel; Bob, who already `Knows` about the channel and gains a `Knows`
  edge to Alice's message via RFC 0050's public-action fanout, runs a `DecisionEngine` that replies
  (`PostMessage` with `reply_to` set to Alice's message) once it observes something it hasn't
  replied to yet; asserted end-to-end across two simulation rounds, with `read_messages` showing
  both the original and the reply, and the reply's event payload correctly carrying `reply_to`.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `Action.reply_to` implemented additively; `ActionKind` unchanged (still exactly 12 variants,
      no escape hatch) — confirmed by diff: zero lines changed in `ActionKind`'s own definition.
- [x] `VirtualForum` implements all seven source-document capabilities (`create_channel`,
      `publish_message`, `reply` via `reply_to`, `like`, `follow`, `share`, `read_messages`), none
      requiring a new `KnowledgeStore` trait method.
- [x] `read_messages` returns messages posted via either the direct `VirtualForum` API or a normal
      `Simulation::run_round`, verified by `alice_posts_bob_observes_and_replies_across_two_rounds`
      (`forum_fixture.rs`), which exercises both paths against the same channel in one test.
- [x] The source document's own "Alice posts → Bob observes → Bob decides → Bob replies" loop
      works end-to-end through the normal round-based Decision/Action/Simulation Engine, not a
      special-cased path — same test, driven entirely by `agent_observation`'s existing `Knows`
      fanout (RFC 0049/0050), no forum-specific observation logic.
- [x] `like`/`follow`/`share` are idempotent — `like_follow_share_are_idempotent`.
- [x] No new round-based action kinds, no real-platform integration, no thread-reconstruction
      helper — confirmed out of scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0053-virtual-social-environment.md` | This RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/src/action.rs` | `Action.reply_to` + `.with_reply_to()` builder |
| `ekos/crates/simulation/src/simulation.rs` | `try_consume_resource`/`ConsumeResult` made `pub(crate)`; `execute_action`'s `PostMessage` branch carries `reply_to` in its payload and appends the `PostedIn` index |
| `ekos/crates/simulation/src/forum.rs` | New: `VirtualForum`, `ForumError`, `index_message_in_channel`; 5 unit tests |
| `ekos/crates/simulation/src/lib.rs` | `pub mod forum;` + re-exports |
| `ekos/crates/simulation/tests/forum_fixture.rs` | New: the Alice/Bob post-observe-decide-reply loop across two rounds, both API paths for `read_messages` |
