//! Snowflake schema observer plugin (Phase 14 — scaffold, see RFC 0012).
//!
//! Observes warehouse/schema/table/view metadata via Snowflake's SQL REST API
//! (`/api/v2/statements`, running `SHOW TABLES`/`SHOW VIEWS`-equivalent
//! queries). `SnowflakeApiClient` is written to the documented API shape but
//! has never been run against a live trial account.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// One table or view in a Snowflake schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaObject {
    pub database: String,
    pub schema: String,
    pub name: String,
    /// "TABLE" | "VIEW"
    pub object_type: String,
}

#[derive(Debug, Error)]
pub enum SnowflakeClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Interface for retrieving Snowflake schema metadata. Constructor-injected
/// into `SnowflakeObserver`.
#[async_trait]
pub trait SnowflakeClient: Send + Sync {
    async fn list_schema_objects(&self) -> Result<Vec<SchemaObject>, SnowflakeClientError>;
}

/// Real client against Snowflake's SQL REST API.
///
/// Written to the documented response shape; never exercised against a live
/// account — see RFC 0012.
pub struct SnowflakeApiClient {
    pub account_url: String,
    pub access_token: String,
    pub warehouse: String,
    http: reqwest::Client,
}

impl SnowflakeApiClient {
    pub fn new(
        account_url: impl Into<String>,
        access_token: impl Into<String>,
        warehouse: impl Into<String>,
    ) -> Self {
        Self {
            account_url: account_url.into(),
            access_token: access_token.into(),
            warehouse: warehouse.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn run_statement(&self, sql: &str) -> Result<serde_json::Value, SnowflakeClientError> {
        let url = format!(
            "{}/api/v2/statements",
            self.account_url.trim_end_matches('/')
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&serde_json::json!({ "statement": sql, "warehouse": self.warehouse }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SnowflakeClientError::Api { status, body });
        }

        Ok(resp.json().await?)
    }
}

#[async_trait]
impl SnowflakeClient for SnowflakeApiClient {
    async fn list_schema_objects(&self) -> Result<Vec<SchemaObject>, SnowflakeClientError> {
        let raw = self
            .run_statement(
                "SELECT table_catalog, table_schema, table_name, table_type \
                 FROM information_schema.tables",
            )
            .await?;
        let rows = raw["data"].as_array().cloned().unwrap_or_default();
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let row = row.as_array()?;
                Some(SchemaObject {
                    database: row.first()?.as_str()?.to_string(),
                    schema: row.get(1)?.as_str()?.to_string(),
                    name: row.get(2)?.as_str()?.to_string(),
                    object_type: row.get(3)?.as_str()?.to_string(),
                })
            })
            .collect())
    }
}

/// In-process client for unit tests — returns fixed metadata, no network calls.
pub struct MockSnowflakeClient {
    pub objects: Vec<SchemaObject>,
}

impl MockSnowflakeClient {
    pub fn new(objects: Vec<SchemaObject>) -> Self {
        Self { objects }
    }
}

#[async_trait]
impl SnowflakeClient for MockSnowflakeClient {
    async fn list_schema_objects(&self) -> Result<Vec<SchemaObject>, SnowflakeClientError> {
        Ok(self.objects.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per table/view.
pub struct SnowflakeObserver {
    client: Arc<dyn SnowflakeClient>,
}

impl SnowflakeObserver {
    pub fn new(client: Arc<dyn SnowflakeClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Observer for SnowflakeObserver {
    fn name(&self) -> &str {
        "snowflake"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let objects =
            self.client.list_schema_objects().await.map_err(|e| {
                ObserveError::connector(format!("snowflake schema list failed: {e}"))
            })?;

        let mut pkg = ObservationPackage::new("snowflake", "account");

        for obj in &objects {
            let data = serde_json::json!({
                "database": obj.database,
                "schema": obj.schema,
                "object_type": obj.object_type,
            });
            let target = format!("{}.{}.{}", obj.database, obj.schema, obj.name);
            pkg.push(
                ObservationArtifact::new("snowflake", &target, data)
                    .with_producer("ekos-plugin-snowflake"),
            );
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders_table() -> SchemaObject {
        SchemaObject {
            database: "ANALYTICS".into(),
            schema: "PUBLIC".into(),
            name: "ORDERS".into(),
            object_type: "TABLE".into(),
        }
    }

    /// A realistic multi-schema account: `information_schema.tables`-shaped rows spanning
    /// two schemas and both real Snowflake object types (`TABLE`, `VIEW`) — near-real,
    /// open-source test data (see RFC 0012 / devlog on "near-real data" fixtures).
    fn sample_account_objects() -> Vec<SchemaObject> {
        vec![
            SchemaObject {
                database: "ANALYTICS".into(),
                schema: "RAW".into(),
                name: "CUSTOMERS".into(),
                object_type: "TABLE".into(),
            },
            SchemaObject {
                database: "ANALYTICS".into(),
                schema: "RAW".into(),
                name: "PRODUCTS".into(),
                object_type: "TABLE".into(),
            },
            orders_table(),
            SchemaObject {
                database: "ANALYTICS".into(),
                schema: "PUBLIC".into(),
                name: "ORDER_SUMMARY".into(),
                object_type: "VIEW".into(),
            },
        ]
    }

    #[tokio::test]
    async fn emits_one_artifact_per_schema_object() {
        let client = Arc::new(MockSnowflakeClient::new(vec![orders_table()]));
        let observer = SnowflakeObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.artifacts[0].content.target, "ANALYTICS.PUBLIC.ORDERS");
    }

    #[tokio::test]
    async fn multi_schema_account_distinguishes_tables_and_views() {
        let client = Arc::new(MockSnowflakeClient::new(sample_account_objects()));
        let observer = SnowflakeObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 4);

        let view = pkg
            .artifacts
            .iter()
            .find(|a| a.content.target == "ANALYTICS.PUBLIC.ORDER_SUMMARY")
            .unwrap();
        assert_eq!(view.content.data["object_type"], "VIEW");

        let raw_schema_tables: Vec<_> = pkg
            .artifacts
            .iter()
            .filter(|a| a.content.data["schema"] == "RAW")
            .collect();
        assert_eq!(
            raw_schema_tables.len(),
            2,
            "CUSTOMERS and PRODUCTS both live in the RAW schema"
        );
    }

    #[tokio::test]
    async fn empty_account_produces_no_artifacts() {
        let client = Arc::new(MockSnowflakeClient::new(vec![]));
        let observer = SnowflakeObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_objects_same_artifact_ids() {
        let ctx = ScanContext::new(".");
        let pkg1 = SnowflakeObserver::new(Arc::new(MockSnowflakeClient::new(vec![orders_table()])))
            .scan(&ctx)
            .await
            .unwrap();
        let pkg2 = SnowflakeObserver::new(Arc::new(MockSnowflakeClient::new(vec![orders_table()])))
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
