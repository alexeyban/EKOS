//! `TalendAnalyzerPass` (RFC 0264): `talend` observation artifacts → `ekos_talend::Project` →
//! `Talend*` objects, their relationships, and one Transformation IR graph per job.
//!
//! Pure and deterministic: ids are UUID v5 of `talend:<kind>:<project root>:<key>`, so a re-read
//! keeps every id, and `tMap_1` in two jobs is two objects. No model calls. No secret value: the
//! model never keeps one (RFC 0262 §Secrets), and the artifacts were redacted at build.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_semantic::merge_graphs;
use ekos_semantic::transform_ir::lower_to_kir;
use ekos_talend::assess::{Evidence, Finding, JobView, assess};
use ekos_talend::java::context_refs;
use ekos_talend::lineage::{ColumnLineage, job_lineage};
use ekos_talend::{
    Component, Diagnostic, Job, Param, Project, ProjectFile, RuntimeContext, read_project,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::talend_ir::lower_job;
use crate::talend_lineage::{JobLineages, SqlQueries, link_jobs};

/// Every object kind this pass writes besides `TransformNode`. Built from strings, which
/// `every_pipeline_custom_kind_is_registered` cannot see: `every_talend_kind_has_a_keyed_registry_row`
/// checks these instead, and `every_emitted_kind_is_listed` keeps this list complete.
pub const KINDS: &[&str] = &[
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

/// What one run read, for `ekos recover`'s summary line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TalendStats {
    pub projects: usize,
    pub jobs: usize,
    pub components: usize,
    pub data_components: usize,
    pub mapped_components: usize,
    pub queries: usize,
    pub queries_folded: usize,
    pub diagnostics: usize,
    /// Joblet versions read, and uses inlined into jobs (nested ones included).
    pub joblets: usize,
    pub joblet_uses: usize,
    /// Target columns with a lineage, and how many of them stop at a gap.
    pub lineages: usize,
    pub lineage_gaps: usize,
    /// Lineage links between jobs through a table.
    pub column_feeds: usize,
    /// RFC 0265 assessment findings.
    pub findings: usize,
}

impl TalendStats {
    pub fn mapped_percent(&self) -> f32 {
        if self.data_components == 0 {
            0.0
        } else {
            100.0 * self.mapped_components as f32 / self.data_components as f32
        }
    }

    fn add(&mut self, other: &Self) {
        self.projects += other.projects;
        self.jobs += other.jobs;
        self.components += other.components;
        self.data_components += other.data_components;
        self.mapped_components += other.mapped_components;
        self.queries += other.queries;
        self.queries_folded += other.queries_folded;
        self.diagnostics += other.diagnostics;
        self.joblets += other.joblets;
        self.findings += other.findings;
        self.joblet_uses += other.joblet_uses;
        self.lineages += other.lineages;
        self.lineage_gaps += other.lineage_gaps;
        self.column_feeds += other.column_feeds;
    }
}

pub struct TalendAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<TalendStats>>,
}

impl TalendAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("talend-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(TalendStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<TalendStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for TalendAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn version(&self) -> &str {
        // v2: joblets and column lineage (RFC 0264 Phase 2).
        // v3: lineage through `tBufferOutput` → `tRunJob` and through files between jobs.
        // v4: ELT maps (RFC 0264 §ELT): IR, lineage, reads and writes.
        // v5: the migration assessment (RFC 0265): `TalendFinding`, `assessment` summaries.
        // v6: a component-framework input's manual query is its `QUERY` (Snowflake).
        "v6"
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(ToString::to_string).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut projects: BTreeMap<String, Vec<ProjectFile>> = BTreeMap::new();
        for id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("TALEND100", format!("cannot read artifact {id}: {e}"));
                    continue;
                }
            };
            let data = &json["data"];
            let (Some(root), Some(path)) =
                (data["project_root"].as_str(), data["project_path"].as_str())
            else {
                continue;
            };
            projects
                .entry(root.to_string())
                .or_default()
                .push(ProjectFile {
                    path: path.to_string(),
                    text: data["text"].as_str().unwrap_or_default().to_string(),
                });
        }
        let mut combined = KirGraph::new();
        let mut stats = TalendStats::default();
        for (root, files) in &projects {
            let project = read_project(root, files);
            for d in &project.diagnostics {
                ctx.diagnostics.lock().unwrap().warning(
                    &d.code,
                    format!("{}: {}", workspace_path(root, &d.path), d.message),
                );
            }
            let (graph, project_stats, expansion) = project_kir(&project);
            for d in &expansion {
                ctx.diagnostics.lock().unwrap().warning(
                    &d.code,
                    format!("{}: {}", workspace_path(root, &d.path), d.message),
                );
            }
            stats.add(&project_stats);
            merge_graphs(&mut combined, graph);
        }
        *self.stats.lock().unwrap() = stats;
        if combined.objects.is_empty() {
            return Ok(());
        }
        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], combined)
            .superseding_earlier_runs();
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;
        Ok(())
    }
}

/// A project-relative path as the workspace sees it.
fn workspace_path(root: &str, path: &str) -> String {
    if root.is_empty() {
        path.to_string()
    } else {
        format!("{root}/{path}")
    }
}

/// Builds KIR for one project.
struct Emit<'a> {
    project: &'a Project,
    kir: KirGraph,
}

impl Emit<'_> {
    fn id(&self, kind: &str, key: &str) -> KirId {
        KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("talend:{kind}:{}:{key}", self.project.root).as_bytes(),
        ))
    }

    fn object(
        &mut self,
        kind: &str,
        key: &str,
        name: &str,
        props: Value,
        at: (&str, Option<u32>),
    ) -> KirId {
        let id = self.id(kind, key);
        let path = workspace_path(&self.project.root, at.0);
        let location = match at.1 {
            Some(line) => SourceLocation::at(path.clone(), line),
            None => SourceLocation::file(path.clone()),
        };
        let mut evidence = KirEvidence::new(location, format!("{kind} {name} in {path}"));
        evidence.id = self.id("evidence", &format!("{kind}:{key}"));
        let evidence_id = self.kir.add_evidence(evidence);
        let mut obj = KirObject::new(name, ObjectKind::Custom(kind.to_string()));
        obj.id = id;
        if let Value::Object(map) = props {
            obj.properties = map.into_iter().filter(|(_, v)| !v.is_null()).collect();
        }
        obj.properties
            .insert("project".into(), json!(self.project.technical_label));
        obj.properties.insert("source_path".into(), json!(path));
        obj.evidence.push(evidence_id);
        self.kir.add_object(obj);
        id
    }

    fn relate(&mut self, kind: &str, from: KirId, to: KirId, discriminator: &str, props: Value) {
        let kind = match kind {
            "Contains" => RelationshipKind::Contains,
            "Calls" => RelationshipKind::Calls,
            other => RelationshipKind::Custom(other.to_string()),
        };
        let mut rel = KirRelationship::deterministic(kind, from, to, discriminator);
        if let Value::Object(map) = props {
            rel.properties = map.into_iter().filter(|(_, v)| !v.is_null()).collect();
        }
        if let Some(ev) = self
            .kir
            .objects
            .iter()
            .find(|o| o.id == from)
            .and_then(|o| o.evidence.first())
        {
            rel.evidence.push(*ev);
        }
        self.kir.add_relationship(rel);
    }
}

