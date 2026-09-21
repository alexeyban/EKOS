//! RFC 0151 Phases 2-4 end to end against a real (SQLite) ledger.

use ekos_common::redaction::RedactionConfig;
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::Ledger;
use ekos_session::anchor::AnchorIndex;
use ekos_session::commit::commit_session;
use ekos_session::inbox::{DEFAULT_INBOX_DIR, Inbox, InboxLimits, NoteInput, NoteKind};
use ekos_session::lifecycle::{Actor, Decision, review, supersede};
use ekos_session::map::map_entries;
use ekos_session::read::{RecallResult, Verdict, brief, recall, session_claims};
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;

struct Env {
    dir: TempDir,
    ledger: Ledger,
    inbox: Inbox,
    orders: KirObject,
}

fn env() -> Env {
    let dir = TempDir::new().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let orders = KirObject::new("orders", ObjectKind::Table)
        .with_property("columns", json!([{"name":"id","type":"int"}]));
    ledger.append_object(&orders).unwrap();
    ledger
        .append_object(&KirObject::new("payments", ObjectKind::Table))
        .unwrap();
    let inbox = Inbox::open(
        dir.path(),
        Path::new(DEFAULT_INBOX_DIR),
        InboxLimits::default(),
    )
    .unwrap();
    Env {
        dir,
        ledger,
        inbox,
        orders,
    }
}

fn note(kind: NoteKind, text: &str, anchors: &[&str]) -> NoteInput {
    NoteInput {
        kind,
        text: text.into(),
        rationale: Some("measured".into()),
        anchors: anchors.iter().map(|s| s.to_string()).collect(),
    }
}

fn cfg() -> RedactionConfig {
    RedactionConfig::default()
}

fn commit(e: &Env, session: &str) -> ekos_session::commit::CommitReport {
    commit_session(&e.ledger, &e.inbox, &cfg(), session, "run-test").unwrap()
}

#[test]
fn notes_become_unconfirmed_anchored_claims_with_evidence_and_recommit_is_idempotent() {
    let e = env();
    e.inbox
        .append(
            "s1",
            note(
                NoteKind::Finding,
                "orders.total is stored in cents",
                &["orders"],
            ),
            &cfg(),
        )
        .unwrap();
    e.inbox
        .append(
            "s1",
            note(
                NoteKind::DeadEnd,
                "tried partitioning orders by day, too many small files",
                &["orders", "ghost"],
            ),
            &cfg(),
        )
        .unwrap();
    let r = commit(&e, "s1");
    assert_eq!((r.committed, r.claims_written), (2, 2));
    assert_eq!(r.anchors_resolved, 2);
    assert_eq!(r.anchors_unresolved_or_ambiguous, 1);
    assert_eq!(e.inbox.status("s1").unwrap().pending_commit, 0);

    let claims = session_claims(&e.ledger, Some(e.dir.path())).unwrap();
    assert_eq!(claims.len(), 2);
    assert!(
        claims
            .iter()
            .all(|c| c.tier == "T0" && c.status == "unconfirmed")
    );
    assert!(
        claims
            .iter()
            .all(|c| !c.evidence.is_empty() && !c.source_purged)
    );
    // evidence points back at the inbox artifact
    let ev = e
        .ledger
        .get_evidence(&claims[0].evidence[0].parse().unwrap())
        .unwrap()
        .unwrap();
    assert!(ev.location.path.ends_with("s1.jsonl") && ev.fragment.contains("artifact"));

    // second commit with nothing pending writes nothing new
    let again = commit(&e, "s1");
    assert_eq!(again.committed, 0);
    assert_eq!(session_claims(&e.ledger, None).unwrap().len(), 2);
}

