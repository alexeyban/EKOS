//! EKL interpreter — compiles an `EklAst` to `Runtime` calls and evaluates
//! predicates generically over `serde_json::Value` rows (RFC 0010).

use crate::parser::{EklAst, Entity, Literal, Op, Order, Predicate};
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, RelationshipKind};
use ekos_runtime::{ImpactDirection, RetrievalRequest, Runtime, RuntimeError};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

pub type Row = HashMap<String, Value>;

#[derive(Debug, Error)]
pub enum EklError {
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("FROM anchor not found: '{0}'")]
    AnchorNotFound(String),
    /// RFC 0096: `AS OF` only reconstructs bulk `FIND Object`/`FIND
    /// Relationship` snapshots today — `FROM`-anchored expansion
    /// (`load_neighborhood`/`trace_impact`) has no time-aware equivalent yet.
    /// Rejected explicitly rather than silently ignoring `AS OF` and
    /// returning current-state neighborhood data under a historical query.
    #[error("AS OF is not yet supported combined with FROM-anchored queries")]
    AsOfWithFromUnsupported,
}

/// A table of result rows, one `HashMap<String, Value>` per row.
#[derive(Debug, Clone)]
pub struct EklResult {
    pub rows: Vec<Row>,
}

/// Default projected columns when a query omits `RETURN`.
pub fn default_returns(entity: &Entity) -> Vec<String> {
    match entity {
        Entity::Object => ["id", "name", "kind"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        Entity::Relationship => ["id", "kind", "from", "to"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

/// Executes `EklAst` queries against a `Runtime`. Never touches the ledger
/// directly.
pub struct EklInterpreter<'a> {
    runtime: &'a Runtime<'a>,
}

impl<'a> EklInterpreter<'a> {
    pub fn new(runtime: &'a Runtime<'a>) -> Self {
        Self { runtime }
    }

    pub fn execute(&self, ast: &EklAst) -> Result<EklResult, EklError> {
        let mut rows = self.candidate_rows(ast)?;
        rows.retain(|row| ast.predicates.iter().all(|p| eval_predicate(row, p)));

        if ast.count {
            let mut rows = aggregate_count(&rows, ast.group_by.as_deref());
            if let Some((field, order)) = &ast.order_by {
                rows.sort_by(|a, b| compare_rows(a, b, field, *order));
            }
            if let Some(limit) = ast.limit {
                rows.truncate(limit as usize);
            }
            return Ok(EklResult { rows });
        }

        if let Some((field, order)) = &ast.order_by {
            rows.sort_by(|a, b| compare_rows(a, b, field, *order));
        }

        let returns = if ast.returns.is_empty() {
            default_returns(&ast.entity)
        } else {
            ast.returns.clone()
        };
        let mut rows: Vec<Row> = rows.iter().map(|row| project(row, &returns)).collect();

        if let Some(limit) = ast.limit {
            rows.truncate(limit as usize);
        }

        Ok(EklResult { rows })
    }

    fn candidate_rows(&self, ast: &EklAst) -> Result<Vec<Row>, EklError> {
        if let Some(at) = ast.as_of {
            if ast.from.is_some() {
                return Err(EklError::AsOfWithFromUnsupported);
            }
            return Ok(match ast.entity {
                Entity::Object => self
                    .runtime
                    .list_objects_at(at)?
                    .iter()
                    .map(object_row)
                    .collect(),
                Entity::Relationship => self
                    .runtime
                    .list_relationships_at(at)?
                    .iter()
                    .map(relationship_row)
                    .collect(),
            });
        }
        match (&ast.entity, &ast.from) {
            (Entity::Object, None) => Ok(self
                .runtime
                .list_objects()?
                .iter()
                .map(object_row)
                .collect()),
            (Entity::Relationship, None) => Ok(self
                .runtime
                .list_relationships()?
                .iter()
                .map(relationship_row)
                .collect()),
            (Entity::Object, Some(name)) => {
                let anchor = self.resolve_anchor(name)?;
                let graph = self.expand_from_anchor(anchor, ast)?;
                Ok(graph.objects.iter().map(object_row).collect())
            }
            (Entity::Relationship, Some(name)) => {
                let anchor = self.resolve_anchor(name)?;
                let graph = self.expand_from_anchor(anchor, ast)?;
                Ok(graph.relationships.iter().map(relationship_row).collect())
            }
        }
    }

    fn resolve_anchor(&self, name: &str) -> Result<KirId, EklError> {
        // RFC 0119: route through the retrieval seam. Phase 0 keeps "top hit wins" exactly.
        self.runtime
            .retrieve(&RetrievalRequest::lexical(name))?
            .hits
            .into_iter()
            .next()
            .map(|hit| hit.id)
            .ok_or_else(|| EklError::AnchorNotFound(name.to_string()))
    }

    /// Expands a `FROM` anchor per RFC 0018: without `VIA`, generalizes the
    /// original single-hop `load_neighborhood(anchor, 1)` to
    /// `load_neighborhood(anchor, depth)` (default 1, fully backward
    /// compatible). With `VIA <kind>`, delegates to the directed,
    /// kind-filtered `trace_impact` (dependencies direction — tracing
    /// outward along the named kind, matching RFC 0010's own FK-chasing
    /// example) and reassembles a `KirGraph` so the existing row
    /// projections keep working unchanged.
    fn expand_from_anchor(&self, anchor: KirId, ast: &EklAst) -> Result<KirGraph, EklError> {
        let depth = ast.depth.unwrap_or(1);
        match &ast.via {
            None => Ok(self.runtime.load_neighborhood(&anchor, depth)?),
            Some(kind_name) => {
                let kind = RelationshipKind::from_str(kind_name).expect("infallible");
                let hops = self.runtime.trace_impact(
                    &anchor,
                    ImpactDirection::Dependencies,
                    &[kind],
                    depth,
                )?;
                let mut graph = KirGraph::new();
                if let Some(root) = self.runtime.load_object(&anchor)? {
                    graph.add_object(root);
                }
                for hop in hops {
                    graph.add_object(hop.object);
                    graph.add_relationship(hop.via);
                }
                Ok(graph)
            }
        }
    }
}

fn object_row(obj: &KirObject) -> Row {
    let mut row = Row::new();
    row.insert("id".into(), Value::String(obj.id.to_string()));
    row.insert("name".into(), Value::String(obj.name.clone()));
    row.insert("kind".into(), Value::String(obj.kind.to_string()));
    row.insert(
        "evidence".into(),
        Value::Array(
            obj.evidence
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    row.insert(
        "created_at".into(),
        Value::String(obj.created_at.to_rfc3339()),
    );
    row
}

fn relationship_row(rel: &KirRelationship) -> Row {
    let mut row = Row::new();
    row.insert("id".into(), Value::String(rel.id.to_string()));
    // Display, not Debug: matches object_row's obj.kind.to_string() above.
    // Debug happened to agree with Display for every built-in unit-variant
    // RelationshipKind (ForeignKey, CoupledWith, ...), which hid this bug
    // until a Custom(String) kind existed (RFC 0017's crypto connector) —
    // Debug renders that as `Custom("DEPLOYED")`, not `DEPLOYED`, breaking
    // `WHERE kind = 'DEPLOYED'`.
    row.insert("kind".into(), Value::String(rel.kind.to_string()));
    row.insert("from".into(), Value::String(rel.from.to_string()));
    row.insert("to".into(), Value::String(rel.to.to_string()));
    row.insert(
        "created_at".into(),
        Value::String(rel.created_at.to_rfc3339()),
    );
    row
}

/// RFC 0096: `COUNT`/`GROUP BY` aggregation over already-filtered rows.
/// Grouped counts are expressed as ordinary `Row`s (`{"<field>": key,
/// "count": N}`) rather than a new `EklResult` variant — the existing flat
/// `Vec<Row>` contract already covers this shape, so `ORDER BY`/`LIMIT`
/// (which operate generically on any `Row`) keep working unchanged on
/// aggregate output too. Without `group_by`, yields exactly one row
/// `{"count": N}`. Group rows are sorted by key ascending by default, for a
/// deterministic result independent of the input rows' original order.
fn aggregate_count(rows: &[Row], group_by: Option<&str>) -> Vec<Row> {
    match group_by {
        None => {
            let mut row = Row::new();
            row.insert("count".into(), Value::Number(rows.len().into()));
            vec![row]
        }
        Some(field) => {
            let mut counts: HashMap<String, u64> = HashMap::new();
            for row in rows {
                let key = row.get(field).map(value_to_string).unwrap_or_default();
                *counts.entry(key).or_insert(0) += 1;
            }
            let mut out: Vec<Row> = counts
                .into_iter()
                .map(|(key, count)| {
                    let mut row = Row::new();
                    row.insert(field.to_string(), Value::String(key));
                    row.insert("count".into(), Value::Number(count.into()));
                    row
                })
                .collect();
            out.sort_by(|a, b| value_to_string(&a[field]).cmp(&value_to_string(&b[field])));
            out
        }
    }
}

fn project(row: &Row, returns: &[String]) -> Row {
    returns
        .iter()
        .map(|f| (f.clone(), row.get(f).cloned().unwrap_or(Value::Null)))
        .collect()
}

/// String rendering of a `Value` for text-based predicate comparisons.
/// Strings render without their JSON quotes; arrays join their elements with
/// commas; everything else uses its JSON representation.
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(value_to_string).collect::<Vec<_>>().join(","),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn literal_as_f64(lit: &Literal) -> Option<f64> {
    match lit {
        Literal::Num(n) => Some(*n),
        Literal::Str(s) => s.parse().ok(),
    }
}

fn literal_to_string(lit: &Literal) -> String {
    match lit {
        Literal::Str(s) => s.clone(),
        Literal::Num(n) => n.to_string(),
    }
}

fn value_eq(v: &Value, lit: &Literal) -> bool {
    match lit {
        Literal::Str(s) => &value_to_string(v) == s,
        Literal::Num(n) => value_as_f64(v).map(|f| f == *n).unwrap_or(false),
    }
}

/// Evaluates one predicate against a row. Numeric comparisons on
/// non-numeric fields evaluate to `false` rather than erroring (RFC 0010 v0
/// simplification).
fn eval_predicate(row: &Row, pred: &Predicate) -> bool {
    let Some(v) = row.get(&pred.field) else {
        return false;
    };
    match pred.op {
        Op::Eq => value_eq(v, &pred.value),
        Op::Ne => !value_eq(v, &pred.value),
        Op::Contains => value_to_string(v).contains(&literal_to_string(&pred.value)),
        Op::Gt | Op::Lt | Op::Ge | Op::Le => {
            let (Some(a), Some(b)) = (value_as_f64(v), literal_as_f64(&pred.value)) else {
                return false;
            };
            match pred.op {
                Op::Gt => a > b,
                Op::Lt => a < b,
                Op::Ge => a >= b,
                Op::Le => a <= b,
                _ => unreachable!(),
            }
        }
    }
}

fn compare_rows(a: &Row, b: &Row, field: &str, order: Order) -> Ordering {
    let ordering = match (a.get(field), b.get(field)) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(x), Some(y)) => value_to_string(x).cmp(&value_to_string(y)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    if order == Order::Desc {
        ordering.reverse()
    } else {
        ordering
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ekl_parse;
    use ekos_kir::{ObjectKind, RelationshipKind};
    use ekos_ledger::Ledger;
    use tempfile::TempDir;

    fn fixture() -> (Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();

        let orders = KirObject::new("orders", ObjectKind::Table);
        let customers = KirObject::new("customers", ObjectKind::Table);
        let order_items = KirObject::new("order_items", ObjectKind::Table);
        let orders_id = orders.id;

        ledger.append_object(&orders).unwrap();
        ledger.append_object(&customers).unwrap();
        ledger.append_object(&order_items).unwrap();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                orders_id,
                customers.id,
            ))
            .unwrap();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                orders_id,
                order_items.id,
            ))
            .unwrap();

        (ledger, dir)
    }

    fn run(runtime: &Runtime, query: &str) -> EklResult {
        let ast = ekl_parse(query).unwrap();
        EklInterpreter::new(runtime).execute(&ast).unwrap()
    }

    #[test]
    fn example_1_all_tables() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE kind = 'Table'");
        assert_eq!(result.rows.len(), 3);
        assert!(result.rows[0].contains_key("id"));
    }

    #[test]
    fn example_2_return_name_only() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE kind = 'Table' RETURN name");
        assert_eq!(result.rows.len(), 3);
        assert_eq!(result.rows[0].len(), 1);
        assert!(result.rows[0].contains_key("name"));
    }

    #[test]
    fn example_3_exact_name_match() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE name = 'orders'");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["name"], Value::String("orders".into()));
    }

    #[test]
    fn example_4_contains_predicate() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE name CONTAINS 'order'");
        assert_eq!(result.rows.len(), 2);
    }

    #[test]
    fn example_5_order_by_and_limit() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(
            &rt,
            "FIND Object WHERE kind = 'Table' ORDER BY name LIMIT 1",
        );
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["name"], Value::String("customers".into()));
    }

    #[test]
    fn example_6_all_foreign_keys() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Relationship WHERE kind = 'ForeignKey'");
        assert_eq!(result.rows.len(), 2);
    }

    #[test]
    fn example_7_relationships_from_anchor() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(
            &rt,
            "FIND Relationship WHERE kind = 'ForeignKey' FROM 'orders'",
        );
        assert_eq!(result.rows.len(), 2);
    }

    #[test]
    fn example_8_object_neighbourhood() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object FROM 'orders'");
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn example_9_no_matches() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE kind = 'Service'");
        assert!(result.rows.is_empty());
    }

    #[test]
    fn example_10_return_projection_with_limit() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(
            &rt,
            "FIND Relationship WHERE kind = 'ForeignKey' RETURN from, to LIMIT 1",
        );
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 2);
        assert!(result.rows[0].contains_key("from") && result.rows[0].contains_key("to"));
    }

    #[test]
    fn via_kind_filters_and_traces_dependencies() {
        let (ledger, _dir) = fixture();
        // Add a non-FK edge from orders that VIA ForeignKey must not follow.
        let shipping = KirObject::new("shipping_notes", ObjectKind::Table);
        let shipping_id = shipping.id;
        ledger.append_object(&shipping).unwrap();
        let orders_id = ledger
            .find_objects("orders")
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .0;
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::CoupledWith,
                orders_id,
                shipping_id,
            ))
            .unwrap();

        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object VIA ForeignKey FROM 'orders'");
        // orders itself + its two FK neighbours (customers, order_items);
        // shipping_notes (CoupledWith) must be excluded.
        assert_eq!(result.rows.len(), 3);
        assert!(
            result
                .rows
                .iter()
                .all(|r| r["name"] != Value::String("shipping_notes".into()))
        );
    }

    #[test]
    fn depth_without_via_generalizes_single_hop_default() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object FROM 'orders' DEPTH 1");
        assert_eq!(result.rows.len(), 3, "same as no-DEPTH default");
    }

    #[test]
    fn unknown_anchor_returns_error() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let ast = ekl_parse("FIND Object FROM 'zzznonexistent'").unwrap();
        let err = EklInterpreter::new(&rt).execute(&ast).unwrap_err();
        assert!(matches!(err, EklError::AnchorNotFound(_)));
    }

    #[test]
    fn finds_objects_of_a_newly_added_object_kind() {
        // AD-001: demonstrates the taxonomy expansion needs zero EKL changes —
        // `WHERE kind = 'Person'` works purely off the new ObjectKind variant's
        // Display impl, same as any pre-existing kind.
        let (ledger, _dir) = fixture();
        ledger
            .append_object(&KirObject::new("Alice", ObjectKind::Person))
            .unwrap();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE kind = 'Person'");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["name"], Value::String("Alice".into()));
    }

    #[test]
    fn as_of_before_anything_written_returns_empty() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let before = (chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
        let query = format!("FIND Object AS OF '{before}'");
        let result = run(&rt, &query);
        assert!(result.rows.is_empty());
    }

    #[test]
    fn as_of_now_returns_current_objects() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let now = chrono::Utc::now().to_rfc3339();
        let query = format!("FIND Object WHERE kind = 'Table' AS OF '{now}'");
        let result = run(&rt, &query);
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn as_of_reconstructs_a_past_version_not_a_later_update() {
        let (ledger, _dir) = fixture();
        let orders_id = ledger
            .find_objects("orders")
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .0;
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mid = chrono::Utc::now();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let mut orders = ledger.get_object(&orders_id).unwrap().unwrap();
        orders
            .properties
            .insert("row_count".into(), serde_json::json!(42));
        ledger.append_object(&orders).unwrap();

        let rt = Runtime::new(&ledger);
        let query = format!(
            "FIND Object WHERE name = 'orders' AS OF '{}'",
            mid.to_rfc3339()
        );
        let result = run(&rt, &query);
        assert_eq!(result.rows.len(), 1);
        // RETURN wasn't specified, so default_returns doesn't include
        // properties — assert via a fresh historical read through the same
        // Runtime the interpreter uses, confirming candidate_rows really
        // used list_objects_at rather than the current-state list.
        let historical = rt.reconstruct_state_at(&orders_id, mid).unwrap().unwrap();
        assert!(!historical.object.properties.contains_key("row_count"));
    }

    #[test]
    fn as_of_combined_with_from_is_rejected() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let ast = ekl_parse("FIND Object FROM 'orders' AS OF '2026-01-01T00:00:00Z'").unwrap();
        let err = EklInterpreter::new(&rt).execute(&ast).unwrap_err();
        assert!(matches!(err, EklError::AsOfWithFromUnsupported));
    }

    #[test]
    fn bare_count_returns_one_row_with_total() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object WHERE kind = 'Table' COUNT");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["count"], Value::Number(3.into()));
    }

    #[test]
    fn count_group_by_kind_groups_correctly() {
        let (ledger, _dir) = fixture();
        ledger
            .append_object(&KirObject::new("Alice", ObjectKind::Person))
            .unwrap();
        ledger
            .append_object(&KirObject::new("Bob", ObjectKind::Person))
            .unwrap();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object COUNT GROUP BY kind");
        assert_eq!(result.rows.len(), 2, "two distinct kinds: Table, Person");
        let table_row = result
            .rows
            .iter()
            .find(|r| r["kind"] == Value::String("Table".into()))
            .unwrap();
        assert_eq!(table_row["count"], Value::Number(3.into()));
        let person_row = result
            .rows
            .iter()
            .find(|r| r["kind"] == Value::String("Person".into()))
            .unwrap();
        assert_eq!(person_row["count"], Value::Number(2.into()));
    }

    #[test]
    fn count_group_by_respects_where_predicate_first() {
        let (ledger, _dir) = fixture();
        ledger
            .append_object(&KirObject::new("Alice", ObjectKind::Person))
            .unwrap();
        let rt = Runtime::new(&ledger);
        // WHERE filters to Table rows only, before grouping — must not see Person.
        let result = run(&rt, "FIND Object WHERE kind = 'Table' COUNT GROUP BY kind");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["kind"], Value::String("Table".into()));
    }

    #[test]
    fn count_group_by_honors_limit() {
        let (ledger, _dir) = fixture();
        ledger
            .append_object(&KirObject::new("Alice", ObjectKind::Person))
            .unwrap();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Object COUNT GROUP BY kind LIMIT 1");
        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn finds_relationships_of_a_custom_kind() {
        // Regression: relationship_row used to format `kind` with `{:?}` (Debug),
        // which renders Custom("DEPLOYED") as the literal string `Custom("DEPLOYED")`
        // instead of `DEPLOYED` — silently breaking `WHERE kind = '...'` for every
        // Custom relationship kind (RFC 0017's crypto connector is the first user).
        // Built-in kinds masked this because Debug of a bare unit variant happens
        // to equal its Display output.
        let (ledger, _dir) = fixture();
        let wallet_obj = KirObject::new("Dep1", ObjectKind::Custom("Wallet".into()));
        let token_obj = KirObject::new("Mint1", ObjectKind::Custom("Token".into()));
        let (wallet_id, token_id) = (wallet_obj.id, token_obj.id);
        ledger.append_object(&wallet_obj).unwrap();
        ledger.append_object(&token_obj).unwrap();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::Custom("DEPLOYED".into()),
                wallet_id,
                token_id,
            ))
            .unwrap();
        let rt = Runtime::new(&ledger);
        let result = run(&rt, "FIND Relationship WHERE kind = 'DEPLOYED'");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["kind"], Value::String("DEPLOYED".into()));
    }
}
