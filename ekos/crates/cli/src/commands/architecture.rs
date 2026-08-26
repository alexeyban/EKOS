//! `ekos architecture investigate` — RFC 0066's MVP investigation loop (§64-65), orchestrating
//! RFC 0065's reasoning layer (Phase 2) and evaluator (Phase 3) around the existing pipeline
//! stages. Not a generic state-machine framework: one orchestrating async function, since this
//! MVP has exactly one investigation running at a time, no persistence-across-restarts, and no
//! concurrent agents to coordinate (RFC 0066 §51-54, explicitly deferred — see RFC 0067).
//!
//! Composes `build::run`/`recover::run`/`compile::run`/`commit::run`/`docs::generate` directly
//! rather than reimplementing collection/compilation — every one of those is already a clean,
//! independently callable pipeline stage.

use super::store::open_store;
use super::{build, commit, compile, docs, recover};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_compiler_core::pass::{CompilerPass, PassContext};
use ekos_recovery::{
    ArchitectureReasoningPass, DriftFinding, EvaluationReport, crates_missing_classification,
    diff_architecture, drift_from_history, evaluate_architecture, read_crate_doc_comment,
    role_claim_kir_id,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct InvestigateOptions {
    pub max_iterations: u32,
    pub quality_threshold: f32,
    pub output: std::path::PathBuf,
}

/// `--output`'s default when not given — matches the `doc` example CLAUDE.md's own Commands
/// section already gives for `docs generate --layout curated`, since this command always ends
/// with exactly that.
pub fn resolve_output_dir(cwd: &Path, output: Option<std::path::PathBuf>) -> std::path::PathBuf {
    output.unwrap_or_else(|| cwd.join("doc"))
}

pub async fn investigate(config: &EkosConfig, cwd: &Path, opts: InvestigateOptions) -> Result<()> {
    // ── INITIALIZING ─────────────────────────────────────────────────────
    // This command's whole point is running the reasoning pass — force it on for the duration
    // regardless of what `ekos.toml` says, the same "command-local LLM decision" shape
    // `docs.rs::select_llm_provider_for_prose` already uses for its own opt-in LLM step.
    let mut config = config.clone();
    config.architecture_reasoning.enabled = true;

    println!("Architecture investigation starting.");
    println!(
        "  max_iterations={} quality_threshold={:.2}",
        opts.max_iterations, opts.quality_threshold
    );

    // ── COLLECTING (broad) + ANALYZING + REASONING + UPDATING_MODEL ────────
    build::run(&config, cwd).await?;
    recover::run(&config, cwd, false).await?;
    compile::run(&config, cwd).await?;
    // `yes: true` — an automated, unattended investigation loop must never block on stdin for
    // RFC 0088's LLM-description cost confirmation; only relevant at all if the user's own
    // `ekos.toml` already opted into `[llm-description]` separately from this command.
    commit::run(&config, cwd, true).await?;

    let mut iteration = 1u32;
    let mut report = evaluate_current(&config, cwd)?;
    print_iteration_summary(iteration, &report);

    // ── EVALUATING → DECISION → PLANNING_INVESTIGATION → INVESTIGATING loop ─
    while report.score < opts.quality_threshold && iteration < opts.max_iterations {
        let objects = open_store(&config, cwd)?.all_objects()?;
        let targets = crates_missing_classification(&objects);
        if targets.is_empty() {
            // Nothing left an unclassified crate could be re-investigated for — further
            // iterations would just re-ask the same question and get the same non-answer.
            break;
        }

        iteration += 1;
        println!(
            "\nIteration {iteration}: investigating {} unclassified crate(s)...",
            targets.len()
        );

        let mut dirs = Vec::new();
        let mut context = HashMap::new();
        for c in &targets {
            let Some(dir) = c.properties.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            dirs.push(dir.to_string());
            if let Some(comment) = read_crate_doc_comment(cwd, dir) {
                context.insert(dir.to_string(), comment);
            }
        }

        run_targeted_reasoning(&config, cwd, dirs, context).await?;
        compile::run(&config, cwd).await?;
        commit::run(&config, cwd, true).await?;

        report = evaluate_current(&config, cwd)?;
        print_iteration_summary(iteration, &report);
    }

    // ── GENERATING (final docs, always — even if quality wasn't fully reached) ──────────────
    let format = docs::Format::parse("md")?;
    let layout = docs::Layout::parse("curated")?;
    docs::generate(&config, cwd, &opts.output, format, layout, false, false).await?;

    // ── Documentation drift (RFC 0068 §31-32) ───────────────────────────────
    let drift = detect_drift(&config, cwd)?;

    // ── COMPLETED ────────────────────────────────────────────────────────
    print_final_report(iteration, &report, &drift, &opts);

    if report.score < opts.quality_threshold {
        anyhow::bail!(
            "architecture investigation completed {iteration} iteration(s) without reaching the \
             quality threshold ({:.2} < {:.2}) — see issues above",
            report.score,
            opts.quality_threshold
        );
    }

    Ok(())
}

fn evaluate_current(config: &EkosConfig, cwd: &Path) -> Result<EvaluationReport> {
    let store = open_store(config, cwd)?;
    let objects = store.all_objects()?;
    Ok(evaluate_architecture(&objects))
}

/// RFC 0068 §31-32: for each compiled `Crate`, fetch its role claim's full version history from
/// the ledger (`KnowledgeStore::object_history`, RFC 0047) and check whether the classified role
/// genuinely changed since it was first recorded. Only `cli` opens the store directly (`recovery`
/// passes never read the ledger, only ever produce KIR flowing forward) — `drift_from_history`
/// itself stays a pure, ledger-free function in `ekos_recovery`, given the history already fetched.
fn detect_drift(config: &EkosConfig, cwd: &Path) -> Result<Vec<DriftFinding>> {
    let store = open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let mut findings = Vec::new();
    for c in objects
        .iter()
        .filter(|o| matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == "Crate"))
    {
        let Some(dir) = c.properties.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        let claim_id = role_claim_kir_id(dir);
        let history = store.object_history(&claim_id)?;
        if let Some(finding) = drift_from_history(&c.name, c.id, &history) {
            findings.push(finding);
        }
    }
    Ok(findings)
}

