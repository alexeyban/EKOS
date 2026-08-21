use anyhow::Result;
use ekos_artifact::{ArtifactStore, PackArtifactStore};
use ekos_compiler_core::EkosConfig;
use ekos_identity::{DefaultResolver, IdentityResolver};
use ekos_kir::KirGraph;
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path, force: bool) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);
    let store = PackArtifactStore::open(&artifact_dir)
        .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?;

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
    // RFC 0063: only exact-normalized-name groups are actually auto-merged by `ekos compile` —
    // fuzzy groups become unconfirmed SameAs relationships for `ekos_identity_review` instead, so
    // this preview must label each proposal with how it will really be handled.
    if result.proposals.is_empty() {
        println!("\nNo merge proposals (all objects appear to be unique).");
    } else {
        let auto_merge_count = result
            .proposals
            .iter()
            .filter(|p| p.exact_name_match)
            .count();
        let review_count = result.proposals.len() - auto_merge_count;
        println!(
            "\nMerge proposals ({}) — {} will auto-merge, {} will be sent to review:",
            result.proposals.len(),
            auto_merge_count,
            review_count,
        );
        for (i, p) in result.proposals.iter().enumerate() {
            let disposition = if p.exact_name_match {
                "auto-merge"
            } else {
                "sent to review (fuzzy match, RFC 0063)"
            };
            println!(
                "  {}. '{}' ({}) — {} objects, confidence {:.2} — {}",
                i + 1,
                p.canonical_name,
                p.canonical_kind,
                p.source_ids.len(),
                p.confidence,
                disposition,
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
    println!(
        "  Candidates evaluated : {}",
        result.stats.candidates_evaluated
    );
    println!("  Pairs compared       : {}", result.stats.pairs_compared);
    println!("  Merges proposed      : {}", result.stats.merges_proposed);
    println!(
        "  Conflicts detected   : {}",
        result.stats.conflicts_detected
    );

    check_conflicts(result.conflicts.len(), force)
}

/// Whether to fail the pipeline on identity conflicts, or continue anyway.
///
/// With `force`, conflicts have already been printed above — they stay visible for the user to
/// revisit, but no longer block `compile`/`commit` from running.
fn check_conflicts(conflict_count: usize, force: bool) -> Result<()> {
    if conflict_count == 0 {
        return Ok(());
    }
    if force {
        println!("\n{conflict_count} identity conflict(s) detected — continuing anyway (--force)");
        return Ok(());
    }
    anyhow::bail!(
        "{conflict_count} identity conflict(s) detected — resolve manually before proceeding, \
         or pass --force to continue anyway"
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_conflicts_always_succeeds_regardless_of_force() {
        assert!(check_conflicts(0, false).is_ok());
        assert!(check_conflicts(0, true).is_ok());
    }

    #[test]
    fn conflicts_without_force_bail() {
        let err = check_conflicts(3, false).unwrap_err();
        assert!(err.to_string().contains("3 identity conflict"));
        assert!(err.to_string().contains("--force"));
    }

    #[test]
    fn conflicts_with_force_succeed() {
        assert!(check_conflicts(230, true).is_ok());
    }
}
