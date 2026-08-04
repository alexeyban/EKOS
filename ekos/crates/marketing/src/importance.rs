//! Importance classification (RFC 0027) — a deterministic keyword heuristic, not an LLM
//! call, so a chores-only devlog never reaches the tweet-generation step at all.

use crate::devlog::DevlogSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importance {
    /// Documentation/tests/refactoring only — per the source design doc, no tweet.
    Low,
    /// Default: some feature-shaped change.
    Medium,
    /// Major feature — an accepted RFC, or an explicit "new capability" signal.
    High,
}

const LOW_SIGNAL_WORDS: &[&str] = &[
    "typo",
    "documentation",
    "docs",
    "formatting",
    "comment",
    "comments",
    "refactor",
    "refactoring",
    "chore",
    "test",
    "tests",
];

const HIGH_SIGNAL_WORDS: &[&str] = &["rfc", "feat", "feature", "new capability"];

/// Words whose presence rules a devlog IN as feature-shaped, overriding a LOW-looking title.
const FEATURE_SIGNAL_WORDS: &[&str] = &[
    "add",
    "adds",
    "added",
    "new",
    "implement",
    "implements",
    "implemented",
];

pub fn classify(devlog: &DevlogSummary) -> Importance {
    let haystack = format!(
        "{} {} {}",
        devlog.title.to_lowercase(),
        devlog.summary.to_lowercase(),
        devlog.section_titles.join(" ").to_lowercase()
    );

    let has_high = HIGH_SIGNAL_WORDS.iter().any(|w| haystack.contains(w));
    if has_high {
        return Importance::High;
    }

    let has_feature_signal = FEATURE_SIGNAL_WORDS.iter().any(|w| haystack.contains(w));
    let has_low_signal = LOW_SIGNAL_WORDS.iter().any(|w| haystack.contains(w));

    if has_low_signal && !has_feature_signal {
        Importance::Low
    } else {
        Importance::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devlog(title: &str, summary: &str, section_titles: Vec<&str>) -> DevlogSummary {
        DevlogSummary {
            number: 1,
            title: title.to_string(),
            date: None,
            summary: summary.to_string(),
            section_titles: section_titles.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn pure_chore_devlog_is_low() {
        let d = devlog(
            "Typo fixes and formatting cleanup",
            "Fixed a few typos in comments and reformatted some files. Also refactored tests.",
            vec!["Files Changed detail"],
        );
        assert_eq!(classify(&d), Importance::Low);
    }

    #[test]
    fn devlog_mentioning_an_rfc_is_high() {
        let d = devlog(
            "RFC 0025/0026: more document formats",
            "This session closed real gaps with new document parsing.",
            vec!["RFC 0025 — Additional Document Formats"],
        );
        assert_eq!(classify(&d), Importance::High);
    }

    #[test]
    fn ordinary_feature_improvement_is_medium() {
        let d = devlog(
            "Improve parser performance",
            "The parser is now significantly faster for large repositories.",
            vec!["Implementation details"],
        );
        assert_eq!(classify(&d), Importance::Medium);
    }

    #[test]
    fn low_signal_words_with_a_feature_signal_stay_medium() {
        // "Fixed a memory leak while adding incremental compilation" — has "fixed" (not a LOW
        // word by itself) and "adding" (a feature signal), so this must not be misclassified
        // as LOW just because bugfix-adjacent language appears.
        let d = devlog(
            "Incremental compiler",
            "Added incremental compilation and refactored the cache layer along the way.",
            vec![],
        );
        assert_eq!(classify(&d), Importance::Medium);
    }
}
