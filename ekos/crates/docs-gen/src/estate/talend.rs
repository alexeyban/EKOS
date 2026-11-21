//! RFC 0265 — the estate's `talend/` pages and the migration assessment report, from the
//! `Talend*` objects the ledger holds. Pure.
//!
//! Expressions, SQL, messages and descriptions are source text: they pass [`Model::text`]'s
//! licence gate. Names, counts and structure are always shown.

use super::{EstatePage, Model, cell, code, custom, link, page, prop, slug, strings, table};
use ekos_kir::{KirId, KirObject, KirRelationship, RelationshipKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const KINDS: &[&str] = &[
    "TalendProject",
    "TalendJob",
    "TalendComponent",
    "TalendContextVariable",
    "TalendColumnLineage",
    "TalendFinding",
];

/// The Talend objects of one project, by kind.
#[derive(Default)]
struct Proj<'a> {
    project: Option<&'a KirObject>,
    jobs: Vec<&'a KirObject>,
    components: Vec<&'a KirObject>,
    variables: Vec<&'a KirObject>,
    lineages: Vec<&'a KirObject>,
    findings: Vec<&'a KirObject>,
}

fn kind_is(o: &KirObject, k: &str) -> bool {
    custom(o) == Some(k)
}

fn rel_kind(r: &KirRelationship) -> String {
    match &r.kind {
        RelationshipKind::Custom(k) => k.clone(),
        other => format!("{other:?}"),
    }
}

fn num(o: &KirObject, path: &[&str]) -> Option<u64> {
    let mut v = o.properties.get(path[0])?;
    for k in &path[1..] {
        v = v.get(k)?;
    }
    v.as_u64()
}

fn projects<'a>(objects: &'a [KirObject]) -> BTreeMap<String, Proj<'a>> {
    let mut out: BTreeMap<String, Proj<'a>> = BTreeMap::new();
    for o in objects {
        let Some(k) = custom(o).filter(|k| KINDS.contains(k)) else {
            continue;
        };
        let p = out
            .entry(prop(o, "project").unwrap_or_default().to_string())
            .or_default();
        match k {
            "TalendProject" => p.project = Some(o),
            "TalendJob" if o.properties.get("latest") == Some(&Value::Bool(true)) => p.jobs.push(o),
            "TalendComponent" => p.components.push(o),
            "TalendContextVariable" => p.variables.push(o),
            "TalendColumnLineage" if o.properties.get("latest") == Some(&Value::Bool(true)) => {
                p.lineages.push(o);
            }
            "TalendFinding" => p.findings.push(o),
            _ => {}
        }
    }
    out.retain(|_, p| p.project.is_some());
    for p in out.values_mut() {
        p.jobs.sort_by_key(|o| o.name.clone());
        p.findings
            .sort_by_key(|o| (prop(o, "rule").map(str::to_string), o.name.clone()));
    }
    out
}

/// Whether the ledger holds a Talend project (the estate home then links `talend/`).
pub(super) fn present(objects: &[KirObject]) -> bool {
    objects.iter().any(|o| kind_is(o, "TalendProject"))
}

/// The `talend/` pages; returns the number of latest jobs documented.
pub(super) fn pages(m: &Model, out: &mut Vec<EstatePage>) -> usize {
    let projects = projects(m.input.objects);
    if projects.is_empty() {
        return 0;
    }
    let by_id: HashMap<KirId, &KirObject> = m.input.objects.iter().map(|o| (o.id, o)).collect();
    let rels = m.input.relationships;
    let mut rows = Vec::new();
    let mut jobs_total = 0;
    for (name, p) in &projects {
        let project = p.project.expect("retained");
        let dir = format!("talend/{}", slug(name));
        let engines: BTreeSet<&str> = p
            .components
            .iter()
            .filter_map(|c| prop(c, "engine"))
            .collect();
        rows.push(vec![
            link("talend/index.md", &format!("{dir}/index.md"), name),
            cell(prop(project, "product_version").unwrap_or("")),
            p.jobs.len().to_string(),
            p.components.len().to_string(),
            engines.into_iter().collect::<Vec<_>>().join(", "),
            p.findings.len().to_string(),
        ]);
        jobs_total += p.jobs.len();
        out.push(project_page(m, &dir, name, p, &by_id, rels));
        for job in &p.jobs {
            out.push(job_page(m, &dir, job, p, &by_id, rels));
        }
        out.push(contexts_page(m, &dir, name, p, &by_id, rels));
        let text = |o: &KirObject, t: &str| m.text(o, Some(t));
        out.push(page(
            format!("{dir}/assessment.md"),
            &format!("{name}: migration assessment"),
            90,
            assessment_markdown(project, &p.jobs, &p.findings, &text),
        ));
    }
    let body = format!(
        "Talend projects read from their items (RFC 0262–0265). Every page cites the job items it was \
         compiled from; nothing here comes from Talend Studio or a model call.\n\n{}",
        table(
            &[
                "project",
                "product",
                "jobs",
                "components",
                "engines",
                "findings"
            ],
            &rows
        )
    );
    out.push(page("talend/index.md".into(), "Talend", 60, body));
    jobs_total
}

