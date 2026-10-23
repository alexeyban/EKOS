//! `JavaAnalyzerPass` — RFC 0209: Java source into KIR, deterministically, with no model call.
//!
//! Each `.java` artifact from `plugins/java` is parsed at declaration level by
//! [`crate::java_syntax`]. What comes out:
//!
//! - `Custom("JavaType")` per class / interface / enum / record / annotation type, keyed by its
//!   fully qualified name; `Contains` from its `File` (top level) or its enclosing type (nested).
//! - `Custom("JavaMethod")` per method and constructor, keyed by (type, name, parameter types) so
//!   overloads stay distinct; `Contains` from its type. `source_span`, `signature`, `visibility`,
//!   Javadoc `description`, and `KirEvidence` from the span.
//! - `Extends` / `Custom("Implements")` / `DependsOn` (imports) — **only to types declared in the
//!   observed tree**. A name is resolved the way Java resolves it (single-type import, nested type,
//!   same package, on-demand import) against the types this run actually parsed; a type that is
//!   not in the tree (`java.util.List`, a library) is listed as written on the source type and
//!   never becomes a fabricated object.
//! - Generated files (`Generated Model - DO NOT CHANGE`, `Generated Interface for`, `@Generated`)
//!   produce their types with `generated: true` and `member_count`, but no `JavaMethod` objects:
//!   their accessors are recovered as columns of a `ModelTable` instead.
//! - `Custom("ModelTable")` per table of an iDempiere-family application model
//!   ([`crate::app_model`]), with `Custom("Models")` from the generated and business classes, and
//!   `References` between tables for each code-declared logical foreign key.
//!
//! Not a call graph: receiver types of locals are unknown without type inference (RFC 0209 §2).

use crate::app_model::{self, AppModel, SourceFile};
use crate::java_sql::{self, ConstantLookup, EmbeddedSql};
use crate::java_syntax::{self, FileStatus, JavaFile, TypeDecl, TypeKind, doc_description};
use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const JAVA_TYPE_KIND: &str = "JavaType";
pub const JAVA_METHOD_KIND: &str = "JavaMethod";
pub const MODEL_TABLE_KIND: &str = "ModelTable";

#[derive(Debug, Deserialize)]
struct JavaArtifactData {
    path: String,
    source: String,
    /// RFC 0079: qualifies id hashing only in a multi-`[observe] paths` workspace.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run (RFC 0209 §5.3).
#[derive(Debug, Clone, Copy, Default)]
pub struct JavaStats {
    pub files_processed: usize,
    pub files_parsed: usize,
    pub files_partial: usize,
    pub files_skipped: usize,
    pub unclassified_members: usize,
    pub types_total: usize,
    pub generated_types: usize,
    pub methods_total: usize,
    pub model_tables: usize,
    pub model_columns: usize,
    pub model_references: usize,
    pub sql_statements: usize,
    pub sql_parsed: usize,
    pub sql_partial: usize,
    pub sql_unparsed: usize,
    /// `ReadsFrom` / `WritesTo` edges from a method to a `ModelTable`.
    pub sql_links: usize,
}

pub struct JavaAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<JavaStats>>,
}

impl JavaAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("java-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(JavaStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<JavaStats>> {
        Arc::clone(&self.stats)
    }
}

/// One parsed file and where it came from.
struct Parsed {
    path: String,
    project: Option<String>,
    source: String,
    file: JavaFile,
    generated: bool,
}

#[async_trait]
impl CompilerPass for JavaAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    /// Bump on any change to this pass's output shape (see `rust_analyzer`'s `version`).
    fn version(&self) -> &str {
        "v1"
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .artifact_ids
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut parsed: Vec<Parsed> = Vec::new();
        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "JAVA001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: JavaArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "JAVA002",
                        format!("malformed java payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let file = java_syntax::parse(&data.source);
            if file.status == FileStatus::Skipped {
                ctx.diagnostics.lock().unwrap().warning(
                    "JAVA003",
                    format!("{}: brackets do not balance; file skipped", data.path),
                );
            }
            let generated = is_generated(&file);
            parsed.push(Parsed {
                path: data.path,
                project: data.project,
                source: data.source,
                file,
                generated,
            });
        }
        // Artifact order is not stable across recovers; output must not depend on it.
        parsed.sort_by(|a, b| (&a.project, &a.path).cmp(&(&b.project, &b.path)));

        let (graph, stats) = compile(&parsed);
        *self.stats.lock().unwrap() = stats;
        if graph.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            files = stats.files_processed,
            partial = stats.files_partial,
            skipped = stats.files_skipped,
            types = stats.types_total,
            methods = stats.methods_total,
            model_tables = stats.model_tables,
            "java-analyzer complete"
        );
        Ok(())
    }
}

/// Issue-tracker keys whose ids are anchored exactly (RFC 0209 §8).
const TICKET_KEYS: &[&str] = &["IDEMPIERE", "ADEMPIERE"];

/// `IDEMPIERE-1234` style ids in a comment, exact matches only.
fn tickets_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in TICKET_KEYS {
        let needle = format!("{key}-");
        let mut from = 0;
        while let Some(pos) = text[from..].find(&needle) {
            let start = from + pos;
            let digits: String = text[start + needle.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            let boundary_before = text[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            if !digits.is_empty() && boundary_before {
                out.push(format!("{needle}{digits}"));
            }
            from = start + needle.len();
        }
    }
    out
}

/// Ticket ids by the line their comment ends on.
fn ticket_lines(file: &JavaFile) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for c in &file.comments {
        for t in tickets_in(&c.text) {
            out.push((c.end_line, t));
        }
    }
    out
}

