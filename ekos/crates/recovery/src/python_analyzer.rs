//! `PythonAnalyzerPass` — converts Python/PySpark observation artifacts (RFC 0038/0040 Phase 2)
//! into the Transformation IR (RFC 0027) for recognized DataFrame method chains, plus plain KIR
//! objects for imports and function/class definitions — real AST parsing via `rustpython-parser`,
//! no LLM in the loop, same shape as `PentahoAnalyzerPass`.
//!
//! Scope, deliberately narrower than "understand a whole notebook's pipeline" (RFC 0040's
//! Context section, grounded against a real Databricks Asset Bundle repo, not assumed):
//! - DataFrame method chains are recognized *within one statement's expression* — a top-level
//!   module statement, or a statement directly inside one function body (not nested deeper, and
//!   never traced across function/file call boundaries — real PySpark repos concentrate business
//!   logic in shared library functions notebooks call, which this phase does not follow into).
//! - `spark.sql(...)` calls are never parsed as SQL — the argument is very often an f-string
//!   with `{var}` interpolation, which isn't valid SQL syntax, so guessing a substitution would
//!   risk a wrong answer. Always `Unmapped` with the raw argument text.
//! - Imports become `KirObject(Custom("PythonModule"))` + `DependsOn` edges from the file's own
//!   `KirId` (same UUIDv5-over-relative-path scheme `build.rs` already uses for `ObjectKind::File`
//!   objects, so these attach to the real existing file object rather than duplicating it).
//! - Function/class defs become `KirObject(Custom("PythonSymbol"))` + `Contains` edges from the
//!   file — a real upgrade over `plugins/file`'s substring-based `harvest_symbols` scan.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_semantic::merge_graphs;
use ekos_semantic::transform_ir::{
    AggExpr, JoinKind, NodeId, TransformGraph, TransformNode, TransformOrigin, lower_to_kir,
};
use rustpython_parser::Parse;
use rustpython_parser::ast::{self, Ranged};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct PythonArtifactData {
    path: String,
    source: String,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace (`build.rs`'s own choke
    /// point). Qualifies id hashing only — `path` above stays bare everywhere it's displayed.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run, mirroring `PentahoStats`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonStats {
    pub files_processed: usize,
    pub nodes_total: usize,
    /// Non-`Unmapped` nodes.
    pub nodes_mapped: usize,
}

impl PythonStats {
    pub fn coverage_percent(&self) -> f32 {
        if self.nodes_total == 0 {
            0.0
        } else {
            100.0 * self.nodes_mapped as f32 / self.nodes_total as f32
        }
    }
}

pub struct PythonAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<PythonStats>>,
}

impl PythonAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("python-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(PythonStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<PythonStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for PythonAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = PythonStats::default();
        // Dedup Python-module target objects across files within this one run — many files can
        // import the same module; the ledger itself also dedupes by id on append, but keeping
        // this pass's own output lean mirrors `dependency_analyzer.rs`'s "create/reuse" Technology
        // object discipline.
        let mut seen_modules: HashSet<KirId> = HashSet::new();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "PYTHON001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: PythonArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "PYTHON002",
                        format!("malformed python payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            // RFC 0079: `parse_python_file`'s `path` feeds both `add_symbol`'s id hash *and*
            // `TransformOrigin.source_path` (a real displayed label, `"{path}#{index}"`, shown in
            // `SequenceDiagrams.md` and used as `transform_node_kir_id`'s own hash input) — unlike
            // `rust_analyzer.rs`, there's no id-only/display-only split to preserve here, so the
            // qualified path is passed through everywhere: correct for id-safety, and arguably an
            // improvement for the display case too (distinguishes which project's pipeline a
            // sequence belongs to in a multi-project workspace); a no-op change for every
            // single-project workspace, where `id_path == data.path`.
            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_path.as_bytes()));
            let result = match parse_python_file(&id_path, &data.source, file_id) {
                Ok(r) => r,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("PYTHON003", format!("cannot parse {}: {e}", data.path));
                    continue;
                }
            };

            stats.files_processed += 1;
            for graph in &result.transform_graphs {
                stats.nodes_total += graph.nodes.len();
                stats.nodes_mapped += graph
                    .nodes
                    .iter()
                    .filter(|n| !matches!(n, TransformNode::Unmapped { .. }))
                    .count();
                merge_graphs(&mut combined, lower_to_kir(graph));
            }
            for obj in result.objects {
                if seen_modules.insert(obj.id)
                    || !matches!(obj.kind, ObjectKind::Custom(ref k) if k == "PythonModule")
                {
                    combined.add_object(obj);
                }
            }
            for rel in result.relationships {
                combined.add_relationship(rel);
            }
            for ev in result.evidence {
                combined.add_evidence(ev);
            }
        }

        *self.stats.lock().unwrap() = stats;

        if combined.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], combined);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            files = stats.files_processed,
            nodes = stats.nodes_total,
            mapped = stats.nodes_mapped,
            coverage_pct = stats.coverage_percent(),
            "python-analyzer complete"
        );
        Ok(())
    }
}

// ── Parsing ──────────────────────────────────────────────────────────────────

struct PythonFileResult {
    transform_graphs: Vec<TransformGraph>,
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    evidence: Vec<KirEvidence>,
}

fn parse_python_file(
    path: &str,
    source: &str,
    file_id: KirId,
) -> Result<PythonFileResult, rustpython_parser::ParseError> {
    let stmts = ast::Suite::parse(source, path)?;
    let mut result = PythonFileResult {
        transform_graphs: Vec::new(),
        objects: Vec::new(),
        relationships: Vec::new(),
        evidence: Vec::new(),
    };
    let mut graph_index = 0usize;

    // RFC 0091: real `__tablename__`s declared anywhere in this file, collected before the main
    // walk so a `ForeignKey("other_table.col")` resolves regardless of whether `other_table`'s
    // own class appears earlier or later in the file (real shape: `pdf-reader`'s `db/models.py`
    // declares `Document` before `PageCache`, which references it — but the reverse order is
    // equally valid Python).
    let known_tables: HashMap<String, KirId> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            ast::Stmt::ClassDef(c) => {
                extract_tablename(&c.body).map(|t| (t.clone(), orm_table_kir_id(&t)))
            }
            _ => None,
        })
        .collect();

    // RFC 0092: every real class declared in this file, collected before the main walk for the
    // same reason as `known_tables` above — a base class can legitimately appear either before or
    // after its subclass in real source.
    let known_classes: HashMap<String, KirId> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            ast::Stmt::ClassDef(c) => Some((
                c.name.to_string(),
                python_symbol_kir_id(path, c.name.as_str()),
            )),
            _ => None,
        })
        .collect();

    for stmt in &stmts {
        walk_top_level_statement(
            stmt,
            path,
            source,
            file_id,
            &mut result,
            &mut graph_index,
            &known_tables,
            &known_classes,
        );
    }

    Ok(result)
}

