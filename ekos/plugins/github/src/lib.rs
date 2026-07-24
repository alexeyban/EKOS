//! GitHub issues/PRs observer plugin (proof-of-concept — RFC 0020).
//!
//! Observes issues and pull requests for one `owner/repo` via the GitHub
//! REST v3 API. `GitHubApiClient` is written to the documented API shape but
//! has never been run against the live API — no token/repo available in this
//! environment, the same honest scoping RFC 0012 used for the Salesforce/SAP
//! sandboxes. `MockGitHubClient` exercises the real mapping logic
//! (`GitHubObserver::scan`) without any network dependency.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// One issue or pull request. `files_changed` is populated for pull requests
/// only — GitHub's issues API has no notion of changed files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubItem {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub is_pull_request: bool,
    #[serde(default)]
    pub files_changed: Vec<String>,
}

#[derive(Debug, Error)]
pub enum GitHubClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
}

/// Interface for retrieving issues/PRs for a repo. Constructor-injected into
/// `GitHubObserver`, mirroring `SalesforceClient` (RFC 0012) — credential
/// assembly and API-version selection are the caller's job, not the
/// observer's.
#[async_trait]
pub trait GitHubClient: Send + Sync {
    /// All issues and pull requests for `owner/repo` (GitHub's issues
    /// endpoint returns both; pull requests carry a `pull_request` key).
    async fn list_items(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<GitHubItem>, GitHubClientError>;
}

/// Real client against the GitHub REST v3 API
/// (`GET /repos/{owner}/{repo}/issues?state=all`, plus
/// `GET /repos/{owner}/{repo}/pulls/{number}/files` per pull request).
///
/// Written to the documented response shape; never exercised against the
/// live API (no token/repo available in this environment) — see RFC 0020.
pub struct GitHubApiClient {
    pub token: Option<String>,
    http: reqwest::Client,
}

impl GitHubApiClient {
    pub fn new(token: Option<String>) -> Self {
        Self {
            token,
            http: reqwest::Client::new(),
        }
    }

    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self
            .http
            .get(url)
            .header("User-Agent", "ekos-plugin-github")
            .header("Accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        req
    }

    async fn list_files(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Vec<String>, GitHubClientError> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}/files");
        let resp = self.request(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubClientError::Api { status, body });
        }
        let raw: Vec<serde_json::Value> = resp.json().await?;
        Ok(raw
            .into_iter()
            .filter_map(|f| f["filename"].as_str().map(str::to_string))
            .collect())
    }
}

#[async_trait]
impl GitHubClient for GitHubApiClient {
    async fn list_items(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<GitHubItem>, GitHubClientError> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/issues?state=all");
        let resp = self.request(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubClientError::Api { status, body });
        }
        let raw: Vec<serde_json::Value> = resp.json().await?;

        let mut items = Vec::with_capacity(raw.len());
        for entry in raw {
            let number = entry["number"].as_u64().unwrap_or_default();
            let is_pull_request = !entry["pull_request"].is_null();
            let files_changed = if is_pull_request {
                self.list_files(owner, repo, number).await?
            } else {
                Vec::new()
            };
            items.push(GitHubItem {
                number,
                title: entry["title"].as_str().unwrap_or_default().to_string(),
                body: entry["body"].as_str().unwrap_or_default().to_string(),
                state: entry["state"].as_str().unwrap_or_default().to_string(),
                is_pull_request,
                files_changed,
            });
        }
        Ok(items)
    }
}

/// In-process client for unit tests — returns fixed items, no network calls.
pub struct MockGitHubClient {
    pub items: Vec<GitHubItem>,
}

impl MockGitHubClient {
    pub fn new(items: Vec<GitHubItem>) -> Self {
        Self { items }
    }
}

