//! SAP OData observer plugin (Phase 14 — scaffold, see RFC 0012).
//!
//! Observes business objects (exposed as OData entity sets — the REST-friendly
//! surface over BAPIs) and organizational units. Deliberately OData-only, not
//! RFC/`nwrfc` — the SAP NetWeaver RFC SDK is a proprietary native dependency
//! not installable in this environment; see RFC 0012 for the full rationale.
//! `SapODataClient` is written to the documented OData shape but has never
//! been run against a live SAP sandbox.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObserveError, ObservationPackage, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// A business object exposed as an OData entity set (the REST-friendly surface over a BAPI).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BusinessObject {
    pub name: String,
    pub entity_set: String,
    pub description: Option<String>,
}

/// One node in the SAP organizational hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrganizationalUnit {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum SapClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Interface for retrieving SAP business objects and organizational hierarchy.
/// Constructor-injected into `SapObserver`.
#[async_trait]
pub trait SapClient: Send + Sync {
    async fn list_business_objects(&self) -> Result<Vec<BusinessObject>, SapClientError>;
    async fn list_organizational_units(&self) -> Result<Vec<OrganizationalUnit>, SapClientError>;
}

/// Real client against a SAP Gateway OData service.
///
/// Written to the documented OData `$metadata`/entity-set response shape;
/// never exercised against a live sandbox — see RFC 0012.
pub struct SapODataClient {
    pub service_url: String,
    pub username: String,
    pub password: String,
    http: reqwest::Client,
}

impl SapODataClient {
    pub fn new(
        service_url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            service_url: service_url.into(),
            username: username.into(),
            password: password.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, SapClientError> {
        let url = format!("{}/{}", self.service_url.trim_end_matches('/'), path);
        let resp = self
            .http
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SapClientError::Api { status, body });
        }

        Ok(resp.json().await?)
    }
}

#[async_trait]
impl SapClient for SapODataClient {
    async fn list_business_objects(&self) -> Result<Vec<BusinessObject>, SapClientError> {
        let raw = self.get_json("BusinessObjectSet").await?;
        let entries = raw["d"]["results"].as_array().cloned().unwrap_or_default();
        Ok(entries
            .into_iter()
            .map(|e| BusinessObject {
                name: e["Name"].as_str().unwrap_or_default().to_string(),
                entity_set: e["EntitySet"].as_str().unwrap_or_default().to_string(),
                description: e["Description"].as_str().map(str::to_string),
            })
            .collect())
    }

    async fn list_organizational_units(&self) -> Result<Vec<OrganizationalUnit>, SapClientError> {
        let raw = self.get_json("OrgUnitSet").await?;
        let entries = raw["d"]["results"].as_array().cloned().unwrap_or_default();
        Ok(entries
            .into_iter()
            .map(|e| OrganizationalUnit {
                id: e["OrgUnitId"].as_str().unwrap_or_default().to_string(),
                name: e["Name"].as_str().unwrap_or_default().to_string(),
                parent_id: e["ParentId"].as_str().map(str::to_string),
            })
            .collect())
    }
}

/// In-process client for unit tests — returns fixed metadata, no network calls.
pub struct MockSapClient {
    pub business_objects: Vec<BusinessObject>,
    pub org_units: Vec<OrganizationalUnit>,
}

impl MockSapClient {
    pub fn new(business_objects: Vec<BusinessObject>, org_units: Vec<OrganizationalUnit>) -> Self {
        Self { business_objects, org_units }
    }
}

#[async_trait]
impl SapClient for MockSapClient {
    async fn list_business_objects(&self) -> Result<Vec<BusinessObject>, SapClientError> {
        Ok(self.business_objects.clone())
    }

    async fn list_organizational_units(&self) -> Result<Vec<OrganizationalUnit>, SapClientError> {
        Ok(self.org_units.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per business object plus one per
/// organizational unit.
pub struct SapObserver {
    client: Arc<dyn SapClient>,
}

impl SapObserver {
    pub fn new(client: Arc<dyn SapClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Observer for SapObserver {
    fn name(&self) -> &str {
        "sap"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let business_objects = self
            .client
            .list_business_objects()
            .await
            .map_err(|e| ObserveError::connector(format!("sap business object list failed: {e}")))?;
        let org_units = self
            .client
            .list_organizational_units()
            .await
            .map_err(|e| ObserveError::connector(format!("sap org unit list failed: {e}")))?;

        let mut pkg = ObservationPackage::new("sap", "landscape");

        for bo in &business_objects {
            let data = serde_json::json!({
                "kind": "business_object",
                "entity_set": bo.entity_set,
                "description": bo.description,
            });
            pkg.push(
                ObservationArtifact::new("sap", &bo.name, data).with_producer("ekos-plugin-sap"),
            );
        }

        for unit in &org_units {
            let data = serde_json::json!({
                "kind": "org_unit",
                "name": unit.name,
                "parent_id": unit.parent_id,
            });
            pkg.push(
                ObservationArtifact::new("sap", &unit.id, data).with_producer("ekos-plugin-sap"),
            );
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bo() -> BusinessObject {
        BusinessObject {
            name: "SalesOrder".into(),
            entity_set: "SalesOrderSet".into(),
            description: Some("Sales order header and items".into()),
        }
    }

    fn sample_org_unit() -> OrganizationalUnit {
        OrganizationalUnit { id: "1000".into(), name: "Sales EMEA".into(), parent_id: None }
    }

    #[tokio::test]
    async fn emits_artifact_per_business_object_and_org_unit() {
        let client = Arc::new(MockSapClient::new(vec![sample_bo()], vec![sample_org_unit()]));
        let observer = SapObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 2);
    }

    #[tokio::test]
    async fn business_object_artifact_has_expected_shape() {
        let client = Arc::new(MockSapClient::new(vec![sample_bo()], vec![]));
        let observer = SapObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        let artifact = &pkg.artifacts[0];
        assert_eq!(artifact.content.target, "SalesOrder");
        assert_eq!(artifact.content.data["kind"], "business_object");
        assert_eq!(artifact.content.data["entity_set"], "SalesOrderSet");
    }

    #[tokio::test]
    async fn empty_landscape_produces_no_artifacts() {
        let client = Arc::new(MockSapClient::new(vec![], vec![]));
        let observer = SapObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_landscape_same_artifact_ids() {
        let ctx = ScanContext::new(".");
        let pkg1 = SapObserver::new(Arc::new(MockSapClient::new(vec![sample_bo()], vec![])))
            .scan(&ctx)
            .await
            .unwrap();
        let pkg2 = SapObserver::new(Arc::new(MockSapClient::new(vec![sample_bo()], vec![])))
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
