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
use ekos_talend::java::context_refs;
use ekos_talend::{Component, Job, Param, Project, ProjectFile, RuntimeContext, read_project};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::talend_ir::lower_job;

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
        "v1"
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
            let (graph, project_stats) = project_kir(&project);
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

fn component_key(job: &Job, c: &Component) -> String {
    format!("{}:{}", job_key(job), c.unique_name)
}

/// Every object, relationship and IR graph for one project.
pub fn project_kir(project: &Project) -> (KirGraph, TalendStats) {
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
    for job in &project.jobs {
        emit_job(&mut e, project_id, job, &mut stats);
    }
    (e.kir, stats)
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

fn emit_job(e: &mut Emit, project_id: KirId, job: &Job, stats: &mut TalendStats) {
    let project = e.project;
    let calls = project.calls(job);
    let unresolved: Vec<&str> = calls
        .iter()
        .filter(|c| c.resolved.is_none())
        .map(|c| c.child_name.as_str())
        .collect();
    let props = json!({
        "job": job.name,
        "version": job.version,
        "latest": job.latest,
        "repository_id": job.id,
        "folder": job.folder,
        "job_type": job.job_type,
        "purpose": job.purpose,
        "description": job.description,
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
    for c in &job.components {
        let cid = emit_component(e, job, c, stats);
        e.relate("Contains", job_id, cid, "", Value::Null);
    }
    emit_flow(e, job);
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
    emit_ir(e, job, stats);
}

fn emit_component(e: &mut Emit, job: &Job, c: &Component, stats: &mut TalendStats) -> KirId {
    stats.components += 1;
    if c.logical == "db_input" && c.params.contains_key("QUERY") {
        stats.queries += 1;
        if let Some(q) = c.params["QUERY"].text()
            && job.fold(q).complete()
        {
            stats.queries_folded += 1;
        }
    }
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
    });
    let name = format!("{}.{}", job.name, c.unique_name);
    let id = e.object(
        "TalendComponent",
        &component_key(job, c),
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

fn emit_flow(e: &mut Emit, job: &Job) {
    let cid = |e: &Emit, name: &str| {
        let c = job.component(name)?;
        Some(e.id("TalendComponent", &component_key(job, c)))
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
fn job_texts(job: &Job) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for c in &job.components {
        for p in c.params.values() {
            match p {
                Param::Value { value, .. } => out.push(value),
                Param::Table { rows, .. } => {
                    out.extend(rows.iter().flat_map(|r| r.values().map(String::as_str)))
                }
                Param::Secret { .. } => {}
            }
        }
        if let Some(m) = &c.mapper {
            let entries = m
                .inputs
                .iter()
                .flat_map(|t| &t.entries)
                .chain(m.outputs.iter().flat_map(|t| &t.entries))
                .chain(&m.vars);
            out.extend(entries.filter_map(|x| x.expression.as_deref()));
            out.extend(m.inputs.iter().filter_map(|t| t.filter.as_deref()));
            out.extend(m.outputs.iter().filter_map(|t| t.filter.as_deref()));
        }
    }
    out.extend(job.triggers.iter().filter_map(|t| t.condition.as_deref()));
    out
}

fn emit_uses(e: &mut Emit, job: &Job, job_id: KirId) {
    let texts = job_texts(job);
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

/// `routine.method(` at a word boundary.
fn calls_routine(text: &str, routine: &str) -> bool {
    let needle = format!("{routine}.");
    text.match_indices(&needle).any(|(at, _)| {
        let before_ok = text[..at]
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

fn emit_ir(e: &mut Emit, job: &Job, stats: &mut TalendStats) {
    let mut lowered = lower_job(job);
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
    fn a_project_becomes_objects_relationships_and_ir() {
        let project = read_project("etl/demo", &demo());
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        let (kir, stats) = project_kir(&project);
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
        let mut files = demo();
        files.push(file(
            "metadata/genericSchema/S_0.1.item",
            r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI"/>"#,
        ));
        let (kir, _) = project_kir(&read_project("", &files));
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
