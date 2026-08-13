//! RFC 0053's own worked loop (source document §16): "Alice posts → Message
//! event → Bob observes → Bob decides → Bob replies," through the *normal*
//! round-based Decision/Action/Simulation Engine (RFC 0050), not a
//! special-cased forum path — `reply` is just `PostMessage` with
//! `Action.reply_to` set. Also proves `VirtualForum::read_messages` sees
//! messages from both the round-based path and the direct API against the
//! same channel.

use ekos_kir::KirId;
use ekos_ledger::Ledger;
use ekos_simulation::{
    Action, ActionKind, Decision, DecisionContext, DecisionEngine, Simulation, SimulationConfig,
    VirtualForum,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use tempfile::tempdir;

/// Posts once (round 0), then does nothing — a `Cell` flag stands in for
/// "already said my piece," since `DecisionEngine::decide` takes `&self`,
/// not `&mut self`.
struct PostOnceEngine {
    channel: KirId,
    already_posted: Cell<bool>,
}

impl DecisionEngine for PostOnceEngine {
    fn decide(&self, _ctx: &DecisionContext) -> Decision {
        if self.already_posted.get() {
            return Decision {
                action: Action::new(ActionKind::DoNothing),
                reasoning_summary: "already posted".to_string(),
                confidence: 0.5,
            };
        }
        self.already_posted.set(true);
        Decision {
            action: Action::new(ActionKind::PostMessage)
                .with_target(self.channel)
                .with_content("Alice's original post"),
            reasoning_summary: "posting the original message".to_string(),
            confidence: 0.9,
        }
    }
}

/// Replies to the first message it observes from someone else that it
/// hasn't already replied to — the "Bob decides → Bob replies" half of the
/// source document's loop, driven entirely by what `agent_observation`
/// (RFC 0049) puts in `ctx.observation.events`.
struct ReplyOnceEngine {
    channel: KirId,
    replied_to: RefCell<HashSet<KirId>>,
}

impl DecisionEngine for ReplyOnceEngine {
    fn decide(&self, ctx: &DecisionContext) -> Decision {
        let mut replied = self.replied_to.borrow_mut();
        for event in &ctx.observation.events {
            if event.subject != ctx.agent_id && !replied.contains(&event.id) {
                replied.insert(event.id);
                return Decision {
                    action: Action::new(ActionKind::PostMessage)
                        .with_target(self.channel)
                        .with_content("Bob's reply")
                        .with_reply_to(event.id),
                    reasoning_summary: format!("replying to message {}", event.id),
                    confidence: 0.8,
                };
            }
        }
        Decision {
            action: Action::new(ActionKind::DoNothing),
            reasoning_summary: "nothing new to reply to".to_string(),
            confidence: 0.5,
        }
    }
}

#[test]
fn alice_posts_bob_observes_and_replies_across_two_rounds() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let forum = VirtualForum::new(&ledger);

    let channel = forum.create_channel("general", Some(10.0)).unwrap();

    let alice = KirId::new();
    let bob = KirId::new();
    // Both agents need to already Know about the channel for PostMessage's
    // structural target-observed check (validate_action) to pass — separate
    // from whether they Know about any particular message inside it.
    for agent in [alice, bob] {
        ledger
            .append_relationship(&ekos_kir::KirRelationship::new(
                ekos_kir::RelationshipKind::Custom("Knows".to_string()),
                agent,
                channel.id,
            ))
            .unwrap();
    }

    let mut engines: HashMap<KirId, Box<dyn DecisionEngine>> = HashMap::new();
    engines.insert(
        alice,
        Box::new(PostOnceEngine {
            channel: channel.id,
            already_posted: Cell::new(false),
        }),
    );
    engines.insert(
        bob,
        Box::new(ReplyOnceEngine {
            channel: channel.id,
            replied_to: RefCell::new(HashSet::new()),
        }),
    );

    let config = SimulationConfig {
        agents: vec![alice, bob],
        available_actions: vec![ActionKind::PostMessage, ActionKind::DoNothing],
        seed: None,
    };
    let sim = Simulation::new(&ledger, config, engines);

    let round0 = sim.run_round(0).unwrap();
    let alice_decision_0 = &round0
        .decisions
        .iter()
        .find(|(id, _)| *id == alice)
        .unwrap()
        .1;
    assert_eq!(alice_decision_0.action.kind, ActionKind::PostMessage);
    let bob_decision_0 = &round0
        .decisions
        .iter()
        .find(|(id, _)| *id == bob)
        .unwrap()
        .1;
    assert_eq!(
        bob_decision_0.action.kind,
        ActionKind::DoNothing,
        "Bob has nothing to reply to yet in round 0 — Alice's post hasn't executed when he decides"
    );

    let round1 = sim.run_round(1).unwrap();
    let bob_decision_1 = &round1
        .decisions
        .iter()
        .find(|(id, _)| *id == bob)
        .unwrap()
        .1;
    assert_eq!(
        bob_decision_1.action.kind,
        ActionKind::PostMessage,
        "by round 1, Bob has Knows-fanout access to Alice's round-0 post and should reply"
    );
    assert!(bob_decision_1.action.reply_to.is_some());

    let messages = forum.read_messages(&channel.id).unwrap();
    assert_eq!(messages.len(), 2, "Alice's original + Bob's reply");
    assert_eq!(
        messages[0].payload["actor"],
        serde_json::json!(alice.to_string())
    );
    assert_eq!(
        messages[1].payload["actor"],
        serde_json::json!(bob.to_string())
    );
    assert_eq!(
        messages[1].payload["reply_to"],
        serde_json::json!(messages[0].id.to_string()),
        "Bob's reply must point back at Alice's original message"
    );

    // Both paths populate the same index: seed one more message directly
    // through VirtualForum (not through a simulation round) and confirm
    // read_messages sees all three, correctly ordered.
    let direct_post = forum
        .publish_message(&alice, &channel.id, "seeded via the direct API", None)
        .unwrap();
    let messages_after_direct_post = forum.read_messages(&channel.id).unwrap();
    assert_eq!(messages_after_direct_post.len(), 3);
    assert_eq!(messages_after_direct_post[2].id, direct_post.id);
}
