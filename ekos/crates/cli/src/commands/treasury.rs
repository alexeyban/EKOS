//! `ekos treasury scan` — payment ↔ governance-approval matching (RFC 0032).
//!
//! Reads **already-committed** ledger objects (`TreasuryPayment` and `GovernanceProposal`, written
//! by the normal `build → recover → resolve → compile → commit` pipeline), scores candidate
//! approvals with `ekos_identity::treasury`, and writes each candidate at or above the floor as an
//! `unconfirmed` `Custom("AuthorizedBy")` relationship (payment → proposal). Never confirms
//! anything: the runtime is read-only and EKOS does not render a verdict on whether real money was
//! authorized. A candidate becomes trusted only when a human or agent confirms it through the
//! `ekos_identity_review` MCP tool.
//!
//! The summary ends with the **watchlist** — payments with no candidate at or above the floor.
//! That is an observation for a person to judge ("no `AuthorizedBy` relationship found" is itself a
//! citable answer), not an accusation.

use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_identity::treasury::{find_treasury_approval_candidates, payments_without_candidate};
use ekos_kir::{KirEvidence, KirId, KirRelationship, RelationshipKind, SourceLocation};
use std::collections::HashSet;
use std::path::Path;

pub const AUTHORIZED_BY: &str = "AuthorizedBy";
/// How many watchlist payments the summary prints before eliding the rest.
const WATCHLIST_PRINT_LIMIT: usize = 20;