fn job_key(job: &Job) -> String {
    format!("{}@{}", job.name, job.version)
}

/// The key scope of a job's components (`<job>@<version>`) or a joblet's
/// (`joblet:<joblet>@<version>`).
fn scope(job: &Job, joblet: bool) -> String {
    if joblet {
        format!("joblet:{}", job_key(job))
    } else {
        job_key(job)
    }
}

fn component_key(scope: &str, c: &Component) -> String {
    format!("{scope}:{}", c.unique_name)
}

/// Every object, relationship and IR graph for one project, and the diagnostics of joblet
/// expansion (`TALEND005`/`TALEND006`).
pub fn project_kir(project: &Project) -> (KirGraph, TalendStats, Vec<Diagnostic>) {
    let mut e = Emit {
        project,
        kir: KirGraph::new(),
    };
    let mut stats = TalendStats {
        projects: 1,
        diagnostics: project.diagnostics.len(),
        ..TalendStats::default()
    };
    let project_id = e.object(
        "TalendProject",
        "",
        if project.technical_label.is_empty() {
            &project.label
        } else {
            &project.technical_label
        },
        json!({
            "label": project.label,
            "product_version": project.product_version,
            "jobs": project.jobs.len(),
            "not_read": project.not_read,
            "diagnostics": project.diagnostics,
        }),
        ("talend.project", None),
    );
    emit_repository(&mut e, project_id);
    for joblet in &project.joblets {
        emit_joblet(&mut e, project_id, joblet, &mut stats);
    }
    let mut diagnostics = Vec::new();
    let mut lineages: Vec<(&Job, Job, Vec<ColumnLineage>)> = Vec::new();
    for job in &project.jobs {
        let expanded = project.expand(job);
        stats.joblet_uses += expanded.inlined;
        diagnostics.extend(expanded.diagnostics);
        emit_job(&mut e, project_id, job, &expanded.job, &mut stats);
        let found = job_lineage(&expanded.job, &SqlQueries);
        lineages.push((job, expanded.job, found));
    }
    emit_lineages(&mut e, &lineages, &mut stats);
    emit_assessment(&mut e, project_id, &lineages, &mut stats);
    stats.diagnostics += diagnostics.len();
    (e.kir, stats, diagnostics)
}

/// RFC 0265: the migration assessment, from the same expanded jobs and lineage. Findings become
/// keyed `TalendFinding` objects (`About` their job); summaries land on `TalendJob` and
/// `TalendProject` as `assessment`.
fn emit_assessment(
    e: &mut Emit,
    project_id: KirId,
    lineages: &[(&Job, Job, Vec<ColumnLineage>)],
    stats: &mut TalendStats,
) {
    let io: Vec<(crate::talend_ir::DataIo, usize, usize)> = lineages
        .iter()
        .map(|(_, expanded, _)| {
            let lowered = lower_job(expanded);
            (
                crate::talend_ir::data_io(expanded),
                lowered.mapped_components,
                lowered.data_components,
            )
        })
        .collect();
    let views: Vec<JobView> = lineages
        .iter()
        .zip(&io)
        .map(|((job, expanded, lineage), (io, mapped, data))| JobView {
            job,
            expanded,
            lineage,
            mapped: *mapped,
            data_components: *data,
            reads: &io.reads,
            writes: &io.writes,
            files_read: &io.files_read,
        })
        .collect();
    let mut a = assess(e.project, &views);
    a.findings.extend(unused_items(e, project_id));
    for f in &a.findings {
        let at = f.evidence.first();
        let path = at
            .map_or("talend.project", |ev| ev.path.as_str())
            .to_string();
        let line = at.and_then(|ev| ev.line);
        let evidence: Vec<Value> = f
            .evidence
            .iter()
            .map(|ev| json!({ "path": workspace_path(&e.project.root, &ev.path), "line": ev.line }))
            .collect();
        let props = json!({
            "rule": f.rule,
            "class": f.class,
            "subject": f.subject,
            "job": f.job,
            "message": f.message,
            "evidence": evidence,
            "status": "hypothesis",
        });
        let key = format!("{}:{}", f.rule, f.subject);
        // Named `rule: subject`, never the subject alone: a finding named like its job or
        // component made identity report the pair as one name in two kinds.
        let name = format!("{}: {}", f.rule, f.subject);
        let id = e.object("TalendFinding", &key, &name, props, (&path, line));
        let about = f
            .job
            .as_ref()
            .and_then(|name| e.project.jobs.iter().find(|j| j.latest && &j.name == name))
            .map_or(project_id, |j| e.id("TalendJob", &job_key(j)));
        e.relate("About", id, about, "", Value::Null);
        stats.findings += 1;
    }
    for (key, ja) in &a.jobs {
        let id = e.id("TalendJob", key);
        if let Some(o) = e.kir.objects.iter_mut().find(|o| o.id == id) {
            o.properties.insert("assessment".into(), json!(ja));
        }
    }
    if let Some(o) = e.kir.objects.iter_mut().find(|o| o.id == project_id) {
        o.properties.insert("assessment".into(), json!(a.project));
    }
}

/// Contexts, connections and custom routines nothing uses (RFC 0265 `unused_item`), from the
/// `Uses*` relationships the pass already emitted.
fn unused_items(e: &Emit, project_id: KirId) -> Vec<Finding> {
    let used: std::collections::HashSet<KirId> = e
        .kir
        .relationships
        .iter()
        .filter(|r| {
            matches!(&r.kind, RelationshipKind::Custom(k)
                if matches!(k.as_str(), "UsesContext" | "UsesConnection" | "UsesRoutine"))
        })
        .map(|r| r.to)
        .collect();
    let contains: Vec<(KirId, KirId)> = e
        .kir
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Contains)
        .map(|r| (r.from, r.to))
        .collect();
    let mut out = Vec::new();
    for o in &e.kir.objects {
        let kind = match &o.kind {
            ObjectKind::Custom(k) => k.as_str(),
            _ => continue,
        };
        let what = match kind {
            "TalendContext" => "context",
            "TalendDbConnection" => "database connection",
            "TalendRoutine" => "routine",
            _ => continue,
        };
        let in_use = used.contains(&o.id)
            || (kind == "TalendContext"
                && contains
                    .iter()
                    .any(|(from, to)| *from == o.id && used.contains(to)));
        if in_use || !contains.iter().any(|(f, t)| *f == project_id && *t == o.id) {
            continue;
        }
        let path = o
            .properties
            .get("source_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let rel = path
            .strip_prefix(&format!("{}/", e.project.root))
            .unwrap_or(path)
            .to_string();
        out.push(Finding {
            rule: "unused_item".into(),
            class: "dead_item".into(),
            subject: format!("{kind}:{}", o.name),
            job: None,
            message: format!("the {what} `{}` is not used by any job", o.name),
            evidence: vec![Evidence {
                path: rel,
                line: None,
            }],
        });
    }
    out
}

