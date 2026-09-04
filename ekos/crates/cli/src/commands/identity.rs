//! `ekos identity scan` — cross-system identity resolution (RFC 0029).
//!
//! Reads already-committed ledger objects (`Table`s and Transformation IR
//! `Source`/`Sink` nodes), scores candidate cross-system matches, and writes
//! each one as an `unconfirmed` `Custom("SameAs")` relationship. Never
//! merges anything — a candidate only becomes trusted once a human or agent
//! confirms it via the `ekos_identity_review` MCP tool.

use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_identity::cross_system::find_cross_system_candidates;
use ekos_kir::{KirEvidence, KirId, KirRelationship, RelationshipKind, SourceLocation};
use std::collections::HashSet;
use std::path::Path;

pub fn scan(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ledger = open_store(config, cwd).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nRun `ekos build && ekos recover && ekos compile && ekos commit` first."
        )
    })?;

    let objects = ledger.all_objects()?;
    let candidates = find_cross_system_candidates(&objects);

    // Idempotent re-scan: don't duplicate a candidate for a pair already
    // connected by a SameAs relationship, confirmed or not — a human
    // decision (or an existing unconfirmed proposal) already covers it.
    let existing = ledger.all_relationships()?;
    let already_known: HashSet<(KirId, KirId)> = existing
        .iter()
        .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "SameAs"))
        .flat_map(|r| [(r.from, r.to), (r.to, r.from)])
        .collect();

    let mut written = 0usize;
    let mut skipped = 0usize;

    for c in &candidates {
        if already_known.contains(&(c.a, c.b)) {
            skipped += 1;
            continue;
        }

        let ev = KirEvidence::new(
            SourceLocation::file("ekos identity scan"),
            format!(
                "column_overlap={:.2?} name_pattern={:.2} type_compat={:.2?}",
                c.signals.column_overlap, c.signals.name_pattern, c.signals.type_compat
            ),
        )
        .with_confidence(c.confidence);
        let ev_id = ev.id;
        ledger.append_evidence(&ev)?;

        // RFC 0135 Part C — one candidate per (a, b) pair; a re-scan must not pile up duplicates.
        let mut rel = KirRelationship::deterministic(
            RelationshipKind::Custom("SameAs".to_string()),
            c.a,
            c.b,
            "",
        );
        rel.properties
            .insert("status".into(), serde_json::json!("unconfirmed"));
        rel.properties
            .insert("confidence".into(), serde_json::json!(c.confidence));
        rel.properties.insert(
            "column_overlap".into(),
            serde_json::json!(c.signals.column_overlap),
        );
        rel.properties.insert(
            "name_pattern".into(),
            serde_json::json!(c.signals.name_pattern),
        );
        rel.properties.insert(
            "type_compat".into(),
            serde_json::json!(c.signals.type_compat),
        );
        rel.evidence.push(ev_id);
        ledger.append_relationship(&rel)?;
        written += 1;
    }

    println!("Identity scan complete.");
    println!("  Objects scanned: {}", objects.len());
    println!("  Candidates found: {}", candidates.len());
    println!("  New unconfirmed matches written: {written}");
    if skipped > 0 {
        println!("  Already known (skipped): {skipped}");
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

    fn seed_table(ledger: &Ledger, name: &str, columns: &[(&str, &str)]) -> KirId {
        let cols: Vec<serde_json::Value> = columns
            .iter()
            .map(|(n, t)| json!({"name": n, "data_type": t}))
            .collect();
        let obj = KirObject::new(name, ObjectKind::Table).with_property("columns", json!(cols));
        let id = obj.id;
        ledger.append_object(&obj).unwrap();
        id
    }

    #[test]
    fn scan_writes_unconfirmed_same_as_relationships() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        seed_table(
            &ledger,
            "cust_mstr",
            &[("id", "int"), ("name", "char"), ("status", "char")],
        );
        seed_table(
            &ledger,
            "customers",
            &[("id", "int"), ("name", "varchar"), ("status", "varchar")],
        );

        scan(&config, dir.path()).unwrap();

        let ledger2 = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let rels = ledger2.all_relationships().unwrap();
        assert_eq!(rels.len(), 1);
        assert!(matches!(&rels[0].kind, RelationshipKind::Custom(k) if k == "SameAs"));
        assert_eq!(rels[0].properties["status"], "unconfirmed");
        assert!(!rels[0].evidence.is_empty());
    }

    #[test]
    fn rescan_does_not_duplicate_known_candidates() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        seed_table(&ledger, "cust_mstr", &[("id", "int")]);
        seed_table(&ledger, "customers", &[("id", "int")]);

        scan(&config, dir.path()).unwrap();
        scan(&config, dir.path()).unwrap();

        let ledger2 = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let same_as_count = ledger2
            .all_relationships()
            .unwrap()
            .iter()
            .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "SameAs"))
            .count();
        assert_eq!(
            same_as_count, 1,
            "re-running scan must not duplicate a known candidate"
        );
    }

    #[test]
    fn scan_on_empty_ledger_writes_nothing() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let _ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        scan(&config, dir.path()).unwrap();
        let ledger2 = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        assert!(ledger2.all_relationships().unwrap().is_empty());
    }
}