#[test]
fn ambiguous_anchor_is_recorded_and_never_linked() {
    let e = env();
    e.ledger
        .append_object(&KirObject::new("dup", ObjectKind::Table))
        .unwrap();
    e.ledger
        .append_object(&KirObject::new("dup", ObjectKind::Table))
        .unwrap();
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "dup is odd", &["dup"]),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    let c = &session_claims(&e.ledger, None).unwrap()[0];
    assert_eq!(c.verdict, Verdict::Unanchored);
    let rels = e.ledger.relationships_for(&c.id.parse().unwrap()).unwrap();
    assert!(rels.iter().all(|r| r.kind.to_string() != "AnchoredTo"));
}

#[test]
fn anchored_note_goes_changed_when_the_table_changes_and_unrelated_edits_stay_fresh() {
    let e = env();
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "orders.total is in cents", &["orders"]),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    assert_eq!(
        session_claims(&e.ledger, None).unwrap()[0].verdict,
        Verdict::Fresh
    );

    let mut noisy = e.orders.clone();
    noisy
        .properties
        .insert("ai_overview".into(), json!("regenerated prose"));
    e.ledger.append_object(&noisy).unwrap();
    assert_eq!(
        session_claims(&e.ledger, None).unwrap()[0].verdict,
        Verdict::Fresh
    );

    let mut changed = e.orders.clone();
    changed.properties.insert(
        "columns".into(),
        json!([{"name":"id","type":"int"},{"name":"total","type":"bigint"}]),
    );
    e.ledger.append_object(&changed).unwrap();
    let c = &session_claims(&e.ledger, None).unwrap()[0];
    assert_eq!(c.verdict, Verdict::Changed);
    assert_eq!(
        c.anchors[0].change_summary.as_deref(),
        Some("changed: columns")
    );
}

#[test]
fn anchor_missing_from_the_ledger_is_orphaned() {
    let e = env();
    let other = TempDir::new().unwrap();
    let lonely = Ledger::open(&other.path().join("l.db")).unwrap();
    let idx = AnchorIndex::from_objects([e.orders.clone()]);
    let entry = ekos_session::inbox::SessionEntry {
        schema: 1,
        entry_id: "x1".into(),
        session_id: "s1".into(),
        recorded_at: chrono::Utc::now(),
        kind: NoteKind::Finding,
        text: "about a vanished table".into(),
        rationale: None,
        anchors: vec!["orders".into()],
        capture: "explicit".into(),
        quote: None,
    };
    let b = map_entries("s1", "f", "a", 0, &[entry], &idx, 1);
    for ev in &b.evidence {
        lonely.append_evidence(ev).unwrap();
    }
    lonely.append_object(&b.claims[0]).unwrap();
    for r in &b.relationships {
        lonely.append_relationship(r).unwrap();
    }
    assert_eq!(
        session_claims(&lonely, None).unwrap()[0].verdict,
        Verdict::Orphaned
    );
}