fn emit_repository(e: &mut Emit, project_id: KirId) {
    let project = e.project;
    for item in &project.contexts {
        let key = format!("{}@{}", item.name, item.version);
        let props =
            json!({ "version": item.version, "repository_id": item.id, "groups": item.groups });
        let id = e.object("TalendContext", &key, &item.name, props, (&item.path, None));
        e.relate("Contains", project_id, id, "", Value::Null);
        for group in &item.groups {
            for var in &group.variables {
                let var_key = format!("{key}:{}:{}", group.name, var.name);
                let props = json!({
                    "context": item.name,
                    "group": group.name,
                    "variable": var.name,
                    "talend_type": var.talend_type,
                    "default": var.value,
                    "secret": var.secret,
                });
                let name = format!("{}.{}", item.name, var.name);
                let vid = e.object(
                    "TalendContextVariable",
                    &var_key,
                    &name,
                    props,
                    (&item.path, None),
                );
                e.relate("Contains", id, vid, "", Value::Null);
            }
        }
    }
    for c in &project.connections {
        let key = format!("{}@{}", c.name, c.version);
        let props = serde_json::to_value(c).unwrap_or(Value::Null);
        let id = e.object("TalendDbConnection", &key, &c.name, props, (&c.path, None));
        e.relate("Contains", project_id, id, "", Value::Null);
    }
    for s in &project.schemas {
        let props = json!({ "family": s.family, "version": s.version, "repository_id": s.id, "tables": s.tables });
        let id = e.object("TalendSchema", &s.path, &s.name, props, (&s.path, None));
        e.relate("Contains", project_id, id, "", Value::Null);
    }
    for r in project.routines.iter().filter(|r| !r.system) {
        let key = format!("{}@{}", r.name, r.version);
        let props = json!({ "version": r.version, "methods": r.methods });
        let id = e.object("TalendRoutine", &key, &r.name, props, (&r.path, None));
        e.relate("Contains", project_id, id, "", Value::Null);
    }
}

fn emit_joblet(e: &mut Emit, project_id: KirId, joblet: &Job, stats: &mut TalendStats) {
    let ports: Vec<&str> = joblet
        .components
        .iter()
        .filter(|c| c.logical.starts_with("joblet_"))
        .map(|c| c.unique_name.as_str())
        .collect();
    let props = json!({
        "joblet": joblet.name,
        "version": joblet.version,
        "latest": joblet.latest,
        "repository_id": joblet.id,
        "folder": joblet.folder,
        "purpose": joblet.purpose,
        "description": joblet.description,
        "components": joblet.components.len(),
        "ports": ports,
    });
    let id = e.object(
        "TalendJoblet",
        &job_key(joblet),
        &joblet.name,
        props,
        (&joblet.path, None),
    );
    e.relate("Contains", project_id, id, "", Value::Null);
    stats.joblets += 1;
    let scope = scope(joblet, true);
    for c in &joblet.components {
        let cid = emit_component(e, joblet, &scope, c);
        e.relate("Contains", id, cid, "", Value::Null);
    }
    emit_flow(e, joblet, &scope);
}

/// The searchable sentence of a job: what its author wrote, then what it reads, writes and runs
/// (the author's text can be wrong — fixture B's `jDwhProduit` says it loads `DIM_CLIENT`).
fn job_description(job: &Job, io: &crate::talend_ir::DataIo, called: &[&str]) -> String {
    let mut text = String::new();
    if let Some(d) = job.description.as_deref().filter(|d| !d.trim().is_empty()) {
        text.push_str(d.trim());
        text.push_str(". ");
    }
    text.push_str(&format!(
        "Talend job {} ({} {}).",
        job.name, job.folder, job.version
    ));
    for (what, list) in [
        ("Reads tables", &io.reads),
        ("Writes tables", &io.writes),
        ("Reads files", &io.files_read),
        ("Writes files", &io.files_written),
    ] {
        if !list.is_empty() {
            text.push_str(&format!(" {what} {}.", list.join(", ")));
        }
    }
    if !called.is_empty() {
        text.push_str(&format!(" Runs jobs {}.", called.join(", ")));
    }
    text
}

fn emit_job(e: &mut Emit, project_id: KirId, job: &Job, expanded: &Job, stats: &mut TalendStats) {
    let project = e.project;
    let calls = project.calls(job);
    let unresolved: Vec<&str> = calls
        .iter()
        .filter(|c| c.resolved.is_none())
        .map(|c| c.child_name.as_str())
        .collect();
    let io = crate::talend_ir::data_io(expanded);
    let called: Vec<&str> = calls.iter().map(|c| c.child_name.as_str()).collect();
    let props = json!({
        "job": job.name,
        "version": job.version,
        "latest": job.latest,
        "repository_id": job.id,
        "folder": job.folder,
        "job_type": job.job_type,
        "purpose": job.purpose,
        "author_description": job.description,
        "description": job_description(job, &io, &called),
        "reads": io.reads,
        "writes": io.writes,
        "files_read": io.files_read,
        "files_written": io.files_written,
        "created": job.created,
        "modified": job.modified,
        "default_context": job.default_context,
        "implicit_context_load": job.implicit_context_load,
        "components": job.components.len(),
        "subjobs": job.subjob_count,
        "unresolved_calls": unresolved,
    });
    let job_id = e.object(
        "TalendJob",
        &job_key(job),
        &job.name,
        props,
        (&job.path, None),
    );
    e.relate("Contains", project_id, job_id, "", Value::Null);
    stats.jobs += 1;
    let scope = scope(job, false);
    for c in &job.components {
        count_component(job, c, stats);
        let cid = emit_component(e, job, &scope, c);
        e.relate("Contains", job_id, cid, "", Value::Null);
    }
    emit_flow(e, job, &scope);
    if let Some(conn) = &job.implicit_context_connection
        && let Some(target) = project
            .connections
            .iter()
            .find(|x| x.id.as_deref() == Some(conn.as_str()))
    {
        let to = e.id(
            "TalendDbConnection",
            &format!("{}@{}", target.name, target.version),
        );
        e.relate(
            "UsesConnection",
            job_id,
            to,
            "implicit_context",
            json!({ "for": "implicit context load" }),
        );
    }
    for call in &calls {
        let Some(target) = &call.resolved else {
            continue;
        };
        let to = e.id("TalendJob", &format!("{}@{}", target.name, target.version));
        let props = json!({
            "component": call.component,
            "requested_version": call.requested_version,
            "via_latest": target.via_latest,
            "context": call.context,
            "transmit_whole_context": call.transmit_whole_context,
            "die_on_child_error": call.die_on_child_error,
            "dynamic": call.dynamic,
        });
        e.relate("Calls", job_id, to, &call.component, props);
    }
    emit_uses(e, job, job_id);
    emit_ir(e, job, expanded, stats);
}

fn count_component(job: &Job, c: &Component, stats: &mut TalendStats) {
    stats.components += 1;
    if c.logical == "db_input" && c.params.contains_key("QUERY") {
        stats.queries += 1;
        if let Some(q) = c.params["QUERY"].text()
            && job.fold(q).complete()
        {
            stats.queries_folded += 1;
        }
    }
}

