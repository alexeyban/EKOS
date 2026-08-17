//! Gathers the compiled ClickHouse schema out of the ledger (RFC 0056 Stage 2, "analyze
//! metadata" step). Reads the exact `KirObject(ObjectKind::Table)` shape
//! `ClickHouseAnalyzerPass` (`ekos-recovery`) produces — no live fetch, so this is fast and
//! offline-capable, but reflects whatever `ekos build`/`ekos recover` last compiled, not the
//! database's current state.

use ekos_kir::ObjectKind;
use ekos_runtime::{Runtime, RuntimeError};

#[derive(Debug, Clone)]
pub struct TableSchema {
    /// `"<database>.<table>"`, matching `ClickHouseAnalyzerPass`'s object name.
    pub name: String,
    pub columns: Vec<(String, String)>,
    pub engine: String,
    pub order_by: Vec<String>,
}

/// Every compiled `ObjectKind::Table` object tagged `source_system: "clickhouse"` — the full
/// schema, not a fuzzy top-N match, since a query builder needs every table/column it might
/// legitimately reference, not just the ones textually similar to the question.
pub fn gather_clickhouse_schema(runtime: &Runtime<'_>) -> Result<Vec<TableSchema>, RuntimeError> {
    let objects = runtime.list_objects()?;
    Ok(objects
        .into_iter()
        .filter(|obj| {
            obj.kind == ObjectKind::Table
                && obj.properties.get("source_system").and_then(|v| v.as_str())
                    == Some("clickhouse")
        })
        .map(|obj| {
            let columns = obj
                .properties
                .get("columns")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            let name = c.get("name")?.as_str()?.to_string();
                            let data_type = c.get("data_type")?.as_str()?.to_string();
                            Some((name, data_type))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let order_by = obj
                .properties
                .get("order_by")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let engine = obj
                .properties
                .get("engine")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            TableSchema {
                name: obj.name,
                columns,
                engine,
                order_by,
            }
        })
        .collect())
}

/// Renders a schema as prompt text for the query-building LLM call — table name, engine, sort
/// key, and every column with its type.
pub fn render_schema_prompt(tables: &[TableSchema]) -> String {
    let mut out = String::new();
    for table in tables {
        out.push_str(&format!(
            "Table {} (engine={}, order_by={:?})\n",
            table.name, table.engine, table.order_by
        ));
        for (name, data_type) in &table.columns {
            out.push_str(&format!("  - {name}: {data_type}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirObject;
    use ekos_ledger::Ledger;
    use tempfile::TempDir;

    fn temp_ledger() -> (Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ledger.db");
        (Ledger::open(&path).unwrap(), dir)
    }

    #[test]
    fn gathers_only_clickhouse_tagged_tables() {
        let (ledger, _dir) = temp_ledger();

        let mut ch_table = KirObject::new("analytics.orders", ObjectKind::Table);
        ch_table.properties.insert(
            "columns".into(),
            serde_json::json!([{"name": "id", "data_type": "UInt64"}]),
        );
        ch_table
            .properties
            .insert("engine".into(), serde_json::json!("MergeTree"));
        ch_table
            .properties
            .insert("order_by".into(), serde_json::json!(["id"]));
        ch_table
            .properties
            .insert("source_system".into(), serde_json::json!("clickhouse"));
        ledger.append_object(&ch_table).unwrap();

        // A same-kind object from a different source system must not leak in.
        let mut pg_table = KirObject::new("customers", ObjectKind::Table);
        pg_table
            .properties
            .insert("columns".into(), serde_json::json!([]));
        ledger.append_object(&pg_table).unwrap();

        let runtime = Runtime::new(&ledger);
        let tables = gather_clickhouse_schema(&runtime).unwrap();

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "analytics.orders");
        assert_eq!(
            tables[0].columns,
            vec![("id".to_string(), "UInt64".to_string())]
        );
        assert_eq!(tables[0].engine, "MergeTree");
    }

    #[test]
    fn renders_readable_schema_prompt() {
        let tables = vec![TableSchema {
            name: "analytics.orders".into(),
            columns: vec![("id".into(), "UInt64".into())],
            engine: "MergeTree".into(),
            order_by: vec!["id".into()],
        }];
        let prompt = render_schema_prompt(&tables);
        assert!(prompt.contains("Table analytics.orders"));
        assert!(prompt.contains("id: UInt64"));
    }
}
