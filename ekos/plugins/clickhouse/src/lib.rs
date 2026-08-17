//! ClickHouse schema observer plugin (RFC 0056).
//!
//! Observes table/column metadata via ClickHouse's stock HTTP interface
//! (`POST /` with the SQL query as the raw body, `FORMAT JSON` response) —
//! plain REST/JSON, no native driver, no `bindgen`, unlike the Oracle/SAP
//! scaffolds from RFC 0012. `ClickHouseHttpClient` is a real implementation,
//! written to ClickHouse's documented HTTP interface and `system.tables`/
//! `system.columns` schema but not yet exercised against a live server in
//! this environment — see RFC 0056 for what remains to be verified live.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// One row of `system.tables` for a configured database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableRow {
    pub database: String,
    pub name: String,
    pub engine: String,
    /// Comma-separated column list as ClickHouse renders it (e.g. `"id, created_at"`), empty
    /// string if the engine has no sorting key (e.g. `Log`/`TinyLog`).
    pub sorting_key: String,
    pub partition_key: String,
}

/// One row of `system.columns` for a configured database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnRow {
    pub database: String,
    pub table: String,
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Error)]
pub enum ClickHouseClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Interface for retrieving ClickHouse schema metadata. Constructor-injected
/// into `ClickHouseObserver`.
#[async_trait]
pub trait ClickHouseClient: Send + Sync {
    async fn list_tables(&self) -> Result<Vec<TableRow>, ClickHouseClientError>;
    async fn list_columns(&self) -> Result<Vec<ColumnRow>, ClickHouseClientError>;
}

/// Real client against ClickHouse's HTTP interface. Written to the
/// documented `system.tables`/`system.columns` response shape; never
/// exercised against a live server in this environment — see RFC 0056.
pub struct ClickHouseHttpClient {
    /// Base URL, e.g. `http://localhost:8123`.
    pub url: String,
    pub user: String,
    pub password: String,
    pub database: String,
    http: reqwest::Client,
}

impl ClickHouseHttpClient {
    pub fn new(
        url: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
        database: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            user: user.into(),
            password: password.into(),
            database: database.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Runs one SQL statement (expected to end in `FORMAT JSON`) and returns the parsed
    /// `{"meta": [...], "data": [...], "rows": N}` response body.
    async fn run_query(&self, sql: &str) -> Result<serde_json::Value, ClickHouseClientError> {
        let resp = self
            .http
            .post(&self.url)
            .header("X-ClickHouse-User", &self.user)
            .header("X-ClickHouse-Key", &self.password)
            .body(sql.to_string())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClickHouseClientError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Single-quotes and escapes a value for embedding in a ClickHouse SQL string literal.
    /// Only ever used with our own configured `database` name, never with untrusted/LLM-built
    /// input — Stage 2's live query path validates SQL structurally instead (RFC 0056).
    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "\\'"))
    }
}

#[async_trait]
impl ClickHouseClient for ClickHouseHttpClient {
    async fn list_tables(&self) -> Result<Vec<TableRow>, ClickHouseClientError> {
        let sql = format!(
            "SELECT database, name, engine, sorting_key, partition_key \
             FROM system.tables WHERE database = {} FORMAT JSON",
            Self::quote(&self.database)
        );
        let raw = self.run_query(&sql).await?;
        let rows = raw["data"].as_array().cloned().unwrap_or_default();
        Ok(rows
            .into_iter()
            .filter_map(|row| serde_json::from_value(row).ok())
            .collect())
    }

    async fn list_columns(&self) -> Result<Vec<ColumnRow>, ClickHouseClientError> {
        let sql = format!(
            "SELECT database, table, name, type AS data_type \
             FROM system.columns WHERE database = {} FORMAT JSON",
            Self::quote(&self.database)
        );
        let raw = self.run_query(&sql).await?;
        let rows = raw["data"].as_array().cloned().unwrap_or_default();
        Ok(rows
            .into_iter()
            .filter_map(|row| serde_json::from_value(row).ok())
            .collect())
    }
}

/// In-process client for unit tests — returns fixed metadata, no network calls.
pub struct MockClickHouseClient {
    pub tables: Vec<TableRow>,
    pub columns: Vec<ColumnRow>,
}

impl MockClickHouseClient {
    pub fn new(tables: Vec<TableRow>, columns: Vec<ColumnRow>) -> Self {
        Self { tables, columns }
    }
}

#[async_trait]
impl ClickHouseClient for MockClickHouseClient {
    async fn list_tables(&self) -> Result<Vec<TableRow>, ClickHouseClientError> {
        Ok(self.tables.clone())
    }

    async fn list_columns(&self) -> Result<Vec<ColumnRow>, ClickHouseClientError> {
        Ok(self.columns.clone())
    }
}

