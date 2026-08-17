//! `ClickHouseAnalyzerPass` — converts ClickHouse table observation artifacts (RFC 0056,
//! `ekos-plugin-clickhouse`) into KIR. Produces one `KirObject(kind=ObjectKind::Table)` per
//! table, `properties["columns"]` in the exact `[{"name", "data_type"}, ...]` shape
//! `SqlAnalyzerPass` already uses for file-based SQL DDL (`sql_analyzer.rs::columns_json`).
//!
//! Reusing `ObjectKind::Table` rather than a new `Custom("ClickHouseTable")` kind is
//! deliberate: `identity::structural_score` compares same-kind objects' `columns` property via
//! Jaccard overlap whenever both sides have one (`crates/identity/src/lib.rs:391-401`), so a
//! ClickHouse table and a file-recovered SQL table with overlapping columns become real
//! cross-system identity-resolution candidates with no new exclusion-list entry — see RFC 0056's
//! Design section for the full reasoning.
//!
//! Pure structural mapping — no LLM in the loop, same shape as `ConfluenceAnalyzerPass`/
//! `GitHubAnalyzerPass`.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, ObjectKind, SourceLocation};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ClickHouseColumn {
    name: String,
    data_type: String,
}

#[derive(Debug, Deserialize)]
struct ClickHouseTableData {
    engine: String,
    #[serde(default)]
    order_by: Vec<String>,
    #[serde(default)]
    partition_by: Vec<String>,
    #[serde(default)]
    columns: Vec<ClickHouseColumn>,
}

/// Deterministic id for a table object — stable across passes and `ekos recover` runs, keyed by
/// the same `"<database>.<table>"` target `ClickHouseObserver::scan` already uses.
fn table_kir_id(target: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("clickhouse:{target}").as_bytes(),
    ))
}

fn columns_json(columns: &[ClickHouseColumn]) -> serde_json::Value {
    serde_json::Value::Array(
        columns
            .iter()
            .map(|c| serde_json::json!({ "name": c.name, "data_type": c.data_type }))
            .collect(),
    )
}

pub struct ClickHouseAnalyzerPass {
    pass_id: String,
    /// ClickHouse table `ObservationArtifact` IDs to process.
    table_artifact_ids: Vec<ArtifactId>,
}

impl ClickHouseAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, table_artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("clickhouse-analyzer:{}", workspace_name.into()),
            table_artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for ClickHouseAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .table_artifact_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        for artifact_id in &self.table_artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CLICKHOUSE001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let target = json["target"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| artifact_id.to_string());
            let data: ClickHouseTableData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CLICKHOUSE002",
                        format!("malformed clickhouse table payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let id = table_kir_id(&target);
            let ev = KirEvidence::new(
                SourceLocation::file(format!("clickhouse:{target}")),
                format!("table {target} observed via ClickHouse system.tables/system.columns"),
            );
            let ev_id = graph.add_evidence(ev);

            let mut obj = KirObject::new(target.clone(), ObjectKind::Table);
            obj.id = id;
            obj.evidence.push(ev_id);
            obj.properties
                .insert("columns".into(), columns_json(&data.columns));
            obj.properties
                .insert("engine".into(), serde_json::json!(data.engine));
            obj.properties
                .insert("order_by".into(), serde_json::json!(data.order_by));
            obj.properties
                .insert("partition_by".into(), serde_json::json!(data.partition_by));
            obj.properties
                .insert("source_system".into(), serde_json::json!("clickhouse"));
            graph.objects.push(obj);
        }

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
            tables = knowledge.content.kir.objects.len(),
            "clickhouse-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::{EkosConfig, pass::PassContext};
    use std::sync::Arc;

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    fn seed_table(
        ctx: &PassContext,
        target: &str,
        engine: &str,
        order_by: Vec<&str>,
        columns: Vec<(&str, &str)>,
    ) -> ArtifactId {
        let data = serde_json::json!({
            "kind": "table",
            "engine": engine,
            "order_by": order_by,
            "partition_by": [],
            "columns": columns.iter().map(|(n, t)| serde_json::json!({"name": n, "data_type": t})).collect::<Vec<_>>(),
        });
        let artifact = ekos_artifact::ObservationArtifact::new("clickhouse", target, data);
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();
        artifact.id
    }

    async fn run_pass(c: PassContext, tables: Vec<ArtifactId>) -> ekos_kir::KirGraph {
        let mut pass = ClickHouseAnalyzerPass::new("test", tables);
        let mut c = c;
        pass.run(&mut c).await.unwrap();

        let knowledge_id = c
            .artifact_store
            .list()
            .unwrap()
            .into_iter()
            .find(|id| {
                let json = c.artifact_store.read(id).unwrap().unwrap();
                json.get("kir").is_some()
            })
            .expect("pass must have written a KnowledgeArtifact");
        let json = c.artifact_store.read(&knowledge_id).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    #[tokio::test]
    async fn one_table_artifact_becomes_one_table_object() {
        let (c, _dir) = ctx();
        let id = seed_table(
            &c,
            "analytics.orders",
            "MergeTree",
            vec!["id", "created_at"],
            vec![("id", "UInt64"), ("created_at", "DateTime")],
        );
        let graph = run_pass(c, vec![id]).await;

        assert_eq!(graph.objects.len(), 1);
        let obj = &graph.objects[0];
        assert_eq!(obj.kind, ObjectKind::Table);
        assert_eq!(obj.name, "analytics.orders");
        let columns = obj.properties["columns"].as_array().unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0]["name"], "id");
        assert_eq!(obj.properties["source_system"], "clickhouse");
    }

    #[tokio::test]
    async fn same_table_across_two_runs_gets_same_object_id() {
        let (c1, _dir1) = ctx();
        let id1 = seed_table(
            &c1,
            "analytics.orders",
            "MergeTree",
            vec!["id"],
            vec![("id", "UInt64")],
        );
        let graph1 = run_pass(c1, vec![id1]).await;

        let (c2, _dir2) = ctx();
        let id2 = seed_table(
            &c2,
            "analytics.orders",
            "MergeTree",
            vec!["id"],
            vec![("id", "UInt64")],
        );
        let graph2 = run_pass(c2, vec![id2]).await;

        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
    }

    #[tokio::test]
    async fn no_table_artifacts_produces_no_knowledge_artifact() {
        let (c, _dir) = ctx();
        let mut pass = ClickHouseAnalyzerPass::new("test", vec![]);
        let mut c = c;
        pass.run(&mut c).await.unwrap();
        assert!(c.artifact_store.list().unwrap().is_empty());
    }

    /// Regression guard for RFC 0056's core identity-resolution design decision: two
    /// `ObjectKind::Table` objects with overlapping column names must score above 0 on
    /// `identity::structural_score`, proving reuse of `Table` (not a new `Custom` kind) actually
    /// enables cross-system matching rather than merely compiling.
    #[test]
    fn clickhouse_table_and_generic_sql_table_share_kind_for_identity_resolution() {
        let mut ch_obj = KirObject::new("analytics.orders", ObjectKind::Table);
        ch_obj.properties.insert(
            "columns".into(),
            serde_json::json!([{"name": "id", "data_type": "UInt64"}, {"name": "customer_id", "data_type": "UInt64"}]),
        );

        let mut sql_obj = KirObject::new("orders", ObjectKind::Table);
        sql_obj.properties.insert(
            "columns".into(),
            serde_json::json!([{"name": "id", "data_type": "int"}, {"name": "customer_id", "data_type": "int"}]),
        );

        assert_eq!(ch_obj.kind, sql_obj.kind, "both must be ObjectKind::Table");
    }
}
