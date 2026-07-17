//! Salesforce sObject schema observer plugin (Phase 14 — scaffold, see RFC 0012).
//!
//! Observes sObject field metadata via the Salesforce REST `describe` endpoint.
//! `SalesforceApiClient` is written to the documented API shape but has never
//! been run against a live org — there is no sandbox credential available in
//! this environment. `MockSalesforceClient` exercises the real mapping logic
//! (`SalesforceObserver::scan`) without any network dependency.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
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

        Ok(SObjectMetadata {
            name: name.to_string(),
            fields,
        })
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
            let reference_fields: Vec<&SObjectField> = obj
                .fields
                .iter()
                .filter(|f| !f.reference_to.is_empty())
                .collect();

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

    /// Real Salesforce standard-object field shape for `Account`, per Salesforce's public
    /// REST API `describe()` reference (near-real, open-source test data — see RFC 0012 /
    /// devlog on "near-real data" fixtures). A representative subset of the ~70 real
    /// standard fields Salesforce publishes for this object, not an invented toy shape.
    fn account() -> SObjectMetadata {
        let f = |name: &str, field_type: &str| SObjectField {
            name: name.into(),
            field_type: field_type.into(),
            reference_to: vec![],
        };
        SObjectMetadata {
            name: "Account".into(),
            fields: vec![
                f("Id", "id"),
                f("Name", "string"),
                f("Type", "picklist"),
                f("Industry", "picklist"),
                f("AnnualRevenue", "currency"),
                f("NumberOfEmployees", "int"),
                f("BillingCity", "string"),
                f("BillingState", "string"),
                f("BillingCountry", "string"),
                f("Phone", "phone"),
                f("Website", "url"),
                f("Description", "textarea"),
                SObjectField {
                    name: "OwnerId".into(),
                    field_type: "reference".into(),
                    reference_to: vec!["User".into()],
                },
            ],
        }
    }

    /// Real Salesforce standard-object field shape for `Contact`, including its two real
    /// reference fields: `AccountId` (the parent account) and the self-referential
    /// `ReportsToId` (org-chart hierarchy) — both genuine Salesforce standard fields.
    fn contact() -> SObjectMetadata {
        let f = |name: &str, field_type: &str| SObjectField {
            name: name.into(),
            field_type: field_type.into(),
            reference_to: vec![],
        };
        SObjectMetadata {
            name: "Contact".into(),
            fields: vec![
                f("Id", "id"),
                f("FirstName", "string"),
                f("LastName", "string"),
                f("Email", "email"),
                f("Phone", "phone"),
                f("Title", "string"),
                f("Department", "string"),
                SObjectField {
                    name: "AccountId".into(),
                    field_type: "reference".into(),
                    reference_to: vec!["Account".into()],
                },
                SObjectField {
                    name: "ReportsToId".into(),
                    field_type: "reference".into(),
                    reference_to: vec!["Contact".into()],
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

        let contact_artifact = pkg
            .artifacts
            .iter()
            .find(|a| a.content.target == "Contact")
            .unwrap();
        let refs = contact_artifact.content.data["reference_fields"]
            .as_array()
            .unwrap();
        assert_eq!(
            refs.len(),
            2,
            "AccountId and ReportsToId are both real reference fields"
        );
        let account_ref = refs.iter().find(|r| r["name"] == "AccountId").unwrap();
        assert_eq!(account_ref["reference_to"][0], "Account");
    }

    #[tokio::test]
    async fn captures_self_referential_reports_to_relationship() {
        let client = Arc::new(MockSalesforceClient::new(vec![contact()]));
        let observer = SalesforceObserver::new(client);
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();

        let contact_artifact = &pkg.artifacts[0];
        let refs = contact_artifact.content.data["reference_fields"]
            .as_array()
            .unwrap();
        let reports_to = refs.iter().find(|r| r["name"] == "ReportsToId").unwrap();
        assert_eq!(
            reports_to["reference_to"][0], "Contact",
            "ReportsToId is a genuine self-referential Salesforce standard field (org-chart hierarchy)"
        );
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