fn emit_component(e: &mut Emit, job: &Job, scope: &str, c: &Component) -> KirId {
    let props = json!({
        "job": job.name,
        "job_version": job.version,
        "unique_name": c.unique_name,
        "component_type": c.component_type,
        "type_version": c.type_version,
        "category": c.category.as_str(),
        "logical": c.logical,
        "engine": c.engine,
        "label": c.label,
        "line": c.line,
        "parameters": c.params,
        "schemas": c.schemas,
        "mapper": c.mapper,
        "elt": c.elt,
        "in_joblet": scope.starts_with("joblet:"),
    });
    let name = format!("{}.{}", job.name, c.unique_name);
    let id = e.object(
        "TalendComponent",
        &component_key(scope, c),
        &name,
        props,
        (&job.path, Some(c.line)),
    );
    if let Some(conn) = repository_connection(job, c)
        && let Some(target) = e
            .project
            .connections
            .iter()
            .find(|x| x.id.as_deref() == Some(conn))
    {
        let to = e.id(
            "TalendDbConnection",
            &format!("{}@{}", target.name, target.version),
        );
        e.relate("UsesConnection", id, to, "", Value::Null);
    }
    if let Some((joblet, via_latest)) = e.project.joblet_for(c) {
        let to = e.id("TalendJoblet", &job_key(joblet));
        e.relate(
            "UsesJoblet",
            id,
            to,
            "",
            json!({ "version": joblet.version, "via_latest": via_latest }),
        );
    }
    id
}

/// The repository connection id a component uses: its own, or that of the connection component
/// named by `CONNECTION`.
fn repository_connection<'a>(job: &'a Job, c: &'a Component) -> Option<&'a str> {
    let own = |c: &'a Component| {
        let repo =
            c.params.get("PROPERTY:PROPERTY_TYPE").and_then(Param::text) == Some("REPOSITORY");
        repo.then(|| {
            c.params
                .get("PROPERTY:REPOSITORY_PROPERTY_TYPE")
                .and_then(Param::text)
        })
        .flatten()
    };
    own(c).or_else(|| {
        let name = c.params.get("CONNECTION").and_then(Param::text)?;
        own(job.component(name)?)
    })
}

fn emit_flow(e: &mut Emit, job: &Job, scope: &str) {
    let cid = |e: &Emit, name: &str| {
        let c = job.component(name)?;
        Some(e.id("TalendComponent", &component_key(scope, c)))
    };
    for link in &job.links {
        let (Some(from), Some(to)) = (cid(e, &link.from), cid(e, &link.to)) else {
            continue;
        };
        let kind = serde_json::to_value(&link.kind).unwrap_or(Value::Null);
        let props = json!({ "link_kind": kind, "label": link.label, "metaname": link.metaname, "line": link.line });
        let disc = format!(
            "{}:{}",
            link.label.as_deref().unwrap_or_default(),
            link.line
        );
        e.relate("RowLink", from, to, &disc, props);
    }
    for t in &job.triggers {
        let (Some(from), Some(to)) = (cid(e, &t.from), cid(e, &t.to)) else {
            continue;
        };
        let kind = serde_json::to_value(&t.kind).unwrap_or(Value::Null);
        let props = json!({ "trigger_kind": kind, "condition": t.condition, "line": t.line });
        e.relate("Triggers", from, to, &t.line.to_string(), props);
    }
}

/// Every text a job's behaviour depends on: parameter values and table cells, tMap expressions
/// and filters, trigger conditions.
fn emit_uses(e: &mut Emit, job: &Job, job_id: KirId) {
    let texts = job.texts();
    let runtime = RuntimeContext::of(job);
    let used: BTreeSet<String> = texts.iter().flat_map(|t| context_refs(t)).collect();
    let mut unresolved: Vec<String> = Vec::new();
    for name in &used {
        match context_variable_key(e.project, job, name) {
            Some(key) => {
                let to = e.id("TalendContextVariable", &key);
                let props = json!({ "variable": name, "overridable_at_runtime": runtime.overridable(name) });
                e.relate("UsesContext", job_id, to, name, props);
            }
            None if job.context_var(name).is_none() => unresolved.push(name.clone()),
            None => {}
        }
    }
    if !unresolved.is_empty()
        && let Some(obj) = e.kir.objects.iter_mut().find(|o| o.id == job_id)
    {
        obj.properties
            .insert("unresolved_context".into(), json!(unresolved));
    }
    for r in e.project.routines.iter().filter(|r| !r.system) {
        if texts.iter().any(|t| calls_routine(t, &r.name)) {
            let to = e.id("TalendRoutine", &format!("{}@{}", r.name, r.version));
            e.relate("UsesRoutine", job_id, to, "", Value::Null);
        }
    }
}

/// The `TalendContextVariable` key of a job's context variable that comes from a repository item.
fn context_variable_key(project: &Project, job: &Job, name: &str) -> Option<String> {
    let var = job.context_var(name)?;
    let repo = var.repository_context_id.as_deref()?;
    let item = project
        .contexts
        .iter()
        .find(|c| c.id.as_deref() == Some(repo))?;
    let group = item
        .groups
        .iter()
        .find(|g| g.name == job.default_context && g.variables.iter().any(|v| v.name == name))
        .or_else(|| {
            item.groups
                .iter()
                .find(|g| g.variables.iter().any(|v| v.name == name))
        })?;
    Some(format!(
        "{}@{}:{}:{}",
        item.name, item.version, group.name, name
    ))
}

/// `routine.method(` at a word boundary, or written in full as `routines.routine.method(`.
fn calls_routine(text: &str, routine: &str) -> bool {
    let needle = format!("{routine}.");
    text.match_indices(&needle).any(|(at, _)| {
        let before = &text[..at];
        let unqualified = before.strip_suffix("routines.").unwrap_or(before);
        let before_ok = unqualified
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'));
        let rest = &text[at + needle.len()..];
        let name_end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        before_ok && name_end > 0 && rest[name_end..].trim_start().starts_with('(')
    })
}

fn emit_ir(e: &mut Emit, job: &Job, expanded: &Job, stats: &mut TalendStats) {
    let mut lowered = lower_job(expanded);
    stats.data_components += lowered.data_components;
    stats.mapped_components += lowered.mapped_components;
    if lowered.graph.nodes.is_empty() {
        return;
    }
    lowered.graph.origin.source_path = workspace_path(&e.project.root, &job.path);
    let mut kir = lower_to_kir(&lowered.graph);
    for (index, extra) in lowered.extra.into_iter().enumerate() {
        if let Some(obj) = kir.objects.get_mut(index) {
            obj.properties.extend(extra);
            obj.properties.insert("talend_job".into(), json!(job.name));
            obj.properties
                .insert("talend_job_version".into(), json!(job.version));
        }
    }
    merge_graphs(&mut e.kir, kir);
}

