//! `ekos coverage` (RFC 0152) — did every input kind actually compile into anything?

use crate::coverage::{CoverageReport, compute_coverage, render};
use crate::detect::detect_workspace;
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_semantic::CkModel;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct CoverageJson {
    schema_version: u32,
    ok: bool,
    total_objects: usize,
    total_relationships: usize,
    unattributed_objects: usize,
    rows: Vec<RowJson>,
}

#[derive(Serialize)]
struct RowJson {
    kind: String,
    files_present: usize,
    objects_produced: usize,
    relationships_produced: usize,
    status: String,
    /// Present only on a finding — the likely cause, so a machine consumer surfaces the same
    /// diagnosis the text report does rather than a bare status string.
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

fn build_json(report: &CoverageReport) -> CoverageJson {
    CoverageJson {
        schema_version: 1,
        ok: !report.has_zero_coverage(),
        total_objects: report.total_objects,
        total_relationships: report.total_relationships,
        unattributed_objects: report.unattributed_objects,
        rows: report
            .rows
            .iter()
            .map(|r| RowJson {
                kind: r.kind.label().to_string(),
                files_present: r.files_present,
                objects_produced: r.objects_produced,
                relationships_produced: r.relationships_produced,
                status: r.status.label().to_string(),
                hint: r
                    .status
                    .is_finding()
                    .then(|| crate::coverage::hint_for(r).to_string()),
            })
            .collect(),
    }
}

/// Load the compiled model, or explain which step is missing.
pub fn load_model(config: &EkosConfig, cwd: &Path) -> Result<CkModel> {
    let model_path = config.ekos_dir(cwd).join("ckm").join("model.json");
    if ekos_common::compress::resolve_auto(&model_path).is_none() {
        anyhow::bail!(
            "no compiled model at {} — run `ekos build && ekos recover && ekos resolve && ekos \
             compile` first",
            model_path.display()
        );
    }
    ekos_common::compress::read_json_auto(&model_path).context("cannot read the compiled model")
}

pub fn run(config: &EkosConfig, cwd: &Path, json: bool, strict: bool, all: bool) -> Result<()> {
    let detection = detect_workspace(cwd, config)?;
    let model = load_model(config, cwd)?;
    let report = compute_coverage(&detection, &model);

    if json {
        println!("{}", serde_json::to_string_pretty(&build_json(&report))?);
    } else {
        print!("{}", render(&report, all));
    }

    // `--strict` is an explicit request for an exit code, so it overrides the `--json` "always
    // exit 0, read the `ok` field" contract `doctor --json` established rather than silently
    // doing nothing when both are passed.
    if strict && report.has_zero_coverage() {
        anyhow::bail!("coverage: one or more input kinds compiled to nothing");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::compute_coverage;
    use crate::detect::Detection;

    fn empty_report() -> CoverageReport {
        compute_coverage(
            &Detection::default(),
            &CkModel {
                version: 1,
                compiled_at: chrono::Utc::now(),
                objects: Vec::new(),
                relationships: Vec::new(),
                evidence_index: Default::default(),
            },
        )
    }

    #[test]
    fn json_carries_the_verdict_and_omits_hints_on_healthy_rows() {
        let json = build_json(&empty_report());
        assert!(json.ok);
        assert_eq!(json.schema_version, 1);
        assert!(
            json.rows.iter().all(|r| r.hint.is_none()),
            "a no-input row is not a finding and must carry no hint"
        );
    }

    #[test]
    fn missing_model_names_the_command_that_produces_it() {
        let tmp = tempfile::tempdir().unwrap();
        let err = load_model(&EkosConfig::default(), tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("ekos compile"),
            "the error must name the missing step, not just the missing file: {err}"
        );
    }
}
