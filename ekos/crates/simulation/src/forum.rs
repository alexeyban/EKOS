//! `VirtualForum` (RFC 0053, source document §16) — a thin, *direct*
//! (non-round) API over `&dyn KnowledgeStore` for the social-interaction
//! capabilities the closed `ActionKind` vocabulary (RFC 0050) deliberately
//! doesn't cover: `create_channel`, `publish_message`, `like`, `follow`,
//! `share`, `read_messages`. Replying is not a separate capability here — it
//! goes through the normal round-based `PostMessage` action with
//! `Action.reply_to` set (see `action.rs`), the same event shape either way.

use crate::simulation::{ConsumeResult, SimulationError, try_consume_resource};
use chrono::Utc;
use ekos_kir::{
    EventKind, KirEvent, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
};
use ekos_ledger::{KnowledgeStore, LedgerError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ForumError {
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error("{0} is not a Channel object")]
    NotAChannel(KirId),
    #[error("channel {channel}'s capacity is exhausted (had {available})")]
    ChannelCapacityExhausted { channel: KirId, available: f64 },
}

/// Appends a `Custom("PostedIn")` relationship from a message event to its
/// channel — the index [`VirtualForum::read_messages`] queries, since
/// `KnowledgeStore` has no bulk "every event" scan (confirmed before
/// designing this RFC). Shared by both `VirtualForum::publish_message` and
/// `simulation.rs::execute_action`'s round-based `PostMessage` branch, so
/// `read_messages` sees a channel's messages regardless of which path
/// posted them.
pub(crate) fn index_message_in_channel(
    store: &dyn KnowledgeStore,
    message_id: &KirId,
    channel_id: &KirId,
) -> Result<(), SimulationError> {
    let rel = KirRelationship::new(
        RelationshipKind::Custom("PostedIn".to_string()),
        *message_id,
        *channel_id,
    );
    store.append_relationship(&rel)?;
    Ok(())
}

/// Appends a `Custom(kind_name)` relationship from `from` to `to`, unless one
/// already exists — the same idempotency pattern `simulation.rs::
/// append_knows` already uses, so `like`/`follow`/`share` are safe to call
/// more than once for the same pair.
fn append_relationship_once(
    store: &dyn KnowledgeStore,
    kind_name: &str,
    from: &KirId,
    to: &KirId,
) -> Result<(), ForumError> {
    let exists = store.relationships_for(from)?.into_iter().any(|r| {
        r.from == *from
            && r.to == *to
            && matches!(&r.kind, RelationshipKind::Custom(k) if k == kind_name)
    });
    if exists {
        return Ok(());
    }
    let rel = KirRelationship::new(RelationshipKind::Custom(kind_name.to_string()), *from, *to);
    store.append_relationship(&rel)?;
    Ok(())
}

pub struct VirtualForum<'a> {
    store: &'a dyn KnowledgeStore,
}

impl<'a> VirtualForum<'a> {
    pub fn new(store: &'a dyn KnowledgeStore) -> Self {
        Self { store }
    }

    /// A `Custom("Channel")` object (RFC 0048's existing convention) with an
    /// optional `resources.capacity` — a named helper over what was already
    /// possible, not new storage.
    pub fn create_channel(
        &self,
        name: impl Into<String>,
        capacity: Option<f64>,
    ) -> Result<KirObject, ForumError> {
        let mut channel = KirObject::new(name, ObjectKind::Custom("Channel".to_string()));
        if let Some(capacity) = capacity {
            channel.properties.insert(
                "resources".to_string(),
                serde_json::json!({ "capacity": capacity }),
            );
        }
        self.store.append_object(&channel)?;
        Ok(channel)
    }

    /// Posts a message to `channel`, outside the round-based Decision/Action
    /// loop — for scenario setup/seeding, not for something a
    /// `DecisionEngine` chooses mid-round. Same effect shape as
    /// `execute_action`'s round-based `PostMessage` (capacity check,
    /// `Custom("ActionExecuted")` event, `PostedIn` index), so
    /// `read_messages` can't tell the two apart.
    pub fn publish_message(
        &self,
        actor: &KirId,
        channel: &KirId,
        content: impl Into<String>,
        reply_to: Option<KirId>,
    ) -> Result<KirEvent, ForumError> {
        let channel_obj = self
            .store
            .get_object(channel)?
            .ok_or(ForumError::NotAChannel(*channel))?;
        if channel_obj.kind != ObjectKind::Custom("Channel".to_string()) {
            return Err(ForumError::NotAChannel(*channel));
        }

        if let ConsumeResult::Insufficient { available } =
            try_consume_resource(self.store, channel, "capacity", 1.0)?
        {
            return Err(ForumError::ChannelCapacityExhausted {
                channel: *channel,
                available,
            });
        }

        let payload = serde_json::json!({
            "actor": actor.to_string(),
            "kind": "PostMessage",
            "target": channel.to_string(),
            "content": content.into(),
            "reply_to": reply_to.map(|t| t.to_string()),
        });
        let event = KirEvent {
            id: KirId::new(),
            kind: EventKind::Custom("ActionExecuted".to_string()),
            subject: *actor,
            payload,
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        };
        self.store.append_event(&event)?;
        index_message_in_channel(self.store, &event.id, channel)?;
        Ok(event)
    }