/// The ledger key of the `TalendComponent` a component of an expanded job stands for: the job's
/// own, or the joblet's it was inlined from.
fn origin_component_key(job: &Job, c: &Component) -> String {
    match &c.joblet {
        Some(o) => format!("joblet:{}@{}:{}", o.name, o.version, o.component),
        None => component_key(&job_key(job), c),
    }
}

/// One `TalendColumnLineage` per target column, `Contains` from its target component, and
/// `ColumnFeeds` between jobs (RFC 0264 §"Phase 2").
fn emit_lineages(e: &mut Emit, all: &[(&Job, Job, Vec<ColumnLineage>)], stats: &mut TalendStats) {
    let views: Vec<JobLineages> = all
        .iter()
        .map(|(job, _, l)| JobLineages {
            job: &job.name,
            latest: job.latest,
            lineages: l,
        })
        .collect();
    let linked = link_jobs(&views);
    let mut ids: BTreeMap<(usize, usize), KirId> = BTreeMap::new();
    for (j, (job, expanded, lineages)) in all.iter().enumerate() {
        for (i, l) in lineages.iter().enumerate() {
            let key = format!("{}:{}:{}", job_key(job), l.component, l.target.column);
            let id = e.id("TalendColumnLineage", &key);
            if e.kir.objects.iter().any(|o| o.id == id) {
                continue;
            }
            let Some(c) = expanded.component(&l.component) else {
                continue;
            };
            let upstream = linked.upstream.get(&(j, i));
            let path = match &c.joblet {
                Some(o) => e
                    .project
                    .joblets
                    .iter()
                    .find(|x| x.name == o.name && x.version == o.version)
                    .map_or(job.path.as_str(), |x| x.path.as_str()),
                None => job.path.as_str(),
            };
            let name = format!("{}.{}", l.target.name, l.target.column);
            let props = json!({
                "job": job.name,
                "job_version": job.version,
                "latest": job.latest,
                "component": l.component,
                "target": l.target,
                "data_action": l.data_action,
                "update_key": l.update_key,
                "formula": l.formula,
                "formula_truncated": l.formula_truncated,
                "steps": l.steps,
                "origins": l.origins,
                "gaps": l.gaps,
                "upstream": upstream.map(|u| &u.0),
                "upstream_jobs": upstream.map(|u| &u.1),
                "description": lineage_description(job, l, upstream.map(|u| u.0.as_slice())),
            });
            let id = e.object(
                "TalendColumnLineage",
                &key,
                &name,
                props,
                (path, Some(c.line)),
            );
            ids.insert((j, i), id);
            let owner = e.id("TalendComponent", &origin_component_key(job, c));
            e.relate("Contains", owner, id, &key, Value::Null);
            stats.lineages += 1;
            if !l.gaps.is_empty() {
                stats.lineage_gaps += 1;
            }
        }
    }
    for f in &linked.feeds {
        let (Some(&from), Some(&to)) = (ids.get(&f.from), ids.get(&f.to)) else {
            continue;
        };
        e.relate("ColumnFeeds", from, to, &f.via, json!({ "via": f.via }));
        stats.column_feeds += 1;
    }
}

