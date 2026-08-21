# Devlog 53 — RFC 0053: Virtual Social Environment, without reopening a closed design decision

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Seventh RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
RFC 0052 (Conflict Resolution), and now RFC 0053 — Phase 11 of `EKOS_World_Engine_Development_
Plan.md`, the user's explicit next step ("go next phase 11"): a `VirtualForum` giving scenarios
channels, replies, likes, follows, and shares. The central design question was whether the source
document's seven capabilities require reopening RFC 0050's closed, escape-hatch-free `ActionKind`
vocabulary. They don't: a reply is a message with a parent pointer (an additive field, not a new
kind), and `like`/`follow`/`share` are relationship-shaped social facts, not verbs an agent
"decides" mid-round in this RFC's scope. `VirtualForum` ships as a direct API layered on
`&dyn KnowledgeStore`, built almost entirely from conventions the last three RFCs already
established — the only genuinely new mechanism is a `PostedIn` relationship index, needed because
`KnowledgeStore` (checked before designing, not assumed) has no bulk "every event" query.

---

## RFC 0053 — Virtual Social Environment

### Problem / motivation

Checked before designing, same discipline as the prior six RFCs:

- `ObjectKind::Custom("Channel")` (RFC 0048) and `ActionKind::PostMessage` with capacity
  consumption (RFC 0052) already implement two of the source document's seven capabilities
  (`create_channel`, `publish_message`) almost completely — re-confirmed by re-reading
  `execute_action`'s existing branch before assuming anything needed building from scratch.
