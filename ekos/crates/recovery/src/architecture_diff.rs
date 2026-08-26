//! Architecture Diff (RFC 0068 §55, RFC 0108) — a real, semantically-meaningful diff between two
//! points in time, distinct from `ekos diff`'s raw ledger-entry-id report (`Added: N`, bare
//! `entry #N` lines). A sibling of [`crate::architecture_drift`] (which compares one role
//! `Claim`'s oldest-vs-newest whole history) — this compares *every* architecturally-meaningful
//! KIR kind between two specific timestamps.
//!
//! Every kind covered here mints a **deterministic** `KirId` for the real-world thing it
//! represents (confirmed by reading each analyzer directly, not assumed):
//! `dependency_analyzer.rs`/`elixir_analyzer.rs`/`package_json_analyzer.rs`'s own
//! `technology_kir_id` (keyed by name), `architecture_reasoning::role_claim_kir_id` (keyed by
//! crate manifest dir), `crate_topology_analyzer::architecture_gap_kir_id` (keyed by crate dir +
//! dependency name), and `ekos-semantic`'s `concentration_risk_kir_id` (keyed by the target
//! object). Deterministic ids mean "the same real-world thing" reliably keeps the same `KirId`
//! across snapshots, so this diff is a plain id-set comparison per kind, not fuzzy name matching.

use ekos_kir::{KirObject, ObjectKind};
use std::collections::{HashMap, HashSet};

/// One role `Claim`'s value changing between the two snapshots — present in both, but with a
/// different `properties["value"]`. A claim present only in `after` is a *new* claim, not a role
/// change, and is intentionally not reported here (see [`diff_architecture`]'s own doc comment).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoleChange {
    pub crate_name: String,
    pub from: String,
    pub to: String,
}

/// RFC 0068 §55's real architecture-level diff. Every field is a plain, honest list — an empty
/// list means "none," never omitted, matching this crate's own established "honest empty state"
/// convention.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ArchitectureDiff {
    pub technologies_added: Vec<String>,
    pub technologies_removed: Vec<String>,
    pub role_changes: Vec<RoleChange>,
    pub risks_added: Vec<String>,
    pub risks_resolved: Vec<String>,
    pub gaps_added: Vec<String>,
    pub gaps_resolved: Vec<String>,
}

impl ArchitectureDiff {
    /// `true` when every field is empty — the honest "nothing changed" case, checked once here
    /// rather than at every call site that needs to know.
    pub fn is_empty(&self) -> bool {
        self.technologies_added.is_empty()
            && self.technologies_removed.is_empty()
            && self.role_changes.is_empty()
            && self.risks_added.is_empty()
            && self.risks_resolved.is_empty()
            && self.gaps_added.is_empty()
            && self.gaps_resolved.is_empty()
    }
}

fn is_kind(obj: &KirObject, kind: &str) -> bool {
    matches!(&obj.kind, ObjectKind::Custom(s) if s == kind)
}

fn by_id<'a>(objects: &'a [KirObject], kind: &str) -> HashMap<ekos_kir::KirId, &'a KirObject> {
    objects
        .iter()
        .filter(|o| is_kind(o, kind))
        .map(|o| (o.id, o))
        .collect()
}

fn added_and_removed_names(
    before: &HashMap<ekos_kir::KirId, &KirObject>,
    after: &HashMap<ekos_kir::KirId, &KirObject>,
) -> (Vec<String>, Vec<String>) {
    let before_ids: HashSet<_> = before.keys().copied().collect();
    let after_ids: HashSet<_> = after.keys().copied().collect();

    let mut added: Vec<String> = after_ids
        .difference(&before_ids)
        .map(|id| after[id].name.clone())
        .collect();
    added.sort();

    let mut removed: Vec<String> = before_ids
        .difference(&after_ids)
        .map(|id| before[id].name.clone())
        .collect();
    removed.sort();

    (added, removed)
}