/// The tickets whose comment falls inside `[start, end]` and inside no narrower span in `inner`.
fn tickets_for(
    tickets: &[(u32, String)],
    (start, end): (u32, u32),
    inner: &[(u32, u32)],
) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for (line, t) in tickets {
        let within = *line >= start && *line <= end;
        let claimed = inner.iter().any(|&(s, e)| *line >= s && *line <= e);
        if within && !claimed {
            out.insert(t.clone());
        }
    }
    out.into_iter().collect()
}

/// A file the generator wrote: its own header says so, or a type carries `@Generated`.
fn is_generated(file: &JavaFile) -> bool {
    let first_type_line = file.types.first().map_or(u32::MAX, |t| t.start_line);
    let header = file.comments.iter().any(|c| {
        c.start_line <= first_type_line
            && (c.text.contains("Generated Model - DO NOT CHANGE")
                || c.text.contains("Generated Interface for"))
    });
    header
        || file
            .types
            .iter()
            .any(|t| t.annotations.iter().any(|a| a.simple_name() == "Generated"))
}

// ── Deterministic ids ────────────────────────────────────────────────────────

fn file_kir_id(path: &str, project: Option<&str>) -> KirId {
    let id_path = ekos_common::project::project_qualify(path, project);
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_path.as_bytes()))
}

pub fn java_type_kir_id(qualified: &str, project: Option<&str>) -> KirId {
    let q = ekos_common::project::project_qualify(qualified, project);
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("java-type:{q}").as_bytes(),
    ))
}

fn java_method_kir_id(owner: KirId, key: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("java-method:{owner}:{key}").as_bytes(),
    ))
}

pub fn model_table_kir_id(table: &str, project: Option<&str>) -> KirId {
    let q = ekos_common::project::project_qualify(table, project);
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("model-table:{}:{q}", app_model::FAMILY).as_bytes(),
    ))
}

// ── Name resolution ──────────────────────────────────────────────────────────

/// Every type the run parsed, by project and fully qualified name.
type TypeIndex = HashMap<(Option<String>, String), ()>;

type TypeKey = (Option<String>, String);

/// What compiling one file may consult about the rest of the tree.
struct Context {
    index: TypeIndex,
    /// Literal `static final` constants of every type, generated ones included.
    constants: HashMap<TypeKey, BTreeMap<String, String>>,
    /// Supertypes of every type, resolved to types in the tree.
    supers: HashMap<TypeKey, Vec<String>>,
    /// `(project, lower-cased table name)` → the `ModelTable`'s name, for embedded SQL.
    tables: HashMap<TypeKey, String>,
}

/// Constant lookup for the methods of one type: its own constants, then its enclosing types', then
/// up its supertypes breadth first — the order Java's own name resolution visits them in.
struct Lookup<'a> {
    ctx: &'a Context,
    file: &'a JavaFile,
    /// Outermost first, the type itself last.
    chain: Vec<&'a TypeDecl>,
    project: &'a Option<String>,
}

impl Lookup<'_> {
    fn find_from(&self, start: Vec<String>, name: &str) -> Option<String> {
        let mut queue: VecDeque<String> = start.into();
        let mut seen = HashSet::new();
        while let Some(q) = queue.pop_front() {
            if !seen.insert(q.clone()) {
                continue;
            }
            let key = (self.project.clone(), q);
            if let Some(v) = self.ctx.constants.get(&key).and_then(|c| c.get(name)) {
                return Some(v.clone());
            }
            if let Some(s) = self.ctx.supers.get(&key) {
                queue.extend(s.iter().cloned());
            }
        }
        None
    }
}

impl ConstantLookup for Lookup<'_> {
    fn constant(&self, qualifier: Option<&str>, name: &str) -> Option<String> {
        match qualifier {
            None => self.find_from(
                self.chain
                    .iter()
                    .rev()
                    .map(|t| t.qualified.clone())
                    .collect(),
                name,
            ),
            Some(q) => {
                let resolved = resolve(q, self.file, &self.chain, self.project, &self.ctx.index)?;
                self.find_from(vec![resolved], name)
            }
        }
    }
}

/// Resolve a type name as written in `file` to the qualified name of a type **in the tree**, by
/// Java's own order: nested type of an enclosing type, single-type import, same package, then
/// on-demand (`.*`) imports. `None` when the type is not one this run parsed.
fn resolve(
    name: &str,
    file: &JavaFile,
    enclosing: &[&TypeDecl],
    project: &Option<String>,
    index: &TypeIndex,
) -> Option<String> {
    let known = |q: &str| index.contains_key(&(project.clone(), q.to_string()));
    let (head, tail) = match name.split_once('.') {
        Some((h, t)) => (h, Some(t)),
        None => (name, None),
    };
    let with_tail = |base: String| match tail {
        Some(t) => format!("{base}.{t}"),
        None => base,
    };
    // Fully qualified as written.
    if tail.is_some() && known(name) {
        return Some(name.to_string());
    }
    for outer in enclosing.iter().rev() {
        let q = with_tail(format!("{}.{head}", outer.qualified));
        if known(&q) {
            return Some(q);
        }
    }
    for imp in file.imports.iter().filter(|i| !i.is_static && !i.wildcard) {
        if imp.path.rsplit('.').next() == Some(head) {
            let q = with_tail(imp.path.clone());
            return known(&q).then_some(q);
        }
    }
    let local = match &file.package {
        Some(p) => format!("{p}.{head}"),
        None => head.to_string(),
    };
    if known(&with_tail(local.clone())) {
        return Some(with_tail(local));
    }
    for imp in file.imports.iter().filter(|i| !i.is_static && i.wildcard) {
        let q = with_tail(format!("{}.{head}", imp.path));
        if known(&q) {
            return Some(q);
        }
    }
    None
}