- `ActionKind` was closed at 12 variants by explicit design in RFC 0050 ("a scope decision, not a
  taxonomy"). Before treating Phase 11's remaining five capabilities as a reason to reopen that,
  each was checked structurally: `reply` is just "a message with a parent" (an additive `Action`
  field suffices); `like`/`follow`/`share` are facts about existing entities (relationship-shaped),
  not something that needs its own verb in the vocabulary. None forced the question.
- `KnowledgeStore`'s trait surface (re-read directly, not assumed): `all_objects`/
  `all_relationships` exist; there is no `all_events`, only `get_event(id)` by known id. Any
  `read_messages(channel)` implementation therefore needs an index, not a scan — resolved by
  reusing an already-valid pattern (a relationship can point *from* an event, confirmed by how RFC
  0049's `Knows` edges already point *at* events) rather than adding a new query method.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0053-virtual-social-environment.md` |
| `Action.reply_to: Option<KirId>` + `.with_reply_to()` | `crates/simulation/src/action.rs` |
| `VirtualForum` (`create_channel`/`publish_message`/`like`/`follow`/`share`/`read_messages`) | `crates/simulation/src/forum.rs` |
| `PostedIn` index, shared by round-based and direct-API posting | `simulation.rs` + `forum.rs` |

**Deliberately not built**, per the RFC's own Non-goals: round-based `Like`/`Follow`/`Share`
actions (`ActionKind` stays closed at 12, unmodified — reaffirmed, not reopened); any real
platform integration (the source document's own instruction: "do not integrate X or Reddit
initially"); a nested-thread-reconstruction helper (`reply_to` is a flat pointer; walking a full
thread is left to the caller); any new `KnowledgeStore` trait method; moderation/rate-limiting.

### Why `ActionKind` didn't need to grow

This was the real design fork. The source document lists `create_channel`, `publish_message`,
`reply`, `like`, `share`, `follow`, `read_messages` as one flat capability list, which reads
naturally as "add five more actions." Structurally checking each one against what it actually *is*
changed that:

- `reply` — not a new verb, a modifier on an existing one. `PostMessage` already produces a
  message event; a reply is the same event with a parent pointer. `Action.reply_to` (additive
  field, `#[serde(default)]` so nothing existing breaks) carries this without touching
  `ActionKind` at all.
- `like`/`follow`/`share` — none of these describe "what an agent does in a round" so much as
  "a fact about how one entity relates to another," the same shape `Knows`/`Trusts`/`Believes`
  already are. Building them as `Custom()` relationship kinds (the open escape hatch, unlike
  `ActionKind`) costs nothing and needed zero enum changes.

The one real trade-off, named honestly in Non-goals rather than glossed over: an agent's
`DecisionEngine` cannot currently *choose* to like or follow something mid-round — only a direct
`VirtualForum` call or scenario-setup code can. If a future scenario genuinely needs round-based
liking/following, reopening `ActionKind`'s closed vocabulary becomes its own deliberate scope
decision at that point, not something this RFC backed into by default.

### The index that had to exist for `read_messages` to work at all

`KnowledgeStore` was designed (RFC 0005/0016) around lookups by known id, not bulk scans —
`all_objects`/`all_relationships` are the only two "give me everything" queries, and neither
covers events. This meant `read_messages(channel)` had no way to enumerate "every message ever
posted here" without a purpose-built index. The fix reuses machinery already proven to work: RFC
0049's `Knows` relationships already point *from* an agent *at* an event id, so a relationship's
endpoint being an event (not just an object) was already a validated pattern, not a new one. Every
published message gets a `Custom("PostedIn")` relationship from the message event to its channel;
`read_messages` is just `relationships_for(channel)`, filtered and dereferenced. Both
`VirtualForum::publish_message` (direct API) and `execute_action`'s round-based `PostMessage`
branch append the same index entry — the one piece of logic genuinely shared between the two call
sites, verified by a single test that exercises both paths against one channel and checks all
three resulting messages are visible and correctly ordered.

### The worked loop, proven through the real engine, not a shortcut

The source document's own example (§16: Alice posts → message event → Bob observes → Bob decides →
Bob replies) is exactly what `forum_fixture.rs`'s integration test runs, using nothing forum-
specific for the "observes" step: `PostMessage` is already `is_public()` (RFC 0050), so
`execute_action`'s existing `Knows`-fanout automatically gives every configured agent — Bob
included — a `Knows` edge to Alice's message the moment it executes. Round 1's `agent_observation`
(RFC 0049) then naturally surfaces that event in Bob's own observation, with zero new plumbing.
Bob's test-local `DecisionEngine` scans `ctx.observation.events` for anything from someone else it
hasn't replied to yet, and replies via ordinary `PostMessage` with `reply_to` set — proving the
loop runs through the real Decision/Action/Simulation Engine end to end, not a special-cased forum
path that only looks like it does.

### Decisions (alternatives considered, why this choice)

- **Adding `Like`/`Follow`/`Share`/`Reply` as new `ActionKind` variants** — rejected; would reopen
  RFC 0050's closed-vocabulary decision without a concrete round-based scenario forcing the
  question, and none of the four actually needs it structurally.
- **A dedicated `Message` KIR type** distinct from the existing `Custom("ActionExecuted")` event
  shape — rejected; the source document's own instruction ("messages become graph events") is
  already exactly what `PostMessage` already produces; a parallel type would duplicate it.
- **Scanning all events for `read_messages`** — not available; confirmed by re-reading
  `KnowledgeStore` before designing, not assumed. The `PostedIn` index is the direct consequence
  of that constraint.
- **Fully unifying the round-based and direct-API posting paths into one shared function** —
  rejected; the two call sites have genuinely different available context (a `Decision` with
  `reasoning_summary`/`confidence` vs. a bare direct call), so their event payloads legitimately
  differ. Only the capacity check (`try_consume_resource`, already shared) and the new `PostedIn`
  append are actually common — forcing full unification would blur two different call shapes for a
  cosmetic reduction in line count.

---

## Knowledge Captured

- **Before treating a source document's flat capability list as "add N new actions," check each
  capability structurally against what it actually represents.** `reply`/`like`/`follow`/`share`
  read like verbs but are mostly facts-about-relationships or modifiers-on-existing-actions, not
  genuinely new things an agent chooses to do. This is the same discipline RFC 0052 applied when it
  found the source document's own conflict-resolution example didn't structurally produce a
  collision — checking what something *is*, not what it's *named*, before building around it.
- **A relationship's endpoint being an event, not just an object, is a pattern worth re-establishing
  explicitly whenever a new query need comes up** — RFC 0049 proved it once (`Knows` → event); this
  RFC reused the exact same fact to build a message index, without needing new `KnowledgeStore`
  surface. Worth remembering as a general technique: an index over "things that point at X" is
  often cheaper than a new bulk-query method.
- **Two call sites sharing *some* logic doesn't mean they should share *all* of it.** The
  round-based and direct-API posting paths only really needed to share the capacity check and the
  index append; forcing a single unified function for both would have required threading
  round-specific fields (`reasoning_summary`, `confidence`, `round`) through a call site that has
  no meaningful values for them.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0053-virtual-social-environment.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/src/action.rs` | `Action.reply_to` + `.with_reply_to()` builder |
| `ekos/crates/simulation/src/simulation.rs` | `try_consume_resource`/`ConsumeResult` made `pub(crate)`; `PostMessage` branch carries `reply_to`, appends `PostedIn` |
| `ekos/crates/simulation/src/forum.rs` | New: `VirtualForum`, `ForumError`, `index_message_in_channel`; 5 unit tests |
| `ekos/crates/simulation/src/lib.rs` | `pub mod forum;` + re-exports |
| `ekos/crates/simulation/tests/forum_fixture.rs` | New: Alice/Bob post-observe-decide-reply loop, both `read_messages` API paths |

## Still open (tracked, not silently dropped)

- **Whether to continue further** — `world.sources` document ingestion is the one remaining named
  fork from the last several devlogs; Phase 12+ (Event Store as a distinct concept, Replay,
  Metrics, Turning Point Detection, and beyond) haven't been scoped at all yet.
- **No round-based `Like`/`Follow`/`Share`** — real, deferred; would mean deliberately reopening
  `ActionKind`'s closed vocabulary, not something to back into incidentally.
- **No thread-reconstruction helper** — `reply_to` is a flat pointer; a caller wanting a full
  thread walks it themselves.
- **No scenario-YAML support for seeding channels/messages directly** (RFC 0051's `events:` section
  covers generic seed events, not forum-specific ones) — real, deferred if a scenario needs it.
