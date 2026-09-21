//! The deterministic mapping pass (RFC 0151 Phase 2): inbox entries → `Session` + `SessionClaim`
//! objects, evidence, `ObservedIn` / `AnchoredTo` relationships and `DeadEnd` events. Pure: the
//! same entries + the same anchor index always produce byte-identical KIR (ids are UUIDv5,
//! timestamps come from the entries, never the clock).

use crate::anchor::{AnchorIndex, Resolution, anchor_fingerprint, anchor_projection};
use crate::inbox::{NoteKind, SessionEntry};
use ekos_kir::custom_kinds::{SESSION_CLAIM_KIND, SESSION_KIND};
use ekos_kir::{
    EventKind, KirEvent, KirEvidence, KirId, KirObject, KirRelationship, ObjectKind,
    RelationshipKind, SourceLocation,
};
use serde_json::json;
use uuid::Uuid;

pub const TIER_UNCONFIRMED: &str = "T0";
pub const STATUS_UNCONFIRMED: &str = "unconfirmed";

#[derive(Debug, Clone, Default)]
pub struct MappedBatch {
    pub session: Option<KirObject>,
    pub claims: Vec<KirObject>,
    pub evidence: Vec<KirEvidence>,
    pub relationships: Vec<KirRelationship>,
    pub events: Vec<KirEvent>,
}

fn v5(seed: String) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

pub fn session_object_id(session_id: &str) -> KirId {
    v5(format!("session:{session_id}"))
}

pub fn claim_object_id(entry_id: &str) -> KirId {
    v5(format!("session-claim:{entry_id}"))
}

