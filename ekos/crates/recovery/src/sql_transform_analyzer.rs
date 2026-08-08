//! `SqlTransformAnalyzerPass` — converts SQL `SELECT`/`VIEW`/stored-procedure
//! observation into the Transformation IR (RFC 0027 Phase 1), then lowers
//! that IR into KIR via `ekos_semantic::transform_ir::lower_to_kir`.
//!
//! Distinct from `sql_analyzer.rs`'s `SqlAnalyzerPass`: that pass extracts
//! `CREATE TABLE` DDL into entities/FK relationships, with LLM enrichment.
//! This pass is pure structural DML analysis — no LLM in the loop, same
//! shape as `PentahoAnalyzerPass` — extracting *transformation logic*
//! (`SELECT`/`VIEW`/procedure bodies) rather than schema.
//!
//! Scope (RFC 0027 Phase 2, per the implementation plan):
//! - `SELECT`/`VIEW`: near-direct AST → IR mapping. Deterministic, low risk.
//! - Stored procedures/functions: **not pure SQL** — control flow (loops,
//!   cursors, conditionals, variables) is out of scope. MVP: embedded SQL
//!   statements become real IR fragments; everything else becomes
//!   `Unmapped`. `IF`/`WHILE` specifically have no `sqlparser` grammar support at all (verified
//!   directly, RFC 0039) — a procedure body using either fails whole-file structured parsing, so
//!   this module falls back to a per-statement text split (see
//!   `parse_sql_statement_by_statement`) rather than losing every other statement in the file.
//! - Dialects: Postgres, MySQL, MSSQL (T-SQL), Snowflake, and Databricks via their native
//!   `sqlparser` dialects (Databricks/Snowflake coverage not independently verified against
//!   every real syntax extension — flagged, not blocking); Informix has no dedicated `sqlparser`
//!   dialect, so it falls back to `GenericDialect` and accepts incomplete coverage, per the
//!   plan's explicit scoping.
//!
//! Dialect selection is config-driven as of RFC 0031, and — as of RFC 0039 — fully unified with
//! `SqlAnalyzerPass`: `recover.rs` resolves one `SqlDialectParser` per `.sql` file (via
//! `sql_dialect_registry`'s registry + `ekos.toml`'s `[recover.sql]` rules, falling back to
//! `"generic"`) and passes both the resolved dialect name (still needed for
//! `SqlTransformStats.dialect`/`TransformOrigin.source_kind` display/tagging) and the resolved
//! `sqlparser::Dialect` object itself into `SqlTransformAnalyzerPass::new` — this pass no longer
//! owns a private `dialect_for` that could silently disagree with the registry (RFC 0031's
//! previously-unchecked acceptance criterion, closed by RFC 0039).

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::KirGraph;
use ekos_semantic::merge_graphs;
use ekos_semantic::transform_ir::{
    AggExpr, JoinKind, NodeId, TransformGraph, TransformNode, TransformOrigin, lower_to_kir,
};
use sqlparser::ast::{
    BinaryOperator, CreateFunctionBody, Expr, Function, GroupByExpr, Join, JoinConstraint,
    JoinOperator, Query, Select, SelectItem, SetExpr, Statement, TableFactor, Value,
};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use std::sync::{Arc, Mutex};

const AGG_FUNCS: &[&str] = &["SUM", "COUNT", "AVG", "MIN", "MAX"];

/// Coverage counters from one run, mirroring `PentahoStats` — the phase's
/// readiness metric per the implementation plan: a concrete, measurable exit
/// criterion, reported per dialect.
#[derive(Debug, Clone, Default)]
pub struct SqlTransformStats {
    pub dialect: String,
    pub statements_processed: usize,
    pub nodes_total: usize,
    /// Non-`Unmapped` nodes.
    pub nodes_mapped: usize,
}

impl SqlTransformStats {
    pub fn coverage_percent(&self) -> f32 {
        if self.nodes_total == 0 {
            0.0
        } else {
            100.0 * self.nodes_mapped as f32 / self.nodes_total as f32
        }
    }
}

pub struct SqlTransformAnalyzerPass {
    pass_id: String,
    sql: String,
    source_path: String,
    dialect_name: String,
    /// RFC 0039: the same resolved `SqlDialectParser::sqlparser_dialect()` output
    /// `SqlAnalyzerPass` gets, passed in by the caller (`recover.rs`) instead of being
    /// re-derived from `dialect_name` via a private `dialect_for` match — closes RFC 0031's
    /// previously-unchecked acceptance criterion that both passes use one shared resolution.
    /// `dialect_name` is kept alongside it (not replaced) because it's still needed for
    /// `SqlTransformStats.dialect`/`TransformOrigin.source_kind` display/tagging.
    dialect: Box<dyn Dialect + Send + Sync>,
    stats: Arc<Mutex<SqlTransformStats>>,
}

impl SqlTransformAnalyzerPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        dialect_name: impl Into<String>,
        dialect: Box<dyn Dialect + Send + Sync>,
    ) -> Self {
        let source_path = source_path.into();
        let dialect_name = dialect_name.into();
        Self {
            pass_id: format!("sql-transform-analyzer:{source_path}"),
            sql: sql.into(),
            source_path,
            stats: Arc::new(Mutex::new(SqlTransformStats {
                dialect: dialect_name.clone(),
                ..Default::default()
            })),
            dialect_name,
            dialect,
        }
    }

    /// Handle onto this pass's counters, for printing a summary after the
    /// `PassManager` has taken ownership of the pass.
    pub fn stats_handle(&self) -> Arc<Mutex<SqlTransformStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for SqlTransformAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.sql.as_bytes());
        hasher.update(self.dialect_name.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let graphs = parse_sql_to_transform_graphs(
            &self.sql,
            &self.source_path,
            &self.dialect_name,
            self.dialect.as_ref(),
        );

        let mut combined = KirGraph::new();
        let mut stats = SqlTransformStats {
            dialect: self.dialect_name.clone(),
            ..Default::default()
        };

        for graph in &graphs {
            stats.statements_processed += 1;
            stats.nodes_total += graph.nodes.len();
            stats.nodes_mapped += graph
                .nodes
                .iter()
                .filter(|n| !matches!(n, TransformNode::Unmapped { .. }))
                .count();
            merge_graphs(&mut combined, lower_to_kir(graph));
        }

        *self.stats.lock().unwrap() = stats.clone();

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
            statements = stats.statements_processed,
            nodes = stats.nodes_total,
            mapped = stats.nodes_mapped,
            coverage_pct = stats.coverage_percent(),
            "sql-transform-analyzer complete"
        );
        Ok(())
    }
}

// ── Dialect selection ────────────────────────────────────────────────────────
//
// RFC 0039: the actual `sqlparser::Dialect` to parse with is now always supplied by the caller
// (resolved from the shared `sql_dialect_registry`, same object `SqlAnalyzerPass` gets) —
// no more private `dialect_for` match to fall out of sync with the registry. `source_kind_for`
// stays a name-keyed match: it's a pure display/tagging label for `TransformOrigin`, not a
// parsing decision, so it doesn't duplicate the registry's actual dialect-selection behavior.

fn source_kind_for(dialect_name: &str) -> &'static str {
    match dialect_name {
        "postgres" | "postgresql" => "sql-postgres",
        "mysql" => "sql-mysql",
        "mssql" | "tsql" | "synapse" => "sql-mssql",
        "databricks" | "spark" => "sql-databricks",
        "snowflake" => "sql-snowflake",
        "informix" => "sql-informix",
        _ => "sql-generic",
    }
}

// ── Top-level statement dispatch ─────────────────────────────────────────────

/// Parses `sql` and returns one `TransformGraph` per top-level statement this
/// pass understands (`SELECT`, `CREATE VIEW`, `CREATE PROCEDURE`, `CREATE
/// FUNCTION`). `CREATE TABLE` and other DDL are left to `sql_analyzer.rs`'s
/// `SqlAnalyzerPass` — this function only ever looks at transformation logic.
pub fn parse_sql_to_transform_graphs(
    sql: &str,
    source_path: &str,
    dialect_name: &str,
    dialect: &dyn Dialect,
) -> Vec<TransformGraph> {
    let source_kind = source_kind_for(dialect_name);

    let stmts = match Parser::parse_sql(dialect, sql) {
        Ok(s) => s,
        Err(first_err) => {
            // Fallback (GitHub issue #3): some hand-written scripts omit `;` between top-level
            // statements — retry once with synthetic separators inserted. See
            // `statement_repair`'s doc comment for why this is only attempted after the
            // unmodified text has already failed to parse.
            let repaired = crate::statement_repair::ensure_statement_separators(sql);
            match Parser::parse_sql(dialect, &repaired) {
                Ok(s) => s,
                Err(_) => {
                    // RFC 0039: found by verifying directly against real `sqlparser` 0.53
                    // behavior — a `CREATE PROCEDURE` body using `IF`/`WHILE` control flow has
                    // no grammar support at all (any dialect), so the *whole file* fails to
                    // parse here, not just the procedure. Before this fallback, every other
                    // statement in the same file (an unrelated `CREATE VIEW`, another `SELECT`)
                    // was silently lost too. Falls back to a per-statement text split so those
                    // survive — see `parse_sql_statement_by_statement`'s doc comment for the
                    // honest limitation this introduces.
                    tracing::warn!(
                        "sql-transform-analyzer: sqlparser failed on {source_path} ({dialect_name}): {first_err} — falling back to per-statement recovery"
                    );
                    return parse_sql_statement_by_statement(
                        sql,
                        source_path,
                        source_kind,
                        dialect,
                    );
                }
            }
        }
    };

    let mut graphs = Vec::new();
    for (index, stmt) in stmts.iter().enumerate() {
        let origin = TransformOrigin {
            source_path: format!("{source_path}#{index}"),
            source_kind: source_kind.to_string(),
            extracted_at: chrono::Utc::now(),
        };
        if let Some(graph) = dispatch_one_statement(stmt, origin, dialect) {
            graphs.push(graph);
        }
    }

    graphs
}

