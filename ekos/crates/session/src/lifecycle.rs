//! Claim lifecycle (RFC 0151 Phase 4). **Human-only**: nothing in `commands/mcp.rs` may call this
//! module — a source-scanning test in the CLI enforces it. Nothing is ever deleted: a status
//! change re-appends the claim (a new ledger version) and appends a `ClaimStatusChanged` event.

use ekos_kir::{EventKind, KirEvent, KirId, KirObject, KirRelationship, RelationshipKind};
use ekos_ledger::{KnowledgeStore, LedgerError};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("not a session claim: {0}")]
    NotASessionClaim(String),
}

/// The only actor that may change a claim's status. There is deliberately no `Agent` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Confirm,
    Reject,
}

fn load(store: &dyn KnowledgeStore, id: &KirId) -> Result<KirObject, LifecycleError> {
    let o = store
        .get_object(id)?
        .ok_or_else(|| LifecycleError::NotASessionClaim(id.to_string()))?;
    if !matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == ekos_kir::custom_kinds::SESSION_CLAIM_KIND)
    {
        return Err(LifecycleError::NotASessionClaim(id.to_string()));
    }
    Ok(o)
}

fn record(
    store: &dyn KnowledgeStore,
    mut claim: KirObject,
    to_status: &str,
    to_tier: &str,
    actor: Actor,
    extra: serde_json::Value,
) -> Result<(), LifecycleError> {
    let from = claim
        .properties
        .get("review_status")
        .cloned()
        .unwrap_or(json!("unconfirmed"));
    let now = chrono::Utc::now();
    claim
        .properties
        .insert("review_status".into(), json!(to_status));
    claim.properties.insert("tier".into(), json!(to_tier));
    claim.properties.insert(
        "reviewed_by".into(),
        json!(format!("{actor:?}").to_lowercase()),
    );
    claim
        .properties
        .insert("reviewed_at".into(), json!(now.to_rfc3339()));
    store.append_object(&claim)?;
    store.append_event(&KirEvent {
        id: KirId::new(),
        kind: EventKind::Custom("ClaimStatusChanged".into()),
        subject: claim.id,
        payload: json!({ "from": from, "to": to_status, "actor": "human", "extra": extra }),
        evidence: Vec::new(),
        occurred_at: now,
    })?;
    Ok(())
}

/// Confirm (T0 → T1) or reject a claim.
pub fn review(
    store: &dyn KnowledgeStore,
    claim_id: &KirId,
    decision: Decision,
    actor: Actor,
) -> Result<(), LifecycleError> {
    let claim = load(store, claim_id)?;
    let (status, tier) = match decision {
        Decision::Confirm => ("confirmed", "T1"),
        Decision::Reject => ("rejected", "T0"),
    };
    record(store, claim, status, tier, actor, json!({}))
}

/// `new` supersedes `old`: `old` leaves default ranking, a `Supersedes` edge is recorded, nothing
/// is deleted.
pub fn supersede(
    store: &dyn KnowledgeStore,
    old_id: &KirId,
    new_id: &KirId,
    actor: Actor,
) -> Result<(), LifecycleError> {
    let old = load(store, old_id)?;
    load(store, new_id)?;
    record(
        store,
        old,
        "superseded",
        "T0",
        actor,
        json!({ "superseded_by": new_id.to_string() }),
    )?;
    store.append_relationship(&KirRelationship::deterministic(
        RelationshipKind::Custom("Supersedes".into()),
        *new_id,
        *old_id,
        "",
    ))?;
    Ok(())
}
