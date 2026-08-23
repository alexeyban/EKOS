//! Documentation drift (RFC 0068 §31-32) — "a discrepancy between documented architecture and
//! architecture supported by current evidence." Deliberately a pure function over an already-
//! fetched object-version history, not a `KnowledgeStore`-querying one: `recovery` passes have
//! never read the ledger (only ever produced KIR flowing forward through compile→commit), and
//! `evaluate_architecture` (this crate) is intentionally a plain function over an object snapshot.
//! The real primitive this needs — `KnowledgeStore::object_history` — already exists in
//! `ekos-ledger`; the caller (`ekos architecture investigate`, which already has a live store
//! handle) fetches the history and hands it in here, keeping this crate free of a new ledger
//! dependency and this function trivially unit-testable with a hand-built `Vec<KirObject>`.
//!
//! A role `Claim`'s id is deterministic per crate (`architecture_reasoning::role_claim_kir_id`),
//! and `append_object`'s `(id, content_signature)` versioning (RFC 0015) means identical content
//! is deduplicated — a claim's `object_history` only grows a new entry when a later
//! `architecture-reasoning` run genuinely assigned a *different* role. That already-existing
//! behavior is this RFC's entire "documented claim" concept: an older version already sitting in
//! this project's own append-only ledger, not a separately modeled snapshot.

use chrono::{DateTime, Utc};
use ekos_kir::{KirId, KirObject};

/// RFC 0068 §32's drift record, using this project's real types (`KirId`, plain strings for the
/// role value) rather than inventing new ones.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DriftFinding {
    pub subject_name: String,
    pub subject_id: KirId,
    /// The oldest recorded value for this claim — RFC 0068 §32's `documented_claim`.
    pub documented_value: String,
    /// The newest recorded value — RFC 0068 §32's `observed_claim`.
    pub observed_value: String,
    pub detected_at: DateTime<Utc>,
}

/// Compares the oldest and newest version of one role `Claim`'s history and reports a finding if
/// the classified role genuinely changed. `history` must be oldest-to-newest, matching
/// `KnowledgeStore::object_history`'s own documented ordering. `None` for a claim with fewer than
/// two versions (nothing to compare) or missing/malformed `properties["value"]` (never fabricated).
pub fn drift_from_history(
    subject_name: &str,
    subject_id: KirId,
    history: &[KirObject],
) -> Option<DriftFinding> {
    if history.len() < 2 {
        return None;
    }
    let oldest_value = history.first()?.properties.get("value")?.as_str()?;
    let newest_value = history.last()?.properties.get("value")?.as_str()?;
    if oldest_value == newest_value {
        return None;
    }
    Some(DriftFinding {
        subject_name: subject_name.to_string(),
        subject_id,
        documented_value: oldest_value.to_string(),
        observed_value: newest_value.to_string(),
        detected_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;

    fn role_claim_version(value: &str) -> KirObject {
        KirObject::new(
            format!("crate has_role {value}"),
            ObjectKind::Custom("Claim".to_string()),
        )
        .with_property("predicate", serde_json::json!("has_role"))
        .with_property("value", serde_json::json!(value))
    }

    #[test]
    fn two_different_versions_produce_a_finding() {
        let subject_id = KirId::new();
        let history = vec![
            role_claim_version("shared utility"),
            role_claim_version("core library"),
        ];
        let finding = drift_from_history("ekos-kir", subject_id, &history)
            .expect("a real role change must produce a finding");
        assert_eq!(finding.subject_name, "ekos-kir");
        assert_eq!(finding.subject_id, subject_id);
        assert_eq!(finding.documented_value, "shared utility");
        assert_eq!(finding.observed_value, "core library");
    }

    #[test]
    fn identical_repeated_versions_produce_no_finding() {
        // Real behavior this relies on: `append_object`'s (id, content_signature) versioning
        // (RFC 0015) already deduplicates identical content, so this case is what `object_history`
        // returns for a claim re-derived unchanged across several `recover` runs.
        let history = vec![role_claim_version("core library")];
        assert!(drift_from_history("ekos-kir", KirId::new(), &history).is_none());
    }

    #[test]
    fn single_version_produces_no_finding() {
        let history = vec![role_claim_version("core library")];
        assert!(drift_from_history("ekos-kir", KirId::new(), &history).is_none());
    }

    #[test]
    fn empty_history_produces_no_finding() {
        assert!(drift_from_history("ekos-kir", KirId::new(), &[]).is_none());
    }

    #[test]
    fn three_versions_compares_oldest_to_newest_not_adjacent_pairs() {
        let history = vec![
            role_claim_version("core library"),
            role_claim_version("shared utility"),
            role_claim_version("core library"),
        ];
        // Net change across the whole history is none (back to the original) — matches
        // `object_history`'s documented oldest-to-newest ordering, comparing endpoints.
        assert!(drift_from_history("ekos-kir", KirId::new(), &history).is_none());
    }

    #[test]
    fn missing_value_property_produces_no_finding_not_a_panic() {
        let malformed = KirObject::new("x", ObjectKind::Custom("Claim".to_string()));
        let history = vec![malformed.clone(), malformed];
        assert!(drift_from_history("ekos-kir", KirId::new(), &history).is_none());
    }
}
