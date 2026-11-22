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

/// `ekos talend explain`: [`explain_json`] from the CLI (`--component JOB:COMPONENT` names both).
pub fn explain(
    config: &EkosConfig,
    cwd: &Path,
    job: Option<&str>,
    component: Option<&str>,
    project: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let (job, component) = match (job, component.map(|c| c.split_once(':'))) {
        (_, Some(Some((j, c)))) => (j.to_string(), Some(c.to_string())),
        (Some(j), Some(None)) => (j.to_string(), component.map(str::to_string)),
        (Some(j), None) => (j.to_string(), None),
        _ => bail!("name a job: `--job <JOB>` or `--component <JOB>:<COMPONENT>`"),
    };
    let ledger = open_store_read_only(config, cwd).map_err(|e| {
        anyhow::anyhow!("{e}\nRun the pipeline (`ekos build` … `ekos commit`) first.")
    })?;
    let mut args = json!({ "job": job });
    if let Some(c) = component {
        args["component"] = json!(c);
    }
    if let Some(p) = project {
        args["project"] = json!(p);
    }
    let v = explain_json(config, cwd, &*ledger, &args)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let list = |k: &str| v[k].as_array().map_or(0, Vec::len);
    println!(
        "{} (project {}, version {}): {} components, {} links, {} context variables, {} lineage rows, {} findings",
        v["job"].as_str().unwrap_or_default(),
        v["project"].as_str().unwrap_or("?"),
        v["version"].as_str().unwrap_or("?"),
        list("components"),
        list("links"),
        list("context"),
        list("lineage"),
        list("findings"),
    );
    for c in v["components"].as_array().into_iter().flatten() {
        println!(
            "  {} ({})",
            c["component"].as_str().unwrap_or_default(),
            c["type"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

/// RFC 0268 phase 2: `ekos_talend_explain` — one job, or one of its components, as the ledger
/// holds it: what it reads and writes, its components and the links between them, the context
/// variables it uses (with whether a run can replace them), its column lineage and its findings.
/// Read-only, through the call's reader (a scoped store under a restricting scope). Objects the
/// latest compile no longer holds are left out.
pub fn explain_json(
    config: &EkosConfig,
    cwd: &Path,
    reader: &dyn ekos_ledger::KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let job_name = args
        .get("job")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("`job` is required"))?;
    let component = args.get("component").and_then(Value::as_str);
    let objects = current_only(config, cwd, reader.all_objects()?);
    let job = one_job(
        &objects,
        job_name,
        args.get("project").and_then(Value::as_str),
    )?;
    let version = prop(job, "version");
    let job_project = prop(job, "project");
    let of_job = |o: &&KirObject| {
        prop(o, "job") == Some(job_name)
            && prop(o, "project").is_none_or(|p| Some(p) == job_project)
    };
    let job_rels = reader.relationships_for(&job.id)?;
    let contained: std::collections::HashSet<String> = job_rels
        .iter()
        .filter(|r| r.from == job.id && r.kind.to_string() == "Contains")
        .map(|r| r.to.to_string())
        .collect();
    let components: Vec<&KirObject> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendComponent") && contained.contains(&o.id.to_string()))
        .collect();
    if let Some(c) = component
        && !components.iter().any(|o| prop(o, "unique_name") == Some(c))
    {
        bail!("`{job_name}` has no component `{c}`");
    }
    let lineage: Vec<Value> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendColumnLineage"))
        .filter(of_job)
        .filter(|o| prop(o, "job_version") == version)
        .filter(|o| component.is_none_or(|c| prop(o, "component") == Some(c)))
        .map(|o| {
            json!({
                "component": prop(o, "component"),
                "target": o.properties.get("target"),
                "data_action": prop(o, "data_action"),
                "description": prop(o, "description"),
                "origins": o.properties.get("origins"),
                "gaps": o.properties.get("gaps"),
            })
        })
        .collect();
    let findings: Vec<Value> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendFinding"))
        .filter(of_job)
        .map(|f| {
            json!({
                "rule": prop(f, "rule"),
                "subject": prop(f, "subject"),
                "message": prop(f, "message"),
                "status": prop(f, "status"),
            })
        })
        .collect();
    Ok(json!({
        "project": job_project,
        "job": job_name,
        "version": version,
        "folder": prop(job, "folder"),
        "description": prop(job, "description"),
        "reads": job.properties.get("reads"),
        "writes": job.properties.get("writes"),
        "implicit_context_load": job.properties.get("implicit_context_load"),
        "assessment": job.properties.get("assessment"),
        "components": components_json(&components, component),
        "links": links_json(reader, &components, component)?,
        "context": context_json(&objects, job, &job_rels),
        "lineage": lineage,
        "findings": findings,
        "note": "As recovered from the project's files; for its translation to dbt (fidelity, rule, SQL), see `ekos lakehouse talend report`.",
    }))
}

/// The latest version of job `name`, in `project` when given; refused when absent or ambiguous.
fn one_job<'a>(
    objects: &'a [KirObject],
    name: &str,
    project: Option<&str>,
) -> Result<&'a KirObject> {
    let jobs: Vec<&KirObject> = objects
        .iter()
        .filter(|o| kind_is(o, "TalendJob") && prop(o, "job") == Some(name))
        .filter(|o| o.properties.get("latest") == Some(&Value::Bool(true)))
        .filter(|o| project.is_none_or(|p| prop(o, "project") == Some(p)))
        .collect();
    match jobs.as_slice() {
        [j] => Ok(*j),
        [] => bail!("no Talend job `{name}` in the compiled ledger"),
        many => bail!(
            "`{name}` is a job of several projects ({}): name the `project`",
            many.iter()
                .filter_map(|j| prop(j, "project"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The context variables a job uses, each with its default and whether a run can replace it.
fn context_json(
    objects: &[KirObject],
    job: &KirObject,
    rels: &[ekos_kir::KirRelationship],
) -> Vec<Value> {
    let by_id: std::collections::HashMap<String, &KirObject> =
        objects.iter().map(|o| (o.id.to_string(), o)).collect();
    rels.iter()
        .filter(|r| r.kind.to_string() == "UsesContext" && r.from == job.id)
        .filter_map(|r| by_id.get(&r.to.to_string()).map(|v| (r, *v)))
        .map(|(r, v)| {
            let secret = v.properties.get("secret") == Some(&Value::Bool(true));
            // A secret's value is never stored (RFC 0043); say so instead of nothing.
            let default = if secret {
                json!("(secret, not stored)")
            } else {
                v.properties.get("default").cloned().unwrap_or(Value::Null)
            };
            json!({
                "variable": prop(v, "variable"),
                "context": prop(v, "context"),
                "type": prop(v, "talend_type"),
                "default": default,
                "overridable_at_runtime": r.properties.get("overridable_at_runtime"),
            })
        })
        .collect()
}

/// The row and trigger links between a job's components (touching `component`, when given).
fn links_json(
    reader: &dyn ekos_ledger::KnowledgeReader,
    components: &[&KirObject],
    component: Option<&str>,
) -> Result<Vec<Value>> {
    let name_of: std::collections::HashMap<String, &str> = components
        .iter()
        .map(|o| (o.id.to_string(), prop(o, "unique_name").unwrap_or(&o.name)))
        .collect();
    let mut links = Vec::new();
    for c in components {
        for r in reader.relationships_for(&c.id)? {
            let kind = r.kind.to_string();
            if r.from != c.id || !matches!(kind.as_str(), "RowLink" | "Triggers") {
                continue;
            }
            let (Some(from), Some(to)) = (
                name_of.get(&r.from.to_string()),
                name_of.get(&r.to.to_string()),
            ) else {
                continue;
            };
            if component.is_none_or(|x| x == *from || x == *to) {
                links.push(
                    json!({ "kind": kind, "from": from, "to": to, "properties": r.properties }),
                );
            }
        }
    }
    Ok(links)
}

/// The components (only `component`, when given, with its whole configuration).
fn components_json(components: &[&KirObject], component: Option<&str>) -> Vec<Value> {
    let text = |x: &Value| {
        if x.is_null() {
            String::new()
        } else {
            serde_json::to_string_pretty(x).unwrap_or_default()
        }
    };
    components
        .iter()
        .filter(|o| component.is_none_or(|c| prop(o, "unique_name") == Some(c)))
        .map(|o| {
            let mut v = json!({
                "component": prop(o, "unique_name"),
                "type": prop(o, "component_type"),
                "category": prop(o, "category"),
                "label": prop(o, "label"),
                "line": o.properties.get("line"),
            });
            if component.is_none() {
                return v;
            }
            // One component: its whole configuration (parameters as the analyzer kept them,
            // already redacted), schemas and map, and the same as text for a code view.
            for k in ["parameters", "schemas", "mapper", "elt", "engine"] {
                v[k] = o.properties.get(k).cloned().unwrap_or(Value::Null);
            }
            v["configuration"] = json!(text(&v["parameters"]));
            let map = ["mapper", "elt", "schemas"]
                .iter()
                .map(|k| &v[*k])
                .find(|x| !x.is_null())
                .map(text)
                .unwrap_or_default();
            v["map"] = json!(map);
            v
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(
        store: &dyn ekos_ledger::KnowledgeStore,
        kind: &str,
        key: &str,
        props: Value,
    ) -> ekos_kir::KirId {
        let mut o = KirObject::new(key, ObjectKind::Custom(kind.into()));
        o.id = ekos_kir::KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            key.as_bytes(),
        ));
        if let Value::Object(m) = props {
            o.properties.extend(m);
        }
        store.append_object(&o).unwrap();
        o.id
    }

    fn rel(
        store: &dyn ekos_ledger::KnowledgeStore,
        kind: &str,
        from: ekos_kir::KirId,
        to: ekos_kir::KirId,
        props: Value,
    ) {
        let mut r = ekos_kir::KirRelationship::deterministic(
            ekos_kir::RelationshipKind::Custom(kind.into()),
            from,
            to,
            "",
        );
        if let Value::Object(m) = props {
            r.properties.extend(m);
        }
        store.append_relationship(&r).unwrap();
    }

    /// RFC 0268 phase 2: `ekos_talend_explain` gives a job's components, links, context (a
    /// secret's value never) and lineage; one component, its whole configuration.
    #[test]
    fn a_job_is_explained_with_its_components_links_context_and_lineage() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = ekos_ledger::Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let job = put(
            &ledger,
            "TalendJob",
            "j",
            json!({"job": "jLoad", "project": "P", "version": "0.1", "latest": true, "reads": ["s.a"], "writes": ["s.b"]}),
        );
        let old = put(
            &ledger,
            "TalendJob",
            "j-old",
            json!({"job": "jLoad", "project": "P", "version": "0.0", "latest": false}),
        );
        let _ = old;
        let a = put(
            &ledger,
            "TalendComponent",
            "a",
            json!({"job": "jLoad", "job_version": "0.1", "unique_name": "tDBInput_1", "component_type": "tPostgresqlInput", "parameters": {"QUERY": "select 1"}}),
        );
        let b = put(
            &ledger,
            "TalendComponent",
            "b",
            json!({"job": "jLoad", "job_version": "0.1", "unique_name": "tDBOutput_1", "component_type": "tPostgresqlOutput"}),
        );
        rel(&ledger, "Contains", job, a, json!({}));
        rel(&ledger, "Contains", job, b, json!({}));
        rel(&ledger, "RowLink", a, b, json!({"label": "row1"}));
        let v = put(
            &ledger,
            "TalendContextVariable",
            "v",
            json!({"variable": "schema", "talend_type": "id_String", "default": "ODS"}),
        );
        let pw = put(
            &ledger,
            "TalendContextVariable",
            "pw",
            json!({"variable": "password", "secret": true, "default": null}),
        );
        rel(
            &ledger,
            "UsesContext",
            job,
            v,
            json!({"overridable_at_runtime": true}),
        );
        rel(
            &ledger,
            "UsesContext",
            job,
            pw,
            json!({"overridable_at_runtime": false}),
        );
        put(
            &ledger,
            "TalendColumnLineage",
            "l",
            json!({"job": "jLoad", "job_version": "0.1", "component": "tDBOutput_1", "description": "b.x from a.x"}),
        );
        let config = EkosConfig::default();

        let out = explain_json(&config, dir.path(), &ledger, &json!({"job": "jLoad"})).unwrap();
        assert_eq!(out["version"], "0.1");
        assert_eq!(out["components"].as_array().unwrap().len(), 2);
        assert!(
            out["components"][0].get("parameters").is_none(),
            "no configuration without a component"
        );
        assert_eq!(out["links"][0]["from"], "tDBInput_1");
        assert_eq!(out["links"][0]["to"], "tDBOutput_1");
        let ctx = out["context"].as_array().unwrap();
        assert!(ctx.iter().any(|c| c["variable"] == "schema"
            && c["default"] == "ODS"
            && c["overridable_at_runtime"] == true));
        assert!(
            ctx.iter()
                .any(|c| c["variable"] == "password" && c["default"] == "(secret, not stored)")
        );
        assert_eq!(out["lineage"][0]["description"], "b.x from a.x");

        let one = explain_json(
            &config,
            dir.path(),
            &ledger,
            &json!({"job": "jLoad", "component": "tDBInput_1"}),
        )
        .unwrap();
        assert_eq!(one["components"].as_array().unwrap().len(), 1);
        assert_eq!(one["components"][0]["parameters"]["QUERY"], "select 1");
        assert!(
            one["components"][0]["configuration"]
                .as_str()
                .unwrap()
                .contains("select 1")
        );
        assert!(one["lineage"].as_array().unwrap().is_empty());

        assert!(explain_json(&config, dir.path(), &ledger, &json!({"job": "nope"})).is_err());
        assert!(
            explain_json(
                &config,
                dir.path(),
                &ledger,
                &json!({"job": "jLoad", "component": "tX"})
            )
            .is_err()
        );
    }

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
