//! Microsoft Fabric workspace observer plugin (Phase 14 — scaffold, see RFC 0012).
//!
//! Observes workspace items (lakehouses, warehouses, datasets, etc.) via the
//! Fabric REST API. `FabricApiClient` is written to the documented API shape
//! but has never been run against a live Fabric trial tenant.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// One item inside a Fabric workspace (lakehouse, warehouse, dataset, report, ...).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FabricItem {
    pub id: String,
    pub name: String,
    pub item_type: String,
    pub workspace_id: String,
}

#[derive(Debug, Error)]
pub enum FabricClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Interface for retrieving Fabric workspace items. Constructor-injected into
/// `FabricObserver`.
#[async_trait]
pub trait FabricClient: Send + Sync {
    /// Every item across the configured set of workspaces.
    async fn list_items(&self) -> Result<Vec<FabricItem>, FabricClientError>;
}

/// Real client against the Fabric REST API (`/v1/workspaces/<id>/items`).
///
/// Written to the documented response shape; never exercised against a live
/// tenant — see RFC 0012.
pub struct FabricApiClient {
    pub base_url: String,
    pub access_token: String,
    pub workspace_ids: Vec<String>,
    http: reqwest::Client,
}

impl FabricApiClient {
    pub fn new(access_token: impl Into<String>, workspace_ids: Vec<String>) -> Self {
        Self {
            base_url: "https://api.fabric.microsoft.com/v1".to_string(),
            access_token: access_token.into(),
            workspace_ids,
            http: reqwest::Client::new(),
        }
    }

    async fn items_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<FabricItem>, FabricClientError> {
        let url = format!("{}/workspaces/{workspace_id}/items", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(FabricClientError::Api { status, body });
        }

        let raw: serde_json::Value = resp.json().await?;
        let entries = raw["value"].as_array().cloned().unwrap_or_default();
        Ok(entries
            .into_iter()
            .map(|e| FabricItem {
                id: e["id"].as_str().unwrap_or_default().to_string(),
                name: e["displayName"].as_str().unwrap_or_default().to_string(),
                item_type: e["type"].as_str().unwrap_or_default().to_string(),
                workspace_id: workspace_id.to_string(),
            })
            .collect())
    }
}

#[async_trait]
impl FabricClient for FabricApiClient {
    async fn list_items(&self) -> Result<Vec<FabricItem>, FabricClientError> {
        let mut out = Vec::new();
        for ws in &self.workspace_ids {
            out.extend(self.items_for_workspace(ws).await?);
        }
        Ok(out)
    }
}

/// In-process client for unit tests — returns fixed metadata, no network calls.
pub struct MockFabricClient {
    pub items: Vec<FabricItem>,
}

impl MockFabricClient {
    pub fn new(items: Vec<FabricItem>) -> Self {
        Self { items }
    }
}

#[async_trait]
impl FabricClient for MockFabricClient {
    async fn list_items(&self) -> Result<Vec<FabricItem>, FabricClientError> {
        Ok(self.items.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per workspace item.
pub struct FabricObserver {
    client: Arc<dyn FabricClient>,
}

impl FabricObserver {
    pub fn new(client: Arc<dyn FabricClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Observer for FabricObserver {
    fn name(&self) -> &str {
        "fabric"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let items = self
            .client
            .list_items()
            .await
            .map_err(|e| ObserveError::connector(format!("fabric item list failed: {e}")))?;

        let mut pkg = ObservationPackage::new("fabric", "tenant");

        for item in &items {
            let data = serde_json::json!({
                "item_type": item.item_type,
                "workspace_id": item.workspace_id,
            });
            let target = format!("{}/{}", item.workspace_id, item.name);
            pkg.push(
                ObservationArtifact::new("fabric", &target, data)
                    .with_producer("ekos-plugin-fabric"),
            );
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lakehouse() -> FabricItem {
        FabricItem {
            id: "item-1".into(),
            name: "SalesLakehouse".into(),
            item_type: "Lakehouse".into(),
            workspace_id: "ws-1".into(),
        }
    }

    /// A realistic mixed-item-type workspace, matching the item `type` values Microsoft
    /// publishes in its Fabric REST API `items` list reference docs (near-real, open-source
    /// test data — see RFC 0012 / devlog on "near-real data" fixtures).
    fn sample_workspace_items() -> Vec<FabricItem> {
        let ws = "ws-1";
        vec![
            lakehouse(),
            FabricItem {
                id: "item-2".into(),
                name: "SalesWarehouse".into(),
                item_type: "Warehouse".into(),
                workspace_id: ws.into(),
            },
            FabricItem {
                id: "item-3".into(),
                name: "SalesSemanticModel".into(),
                item_type: "SemanticModel".into(),
                workspace_id: ws.into(),
            },
            FabricItem {
                id: "item-4".into(),
                name: "MonthlySalesReport".into(),
                item_type: "Report".into(),
                workspace_id: ws.into(),
            },
            FabricItem {
                id: "item-5".into(),
                name: "IngestPipeline".into(),
                item_type: "DataPipeline".into(),
                workspace_id: ws.into(),
            },
        ]
    }

    #[tokio::test]
    async fn emits_one_artifact_per_item() {
        let client = Arc::new(MockFabricClient::new(vec![lakehouse()]));
        let observer = FabricObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.artifacts[0].content.data["item_type"], "Lakehouse");
    }

    #[tokio::test]
    async fn mixed_workspace_emits_every_item_type() {
        let client = Arc::new(MockFabricClient::new(sample_workspace_items()));
        let observer = FabricObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 5);
        let types: std::collections::HashSet<_> = pkg
            .artifacts
            .iter()
            .map(|a| a.content.data["item_type"].as_str().unwrap())
            .collect();
        assert!(types.contains("Lakehouse"));
        assert!(types.contains("Warehouse"));
        assert!(types.contains("SemanticModel"));
        assert!(types.contains("Report"));
        assert!(types.contains("DataPipeline"));
    }

    #[tokio::test]
    async fn empty_tenant_produces_no_artifacts() {
        let client = Arc::new(MockFabricClient::new(vec![]));
        let observer = FabricObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_items_same_artifact_ids() {
        let ctx = ScanContext::new(".");
        let pkg1 = FabricObserver::new(Arc::new(MockFabricClient::new(vec![lakehouse()])))
            .scan(&ctx)
            .await
            .unwrap();
        let pkg2 = FabricObserver::new(Arc::new(MockFabricClient::new(vec![lakehouse()])))
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