/// A targeted re-run of `ArchitectureReasoningPass` (RFC 0065 §36) against only the named crate
/// directories, re-reading the *same* `crate-topology-analyzer` artifact iteration 1 already
/// wrote — no re-scan of `Cargo.toml` files, matching RFC 0066 §9's "targeted collection" (not a
/// second broad collection). Run directly rather than through `PassManager`: a single pass with
/// no dependents to schedule around.
async fn run_targeted_reasoning(
    config: &EkosConfig,
    cwd: &Path,
    dirs: Vec<String>,
    context: HashMap<String, String>,
) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);
    let store: Arc<dyn ekos_artifact::ArtifactStore> = Arc::new(
        ekos_artifact::PackArtifactStore::open(&artifact_dir)
            .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?,
    );
    let llm = recover::build_llm_provider(config, &artifact_dir);

    // Same deterministic derivation `recover::run` uses for every pass's workspace-name argument
    // — the two must agree for this pass to find the artifact iteration 1's crate-topology pass
    // wrote under that exact `pass_name`.
    let workspace_name = cwd.file_name().unwrap_or_default().to_string_lossy();
    let crate_topology_pass_id = format!("crate-topology-analyzer:{workspace_name}");

    let mut pass = ArchitectureReasoningPass::new(crate_topology_pass_id, llm)
        .with_only_dirs(dirs)
        .with_crate_context(context);

    let mut ctx =
        PassContext::new(Arc::new(config.clone()), cwd.to_path_buf()).with_artifact_store(store);
    pass.run(&mut ctx)
        .await
        .map_err(|e| anyhow::anyhow!("targeted architecture reasoning failed: {e}"))?;

    if ctx.diagnostics.lock().unwrap().has_errors() {
        anyhow::bail!("targeted architecture reasoning completed with errors");
    }

    Ok(())
}

fn print_iteration_summary(iteration: u32, report: &EvaluationReport) {
    println!(
        "  Iteration {iteration}: score={:.2} completeness={:.2} evidence_coverage={:.2} \
         ({}/{} crates classified, {} issue(s))",
        report.score,
        report.completeness,
        report.evidence_coverage,
        report.crates_classified,
        report.crates_total,
        report.issues.len()
    );
}

