use super::store::{open_store, store_display};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirEvidence, KirGraph, KirObject, KirRelationship, SourceLocation};
use ekos_ledger::KnowledgeStore;
use ekos_semantic::{CkModel, CkmRelationship, EvidenceRecord, rollup};
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

    // RFC 0044: hierarchical rollups run here, not in `SemanticCompilerPass` (`ekos compile`) —
    // `File` objects, the only kind rollups group by directly, are written straight to the ledger
    // by `ekos build`, never through a `KnowledgeArtifact` the compiler reads. This is the first
    // point in the pipeline where `File` objects (just-committed-or-earlier) and CKM-derived
    // objects (just committed above) coexist in one place.
    let rollups_added = commit_rollups(&*ledger)?;

    println!("Commit complete.");
    println!("  Objects written:       {objects_written}");
    println!("  Objects skipped:       {objects_skipped} (already in ledger)");
    println!("  Relationships written: {rels_written}");
    println!("  Evidence records:      {evidence_written}");
    if rollups_added > 0 {
        println!("  Subsystem rollups:     {rollups_added}");
    }
    println!("  Ledger:                {}", store_display(config, cwd));

    Ok(())
}

/// Reads the ledger's full current object/relationship set, synthesizes subsystem rollups over
/// it (RFC 0044), and appends only the newly-produced `Rollup` objects/relationships/evidence —
/// everything else was already committed above or in a prior run. Returns the number of rollup
/// objects newly written (0 on a re-run against unchanged input, since `append_object` on an
/// already-known deterministic id is a no-op).
fn commit_rollups(ledger: &dyn KnowledgeStore) -> Result<usize> {
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;

    let original_object_count = objects.len();
    let original_relationship_count = relationships.len();

    let mut graph = KirGraph {
        objects,
        relationships,
        events: Vec::new(),
        evidence: Vec::new(),
    };
    rollup::synthesize_rollups(&mut graph, rollup::DEFAULT_DIRECTORY_DEPTH);

    let new_objects = &graph.objects[original_object_count..];
    let new_relationships = &graph.relationships[original_relationship_count..];

    for ev in &graph.evidence {
        ledger.append_evidence(ev)?;
    }
    let mut written = 0usize;
    for obj in new_objects {
        if ledger.append_object(obj)? {
            written += 1;
        }
    }
    for rel in new_relationships {
        ledger.append_relationship(rel)?;
    }

    Ok(written)
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
