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
};
use ekos_semantic::merge_graphs;
use ekos_semantic::transform_ir::{
    AggExpr, JoinKind, NodeId, TransformGraph, TransformNode, TransformOrigin, lower_to_kir,
};
use rustpython_parser::Parse;
use rustpython_parser::ast::{self, Ranged};
use serde::Deserialize;
use std::collections::HashSet;
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

    for stmt in &stmts {
        walk_top_level_statement(stmt, path, source, file_id, &mut result, &mut graph_index);
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

fn add_symbol(
    name: &str,
    kind: &str,
    path: &str,
    file_id: KirId,
    result: &mut PythonFileResult,
    doc: Option<String>,
    span: Option<(u32, u32)>,
) {
    let target_id = KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("python-symbol:{path}:{name}").as_bytes(),
    ));
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

fn walk_top_level_statement(
    stmt: &ast::Stmt,
    path: &str,
    source: &str,
    file_id: KirId,
    result: &mut PythonFileResult,
    graph_index: &mut usize,
) {
    match stmt {
        ast::Stmt::Import(imp) => {
            for alias in &imp.names {
                add_import(alias.name.as_str(), file_id, result);
            }
        }
        ast::Stmt::ImportFrom(imp) => {
            if let Some(module) = &imp.module {
                add_import(module.as_str(), file_id, result);
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
        assert!(result.objects.iter().any(|o| o.name == "dp.io.delta"));
        assert!(
            result
                .relationships
                .iter()
                .all(|r| r.kind == RelationshipKind::DependsOn)
        );
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
}