/// Turns one already-parsed `Statement` into a `TransformGraph`, or `None` for statement kinds
/// this pass doesn't model (DDL other than `CREATE VIEW`, etc. — the same `_ => {}` no-op the
/// original inline `match` had). Factored out so the whole-file happy path and
/// `parse_sql_statement_by_statement`'s per-fragment fallback share one dispatch, instead of two
/// copies that could silently drift apart.
fn dispatch_one_statement(
    stmt: &Statement,
    origin: TransformOrigin,
    dialect: &dyn Dialect,
) -> Option<TransformGraph> {
    match stmt {
        Statement::Query(query) => Some(query_to_graph(query, origin)),
        Statement::CreateView { name, query, .. } => {
            let mut graph = query_to_graph(query, origin);
            let last = graph.nodes.len().checked_sub(1).map(|i| NodeId(i as u32));
            let sink_id = push(
                &mut graph.nodes,
                TransformNode::Sink {
                    object_name: name.to_string(),
                    columns: Vec::new(),
                },
            );
            if let Some(prev) = last {
                graph.edges.push((prev, sink_id));
            }
            Some(graph)
        }
        Statement::CreateProcedure { name, body, .. } => {
            Some(procedure_body_to_graph(name.to_string(), body, &origin))
        }
        Statement::CreateFunction(cf) => Some(function_to_graph(
            cf.name.to_string(),
            cf.function_body.as_ref(),
            dialect,
            &origin,
        )),
        _ => None,
    }
}

/// Whole-file parse fallback (RFC 0039), engaged only when full-file structured parsing (plus
/// the missing-`;` repair retry) both fail. Splits `sql` on top-level `;` and retries each
/// fragment independently, so a `CREATE PROCEDURE` using unmodelable control flow doesn't take
/// every other statement in the same file down with it.
///
/// Honest limitation, stated not hidden: this split does not track nested `BEGIN...END` blocks,
/// so a failing procedure's own internal semicolons can produce several partial/duplicate
/// `Unmapped` fragments instead of one clean node for the whole procedure — an approximation, not
/// a silently wrong answer. The procedure's control flow was never going to be modeled either
/// way (RFC 0027's documented MVP scope); this only changes whether *unrelated* statements in the
/// same file survive.
fn parse_sql_statement_by_statement(
    sql: &str,
    source_path: &str,
    source_kind: &str,
    dialect: &dyn Dialect,
) -> Vec<TransformGraph> {
    let mut graphs = Vec::new();
    for (index, fragment) in sql.split(';').enumerate() {
        let fragment = fragment.trim();
        if fragment.is_empty() {
            continue;
        }
        let origin = TransformOrigin {
            source_path: format!("{source_path}#{index}"),
            source_kind: source_kind.to_string(),
            extracted_at: chrono::Utc::now(),
        };
        let graph = match Parser::parse_sql(dialect, fragment) {
            Ok(stmts) if stmts.len() == 1 => {
                dispatch_one_statement(&stmts[0], origin.clone(), dialect).unwrap_or_else(|| {
                    TransformGraph {
                        nodes: vec![TransformNode::Unmapped {
                            raw: fragment.to_string(),
                            reason: "statement type not modeled".to_string(),
                        }],
                        edges: Vec::new(),
                        origin,
                    }
                })
            }
            _ => TransformGraph {
                nodes: vec![TransformNode::Unmapped {
                    raw: fragment.to_string(),
                    reason: "statement-level parse failure (likely control flow), not modeled"
                        .to_string(),
                }],
                edges: Vec::new(),
                origin,
            },
        };
        graphs.push(graph);
    }
    graphs
}

fn push(nodes: &mut Vec<TransformNode>, node: TransformNode) -> NodeId {
    let id = NodeId(nodes.len() as u32);
    nodes.push(node);
    id
}

// ── SELECT/Query → TransformGraph ───────────────────────────────────────────

fn query_to_graph(query: &Query, origin: TransformOrigin) -> TransformGraph {
    if query.with.is_some() {
        return TransformGraph {
            nodes: vec![TransformNode::Unmapped {
                raw: query.to_string(),
                reason: "CTE (WITH clause), not modeled".to_string(),
            }],
            edges: Vec::new(),
            origin,
        };
    }

    match query.body.as_ref() {
        SetExpr::Select(select) => select_to_graph(select, origin),
        other => TransformGraph {
            nodes: vec![TransformNode::Unmapped {
                raw: other.to_string(),
                reason: "unsupported query construct (set operation/subquery/VALUES), not modeled"
                    .to_string(),
            }],
            edges: Vec::new(),
            origin,
        },
    }
}

