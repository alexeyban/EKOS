//! Shared parsing helpers for LLM responses (RFC 0008).

/// Strip markdown code fences from an LLM response so the remainder can be fed
/// straight to `serde_json`.
///
/// Every prompt in this workspace asks for bare JSON, but models routinely wrap
/// it in ```json fences anyway; both `SqlAnalyzerPass` and
/// `DocumentSemanticsAnalyzerPass` need identical handling.
pub fn strip_json_fences(s: &str) -> &str {
    s.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_json_is_unchanged() {
        assert_eq!(strip_json_fences(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    #[test]
    fn json_fence_is_stripped() {
        assert_eq!(
            strip_json_fences("```json\n{\"a\":1}\n```"),
            "{\"a\":1}",
            "a ```json-fenced body must come back as parseable JSON"
        );
    }

    #[test]
    fn bare_fence_is_stripped() {
        assert_eq!(strip_json_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(strip_json_fences("  \n {\"a\":1} \n "), "{\"a\":1}");
    }
}