/// The searchable sentence of a lineage: target, job, formula, origins, and where those came
/// from across jobs.
fn lineage_description(
    job: &Job,
    l: &ColumnLineage,
    upstream: Option<&[ekos_talend::lineage::Origin]>,
) -> String {
    let labels = |o: &[ekos_talend::lineage::Origin]| {
        o.iter()
            .map(ekos_talend::lineage::Origin::label)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut text = format!(
        "{}.{} is written by Talend job {} ({}) as {}.",
        l.target.name,
        l.target.column,
        job.name,
        l.data_action.as_deref().unwrap_or("write"),
        l.formula
    );
    if !l.origins.is_empty() {
        text.push_str(&format!(" It comes from {}.", labels(&l.origins)));
    }
    if let Some(u) = upstream.filter(|u| *u != l.origins.as_slice() && !u.is_empty()) {
        text.push_str(&format!(" Across jobs, from {}.", labels(u)));
    }
    if !l.gaps.is_empty() {
        let at: Vec<String> = l
            .gaps
            .iter()
            .map(|g| format!("{} ({})", g.component, g.reason))
            .collect();
        text.push_str(&format!(" Not traced through {}.", at.join("; ")));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_talend::ProjectFile;

    fn file(path: &str, text: &str) -> ProjectFile {
        ProjectFile {
            path: path.into(),
            text: text.into(),
        }
    }

    /// Synthetic, in fixture B's shapes: a parent job calling a child; the child reads a table
    /// through a repository connection, maps it through a tMap with a lookup, and writes it.
    fn demo() -> Vec<ProjectFile> {
        let props = |id: &str, label: &str| {
            format!(
                r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties"><TalendProperties:Property id="{id}" label="{label}" version="0.1"/><TalendProperties:ItemState path="ODS"/></xmi:XMI>"#
            )
        };
        let child = r#"<talendfile:ProcessType xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:TalendMapper="http://www.talend.org/mapper" xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default">
  <context name="Default">
    <contextParameter name="schema_ods" type="id_String" value="ODS" repositoryContextId="_ctx"/>
  </context>
  <node componentName="tPostgresqlConnection"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBConnection_1"/><elementParameter field="TECHNICAL" name="PROPERTY:PROPERTY_TYPE" value="REPOSITORY"/><elementParameter field="TECHNICAL" name="PROPERTY:REPOSITORY_PROPERTY_TYPE" value="_conn"/><elementParameter field="TEXT" name="SCHEMA_DB" value="context.schema_ods"/></node>
  <node componentName="tPostgresqlInput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBInput_1"/><elementParameter field="TEXT" name="CONNECTION" value="tDBConnection_1"/><elementParameter field="DBTABLE" name="TABLE" value="&quot;SALES&quot;"/><elementParameter field="MEMO_SQL" name="QUERY" value="&quot;SELECT ID, QTY, PRICE FROM &quot;+context.schema_ods+&quot;.SALES&quot;"/><metadata connector="FLOW" name="tDBInput_1"><column name="ID" type="id_String"/><column name="QTY" type="id_Integer"/><column name="PRICE" type="id_Double"/></metadata></node>
  <node componentName="tPostgresqlInput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBInput_2"/><elementParameter field="DBTABLE" name="TABLE" value="&quot;PRODUCT&quot;"/></node>
  <node componentName="tMap"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tMap_1"/>
    <nodeData xsi:type="TalendMapper:MapperData">
      <outputTables name="out"><mapperTableEntries name="ID" expression="row1.ID"/><mapperTableEntries name="AMOUNT" expression="row1.QTY * row1.PRICE"/><mapperTableEntries name="NAME" expression="row2.NAME"/></outputTables>
      <inputTables name="row1"/>
      <inputTables name="row2" innerJoin="true" matchingMode="UNIQUE_MATCH"><mapperTableEntries name="ID" expression="row1.ID" operator="="/></inputTables>
    </nodeData>
  </node>
  <node componentName="tPostgresqlOutput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBOutput_1"/><elementParameter field="DBTABLE" name="TABLE" value="&quot;FACT&quot;"/><elementParameter field="CLOSED_LIST" name="DATA_ACTION" value="INSERT_OR_UPDATE"/><metadata connector="FLOW" name="tDBOutput_1"><column name="ID" key="true"/><column name="AMOUNT"/></metadata></node>
  <node componentName="tJava"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tJava_1"/><elementParameter field="MEMO_JAVA" name="CODE" value="System.out.println(customCode.clean(&quot;x&quot;));"/></node>
  <connection connectorName="FLOW" label="row1" lineStyle="0" source="tDBInput_1" target="tMap_1"/>
  <connection connectorName="FLOW" label="row2" lineStyle="8" source="tDBInput_2" target="tMap_1"/>
  <connection connectorName="FLOW" label="out" lineStyle="0" metaname="out" source="tMap_1" target="tDBOutput_1"/>
  <connection connectorName="COMPONENT_OK" label="OnComponentOk" lineStyle="3" source="tDBConnection_1" target="tJava_1"/>
</talendfile:ProcessType>"#;
        let parent = r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default"><node componentName="tRunJob"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tRunJob_1"/><elementParameter field="PROCESS_TYPE" name="PROCESS" value="jChild"/><elementParameter field="TECHNICAL" name="PROCESS:PROCESS_TYPE_PROCESS" value="DEMO:_child"/><elementParameter field="TECHNICAL" name="PROCESS:PROCESS_TYPE_VERSION" value="Latest"/></node></talendfile:ProcessType>"#;
        vec![
            file(
                "talend.project",
                r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties"><TalendProperties:Project label="Demo" technicalLabel="DEMO"/></xmi:XMI>"#,
            ),
            file("process/ODS/jChild_0.1.item", child),
            file(
                "process/ODS/jChild_0.1.properties",
                &props("_child", "jChild"),
            ),
            file("process/ODS/jParent_0.1.item", parent),
            file(
                "process/ODS/jParent_0.1.properties",
                &props("_parent", "jParent"),
            ),
            file(
                "context/Conn_0.1.item",
                r#"<talendfile:ContextType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" name="Default"><contextParameter name="schema_ods" type="id_String" value="ODS"/></talendfile:ContextType>"#,
            ),
            file("context/Conn_0.1.properties", &props("_ctx", "Conn")),
            file(
                "metadata/connections/ODS_0.1.item",
                r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendMetadata="http://www.talend.org/metadata/connection/2010"><TalendMetadata:DatabaseConnection id="_conn" label="ODS" DatabaseType="PostgreSQL"/></xmi:XMI>"#,
            ),
            file(
                "metadata/connections/ODS_0.1.properties",
                &props("_conn", "ODS"),
            ),
            file(
                "code/routines/customCode_0.1.item",
                "package routines;\npublic class customCode {\n  public static String clean(String s) { return s; }\n}\n",
            ),
        ]
    }

    /// [`demo`] plus a joblet and a report job that reads the child's table through it.
    fn demo_with_joblet() -> Vec<ProjectFile> {
        let mut files = demo();
        let joblet = r#"<model:JobletProcess xmlns:model="http://www.talend.com/joblet.ecore" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:TalendMapper="http://www.talend.org/mapper" defaultContext="Default">
  <jobletNodes componentName="INPUT"><elementParameter field="TEXT" name="UNIQUE_NAME" value="INPUT_1"/></jobletNodes>
  <node componentName="tMap"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tMap_1"/>
    <nodeData xsi:type="TalendMapper:MapperData"><outputTables name="o"><mapperTableEntries name="AMT" expression="row1.AMOUNT * 2"/></outputTables><inputTables name="row1"/></nodeData>
  </node>
  <jobletNodes componentName="OUTPUT"><elementParameter field="TEXT" name="UNIQUE_NAME" value="OUTPUT_1"/></jobletNodes>
  <connection connectorName="FLOW" label="row1" lineStyle="0" source="INPUT_1" target="tMap_1"/>
  <connection connectorName="FLOW" label="o" lineStyle="0" metaname="o" source="tMap_1" target="OUTPUT_1"/>
</model:JobletProcess>"#;
        let report = r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default">
  <node componentName="tPostgresqlInput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBInput_1"/><elementParameter field="DBTABLE" name="TABLE" value="&quot;FACT&quot;"/><metadata connector="FLOW" name="tDBInput_1"><column name="ID"/><column name="AMOUNT"/></metadata></node>
  <node componentName="jlDouble"><elementParameter field="TEXT" name="UNIQUE_NAME" value="jlDouble_1"/></node>
  <node componentName="tPostgresqlOutput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tDBOutput_1"/><elementParameter field="DBTABLE" name="TABLE" value="&quot;REPORT&quot;"/><elementParameter field="CLOSED_LIST" name="DATA_ACTION" value="INSERT"/><metadata connector="FLOW" name="tDBOutput_1"><column name="AMT"/></metadata></node>
  <connection connectorName="FLOW" label="row1" lineStyle="0" source="tDBInput_1" target="jlDouble_1"/>
  <connection connectorName="FLOW" label="row2" lineStyle="0" source="jlDouble_1" target="tDBOutput_1"/>
</talendfile:ProcessType>"#;
        files.push(file("joblets/jlDouble_0.1.item", joblet));
        files.push(file("process/RPT/jReport_0.1.item", report));
        files
    }

    #[test]
    fn a_routine_is_called_by_its_short_or_full_name() {
        assert!(calls_routine("HashUtil.sha256(row1.email)", "HashUtil"));
        assert!(calls_routine(
            "routines.HashUtil.sha256(row1.email)",
            "HashUtil"
        ));
        assert!(!calls_routine("other.HashUtil.sha256(x)", "HashUtil"));
        assert!(!calls_routine("MyHashUtil.sha256(x)", "HashUtil"));
    }

    #[test]
    fn lineage_runs_through_a_joblet_and_across_jobs_to_the_first_source() {
        let project = read_project("", &demo_with_joblet());
        let (kir, stats, diagnostics) = project_kir(&project);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!((stats.joblets, stats.joblet_uses), (1, 1));
        let lineage = |name: &str| {
            kir.objects
                .iter()
                .find(|o| o.kind.to_string() == "TalendColumnLineage" && o.name == name)
                .unwrap_or_else(|| panic!("no lineage {name}"))
        };
        let fact = lineage("FACT.AMOUNT");
        assert_eq!(fact.properties["formula"], "SALES.QTY * SALES.PRICE");
        let report = lineage("REPORT.AMT");
        assert_eq!(report.properties["formula"], "FACT.AMOUNT * 2");
        let map = report.properties["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["kind"] == "map")
            .unwrap();
        assert_eq!(map["component"], "jlDouble_1__tMap_1");
        assert_eq!(map["joblet"]["name"], "jlDouble");
        // Across jobs: the report's amount comes from the sales columns the child job reads.
        let upstream: Vec<String> = report.properties["upstream"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                format!(
                    "{}.{}",
                    o["table"].as_str().unwrap(),
                    o["column"].as_str().unwrap()
                )
            })
            .collect();
        assert_eq!(upstream, ["SALES.PRICE", "SALES.QTY"]);
        assert_eq!(report.properties["upstream_jobs"], json!(["jChild"]));
        // The job says what it reads and writes, joblet components included.
        let job = kir
            .objects
            .iter()
            .find(|o| o.kind.to_string() == "TalendJob" && o.name == "jReport")
            .unwrap();
        assert_eq!(job.properties["reads"], json!(["FACT"]));
        assert_eq!(job.properties["writes"], json!(["REPORT"]));
        let text = job.properties["description"].as_str().unwrap();
        assert!(
            text.contains("Reads tables FACT. Writes tables REPORT."),
            "{text}"
        );
        let feeds = rels(&kir, "ColumnFeeds");
        assert_eq!(feeds.len(), 1);
        assert_eq!((feeds[0].from, feeds[0].to), (fact.id, report.id));
        // The lineage hangs off the target component; the joblet use points at the joblet.
        assert!(
            rels(&kir, "Contains")
                .iter()
                .any(|r| r.to == report.id && by_id(&kir, r.from).name == "jReport.tDBOutput_1")
        );
        let uses = rels(&kir, "UsesJoblet");
        assert_eq!(uses.len(), 1);
        assert_eq!(by_id(&kir, uses[0].to).name, "jlDouble");
        assert!(
            report.properties["description"]
                .as_str()
                .unwrap()
                .contains("Across jobs, from SALES.PRICE, SALES.QTY.")
        );
    }

    fn rels<'a>(kir: &'a KirGraph, kind: &str) -> Vec<&'a KirRelationship> {
        kir.relationships
            .iter()
            .filter(|r| r.kind.to_string() == kind)
            .collect()
    }

    fn by_id(kir: &KirGraph, id: KirId) -> &KirObject {
        kir.objects.iter().find(|o| o.id == id).unwrap()
    }

    #[test]
    fn findings_never_share_a_name_with_another_object() {
        // Identity reports one name in two kinds as a conflict (`resolve` stops).
        let project = read_project("", &elt_project());
        let (kir, stats, _) = project_kir(&project);
        assert!(stats.findings > 0, "the lone ELT job is an uncalled job");
        let finding = ObjectKind::Custom("TalendFinding".into());
        for f in kir.objects.iter().filter(|o| o.kind == finding) {
            let same = kir
                .objects
                .iter()
                .filter(|o| o.kind != finding && o.name.eq_ignore_ascii_case(&f.name))
                .count();
            assert_eq!(same, 0, "{}", f.name);
        }
    }

    #[test]
    fn a_project_becomes_objects_relationships_and_ir() {
        let project = read_project("etl/demo", &demo());
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let (kir, stats, _) = project_kir(&project);
        let kinds: BTreeMap<String, usize> =
            kir.objects.iter().fold(BTreeMap::new(), |mut m, o| {
                *m.entry(o.kind.to_string()).or_insert(0) += 1;
                m
            });
        assert_eq!(kinds["TalendJob"], 2);
        assert_eq!(kinds["TalendComponent"], 7);
        assert_eq!(kinds["TalendContextVariable"], 1);
        assert_eq!(kinds["TalendDbConnection"], 1);
        assert_eq!(kinds["TalendRoutine"], 1);

        let calls = rels(&kir, "Calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(by_id(&kir, calls[0].to).name, "jChild");
        assert_eq!(rels(&kir, "RowLink").len(), 3);
        assert_eq!(rels(&kir, "Triggers").len(), 1);
        // The input uses the repository connection through the connection component.
        let uses_conn: Vec<_> = rels(&kir, "UsesConnection")
            .iter()
            .map(|r| by_id(&kir, r.from).name.clone())
            .collect();
        assert_eq!(
            uses_conn,
            vec!["jChild.tDBConnection_1", "jChild.tDBInput_1"]
        );
        assert_eq!(rels(&kir, "UsesContext").len(), 1);
        assert_eq!(rels(&kir, "UsesRoutine").len(), 1);

        assert_eq!((stats.data_components, stats.mapped_components), (4, 4));
        assert_eq!((stats.queries, stats.queries_folded), (1, 1));
        // The tMap: one join, three Calculate nodes; AMOUNT names both input columns.
        let nodes: Vec<&KirObject> = kir
            .objects
            .iter()
            .filter(|o| o.kind.to_string() == "TransformNode")
            .collect();
        let amount = nodes
            .iter()
            .find(|o| {
                o.properties["node_type"] == "Calculate" && o.properties["output"] == "out.AMOUNT"
            })
            .unwrap();
        let refs: Vec<String> = amount.properties["column_refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                format!(
                    "{}:{}",
                    r["component"].as_str().unwrap(),
                    r["column"].as_str().unwrap()
                )
            })
            .collect();
        assert_eq!(refs, vec!["tDBInput_1:QTY", "tDBInput_1:PRICE"]);
        let sink = nodes
            .iter()
            .find(|o| o.properties["node_type"] == "Sink")
            .unwrap();
        assert_eq!(sink.properties["object_name"], "FACT");
        assert_eq!(sink.properties["data_action"], "INSERT_OR_UPDATE");
        assert_eq!(sink.properties["keys"], json!(["ID"]));
        let source = nodes
            .iter()
            .find(|o| o.properties["object_name"] == "SALES")
            .unwrap();
        assert_eq!(source.properties["schema"], "ODS");
        assert_eq!(
            source.properties["query"]["text"],
            "SELECT ID, QTY, PRICE FROM ODS.SALES"
        );
    }

    /// Synthetic, in fixture E's shape (RFC 0264 §ELT): two ELT inputs joined, one insert.
    fn elt_project() -> Vec<ProjectFile> {
        let input = |name: &str, schema: &str, table: &str, cols: &str| {
            format!(
                r#"<node componentName="tELTInput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="{name}"/><elementParameter field="TEXT" name="ELT_TABLE_NAME" value="&quot;{table}&quot;"/><elementParameter field="TEXT" name="ELT_SCHEMA_NAME" value="&quot;{schema}&quot;"/><elementParameter field="MAPPING_TYPE" name="MAPPING" value="snowflake_id"/><metadata connector="TABLE" name="{name}">{cols}</metadata></node>"#
            )
        };
        let job = format!(
            r#"<talendfile:ProcessType xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default"><context name="Default"/>{}{}
  <node componentName="tELTMap"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tELTMap_1"/>
    <nodeData xsi:type="TalendDBMap:DBMapData">
      <InputTables alias="S" joinType="NO_JOIN" name="S" tableName="STG.SALES"><DBMapperTableEntries name="PID"/><DBMapperTableEntries name="QTY"/></InputTables>
      <InputTables alias="P" joinType="INNER_JOIN" name="P" tableName="ODS.PRODUCT"><DBMapperTableEntries expression="S.PID" join="true" name="ID" operator="="/><DBMapperTableEntries name="PRICE"/></InputTables>
      <OutputTables name="table3" tableName="FACT"><DBMapperTableEntries expression="sum(S.QTY * P.PRICE)" name="AMOUNT"/><FilterEntries expression="S.QTY &gt; 0" filterKind="WHERE_FILTER" name="f"/><FilterEntries expression="GROUP BY S.PID" filterKind="OTHER_FILTER" name="g"/></OutputTables>
    </nodeData>
  </node>
  <node componentName="tELTOutput"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tELTOutput_1"/><elementParameter field="CLOSED_LIST" name="DATA_ACTION" value="INSERT"/><elementParameter field="TEXT" name="ELT_TABLE_NAME" value="&quot;FACT&quot;"/><elementParameter field="TEXT" name="ELT_SCHEMA_NAME" value="&quot;DWH&quot;"/><metadata connector="TABLE" name="tELTOutput_1"><column name="AMOUNT"/></metadata></node>
  <connection connectorName="TABLE" label="STG.SALES" lineStyle="9" metaname="t1" source="tELTInput_1" target="tELTMap_1"/>
  <connection connectorName="TABLE" label="ODS.PRODUCT" lineStyle="9" metaname="t2" source="tELTInput_2" target="tELTMap_1"/>
  <connection connectorName="TABLE" label="FACT" lineStyle="9" metaname="table3" source="tELTMap_1" target="tELTOutput_1"/>
</talendfile:ProcessType>"#,
            input(
                "tELTInput_1",
                "STG",
                "SALES",
                r#"<column name="PID"/><column name="QTY"/>"#
            ),
            input(
                "tELTInput_2",
                "ODS",
                "PRODUCT",
                r#"<column name="ID"/><column name="PRICE"/>"#
            ),
        );
        vec![
            ProjectFile {
                path: "talend.project".into(),
                text: r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties"><TalendProperties:Project label="E" technicalLabel="E"/></xmi:XMI>"#.into(),
            },
            ProjectFile {
                path: "process/jElt_0.1.properties".into(),
                text: r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties"><TalendProperties:Property id="_elt" label="jElt" version="0.1"/><TalendProperties:ItemState path=""/></xmi:XMI>"#.into(),
            },
            ProjectFile {
                path: "process/jElt_0.1.item".into(),
                text: job,
            },
        ]
    }

    #[test]
    fn an_elt_job_lowers_to_a_join_filter_and_sink_and_names_its_tables() {
        use ekos_semantic::transform_ir::{JoinKind, TransformNode};
        let project = read_project("", &elt_project());
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let job = &project.jobs[0];
        let lowered = lower_job(job);
        assert_eq!(lowered.mapped_components, lowered.data_components);
        let g = &lowered.graph;
        let name = |i: u32| {
            lowered.extra[i as usize]["component"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let join = g
            .nodes
            .iter()
            .find_map(|n| match n {
                TransformNode::Join {
                    left,
                    right,
                    kind,
                    keys,
                } => Some((*left, *right, *kind, keys)),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (name(join.0.0), name(join.1.0)),
            ("tELTInput_1".into(), "tELTInput_2".into())
        );
        assert_eq!(join.2, JoinKind::Inner);
        assert_eq!(join.3, &vec![("S.PID".to_string(), "P.ID".to_string())]);
        assert!(
            g.nodes.iter().any(
                |n| matches!(n, TransformNode::Filter { condition } if condition == "S.QTY > 0")
            )
        );
        assert!(g.nodes.iter().any(
            |n| matches!(n, TransformNode::Sink { object_name, .. } if object_name == "FACT")
        ));
        let calc = g
            .nodes
            .iter()
            .position(|n| matches!(n, TransformNode::Calculate { .. }))
            .unwrap();
        assert_eq!(lowered.extra[calc]["clauses"], json!(["GROUP BY S.PID"]));
        assert_eq!(
            lowered.extra[calc]["column_refs"][1],
            json!({"input": "P", "column": "PRICE", "component": "tELTInput_2"})
        );
        let io = crate::talend_ir::data_io(job);
        assert_eq!(io.reads, ["PRODUCT", "SALES"]);
        assert_eq!(io.writes, ["FACT"]);
        let (kir, _, diagnostics) = project_kir(&project);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(
            kir.objects
                .iter()
                .any(|o| o.properties.get("formula").and_then(Value::as_str)
                    == Some("sum(SALES.QTY * PRODUCT.PRICE)"))
        );
    }

    #[test]
    fn the_tmap_join_names_both_upstream_nodes() {
        let project = read_project("", &demo());
        let job = project.jobs.iter().find(|j| j.name == "jChild").unwrap();
        let lowered = lower_job(job);
        let g = &lowered.graph;
        let (join_index, join) = g
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| matches!(n, ekos_semantic::transform_ir::TransformNode::Join { .. }))
            .unwrap();
        let ekos_semantic::transform_ir::TransformNode::Join {
            left, right, keys, ..
        } = join
        else {
            unreachable!()
        };
        let name = |i: u32| {
            lowered.extra[i as usize]["component"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            (name(left.0), name(right.0)),
            ("tDBInput_1".into(), "tDBInput_2".into())
        );
        assert_eq!(keys, &vec![("row1.ID".to_string(), "row2.ID".to_string())]);
        // The join feeds all three Calculate nodes, which feed the output.
        let out_edges = g
            .edges
            .iter()
            .filter(|(f, _)| f.0 as usize == join_index)
            .count();
        assert_eq!(out_edges, 3);
    }

    #[test]
    fn ids_are_stable_across_reads_and_scoped_to_the_project() {
        let a = project_kir(&read_project("x", &demo())).0;
        let b = project_kir(&read_project("x", &demo())).0;
        let c = project_kir(&read_project("y", &demo())).0;
        let ids = |k: &KirGraph| {
            k.objects
                .iter()
                .map(|o| o.id.to_string())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(ids(&a), ids(&b));
        assert!(ids(&a).is_disjoint(&ids(&c)));
    }

    #[test]
    fn every_talend_kind_has_a_keyed_registry_row() {
        for kind in KINDS {
            assert!(
                ekos_kir::custom_kinds::is_structurally_keyed(kind),
                "{kind} must be registered as structurally keyed"
            );
        }
    }

    #[test]
    fn every_emitted_kind_is_listed() {
        let mut files = demo_with_joblet();
        files.push(file(
            "metadata/genericSchema/S_0.1.item",
            r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI"/>"#,
        ));
        let (kir, _, _) = project_kir(&read_project("", &files));
        let emitted: BTreeSet<String> = kir
            .objects
            .iter()
            .map(|o| o.kind.to_string())
            .filter(|k| k != "TransformNode")
            .collect();
        let listed: BTreeSet<String> = KINDS.iter().map(ToString::to_string).collect();
        // The demo has no context-free item of every kind; what it emits must all be listed.
        assert!(
            emitted.is_subset(&listed),
            "{:?}",
            emitted.difference(&listed).collect::<Vec<_>>()
        );
        assert_eq!(
            emitted.len(),
            KINDS.len(),
            "the demo should cover every kind: {emitted:?}"
        );
    }

    #[test]
    fn a_routine_call_needs_a_method_and_parentheses() {
        assert!(calls_routine("x = customCode.clean(y)", "customCode"));
        assert!(!calls_routine("my.customCode.clean(y)", "customCode"));
        assert!(!calls_routine("customCode.FIELD", "customCode"));
    }
}