fn python_module_kir_id(module: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("python-module:{module}").as_bytes(),
    ))
}

fn add_import(module: &str, file_id: KirId, result: &mut PythonFileResult) {
    let target_id = python_module_kir_id(module);
    let mut obj = KirObject::new(module, ObjectKind::Custom("PythonModule".to_string()));
    obj.id = target_id;
    result.objects.push(obj);
    result.relationships.push(KirRelationship::new(
        RelationshipKind::DependsOn,
        file_id,
        target_id,
    ));
}

/// Deterministic id for a `Custom("PythonSymbol")` object — shared by `add_symbol` (which mints
/// it for the object it creates) and RFC 0092's `known_classes` pre-pass (which needs the same id
/// for a class *before* `add_symbol` runs for it, to resolve a same-file `Extends` base
/// regardless of declaration order).
fn python_symbol_kir_id(path: &str, name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("python-symbol:{path}:{name}").as_bytes(),
    ))
}

/// Deterministic id for a real `Extends` edge (RFC 0092), keyed by `(from, to)` — a class extends
/// a given base as a boolean fact, matching `crate_topology_analyzer.rs`'s `depends_on_kir_id`
/// precedent (RFC 0070/0071's fix for the failure mode a non-deterministic relationship id causes:
/// unbounded duplicate accumulation across repeated `recover` runs).
fn extends_kir_id(from: KirId, to: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("extends:{from}:{to}").as_bytes(),
    ))
}

fn add_symbol(
    name: &str,
    kind: &str,
    path: &str,
    file_id: KirId,
    result: &mut PythonFileResult,
    doc: Option<String>,
    span: Option<(u32, u32)>,
) {
    let target_id = python_symbol_kir_id(path, name);
    let mut obj = KirObject::new(name, ObjectKind::Custom("PythonSymbol".to_string()))
        .with_property("kind", serde_json::Value::String(kind.to_string()));
    obj.id = target_id;
    // Real, only when the body's own first statement is a real string-literal docstring — never
    // fabricated.
    if let Some(doc) = doc {
        obj.properties
            .insert("description".into(), serde_json::json!(doc));
    }
    // RFC 0088 (fast-follow — Rust/Elixir only at launch): the real `def`/`class` statement's own
    // byte range, converted to 1-indexed lines via `line_number` below, so `llm_description.rs`
    // can slice and send the real source text — without this, every Python symbol is honestly
    // skipped, never described, regardless of `[llm-description] scope`.
    if let Some((start, end)) = span {
        obj.properties.insert(
            "source_span".into(),
            serde_json::json!({"start_line": start, "end_line": end}),
        );
    }
    result.objects.push(obj);
    result.relationships.push(KirRelationship::new(
        RelationshipKind::Contains,
        file_id,
        target_id,
    ));
}

// ── RFC 0091 — SQLAlchemy ORM model recognition ─────────────────────────────

/// A real column extracted from an ORM model's class body.
struct OrmColumn {
    name: String,
    /// Best-effort — the callable name of the column-type call (`String`/`Integer`/`DateTime`/
    /// ...) when the first positional argument to `mapped_column(...)`/`Column(...)` is itself a
    /// call. Never a real SQL type-system mapping; `None` when it can't be determined this way.
    data_type: Option<String>,
    /// `(referenced table, referenced column)` from a real `ForeignKey("table.column")` string
    /// literal nested in this column's call — `None` when no `ForeignKey(...)` was found.
    fk_target: Option<(String, String)>,
}

/// Real id for an ORM-recognized `Table` — same scheme `sql_analyzer.rs::table_kir_id` uses
/// (lowercased, analyzer-prefixed so a same-named table recovered by DDL parsing and by ORM-model
/// recognition never silently collides onto one id; cross-origin recognition that they describe
/// the same real table is identity resolution's job, RFC 0029, not an accidental id match).
fn orm_table_kir_id(tablename: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("python-orm-table:{}", tablename.to_lowercase()).as_bytes(),
    ))
}

/// Same reasoning as `sql_analyzer.rs::foreign_key_kir_id` — `fk_desc` is part of the hash input,
/// not just the evidence text, so two FK columns to the same target table produce two distinct
/// real edges rather than colliding.
fn orm_foreign_key_kir_id(from: KirId, to: KirId, fk_desc: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("python-orm-fk:{from}:{to}:{fk_desc}").as_bytes(),
    ))
}

/// A real `__tablename__ = "..."` string-literal assignment in a class body — the one signal this
/// recognizes an ORM model by (RFC 0091's own Motivation: deliberately not tracing `bases` back to
/// a `declarative_base()`/`DeclarativeBase` definition, which is fragile against aliasing/
/// re-exports; `__tablename__` is unambiguous and SQLAlchemy-specific).
fn extract_tablename(body: &[ast::Stmt]) -> Option<String> {
    body.iter().find_map(|stmt| {
        let ast::Stmt::Assign(a) = stmt else {
            return None;
        };
        let ast::Expr::Name(target) = a.targets.first()? else {
            return None;
        };
        if target.id.as_str() != "__tablename__" {
            return None;
        }
        string_constant(&a.value)
    })
}

/// The callable name of `expr` when it's a `Call` — `mapped_column(String(64), ...)`'s first
/// positional arg is `Call{func: Name{"String"}, ...}`, so this gives the real `"String"` hint;
/// `sa.Integer` (qualified) resolves via the `Attribute` arm the same way `linearize_chain`
/// already navigates `Expr::Attribute` elsewhere in this file.
fn call_name(expr: &ast::Expr) -> Option<&str> {
    let ast::Expr::Call(call) = expr else {
        return None;
    };
    match call.func.as_ref() {
        ast::Expr::Name(n) => Some(n.id.as_str()),
        ast::Expr::Attribute(a) => Some(a.attr.as_str()),
        _ => None,
    }
}

