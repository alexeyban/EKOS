//! Oracle schema observer plugin (Phase 14 — scaffold, see RFC 0012).
//!
//! Same surface as a Postgres/SQL Server connector would have: tables,
//! constraints, and views. `OracleDbClient`'s methods are a **documented
//! stub** returning `OracleClientError::NotImplemented` — the `oracle` crate
//! (ODPI-C bindings) needs Oracle Instant Client native libraries that aren't
//! installable in this environment, and adding that dependency risks breaking
//! `cargo build --workspace` for anyone without them. The trait, metadata
//! types, and `Observer` mapping logic are real and unit-tested against
//! `MockOracleClient`; only the live-database wiring is deferred (RFC 0012).

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnMetadata {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableMetadata {
    pub name: String,
    pub columns: Vec<ColumnMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConstraintMetadata {
    pub name: String,
    pub table: String,
    pub constraint_type: String, // "PRIMARY KEY" | "FOREIGN KEY" | "UNIQUE" | "CHECK"
    pub columns: Vec<String>,
    /// (referenced_table, referenced_columns) — populated for FOREIGN KEY constraints.
    pub references: Option<(String, Vec<String>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewMetadata {
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Error)]
pub enum OracleClientError {
    #[error("not implemented: live Oracle driver wiring is deferred (RFC 0012) — {0}")]
    NotImplemented(&'static str),
    #[error("{0}")]
    Other(String),
}

/// Interface for retrieving Oracle schema metadata. Constructor-injected into
/// `OracleObserver`.
#[async_trait]
pub trait OracleClient: Send + Sync {
    async fn list_tables(&self) -> Result<Vec<TableMetadata>, OracleClientError>;
    async fn list_constraints(&self) -> Result<Vec<ConstraintMetadata>, OracleClientError>;
    async fn list_views(&self) -> Result<Vec<ViewMetadata>, OracleClientError>;
}

/// Real client — stubbed. See module docs and RFC 0012 for why no live driver
/// is wired up yet.
pub struct OracleDbClient {
    pub connection_string: String,
}

impl OracleDbClient {
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
        }
    }
}

#[async_trait]
impl OracleClient for OracleDbClient {
    async fn list_tables(&self) -> Result<Vec<TableMetadata>, OracleClientError> {
        Err(OracleClientError::NotImplemented("list_tables"))
    }

    async fn list_constraints(&self) -> Result<Vec<ConstraintMetadata>, OracleClientError> {
        Err(OracleClientError::NotImplemented("list_constraints"))
    }

    async fn list_views(&self) -> Result<Vec<ViewMetadata>, OracleClientError> {
        Err(OracleClientError::NotImplemented("list_views"))
    }
}

/// In-process client for unit tests — returns fixed metadata, no database dependency.
pub struct MockOracleClient {
    pub tables: Vec<TableMetadata>,
    pub constraints: Vec<ConstraintMetadata>,
    pub views: Vec<ViewMetadata>,
}

impl MockOracleClient {
    pub fn new(
        tables: Vec<TableMetadata>,
        constraints: Vec<ConstraintMetadata>,
        views: Vec<ViewMetadata>,
    ) -> Self {
        Self {
            tables,
            constraints,
            views,
        }
    }
}

#[async_trait]
impl OracleClient for MockOracleClient {
    async fn list_tables(&self) -> Result<Vec<TableMetadata>, OracleClientError> {
        Ok(self.tables.clone())
    }

    async fn list_constraints(&self) -> Result<Vec<ConstraintMetadata>, OracleClientError> {
        Ok(self.constraints.clone())
    }

    async fn list_views(&self) -> Result<Vec<ViewMetadata>, OracleClientError> {
        Ok(self.views.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per table (with its constraints
/// attached) plus one per view.
pub struct OracleObserver {
    client: Arc<dyn OracleClient>,
}

impl OracleObserver {
    pub fn new(client: Arc<dyn OracleClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Observer for OracleObserver {
    fn name(&self) -> &str {
        "oracle"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let tables = self
            .client
            .list_tables()
            .await
            .map_err(|e| ObserveError::connector(format!("oracle table list failed: {e}")))?;
        let constraints =
            self.client.list_constraints().await.map_err(|e| {
                ObserveError::connector(format!("oracle constraint list failed: {e}"))
            })?;
        let views = self
            .client
            .list_views()
            .await
            .map_err(|e| ObserveError::connector(format!("oracle view list failed: {e}")))?;

        let mut pkg = ObservationPackage::new("oracle", "database");

        for table in &tables {
            let table_constraints: Vec<&ConstraintMetadata> = constraints
                .iter()
                .filter(|c| c.table == table.name)
                .collect();
            let data = serde_json::json!({
                "kind": "table",
                "columns": table.columns,
                "constraints": table_constraints,
            });
            pkg.push(
                ObservationArtifact::new("oracle", &table.name, data)
                    .with_producer("ekos-plugin-oracle"),
            );
        }

        for view in &views {
            let data = serde_json::json!({
                "kind": "view",
                "definition": view.definition,
            });
            pkg.push(
                ObservationArtifact::new("oracle", &view.name, data)
                    .with_producer("ekos-plugin-oracle"),
            );
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders_table() -> TableMetadata {
        TableMetadata {
            name: "ORDERS".into(),
            columns: vec![
                ColumnMetadata {
                    name: "ID".into(),
                    data_type: "NUMBER".into(),
                    nullable: false,
                },
                ColumnMetadata {
                    name: "CUSTOMER_ID".into(),
                    data_type: "NUMBER".into(),
                    nullable: false,
                },
            ],
        }
    }

    fn fk_constraint() -> ConstraintMetadata {
        ConstraintMetadata {
            name: "FK_ORDERS_CUSTOMER".into(),
            table: "ORDERS".into(),
            constraint_type: "FOREIGN KEY".into(),
            columns: vec!["CUSTOMER_ID".into()],
            references: Some(("CUSTOMERS".into(), vec!["ID".into()])),
        }
    }

    #[tokio::test]
    async fn emits_artifact_per_table_and_view() {
        let client = Arc::new(MockOracleClient::new(
            vec![orders_table()],
            vec![fk_constraint()],
            vec![ViewMetadata {
                name: "ORDER_SUMMARY".into(),
                definition: "SELECT ...".into(),
            }],
        ));
        let observer = OracleObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 2);
    }

    #[tokio::test]
    async fn table_artifact_carries_its_constraints() {
        let client = Arc::new(MockOracleClient::new(
            vec![orders_table()],
            vec![fk_constraint()],
            vec![],
        ));
        let observer = OracleObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        let artifact = pkg
            .artifacts
            .iter()
            .find(|a| a.content.target == "ORDERS")
            .unwrap();
        let constraints = artifact.content.data["constraints"].as_array().unwrap();
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0]["constraint_type"], "FOREIGN KEY");
    }

    #[tokio::test]
    async fn stub_real_client_returns_not_implemented() {
        let client = OracleDbClient::new("oracle://scratch");
        let err = client.list_tables().await.unwrap_err();
        assert!(matches!(err, OracleClientError::NotImplemented(_)));
    }

    #[tokio::test]
    async fn empty_database_produces_no_artifacts() {
        let client = Arc::new(MockOracleClient::new(vec![], vec![], vec![]));
        let observer = OracleObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }
}
