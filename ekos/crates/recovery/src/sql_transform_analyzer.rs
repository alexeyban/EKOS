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
//!   `Unmapped` with reason `"control flow present, not modeled"`.
//! - Dialects: Postgres and MSSQL (T-SQL) via their native `sqlparser`
//!   dialects; Databricks via `sqlparser`'s native `DatabricksDialect`
//!   (coverage not independently verified against real Spark SQL
//!   extensions — flagged, not blocking); Informix has no dedicated
//!   `sqlparser` dialect, so it falls back to `GenericDialect` and accepts
//!   incomplete coverage, per the plan's explicit scoping.
//!
//! Dialect selection is not yet wired to `ekos.toml` — `SqlTransformAnalyzerPass::new`
//! takes a dialect name directly and `recover.rs`'s existing SQL-file walk
//! (which has no per-file dialect config today) passes `"generic"`.
//! Config-driven per-path dialect selection is future work, not blocking
//! this phase.

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
use sqlparser::dialect::{
    DatabricksDialect, Dialect, GenericDialect, MsSqlDialect, PostgreSqlDialect,
};
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
    stats: Arc<Mutex<SqlTransformStats>>,
}

impl SqlTransformAnalyzerPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        dialect_name: impl Into<String>,
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
        let graphs =
            parse_sql_to_transform_graphs(&self.sql, &self.source_path, &self.dialect_name);

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

fn dialect_for(name: &str) -> Box<dyn Dialect> {
    match name {
        "postgres" | "postgresql" => Box::new(PostgreSqlDialect {}),
        "mssql" | "tsql" | "synapse" => Box::new(MsSqlDialect {}),
        "databricks" | "spark" => Box::new(DatabricksDialect {}),
        // Informix has no dedicated sqlparser dialect — Generic accepts
        // incomplete coverage, per the implementation plan's explicit scope.
        _ => Box::new(GenericDialect {}),
    }
}

fn source_kind_for(dialect_name: &str) -> &'static str {
    match dialect_name {
        "postgres" | "postgresql" => "sql-postgres",
        "mssql" | "tsql" | "synapse" => "sql-mssql",
        "databricks" | "spark" => "sql-databricks",
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
) -> Vec<TransformGraph> {
    let dialect = dialect_for(dialect_name);
    let source_kind = source_kind_for(dialect_name);

    let stmts = match Parser::parse_sql(dialect.as_ref(), sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                "sql-transform-analyzer: sqlparser failed on {source_path} ({dialect_name}): {e}"
            );
            return Vec::new();
        }
    };

    let mut graphs = Vec::new();
    for (index, stmt) in stmts.iter().enumerate() {
        let origin = || TransformOrigin {
            source_path: format!("{source_path}#{index}"),
            source_kind: source_kind.to_string(),
            extracted_at: chrono::Utc::now(),
        };

        match stmt {
            Statement::Query(query) => {
                graphs.push(query_to_graph(query, origin()));
            }
            Statement::CreateView { name, query, .. } => {
                let mut graph = query_to_graph(query, origin());
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
                graphs.push(graph);
            }
            Statement::CreateProcedure { name, body, .. } => {
                graphs.push(procedure_body_to_graph(name.to_string(), body, &origin()));
            }
            Statement::CreateFunction(cf) => {
                graphs.push(function_to_graph(
                    cf.name.to_string(),
                    cf.function_body.as_ref(),
                    dialect.as_ref(),
                    &origin(),
                ));
            }
            _ => {}
        }
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

    fn graphs(sql: &str, dialect: &str) -> Vec<TransformGraph> {
        parse_sql_to_transform_graphs(sql, "test.sql", dialect)
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
