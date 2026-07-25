//! Confluence page observer plugin (proof-of-concept — RFC 0022).
//!
//! Observes pages in one Confluence space via the documented REST API.
//! `ConfluenceApiClient` is written to the documented API shape but has
//! never been run against a live site — no credential available in this
//! environment, the same honest scoping RFC 0012 used for the
//! Salesforce/SAP sandboxes and RFC 0020 used for GitHub.
//! `MockConfluenceClient` exercises the real mapping logic
//! (`ConfluenceObserver::scan`) without any network dependency.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// One Confluence page. `body` is the storage-format markup (Confluence's
/// own XHTML-like representation) — read directly, never rendered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfluencePage {
    pub id: String,
    pub space_key: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfluenceClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
}

/// Interface for retrieving pages in a space. Constructor-injected into
/// `ConfluenceObserver`, mirroring `SalesforceClient`/`GitHubClient` — base
/// URL and credential assembly are the caller's job, not the observer's.
#[async_trait]
pub trait ConfluenceClient: Send + Sync {
    /// Every page in `space_key`, with body and parent-page id populated.
    async fn list_pages(
        &self,
        space_key: &str,
    ) -> Result<Vec<ConfluencePage>, ConfluenceClientError>;
}

/// Real client against the Confluence Cloud REST API
/// (`GET /wiki/rest/api/content?spaceKey={key}&expand=body.storage,ancestors`).
///
/// Written to the documented response shape; never exercised against a
/// live site (no credential available in this environment) — see RFC 0022.
pub struct ConfluenceApiClient {
    pub base_url: String,
    pub token: Option<String>,
    http: reqwest::Client,
}

impl ConfluenceApiClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token,
            http: reqwest::Client::new(),
        }
    }

    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.get(url).header("Accept", "application/json");
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        req
    }
}

#[async_trait]
impl ConfluenceClient for ConfluenceApiClient {
    async fn list_pages(
        &self,
        space_key: &str,
    ) -> Result<Vec<ConfluencePage>, ConfluenceClientError> {
        let url = format!(
            "{}/wiki/rest/api/content?spaceKey={space_key}&expand=body.storage,ancestors&limit=200",
            self.base_url.trim_end_matches('/')
        );
        let resp = self.request(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ConfluenceClientError::Api { status, body });
        }
        let raw: serde_json::Value = resp.json().await?;
        let results = raw["results"].as_array().cloned().unwrap_or_default();

        Ok(results
            .into_iter()
            .map(|entry| {
                let ancestors = entry["ancestors"].as_array().cloned().unwrap_or_default();
                let parent_id = ancestors
                    .last()
                    .and_then(|a| a["id"].as_str())
                    .map(str::to_string);
                ConfluencePage {
                    id: entry["id"].as_str().unwrap_or_default().to_string(),
                    space_key: space_key.to_string(),
                    title: entry["title"].as_str().unwrap_or_default().to_string(),
                    body: entry["body"]["storage"]["value"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    parent_id,
                }
            })
            .collect())
    }
}

/// In-process client for unit tests — returns fixed pages, no network calls.
pub struct MockConfluenceClient {
    pub pages: Vec<ConfluencePage>,
}

impl MockConfluenceClient {
    pub fn new(pages: Vec<ConfluencePage>) -> Self {
        Self { pages }
    }
}

#[async_trait]
impl ConfluenceClient for MockConfluenceClient {
    async fn list_pages(
        &self,
        _space_key: &str,
    ) -> Result<Vec<ConfluencePage>, ConfluenceClientError> {
        Ok(self.pages.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per page.
pub struct ConfluenceObserver {
    client: Arc<dyn ConfluenceClient>,
    space_key: String,
}

impl ConfluenceObserver {
    pub fn new(client: Arc<dyn ConfluenceClient>, space_key: impl Into<String>) -> Self {
        Self {
            client,
            space_key: space_key.into(),
        }
    }
}

#[async_trait]
impl Observer for ConfluenceObserver {
    fn name(&self) -> &str {
        "confluence"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let pages =
            self.client.list_pages(&self.space_key).await.map_err(|e| {
                ObserveError::connector(format!("confluence list_pages failed: {e}"))
            })?;

        let mut pkg = ObservationPackage::new("confluence", self.space_key.clone());

        for page in &pages {
            let target = format!("{}:{}", page.space_key, page.id);
            let data = serde_json::json!({
                "id": page.id,
                "space_key": page.space_key,
                "title": page.title,
                "body": page.body,
                "parent_id": page.parent_id,
            });
            let artifact = ObservationArtifact::new("confluence", &target, data)
                .with_producer("ekos-plugin-confluence");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str, title: &str, body: &str, parent_id: Option<&str>) -> ConfluencePage {
        ConfluencePage {
            id: id.into(),
            space_key: "ENG".into(),
            title: title.into(),
            body: body.into(),
            parent_id: parent_id.map(String::from),
        }
    }

    #[tokio::test]
    async fn emits_one_artifact_per_page() {
        let client = Arc::new(MockConfluenceClient::new(vec![
            page("1", "Overview", "top-level page", None),
            page("2", "Details", "child page", Some("1")),
        ]));
        let observer = ConfluenceObserver::new(client, "ENG");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 2);
    }

    #[tokio::test]
    async fn parent_id_rides_on_the_artifact() {
        let client = Arc::new(MockConfluenceClient::new(vec![page(
            "2",
            "Details",
            "child page",
            Some("1"),
        )]));
        let observer = ConfluenceObserver::new(client, "ENG");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.artifacts[0].content.data["parent_id"], "1");
    }

    #[tokio::test]
    async fn empty_space_produces_no_artifacts() {
        let client = Arc::new(MockConfluenceClient::new(vec![]));
        let observer = ConfluenceObserver::new(client, "ENG");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_pages_same_artifact_ids() {
        let client1 = Arc::new(MockConfluenceClient::new(vec![page(
            "1", "Overview", "body", None,
        )]));
        let client2 = Arc::new(MockConfluenceClient::new(vec![page(
            "1", "Overview", "body", None,
        )]));
        let ctx = ScanContext::new(".");
        let pkg1 = ConfluenceObserver::new(client1, "ENG")
            .scan(&ctx)
            .await
            .unwrap();
        let pkg2 = ConfluenceObserver::new(client2, "ENG")
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