/// Real `(table, column)` from a `ForeignKey("table.column")` call nested anywhere in `call`'s
/// positional or keyword arguments — `mapped_column(String(64), ForeignKey("documents.file_hash"))`
/// is the real shape this looks for.
fn find_fk_target(call: &ast::ExprCall) -> Option<(String, String)> {
    let candidates = call
        .args
        .iter()
        .chain(call.keywords.iter().map(|k| &k.value));
    for arg in candidates {
        if call_name(arg) == Some("ForeignKey")
            && let ast::Expr::Call(fk_call) = arg
            && let Some(target) = positional_string_arg(fk_call, 0)
            && let Some((table, column)) = target.split_once('.')
        {
            return Some((table.to_string(), column.to_string()));
        }
    }
    None
}

/// Best-effort `data_type` hint from a column-type expression — either *called*
/// (`String(64)`/`sa.String(64)`, via `call_name`) or a bare, uninstantiated type reference
/// (`Integer`, real and common SQLAlchemy usage for a type with no constructor arguments).
fn type_hint(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Call(_) => call_name(expr),
        ast::Expr::Name(n) => Some(n.id.as_str()),
        ast::Expr::Attribute(a) => Some(a.attr.as_str()),
        _ => None,
    }
}

/// Real columns from a class body — both SQLAlchemy 2.0 style (`field: Mapped[T] =
/// mapped_column(...)`, `ast::Stmt::AnnAssign`) and classic style (`field = Column(...)`,
/// `ast::Stmt::Assign`). Only a statement whose right-hand side is itself a `Call` counts — a
/// plain class-body assignment with no column-constructor call (e.g. a real class-level constant)
/// is never guessed at as a column.
fn extract_orm_columns(body: &[ast::Stmt]) -> Vec<OrmColumn> {
    let mut columns = Vec::new();
    for stmt in body {
        let (name, call_expr) = match stmt {
            ast::Stmt::AnnAssign(a) => {
                let ast::Expr::Name(n) = a.target.as_ref() else {
                    continue;
                };
                let Some(value) = &a.value else { continue };
                (n.id.as_str(), value.as_ref())
            }
            ast::Stmt::Assign(a) => {
                let Some(ast::Expr::Name(n)) = a.targets.first() else {
                    continue;
                };
                (n.id.as_str(), a.value.as_ref())
            }
            _ => continue,
        };
        let ast::Expr::Call(call) = call_expr else {
            continue;
        };
        let data_type = call.args.first().and_then(type_hint).map(|s| s.to_string());
        columns.push(OrmColumn {
            name: name.to_string(),
            data_type,
            fk_target: find_fk_target(call),
        });
    }
    columns
}

/// Adds a real `ObjectKind::Table` for a recognized ORM model, alongside (never replacing) its
/// existing `PythonSymbol` object — see this function's caller and RFC 0091's own Design section
/// for why these are two separate real objects, not one. `known_tables` (built once per file
/// before any class is walked, so forward and backward references both resolve) supplies real FK
/// targets found within *this same file*; a target not found there gets no FK edge at all —
/// honest, not a fabricated edge to a possibly-nonexistent id.
fn add_orm_table(
    tablename: &str,
    body: &[ast::Stmt],
    path: &str,
    file_id: KirId,
    known_tables: &HashMap<String, KirId>,
    result: &mut PythonFileResult,
) {
    let table_id = orm_table_kir_id(tablename);
    let columns = extract_orm_columns(body);

    let columns_json: Vec<serde_json::Value> = columns
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "data_type": c.data_type.as_deref().unwrap_or("unknown"),
            })
        })
        .collect();

    let ev = KirEvidence::new(
        SourceLocation::file(path),
        format!("class with __tablename__ = \"{tablename}\""),
    );
    let ev_id = ev.id;
    result.evidence.push(ev);

    let mut obj = KirObject::new(tablename, ObjectKind::Table)
        .with_property("columns", serde_json::Value::Array(columns_json))
        .with_evidence(ev_id);
    obj.id = table_id;
    result.objects.push(obj);
    result.relationships.push(KirRelationship::new(
        RelationshipKind::Contains,
        file_id,
        table_id,
    ));

    for column in &columns {
        let Some((ref_table, ref_column)) = &column.fk_target else {
            continue;
        };
        let Some(&to_id) = known_tables.get(ref_table) else {
            continue;
        };
        let fk_desc = format!("{tablename}.{} → {ref_table}.{ref_column}", column.name);
        let fk_ev = KirEvidence::new(SourceLocation::file(path), fk_desc.clone());
        let fk_ev_id = fk_ev.id;
        result.evidence.push(fk_ev);

        let mut rel = KirRelationship::new(RelationshipKind::ForeignKey, table_id, to_id);
        rel.id = orm_foreign_key_kir_id(table_id, to_id, &fk_desc);
        rel.properties
            .insert("fk_desc".into(), serde_json::Value::String(fk_desc));
        rel.evidence.push(fk_ev_id);
        result.relationships.push(rel);
    }
}

/// 1-indexed line number containing byte offset `offset` in `source` — counts `\n` bytes before
/// it. `rustpython_parser`'s `Ranged::range()` gives byte offsets (`TextSize`), not line/column;
/// `syn`'s `LineColumn` (what `rust_analyzer.rs`'s own `item_span` uses) has no Python equivalent
/// here, so this is the real conversion RFC 0088's `source_span` needs for a Python symbol.
fn line_number(source: &str, offset: usize) -> u32 {
    let offset = offset.min(source.len());
    1 + source.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
}

/// Real `{start_line, end_line}` for `item`'s own AST range — both ends converted via
/// `line_number`. Matches `rust_analyzer.rs`'s `item_span` in spirit (whole real declaration
/// span, decorators included since they're part of the same statement's own range in
/// `rustpython_parser`'s AST, same "don't try to exclude attached-comment-like syntax" choice
/// RFC 0087 already made for Rust's own doc-comment-inclusive span).
fn item_span<T: Ranged>(item: &T, source: &str) -> (u32, u32) {
    let range = item.range();
    (
        line_number(source, range.start().to_usize()),
        line_number(source, range.end().to_usize()),
    )
}

