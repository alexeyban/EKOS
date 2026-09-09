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

/// The first balanced `{...}` object in `s`, ignoring braces inside string literals.
///
/// [`strip_json_fences`] only helps when the *whole* response is JSON once fences are removed. It
/// does not survive the thing local models actually do: a preamble. Observed live on
/// `llama3:latest` (2026-09-09) — `llm_description` failed on a real object with
/// `expected value at line 1 column 1`, serde's message for "this did not start with JSON".
///
/// A response like `Here is the JSON:\n{"overview": "..."}` is a *correct answer wrapped in
/// politeness*, and throwing it away costs a real LLM call that has already been paid for. This
/// recovers the object instead.
///
/// String-aware on purpose: a naive scan for the last `}` breaks on a brace inside a string value
/// (`{"overview": "handles the {id} route"}`), and `ai.rs` learned that the hard way with citation
/// blocks. Escapes are honoured so `\"` inside a string does not end it early.
///
/// Returns `None` when there is no balanced object at all — a genuinely unusable response, which
/// the caller should report rather than paper over.
pub fn extract_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// [`strip_json_fences`], then [`extract_json_object`] as a fallback — the parse most callers
/// want. Falls back to the fence-stripped text when no balanced object is found, so the caller's
/// own error message still describes the real response rather than an empty string.
pub fn json_body(s: &str) -> &str {
    let stripped = strip_json_fences(s);
    extract_json_object(stripped).unwrap_or(stripped)
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

    /// The real failure this was written for: a model that answers correctly but chats first.
    #[test]
    fn a_preamble_before_the_json_is_recovered() {
        let raw = "Here is the JSON:\n{\"overview\": \"does the thing\"}";
        assert_eq!(
            json_body(raw),
            "{\"overview\": \"does the thing\"}",
            "a correct answer wrapped in politeness must not cost a paid LLM call"
        );
        assert!(serde_json::from_str::<serde_json::Value>(json_body(raw)).is_ok());
    }

    #[test]
    fn trailing_commentary_after_the_json_is_dropped() {
        let raw = "{\"overview\": \"x\"}\n\nLet me know if you need more detail!";
        assert_eq!(json_body(raw), "{\"overview\": \"x\"}");
    }

    /// A naive "first `{` to last `}`" scan passes the preamble test and fails this one.
    #[test]
    fn braces_inside_string_values_do_not_confuse_the_scan() {
        let raw = r#"{"overview": "handles the {id} route", "usage": "}"}"#;
        assert_eq!(json_body(raw), raw);
        let v: serde_json::Value = serde_json::from_str(json_body(raw)).unwrap();
        assert_eq!(v["overview"], "handles the {id} route");
        assert_eq!(v["usage"], "}");
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string_early() {
        let raw = r#"prose {"overview": "he said \"hi\" {x}"} more"#;
        let v: serde_json::Value = serde_json::from_str(json_body(raw)).unwrap();
        assert_eq!(v["overview"], r#"he said "hi" {x}"#);
    }

    #[test]
    fn nested_objects_are_kept_whole() {
        let raw = r#"ok: {"a": {"b": {"c": 1}}} done"#;
        assert_eq!(json_body(raw), r#"{"a": {"b": {"c": 1}}}"#);
    }

    /// An unusable response must stay unusable — silently inventing a parse would be worse than
    /// the error it replaces.
    #[test]
    fn a_response_with_no_object_is_not_rescued() {
        assert_eq!(extract_json_object("I cannot answer that."), None);
        assert_eq!(extract_json_object(""), None);
        // Unterminated: no balanced object exists, so there is nothing honest to return.
        assert_eq!(extract_json_object(r#"{"overview": "truncated"#), None);
        // json_body still hands back the real text so the caller can report what arrived.
        assert_eq!(json_body("I cannot answer that."), "I cannot answer that.");
    }
}
