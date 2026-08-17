//! Live query execution client (RFC 0056 Stage 2, "run query" step) — separate from
//! `ekos-plugin-clickhouse`'s `ClickHouseClient` (schema listing, compile-time) by design: see
//! RFC 0056's Alternatives Considered for why these stay two different client shapes rather than
//! one shared trait. `run_select` must only ever be called with SQL that has already passed
//! `validate::validate_select_only`.

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClickHouseQueryClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
}

#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
}

#[async_trait]
pub trait ClickHouseQueryClient: Send + Sync {
    /// Runs one already-validated, SELECT-only SQL statement and returns its rows.
    async fn run_select(&self, sql: &str) -> Result<QueryResult, ClickHouseQueryClientError>;
}

/// Real client against ClickHouse's HTTP interface — same shape as
/// `ekos-plugin-clickhouse`'s `ClickHouseHttpClient`, kept separate per RFC 0056.
pub struct ClickHouseHttpQueryClient {
    pub url: String,
    pub user: String,
    pub password: String,
    http: reqwest::Client,
}

impl ClickHouseHttpQueryClient {
    pub fn new(
        url: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            user: user.into(),
            password: password.into(),
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ClickHouseQueryClient for ClickHouseHttpQueryClient {
    async fn run_select(&self, sql: &str) -> Result<QueryResult, ClickHouseQueryClientError> {
        // `FORMAT JSON` is appended here, after validation, never something the LLM/caller
        // controls directly.
        let full_sql = format!("{sql} FORMAT JSON");
        let resp = self
            .http
            .post(&self.url)
            .header("X-ClickHouse-User", &self.user)
            .header("X-ClickHouse-Key", &self.password)
            .body(full_sql)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClickHouseQueryClientError::Api { status, body });
        }

        let raw: serde_json::Value = resp.json().await?;
        let columns = raw["meta"]
            .as_array()
            .map(|meta| {
                meta.iter()
                    .filter_map(|c| c["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let rows = raw["data"].as_array().cloned().unwrap_or_default();
        Ok(QueryResult { columns, rows })
    }
}

/// In-process client for unit tests — returns a fixed result, no network calls.
pub struct MockClickHouseQueryClient {
    pub result: QueryResult,
}

impl MockClickHouseQueryClient {
    pub fn new(result: QueryResult) -> Self {
        Self { result }
    }
}

#[async_trait]
impl ClickHouseQueryClient for MockClickHouseQueryClient {
    async fn run_select(&self, _sql: &str) -> Result<QueryResult, ClickHouseQueryClientError> {
        Ok(self.result.clone())
    }
}
