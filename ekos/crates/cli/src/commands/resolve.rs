use anyhow::Result;
use ekos_artifact::{ArtifactStore, FileSystemArtifactStore};
use ekos_compiler_core::EkosConfig;
use ekos_identity::{DefaultResolver, IdentityResolver};
use ekos_kir::KirGraph;
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);
    let store = FileSystemArtifactStore::new(&artifact_dir);

    // ── Collect all KnowledgeArtifact KirGraphs ───────────────────────────
    let ids = match store.list() {
        Ok(ids) => ids,
        Err(e) => anyhow::bail!("cannot list artifact store: {e}"),
    };

    let mut combined = KirGraph::new();
    let mut knowledge_count = 0usize;

    for id in &ids {
        let json = match store.read(id) {
            Ok(Some(j)) => j,
            _ => continue,
        };
        if json["artifact_type"].as_str() != Some("knowledge") {
            continue;
        }
        let graph: KirGraph = match serde_json::from_value(json["kir"].clone()) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("skipping artifact {id}: cannot decode KIR — {e}");
                continue;
            }
        };
        merge_into(&mut combined, graph);
        knowledge_count += 1;
    }

    if knowledge_count == 0 {
        println!("No knowledge artifacts found. Run `ekos recover` first.");
        return Ok(());
    }

    println!(
        "Loaded {} knowledge artifact(s) → {} objects, {} relationships",
        knowledge_count,
        combined.objects.len(),
        combined.relationships.len(),
    );

    // ── Run identity resolution ───────────────────────────────────────────
    let resolver = DefaultResolver::new();
    let result = resolver.resolve(&combined);

    // ── Print proposals ───────────────────────────────────────────────────
    if result.proposals.is_empty() {
        println!("\nNo merge proposals (all objects appear to be unique).");
    } else {
        println!("\nMerge proposals ({}):", result.proposals.len());
        for (i, p) in result.proposals.iter().enumerate() {
            println!(
                "  {}. '{}' ({}) — {} objects merged, confidence {:.2}",
                i + 1,
                p.canonical_name,
                p.canonical_kind,
                p.source_ids.len(),
                p.confidence,
            );
            for id in &p.source_ids {
                if let Some(obj) = combined.get_object(id) {
                    println!("       • {} ({})", obj.name, obj.kind);
                }
            }
        }
    }

    // ── Print conflicts ───────────────────────────────────────────────────
    if !result.conflicts.is_empty() {
        println!("\nConflicts ({}):", result.conflicts.len());
        for c in &result.conflicts {
            println!("  [CONFLICT] {}", c.description);
        }
    }

    // ── Stats ─────────────────────────────────────────────────────────────
    println!("\nStats:");
    println!("  Candidates evaluated : {}", result.stats.candidates_evaluated);
    println!("  Pairs compared       : {}", result.stats.pairs_compared);
    println!("  Merges proposed      : {}", result.stats.merges_proposed);
    println!("  Conflicts detected   : {}", result.stats.conflicts_detected);

    if !result.conflicts.is_empty() {
        anyhow::bail!(
            "{} identity conflict(s) detected — resolve manually before proceeding",
            result.conflicts.len()
        );
    }

    Ok(())
}

/// Append all nodes from `src` into `dst`.
fn merge_into(dst: &mut KirGraph, src: KirGraph) {
    for ev in src.evidence {
        dst.evidence.push(ev);
    }
    for obj in src.objects {
        dst.objects.push(obj);
    }
    for rel in src.relationships {
        dst.relationships.push(rel);
    }
    for ev in src.events {
        dst.events.push(ev);
    }
}

