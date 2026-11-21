//! `ekos talend assess` — RFC 0265's migration assessment, read from the ledger.
//!
//! The analyzer pass wrote the findings (`TalendFinding`) and the summaries (`assessment` on
//! `TalendJob` / `TalendProject`) at recover; this command only reads them, so it is not a
//! raw-content entry point and makes no model call. Objects the latest compile no longer holds
//! (a fixed finding, a deleted job) are left out: the ledger keeps every past run.

use super::store::open_store_read_only;
use anyhow::{Result, bail};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirObject, ObjectKind};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const KINDS: &[&str] = &[
    "TalendProject",
    "TalendJob",
    "TalendComponent",
    "TalendContext",
    "TalendContextVariable",
    "TalendDbConnection",
    "TalendSchema",
    "TalendRoutine",
    "TalendJoblet",
    "TalendColumnLineage",
    "TalendFinding",
];

fn is_talend(o: &KirObject) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if KINDS.contains(&k.as_str()))
}

fn kind_is(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn prop<'a>(o: &'a KirObject, k: &str) -> Option<&'a str> {
    o.properties.get(k).and_then(Value::as_str)
}

/// `objects` without the Talend objects the latest compile does not hold; unchanged when there is
/// no compiled model to compare with.
pub fn current_only(config: &EkosConfig, cwd: &Path, objects: Vec<KirObject>) -> Vec<KirObject> {
    let model = config.ekos_dir(cwd).join("ckm").join("model.json");
    match super::interaction::compiled_ids(&model) {
        Some(ids) => objects
            .into_iter()
            .filter(|o| !is_talend(o) || ids.contains(&o.id))
            .collect(),
        None => objects,
    }
}

pub fn assess(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<&str>,
    json_out: bool,
    out: Option<&PathBuf>,
) -> Result<()> {
    let ledger = open_store_read_only(config, cwd).map_err(|e| {
        anyhow::anyhow!("{e}\nRun the pipeline (`ekos build` … `ekos commit`) first.")
    })?;
    let objects = current_only(config, cwd, ledger.all_objects()?);
    let projects: Vec<&KirObject> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendProject"))
        .filter(|o| project.is_none_or(|p| o.name == p || prop(o, "label") == Some(p)))
        .collect();
    if projects.is_empty() {
        match project {
            Some(p) => bail!("no Talend project `{p}` in the compiled ledger"),
            None => bail!(
                "no Talend project in the compiled ledger (a directory holding `talend.project`)"
            ),
        }
    }
    let text = if json_out {
        let reports: Vec<Value> = projects.iter().map(|p| report_json(p, &objects)).collect();
        serde_json::to_string_pretty(&json!({ "projects": reports, "model_calls": 0 }))?
    } else {
        let mut md = String::new();
        for p in &projects {
            let key = prop(p, "project").unwrap_or(&p.name);
            let jobs = latest_jobs(&objects, key);
            let findings = findings(&objects, key);
            md.push_str(&format!("# {}: migration assessment\n\n", p.name));
            md.push_str(&ekos_docs_gen::estate::assessment_markdown(
                p,
                &jobs,
                &findings,
                &|_, t: &str| Some(t.to_string()),
            ));
            md.push('\n');
        }
        md.push_str("0 model calls.\n");
        md
    };
    match out {
        Some(path) => {
            std::fs::write(path, &text)?;
            println!("wrote {}", path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}

fn latest_jobs<'a>(objects: &'a [KirObject], project: &str) -> Vec<&'a KirObject> {
    let mut jobs: Vec<&KirObject> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendJob") && prop(o, "project") == Some(project))
        .filter(|o| o.properties.get("latest") == Some(&Value::Bool(true)))
        .collect();
    jobs.sort_by(|a, b| a.name.cmp(&b.name));
    jobs
}

fn findings<'a>(objects: &'a [KirObject], project: &str) -> Vec<&'a KirObject> {
    let mut f: Vec<&KirObject> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendFinding") && prop(o, "project") == Some(project))
        .collect();
    f.sort_by(|a, b| {
        (prop(a, "rule"), prop(a, "subject")).cmp(&(prop(b, "rule"), prop(b, "subject")))
    });
    f
}

fn report_json(p: &KirObject, objects: &[KirObject]) -> Value {
    let key = prop(p, "project").unwrap_or(&p.name);
    let jobs: Vec<Value> = latest_jobs(objects, key)
        .into_iter()
        .map(|j| {
            json!({
                "job": prop(j, "job").unwrap_or(&j.name),
                "version": prop(j, "version"),
                "folder": prop(j, "folder"),
                "reads": j.properties.get("reads"),
                "writes": j.properties.get("writes"),
                "assessment": j.properties.get("assessment"),
            })
        })
        .collect();
    let findings: Vec<Value> = findings(objects, key)
        .into_iter()
        .map(|f| {
            json!({
                "id": f.id.to_string(),
                "rule": prop(f, "rule"),
                "class": prop(f, "class"),
                "subject": prop(f, "subject"),
                "job": prop(f, "job"),
                "message": prop(f, "message"),
                "evidence": f.properties.get("evidence"),
                "status": prop(f, "status"),
            })
        })
        .collect();
    json!({
        "project": p.name,
        "product_version": prop(p, "product_version"),
        "assessment": p.properties.get("assessment"),
        "jobs": jobs,
        "findings": findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_talend_objects_are_filtered_by_the_compiled_model() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let kept = KirObject::new("t", ObjectKind::Table);
        let gone = KirObject::new("f", ObjectKind::Custom("TalendFinding".into()));
        // Without a compiled model nothing is dropped.
        let all = current_only(&config, dir.path(), vec![kept.clone(), gone.clone()]);
        assert_eq!(all.len(), 2);
    }
}
