//! Marketing Agent v1 (RFC 0027) — reads a `devlog_N.md`, classifies its importance,
//! drafts a validated X (Twitter) post via an `LlmProvider`, and publishes it after human
//! approval, with duplicate-post protection. See `docs/rfcs/0027-marketing-agent.md`.

pub mod devlog;
pub mod importance;
pub mod oauth1;
pub mod prompt;
pub mod publisher;
pub mod store;
pub mod tweet;

pub use devlog::{DevlogParseError, DevlogSummary};
pub use importance::Importance;
pub use oauth1::OauthCredentials;
pub use publisher::{NoopPublisher, PublishError, Publisher, TwitterPublisher};
pub use store::{PostedStore, PostedTweet, StoreError};
pub use tweet::{MarketingError, TweetDraft, TweetValidationError, generate_tweet, validate_tweet};
