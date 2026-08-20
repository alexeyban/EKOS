//! GitHub issues/PRs observer plugin (RFC 0020, live-verified RFC 0062).
//!
//! Observes issues and pull requests for one `owner/repo` via the GitHub
//! REST v3 API. `GitHubApiClient` is real `reqwest`-based HTTP code, live-
//! verified 2026-08-20 against `github.com/plausible/analytics` (1,600 real
//! issues/PRs fetched, paginated) — see RFC 0062 and `devlog_63.md`.
//! `MockGitHubClient` remains the only path unit tests take.
//!
//! **Pagination (RFC 0062):** `list_items` fetches at most one page unless
//! `GitHubApiClient::with_pagination` is called — GitHub's default (30 items,
//! newest-first) would otherwise silently truncate any repo with more than
//! one page of history (confirmed live: `plausible/analytics` has on the
//! order of 4,600 real issues/PRs). Pagination follows the response's
//! `Link: <url>; rel="next"` header (standard GitHub API pagination), bounded
//! by `max_pages` — never an unbounded full-history crawl. Live cost is
//! dominated by the per-PR `list_files` call (~0.5s each, sequential, no
//! concurrency) — 1,600 items took ~23 minutes wall-clock; budget
//! accordingly, this is not yet fast enough for routine re-runs.

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
/// Live-verified against `github.com/plausible/analytics` — see RFC 0020, RFC 0062.
pub struct GitHubApiClient {
    pub token: Option<String>,
    http: reqwest::Client,
    per_page: Option<u32>,
    max_pages: u32,
}

impl GitHubApiClient {
    pub fn new(token: Option<String>) -> Self {
        Self {
            token,
            http: reqwest::Client::new(),
            per_page: None,
            max_pages: 1,
        }
    }

    /// Opt-in pagination (RFC 0062) — the exact URL `list_items` sends for
    /// its first request is unchanged unless this is called (`per_page` and
    /// `page` are only appended when set), so no existing caller's behavior
    /// changes by default. `max_pages` bounds how many `Link: rel="next"`
    /// hops `list_items` follows; `0` is treated as `1` (always fetch at
    /// least the first page).
    pub fn with_pagination(mut self, per_page: Option<u32>, max_pages: u32) -> Self {
        self.per_page = per_page;
        self.max_pages = max_pages.max(1);
        self
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

/// Builds the first-page issues-list URL. With `per_page = None` this is
/// byte-identical to the URL this client has always sent — zero behavior
/// change for any caller that never opts into pagination.
fn issues_url(owner: &str, repo: &str, per_page: Option<u32>) -> String {
    let mut url = format!("https://api.github.com/repos/{owner}/{repo}/issues?state=all");
    if let Some(pp) = per_page {
        url.push_str(&format!("&per_page={pp}"));
    }
    url
}

/// Parses a standard HTTP `Link` header
/// (`<url1>; rel="prev", <url2>; rel="next", ...`) and returns the
/// `rel="next"` URL, or `None` if absent — GitHub's own documented REST API
/// pagination convention.
fn parse_next_page_url(link_header: &str) -> Option<String> {
    for part in link_header.split(',') {
        let mut segments = part.split(';');
        let url = segments
            .next()?
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        let is_next = segments.any(|s| s.trim() == "rel=\"next\"");
        if is_next {
            return Some(url.to_string());
        }
    }
    None
}

#[async_trait]
impl GitHubClient for GitHubApiClient {
    async fn list_items(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<GitHubItem>, GitHubClientError> {
        let mut items = Vec::new();
        let mut url = issues_url(owner, repo, self.per_page);
        let mut pages_fetched = 0u32;

        loop {
            let resp = self.request(&url).send().await?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(GitHubClientError::Api { status, body });
            }
            let next_url = resp
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_next_page_url);
            let raw: Vec<serde_json::Value> = resp.json().await?;

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

            pages_fetched += 1;
            match next_url {
                Some(next) if pages_fetched < self.max_pages => url = next,
                _ => break,
            }
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

    #[test]
    fn issues_url_with_no_per_page_matches_the_legacy_url_exactly() {
        assert_eq!(
            issues_url("plausible", "analytics", None),
            "https://api.github.com/repos/plausible/analytics/issues?state=all"
        );
    }

    #[test]
    fn issues_url_appends_per_page_when_set() {
        assert_eq!(
            issues_url("plausible", "analytics", Some(100)),
            "https://api.github.com/repos/plausible/analytics/issues?state=all&per_page=100"
        );
    }

    #[test]
    fn parse_next_page_url_finds_rel_next_among_multiple_links() {
        let header = r#"<https://api.github.com/…?page=2>; rel="next", <https://api.github.com/…?page=9>; rel="last""#;
        assert_eq!(
            parse_next_page_url(header),
            Some("https://api.github.com/…?page=2".to_string())
        );
    }

    #[test]
    fn parse_next_page_url_returns_none_without_a_next_rel() {
        let header = r#"<https://api.github.com/…?page=1>; rel="prev", <https://api.github.com/…?page=9>; rel="last""#;
        assert_eq!(parse_next_page_url(header), None);
    }

    #[test]
    fn parse_next_page_url_returns_none_for_empty_header() {
        assert_eq!(parse_next_page_url(""), None);
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
