//! Tweet drafting + validation (RFC 0030). One `LlmProvider::complete` call, then hard
//! validation server-side rather than trusting the model followed every prompt rule.

use ekos_compiler_core::config::MarketingConfig;
use ekos_recovery::{LlmProvider, LlmRequest, strip_json_fences};
use thiserror::Error;

use crate::devlog::DevlogSummary;
use crate::prompt::{PROMPT_VERSION, SYSTEM_PROMPT, build_retry_suffix, build_user_prompt};

const MAX_TWEET_LEN: usize = 280;
const MAX_HASHTAGS: usize = 3;
const MAX_TOKENS: u32 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TweetDraft {
    pub text: String,
}

#[derive(Debug, Error)]
pub enum MarketingError {
    #[error("llm error: {0}")]
    Llm(#[from] ekos_recovery::LlmError),
    #[error("llm response was not valid JSON: {0} (raw: {1})")]
    InvalidJson(serde_json::Error, String),
    #[error("generated tweet failed validation after retry: {0}")]
    ValidationFailed(#[from] TweetValidationError),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TweetValidationError {
    #[error("tweet was {0} characters, maximum is 280")]
    TooLong(usize),
    #[error("tweet does not mention EKOS")]
    MissingEkosMention,
    #[error("tweet does not include the GitHub link {0}")]
    MissingGithubLink(String),
    #[error("tweet has {0} hashtags, maximum is 3")]
    TooManyHashtags(usize),
    #[error("tweet is empty")]
    Empty,
}

#[derive(serde::Deserialize)]
struct LlmTweetOutput {
    tweet: String,
}

/// Validate the four hard constraints from RFC 0030 / the source design doc's Tweet Prompt.
pub fn validate_tweet(text: &str, config: &MarketingConfig) -> Result<(), TweetValidationError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(TweetValidationError::Empty);
    }
    if trimmed.chars().count() > MAX_TWEET_LEN {
        return Err(TweetValidationError::TooLong(trimmed.chars().count()));
    }
    if !trimmed.to_lowercase().contains("ekos") {
        return Err(TweetValidationError::MissingEkosMention);
    }
    if !trimmed.contains(&config.github) {
        return Err(TweetValidationError::MissingGithubLink(
            config.github.clone(),
        ));
    }
    let hashtag_count = trimmed
        .split_whitespace()
        .filter(|w| w.starts_with('#'))
        .count();
    if hashtag_count > MAX_HASHTAGS {
        return Err(TweetValidationError::TooManyHashtags(hashtag_count));
    }
    Ok(())
}

/// Draft a tweet for `devlog`, retrying once (with the validation failure fed back to the
/// model) if the first draft doesn't pass `validate_tweet`.
pub async fn generate_tweet(
    llm: &dyn LlmProvider,
    config: &MarketingConfig,
    devlog: &DevlogSummary,
) -> Result<TweetDraft, MarketingError> {
    let base_user_prompt = build_user_prompt(devlog, config);

    let first = draft_once(llm, &base_user_prompt).await?;
    match validate_tweet(&first.text, config) {
        Ok(()) => Ok(first),
        Err(e) => {
            tracing::warn!("first tweet draft failed validation ({e}); retrying once");
            let retry_prompt = format!(
                "{base_user_prompt}{}",
                build_retry_suffix(&e.to_string(), &first.text)
            );
            let second = draft_once(llm, &retry_prompt).await?;
            validate_tweet(&second.text, config)?;
            Ok(second)
        }
    }
}

