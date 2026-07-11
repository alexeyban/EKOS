//! Salesforce sObject schema observer plugin (Phase 14 — scaffold, see RFC 0012).
//!
//! Observes sObject field metadata via the Salesforce REST `describe` endpoint.
//! `SalesforceApiClient` is written to the documented API shape but has never
//! been run against a live org — there is no sandbox credential available in
//! this environment. `MockSalesforceClient` exercises the real mapping logic
//! (`SalesforceObserver::scan`) without any network dependency.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObserveError, ObservationPackage, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// One field on a Salesforce sObject.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SObjectField {
    pub name: String,
    pub field_type: String,
    /// sObject names this field references (non-empty only for lookup/master-detail fields).
    #[serde(default)]
    pub reference_to: Vec<String>,
}

/// Field metadata for one sObject, as returned by `sobjects/<Name>/describe`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SObjectMetadata {
    pub name: String,
    pub fields: Vec<SObjectField>,
}

#[derive(Debug, Error)]
pub enum SalesforceClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Interface for retrieving sObject schema metadata. Constructor-injected into
/// `SalesforceObserver`, mirroring `LlmProvider` in `ekos-recovery` — credential
/// assembly is the caller's job, not the observer's.
#[async_trait]
pub trait SalesforceClient: Send + Sync {
    /// Describe every configured sObject (e.g. `["Account", "Contact"]`).
    async fn list_sobjects(&self) -> Result<Vec<SObjectMetadata>, SalesforceClientError>;
}

/// Real client against the Salesforce REST API (`/services/data/<version>/sobjects/<name>/describe`).
///
/// Written to the documented response shape; never exercised against a live
/// org (no developer-org credential available in this environment) — see
/// RFC 0012.
pub struct SalesforceApiClient {
    pub instance_url: String,
    pub access_token: String,
    pub api_version: String,
    pub sobject_names: Vec<String>,
    http: reqwest::Client,
}

impl SalesforceApiClient {
    pub fn new(
        instance_url: impl Into<String>,
        access_token: impl Into<String>,
        sobject_names: Vec<String>,
    ) -> Self {
        Self {
            instance_url: instance_url.into(),
            access_token: access_token.into(),
            api_version: "v59.0".to_string(),
            sobject_names,
            http: reqwest::Client::new(),
        }
    }

    async fn describe(&self, name: &str) -> Result<SObjectMetadata, SalesforceClientError> {
        let url = format!(
            "{}/services/data/{}/sobjects/{name}/describe",
            self.instance_url.trim_end_matches('/'),
            self.api_version
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SalesforceClientError::Api { status, body });
        }

        let raw: serde_json::Value = resp.json().await?;
        let fields = raw["fields"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|f| SObjectField {
                name: f["name"].as_str().unwrap_or_default().to_string(),
                field_type: f["type"].as_str().unwrap_or_default().to_string(),
                reference_to: f["referenceTo"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            })
            .collect();

        Ok(SObjectMetadata { name: name.to_string(), fields })
    }
}

#[async_trait]
impl SalesforceClient for SalesforceApiClient {
    async fn list_sobjects(&self) -> Result<Vec<SObjectMetadata>, SalesforceClientError> {
        let mut out = Vec::with_capacity(self.sobject_names.len());
        for name in &self.sobject_names {
            out.push(self.describe(name).await?);
        }
        Ok(out)
    }
}

/// In-process client for unit tests — returns fixed metadata, no network calls.
pub struct MockSalesforceClient {
    pub objects: Vec<SObjectMetadata>,
}

impl MockSalesforceClient {
    pub fn new(objects: Vec<SObjectMetadata>) -> Self {
        Self { objects }
    }
}

#[async_trait]
impl SalesforceClient for MockSalesforceClient {
    async fn list_sobjects(&self) -> Result<Vec<SObjectMetadata>, SalesforceClientError> {
        Ok(self.objects.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per sObject.
pub struct SalesforceObserver {
    client: Arc<dyn SalesforceClient>,
}

impl SalesforceObserver {
    pub fn new(client: Arc<dyn SalesforceClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Observer for SalesforceObserver {
    fn name(&self) -> &str {
        "salesforce"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let objects = self
            .client
            .list_sobjects()
            .await
            .map_err(|e| ObserveError::connector(format!("salesforce describe failed: {e}")))?;

        let mut pkg = ObservationPackage::new("salesforce", "org");

        for obj in &objects {
            let reference_fields: Vec<&SObjectField> =
                obj.fields.iter().filter(|f| !f.reference_to.is_empty()).collect();

            let data = serde_json::json!({
                "sobject": obj.name,
                "field_count": obj.fields.len(),
                "fields": obj.fields,
                "reference_fields": reference_fields,
            });

            let artifact = ObservationArtifact::new("salesforce", &obj.name, data)
                .with_producer("ekos-plugin-salesforce");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> SObjectMetadata {
        SObjectMetadata {
            name: "Account".into(),
            fields: vec![SObjectField {
                name: "Name".into(),
                field_type: "string".into(),
                reference_to: vec![],
            }],
        }
    }

    fn contact() -> SObjectMetadata {
        SObjectMetadata {
            name: "Contact".into(),
            fields: vec![
                SObjectField { name: "LastName".into(), field_type: "string".into(), reference_to: vec![] },
                SObjectField {
                    name: "AccountId".into(),
                    field_type: "reference".into(),
                    reference_to: vec!["Account".into()],
                },
            ],
        }
    }

    #[tokio::test]
    async fn emits_one_artifact_per_sobject() {
        let client = Arc::new(MockSalesforceClient::new(vec![account(), contact()]));
        let observer = SalesforceObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 2);
    }

    #[tokio::test]
    async fn captures_reference_field_as_relationship_signal() {
        let client = Arc::new(MockSalesforceClient::new(vec![account(), contact()]));
        let observer = SalesforceObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();

        let contact_artifact =
            pkg.artifacts.iter().find(|a| a.content.target == "Contact").unwrap();
        let refs = contact_artifact.content.data["reference_fields"].as_array().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0]["name"], "AccountId");
        assert_eq!(refs[0]["reference_to"][0], "Account");
    }

    #[tokio::test]
    async fn same_metadata_same_artifact_id() {
        let client1 = Arc::new(MockSalesforceClient::new(vec![account()]));
        let client2 = Arc::new(MockSalesforceClient::new(vec![account()]));
        let ctx = ScanContext::new(".");
        let pkg1 = SalesforceObserver::new(client1).scan(&ctx).await.unwrap();
        let pkg2 = SalesforceObserver::new(client2).scan(&ctx).await.unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }

    #[tokio::test]
    async fn empty_org_produces_no_artifacts() {
        let client = Arc::new(MockSalesforceClient::new(vec![]));
        let observer = SalesforceObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }
}