fn select_to_graph(select: &Select, origin: TransformOrigin) -> TransformGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut last: Option<NodeId> = None;

    for twj in &select.from {
        let base_id = table_factor_node(&twj.relation, &mut nodes);
        if let Some(prev) = last {
            edges.push((prev, base_id));
        }
        let mut left = base_id;

        for join in &twj.joins {
            let right = table_factor_node(&join.relation, &mut nodes);
            let join_id = join_node(join, left, right, &mut nodes);
            edges.push((left, join_id));
            edges.push((right, join_id));
            left = join_id;
        }
        last = Some(left);
    }

    if let Some(selection) = &select.selection {
        let filter_id = push(
            &mut nodes,
            TransformNode::Filter {
                condition: selection.to_string(),
            },
        );
        if let Some(prev) = last {
            edges.push((prev, filter_id));
        }
        last = Some(filter_id);
    }

    let group_by_exprs: Vec<String> = match &select.group_by {
        GroupByExpr::Expressions(exprs, _) => exprs.iter().map(|e| e.to_string()).collect(),
        GroupByExpr::All(_) => vec!["ALL".to_string()],
    };
    let aggs = extract_aggregates(&select.projection);
    if !group_by_exprs.is_empty() || !aggs.is_empty() {
        let agg_id = push(
            &mut nodes,
            TransformNode::Aggregate {
                group_by: group_by_exprs,
                aggs,
            },
        );
        if let Some(prev) = last {
            edges.push((prev, agg_id));
        }
        last = Some(agg_id);
    }

    for item in &select.projection {
        if let Some((output, expr)) = calculated_projection(item) {
            let calc_id = push(&mut nodes, TransformNode::Calculate { output, expr });
            if let Some(prev) = last {
                edges.push((prev, calc_id));
            }
            last = Some(calc_id);
        }
    }

    TransformGraph {
        nodes,
        edges,
        origin,
    }
}

fn table_factor_node(factor: &TableFactor, nodes: &mut Vec<TransformNode>) -> NodeId {
    match factor {
        TableFactor::Table { name, .. } => push(
            nodes,
            TransformNode::Source {
                object_name: name.to_string(),
                columns: Vec::new(),
            },
        ),
        other => push(
            nodes,
            TransformNode::Unmapped {
                raw: other.to_string(),
                reason: "unsupported FROM/JOIN table factor (derived subquery, table function, or similar), not modeled".to_string(),
            },
        ),
    }
}

fn join_node(join: &Join, left: NodeId, right: NodeId, nodes: &mut Vec<TransformNode>) -> NodeId {
    let (kind, constraint) = match &join.join_operator {
        JoinOperator::Inner(c) => (JoinKind::Inner, Some(c)),
        JoinOperator::LeftOuter(c) => (JoinKind::Left, Some(c)),
        JoinOperator::RightOuter(c) => (JoinKind::Right, Some(c)),
        JoinOperator::FullOuter(c) => (JoinKind::Full, Some(c)),
        JoinOperator::CrossJoin => (JoinKind::Cross, None),
        // SEMI/ANTI/ASOF/APPLY joins: MVP treats them as Inner with no
        // extracted keys — a documented approximation, not a blocker,
        // mirroring the DatabaseJoin/MergeJoin approximation in
        // pentaho_analyzer.rs.
        _ => (JoinKind::Inner, None),
    };
    let keys = constraint.map(extract_equi_keys).unwrap_or_default();
    push(
        nodes,
        TransformNode::Join {
            left,
            right,
            keys,
            kind,
        },
    )
}

fn extract_equi_keys(constraint: &JoinConstraint) -> Vec<(String, String)> {
    match constraint {
        JoinConstraint::On(expr) => {
            let mut keys = Vec::new();
            collect_equi_keys(expr, &mut keys);
            keys
        }
        JoinConstraint::Using(idents) => idents
            .iter()
            .map(|i| (i.value.clone(), i.value.clone()))
            .collect(),
        JoinConstraint::Natural | JoinConstraint::None => Vec::new(),
    }
}

/// Walks an `ON` expression collecting `a = b` equality pairs joined by
/// `AND`. Best-effort: any other operator/shape (`OR`, range predicates,
/// function calls) is simply not collected as a key, not treated as an
/// error — the join node itself is still emitted and mapped.
fn collect_equi_keys(expr: &Expr, out: &mut Vec<(String, String)>) {
    if let Expr::BinaryOp { left, op, right } = expr {
        match op {
            BinaryOperator::And => {
                collect_equi_keys(left, out);
                collect_equi_keys(right, out);
            }
            BinaryOperator::Eq => {
                out.push((left.to_string(), right.to_string()));
            }
            _ => {}
        }
    }
}

fn is_plain_column(expr: &Expr) -> bool {
    matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_))
}

fn as_aggregate_function(expr: &Expr) -> Option<&Function> {
    if let Expr::Function(f) = expr
        && AGG_FUNCS.contains(&f.name.to_string().to_uppercase().as_str())
    {
        return Some(f);
    }
    None
}

/// Extracts `SUM(...)`/`COUNT(...)`/etc. calls from the projection list into
/// `AggExpr`s. `arg` is everything between the function's first `(` and its
/// last `)` in its rendered `Display` text — cheap and exact, since function
/// names never contain parens.
fn extract_aggregates(projection: &[SelectItem]) -> Vec<AggExpr> {
    projection
        .iter()
        .filter_map(|item| {
            let (expr, alias) = match item {
                SelectItem::UnnamedExpr(e) => (e, None),
                SelectItem::ExprWithAlias { expr, alias } => (expr, Some(alias.value.clone())),
                _ => return None,
            };
            let func = as_aggregate_function(expr)?;
            let rendered = func.to_string();
            let arg = rendered
                .find('(')
                .map(|i| rendered[i + 1..rendered.len().saturating_sub(1)].to_string())
                .unwrap_or_default();
            let output = alias.unwrap_or_else(|| rendered.clone());
            Some(AggExpr {
                output,
                func: func.name.to_string().to_uppercase(),
                arg,
            })
        })
        .collect()
}