fn job_slug(job: &KirObject) -> String {
    slug(prop(job, "job").unwrap_or(&job.name))
}

fn outgoing<'a>(
    rels: &'a [KirRelationship],
    from: KirId,
    kind: &'a str,
) -> impl Iterator<Item = &'a KirRelationship> + 'a {
    rels.iter()
        .filter(move |r| r.from == from && rel_kind(r) == kind)
}

fn project_page(
    m: &Model,
    dir: &str,
    name: &str,
    p: &Proj,
    by_id: &HashMap<KirId, &KirObject>,
    rels: &[KirRelationship],
) -> EstatePage {
    let path = format!("{dir}/index.md");
    let project = p.project.expect("retained");
    let mut b = String::new();
    let data: u64 = p
        .jobs
        .iter()
        .filter_map(|j| num(j, &["assessment", "data_components"]))
        .sum();
    let mapped: u64 = p
        .jobs
        .iter()
        .filter_map(|j| num(j, &["assessment", "mapped"]))
        .sum();
    let gaps = p
        .lineages
        .iter()
        .filter(|l| {
            l.properties
                .get("gaps")
                .and_then(Value::as_array)
                .is_some_and(|g| !g.is_empty())
        })
        .count();
    b.push_str(&format!(
        "{} job(s), {} component(s). Data components lowered to the Transformation IR: {mapped} of {data}. \
         Column lineages: {} ({gaps} with a gap). Findings to review: {} — see {}.\n\n",
        p.jobs.len(),
        p.components.len(),
        p.lineages.len(),
        p.findings.len(),
        link(&path, &format!("{dir}/assessment.md"), "the assessment"),
    ));
    if let Some(v) = prop(project, "product_version") {
        b.push_str(&format!("Written by {}.\n\n", code(v)));
    }
    // Orchestration: from each root (a job no job calls), the Calls tree.
    let job_ids: HashMap<KirId, &KirObject> = p.jobs.iter().map(|j| (j.id, *j)).collect();
    let mut children: HashMap<KirId, Vec<(String, KirId)>> = HashMap::new();
    let mut called: HashSet<KirId> = HashSet::new();
    for j in &p.jobs {
        for r in outgoing(rels, j.id, "Calls") {
            if let Some(child) = by_id.get(&r.to) {
                children
                    .entry(j.id)
                    .or_default()
                    .push((child.name.clone(), child.id));
                called.insert(r.to);
            }
        }
    }
    for v in children.values_mut() {
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v.dedup_by(|a, b| a.1 == b.1);
    }
    b.push_str("## Orchestration\n\nJobs run by `tRunJob`, from the jobs no job calls.\n\n");
    let mut tree = String::new();
    for root in p
        .jobs
        .iter()
        .filter(|j| !called.contains(&j.id) && children.contains_key(&j.id))
    {
        let mut stack = vec![(root.id, 0usize)];
        let mut seen = HashSet::new();
        while let Some((id, depth)) = stack.pop() {
            let Some(job) = job_ids.get(&id).or_else(|| by_id.get(&id)) else {
                continue;
            };
            let label = link(
                &path,
                &format!("{dir}/jobs/{}.md", job_slug(job)),
                &job.name,
            );
            tree.push_str(&format!("{}- {label}\n", "  ".repeat(depth)));
            if !seen.insert(id) {
                continue;
            }
            for (_, child) in children.get(&id).into_iter().flatten().rev() {
                stack.push((*child, depth + 1));
            }
        }
    }
    b.push_str(if tree.is_empty() {
        "_No job runs another._\n"
    } else {
        &tree
    });
    let alone: Vec<String> = p
        .jobs
        .iter()
        .filter(|j| !called.contains(&j.id) && !children.contains_key(&j.id))
        .map(|j| link(&path, &format!("{dir}/jobs/{}.md", job_slug(j)), &j.name))
        .collect();
    if !alone.is_empty() {
        b.push_str(&format!(
            "\nNot run by any job and running none: {}.\n",
            alone.join(", ")
        ));
    }
    b.push_str("\n## Jobs\n\n");
    let rows: Vec<Vec<String>> = p
        .jobs
        .iter()
        .map(|j| {
            let findings = p
                .findings
                .iter()
                .filter(|f| prop(f, "job") == prop(j, "job"))
                .count();
            vec![
                link(&path, &format!("{dir}/jobs/{}.md", job_slug(j)), &j.name),
                cell(prop(j, "folder").unwrap_or("")),
                cell(prop(j, "version").unwrap_or("")),
                strings(j, "reads").len().to_string(),
                strings(j, "writes").len().to_string(),
                format!(
                    "{} / {}",
                    num(j, &["assessment", "mapped"]).unwrap_or(0),
                    num(j, &["assessment", "data_components"]).unwrap_or(0)
                ),
                findings.to_string(),
            ]
        })
        .collect();
    b.push_str(&table(
        &[
            "job",
            "folder",
            "version",
            "tables read",
            "tables written",
            "mapped / data components",
            "findings",
        ],
        &rows,
    ));
    let _ = m;
    page(path, name, 10, b)
}