fn evidence_id(entry_id: &str) -> KirId {
    v5(format!("session-evidence:{entry_id}"))
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// `source_file` is the workspace-relative inbox path evidence points back at; `artifact_id` is
/// the content hash of the observed batch.
pub fn map_entries(
    session_id: &str,
    source_file: &str,
    artifact_id: &str,
    first_line: usize,
    entries: &[SessionEntry],
    anchors: &AnchorIndex,
    total_notes: usize,
) -> MappedBatch {
    let mut batch = MappedBatch::default();
    if entries.is_empty() {
        return batch;
    }
    let sid = session_object_id(session_id);
    let first_at = entries
        .iter()
        .map(|e| e.recorded_at)
        .min()
        .unwrap_or_default();
    let mut session = KirObject::new(
        format!("session-{session_id}"),
        ObjectKind::Custom(SESSION_KIND.into()),
    )
    .with_property("session_id", json!(session_id))
    .with_property("note_count", json!(total_notes))
    .with_property("last_artifact", json!(artifact_id));
    session.id = sid;
    session.created_at = first_at;
    batch.session = Some(session);

    for (i, e) in entries.iter().enumerate() {
        let claim_id = claim_object_id(&e.entry_id);
        let ev_id = evidence_id(&e.entry_id);
        let mut ev = KirEvidence::new(
            SourceLocation {
                path: source_file.to_string(),
                line: Some((first_line + i + 1) as u32),
                column: None,
            },
            format!("[artifact {artifact_id}] {}", truncate(&e.text, 200)),
        );
        ev.id = ev_id;
        ev.created_at = e.recorded_at;
        batch.evidence.push(ev);

        let mut anchor_views = Vec::new();
        for hint in &e.anchors {
            let (res, obj) = anchors.resolve(hint);
            let mut view = json!({ "hint": hint, "resolution": &res });
            if let (Resolution::Resolved { object_id }, Some(obj)) = (&res, obj) {
                let fp = anchor_fingerprint(obj);
                view["fingerprint"] = json!(fp);
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::Custom("AnchoredTo".into()),
                    claim_id,
                    obj.id,
                    hint,
                );
                rel.created_at = e.recorded_at;
                rel.properties.insert("anchor_hint".into(), json!(hint));
                rel.properties
                    .insert("anchor_fingerprint".into(), json!(fp));
                rel.properties
                    .insert("anchor_projection".into(), json!(anchor_projection(obj)));
                rel.properties.insert("object_id".into(), json!(object_id));
                rel.evidence.push(ev_id);
                batch.relationships.push(rel);
            }
            anchor_views.push(view);
        }

        let mut claim = KirObject::new(
            format!("session-note {}", truncate(&e.text, 80)),
            ObjectKind::Custom(SESSION_CLAIM_KIND.into()),
        )
        .with_property("claim_type", json!("session_note"))
        .with_property("note_kind", json!(e.kind))
        .with_property("text", json!(e.text))
        .with_property("session_id", json!(session_id))
        .with_property("entry_id", json!(e.entry_id))
        .with_property("capture", json!(e.capture))
        .with_property("tier", json!(TIER_UNCONFIRMED))
        .with_property("review_status", json!(STATUS_UNCONFIRMED))
        .with_property("anchors", json!(anchor_views))
        .with_property("source_file", json!(source_file))
        .with_evidence(ev_id);
        if let Some(r) = &e.rationale {
            claim.properties.insert("rationale".into(), json!(r));
        }
        if let Some(q) = &e.quote {
            claim.properties.insert("quote".into(), json!(q));
        }
        claim.id = claim_id;
        claim.created_at = e.recorded_at;
        batch.claims.push(claim);

        let mut observed = KirRelationship::deterministic(
            RelationshipKind::Custom("ObservedIn".into()),
            claim_id,
            sid,
            "",
        );
        observed.created_at = e.recorded_at;
        observed.evidence.push(ev_id);
        batch.relationships.push(observed);

        if e.kind == NoteKind::DeadEnd {
            batch.events.push(KirEvent {
                id: v5(format!("session-deadend:{}", e.entry_id)),
                kind: EventKind::Custom("DeadEnd".into()),
                subject: claim_id,
                payload: json!({ "session_id": session_id, "text": e.text, "rationale": e.rationale }),
                evidence: vec![ev_id],
                occurred_at: e.recorded_at,
            });
        }
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::SessionEntry;
    use chrono::TimeZone;

    fn entry(id: &str, kind: NoteKind, anchors: &[&str]) -> SessionEntry {
        SessionEntry {
            schema: 1,
            entry_id: id.into(),
            session_id: "s1".into(),
            recorded_at: chrono::Utc.with_ymd_and_hms(2026, 9, 21, 12, 0, 0).unwrap(),
            kind,
            text: format!("note {id}"),
            rationale: Some("because".into()),
            anchors: anchors.iter().map(|s| s.to_string()).collect(),
            capture: "explicit".into(),
            quote: None,
        }
    }

    fn index() -> AnchorIndex {
        AnchorIndex::from_objects([KirObject::new("orders", ObjectKind::Table)
            .with_property("columns", json!([{"name":"id"}]))])
    }

    #[test]
    fn mapping_is_deterministic_and_golden_shaped() {
        let es = [
            entry("e1", NoteKind::Finding, &["orders", "ghost"]),
            entry("e2", NoteKind::DeadEnd, &[]),
        ];
        let idx = index();
        let a = map_entries("s1", ".ekos/session/inbox/s1.jsonl", "art", 0, &es, &idx, 2);
        let b = map_entries("s1", ".ekos/session/inbox/s1.jsonl", "art", 0, &es, &idx, 2);
        assert_eq!(
            serde_json::to_value((&a.claims, &a.relationships, &a.evidence, &a.events)).unwrap(),
            serde_json::to_value((&b.claims, &b.relationships, &b.evidence, &b.events)).unwrap()
        );
        assert_eq!(a.claims.len(), 2);
        assert_eq!(a.events.len(), 1);
        // e1: AnchoredTo(orders) + ObservedIn; e2: ObservedIn. `ghost` is unresolved → no edge.
        let kinds: Vec<String> = a.relationships.iter().map(|r| r.kind.to_string()).collect();
        assert_eq!(kinds.iter().filter(|k| *k == "AnchoredTo").count(), 1);
        assert_eq!(kinds.iter().filter(|k| *k == "ObservedIn").count(), 2);
        let c = &a.claims[0];
        assert_eq!(c.properties["tier"], "T0");
        assert_eq!(c.properties["review_status"], "unconfirmed");
        assert_eq!(
            c.properties["anchors"][1]["resolution"]["status"],
            "unresolved"
        );
        assert_eq!(c.evidence.len(), 1);
        assert!(a.evidence[0].fragment.contains("art"));
    }

    #[test]
    fn ambiguous_anchor_yields_no_edge() {
        let idx = AnchorIndex::from_objects([
            KirObject::new("dup", ObjectKind::Table),
            KirObject::new("dup", ObjectKind::Table),
        ]);
        let b = map_entries(
            "s1",
            "f",
            "a",
            0,
            &[entry("e", NoteKind::Finding, &["dup"])],
            &idx,
            1,
        );
        assert!(
            b.relationships
                .iter()
                .all(|r| r.kind.to_string() != "AnchoredTo")
        );
        assert_eq!(
            b.claims[0].properties["anchors"][0]["resolution"]["status"],
            "ambiguous"
        );
    }

    #[test]
    fn extracted_capture_is_kept_and_still_t0() {
        let mut e = entry("x", NoteKind::Finding, &[]);
        e.capture = "extracted".into();
        e.quote = Some("verbatim".into());
        let b = map_entries("s1", "f", "a", 0, &[e], &index(), 1);
        assert_eq!(b.claims[0].properties["capture"], "extracted");
        assert_eq!(b.claims[0].properties["tier"], "T0");
        assert_eq!(b.claims[0].properties["quote"], "verbatim");
    }
}