/// Real Python docstring extraction (Phase 1 of the "Real Descriptions, Purpose, and Links"
/// plan) — the real PEP 257 convention: a function/class's docstring is its body's own *first*
/// statement, a bare string-literal expression statement. Reuses `string_constant`, already used
/// elsewhere in this file for PySpark chain-argument recognition — the same real AST shape.
fn python_docstring(body: &[ast::Stmt]) -> Option<String> {
    let ast::Stmt::Expr(first) = body.first()? else {
        return None;
    };
    let text = string_constant(&first.value)?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_top_level_statement(
    stmt: &ast::Stmt,
    path: &str,
    source: &str,
    file_id: KirId,
    result: &mut PythonFileResult,
    graph_index: &mut usize,
    known_tables: &HashMap<String, KirId>,
    known_classes: &HashMap<String, KirId>,
) {
    match stmt {
        ast::Stmt::Import(imp) => {
            for alias in &imp.names {
                add_import(alias.name.as_str(), file_id, result);
            }
        }
        ast::Stmt::ImportFrom(imp) => {
            if let Some(module) = &imp.module {
                // `from pkg import name` binds each real, distinct `name` — resolving every one
                // to the bare `pkg` collapsed `from app.services import ai_service` and
                // `from app.services import db_service` to the same object, losing exactly the
                // distinction the real source draws. `pkg.name` is a real dotted reference the
                // source itself makes, whether `name` turns out to be a submodule (the common
                // case that motivated this fix) or a symbol re-exported from `pkg`'s `__init__` —
                // both are real depends-on facts, not fabricated. A star import has no specific
                // name to qualify with, so it falls back to the bare module, same as before.
                for alias in &imp.names {
                    let name = alias.name.as_str();
                    if name == "*" {
                        add_import(module.as_str(), file_id, result);
                    } else {
                        add_import(&format!("{module}.{name}"), file_id, result);
                    }
                }
            }
        }
        ast::Stmt::FunctionDef(f) => {
            let doc = python_docstring(&f.body);
            let span = item_span(f, source);
            add_symbol(
                f.name.as_str(),
                "function",
                path,
                file_id,
                result,
                doc,
                Some(span),
            );
            for inner in &f.body {
                try_recognize_chain_statement(inner, path, source, result, graph_index);
            }
        }
        ast::Stmt::ClassDef(c) => {
            let doc = python_docstring(&c.body);
            let span = item_span(c, source);
            add_symbol(
                c.name.as_str(),
                "class",
                path,
                file_id,
                result,
                doc,
                Some(span),
            );
            // RFC 0091: a real SQLAlchemy declarative model (`__tablename__` present) is *also*
            // compiled as a real `Table` object, alongside its existing `PythonSymbol` — the class
            // still gets its ordinary code-level representation unchanged.
            if let Some(tablename) = extract_tablename(&c.body) {
                add_orm_table(&tablename, &c.body, path, file_id, known_tables, result);
            }
            // RFC 0092: a real `Extends` edge per base class that resolves to another real,
            // same-file `PythonSymbol` class — an `Attribute` base (`orm.DeclarativeBase`) can
            // never refer to a same-file class by construction, so only `Name` bases are checked;
            // a `Name` base with no matching local class (`BaseModel`, imported, not locally
            // defined) is honestly left unmapped, not fabricated.
            let class_id = python_symbol_kir_id(path, c.name.as_str());
            for base in &c.bases {
                if let ast::Expr::Name(n) = base
                    && let Some(&base_id) = known_classes.get(n.id.as_str())
                {
                    let mut rel =
                        KirRelationship::new(RelationshipKind::Extends, class_id, base_id);
                    rel.id = extends_kir_id(class_id, base_id);
                    result.relationships.push(rel);
                }
            }
        }
        other => {
            try_recognize_chain_statement(other, path, source, result, graph_index);
        }
    }
}

fn try_recognize_chain_statement(
    stmt: &ast::Stmt,
    path: &str,
    source: &str,
    result: &mut PythonFileResult,
    graph_index: &mut usize,
) {
    let expr = match stmt {
        ast::Stmt::Assign(a) => Some(a.value.as_ref()),
        ast::Stmt::Return(r) => r.value.as_deref(),
        ast::Stmt::Expr(e) => Some(e.value.as_ref()),
        _ => None,
    };
    let Some(expr) = expr else {
        return;
    };

    let mut calls = Vec::new();
    linearize_chain(expr, &mut calls);
    let nodes = calls_to_nodes(&calls, source);
    if nodes.is_empty() {
        return;
    }

    let mut edges = Vec::new();
    for i in 0..nodes.len().saturating_sub(1) {
        edges.push((NodeId(i as u32), NodeId((i + 1) as u32)));
    }

    let origin = TransformOrigin {
        source_path: format!("{path}#{graph_index}"),
        source_kind: "python".to_string(),
        extracted_at: chrono::Utc::now(),
    };
    *graph_index += 1;

    result.transform_graphs.push(TransformGraph {
        nodes,
        edges,
        origin,
    });
}

// ── DataFrame method-chain recognition ──────────────────────────────────────

/// One `.method(...)` call in a fluent chain, in base-to-outer (source) order.
struct RawCall<'a> {
    method: &'a str,
    call: &'a ast::ExprCall,
    /// Whether the immediate receiver of this call is the bare name `spark` — needed to
    /// distinguish `spark.table(...)`/`spark.sql(...)` from an arbitrary `.table`/`.sql`-named
    /// method on some other object (unlikely in practice, but honest not to assume).
    receiver_is_spark: bool,
}

/// Unwraps a nested `Call(func=Attribute(value=<inner>))` tree into a flat, source-ordered list
/// of method calls — `df.join(x).withColumn(y)` parses AST-inside-out (the `join` call wraps the
/// base `df`, the `withColumn` call wraps the `join` call), so this recurses into the receiver
/// first and pushes after, producing base-to-outer order.
fn linearize_chain<'a>(expr: &'a ast::Expr, calls: &mut Vec<RawCall<'a>>) {
    if let ast::Expr::Call(call) = expr
        && let ast::Expr::Attribute(attr) = call.func.as_ref()
    {
        linearize_chain(&attr.value, calls);
        let receiver_is_spark =
            matches!(attr.value.as_ref(), ast::Expr::Name(n) if n.id.as_str() == "spark");
        calls.push(RawCall {
            method: attr.attr.as_str(),
            call,
            receiver_is_spark,
        });
    }
}