async fn draft_once(
    llm: &dyn LlmProvider,
    user_prompt: &str,
) -> Result<TweetDraft, MarketingError> {
    let req = LlmRequest {
        system: SYSTEM_PROMPT,
        user: user_prompt,
        prompt_version: PROMPT_VERSION,
        max_tokens: MAX_TOKENS,
    };
    let resp = llm.complete(&req).await?;
    let cleaned = strip_json_fences(&resp.content);
    let parsed: LlmTweetOutput = serde_json::from_str(cleaned)
        .map_err(|e| MarketingError::InvalidJson(e, cleaned.to_string()))?;
    Ok(TweetDraft {
        text: parsed.tweet.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_recovery::MockLlmProvider;

    fn config() -> MarketingConfig {
        MarketingConfig {
            github: "https://github.com/alexeyban/EKOS".to_string(),
            hashtags: vec!["Rust".to_string(), "AI".to_string(), "MCP".to_string()],
            twitter: Default::default(),
        }
    }

    fn devlog() -> DevlogSummary {
        DevlogSummary {
            number: 28,
            title: "Incremental compilation".to_string(),
            date: None,
            summary: "Only modified knowledge is rebuilt.".to_string(),
            section_titles: vec![],
        }
    }

    #[test]
    fn valid_tweet_passes() {
        let text = "🚀 EKOS now supports incremental compilation. Only modified knowledge is \
                     rebuilt, making large repos faster. https://github.com/alexeyban/EKOS #Rust #AI #MCP";
        assert!(validate_tweet(text, &config()).is_ok());
    }

    #[test]
    fn rejects_tweet_over_280_chars() {
        let long = "a".repeat(281);
        assert_eq!(
            validate_tweet(&long, &config()),
            Err(TweetValidationError::TooLong(281))
        );
    }

    #[test]
    fn rejects_tweet_missing_ekos_mention() {
        // Deliberately no GitHub link either: this project's URL ends in "/EKOS", so a link
        // alone would satisfy the lowercase "ekos" substring check too — this text has to omit
        // both to isolate the EKOS-mention rule from the GitHub-link rule.
        let text = "New incremental compilation feature landed today. #Rust";
        assert_eq!(
            validate_tweet(text, &config()),
            Err(TweetValidationError::MissingEkosMention)
        );
    }

    #[test]
    fn rejects_tweet_missing_github_link() {
        let text = "EKOS now supports incremental compilation. #Rust";
        assert_eq!(
            validate_tweet(text, &config()),
            Err(TweetValidationError::MissingGithubLink(
                "https://github.com/alexeyban/EKOS".to_string()
            ))
        );
    }

    #[test]
    fn rejects_tweet_with_too_many_hashtags() {
        let text = "EKOS ships incremental compilation. https://github.com/alexeyban/EKOS \
                     #Rust #AI #MCP #DevTools";
        assert_eq!(
            validate_tweet(text, &config()),
            Err(TweetValidationError::TooManyHashtags(4))
        );
    }

    #[test]
    fn rejects_empty_tweet() {
        assert_eq!(
            validate_tweet("   ", &config()),
            Err(TweetValidationError::Empty)
        );
    }

    #[tokio::test]
    async fn generate_tweet_parses_valid_llm_json_response() {
        let llm = MockLlmProvider::new(
            r#"{"tweet": "🚀 EKOS now supports incremental compilation. Only modified knowledge is rebuilt. https://github.com/alexeyban/EKOS #Rust #AI"}"#,
        );
        let draft = generate_tweet(&llm, &config(), &devlog()).await.unwrap();
        assert!(draft.text.contains("incremental compilation"));
        assert!(draft.text.contains("https://github.com/alexeyban/EKOS"));
    }

    #[tokio::test]
    async fn generate_tweet_strips_markdown_json_fences() {
        let llm = MockLlmProvider::new(
            "```json\n{\"tweet\": \"EKOS ships incremental compilation. https://github.com/alexeyban/EKOS #Rust\"}\n```",
        );
        let draft = generate_tweet(&llm, &config(), &devlog()).await.unwrap();
        assert_eq!(
            draft.text,
            "EKOS ships incremental compilation. https://github.com/alexeyban/EKOS #Rust"
        );
    }

    #[tokio::test]
    async fn generate_tweet_fails_when_retry_also_invalid() {
        // A mock always returns the same (over-length) draft, so both the first attempt and
        // the retry fail validation — generate_tweet must surface that, not loop forever.
        let too_long = "a".repeat(300);
        let llm = MockLlmProvider::new(format!(r#"{{"tweet": "{too_long}"}}"#));
        let err = generate_tweet(&llm, &config(), &devlog())
            .await
            .unwrap_err();
        assert!(matches!(err, MarketingError::ValidationFailed(_)));
    }
}