fn job_page(
    m: &Model,
    dir: &str,
    job: &KirObject,
    p: &Proj,
    by_id: &HashMap<KirId, &KirObject>,
    rels: &[KirRelationship],
) -> EstatePage {
    let name = prop(job, "job").unwrap_or(&job.name);
    let path = format!("{dir}/jobs/{}.md", job_slug(job));
    let mut b = String::new();
    if let Some(d) = m.text(job, prop(job, "author_description")) {
        b.push_str(&format!("> {}\n\n", d.replace('\n', " ")));
    }
    b.push_str(&format!(
        "Folder {}, version {}{}. Source: {}.\n\n",
        code(prop(job, "folder").unwrap_or("")),
        prop(job, "version").unwrap_or(""),
        if job.properties.get("implicit_context_load") == Some(&Value::Bool(true)) {
            "; context values can be replaced at run time (implicit context load)"
        } else {
            ""
        },
        code(prop(job, "source_path").unwrap_or("")),
    ));
    let list = |k: &str| {
        let v = strings(job, k);
        if v.is_empty() {
            "—".to_string()
        } else {
            v.iter().map(|t| code(t)).collect::<Vec<_>>().join(", ")
        }
    };
    b.push_str(&table(
        &["", ""],
        &[
            vec!["tables read".into(), list("reads")],
            vec!["tables written".into(), list("writes")],
            vec!["files read".into(), list("files_read")],
            vec!["files written".into(), list("files_written")],
        ],
    ));
    let calls: Vec<String> = outgoing(rels, job.id, "Calls")
        .filter_map(|r| by_id.get(&r.to))
        .map(|c| link(&path, &format!("{dir}/jobs/{}.md", job_slug(c)), &c.name))
        .collect();
    if !calls.is_empty() {
        b.push_str(&format!("\nRuns: {}.\n", calls.join(", ")));
    }
    // Context variables.
    let ctx: Vec<Vec<String>> = outgoing(rels, job.id, "UsesContext")
        .map(|r| {
            let var = r
                .properties
                .get("variable")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let runtime = r.properties.get("overridable_at_runtime") == Some(&Value::Bool(true));
            vec![
                code(var),
                by_id
                    .get(&r.to)
                    .and_then(|v| prop(v, "context"))
                    .map(cell)
                    .unwrap_or_default(),
                if runtime { "yes".into() } else { "no".into() },
            ]
        })
        .collect();
    b.push_str("\n## Context variables\n\n");
    b.push_str(&table(
        &["variable", "context", "can change at run time"],
        &ctx,
    ));
    // Maps.
    let comps: Vec<&&KirObject> = p
        .components
        .iter()
        .filter(|c| prop(c, "job") == Some(name) && prop(c, "job_version") == prop(job, "version"))
        .collect();
    let mut maps = String::new();
    for c in &comps {
        let unique = prop(c, "unique_name").unwrap_or(&c.name);
        let (kind, outputs) = if let Some(mp) = c.properties.get("mapper").filter(|v| !v.is_null())
        {
            ("tMap", mp.get("outputs"))
        } else if let Some(e) = c.properties.get("elt").filter(|v| !v.is_null()) {
            ("ELT map", e.get("outputs"))
        } else {
            continue;
        };
        maps.push_str(&format!("### {} ({kind})\n\n", code(unique)));
        for o in outputs.and_then(Value::as_array).into_iter().flatten() {
            let oname = o.get("name").and_then(Value::as_str).unwrap_or_default();
            let rows: Vec<Vec<String>> = o
                .get("entries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|e| {
                    let col = e.get("name").and_then(Value::as_str).unwrap_or_default();
                    let expr = e.get("expression").and_then(Value::as_str);
                    vec![
                        code(col),
                        m.text(c, expr)
                            .map(|t| code(&t))
                            .unwrap_or_else(|| "—".into()),
                    ]
                })
                .collect();
            maps.push_str(&format!("Output {}:\n\n", code(oname)));
            for f in [o.get("filter")]
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .chain(
                    o.get("filters")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                )
            {
                if let Some(t) = m.text(c, Some(f)) {
                    maps.push_str(&format!("Filter: {}\n\n", code(&t)));
                }
            }
            maps.push_str(&table(&["column", "← expression"], &rows));
            maps.push('\n');
        }
    }
    if !maps.is_empty() {
        b.push_str("\n## Maps\n\n");
        b.push_str(&maps);
    }
    // Lineage.
    let rows: Vec<Vec<String>> = p
        .lineages
        .iter()
        .filter(|l| prop(l, "job") == Some(name))
        .map(|l| {
            let t = l.properties.get("target");
            let target = format!(
                "{}.{}",
                t.and_then(|t| t.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                t.and_then(|t| t.get("column"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
            let origins: BTreeSet<String> = l
                .properties
                .get("origins")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(origin_label)
                .collect();
            let gaps = l
                .properties
                .get("gaps")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            vec![
                code(&target),
                cell(prop(l, "data_action").unwrap_or("")),
                m.text(l, prop(l, "formula"))
                    .map(|f| code(&f))
                    .unwrap_or_else(|| "—".into()),
                origins
                    .into_iter()
                    .map(|o| code(&o))
                    .collect::<Vec<_>>()
                    .join(", "),
                gaps.to_string(),
            ]
        })
        .collect();
    b.push_str("\n## Column lineage\n\n");
    b.push_str(&table(
        &["target", "action", "formula", "origins", "gaps"],
        &rows,
    ));
    let findings: Vec<String> = p
        .findings
        .iter()
        .filter(|f| prop(f, "job") == Some(name))
        .map(|f| {
            format!(
                "- **{}** ({}): {}\n",
                cell(prop(f, "rule").unwrap_or("")),
                cell(prop(f, "class").unwrap_or("")),
                m.text(f, prop(f, "message"))
                    .unwrap_or_else(|| "_text left out (licence)_".into())
            )
        })
        .collect();
    if !findings.is_empty() {
        b.push_str("\n## Findings to review\n\nHypotheses from the assessment, not defects until a person says so.\n\n");
        b.extend(findings);
    }
    page(path, name, 20, b)
}

fn origin_label(o: &Value) -> Option<String> {
    let column = o.get("column")?.as_str()?;
    let base = o
        .get("table")
        .or_else(|| o.get("file"))
        .or_else(|| o.get("job"))
        .and_then(Value::as_str)?;
    let base = base.rsplit(['/', '\\']).next().unwrap_or(base);
    Some(format!("{base}.{column}"))
}

fn contexts_page(
    m: &Model,
    dir: &str,
    name: &str,
    p: &Proj,
    by_id: &HashMap<KirId, &KirObject>,
    rels: &[KirRelationship],
) -> EstatePage {
    let mut users: HashMap<KirId, BTreeSet<String>> = HashMap::new();
    for r in rels.iter().filter(|r| rel_kind(r) == "UsesContext") {
        if let Some(j) = by_id.get(&r.from) {
            users.entry(r.to).or_default().insert(j.name.clone());
        }
    }
    let mut vars: Vec<&&KirObject> = p.variables.iter().collect();
    vars.sort_by_key(|v| {
        (
            prop(v, "context").map(str::to_string),
            prop(v, "group").map(str::to_string),
            prop(v, "variable").map(str::to_string),
        )
    });
    let rows: Vec<Vec<String>> = vars
        .iter()
        .map(|v| {
            let secret = v.properties.get("secret") == Some(&Value::Bool(true));
            let value = if secret {
                "_password: never stored_".to_string()
            } else {
                m.text(v, prop(v, "default"))
                    .map(|t| code(&t))
                    .unwrap_or_else(|| "—".into())
            };
            vec![
                cell(prop(v, "context").unwrap_or("")),
                cell(prop(v, "group").unwrap_or("")),
                code(prop(v, "variable").unwrap_or("")),
                cell(prop(v, "talend_type").unwrap_or("")),
                value,
                users
                    .get(&v.id)
                    .map(|u| u.iter().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default(),
            ]
        })
        .collect();
    let body = format!(
        "Repository context variables with their default values. A job with implicit context load, or \
         one that assigns or loads its context, may run with other values.\n\n{}",
        table(
            &["context", "group", "variable", "type", "default", "used by"],
            &rows
        )
    );
    page(
        format!("{dir}/contexts.md"),
        &format!("{name}: contexts"),
        80,
        body,
    )
}

/// The assessment report of one project, as `ekos talend assess` prints it and the estate's
/// `assessment.md` page shows it. `text` gates source text (a licence rule, or none).
pub fn assessment_markdown(
    project: &KirObject,
    jobs: &[&KirObject],
    findings: &[&KirObject],
    text: &dyn Fn(&KirObject, &str) -> Option<String>,
) -> String {
    let a = project.properties.get("assessment");
    let mut b = String::new();
    b.push_str(
        "Built from the compiled project, with no model call. Every finding is a hypothesis with its \
         evidence: a person decides whether it is a defect. There is no estimate in days: EKOS has \
         no measured basis for one.\n\n",
    );
    let by_class: BTreeMap<String, Vec<&&KirObject>> =
        findings.iter().fold(BTreeMap::new(), |mut acc, f| {
            acc.entry(prop(f, "class").unwrap_or("other").to_string())
                .or_default()
                .push(f);
            acc
        });
    b.push_str("## Findings\n\n");
    if findings.is_empty() {
        b.push_str("_None._\n");
    }
    for (class, fs) in &by_class {
        b.push_str(&format!(
            "### {} ({})\n\n",
            class.replace('_', " "),
            fs.len()
        ));
        for f in fs {
            let ev: Vec<String> = f
                .properties
                .get("evidence")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(6)
                .map(|e| {
                    let path = e.get("path").and_then(Value::as_str).unwrap_or_default();
                    match e.get("line").and_then(Value::as_u64) {
                        Some(l) => format!("{path}:{l}"),
                        None => path.to_string(),
                    }
                })
                .collect();
            b.push_str(&format!(
                "- **{}** `{}` — {} ({})\n",
                cell(prop(f, "rule").unwrap_or("")),
                cell(prop(f, "subject").unwrap_or("")),
                prop(f, "message")
                    .and_then(|m| text(f, m))
                    .unwrap_or_else(|| "_text left out (licence)_".into()),
                ev.join(", ")
            ));
        }
        b.push('\n');
    }
    b.push_str("## Clone clusters\n\nJobs whose component graphs match after names, tables and columns are removed: one dbt macro or generic model could cover each cluster.\n\n");
    let clusters: Vec<Vec<String>> = a
        .and_then(|a| a.get("clone_clusters"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| {
            let jobs: Vec<&str> = c
                .get("jobs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            vec![
                jobs.iter().map(|j| code(j)).collect::<Vec<_>>().join(", "),
                c.get("similarity")
                    .and_then(Value::as_f64)
                    .map_or(String::new(), |s| format!("{s:.2}")),
            ]
        })
        .collect();
    b.push_str(&table(&["jobs", "least similarity"], &clusters));
    b.push_str("\n## Migration order\n\nWaves over the table graph: a job comes after every job writing a table it reads.\n\n");
    let waves: Vec<Vec<String>> = a
        .and_then(|a| a.get("migration_order"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, w)| {
            vec![
                (i + 1).to_string(),
                w.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(code)
                    .collect::<Vec<_>>()
                    .join(", "),
            ]
        })
        .collect();
    b.push_str(&table(&["wave", "jobs"], &waves));
    if let Some(c) = a
        .and_then(|a| a.get("cycles"))
        .and_then(Value::as_array)
        .filter(|c| !c.is_empty())
    {
        let names: Vec<String> = c.iter().filter_map(Value::as_str).map(code).collect();
        b.push_str(&format!(
            "\nOn a cycle of the table graph, left out of the waves: {}.\n",
            names.join(", ")
        ));
    }
    b.push_str("\n## Per job\n\nData components lowered to the IR (a stand-in for the translation's fidelity classes, which do not exist yet), expressions by kind, state kept between runs, and what depends on the run.\n\n");
    let rows: Vec<Vec<String>> = jobs
        .iter()
        .map(|j| {
            let ja = j.properties.get("assessment");
            let get = |k: &str| ja.and_then(|a| a.get(k));
            let exprs = get("expressions")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| format!("{k} {}", v.as_u64().unwrap_or(0)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let state = get("state")
                .and_then(Value::as_array)
                .map(|s| {
                    s.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            let rt = get("runtime");
            let mut runtime = Vec::new();
            if let Some(n) = rt
                .and_then(|r| r.get("overridable_context"))
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty())
            {
                runtime.push(format!("{} context value(s) replaceable", n.len()));
            }
            if rt.and_then(|r| r.get("clock")) == Some(&Value::Bool(true)) {
                runtime.push("clock".into());
            }
            if rt.and_then(|r| r.get("iterates_files")) == Some(&Value::Bool(true)) {
                runtime.push("iterates files".into());
            }
            let idioms: BTreeSet<&str> = get("idioms")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|i| i.get("name").and_then(Value::as_str))
                .collect();
            vec![
                code(prop(j, "job").unwrap_or(&j.name)),
                format!(
                    "{} / {}",
                    get("mapped").and_then(Value::as_u64).unwrap_or(0),
                    get("data_components").and_then(Value::as_u64).unwrap_or(0)
                ),
                cell(&exprs),
                cell(&state),
                runtime.join(", "),
                idioms.into_iter().collect::<Vec<_>>().join(", "),
            ]
        })
        .collect();
    b.push_str(&table(
        &[
            "job",
            "mapped / data",
            "expressions",
            "state",
            "run time",
            "idioms",
        ],
        &rows,
    ));
    b.push_str("\n## Target sketch\n\nWhat a translation would produce, counted before any SQL is written.\n\n");
    let sketch: Vec<Vec<String>> = a
        .and_then(|a| a.get("target_sketch"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(k, v)| vec![k.replace('_', " "), v.to_string()])
        .collect();
    b.push_str(&table(&["", "count"], &sketch));
    if let Some(n) = a
        .and_then(|a| a.get("superseded_versions"))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
    {
        b.push_str(&format!(
            "\n{n} older job version(s) are kept beside the latest and were not assessed.\n"
        ));
    }
    b
}

#[cfg(test)]
mod tests {
    use super::super::{EstateInput, PassportInput, Provenance, build};
    use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    fn obj(kind: &str, name: &str, props: Value) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Custom(kind.into()));
        o.id = KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("{kind}:{name}").as_bytes(),
        ));
        for (k, v) in props.as_object().unwrap() {
            o.properties.insert(k.clone(), v.clone());
        }
        o.properties.insert("project".into(), json!("P"));
        o.properties.insert("source".into(), json!("etl"));
        o
    }

    fn objects() -> (Vec<KirObject>, Vec<KirRelationship>) {
        let project = obj(
            "TalendProject",
            "P",
            json!({"product_version": "TOS 8", "assessment": {
            "clone_clusters": [{"jobs": ["jA", "jB"], "similarity": 0.9}],
            "migration_order": [["jA"], ["jMain"]], "target_sketch": {"models": 1}}}),
        );
        let main = obj(
            "TalendJob",
            "jMain",
            json!({"job": "jMain", "version": "0.1", "latest": true, "folder": "ORCH"}),
        );
        let job = obj(
            "TalendJob",
            "jA",
            json!({"job": "jA", "version": "0.1", "latest": true,
            "folder": "ODS", "reads": ["SRC"], "writes": ["FACT"], "author_description": "Loads FACT",
            "assessment": {"mapped": 2, "data_components": 2, "expressions": {"column": 1}}}),
        );
        let map = obj(
            "TalendComponent",
            "jA.tMap_1",
            json!({"job": "jA", "job_version": "0.1",
            "unique_name": "tMap_1", "mapper": {"outputs": [{"name": "out",
            "entries": [{"name": "TOTAL", "expression": "row1.QTY * row1.PRICE"}]}]}}),
        );
        let lineage = obj(
            "TalendColumnLineage",
            "jA.FACT.TOTAL",
            json!({"job": "jA", "latest": true,
            "target": {"name": "FACT", "column": "TOTAL"}, "formula": "SRC.QTY * SRC.PRICE",
            "origins": [{"kind": "table", "table": "SRC", "column": "QTY"}], "gaps": []}),
        );
        let finding = obj(
            "TalendFinding",
            "uncalled_job: jA",
            json!({"rule": "uncalled_job",
            "class": "dead_item", "subject": "jA", "job": "jA", "message": "`jA` is not called",
            "evidence": [{"path": "P/process/jA_0.1.item", "line": null}]}),
        );
        let calls =
            KirRelationship::deterministic(RelationshipKind::Calls, main.id, job.id, "tRunJob_1");
        (vec![project, main, job, map, lineage, finding], vec![calls])
    }

    fn pages(
        licence: &str,
        objects: &[KirObject],
        rels: &[KirRelationship],
    ) -> Vec<super::EstatePage> {
        let licences: BTreeMap<String, String> = [("etl".to_string(), licence.to_string())].into();
        build(&EstateInput {
            objects,
            relationships: rels,
            scope: &BTreeMap::new(),
            subjects: &[],
            licences: &licences,
            provenance: &Provenance::default(),
            current_conflicts: None,
            max_er_tables: 30,
            integrity: None,
            passport: &PassportInput::default(),
        })
    }

    fn body<'a>(pages: &'a [super::EstatePage], path: &str) -> &'a str {
        &pages
            .iter()
            .find(|p| p.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .body
    }

    #[test]
    fn a_talend_project_gets_its_pages_with_text_under_a_known_licence() {
        let (o, r) = objects();
        let p = pages("MIT", &o, &r);
        let project = body(&p, "talend/p/index.md");
        assert!(project.contains("- [jMain]"), "{project}");
        assert!(project.contains("  - [jA]"), "the call tree: {project}");
        let job = body(&p, "talend/p/jobs/ja.md");
        assert!(job.contains("> Loads FACT"));
        assert!(job.contains("`row1.QTY * row1.PRICE`"));
        assert!(job.contains("`SRC.QTY * SRC.PRICE`"));
        assert!(job.contains("`SRC.QTY`"));
        assert!(job.contains("**uncalled_job**"));
        let assessment = body(&p, "talend/p/assessment.md");
        assert!(assessment.contains("`jA`, `jB`"));
        assert!(assessment.contains("| 2 | `jMain` |"));
        assert!(body(&p, "index.md").contains("| Talend jobs | 2 |"));
        assert!(body(&p, "index.md").contains("talend/index.md"));
    }

    #[test]
    fn without_a_licence_the_text_is_left_out_and_the_structure_stays() {
        let (o, r) = objects();
        let p = pages("unknown", &o, &r);
        let job = body(&p, "talend/p/jobs/ja.md");
        assert!(!job.contains("row1.QTY"));
        assert!(!job.contains("Loads FACT"));
        assert!(job.contains("`TOTAL`") && job.contains("`FACT`"));
        assert!(job.contains("_text left out (licence)_"));
    }

    #[test]
    fn a_workspace_without_a_talend_project_has_no_talend_pages() {
        let p = pages("MIT", &[], &[]);
        assert!(!p.iter().any(|x| x.path.starts_with("talend/")));
        assert!(body(&p, "index.md").contains("| Talend jobs | 0 |"));
    }
}
