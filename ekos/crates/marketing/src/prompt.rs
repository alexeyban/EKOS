//! Tweet-generation prompt (RFC 0030). System prompt is transcribed verbatim from the
//! source design doc's "Tweet Prompt" section; the user prompt embeds the devlog's
//! `Summary` + `section_titles` plus the config-driven GitHub URL and hashtag pool.

use ekos_compiler_core::config::MarketingConfig;

use crate::devlog::DevlogSummary;

pub const PROMPT_VERSION: &str = "marketing-tweet-v1";

pub const SYSTEM_PROMPT: &str = r#"You are an experienced DevRel engineer.

Your job is to announce software releases on X (Twitter).

Rules:
- Write naturally.
- No hype.
- No clickbait.
- No emojis except one optional rocket (🚀) at the start.
- Maximum 280 characters, including the GitHub link and hashtags.
- Focus on developer value.
- Never invent features that are not described in the input.
- Always mention EKOS by name.
- Always include the GitHub link exactly as given.
- Include at most 3 hashtags, chosen from the list given.

Respond ONLY with valid JSON in this exact schema — no markdown fences, no commentary:
{"tweet": "<the tweet text, ready to post>"}"#;

pub fn build_user_prompt(devlog: &DevlogSummary, config: &MarketingConfig) -> String {
    let sections = if devlog.section_titles.is_empty() {
        String::new()
    } else {
        format!("\nSections covered: {}\n", devlog.section_titles.join(", "))
    };
    let hashtags = config.hashtags.join(", ");

    format!(
        "Release summary:\n{}\n{}\nGitHub link to include: {}\nAvailable hashtags (pick at most 3): {}",
        devlog.summary, sections, config.github, hashtags
    )
}

/// Appended to the user prompt on a regeneration retry after `validate_tweet` rejects the
/// first draft (RFC 0030's "Tweet too long → Regenerate" error-handling rule).
pub fn build_retry_suffix(reason: &str) -> String {
    format!(
        "\n\nYour previous draft was rejected: {reason}. Write a new draft that fixes this \
         while still following all the rules above."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
            summary: "Only modified knowledge is rebuilt, making large repositories faster."
                .to_string(),
            section_titles: vec!["RFC 0016 — Fact-Segment Engine".to_string()],
        }
    }

    #[test]
    fn system_prompt_encodes_every_hard_rule() {
        assert!(SYSTEM_PROMPT.contains("Maximum 280 characters"));
        assert!(SYSTEM_PROMPT.contains("Always mention EKOS"));
        assert!(SYSTEM_PROMPT.contains("Always include the GitHub link"));
        assert!(SYSTEM_PROMPT.contains("at most 3 hashtags"));
        assert!(SYSTEM_PROMPT.contains("Never invent features"));
    }

    #[test]
    fn user_prompt_embeds_summary_github_and_hashtags() {
        let prompt = build_user_prompt(&devlog(), &config());
        assert!(prompt.contains("Only modified knowledge is rebuilt"));
        assert!(prompt.contains("https://github.com/alexeyban/EKOS"));
        assert!(prompt.contains("Rust, AI, MCP"));
        assert!(prompt.contains("RFC 0016 — Fact-Segment Engine"));
    }

    #[test]
    fn retry_suffix_carries_the_rejection_reason() {
        let suffix = build_retry_suffix("tweet was 312 characters, max is 280");
        assert!(suffix.contains("312 characters"));
    }
}