/// The chain of types enclosing `idx` (outermost first), including `idx` itself.
fn enclosing(file: &JavaFile, idx: usize) -> Vec<&TypeDecl> {
    let mut chain = vec![&file.types[idx]];
    let mut cur = file.types[idx].parent;
    while let Some(p) = cur {
        chain.push(&file.types[p]);
        cur = file.types[p].parent;
    }
    chain.reverse();
    chain
}

fn visibility(modifiers: &[String]) -> &'static str {
    for m in modifiers {
        match m.as_str() {
            "public" => return "public",
            "protected" => return "protected",
            "private" => return "private",
            _ => {}
        }
    }
    "package"
}

fn span(start: u32, end: u32) -> serde_json::Value {
    serde_json::json!({"start_line": start, "end_line": end})
}

// ── Compilation ──────────────────────────────────────────────────────────────

fn compile(parsed: &[Parsed]) -> (KirGraph, JavaStats) {
    let mut graph = KirGraph::new();
    let mut stats = JavaStats::default();
    let mut index: TypeIndex = HashMap::new();
    for p in parsed {
        for t in &p.file.types {
            index.insert((p.project.clone(), t.qualified.clone()), ());
        }
    }

    let mut ctx = Context {
        index,
        constants: HashMap::new(),
        supers: HashMap::new(),
        tables: HashMap::new(),
    };
    for p in parsed {
        for (idx, t) in p.file.types.iter().enumerate() {
            let key = (p.project.clone(), t.qualified.clone());
            ctx.constants
                .insert(key.clone(), literal_constant_map(&p.file, t));
            let chain = enclosing(&p.file, idx);
            let supers = t
                .extends
                .iter()
                .chain(&t.implements)
                .filter_map(|n| {
                    resolve(
                        n,
                        &p.file,
                        &chain[..chain.len() - 1],
                        &p.project,
                        &ctx.index,
                    )
                })
                .collect();
            ctx.supers.insert(key, supers);
        }
    }

    // The application model is read per project: two projects' `C_Invoice` are two tables.
    let mut projects: Vec<Option<String>> = parsed.iter().map(|p| p.project.clone()).collect();
    projects.dedup();
    let mut models = Vec::new();
    for project in projects {
        let files: Vec<SourceFile<'_>> = parsed
            .iter()
            .filter(|p| p.project == project)
            .map(|p| SourceFile {
                path: &p.path,
                file: &p.file,
            })
            .collect();
        let model = app_model::recognize(&files);
        for name in model.tables.keys() {
            ctx.tables
                .insert((project.clone(), name.to_ascii_lowercase()), name.clone());
        }
        models.push((project, model));
    }

    let mut seen: HashSet<KirId> = HashSet::new();
    for p in parsed {
        stats.files_processed += 1;
        match p.file.status {
            FileStatus::Parsed => stats.files_parsed += 1,
            FileStatus::Partial => stats.files_partial += 1,
            FileStatus::Skipped => stats.files_skipped += 1,
        }
        stats.unclassified_members += p.file.unclassified_members;
        compile_file(p, &ctx, &mut graph, &mut stats, &mut seen);
    }

    for (project, model) in models {
        let sources: HashMap<&str, &str> = parsed
            .iter()
            .filter(|p| p.project == project)
            .map(|p| (p.path.as_str(), p.source.as_str()))
            .collect();
        emit_model(&model, project.as_deref(), &sources, &mut graph, &mut stats);
    }
    (graph, stats)
}

fn compile_file(
    p: &Parsed,
    ctx: &Context,
    graph: &mut KirGraph,
    stats: &mut JavaStats,
    seen: &mut HashSet<KirId>,
) {
    let project = p.project.as_deref();
    let file_id = file_kir_id(&p.path, project);
    let mut type_ids: Vec<KirId> = Vec::with_capacity(p.file.types.len());
    for (idx, t) in p.file.types.iter().enumerate() {
        let type_id = java_type_kir_id(&t.qualified, project);
        type_ids.push(type_id);
        if !seen.insert(type_id) {
            // The same qualified name declared twice (two copies of a class in one tree): the
            // first, in path order, is the object; the second still gets nothing invented.
            continue;
        }
        stats.types_total += 1;
        let mut obj = type_object(p, t, type_id);
        let parent_id = t.parent.map_or(file_id, |pi| type_ids[pi]);
        graph.add_relationship(KirRelationship::deterministic(
            RelationshipKind::Contains,
            parent_id,
            type_id,
            "",
        ));
        add_supertypes(p, idx, type_id, ctx, graph, &mut obj);
        if t.parent.is_none() {
            add_imports(p, type_id, &ctx.index, graph, &mut obj);
        }
        if p.generated {
            stats.generated_types += 1;
            obj.properties
                .insert("generated".into(), serde_json::json!(true));
            obj.properties
                .insert("member_count".into(), serde_json::json!(t.methods.len()));
        } else {
            let constants = literal_constants(&p.file, t);
            if !constants.is_empty() {
                obj.properties
                    .insert("constants".into(), serde_json::json!(constants));
            }
            add_methods(p, idx, type_id, ctx, graph, stats);
        }
        let own = type_tickets(p, idx);
        if !own.is_empty() {
            obj.properties
                .insert("tickets".into(), serde_json::json!(own));
        }
        crate::source_evidence::attach(&mut obj, &p.source, &p.path, graph);
        graph.add_object(obj);
    }
}