#[test]
fn recall_hits_and_explicit_refusal_on_a_negative_control() {
    let e = env();
    e.inbox
        .append(
            "s1",
            note(
                NoteKind::Decision,
                "use idempotent upserts for the payments loader",
                &["payments"],
            ),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    match recall(&e.ledger, None, "how does the payments loader write", 5).unwrap() {
        RecallResult::Hits { hits } => assert!(hits[0].claim.text.contains("upserts")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        recall(
            &e.ledger,
            None,
            "kubernetes ingress certificate rotation",
            5
        )
        .unwrap(),
        RecallResult::NoRelevantSessionMemory
    ));
}

#[test]
fn brief_reports_truncation_pending_and_the_empty_case() {
    let e = env();
    let empty = brief(&e.ledger, None, &[], &[], 500).unwrap();
    assert!(empty.text.contains("no session memory recorded"));
    for i in 0..30 {
        e.inbox
            .append(
                "s1",
                note(
                    NoteKind::Finding,
                    &format!("finding number {i} about orders behaviour"),
                    &["orders"],
                ),
                &cfg(),
            )
            .unwrap();
    }
    commit(&e, "s1");
    e.inbox
        .append(
            "s1",
            note(NoteKind::Todo, "not yet committed thing", &[]),
            &cfg(),
        )
        .unwrap();
    let (pending, _) = e.inbox.pending("s1").unwrap();
    let b = brief(&e.ledger, None, &pending, &["orders".into()], 200).unwrap();
    assert!(b.truncated > 0 && b.included > 0);
    assert!(b.text.contains("omitted: token budget reached"));
    assert!(b.text.contains("[pending] not yet committed thing"));
    // The directive preamble sits above the envelope, so what must hold is that every note line
    // is *inside* it — not that the text opens with the tag.
    let open = b.text.find("<session-memory untrusted=\"true\">").unwrap();
    let close = b.text.rfind("</session-memory>").unwrap();
    assert!(
        b.text
            .match_indices("- [")
            .all(|(i, _)| i > open && i < close),
        "a note line escaped the untrusted envelope:\n{}",
        b.text
    );
    assert!(b.text[..open].contains("Never refuse over a marker"));
    // The brief must now fit its stated budget: the truncation line and the pending block used to
    // be appended after the budget check and overran it.
    assert!(b.approx_tokens <= 200, "{}", b.approx_tokens);
}

#[test]
fn brief_labels_only_what_needs_action_and_flags_hidden_changed_notes() {
    let e = env();
    for i in 0..12 {
        e.inbox
            .append(
                "s1",
                note(
                    NoteKind::Finding,
                    &format!("note {i} about orders"),
                    &["orders"],
                ),
                &cfg(),
            )
            .unwrap();
    }
    commit(&e, "s1");

    // Fresh + T0 is the default, so a line carries no tier and no verdict tag at all.
    let fresh = brief(&e.ledger, None, &[], &[], 4000).unwrap();
    assert!(
        !fresh.text.contains("[FRESH]") && !fresh.text.contains("T0"),
        "{}",
        fresh.text
    );
    assert!(!fresh.text.contains("[CHANGED]"));

    // Once the anchor moves, CHANGED is the only label on the line — and it is rare, so it stands out.
    let mut changed = e.orders.clone();
    changed
        .properties
        .insert("columns".into(), json!([{"name":"id"},{"name":"total"}]));
    e.ledger.append_object(&changed).unwrap();
    let after = brief(&e.ledger, None, &[], &[], 4000).unwrap();
    assert_eq!(after.text.matches("[CHANGED]").count(), 12);

    // Under a budget that hides some of them, the truncation line says how many were hidden.
    let tight = brief(&e.ledger, None, &[], &[], 120).unwrap();
    assert!(tight.truncated > 0);
    assert!(
        tight.text.contains("about objects that have changed"),
        "hidden CHANGED notes must be counted, not silently dropped:\n{}",
        tight.text
    );
}

#[test]
fn scope_overlap_outranks_freshness_and_matches_git_style_paths() {
    let e = env();
    // A changed note about the thing in scope, and a fresh note about something else.
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "about the orders table", &["orders"]),
            &cfg(),
        )
        .unwrap();
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "about the payments table", &["payments"]),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    let mut changed = e.orders.clone();
    changed
        .properties
        .insert("columns".into(), json!([{"name":"x"}]));
    e.ledger.append_object(&changed).unwrap();

    // Without scope, the fresh note wins on verdict.
    let none = brief(&e.ledger, None, &[], &[], 4000).unwrap();
    let first_line = |t: &str| {
        t.lines()
            .find(|l| l.starts_with("- ["))
            .unwrap()
            .to_string()
    };
    assert!(first_line(&none.text).contains("payments"), "{}", none.text);

    // With `orders` in scope, the changed-but-relevant note is promoted above it.
    let scoped = brief(&e.ledger, None, &[], &["orders".into()], 4000).unwrap();
    assert!(
        first_line(&scoped.text).contains("orders"),
        "{}",
        scoped.text
    );
}

