use super::store::{open_store, store_display};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirEvidence, KirObject, KirRelationship, SourceLocation};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::{CkModel, CkmRelationship, EvidenceRecord};
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let model_path = config.ekos_dir(cwd).join("ckm").join("model.json");

    if ekos_common::compress::resolve_auto(&model_path).is_none() {
        anyhow::bail!(
            "CKM not found at {}[.zst]. Run `ekos compile` first.",
            model_path.display()
        );
    }

    let model: CkModel = ekos_common::compress::read_json_auto(&model_path)?;

    let ledger = open_ledger(config, cwd)?;

    let mut objects_written = 0usize;
    let mut objects_skipped = 0usize;
    let mut rels_written = 0usize;
    let mut evidence_written = 0usize;

    // Write evidence first (objects may reference evidence IDs).
    for ev_record in model.evidence_index.values() {
        let kir_ev = evidence_record_to_kir(ev_record);
        ledger.append_evidence(&kir_ev)?;
        evidence_written += 1;
    }

    // Write canonical objects.
    for ckm_obj in &model.objects {
        let kir_obj = ckm_object_to_kir(ckm_obj);
        if ledger.append_object(&kir_obj)? {
            objects_written += 1;
        } else {
            objects_skipped += 1;
        }
    }

    // Write canonical relationships.
    for ckm_rel in &model.relationships {
        let kir_rel = ckm_rel_to_kir(ckm_rel);
        if ledger.append_relationship(&kir_rel)? {
            rels_written += 1;
        }
    }

    println!("Commit complete.");
    println!("  Objects written:       {objects_written}");
    println!("  Objects skipped:       {objects_skipped} (already in ledger)");
    println!("  Relationships written: {rels_written}");
    println!("  Evidence records:      {evidence_written}");
    println!("  Ledger:                {}", store_display(config, cwd));

    Ok(())
}

fn open_ledger(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    open_store(config, cwd)
}

fn ckm_rel_to_kir(rel: &CkmRelationship) -> KirRelationship {
    use chrono::Utc;
    KirRelationship {
        id: rel.id,
        kind: rel.kind.clone(),
        from: rel.from,
        to: rel.to,
        properties: rel.properties.clone(),
        evidence: rel.evidence.iter().map(|e| e.id).collect(),
        created_at: Utc::now(),
    }
}

fn ckm_object_to_kir(obj: &ekos_semantic::CkmObject) -> KirObject {
    use chrono::Utc;
    KirObject {
        id: obj.id,
        name: obj.name.clone(),
        kind: obj.kind.clone(),
        properties: obj.properties.clone(),
        evidence: obj.evidence.iter().map(|e| e.id).collect(),
        created_at: Utc::now(),
    }
}

fn evidence_record_to_kir(ev: &EvidenceRecord) -> KirEvidence {
    use chrono::Utc;
    KirEvidence {
        id: ev.id,
        location: SourceLocation::file(ev.source.clone()),
        fragment: ev.fragment.clone(),
        confidence: ev.confidence,
        created_at: Utc::now(),
    }
}