#[async_trait]
impl GitHubClient for MockGitHubClient {
    async fn list_items(
        &self,
        _owner: &str,
        _repo: &str,
    ) -> Result<Vec<GitHubItem>, GitHubClientError> {
        Ok(self.items.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per issue/PR.
pub struct GitHubObserver {
    client: Arc<dyn GitHubClient>,
    owner: String,
    repo: String,
}

impl GitHubObserver {
    pub fn new(
        client: Arc<dyn GitHubClient>,
        owner: impl Into<String>,
        repo: impl Into<String>,
    ) -> Self {
        Self {
            client,
            owner: owner.into(),
            repo: repo.into(),
        }
    }
}

#[async_trait]
impl Observer for GitHubObserver {
    fn name(&self) -> &str {
        "github"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let items = self
            .client
            .list_items(&self.owner, &self.repo)
            .await
            .map_err(|e| ObserveError::connector(format!("github list_items failed: {e}")))?;

        let mut pkg = ObservationPackage::new("github", format!("{}/{}", self.owner, self.repo));

        for item in &items {
            let target = format!("{}/{}#{}", self.owner, self.repo, item.number);
            let data = serde_json::json!({
                "owner": self.owner,
                "repo": self.repo,
                "number": item.number,
                "title": item.title,
                "body": item.body,
                "state": item.state,
                "is_pull_request": item.is_pull_request,
                "files_changed": item.files_changed,
            });
            let artifact = ObservationArtifact::new("github", &target, data)
                .with_producer("ekos-plugin-github");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u64, title: &str, body: &str) -> GitHubItem {
        GitHubItem {
            number,
            title: title.into(),
            body: body.into(),
            state: "open".into(),
            is_pull_request: false,
            files_changed: vec![],
        }
    }

    fn pr(number: u64, title: &str, body: &str, files: Vec<&str>) -> GitHubItem {
        GitHubItem {
            number,
            title: title.into(),
            body: body.into(),
            state: "open".into(),
            is_pull_request: true,
            files_changed: files.into_iter().map(String::from).collect(),
        }
    }

    #[tokio::test]
    async fn emits_one_artifact_per_item() {
        let client = Arc::new(MockGitHubClient::new(vec![
            issue(1, "Bug report", "it crashes"),
            pr(2, "Fix crash", "Fixes #1", vec!["src/lib.rs"]),
        ]));
        let observer = GitHubObserver::new(client, "alexeyban", "EKOS");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 2);
    }

    #[tokio::test]
    async fn distinguishes_issues_from_pull_requests() {
        let client = Arc::new(MockGitHubClient::new(vec![
            issue(1, "Bug report", "it crashes"),
            pr(2, "Fix crash", "Fixes #1", vec!["src/lib.rs"]),
        ]));
        let observer = GitHubObserver::new(client, "alexeyban", "EKOS");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();

        let issue_artifact = pkg
            .artifacts
            .iter()
            .find(|a| a.content.data["number"] == 1)
            .unwrap();
        assert_eq!(issue_artifact.content.data["is_pull_request"], false);

        let pr_artifact = pkg
            .artifacts
            .iter()
            .find(|a| a.content.data["number"] == 2)
            .unwrap();
        assert_eq!(pr_artifact.content.data["is_pull_request"], true);
        assert_eq!(
            pr_artifact.content.data["files_changed"],
            serde_json::json!(["src/lib.rs"])
        );
    }

    #[tokio::test]
    async fn empty_repo_produces_no_artifacts() {
        let client = Arc::new(MockGitHubClient::new(vec![]));
        let observer = GitHubObserver::new(client, "alexeyban", "EKOS");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_items_same_artifact_ids() {
        let client1 = Arc::new(MockGitHubClient::new(vec![issue(1, "Bug", "body")]));
        let client2 = Arc::new(MockGitHubClient::new(vec![issue(1, "Bug", "body")]));
        let ctx = ScanContext::new(".");
        let pkg1 = GitHubObserver::new(client1, "a", "b")
            .scan(&ctx)
            .await
            .unwrap();
        let pkg2 = GitHubObserver::new(client2, "a", "b")
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