/// Splits ClickHouse's comma-separated sorting/partition key string into individual column
/// expressions, trimming whitespace and dropping empties (an engine with no key renders `""`).
fn split_key(key: &str) -> Vec<String> {
    key.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Observer emitting one `ObservationArtifact` per table, its columns joined in client-side
/// (same shape `OracleObserver::scan` uses to join constraints onto their table).
pub struct ClickHouseObserver {
    client: Arc<dyn ClickHouseClient>,
}

impl ClickHouseObserver {
    pub fn new(client: Arc<dyn ClickHouseClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Observer for ClickHouseObserver {
    fn name(&self) -> &str {
        "clickhouse"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let tables =
            self.client.list_tables().await.map_err(|e| {
                ObserveError::connector(format!("clickhouse table list failed: {e}"))
            })?;
        let columns =
            self.client.list_columns().await.map_err(|e| {
                ObserveError::connector(format!("clickhouse column list failed: {e}"))
            })?;

        let mut pkg = ObservationPackage::new("clickhouse", "database");

        for table in &tables {
            let table_columns: Vec<serde_json::Value> = columns
                .iter()
                .filter(|c| c.database == table.database && c.table == table.name)
                .map(|c| serde_json::json!({ "name": c.name, "data_type": c.data_type }))
                .collect();

            let data = serde_json::json!({
                "kind": "table",
                "engine": table.engine,
                "order_by": split_key(&table.sorting_key),
                "partition_by": split_key(&table.partition_key),
                "columns": table_columns,
            });
            let target = format!("{}.{}", table.database, table.name);
            pkg.push(
                ObservationArtifact::new("clickhouse", &target, data)
                    .with_producer("ekos-plugin-clickhouse"),
            );
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders_table() -> TableRow {
        TableRow {
            database: "analytics".into(),
            name: "orders".into(),
            engine: "MergeTree".into(),
            sorting_key: "id, created_at".into(),
            partition_key: "toYYYYMM(created_at)".into(),
        }
    }

    fn orders_columns() -> Vec<ColumnRow> {
        vec![
            ColumnRow {
                database: "analytics".into(),
                table: "orders".into(),
                name: "id".into(),
                data_type: "UInt64".into(),
            },
            ColumnRow {
                database: "analytics".into(),
                table: "orders".into(),
                name: "created_at".into(),
                data_type: "DateTime".into(),
            },
        ]
    }

    #[tokio::test]
    async fn emits_one_artifact_per_table_with_its_columns_joined() {
        let client = Arc::new(MockClickHouseClient::new(
            vec![orders_table()],
            orders_columns(),
        ));
        let observer = ClickHouseObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 1);
        let artifact = &pkg.artifacts[0];
        assert_eq!(artifact.content.target, "analytics.orders");
        let columns = artifact.content.data["columns"].as_array().unwrap();
        assert_eq!(columns.len(), 2);
    }

    #[tokio::test]
    async fn sorting_and_partition_keys_are_split_into_arrays() {
        let client = Arc::new(MockClickHouseClient::new(
            vec![orders_table()],
            orders_columns(),
        ));
        let observer = ClickHouseObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["order_by"], serde_json::json!(["id", "created_at"]));
        assert_eq!(
            data["partition_by"],
            serde_json::json!(["toYYYYMM(created_at)"])
        );
    }

    #[tokio::test]
    async fn table_with_no_sorting_key_gets_empty_order_by() {
        let mut table = orders_table();
        table.sorting_key = String::new();
        let client = Arc::new(MockClickHouseClient::new(vec![table], vec![]));
        let observer = ClickHouseObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(
            pkg.artifacts[0].content.data["order_by"],
            serde_json::json!([])
        );
    }

    #[tokio::test]
    async fn columns_from_a_different_table_are_not_joined_in() {
        let mut other_columns = orders_columns();
        other_columns.push(ColumnRow {
            database: "analytics".into(),
            table: "customers".into(),
            name: "email".into(),
            data_type: "String".into(),
        });
        let client = Arc::new(MockClickHouseClient::new(
            vec![orders_table()],
            other_columns,
        ));
        let observer = ClickHouseObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        let columns = pkg.artifacts[0].content.data["columns"].as_array().unwrap();
        assert_eq!(
            columns.len(),
            2,
            "customers.email must not leak into orders"
        );
    }

    #[tokio::test]
    async fn empty_database_produces_no_artifacts() {
        let client = Arc::new(MockClickHouseClient::new(vec![], vec![]));
        let observer = ClickHouseObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_tables_same_artifact_ids() {
        let ctx = ScanContext::new(".");
        let pkg1 = ClickHouseObserver::new(Arc::new(MockClickHouseClient::new(
            vec![orders_table()],
            orders_columns(),
        )))
        .scan(&ctx)
        .await
        .unwrap();
        let pkg2 = ClickHouseObserver::new(Arc::new(MockClickHouseClient::new(
            vec![orders_table()],
            orders_columns(),
        )))
        .scan(&ctx)
        .await
        .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
