//! EKL interpreter — compiles an `EklAst` to `Runtime` calls and evaluates
//! predicates generically over `serde_json::Value` rows (RFC 0010).

use crate::parser::{EklAst, Entity, Literal, Op, Order, Predicate};
use ekos_kir::{KirId, KirObject, KirRelationship};
use ekos_runtime::{Runtime, RuntimeError};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use thiserror::Error;

pub type Row = HashMap<String, Value>;

#[derive(Debug, Error)]
pub enum EklError {
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("FROM anchor not found: '{0}'")]
    AnchorNotFound(String),
}

/// A table of result rows, one `HashMap<String, Value>` per row.
#[derive(Debug, Clone)]
pub struct EklResult {
    pub rows: Vec<Row>,
}

/// Default projected columns when a query omits `RETURN`.
pub fn default_returns(entity: &Entity) -> Vec<String> {
    match entity {
        Entity::Object => ["id", "name", "kind"].iter().map(|s| s.to_string()).collect(),
        Entity::Relationship => ["id", "kind", "from", "to"].iter().map(|s| s.to_string()).collect(),
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

        if let Some((field, order)) = &ast.order_by {
            rows.sort_by(|a, b| compare_rows(a, b, field, *order));
        }

        let returns = if ast.returns.is_empty() { default_returns(&ast.entity) } else { ast.returns.clone() };
        let mut rows: Vec<Row> = rows.iter().map(|row| project(row, &returns)).collect();

        if let Some(limit) = ast.limit {
            rows.truncate(limit as usize);
        }

        Ok(EklResult { rows })
    }

    fn candidate_rows(&self, ast: &EklAst) -> Result<Vec<Row>, EklError> {
        match (&ast.entity, &ast.from) {
            (Entity::Object, None) => Ok(self.runtime.list_objects()?.iter().map(object_row).collect()),
            (Entity::Relationship, None) => {
                Ok(self.runtime.list_relationships()?.iter().map(relationship_row).collect())
            }
            (Entity::Object, Some(name)) => {
                let anchor = self.resolve_anchor(name)?;
                let graph = self.runtime.load_neighborhood(&anchor, 1)?;
                Ok(graph.objects.iter().map(object_row).collect())
            }
            (Entity::Relationship, Some(name)) => {
                let anchor = self.resolve_anchor(name)?;
                let graph = self.runtime.load_neighborhood(&anchor, 1)?;
                Ok(graph.relationships.iter().map(relationship_row).collect())
            }
        }
    }

    fn resolve_anchor(&self, name: &str) -> Result<KirId, EklError> {
        self.runtime
            .find_objects(name)?
            .into_iter()
            .next()
            .map(|(id, _)| id)
            .ok_or_else(|| EklError::AnchorNotFound(name.to_string()))
    }
}

fn object_row(obj: &KirObject) -> Row {
    let mut row = Row::new();
    row.insert("id".into(), Value::String(obj.id.to_string()));
    row.insert("name".into(), Value::String(obj.name.clone()));
    row.insert("kind".into(), Value::String(obj.kind.to_string()));
    row.insert(
        "evidence".into(),
        Value::Array(obj.evidence.iter().map(|id| Value::String(id.to_string())).collect()),
    );
    row.insert("created_at".into(), Value::String(obj.created_at.to_rfc3339()));
    row
}

fn relationship_row(rel: &KirRelationship) -> Row {
    let mut row = Row::new();
    row.insert("id".into(), Value::String(rel.id.to_string()));
    row.insert("kind".into(), Value::String(format!("{:?}", rel.kind)));
    row.insert("from".into(), Value::String(rel.from.to_string()));
    row.insert("to".into(), Value::String(rel.to.to_string()));
    row.insert("created_at".into(), Value::String(rel.created_at.to_rfc3339()));
    row
}

fn project(row: &Row, returns: &[String]) -> Row {
    returns.iter().map(|f| (f.clone(), row.get(f).cloned().unwrap_or(Value::Null))).collect()
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
    let Some(v) = row.get(&pred.field) else { return false };
    match pred.op {
        Op::Eq => value_eq(v, &pred.value),
        Op::Ne => !value_eq(v, &pred.value),
        Op::Contains => value_to_string(v).contains(&literal_to_string(&pred.value)),
        Op::Gt | Op::Lt | Op::Ge | Op::Le => {
            let (Some(a), Some(b)) = (value_as_f64(v), literal_as_f64(&pred.value)) else { return false };
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
        (Some(Value::Number(x)), Some(Value::Number(y))) => {
            x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(Ordering::Equal)
        }
        (Some(x), Some(y)) => value_to_string(x).cmp(&value_to_string(y)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    if order == Order::Desc { ordering.reverse() } else { ordering }
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
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, orders_id, customers.id))
            .unwrap();
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, orders_id, order_items.id))
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
        let result = run(&rt, "FIND Object WHERE kind = 'Table' ORDER BY name LIMIT 1");
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
        let result = run(&rt, "FIND Relationship WHERE kind = 'ForeignKey' FROM 'orders'");
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
        let result = run(&rt, "FIND Relationship WHERE kind = 'ForeignKey' RETURN from, to LIMIT 1");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 2);
        assert!(result.rows[0].contains_key("from") && result.rows[0].contains_key("to"));
    }

    #[test]
    fn unknown_anchor_returns_error() {
        let (ledger, _dir) = fixture();
        let rt = Runtime::new(&ledger);
        let ast = ekl_parse("FIND Object FROM 'zzznonexistent'").unwrap();
        let err = EklInterpreter::new(&rt).execute(&ast).unwrap_err();
        assert!(matches!(err, EklError::AnchorNotFound(_)));
    }
}