fn string_constant(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Constant(c) => match &c.value {
            ast::Constant::Str(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn positional_string_arg(call: &ast::ExprCall, index: usize) -> Option<String> {
    call.args.get(index).and_then(string_constant)
}

fn keyword_arg<'a>(call: &'a ast::ExprCall, name: &str) -> Option<&'a ast::Expr> {
    call.keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|i| i.as_str()) == Some(name))
        .map(|k| &k.value)
}

fn source_slice<'a>(source: &'a str, expr: &ast::Expr) -> &'a str {
    let range = expr.range();
    source
        .get(range.start().to_usize()..range.end().to_usize())
        .unwrap_or("<unavailable>")
}

/// `on=` accepts a single column name, a list of column names (an equi-join on same-named
/// columns on both sides — real PySpark shorthand, honestly represented here as the same column
/// name on both sides of the pair), or an arbitrary boolean expression (e.g.
/// `edges["src"] == vertices["id"]`) that this analyzer does not attempt to decompose into
/// column-pair keys — `keys` stays empty in that case, matching this project's "don't guess"
/// posture rather than misrepresenting a complex condition as a simple equi-join.
fn join_keys_from_on(call: &ast::ExprCall) -> Vec<(String, String)> {
    let on = match keyword_arg(call, "on").or_else(|| call.args.get(1)) {
        Some(e) => e,
        None => return Vec::new(),
    };
    match on {
        ast::Expr::Constant(_) => string_constant(on)
            .map(|s| vec![(s.clone(), s)])
            .unwrap_or_default(),
        ast::Expr::List(list) => list
            .elts
            .iter()
            .filter_map(string_constant)
            .map(|s| (s.clone(), s))
            .collect(),
        _ => Vec::new(),
    }
}

fn join_kind_from_how(call: &ast::ExprCall) -> JoinKind {
    let how = keyword_arg(call, "how")
        .or_else(|| call.args.get(2))
        .and_then(string_constant);
    match how.as_deref() {
        Some("left") | Some("leftouter") | Some("left_outer") => JoinKind::Left,
        Some("right") | Some("rightouter") | Some("right_outer") => JoinKind::Right,
        Some("outer") | Some("full") | Some("fullouter") | Some("full_outer") => JoinKind::Full,
        Some("cross") => JoinKind::Cross,
        // Real PySpark also accepts `left_anti`/`left_semi`/`semi`/`anti`, which have no
        // equivalent in this IR's fixed JoinKind vocabulary — defaulting to Inner is an accepted,
        // documented approximation (matching `pentaho_analyzer.rs`'s own `DatabaseJoin`
        // approximation), not a silent misrepresentation of a *recognized* kind.
        _ => JoinKind::Inner,
    }
}

/// Recognizes the real repo shape `F.<func>(<col>).alias(<name>)` found in
/// `azure-databricks-project`'s `src/dp/semantic/graph.py` (e.g.
/// `F.min("component").alias("component")`). Aggregate expressions in any other shape (bare
/// column references, keyword-form `.agg(total=F.sum("amount"))`) are not recognized — an
/// honest, narrower MVP scope, not a claim of full `.agg(...)` coverage.
fn agg_expr_from_arg(expr: &ast::Expr) -> Option<AggExpr> {
    let ast::Expr::Call(outer) = expr else {
        return None;
    };
    let ast::Expr::Attribute(outer_attr) = outer.func.as_ref() else {
        return None;
    };
    if outer_attr.attr.as_str() != "alias" {
        return None;
    }
    let output = positional_string_arg(outer, 0)?;
    let ast::Expr::Call(inner) = outer_attr.value.as_ref() else {
        return None;
    };
    let ast::Expr::Attribute(inner_attr) = inner.func.as_ref() else {
        return None;
    };
    let func = inner_attr.attr.to_string();
    let arg = inner
        .args
        .first()
        .and_then(string_constant)
        .unwrap_or_default();
    Some(AggExpr { output, func, arg })
}

