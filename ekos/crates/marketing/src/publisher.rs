//! `Publisher` trait + implementations (RFC 0027).

use async_trait::async_trait;
use serde::Deserialize;
use thiserror::Error;

use crate::oauth1::{OauthCredentials, authorization_header};

const TWEETS_ENDPOINT: &str = "https://api.twitter.com/2/tweets";

#[derive(Debug, Error)]
pub enum PublishError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("twitter api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("unexpected response shape: {0}")]
    UnexpectedResponse(String),
}

/// Publish a piece of text to a platform, returning a platform-specific post id.
#[async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, text: &str) -> Result<String, PublishError>;
}

/// Real X (Twitter) publisher — `POST /2/tweets`, OAuth 1.0a user-context signed.
#[derive(Debug)]
pub struct TwitterPublisher {
    credentials: OauthCredentials,
    client: reqwest::Client,
}

impl TwitterPublisher {
    pub fn new(credentials: OauthCredentials) -> Self {
        Self {
            credentials,
            client: reqwest::Client::new(),
        }
    }

    /// Reads `TWITTER_API_KEY` / `TWITTER_API_SECRET` / `TWITTER_ACCESS_TOKEN` /
    /// `TWITTER_ACCESS_SECRET` from the environment — never committed, never logged.
    pub fn from_env() -> Result<Self, String> {
        let get = |var: &str| std::env::var(var).map_err(|_| format!("{var} not set"));
        let credentials = OauthCredentials {
            api_key: get("TWITTER_API_KEY")?,
            api_secret: get("TWITTER_API_SECRET")?,
            access_token: get("TWITTER_ACCESS_TOKEN")?,
            access_token_secret: get("TWITTER_ACCESS_SECRET")?,
        };
        Ok(Self::new(credentials))
    }
}

#[derive(Deserialize)]
struct TweetCreateResponse {
    data: TweetCreateData,
}

#[derive(Deserialize)]
struct TweetCreateData {
    id: String,
}

#[async_trait]
impl Publisher for TwitterPublisher {
    async fn publish(&self, text: &str) -> Result<String, PublishError> {
        let auth = authorization_header(&self.credentials, "POST", TWEETS_ENDPOINT);

        let resp = self
            .client
            .post(TWEETS_ENDPOINT)
            .header("Authorization", auth)
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(PublishError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: TweetCreateResponse = serde_json::from_str(&body)
            .map_err(|e| PublishError::UnexpectedResponse(format!("{e}: {body}")))?;
        Ok(parsed.data.id)
    }
}

/// Used for `--dry-run` and `[marketing.twitter] enabled = false` — prints what would be
/// posted, touches no network, returns a recognizably-fake id so callers never mistake it
/// for a real post.
pub struct NoopPublisher;

#[async_trait]
impl Publisher for NoopPublisher {
    async fn publish(&self, text: &str) -> Result<String, PublishError> {
        println!("[dry-run] would publish: {text}");
        Ok(format!("dry-run-{}", uuid::Uuid::new_v4()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_publisher_returns_a_recognizable_fake_id() {
        let publisher = NoopPublisher;
        let id = publisher.publish("hello").await.unwrap();
        assert!(id.starts_with("dry-run-"));
    }

    #[test]
    fn from_env_reports_which_var_is_missing() {
        // Deliberately not setting any TWITTER_* env vars here — asserts the error names the
        // var rather than a generic failure, since dry-run/disabled misconfiguration should be
        // easy for an operator to diagnose from the message alone.
        for var in [
            "TWITTER_API_KEY",
            "TWITTER_API_SECRET",
            "TWITTER_ACCESS_TOKEN",
            "TWITTER_ACCESS_SECRET",
        ] {
            unsafe {
                std::env::remove_var(var);
            }
        }
        let err = TwitterPublisher::from_env().unwrap_err();
        assert!(err.contains("TWITTER_API_KEY"));
    }
}