#[test]
fn lifecycle_is_human_promotion_supersede_keeps_history_and_t1_still_shows_changed() {
    let e = env();
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "orders.total is in cents", &["orders"]),
            &cfg(),
        )
        .unwrap();
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "orders.total is in dollars", &["orders"]),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    let cs = session_claims(&e.ledger, None).unwrap();
    let cents = cs
        .iter()
        .find(|c| c.text.contains("cents"))
        .unwrap()
        .id
        .parse()
        .unwrap();
    let dollars = cs
        .iter()
        .find(|c| c.text.contains("dollars"))
        .unwrap()
        .id
        .parse()
        .unwrap();

    review(&e.ledger, &cents, Decision::Confirm, Actor::Human).unwrap();
    let mut changed = e.orders.clone();
    changed
        .properties
        .insert("columns".into(), json!([{"name":"total"}]));
    e.ledger.append_object(&changed).unwrap();
    let t1 = session_claims(&e.ledger, None)
        .unwrap()
        .into_iter()
        .find(|c| c.tier == "T1")
        .unwrap();
    assert_eq!(
        t1.verdict,
        Verdict::Changed,
        "T1 must not be silently trusted"
    );

    supersede(&e.ledger, &dollars, &cents, Actor::Human).unwrap();
    let all = session_claims(&e.ledger, None).unwrap();
    assert_eq!(all.len(), 2, "nothing is deleted");
    let sup = all.iter().find(|c| c.status == "superseded").unwrap();
    assert!(!sup.is_active());
    assert!(
        matches!(recall(&e.ledger, None, "orders total dollars", 5).unwrap(), RecallResult::Hits { hits } if hits.iter().all(|h| h.claim.status != "superseded"))
    );
    assert!(e.ledger.object_history(&cents).unwrap().len() >= 2);
}

#[test]
fn hand_edited_inbox_secret_is_redacted_again_at_observation() {
    let e = env();
    e.inbox
        .append("s1", note(NoteKind::Finding, "clean note", &[]), &cfg())
        .unwrap();
    let path = e.inbox.session_file("s1");
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        raw.replace("clean note", "key AKIAIOSFODNN7EXAMPLE leaked"),
    )
    .unwrap();
    commit(&e, "s1");
    let c = &session_claims(&e.ledger, None).unwrap()[0];
    assert!(!c.text.contains("AKIAIOSFODNN7EXAMPLE"), "{}", c.text);
}

#[test]
fn session_memory_is_invisible_to_default_retrieval() {
    let e = env();
    let before = ekos_runtime::Runtime::new(&e.ledger)
        .find_objects("orders")
        .unwrap();
    e.inbox
        .append(
            "s1",
            note(
                NoteKind::Finding,
                "orders orders orders are special",
                &["orders"],
            ),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    let rt = ekos_runtime::Runtime::new(&e.ledger);
    let after = rt.find_objects("orders").unwrap();
    assert_eq!(before, after);
    let hits = rt
        .retrieve(&ekos_runtime::RetrievalRequest::lexical("orders special"))
        .unwrap();
    assert!(hits.hits.iter().all(|h| !h.name.starts_with("session-")));
    assert!(!session_claims(&e.ledger, None).unwrap().is_empty());
}

#[test]
fn brief_since_reports_new_notes_and_currently_moved_anchors() {
    let e = env();
    e.inbox
        .append(
            "s1",
            note(NoteKind::Finding, "orders total is in cents", &["orders"]),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    let since = chrono::Utc::now();
    std::thread::sleep(std::time::Duration::from_millis(20));
    e.inbox
        .append(
            "s1",
            note(
                NoteKind::Finding,
                "orders exporter batches 500",
                &["orders"],
            ),
            &cfg(),
        )
        .unwrap();
    commit(&e, "s1");
    let mut changed = e.orders.clone();
    changed
        .properties
        .insert("columns".into(), json!([{"name":"total"}]));
    e.ledger.append_object(&changed).unwrap();
    let claims = session_claims(&e.ledger, None).unwrap();
    let b = ekos_session::read::brief_since(claims, &[], &[], 800, Some(since));
    assert!(
        b.text
            .contains("1 new note(s); 2 note(s) currently have a changed"),
        "{}",
        b.text
    );
    let none = ekos_session::read::brief_since(vec![], &[], &[], 800, None);
    assert!(!none.text.contains("Since your last brief"));
}