/// Turns a linearized chain of `.method(...)` calls into `TransformNode`s. Not every call
/// produces a node — intermediate calls like `.format(...)`/`.mode(...)`/`.option(...)` (part of
/// a `.read`/`.write` builder chain) are structurally passed through without interruption, same
/// as any construct this project doesn't model getting silently skipped rather than forced into
/// a shape that doesn't fit.
fn calls_to_nodes(calls: &[RawCall], source: &str) -> Vec<TransformNode> {
    let mut nodes = Vec::new();
    let mut i = 0;
    while i < calls.len() {
        let c = &calls[i];
        match c.method {
            "sql" if c.receiver_is_spark => {
                if let Some(arg) = c.call.args.first() {
                    nodes.push(TransformNode::Unmapped {
                        raw: source_slice(source, arg).to_string(),
                        reason: "SQL embedded in a Python string, not modeled".to_string(),
                    });
                }
                i += 1;
            }
            "table" if c.receiver_is_spark => {
                if let Some(name) = positional_string_arg(c.call, 0) {
                    nodes.push(TransformNode::Source {
                        object_name: name,
                        columns: Vec::new(),
                    });
                }
                i += 1;
            }
            "load" => {
                let object_name =
                    positional_string_arg(c.call, 0).unwrap_or_else(|| "<unknown>".to_string());
                nodes.push(TransformNode::Source {
                    object_name,
                    columns: Vec::new(),
                });
                i += 1;
            }
            "saveAsTable" => {
                let object_name =
                    positional_string_arg(c.call, 0).unwrap_or_else(|| "<unknown>".to_string());
                nodes.push(TransformNode::Sink {
                    object_name,
                    columns: Vec::new(),
                });
                i += 1;
            }
            "save" => {
                let object_name =
                    positional_string_arg(c.call, 0).unwrap_or_else(|| "<unknown>".to_string());
                nodes.push(TransformNode::Sink {
                    object_name,
                    columns: Vec::new(),
                });
                i += 1;
            }
            "join" => {
                nodes.push(TransformNode::Join {
                    left: NodeId(0),
                    right: NodeId(0),
                    keys: join_keys_from_on(c.call),
                    kind: join_kind_from_how(c.call),
                });
                i += 1;
            }
            "groupBy" if calls.get(i + 1).map(|n| n.method) == Some("agg") => {
                let group_by: Vec<String> =
                    c.call.args.iter().filter_map(string_constant).collect();
                let aggs: Vec<AggExpr> = calls[i + 1]
                    .call
                    .args
                    .iter()
                    .filter_map(agg_expr_from_arg)
                    .collect();
                nodes.push(TransformNode::Aggregate { group_by, aggs });
                i += 2;
            }
            "filter" | "where" => {
                if let Some(arg) = c.call.args.first() {
                    nodes.push(TransformNode::Filter {
                        condition: source_slice(source, arg).to_string(),
                    });
                }
                i += 1;
            }
            "withColumn" => {
                if let (Some(output), Some(expr_arg)) =
                    (positional_string_arg(c.call, 0), c.call.args.get(1))
                {
                    nodes.push(TransformNode::Calculate {
                        output,
                        expr: source_slice(source, expr_arg).to_string(),
                    });
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> PythonFileResult {
        parse_python_file("test.py", source, KirId(Uuid::new_v4())).unwrap()
    }

    #[test]
    fn recognizes_imports_as_depends_on() {
        let result = parse("import sys\nfrom dp.io.delta import write_delta\n");
        assert_eq!(result.objects.len(), 2);
        assert!(
            result
                .objects
                .iter()
                .any(|o| o.name == "sys" && o.kind == ObjectKind::Custom("PythonModule".into()))
        );
        // Real bug, found live 2026-08-24 against `pdf-reader`: this used to resolve to the bare
        // `dp.io.delta` package, collapsing every real submodule imported from it onto the same
        // object. `write_delta` is the actual thing the source references.
        assert!(
            result
                .objects
                .iter()
                .any(|o| o.name == "dp.io.delta.write_delta")
        );
        assert!(!result.objects.iter().any(|o| o.name == "dp.io.delta"));
        assert!(
            result
                .relationships
                .iter()
                .all(|r| r.kind == RelationshipKind::DependsOn)
        );
    }

    #[test]
    fn from_import_with_multiple_names_resolves_each_to_its_own_qualified_module() {
        // The real pdf-reader shape this fix targets: `from app.services import ai_service`
        // previously compiled to one coarse `app.services` object shared by every name imported
        // from that package — two distinct real submodules became indistinguishable.
        let result = parse("from app.services import ai_service, db_service\n");
        assert!(
            result
                .objects
                .iter()
                .any(|o| o.name == "app.services.ai_service")
        );
        assert!(
            result
                .objects
                .iter()
                .any(|o| o.name == "app.services.db_service")
        );
        assert!(!result.objects.iter().any(|o| o.name == "app.services"));
    }

    #[test]
    fn star_import_falls_back_to_the_bare_module() {
        // `from pkg import *` has no specific name to qualify with — `pkg` is the only real fact
        // the source states, not a fabricated `pkg.*`.
        let result = parse("from app.utils import *\n");
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.objects[0].name, "app.utils");
    }

    #[test]
    fn recognizes_function_and_class_defs_as_symbols() {
        let result = parse("def foo():\n    pass\n\nclass Bar:\n    pass\n");
        let names: Vec<&str> = result.objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"Bar"));
        let foo = result.objects.iter().find(|o| o.name == "foo").unwrap();
        assert_eq!(foo.properties["kind"], "function");
    }

    // ── RFC 0091 — SQLAlchemy ORM model recognition ─────────────────────────

    #[test]
    fn orm_model_produces_a_real_table_object_alongside_the_existing_symbol() {
        // The real shape found live in `pdf-reader`'s `backend/app/db/models.py`.
        let result = parse(
            "class Document(Base):\n    __tablename__ = \"documents\"\n    file_hash: Mapped[str] = mapped_column(String(64), primary_key=True)\n    filename: Mapped[str] = mapped_column(String(512))\n    page_count: Mapped[int] = mapped_column(Integer)\n",
        );

        // The existing PythonSymbol object is unchanged.
        let symbol = result
            .objects
            .iter()
            .find(|o| o.name == "Document" && o.kind == ObjectKind::Custom("PythonSymbol".into()));
        assert!(
            symbol.is_some(),
            "the class must still get its ordinary symbol object"
        );

        // A new, separate Table object, named by the real table name (not the class name).
        let table = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table)
            .expect("a Table object must be compiled for a recognized ORM model");
        assert_eq!(table.name, "documents");

        let columns = table.properties["columns"].as_array().unwrap();
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0]["name"], "file_hash");
        assert_eq!(columns[0]["data_type"], "String");
        assert_eq!(columns[2]["name"], "page_count");
        assert_eq!(columns[2]["data_type"], "Integer");

        // Contains-linked from the same file, same convention `add_symbol` already uses.
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains && r.to == table.id)
        );
    }

    #[test]
    fn plain_class_with_no_tablename_produces_no_table_object() {
        let result = parse("class Widget:\n    \"\"\"A real widget.\"\"\"\n    pass\n");
        assert!(!result.objects.iter().any(|o| o.kind == ObjectKind::Table));
    }

    #[test]
    fn orm_foreign_key_resolves_within_the_same_file_regardless_of_declaration_order() {
        // Real shape: `PageCache` references `Document`'s real table, declared *after* it in the
        // real source (`db/models.py`) — must still resolve.
        let result = parse(
            "class Document(Base):\n    __tablename__ = \"documents\"\n    file_hash: Mapped[str] = mapped_column(String(64), primary_key=True)\n\nclass PageCache(Base):\n    __tablename__ = \"page_cache\"\n    file_hash: Mapped[str] = mapped_column(String(64), ForeignKey(\"documents.file_hash\"))\n",
        );

        let documents = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table && o.name == "documents")
            .unwrap();
        let page_cache = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table && o.name == "page_cache")
            .unwrap();

        let fk = result
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::ForeignKey)
            .expect("a real ForeignKey edge must be compiled");
        assert_eq!(fk.from, page_cache.id);
        assert_eq!(fk.to, documents.id);
        assert_eq!(
            fk.properties["fk_desc"],
            "page_cache.file_hash → documents.file_hash"
        );
    }

    #[test]
    fn orm_foreign_key_to_a_table_outside_this_file_is_honestly_skipped_not_fabricated() {
        let result = parse(
            "class PageCache(Base):\n    __tablename__ = \"page_cache\"\n    file_hash: Mapped[str] = mapped_column(String(64), ForeignKey(\"documents.file_hash\"))\n",
        );
        assert!(
            !result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::ForeignKey),
            "no real Table for 'documents' exists in this file — no edge should be invented"
        );
        // The column itself is still real and extracted, just without a resolved FK edge.
        let table = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table)
            .unwrap();
        assert_eq!(table.properties["columns"][0]["name"], "file_hash");
    }

    #[test]
    fn orm_column_with_unrecognizable_data_type_is_honest_not_fabricated() {
        let result = parse(
            "class Document(Base):\n    __tablename__ = \"documents\"\n    file_hash = some_dynamic_column_builder()\n",
        );
        let table = result
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Table)
            .unwrap();
        assert_eq!(table.properties["columns"][0]["data_type"], "unknown");
    }

    #[test]
    fn a_real_docstring_becomes_a_real_description() {
        let result =
            parse("def hash(pw):\n    \"\"\"Hashes a password using bcrypt.\"\"\"\n    pass\n");
        let hash_fn = result.objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(
            hash_fn.properties["description"],
            "Hashes a password using bcrypt."
        );
    }

    #[test]
    fn a_class_docstring_becomes_a_real_description() {
        let result = parse("class Widget:\n    \"\"\"A real widget.\"\"\"\n    pass\n");
        let widget = result.objects.iter().find(|o| o.name == "Widget").unwrap();
        assert_eq!(widget.properties["description"], "A real widget.");
    }

    #[test]
    fn a_function_with_no_real_docstring_has_no_description_property_at_all() {
        let result = parse("def plain():\n    x = 1\n    return x\n");
        let plain = result.objects.iter().find(|o| o.name == "plain").unwrap();
        assert!(!plain.properties.contains_key("description"));
    }

    #[test]
    fn a_real_statement_that_is_not_a_string_literal_is_not_mistaken_for_a_docstring() {
        // The body's first statement is a real expression statement, but not a string literal —
        // must not be misread as a docstring.
        let result = parse("def f():\n    1 + 1\n    return None\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert!(!f.properties.contains_key("description"));
    }

    // ── RFC 0088 (fast-follow) — real source_span capture for Python ───────────────────────────

    #[test]
    fn a_single_line_function_gets_a_real_source_span() {
        // Lines: 1 def, 2 body.
        let result = parse("def f():\n    pass\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert_eq!(
            f.properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 2})
        );
    }

    #[test]
    fn a_multi_line_function_body_gets_a_real_source_span() {
        let result = parse("def f():\n    x = 1\n    y = 2\n    return x + y\n");
        let f = result.objects.iter().find(|o| o.name == "f").unwrap();
        assert_eq!(
            f.properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 4})
        );
    }

    #[test]
    fn a_class_gets_a_real_source_span_too() {
        let result = parse("class Widget:\n    def method(self):\n        pass\n");
        let widget = result.objects.iter().find(|o| o.name == "Widget").unwrap();
        assert_eq!(
            widget.properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 3})
        );
    }

    #[test]
    fn a_function_defined_after_other_real_code_gets_its_own_real_line_numbers() {
        // The real case this matters for: a symbol isn't always the first thing in the file.
        let result = parse("import os\n\nX = 1\n\n\ndef later():\n    return X\n");
        let later = result.objects.iter().find(|o| o.name == "later").unwrap();
        assert_eq!(
            later.properties["source_span"],
            serde_json::json!({"start_line": 6, "end_line": 7})
        );
    }

    #[test]
    fn table_read_becomes_source_node() {
        let result = parse("result = spark.table(\"Sales.SalesPerson\")\n");
        assert_eq!(result.transform_graphs.len(), 1);
        let graph = &result.transform_graphs[0];
        assert_eq!(graph.nodes.len(), 1);
        assert!(matches!(
            &graph.nodes[0],
            TransformNode::Source { object_name, .. } if object_name == "Sales.SalesPerson"
        ));
    }

    #[test]
    fn read_format_load_becomes_source_node() {
        let result = parse("df = spark.read.format(\"delta\").load(bronze_table)\n");
        // `bronze_table` is a variable, not a string literal — falls back to the honest
        // "<unknown>" placeholder rather than guessing at its value.
        let graph = &result.transform_graphs[0];
        assert!(matches!(
            &graph.nodes[0],
            TransformNode::Source { object_name, .. } if object_name == "<unknown>"
        ));
    }

    /// Real shape from `azure-databricks-project`'s `src/dp/transforms/bronze.py`.
    #[test]
    fn real_join_and_select_chain_becomes_join_node() {
        let result =
            parse("deleted = active_bronze.join(pk_df, on=primary_keys, how=\"left_anti\")\n");
        let graph = &result.transform_graphs[0];
        assert_eq!(graph.nodes.len(), 1);
        match &graph.nodes[0] {
            TransformNode::Join { kind, .. } => assert_eq!(*kind, JoinKind::Inner), // left_anti not in JoinKind's vocabulary
            other => panic!("expected Join, got {other:?}"),
        }
    }

    #[test]
    fn join_with_string_on_key_and_left_how() {
        let result = parse("out = a.join(b, on=\"id\", how=\"left\")\n");
        let graph = &result.transform_graphs[0];
        match &graph.nodes[0] {
            TransformNode::Join { keys, kind, .. } => {
                assert_eq!(keys, &vec![("id".to_string(), "id".to_string())]);
                assert_eq!(*kind, JoinKind::Left);
            }
            other => panic!("expected Join, got {other:?}"),
        }
    }

    #[test]
    fn with_column_becomes_calculate_node() {
        let result = parse("df2 = df.withColumn(\"x\", F.lit(1))\n");
        let graph = &result.transform_graphs[0];
        assert!(matches!(
            &graph.nodes[0],
            TransformNode::Calculate { output, expr } if output == "x" && expr == "F.lit(1)"
        ));
    }

    #[test]
    fn multi_step_chain_produces_linked_nodes() {
        // Real shape from `src/dp/transforms/bronze.py::add_metadata_columns`.
        let result = parse(
            "def add_metadata_columns(df):\n\
             \x20   return (\n\
             \x20       df.withColumn(\"_inserted_at\", ts)\n\
             \x20       .withColumn(\"_updated_at\", ts)\n\
             \x20       .filter(\"active = 1\")\n\
             \x20   )\n",
        );
        assert_eq!(result.transform_graphs.len(), 1);
        let graph = &result.transform_graphs[0];
        assert_eq!(graph.nodes.len(), 3);
        assert!(matches!(graph.nodes[0], TransformNode::Calculate { .. }));
        assert!(matches!(graph.nodes[1], TransformNode::Calculate { .. }));
        assert!(matches!(graph.nodes[2], TransformNode::Filter { .. }));
        assert_eq!(
            graph.edges,
            vec![(NodeId(0), NodeId(1)), (NodeId(1), NodeId(2))]
        );
    }

    #[test]
    fn group_by_agg_becomes_aggregate_node() {
        // Real shape from `src/dp/semantic/graph.py`.
        let result = parse(
            "new_vertices = all_msgs.groupBy(\"id\").agg(F.min(\"component\").alias(\"component\"))\n",
        );
        let graph = &result.transform_graphs[0];
        assert_eq!(graph.nodes.len(), 1);
        match &graph.nodes[0] {
            TransformNode::Aggregate { group_by, aggs } => {
                assert_eq!(group_by, &vec!["id".to_string()]);
                assert_eq!(aggs.len(), 1);
                assert_eq!(aggs[0].func, "min");
                assert_eq!(aggs[0].arg, "component");
                assert_eq!(aggs[0].output, "component");
            }
            other => panic!("expected Aggregate, got {other:?}"),
        }
    }

    #[test]
    fn write_save_as_table_becomes_sink_node() {
        let result =
            parse("df.write.format(\"delta\").mode(\"append\").saveAsTable(\"gold.orders\")\n");
        let graph = &result.transform_graphs[0];
        assert!(matches!(
            &graph.nodes[0],
            TransformNode::Sink { object_name, .. } if object_name == "gold.orders"
        ));
    }

    #[test]
    fn spark_sql_call_is_honestly_unmapped_never_parsed_as_sql() {
        let result = parse("result = spark.sql(f\"SELECT * FROM {catalog}.dvdrental.customer\")\n");
        let graph = &result.transform_graphs[0];
        assert_eq!(graph.nodes.len(), 1);
        match &graph.nodes[0] {
            TransformNode::Unmapped { reason, raw } => {
                assert_eq!(reason, "SQL embedded in a Python string, not modeled");
                assert!(raw.contains("SELECT * FROM"));
            }
            other => panic!("expected Unmapped, got {other:?}"),
        }
    }

    #[test]
    fn plain_statement_with_no_recognized_chain_produces_no_graph() {
        let result = parse("catalog = dbutils.widgets.get(\"catalog\")\n");
        assert!(result.transform_graphs.is_empty());
    }

    /// Robustness against the real "Databricks notebook source" `.py` convention — `# MAGIC`/
    /// `# COMMAND ----------` cell markers are ordinary comments to a real Python parser and must
    /// not break parsing.
    #[test]
    fn databricks_notebook_comment_markers_do_not_break_parsing() {
        let source = "# Databricks notebook source\n\
                       # MAGIC %md\n\
                       # MAGIC # Bronze ingest\n\
                       # COMMAND ----------\n\
                       import sys\n\
                       # COMMAND ----------\n\
                       result = spark.table(\"t\")\n";
        let result = parse(source);
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.transform_graphs.len(), 1);
    }

    // ── RFC 0092 — class inheritance (`RelationshipKind::Extends`) ──────────

    #[test]
    fn subclass_of_a_same_file_class_gets_a_real_extends_edge() {
        let result = parse("class Base:\n    pass\n\nclass Document(Base):\n    pass\n");
        let base = result.objects.iter().find(|o| o.name == "Base").unwrap();
        let document = result
            .objects
            .iter()
            .find(|o| o.name == "Document")
            .unwrap();
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Extends
                    && r.from == document.id
                    && r.to == base.id)
        );
    }

    #[test]
    fn base_declared_after_the_subclass_still_resolves() {
        // Real, valid Python: nothing requires a base class to be declared before its subclass
        // within the same module-level statement order this analyzer walks in.
        let result = parse("class Document(Base):\n    pass\n\nclass Base:\n    pass\n");
        let base = result.objects.iter().find(|o| o.name == "Base").unwrap();
        let document = result
            .objects
            .iter()
            .find(|o| o.name == "Document")
            .unwrap();
        assert!(
            result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Extends
                    && r.from == document.id
                    && r.to == base.id)
        );
    }

    #[test]
    fn extending_an_import_only_base_is_honestly_skipped_not_fabricated() {
        // `BaseModel` is never locally defined — no `PythonSymbol` object exists for it in this
        // file, so no `Extends` edge is fabricated pointing at a nonexistent id.
        let result = parse("class TranslateRequest(BaseModel):\n    pass\n");
        assert!(
            !result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Extends)
        );
    }

    #[test]
    fn attribute_form_base_is_never_treated_as_a_local_class() {
        // A dotted base (`orm.DeclarativeBase`) can never refer to a same-file class by
        // construction — must not accidentally match a same-named local class via `attr`.
        let result = parse(
            "class DeclarativeBase:\n    pass\n\nclass Base(orm.DeclarativeBase):\n    pass\n",
        );
        assert!(
            !result
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Extends)
        );
    }

    /// End-to-end shape mirroring `pdf-reader`'s real `db/models.py`/`api/ai.py` exactly: a
    /// locally-defined `Base` that itself extends an external, unresolvable `DeclarativeBase`,
    /// and a real subclass of it — both the resolved and unresolved case in one real file.
    #[test]
    fn real_sqlalchemy_base_chain_resolves_the_local_link_and_skips_the_external_one() {
        let source = "class Base(DeclarativeBase):\n    pass\n\n\
                       class Document(Base):\n    __tablename__ = \"documents\"\n";
        let result = parse(source);
        let base = result.objects.iter().find(|o| o.name == "Base").unwrap();
        let document = result
            .objects
            .iter()
            .find(|o| o.name == "Document")
            .unwrap();
        let extends: Vec<_> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Extends)
            .collect();
        assert_eq!(extends.len(), 1, "only Document→Base should resolve");
        assert_eq!(extends[0].from, document.id);
        assert_eq!(extends[0].to, base.id);
    }

    #[test]
    fn extends_relationship_id_is_deterministic_across_separate_parses() {
        let source = "class Base:\n    pass\n\nclass Document(Base):\n    pass\n";
        let r1 = parse(source);
        let r2 = parse(source);
        let e1 = r1
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::Extends)
            .unwrap();
        let e2 = r2
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::Extends)
            .unwrap();
        assert_eq!(e1.id, e2.id);
    }
}