    /// An agent's reaction to a message — a confirmed behavioral fact (she
    /// did like it), not a claim, the same posture RFC 0049 gave `Knows`.
    pub fn like(&self, actor: &KirId, message: &KirId) -> Result<(), ForumError> {
        append_relationship_once(self.store, "Likes", actor, message)
    }

    /// `target` may be another agent or a channel — `KirRelationship` is
    /// already generic over what its endpoints mean, so no type restriction
    /// is needed here.
    pub fn follow(&self, follower: &KirId, target: &KirId) -> Result<(), ForumError> {
        append_relationship_once(self.store, "Follows", follower, target)
    }

    pub fn share(&self, actor: &KirId, message: &KirId) -> Result<(), ForumError> {
        append_relationship_once(self.store, "Shares", actor, message)
    }

    /// Every message posted to `channel`, oldest first, regardless of
    /// whether it was posted via this API or a normal simulation round —
    /// both paths append the same `PostedIn` index entry.
    pub fn read_messages(&self, channel: &KirId) -> Result<Vec<KirEvent>, ForumError> {
        let mut messages = Vec::new();
        for rel in self.store.relationships_for(channel)? {
            if rel.to == *channel
                && matches!(&rel.kind, RelationshipKind::Custom(k) if k == "PostedIn")
                && let Some(event) = self.store.get_event(&rel.from)?
            {
                messages.push(event);
            }
        }
        messages.sort_by_key(|e| e.occurred_at);
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_ledger::Ledger;
    use tempfile::tempdir;

    #[test]
    fn create_channel_round_trips_with_capacity() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let forum = VirtualForum::new(&ledger);

        let channel = forum.create_channel("general", Some(5.0)).unwrap();
        let reloaded = ledger.get_object(&channel.id).unwrap().unwrap();
        assert_eq!(
            reloaded.properties["resources"]["capacity"],
            serde_json::json!(5.0)
        );
    }

    #[test]
    fn publish_message_respects_capacity_and_indexes_the_message() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let forum = VirtualForum::new(&ledger);
        let actor = KirId::new();

        let channel = forum.create_channel("general", Some(1.0)).unwrap();
        let first = forum
            .publish_message(&actor, &channel.id, "hello", None)
            .unwrap();

        let err = forum
            .publish_message(&actor, &channel.id, "again", None)
            .unwrap_err();
        assert!(matches!(err, ForumError::ChannelCapacityExhausted { .. }));

        let messages = forum.read_messages(&channel.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, first.id);
    }

    #[test]
    fn publish_message_rejects_a_non_channel_target() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let forum = VirtualForum::new(&ledger);
        let actor = KirId::new();
        let not_a_channel =
            KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
        ledger.append_object(&not_a_channel).unwrap();

        let err = forum
            .publish_message(&actor, &not_a_channel.id, "hello", None)
            .unwrap_err();
        assert!(matches!(err, ForumError::NotAChannel(_)));
    }

    #[test]
    fn like_follow_share_are_idempotent() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let forum = VirtualForum::new(&ledger);
        let alice = KirId::new();
        let bob = KirId::new();
        let message = KirId::new();

        forum.like(&alice, &message).unwrap();
        forum.like(&alice, &message).unwrap();
        forum.follow(&alice, &bob).unwrap();
        forum.follow(&alice, &bob).unwrap();
        forum.share(&alice, &message).unwrap();
        forum.share(&alice, &message).unwrap();

        let rels = ledger.relationships_for(&alice).unwrap();
        assert_eq!(
            rels.iter()
                .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "Likes"))
                .count(),
            1
        );
        assert_eq!(
            rels.iter()
                .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "Follows"))
                .count(),
            1
        );
        assert_eq!(
            rels.iter()
                .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "Shares"))
                .count(),
            1
        );
    }

    #[test]
    fn read_messages_is_ordered_and_scoped_to_the_queried_channel() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let forum = VirtualForum::new(&ledger);
        let actor = KirId::new();

        let general = forum.create_channel("general", None).unwrap();
        let random = forum.create_channel("random", None).unwrap();

        let m1 = forum
            .publish_message(&actor, &general.id, "first", None)
            .unwrap();
        let m2 = forum
            .publish_message(&actor, &general.id, "second", Some(m1.id))
            .unwrap();
        forum
            .publish_message(&actor, &random.id, "unrelated", None)
            .unwrap();

        let messages = forum.read_messages(&general.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, m1.id);
        assert_eq!(messages[1].id, m2.id);
        assert_eq!(
            messages[1].payload["reply_to"],
            serde_json::json!(m1.id.to_string())
        );
    }
}