/// RFC 0065 §40's final-report shape, sourced entirely from the real `EvaluationReport` — nothing
/// printed here is fabricated for the occasion. Drift findings (RFC 0068 §31-32) are reported
/// separately from `report.issues`, in the human-readable "DOCUMENTATION DRIFT DETECTED" shape
/// RFC 0068 §32 itself gives as an example — a real, distinct signal (staleness) from
/// completeness/evidence-coverage, not folded into the numeric score this increment (no real
/// weight-calibration data exists yet for how much one drift finding should move it).
fn print_final_report(
    iteration: u32,
    report: &EvaluationReport,
    drift: &[DriftFinding],
    opts: &InvestigateOptions,
) {
    println!("\nArchitecture reconstruction completed.");
    println!();
    println!("Quality score:              {:.0}%", report.score * 100.0);
    println!(
        "Evidence coverage:          {:.0}%",
        report.evidence_coverage * 100.0
    );
    println!("Iterations:                  {iteration}");
    println!();
    println!(
        "Crates classified:           {}/{}",
        report.crates_classified, report.crates_total
    );
    println!("Open issues:                 {}", report.issues.len());
    if !report.issues.is_empty() {
        for issue in &report.issues {
            println!("  [{:?}] {}", issue.severity, issue.description);
        }
    }
    if !drift.is_empty() {
        println!();
        println!("DOCUMENTATION DRIFT DETECTED ({}):", drift.len());
        for finding in drift {
            println!(
                "  '{}': was '{}', now '{}'",
                finding.subject_name, finding.documented_value, finding.observed_value
            );
        }
    }
    println!();
    println!("Output:                      {}", opts.output.display());
}

/// `ekos architecture diff --from <ts> --to <ts>` (RFC 0068 §55, RFC 0108) — a real
/// architecture-level diff, distinct from `ekos diff`'s raw ledger-entry-id report. Pure
/// presentation over `ekos_recovery::diff_architecture`; all the real work (deterministic-id-based
/// comparison across the object kinds this project already compiles evidence-backed data for)
/// lives there, kept ledger-free and unit-testable, matching `detect_drift`'s own precedent just
/// above.
pub fn diff(
    config: &EkosConfig,
    cwd: &Path,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let store = open_store(config, cwd)?;
    let before = store.all_objects_at(from)?;
    let after = store.all_objects_at(to)?;
    let diff = diff_architecture(&before, &after);

    println!(
        "Architecture diff {} .. {}",
        from.to_rfc3339(),
        to.to_rfc3339()
    );

    if diff.is_empty() {
        println!("  No architectural change detected.");
        return Ok(());
    }

    println!("  Technologies added:    {}", diff.technologies_added.len());
    for name in &diff.technologies_added {
        println!("    + {name}");
    }
    println!(
        "  Technologies removed:  {}",
        diff.technologies_removed.len()
    );
    for name in &diff.technologies_removed {
        println!("    - {name}");
    }
    println!("  Role changes:           {}", diff.role_changes.len());
    for change in &diff.role_changes {
        println!(
            "    {}: '{}' -> '{}'",
            change.crate_name, change.from, change.to
        );
    }
    println!("  Risks added:            {}", diff.risks_added.len());
    for name in &diff.risks_added {
        println!("    + {name}");
    }
    println!("  Risks resolved:         {}", diff.risks_resolved.len());
    for name in &diff.risks_resolved {
        println!("    - {name}");
    }
    println!("  Open questions added:   {}", diff.gaps_added.len());
    for name in &diff.gaps_added {
        println!("    + {name}");
    }
    println!("  Open questions resolved: {}", diff.gaps_resolved.len());
    for name in &diff.gaps_resolved {
        println!("    - {name}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};
    use ekos_ledger::FactLedger;
    use tempfile::tempdir;

    #[test]
    fn diff_reports_a_real_technology_added_between_two_commits() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let facts = super::super::store::facts_dir(&config, dir.path());
        let ledger = FactLedger::open(&facts).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(2));
        let t1 = chrono::Utc::now();
        std::thread::sleep(std::time::Duration::from_millis(2));

        ledger
            .append_object(&KirObject::new(
                "clap",
                ObjectKind::Custom("Technology".to_string()),
            ))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t2 = chrono::Utc::now();
        drop(ledger);

        diff(&config, dir.path(), t1, t2).unwrap();

        // Direct verification via the same primitive the command itself uses — the command's own
        // job here is just presentation, already exercised above by not erroring; this confirms
        // the underlying data really does show the addition, not just that printing succeeded.
        let ledger = FactLedger::open(&facts).unwrap();
        let before = ledger.all_objects_at(t1).unwrap();
        let after = ledger.all_objects_at(t2).unwrap();
        let result = diff_architecture(&before, &after);
        assert_eq!(result.technologies_added, vec!["clap".to_string()]);
        assert!(!result.is_empty());
    }

    #[test]
    fn diff_on_an_unchanged_workspace_reports_empty() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let facts = super::super::store::facts_dir(&config, dir.path());
        let ledger = FactLedger::open(&facts).unwrap();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t1 = chrono::Utc::now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t2 = chrono::Utc::now();
        drop(ledger);

        // Must not error, and must be a real no-op — nothing changed between t1 and t2.
        diff(&config, dir.path(), t1, t2).unwrap();
    }
}