/// The `JavaType` object's own properties.
fn type_object(p: &Parsed, t: &TypeDecl, type_id: KirId) -> KirObject {
    let mut obj = KirObject::new(&t.name, ObjectKind::Custom(JAVA_TYPE_KIND.into()))
        .with_property("qualified_name", serde_json::json!(t.qualified))
        .with_property("type_kind", serde_json::json!(t.kind.as_str()))
        .with_property("visibility", serde_json::json!(visibility(&t.modifiers)))
        .with_property("source_span", span(t.start_line, t.end_line))
        .with_property("parse_status", serde_json::json!(p.file.status.as_str()));
    obj.id = type_id;
    let mut put = |k: &str, v: serde_json::Value| {
        obj.properties.insert(k.into(), v);
    };
    if let Some(pkg) = &p.file.package {
        put("package", serde_json::json!(pkg));
    }
    if !t.modifiers.is_empty() {
        put("modifiers", serde_json::json!(t.modifiers));
    }
    if !t.annotations.is_empty() {
        let names: Vec<&str> = t.annotations.iter().map(|a| a.name.as_str()).collect();
        put("annotations", serde_json::json!(names));
    }
    if !t.extends.is_empty() {
        put("extends", serde_json::json!(t.extends));
    }
    if !t.implements.is_empty() {
        put("implements", serde_json::json!(t.implements));
    }
    let d = t.doc.as_deref().map(doc_description).unwrap_or_default();
    if !d.is_empty() {
        put("description", serde_json::json!(d));
    }
    obj
}

/// `Extends` / `Implements` to supertypes in the tree; the rest listed as `external_supertypes`.
fn add_supertypes(
    p: &Parsed,
    idx: usize,
    type_id: KirId,
    ctx: &Context,
    graph: &mut KirGraph,
    obj: &mut KirObject,
) {
    let t = &p.file.types[idx];
    let chain = enclosing(&p.file, idx);
    let outer = &chain[..chain.len() - 1];
    let mut external: Vec<String> = Vec::new();
    // An interface's `extends` are interfaces it extends; a class's is its one superclass.
    let supers = t
        .extends
        .iter()
        .map(|n| (n, RelationshipKind::Extends))
        .chain(
            t.implements
                .iter()
                .map(|n| (n, RelationshipKind::Custom("Implements".into()))),
        );
    for (name, kind) in supers {
        let Some(q) = resolve(name, &p.file, outer, &p.project, &ctx.index) else {
            external.push(name.clone());
            continue;
        };
        graph.add_relationship(KirRelationship::deterministic(
            kind,
            type_id,
            java_type_kir_id(&q, p.project.as_deref()),
            "",
        ));
    }
    if !external.is_empty() {
        obj.properties
            .insert("external_supertypes".into(), serde_json::json!(external));
    }
}

/// The ticket ids a type keeps: those not claimed by one of its methods (which are objects of
/// their own unless the file is generated) or nested types. A file's first top-level type also
/// keeps those in the file header before it.
fn type_tickets(p: &Parsed, idx: usize) -> Vec<String> {
    let tickets = ticket_lines(&p.file);
    if tickets.is_empty() {
        return Vec::new();
    }
    let t = &p.file.types[idx];
    let mut inner: Vec<(u32, u32)> = p
        .file
        .types
        .iter()
        .filter(|o| o.parent == Some(idx))
        .map(|o| (o.start_line, o.end_line))
        .collect();
    if !p.generated {
        inner.extend(t.methods.iter().map(|m| (m.start_line, m.end_line)));
    }
    let start = if idx == 0 { 1 } else { t.start_line };
    tickets_for(&tickets, (start, t.end_line), &inner)
}

/// `DependsOn` from a file's top-level type to every imported type in the tree; the rest listed.
fn add_imports(
    p: &Parsed,
    type_id: KirId,
    index: &TypeIndex,
    graph: &mut KirGraph,
    obj: &mut KirObject,
) {
    let project = p.project.as_deref();
    let mut external = Vec::new();
    let mut targets = BTreeSet::new();
    for imp in &p.file.imports {
        if imp.is_static || imp.wildcard {
            continue;
        }
        if index.contains_key(&(p.project.clone(), imp.path.clone())) {
            targets.insert(imp.path.clone());
        } else {
            external.push(imp.path.clone());
        }
    }
    for q in targets {
        let to = java_type_kir_id(&q, project);
        if to != type_id {
            graph.add_relationship(KirRelationship::deterministic(
                RelationshipKind::DependsOn,
                type_id,
                to,
                "import",
            ));
        }
    }
    let on_demand: Vec<&str> = p
        .file
        .imports
        .iter()
        .filter(|i| i.wildcard || i.is_static)
        .map(|i| i.path.as_str())
        .collect();
    if !external.is_empty() {
        obj.properties
            .insert("external_imports".into(), serde_json::json!(external));
    }
    if !on_demand.is_empty() {
        obj.properties
            .insert("on_demand_imports".into(), serde_json::json!(on_demand));
    }
}

/// [`literal_constant_map`] as a property value.
fn literal_constants(file: &JavaFile, t: &TypeDecl) -> serde_json::Map<String, serde_json::Value> {
    literal_constant_map(file, t)
        .into_iter()
        .map(|(k, v)| (k, serde_json::json!(v)))
        .collect()
}