/// A projection item becomes a `Calculate` node when it is neither a plain
/// column reference nor an aggregate call (aggregates are already captured
/// by `extract_aggregates`/the `Aggregate` node) — i.e. any other computed
/// expression: arithmetic, string concatenation, `CASE`, scalar function
/// calls, etc.
fn calculated_projection(item: &SelectItem) -> Option<(String, String)> {
    let (expr, alias) = match item {
        SelectItem::UnnamedExpr(e) => (e, None),
        SelectItem::ExprWithAlias { expr, alias } => (expr, Some(alias.value.clone())),
        SelectItem::QualifiedWildcard(..) | SelectItem::Wildcard(..) => return None,
    };
    if is_plain_column(expr) || as_aggregate_function(expr).is_some() {
        return None;
    }
    let output = alias.unwrap_or_else(|| expr.to_string());
    Some((output, expr.to_string()))
}

// ── Stored procedures / functions (MVP scope) ───────────────────────────────

/// `CREATE PROCEDURE ... AS BEGIN ... END` (MSSQL): `sqlparser` already
/// parses the body into `Vec<Statement>` natively for this dialect, so no
/// text-splitting heuristic is needed here — embedded `SELECT`s become real
/// fragments, anything else (`SET`, `IF`, `WHILE`, cursors, ...) becomes
/// `Unmapped` with the plan's exact wording.
fn procedure_body_to_graph(
    proc_name: String,
    body: &[Statement],
    origin: &TransformOrigin,
) -> TransformGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for stmt in body {
        match stmt {
            Statement::Query(query) => {
                let fragment = query_to_graph(query, origin.clone());
                append_fragment(&mut nodes, &mut edges, fragment);
            }
            other => {
                push(
                    &mut nodes,
                    TransformNode::Unmapped {
                        raw: other.to_string(),
                        reason: "control flow present, not modeled".to_string(),
                    },
                );
            }
        }
    }

    if nodes.is_empty() {
        nodes.push(TransformNode::Unmapped {
            raw: format!("CREATE PROCEDURE {proc_name} (empty body)"),
            reason: "control flow present, not modeled".to_string(),
        });
    }

    TransformGraph {
        nodes,
        edges,
        origin: origin.clone(),
    }
}

/// `CREATE FUNCTION ... AS $$ ... $$` (Postgres) / `AS '...'`: the body is a
/// single string-literal `Expr` (PL/pgSQL, opaque to `sqlparser`'s
/// expression grammar), not pre-parsed statements — unlike MSSQL's
/// `CREATE PROCEDURE`. MVP: split the raw text on `;` and try parsing each
/// fragment as a standalone SQL statement; fragments that parse as a
/// `SELECT` become real IR nodes, everything else (control flow, `DECLARE`,
/// `LOOP`, assignment) becomes `Unmapped` — exactly the plan's stored
/// procedure MVP scope, applied via a text heuristic instead of relying on a
/// procedural-language grammar `sqlparser` doesn't implement.
fn function_to_graph(
    func_name: String,
    body: Option<&CreateFunctionBody>,
    dialect: &dyn Dialect,
    origin: &TransformOrigin,
) -> TransformGraph {
    let body_text = body.and_then(|b| {
        let expr = match b {
            CreateFunctionBody::AsBeforeOptions(e)
            | CreateFunctionBody::AsAfterOptions(e)
            | CreateFunctionBody::Return(e) => e,
        };
        function_body_text(expr)
    });

    let Some(body_text) = body_text else {
        return TransformGraph {
            nodes: vec![TransformNode::Unmapped {
                raw: format!("CREATE FUNCTION {func_name}"),
                reason: "function body not in string-literal form, not modeled".to_string(),
            }],
            edges: Vec::new(),
            origin: origin.clone(),
        };
    };

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for fragment_sql in body_text.split(';') {
        let fragment_sql = fragment_sql.trim();
        if fragment_sql.is_empty() {
            continue;
        }
        match Parser::parse_sql(dialect, fragment_sql) {
            Ok(stmts) if stmts.len() == 1 => {
                if let Statement::Query(query) = &stmts[0] {
                    let fragment = query_to_graph(query, origin.clone());
                    append_fragment(&mut nodes, &mut edges, fragment);
                } else {
                    push(
                        &mut nodes,
                        TransformNode::Unmapped {
                            raw: fragment_sql.to_string(),
                            reason: "control flow present, not modeled".to_string(),
                        },
                    );
                }
            }
            _ => {
                push(
                    &mut nodes,
                    TransformNode::Unmapped {
                        raw: fragment_sql.to_string(),
                        reason: "control flow present, not modeled".to_string(),
                    },
                );
            }
        }
    }

    if nodes.is_empty() {
        nodes.push(TransformNode::Unmapped {
            raw: format!("CREATE FUNCTION {func_name}"),
            reason: "control flow present, not modeled".to_string(),
        });
    }

    TransformGraph {
        nodes,
        edges,
        origin: origin.clone(),
    }
}