pub fn scan(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ledger = open_store(config, cwd).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nRun `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit` first."
        )
    })?;

    let objects = ledger.all_objects()?;
    let payments = objects
        .iter()
        .filter(|o| ekos_identity::treasury::is_payment(o))
        .count();
    let proposals = objects
        .iter()
        .filter(|o| ekos_identity::treasury::is_proposal(o))
        .count();
    let candidates = find_treasury_approval_candidates(&objects);

    // Idempotent re-scan: a pair already connected by AuthorizedBy — confirmed, rejected or still
    // unconfirmed — is a human decision or an existing proposal; never overwrite or duplicate it.
    let known: HashSet<(KirId, KirId)> = ledger
        .all_relationships()?
        .iter()
        .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == AUTHORIZED_BY))
        .map(|r| (r.from, r.to))
        .collect();

    let (mut written, mut skipped) = (0usize, 0usize);
    for c in &candidates {
        if known.contains(&(c.payment, c.proposal)) {
            skipped += 1;
            continue;
        }
        let tx = objects
            .iter()
            .find(|o| o.id == c.payment)
            .and_then(|o| o.properties.get("tx_hash"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let proposal_id = objects
            .iter()
            .find(|o| o.id == c.proposal)
            .and_then(|o| o.properties.get("proposal_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let s = &c.signals;
        let ev = KirEvidence::new(
            SourceLocation::file("ekos treasury scan"),
            format!(
                "tx {tx} vs proposal {proposal_id}: recipient={:.2?} amount={:.2?} text_reference={:.2?} temporal={:.2?} before_approval={}",
                s.recipient_match, s.amount_match, s.text_reference, s.temporal, s.before_approval
            ),
        )
        .with_confidence(c.confidence);
        let ev_id = ev.id;
        ledger.append_evidence(&ev)?;

        // One candidate per (payment, proposal) pair; a re-scan converges instead of piling up.
        let mut rel = KirRelationship::deterministic(
            RelationshipKind::Custom(AUTHORIZED_BY.to_string()),
            c.payment,
            c.proposal,
            "",
        );
        rel.properties
            .insert("status".into(), serde_json::json!("unconfirmed"));
        rel.properties
            .insert("confidence".into(), serde_json::json!(c.confidence));
        rel.properties.insert(
            "recipient_match".into(),
            serde_json::json!(s.recipient_match),
        );
        rel.properties
            .insert("amount_match".into(), serde_json::json!(s.amount_match));
        rel.properties
            .insert("text_reference".into(), serde_json::json!(s.text_reference));
        rel.properties
            .insert("temporal".into(), serde_json::json!(s.temporal));
        rel.properties.insert(
            "before_approval".into(),
            serde_json::json!(s.before_approval),
        );
        rel.evidence.push(ev_id);
        ledger.append_relationship(&rel)?;
        written += 1;
    }

    let watchlist = payments_without_candidate(&objects, &candidates);
    println!("Treasury scan complete.");
    println!("  Payments: {payments}   Proposals: {proposals}");
    println!("  Candidates at or above the floor: {}", candidates.len());
    println!("  New unconfirmed AuthorizedBy relationships written: {written}");
    if skipped > 0 {
        println!("  Already known (skipped): {skipped}");
    }
    println!(
        "  Payments with no candidate approval: {} of {payments}",
        watchlist.len()
    );
    for p in watchlist.iter().take(WATCHLIST_PRINT_LIMIT) {
        let g = |k: &str| p.properties.get(k).and_then(|v| v.as_str()).unwrap_or("?");
        println!(
            "    - {} {} to {} ({})",
            g("value"),
            p.properties
                .get("token")
                .and_then(|v| v.as_str())
                .unwrap_or("native"),
            g("to"),
            g("tx_hash")
        );
    }
    if watchlist.len() > WATCHLIST_PRINT_LIMIT {
        println!("    … and {} more", watchlist.len() - WATCHLIST_PRINT_LIMIT);
    }
    if payments > 0 && proposals == 0 {
        println!(
            "  (No governance proposals in the ledger — set EKOS_SNAPSHOT_SPACE and re-run the pipeline.)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};
    use ekos_ledger::Ledger;
    use serde_json::json;
    use tempfile::tempdir;

    const RECIPIENT: &str = "0x1111111111111111111111111111111111111111";

    fn payment(tx: &str, to: &str, value: &str) -> KirObject {
        KirObject::new(
            format!("payment {tx}"),
            ObjectKind::Custom("TreasuryPayment".into()),
        )
        .with_property("tx_hash", json!(tx))
        .with_property("to", json!(to))
        .with_property("value", json!(value))
        .with_property("token", json!("USDC"))
        .with_property("timestamp", json!("2026-03-10T00:00:00+00:00"))
    }

    fn approved_proposal() -> KirObject {
        KirObject::new(
            "dao.eth: Fund marketing",
            ObjectKind::Custom("GovernanceProposal".into()),
        )
        .with_property("proposal_id", json!("0xprop"))
        .with_property("outcome", json!("approved"))
        .with_property("approved_at", json!("2026-03-01T00:00:00+00:00"))
        .with_property("approved_recipient", json!(RECIPIENT))
        .with_property("approved_amount", json!("50000"))
        .with_property("approved_token", json!("USDC"))
    }

    fn seed(config: &EkosConfig, dir: &Path) -> (KirId, KirId) {
        let ledger = Ledger::open(&config.ledger_path(dir)).unwrap();
        let matched = payment("0xaaaa", RECIPIENT, "50000");
        let orphan = payment(
            "0xbbbb",
            "0x3333333333333333333333333333333333333333",
            "777",
        );
        let (m, o) = (matched.id, orphan.id);
        ledger.append_object(&matched).unwrap();
        ledger.append_object(&orphan).unwrap();
        ledger.append_object(&approved_proposal()).unwrap();
        (m, o)
    }

    #[test]
    fn scan_writes_unconfirmed_evidenced_authorized_by_only_for_the_true_pair() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let (matched, _orphan) = seed(&config, dir.path());
        scan(&config, dir.path()).unwrap();

        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let rels = ledger.all_relationships().unwrap();
        assert_eq!(
            rels.len(),
            1,
            "the non-matching payment must get no relationship"
        );
        let r = &rels[0];
        assert!(matches!(&r.kind, RelationshipKind::Custom(k) if k == "AuthorizedBy"));
        assert_eq!(r.from, matched);
        assert_eq!(r.properties["status"], "unconfirmed");
        assert!(r.properties["confidence"].as_f64().unwrap() > 0.9);
        assert_eq!(
            r.evidence.len(),
            1,
            "every candidate cites the signals that produced it"
        );
        // The evidence is real and retrievable, naming the transaction.
        let ev = ledger.get_evidence(&r.evidence[0]).unwrap().unwrap();
        assert!(ev.fragment.contains("0xaaaa"));
    }

    #[test]
    fn the_candidate_is_queryable_through_the_runtime_like_any_other_relationship() {
        // RFC 0032: "queryable via existing MCP read tools" — `ekos_state` / `ekos_neighborhood`
        // are `Runtime::state` / relationships_for over the ledger.
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let (matched, orphan) = seed(&config, dir.path());
        scan(&config, dir.path()).unwrap();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let of_matched = ledger.relationships_for(&matched).unwrap();
        assert_eq!(of_matched.len(), 1);
        assert_eq!(of_matched[0].properties["status"], "unconfirmed");
        assert!(
            ledger.relationships_for(&orphan).unwrap().is_empty(),
            "no AuthorizedBy relationship IS the answer for an unmatched payment"
        );
    }

    #[test]
    fn rescan_does_not_duplicate_or_overwrite_a_reviewed_candidate() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        seed(&config, dir.path());
        scan(&config, dir.path()).unwrap();
        // A reviewer confirms it (what ekos_identity_review does).
        {
            let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
            let mut r = ledger.all_relationships().unwrap().remove(0);
            r.properties.insert("status".into(), json!("confirmed"));
            ledger.append_relationship(&r).unwrap();
        }
        scan(&config, dir.path()).unwrap();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let rels = ledger.all_relationships().unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(
            rels[0].properties["status"], "confirmed",
            "a human decision must survive a re-scan"
        );
    }

    #[test]
    fn scan_on_an_empty_ledger_writes_nothing() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let _l = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        scan(&config, dir.path()).unwrap();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        assert!(ledger.all_relationships().unwrap().is_empty());
    }
}