/// `static final` fields (implicitly so in interfaces) whose initialiser is a literal.
fn literal_constant_map(file: &JavaFile, t: &TypeDecl) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for f in &t.fields {
        let is_const =
            f.modifiers.iter().any(|m| m == "static") && f.modifiers.iter().any(|m| m == "final");
        if !is_const || f.name == "serialVersionUID" {
            continue;
        }
        if let Some(v) = f
            .init
            .and_then(|r| app_model::literal_value(&file.tokens, r))
        {
            out.insert(f.name.clone(), v);
        }
    }
    out
}

fn add_methods(
    p: &Parsed,
    idx: usize,
    type_id: KirId,
    ctx: &Context,
    graph: &mut KirGraph,
    stats: &mut JavaStats,
) {
    let t = &p.file.types[idx];
    let lookup = Lookup {
        ctx,
        file: &p.file,
        chain: enclosing(&p.file, idx),
        project: &p.project,
    };
    let tickets = ticket_lines(&p.file);
    let mut seen = HashSet::new();
    for m in &t.methods {
        let id = java_method_kir_id(type_id, &m.key());
        if !seen.insert(id) {
            continue;
        }
        stats.methods_total += 1;
        let mut obj = KirObject::new(&m.name, ObjectKind::Custom(JAVA_METHOD_KIND.into()))
            .with_property("symbol_kind", serde_json::json!(m.kind.as_str()))
            .with_property("visibility", serde_json::json!(visibility(&m.modifiers)))
            .with_property("signature", serde_json::json!(m.signature))
            .with_property("source_span", span(m.start_line, m.end_line))
            .with_property("owner", serde_json::json!(t.qualified));
        obj.id = id;
        if m.modifiers.iter().any(|x| x == "static") {
            obj.properties
                .insert("static".into(), serde_json::json!(true));
        }
        if let Some(r) = &m.return_type {
            obj.properties
                .insert("return_type".into(), serde_json::json!(r));
        }
        if !m.annotations.is_empty() {
            let names: Vec<&str> = m.annotations.iter().map(|a| a.name.as_str()).collect();
            obj.properties
                .insert("annotations".into(), serde_json::json!(names));
        }
        if let Some(doc) = &m.doc {
            let d = doc_description(doc);
            if !d.is_empty() {
                obj.properties
                    .insert("description".into(), serde_json::json!(d));
            }
        }
        if t.kind == TypeKind::Class
            && app_model::LIFECYCLE_METHODS.contains(&m.name.as_str())
            && t.implements
                .iter()
                .any(|i| i.rsplit('.').next() == Some("DocAction"))
        {
            obj.properties
                .insert("document_lifecycle".into(), serde_json::json!(true));
        }
        let own = tickets_for(&tickets, (m.start_line, m.end_line), &[]);
        if !own.is_empty() {
            obj.properties
                .insert("tickets".into(), serde_json::json!(own));
        }
        if let Some(body) = m.body {
            let statements = java_sql::harvest(&p.file.tokens, body, &lookup);
            if !statements.is_empty() {
                link_sql(&statements, id, &p.project, ctx, graph, stats);
                obj.properties
                    .insert("sql".into(), serde_json::json!(statements));
            }
        }
        crate::source_evidence::attach(&mut obj, &p.source, &p.path, graph);
        graph.add_object(obj);
        graph.add_relationship(KirRelationship::deterministic(
            RelationshipKind::Contains,
            type_id,
            id,
            "",
        ));
    }
}

/// The operations and lines behind one method → table edge.
type Link<'a> = (BTreeSet<&'a str>, BTreeSet<u32>);

/// Count a method's statements and link it to the `ModelTable`s they name: `ReadsFrom` for
/// reads, `WritesTo` (with the operations) for inserts, updates and deletes. A table name with no
/// `ModelTable` stays only in the method's `sql` property.
fn link_sql(
    statements: &[EmbeddedSql],
    method_id: KirId,
    project: &Option<String>,
    ctx: &Context,
    graph: &mut KirGraph,
    stats: &mut JavaStats,
) {
    // (kind, table) → (operations, lines)
    let mut links: BTreeMap<(&str, String), Link> = BTreeMap::new();
    for st in statements {
        stats.sql_statements += 1;
        match st.status {
            "parsed" => stats.sql_parsed += 1,
            "partial" => stats.sql_partial += 1,
            _ => stats.sql_unparsed += 1,
        }
        let ops = [
            ("ReadsFrom", "read", &st.reads),
            ("WritesTo", "insert", &st.inserts),
            ("WritesTo", "update", &st.updates),
            ("WritesTo", "delete", &st.deletes),
        ];
        for (kind, op, names) in ops {
            for name in names {
                let bare = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
                if let Some(table) = ctx.tables.get(&(project.clone(), bare)) {
                    let e = links.entry((kind, table.clone())).or_default();
                    e.0.insert(op);
                    e.1.insert(st.line);
                }
            }
        }
    }
    for ((kind, table), (ops, lines)) in links {
        stats.sql_links += 1;
        let mut rel = KirRelationship::deterministic(
            RelationshipKind::Custom(kind.into()),
            method_id,
            model_table_kir_id(&table, project.as_deref()),
            "sql",
        );
        rel.properties
            .insert("operations".into(), serde_json::json!(ops));
        rel.properties
            .insert("lines".into(), serde_json::json!(lines));
        rel.properties
            .insert("via".into(), serde_json::json!("embedded_sql"));
        graph.add_relationship(rel);
    }
}

