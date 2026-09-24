//! Migration lifecycle (RFC 0154 / RFC 0161). **Human-only**: nothing in `commands/mcp.rs` may
//! reference this module — a source-scanning test in the CLI enforces it, exactly as RFC 0151's
//! `session::lifecycle` is enforced.
//!
//! Nothing is ever deleted. A retraction re-appends the object with `status: "superseded"` and a
//! pointer to its replacement, and appends a `MigrationStatusChanged` event. The ledger is
//! append-only, so the wrong decision and its correction are both part of the record — which is
//! the honest outcome anyway.
//!
//! RFC 0161 adds approval and sign-off here. Phase 0 ships the part the state machine needs: the
//! actor type, deliberate abandonment, and supersede.

use crate::Error;
use crate::kinds;
use crate::project::{transition, write_context};
use crate::state::UnitState;
use ekos_kir::{KirEvent, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use serde_json::json;

/// The only actor that may take a lifecycle decision. There is deliberately no `Agent` variant:
/// an absent variant cannot be constructed by a future caller who has not read this RFC, which is
/// a stronger guarantee than a runtime check. Same reasoning as `ekos_session::lifecycle::Actor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Human,
}

impl Actor {
    /// The identity recorded on the fact — an OIDC subject for a console action, or the local
    /// user for a CLI action.
    pub fn label(self, subject: &str) -> String {
        match self {
            Self::Human => format!("human:{subject}"),
        }
    }
}

/// Deliberately remove a unit from scope. A decision, not a failure: it is recorded with a reason
/// and the unit stops blocking the completeness check as *unclassified* — it is classified as
/// out of scope, which is a disposition a human made.
pub fn abandon(
    store: &dyn KnowledgeStore,
    unit_id: &KirId,
    actor: Actor,
    subject: &str,
    reason: &str,
    run_id: &str,
) -> Result<Option<KirId>, Error> {
    if reason.trim().is_empty() {
        return Err(Error::ReasonRequired);
    }
    transition(
        store,
        unit_id,
        UnitState::Abandoned,
        &actor.label(subject),
        reason,
        run_id,
    )
}

/// `new` supersedes `old`. `old` is re-appended as superseded, a `Supersedes` edge is recorded,
/// and nothing is deleted.
pub fn supersede(
    store: &dyn KnowledgeStore,
    old_id: &KirId,
    new_id: &KirId,
    actor: Actor,
    subject: &str,
    reason: &str,
    run_id: &str,
) -> Result<(), Error> {
    store.set_write_context(Some(write_context("supersede", run_id)));
    let result = supersede_inner(store, old_id, new_id, actor, subject, reason);
    store.set_write_context(None);
    result
}

fn supersede_inner(
    store: &dyn KnowledgeStore,
    old_id: &KirId,
    new_id: &KirId,
    actor: Actor,
    subject: &str,
    reason: &str,
) -> Result<(), Error> {
    let mut old: KirObject = store
        .get_object(old_id)?
        .ok_or_else(|| Error::NotFound(old_id.to_string()))?;
    let new = store
        .get_object(new_id)?
        .ok_or_else(|| Error::NotFound(new_id.to_string()))?;
    if !is_migration_object(&old) || !is_migration_object(&new) {
        return Err(Error::NotAMigrationObject(old_id.to_string()));
    }

    let now = chrono::Utc::now();
    let from = old
        .properties
        .get("status")
        .cloned()
        .unwrap_or(json!("active"));
    old.properties.insert("status".into(), json!("superseded"));
    old.properties
        .insert("superseded_by".into(), json!(new_id.to_string()));
    old.properties
        .insert("superseded_at".into(), json!(now.to_rfc3339()));
    store.append_object(&old)?;

    store.append_event(&KirEvent {
        id: KirId::new(),
        kind: ekos_kir::EventKind::Custom(kinds::STATUS_CHANGED_EVENT.into()),
        subject: old.id,
        payload: json!({
            "from": from,
            "to": "superseded",
            "actor": actor.label(subject),
            "reason": reason,
            "superseded_by": new_id.to_string(),
        }),
        evidence: Vec::new(),
        occurred_at: now,
    })?;

    store.append_relationship(&KirRelationship::deterministic(
        RelationshipKind::Custom(kinds::SUPERSEDES.into()),
        *new_id,
        *old_id,
        "",
    ))?;
    Ok(())
}

fn is_migration_object(o: &KirObject) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if kinds::ALL_KINDS.contains(&k.as_str()))
}