/// Diff two object snapshots (e.g. from `KnowledgeStore::all_objects_at` at two different
/// timestamps, RFC 0096) at the architecture level. A `Custom("Claim")` present in both snapshots
/// with a changed `properties["value"]` is a role change; one present only in `after` is a new
/// claim (e.g. a crate compiled for the first time), intentionally reported as neither an
/// addition nor a role change here — there is no "from" role to name for something that didn't
/// exist before, and force-fitting it into `role_changes` would misstate what happened.
pub fn diff_architecture(before: &[KirObject], after: &[KirObject]) -> ArchitectureDiff {
    let tech_before = by_id(before, "Technology");
    let tech_after = by_id(after, "Technology");
    let (technologies_added, technologies_removed) =
        added_and_removed_names(&tech_before, &tech_after);

    let claims_before = by_id(before, "Claim");
    let claims_after = by_id(after, "Claim");
    let mut role_changes: Vec<RoleChange> = Vec::new();
    for (id, before_claim) in &claims_before {
        if before_claim
            .properties
            .get("predicate")
            .and_then(|v| v.as_str())
            != Some("has_role")
        {
            continue;
        }
        let Some(after_claim) = claims_after.get(id) else {
            continue; // claim removed entirely — not a role change, no "to" value to report
        };
        let (Some(from), Some(to)) = (
            before_claim
                .properties
                .get("value")
                .and_then(|v| v.as_str()),
            after_claim.properties.get("value").and_then(|v| v.as_str()),
        ) else {
            continue; // malformed — never fabricated
        };
        if from != to {
            role_changes.push(RoleChange {
                crate_name: before_claim.name.clone(),
                from: from.to_string(),
                to: to.to_string(),
            });
        }
    }
    role_changes.sort_by(|a, b| a.crate_name.cmp(&b.crate_name));

    let risks_before = by_id(before, "Risk");
    let risks_after = by_id(after, "Risk");
    let (risks_added, risks_resolved) = added_and_removed_names(&risks_before, &risks_after);

    let gaps_before = by_id(before, "ArchitectureGap");
    let gaps_after = by_id(after, "ArchitectureGap");
    let (gaps_added, gaps_resolved) = added_and_removed_names(&gaps_before, &gaps_after);

    ArchitectureDiff {
        technologies_added,
        technologies_removed,
        role_changes,
        risks_added,
        risks_resolved,
        gaps_added,
        gaps_resolved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirId;

    fn technology(id: KirId, name: &str) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom("Technology".to_string()));
        o.id = id;
        o
    }

    fn role_claim(id: KirId, crate_name: &str, value: &str) -> KirObject {
        let mut o = KirObject::new(crate_name, ObjectKind::Custom("Claim".to_string()))
            .with_property("predicate", serde_json::json!("has_role"))
            .with_property("value", serde_json::json!(value));
        o.id = id;
        o
    }

    fn risk(id: KirId, statement: &str) -> KirObject {
        let mut o = KirObject::new(statement, ObjectKind::Custom("Risk".to_string()));
        o.id = id;
        o
    }

    fn gap(id: KirId, question: &str) -> KirObject {
        let mut o = KirObject::new(question, ObjectKind::Custom("ArchitectureGap".to_string()));
        o.id = id;
        o
    }

    #[test]
    fn empty_to_empty_reports_nothing() {
        let diff = diff_architecture(&[], &[]);
        assert!(diff.is_empty());
    }

    #[test]
    fn a_new_technology_is_reported_as_added() {
        let id = KirId::new();
        let diff = diff_architecture(&[], &[technology(id, "clap")]);
        assert_eq!(diff.technologies_added, vec!["clap".to_string()]);
        assert!(diff.technologies_removed.is_empty());
    }

    #[test]
    fn a_dropped_technology_is_reported_as_removed() {
        let id = KirId::new();
        let diff = diff_architecture(&[technology(id, "clap")], &[]);
        assert_eq!(diff.technologies_removed, vec!["clap".to_string()]);
        assert!(diff.technologies_added.is_empty());
    }

    #[test]
    fn a_role_change_present_in_both_snapshots_is_reported() {
        let id = KirId::new();
        let before = vec![role_claim(id, "ekos-kir", "shared utility")];
        let after = vec![role_claim(id, "ekos-kir", "core library")];
        let diff = diff_architecture(&before, &after);
        assert_eq!(
            diff.role_changes,
            vec![RoleChange {
                crate_name: "ekos-kir".to_string(),
                from: "shared utility".to_string(),
                to: "core library".to_string(),
            }]
        );
    }

    #[test]
    fn an_unchanged_role_is_not_reported() {
        let id = KirId::new();
        let before = vec![role_claim(id, "ekos-kir", "core library")];
        let after = vec![role_claim(id, "ekos-kir", "core library")];
        assert!(diff_architecture(&before, &after).role_changes.is_empty());
    }

    #[test]
    fn a_claim_new_in_after_is_not_misreported_as_a_role_change() {
        let id = KirId::new();
        let after = vec![role_claim(id, "ekos-cli", "core library")];
        let diff = diff_architecture(&[], &after);
        assert!(
            diff.role_changes.is_empty(),
            "a brand-new claim has no prior role to name — must not appear as a role change"
        );
    }

    #[test]
    fn risks_added_and_resolved_are_detected() {
        let stale = KirId::new();
        let fresh = KirId::new();
        let before = vec![risk(stale, "old risk")];
        let after = vec![risk(fresh, "new risk")];
        let diff = diff_architecture(&before, &after);
        assert_eq!(diff.risks_added, vec!["new risk".to_string()]);
        assert_eq!(diff.risks_resolved, vec!["old risk".to_string()]);
    }

    #[test]
    fn gaps_added_and_resolved_are_detected() {
        let resolved_id = KirId::new();
        let new_id = KirId::new();
        let before = vec![gap(resolved_id, "unresolved dep X")];
        let after = vec![gap(new_id, "unresolved dep Y")];
        let diff = diff_architecture(&before, &after);
        assert_eq!(diff.gaps_added, vec!["unresolved dep Y".to_string()]);
        assert_eq!(diff.gaps_resolved, vec!["unresolved dep X".to_string()]);
    }
}