fn emit_model(
    model: &AppModel,
    project: Option<&str>,
    sources: &HashMap<&str, &str>,
    graph: &mut KirGraph,
    stats: &mut JavaStats,
) {
    let lifecycles: HashMap<&str, &app_model::Lifecycle> = model
        .lifecycles
        .iter()
        .map(|l| (l.class.as_str(), l))
        .collect();
    for (name, t) in &model.tables {
        let id = model_table_kir_id(name, project);
        stats.model_tables += 1;
        stats.model_columns += t.columns.len();
        let mut obj = KirObject::new(name, ObjectKind::Custom(MODEL_TABLE_KIND.into()))
            .with_property("table_name", serde_json::json!(name))
            .with_property("family", serde_json::json!(app_model::FAMILY))
            .with_property("columns", serde_json::json!(t.columns))
            .with_property("column_count", serde_json::json!(t.columns.len()))
            .with_property("claim", serde_json::json!("code"));
        obj.id = id;
        if let Some(tid) = t.table_id {
            obj.properties
                .insert("table_id".into(), serde_json::json!(tid));
        }
        if let Some(level) = &t.access_level {
            obj.properties
                .insert("access_level".into(), serde_json::json!(level));
        }
        let mut classes: Vec<(&str, &str)> = Vec::new();
        if let Some(c) = &t.model_class {
            obj.properties
                .insert("model_class".into(), serde_json::json!(c));
            classes.push((c, "generated_class"));
        }
        if let Some(i) = &t.interface {
            obj.properties
                .insert("interface".into(), serde_json::json!(i));
            classes.push((i, "generated_interface"));
        }
        if !t.business_classes.is_empty() {
            obj.properties.insert(
                "business_classes".into(),
                serde_json::json!(t.business_classes),
            );
        }
        let lifecycle: Vec<serde_json::Value> = t
            .business_classes
            .iter()
            .filter_map(|c| lifecycles.get(c.as_str()))
            .map(|l| {
                serde_json::json!({
                    "class": l.class,
                    "methods": l.methods.iter().map(|(m, s, e)| serde_json::json!({"method": m, "start_line": s, "end_line": e})).collect::<Vec<_>>(),
                })
            })
            .collect();
        if !lifecycle.is_empty() {
            obj.properties
                .insert("document_lifecycle".into(), serde_json::json!(lifecycle));
        }
        for c in &t.business_classes {
            classes.push((c, "business_class"));
        }
        for (class, role) in classes {
            let mut rel = KirRelationship::deterministic(
                RelationshipKind::Custom("Models".into()),
                java_type_kir_id(class, project),
                id,
                role,
            );
            rel.properties
                .insert("role".into(), serde_json::json!(role));
            graph.add_relationship(rel);
        }
        for (col, c) in &t.columns {
            let Some(target) = &c.references else {
                continue;
            };
            if !model.tables.contains_key(target) {
                continue;
            }
            stats.model_references += 1;
            let mut rel = KirRelationship::deterministic(
                RelationshipKind::References,
                id,
                model_table_kir_id(target, project),
                col,
            );
            rel.properties
                .insert("column".into(), serde_json::json!(col));
            rel.properties
                .insert("declared_by".into(), serde_json::json!("code"));
            graph.add_relationship(rel);
        }
        if let Some((path, line)) = &t.evidence {
            let fragment = sources
                .get(path.as_str())
                .map(|s| {
                    crate::source_evidence::slice_lines(s, u64::from(*line), u64::from(*line) + 2)
                })
                .unwrap_or_default();
            let ev = KirEvidence::new(SourceLocation::at(path, *line), fragment);
            obj.evidence.push(ev.id);
            graph.add_evidence(ev);
            obj.properties
                .insert("source_path".into(), serde_json::json!(path));
        }
        graph.add_object(obj);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(path: &str, source: &str) -> Parsed {
        let file = java_syntax::parse(source);
        let generated = is_generated(&file);
        Parsed {
            path: path.into(),
            project: None,
            source: source.into(),
            file,
            generated,
        }
    }

    fn kind_of(o: &KirObject) -> &str {
        match &o.kind {
            ObjectKind::Custom(k) => k,
            _ => "",
        }
    }

    fn rels<'a>(g: &'a KirGraph, kind: &str) -> Vec<&'a KirRelationship> {
        g.relationships
            .iter()
            .filter(|r| r.kind.to_string() == kind)
            .collect()
    }

    const BASE: &str = "package org.x;\nimport java.util.List;\nimport org.x.util.Helper;\n\
        /** A base thing. */\npublic abstract class Base implements Runnable, Marker {\n\
        public static final String NAME = \"base\" + \"-\" + \"x\";\n\
        protected List<String> items;\n\
        /** Run it.\n @param x ignored */\n\
        public void run() { }\n\
        public void run(int x) { }\n\
        private static class Inner extends Base { }\n}\n";
    const MARKER: &str = "package org.x;\npublic interface Marker { }\n";
    const HELPER: &str =
        "package org.x.util;\nimport org.x.*;\npublic class Helper extends Base { Helper() { } }\n";

    #[test]
    fn types_methods_and_edges_resolve_only_inside_the_tree() {
        let files = vec![
            parsed("src/org/x/Base.java", BASE),
            parsed("src/org/x/Marker.java", MARKER),
            parsed("src/org/x/util/Helper.java", HELPER),
        ];
        let (g, stats) = compile(&files);
        assert_eq!(stats.types_total, 4);
        assert_eq!(stats.methods_total, 3, "two overloads of run + Helper()");

        let base = g.objects.iter().find(|o| o.name == "Base").unwrap();
        assert_eq!(base.properties["qualified_name"], "org.x.Base");
        assert_eq!(base.properties["description"], "A base thing.");
        assert_eq!(
            base.properties["external_supertypes"],
            serde_json::json!(["Runnable"])
        );
        assert_eq!(
            base.properties["external_imports"],
            serde_json::json!(["java.util.List"])
        );
        assert_eq!(base.properties["constants"]["NAME"], "base-x");
        assert!(!base.evidence.is_empty());

        let base_id = java_type_kir_id("org.x.Base", None);
        let marker_id = java_type_kir_id("org.x.Marker", None);
        let helper_id = java_type_kir_id("org.x.util.Helper", None);
        let inner_id = java_type_kir_id("org.x.Base.Inner", None);
        let implements = rels(&g, "Implements");
        assert_eq!(implements.len(), 1);
        assert_eq!((implements[0].from, implements[0].to), (base_id, marker_id));
        let extends: Vec<(KirId, KirId)> =
            rels(&g, "Extends").iter().map(|r| (r.from, r.to)).collect();
        assert!(
            extends.contains(&(inner_id, base_id)),
            "nested type resolves its outer"
        );
        assert!(
            extends.contains(&(helper_id, base_id)),
            "on-demand import resolves"
        );
        let deps: Vec<(KirId, KirId)> = rels(&g, "DependsOn")
            .iter()
            .map(|r| (r.from, r.to))
            .collect();
        assert_eq!(deps, vec![(base_id, helper_id)]);

        let contains = rels(&g, "Contains");
        assert!(
            contains
                .iter()
                .any(|r| r.from == base_id && r.to == inner_id)
        );
        let run: Vec<&KirObject> = g.objects.iter().filter(|o| o.name == "run").collect();
        assert_eq!(run.len(), 2);
        assert_ne!(run[0].id, run[1].id);
        let documented = run
            .iter()
            .find(|o| o.properties.contains_key("description"))
            .unwrap();
        assert_eq!(documented.properties["description"], "Run it.");
        assert_eq!(documented.properties["source_span"]["start_line"], 10);
        let ctor = g
            .objects
            .iter()
            .find(|o| o.name == "Helper" && kind_of(o) == JAVA_METHOD_KIND)
            .unwrap();
        assert_eq!(ctor.properties["symbol_kind"], "constructor");
        assert_eq!(ctor.properties["visibility"], "package");
    }

    #[test]
    fn ticket_ids_anchor_to_the_innermost_declaration() {
        assert_eq!(
            tickets_in(
                "// IDEMPIERE-2064 - compat before IDEMPIERE-2477; XIDEMPIERE-1 and IDEMPIERE-x"
            ),
            vec!["IDEMPIERE-2064", "IDEMPIERE-2477"]
        );
        let src = "/** @author d-ruiz - IDEMPIERE-7060 */\npackage p;\npublic class A {\n\
            void m() {\n  x(); // IDEMPIERE-913\n}\n\
            /* ADEMPIERE-12 */ int f;\n}\n";
        let (g, _) = compile(&[parsed("A.java", src)]);
        let a = g.objects.iter().find(|o| o.name == "A").unwrap();
        assert_eq!(
            a.properties["tickets"],
            serde_json::json!(["ADEMPIERE-12", "IDEMPIERE-7060"])
        );
        let m = g.objects.iter().find(|o| o.name == "m").unwrap();
        assert_eq!(
            m.properties["tickets"],
            serde_json::json!(["IDEMPIERE-913"])
        );
    }

    /// The kinds are built from constants, which `every_pipeline_custom_kind_is_registered` (a
    /// source scan for `Custom("…")` literals) cannot see — so this checks them directly.
    #[test]
    fn every_java_kind_has_a_keyed_registry_row() {
        for kind in [JAVA_TYPE_KIND, JAVA_METHOD_KIND, MODEL_TABLE_KIND] {
            assert!(
                ekos_kir::custom_kinds::is_structurally_keyed(kind),
                "{kind} must be registered as structurally keyed"
            );
        }
    }

    #[test]
    fn output_does_not_depend_on_file_order() {
        let a = vec![
            parsed("B.java", BASE),
            parsed("M.java", MARKER),
            parsed("H.java", HELPER),
        ];
        let mut b = vec![
            parsed("H.java", HELPER),
            parsed("B.java", BASE),
            parsed("M.java", MARKER),
        ];
        b.sort_by(|x, y| x.path.cmp(&y.path));
        let mut a = a;
        a.sort_by(|x, y| x.path.cmp(&y.path));
        let ids = |g: &KirGraph| {
            let mut v: Vec<KirId> = g.objects.iter().map(|o| o.id).collect();
            v.extend(g.relationships.iter().map(|r| r.id));
            v.sort_by_key(|k| k.0);
            v
        };
        assert_eq!(ids(&compile(&a).0), ids(&compile(&b).0));
    }

    #[test]
    fn generated_files_yield_types_and_model_tables_not_accessor_methods() {
        let i = "package org.compiere.model;\n/** Generated Interface for C_Order */\npublic interface I_C_Order {\n\
            public static final String Table_Name = \"C_Order\";\n public static final int Table_ID = 259;\n\
            public static final String COLUMNNAME_C_BPartner_ID = \"C_BPartner_ID\";\n\
            public int getC_BPartner_ID();\n\
            public org.compiere.model.I_C_BPartner getC_BPartner() throws RuntimeException;\n}\n";
        let ib = "package org.compiere.model;\n/** Generated Interface for C_BPartner */\npublic interface I_C_BPartner {\n\
            public static final String Table_Name = \"C_BPartner\";\n public static final int Table_ID = 291;\n\
            public static final String COLUMNNAME_Name = \"Name\";\n}\n";
        let x = "/** Generated Model - DO NOT CHANGE */\npackage org.compiere.model;\n\
            @org.adempiere.base.Model(table=\"C_Order\")\npublic class X_C_Order extends PO implements I_C_Order {\n\
            /** Set Business Partner.\n @param C_BPartner_ID Identifies a Business Partner */\n\
            public void setC_BPartner_ID (int C_BPartner_ID) { set_Value (COLUMNNAME_C_BPartner_ID, C_BPartner_ID); }\n}\n";
        let m = "package org.compiere.model;\npublic class MOrder extends X_C_Order implements DocAction {\n\
            public String completeIt() { return null; }\n\
            void purge() { DB.executeUpdateEx(\"DELETE FROM \" + Table_Name + \" WHERE Processed='Y'\", null);\n\
              new Query(getCtx(), I_C_BPartner.Table_Name, null, null).list(); }\n}\n";
        let files = vec![
            parsed("I_C_BPartner.java", ib),
            parsed("I_C_Order.java", i),
            parsed("MOrder.java", m),
            parsed("X_C_Order.java", x),
        ];
        let (g, stats) = compile(&files);
        assert_eq!(stats.generated_types, 3);
        assert_eq!(stats.methods_total, 2, "only MOrder's own methods");
        assert_eq!(stats.sql_statements, 2);
        assert_eq!(stats.sql_parsed, 2);
        assert_eq!(stats.model_tables, 2);

        let order = g
            .objects
            .iter()
            .find(|o| kind_of(o) == MODEL_TABLE_KIND && o.name == "C_Order")
            .unwrap();
        assert_eq!(order.properties["table_id"], 259);
        let col = &order.properties["columns"]["C_BPartner_ID"];
        assert_eq!(col["business_name"], "Business Partner");
        assert_eq!(col["references"], "C_BPartner");
        assert_eq!(
            order.properties["business_classes"],
            serde_json::json!(["org.compiere.model.MOrder"])
        );
        assert_eq!(
            order.properties["document_lifecycle"][0]["methods"][0]["method"],
            "completeIt"
        );
        assert!(!order.evidence.is_empty());

        let refs = rels(&g, "References");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to, model_table_kir_id("C_BPartner", None));
        assert_eq!(refs[0].properties["column"], "C_BPartner_ID");
        let models = rels(&g, "Models");
        assert_eq!(
            models.len(),
            4,
            "X_, I_ and M for C_Order; I_ for C_BPartner"
        );
        let x_type = g.objects.iter().find(|o| o.name == "X_C_Order").unwrap();
        assert_eq!(x_type.properties["generated"], true);
        // `Table_Name` resolves through MOrder → X_C_Order → I_C_Order; the edges land on the
        // ModelTables, not on fabricated ones.
        let purge = g.objects.iter().find(|o| o.name == "purge").unwrap();
        let writes = rels(&g, "WritesTo");
        assert_eq!(writes.len(), 1);
        assert_eq!(
            (writes[0].from, writes[0].to),
            (purge.id, model_table_kir_id("C_Order", None))
        );
        assert_eq!(
            writes[0].properties["operations"],
            serde_json::json!(["delete"])
        );
        let reads = rels(&g, "ReadsFrom");
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].to, model_table_kir_id("C_BPartner", None));
        assert_eq!(purge.properties["sql"][0]["status"], "parsed");
        let complete = g.objects.iter().find(|o| o.name == "completeIt").unwrap();
        assert_eq!(complete.properties["document_lifecycle"], true);
    }

    /// `EKOS_JAVA_CORPUS=<dir> cargo test -p ekos-recovery java_analyzer_corpus -- --ignored
    /// --nocapture`: the whole pass over a real tree, with its counters and samples of what the
    /// SQL harvester could not parse.
    #[test]
    #[ignore = "needs EKOS_JAVA_CORPUS"]
    fn java_analyzer_corpus() {
        let Ok(root) = std::env::var("EKOS_JAVA_CORPUS") else {
            return;
        };
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("java") {
                let bytes = std::fs::read(path).unwrap();
                let rel = path.strip_prefix(&root).unwrap().display().to_string();
                files.push(parsed(&rel, &String::from_utf8_lossy(&bytes)));
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let (g, stats) = compile(&files);
        println!("{stats:#?}");
        let mut errors: BTreeMap<String, usize> = BTreeMap::new();
        let mut with_tickets = 0;
        for o in &g.objects {
            with_tickets += usize::from(o.properties.contains_key("tickets"));
            let Some(sql) = o.properties.get("sql").and_then(|v| v.as_array()) else {
                continue;
            };
            for st in sql {
                if let Some(e) = st.get("error").and_then(|e| e.as_str()) {
                    let key: String = e
                        .split(" at Line")
                        .next()
                        .unwrap_or(e)
                        .chars()
                        .take(70)
                        .collect();
                    *errors.entry(key).or_default() += 1;
                }
            }
        }
        println!("objects with tickets: {with_tickets}");
        let mut errors: Vec<_> = errors.into_iter().collect();
        errors.sort_by_key(|e| std::cmp::Reverse(e.1));
        for (e, n) in errors.iter().take(12) {
            println!("  {n:>5} {e}");
        }
    }
}