fn function_body_text(expr: &Expr) -> Option<String> {
    if let Expr::Value(v) = expr {
        return match v {
            Value::SingleQuotedString(s) | Value::TripleSingleQuotedString(s) => Some(s.clone()),
            Value::DollarQuotedString(dq) => Some(dq.value.clone()),
            _ => None,
        };
    }
    None
}

/// Appends a fragment graph's nodes/edges into an accumulator, offsetting
/// node indices — used when multiple independent statement fragments (each
/// its own `TransformGraph` from `query_to_graph`) are combined into one
/// procedure/function-level graph.
fn append_fragment(
    nodes: &mut Vec<TransformNode>,
    edges: &mut Vec<(NodeId, NodeId)>,
    fragment: TransformGraph,
) {
    let offset = nodes.len() as u32;
    nodes.extend(fragment.nodes);
    edges.extend(
        fragment
            .edges
            .into_iter()
            .map(|(from, to)| (NodeId(from.0 + offset), NodeId(to.0 + offset))),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::dialect::GenericDialect;

    /// Resolves `dialect_name` the same way `recover.rs` does in production — via the shared
    /// registry — rather than duplicating a separate name→`Dialect` table in tests. Names not in
    /// the registry (e.g. `"informix"`, which has no dedicated `sqlparser` dialect) fall back to
    /// `GenericDialect`, matching this crate's documented "accept incomplete coverage" scope.
    fn graphs(sql: &str, dialect_name: &str) -> Vec<TransformGraph> {
        let registry = crate::sql_dialect_registry::build_dialect_registry();
        let dialect: Box<dyn Dialect + Send + Sync> = match registry.get(dialect_name) {
            Some(parser) => parser.sqlparser_dialect(),
            None => Box::new(GenericDialect {}),
        };
        parse_sql_to_transform_graphs(sql, "test.sql", dialect_name, dialect.as_ref())
    }

    /// GitHub issue #3's second root cause: a script with an `UPDATE` and a `SELECT` and no
    /// `;` separating them fails to parse at all — the `statement_repair` fallback recovers
    /// both statements as independent transform graphs.
    #[test]
    fn recovers_statements_from_script_missing_semicolons_between_them() {
        let g = graphs(
            "\
UPDATE customers SET status = 'active' WHERE id = 1

SELECT id, status FROM customers
",
            "mssql",
        );
        assert_eq!(
            g.len(),
            1,
            "UPDATE is not a modeled statement kind, only SELECT is"
        );
        assert!(matches!(&g[0].nodes[0], TransformNode::Source { .. }));
    }

    // ── Golden examples, per the implementation plan's own list ────────────

    #[test]
    fn simple_select_with_where() {
        let g = graphs(
            "SELECT id, status FROM customers WHERE status = 'active'",
            "postgres",
        );
        assert_eq!(g.len(), 1);
        let graph = &g[0];
        assert!(matches!(
            &graph.nodes[0],
            TransformNode::Source { object_name, .. } if object_name == "customers"
        ));
        assert!(matches!(
            &graph.nodes[1],
            TransformNode::Filter { condition } if condition == "status = 'active'"
        ));
        assert_eq!(graph.edges, vec![(NodeId(0), NodeId(1))]);
    }

    #[test]
    fn select_with_join() {
        let g = graphs(
            "SELECT o.id FROM orders o INNER JOIN customers c ON o.customer_id = c.id",
            "postgres",
        );
        let graph = &g[0];
        assert!(
            matches!(&graph.nodes[0], TransformNode::Source { object_name, .. } if object_name == "orders")
        );
        assert!(
            matches!(&graph.nodes[1], TransformNode::Source { object_name, .. } if object_name == "customers")
        );
        match &graph.nodes[2] {
            TransformNode::Join {
                left,
                right,
                keys,
                kind,
            } => {
                assert_eq!(*left, NodeId(0));
                assert_eq!(*right, NodeId(1));
                assert_eq!(*kind, JoinKind::Inner);
                assert_eq!(
                    keys,
                    &vec![("o.customer_id".to_string(), "c.id".to_string())]
                );
            }
            other => panic!("expected Join, got {other:?}"),
        }
        assert!(graph.edges.contains(&(NodeId(0), NodeId(2))));
        assert!(graph.edges.contains(&(NodeId(1), NodeId(2))));
    }

    #[test]
    fn select_with_group_by() {
        let g = graphs(
            "SELECT region, SUM(amount) AS total FROM sales GROUP BY region",
            "postgres",
        );
        let graph = &g[0];
        assert!(matches!(&graph.nodes[0], TransformNode::Source { .. }));
        match &graph.nodes[1] {
            TransformNode::Aggregate { group_by, aggs } => {
                assert_eq!(group_by, &vec!["region".to_string()]);
                assert_eq!(aggs.len(), 1);
                assert_eq!(aggs[0].func, "SUM");
                assert_eq!(aggs[0].arg, "amount");
                assert_eq!(aggs[0].output, "total");
            }
            other => panic!("expected Aggregate, got {other:?}"),
        }
    }

    #[test]
    fn view_wrapping_multi_table_query_gets_a_sink() {
        let g = graphs(
            "CREATE VIEW customer_orders AS \
             SELECT o.id, c.name FROM orders o INNER JOIN customers c ON o.customer_id = c.id",
            "postgres",
        );
        assert_eq!(g.len(), 1);
        let graph = &g[0];
        let last = graph.nodes.last().unwrap();
        assert!(matches!(
            last,
            TransformNode::Sink { object_name, .. } if object_name == "customer_orders"
        ));
        let last_id = NodeId((graph.nodes.len() - 1) as u32);
        assert!(graph.edges.iter().any(|(_, to)| *to == last_id));
    }

    #[test]
    fn stored_procedure_with_embedded_select_and_control_flow() {
        // No trailing `;` before `END` — sqlparser 0.53's `parse_statements`
        // only recognizes `END` as the body terminator right after a
        // statement delimiter; a `;` immediately before `END` makes it try
        // (and fail) to parse `END` itself as a new statement.
        let sql = "CREATE PROCEDURE load_active_customers AS \
                    BEGIN \
                    SELECT id FROM customers WHERE status = 'active'; \
                    DECLARE @x INT \
                    END";
        let g = graphs(sql, "mssql");
        assert_eq!(g.len(), 1);
        let graph = &g[0];

        let has_source = graph
            .nodes
            .iter()
            .any(|n| matches!(n, TransformNode::Source { object_name, .. } if object_name == "customers"));
        assert!(
            has_source,
            "embedded SELECT must produce a real Source node"
        );

        let has_unmapped_control_flow = graph.nodes.iter().any(|n| {
            matches!(n, TransformNode::Unmapped { reason, .. } if reason == "control flow present, not modeled")
        });
        assert!(
            has_unmapped_control_flow,
            "non-SQL statement in the procedure body must become Unmapped, not silently dropped"
        );
    }

    /// Regression test for a real bug found this session (RFC 0039): verified directly against
    /// real `sqlparser` 0.53 behavior that `IF`/`WHILE` have no grammar support at all, in any
    /// dialect — a `CREATE PROCEDURE` body containing either fails the *entire file's* parse,
    /// not just the procedure. Before the whole-file fallback, an unrelated `CREATE VIEW` in the
    /// same file was silently lost too. This must not happen anymore.
    #[test]
    fn independent_statement_survives_when_another_procedure_in_the_same_file_has_unparseable_control_flow()
     {
        let sql = "\
CREATE VIEW active_customers AS SELECT id FROM customers WHERE status = 'active';
CREATE PROCEDURE notify_if_active AS
BEGIN
    IF EXISTS (SELECT 1 FROM customers WHERE status = 'active')
    BEGIN
        SELECT id FROM customers WHERE status = 'active'
    END
END;
";
        // Confirm the premise directly: the whole file really does fail full-file structured
        // parsing under MsSqlDialect because of the IF.
        let registry = crate::sql_dialect_registry::build_dialect_registry();
        let dialect = registry.get("mssql").unwrap().sqlparser_dialect();
        assert!(
            Parser::parse_sql(dialect.as_ref(), sql).is_err(),
            "premise check: sqlparser is expected to reject IF in a procedure body"
        );

        let g = graphs(sql, "mssql");
        let has_recovered_view = g.iter().any(|graph| {
            graph.nodes.iter().any(|n| {
                matches!(n, TransformNode::Sink { object_name, .. } if object_name == "active_customers")
            })
        });
        assert!(
            has_recovered_view,
            "an independent CREATE VIEW earlier in the same file must survive the other \
             procedure's unparseable control flow, not be dropped along with it"
        );

        let has_unmapped_for_the_failing_procedure = g.iter().any(|graph| {
            graph
                .nodes
                .iter()
                .any(|n| matches!(n, TransformNode::Unmapped { .. }))
        });
        assert!(
            has_unmapped_for_the_failing_procedure,
            "the procedure's own unparseable fragments must become Unmapped, not silently \
             vanish either"
        );
    }

    #[test]
    fn function_with_dollar_quoted_body_extracts_embedded_select() {
        let sql = "CREATE FUNCTION active_customer_count() RETURNS INTEGER AS $$ \
                    SELECT id FROM customers WHERE status = 'active'; \
                    DECLARE x INTEGER; \
                    $$ LANGUAGE SQL";
        let g = graphs(sql, "postgres");
        assert_eq!(g.len(), 1);
        let graph = &g[0];

        let has_source = graph
            .nodes
            .iter()
            .any(|n| matches!(n, TransformNode::Source { object_name, .. } if object_name == "customers"));
        assert!(has_source);

        let has_unmapped = graph
            .nodes
            .iter()
            .any(|n| matches!(n, TransformNode::Unmapped { .. }));
        assert!(
            has_unmapped,
            "non-SQL body text must become Unmapped, not dropped"
        );
    }

    // ── Additional coverage ─────────────────────────────────────────────────

    #[test]
    fn calculated_projection_becomes_calculate_node() {
        let g = graphs(
            "SELECT first_name || ' ' || last_name AS full_name FROM customers",
            "postgres",
        );
        let graph = &g[0];
        let calc = graph.nodes.iter().find_map(|n| match n {
            TransformNode::Calculate { output, expr } => Some((output.clone(), expr.clone())),
            _ => None,
        });
        assert_eq!(
            calc,
            Some((
                "full_name".to_string(),
                "first_name || ' ' || last_name".to_string()
            ))
        );
    }

    #[test]
    fn left_join_maps_to_left_join_kind() {
        let g = graphs(
            "SELECT o.id FROM orders o LEFT JOIN customers c ON o.customer_id = c.id",
            "postgres",
        );
        match &g[0].nodes[2] {
            TransformNode::Join { kind, .. } => assert_eq!(*kind, JoinKind::Left),
            other => panic!("expected Join, got {other:?}"),
        }
    }

    #[test]
    fn cte_query_becomes_unmapped_not_dropped() {
        let g = graphs(
            "WITH active AS (SELECT id FROM customers WHERE status = 'active') \
             SELECT id FROM active",
            "postgres",
        );
        assert_eq!(g.len(), 1);
        match &g[0].nodes[0] {
            TransformNode::Unmapped { raw, reason } => {
                assert!(reason.contains("CTE"));
                assert!(!raw.is_empty());
            }
            other => panic!("expected Unmapped, got {other:?}"),
        }
    }

    #[test]
    fn informix_falls_back_to_generic_dialect_and_still_parses_simple_select() {
        let g = graphs("SELECT id FROM customers", "informix");
        assert_eq!(g.len(), 1);
        assert!(
            matches!(&g[0].nodes[0], TransformNode::Source { object_name, .. } if object_name == "customers")
        );
    }

    #[test]
    fn databricks_dialect_parses_simple_select() {
        let g = graphs(
            "SELECT id FROM customers WHERE status = 'active'",
            "databricks",
        );
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].nodes.len(), 2);
    }

    /// RFC 0031: MySQL is now a real `dialect_for` entry, not a `GenericDialect` fallback.
    #[test]
    fn mysql_dialect_parses_simple_select() {
        let g = graphs("SELECT id FROM customers WHERE status = 'active'", "mysql");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].nodes.len(), 2);
    }

    /// The concrete regression case from GitHub issue #3 / devlog_31: a MySQL `#`-style line
    /// comment. `GenericDialect` (the pre-RFC-0031 default for everything) rejects this;
    /// `dialect_for("mysql")` must not.
    #[test]
    fn mysql_dialect_parses_hash_comment_that_generic_dialect_rejects() {
        let sql = "# a mysql-style comment\nSELECT id FROM customers";
        assert!(
            Parser::parse_sql(&GenericDialect {}, sql).is_err(),
            "GenericDialect is expected to reject a leading '#' comment"
        );
        let g = graphs(sql, "mysql");
        assert_eq!(g.len(), 1, "mysql dialect must parse a leading '#' comment");
    }

    #[test]
    fn ddl_statements_produce_no_transform_graphs() {
        let g = graphs(
            "CREATE TABLE customers (id INT PRIMARY KEY, name TEXT)",
            "postgres",
        );
        assert!(g.is_empty());
    }

    #[test]
    fn coverage_percent_reflects_unmapped_ratio() {
        let full = SqlTransformStats {
            dialect: "postgres".into(),
            statements_processed: 1,
            nodes_total: 4,
            nodes_mapped: 4,
        };
        assert_eq!(full.coverage_percent(), 100.0);

        let partial = SqlTransformStats {
            dialect: "postgres".into(),
            statements_processed: 1,
            nodes_total: 4,
            nodes_mapped: 2,
        };
        assert_eq!(partial.coverage_percent(), 50.0);
    }

    #[tokio::test]
    async fn sql_transform_analyzer_pass_produces_transform_node_objects() {
        use ekos_artifact::FileSystemArtifactStore;
        use ekos_compiler_core::{EkosConfig, pass::PassContext};
        use std::sync::Arc as StdArc;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let store = StdArc::new(FileSystemArtifactStore::new(dir.path().join("artifacts")));
        let config = StdArc::new(EkosConfig::default());
        let mut ctx = PassContext::new(config, dir.path().to_path_buf()).with_artifact_store(store);

        let mut pass = SqlTransformAnalyzerPass::new(
            "queries/active_customers.sql",
            "SELECT id, status FROM customers WHERE status = 'active'",
            "postgres",
            Box::new(sqlparser::dialect::PostgreSqlDialect {}),
        );
        let stats_handle = pass.stats_handle();
        pass.run(&mut ctx).await.unwrap();

        let stats = stats_handle.lock().unwrap().clone();
        assert_eq!(stats.statements_processed, 1);
        assert_eq!(stats.nodes_total, 2);
        assert_eq!(stats.nodes_mapped, 2);
        assert_eq!(stats.coverage_percent(), 100.0);
    }
}
