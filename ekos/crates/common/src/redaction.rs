//! Global secrets/PII redaction (RFC 0043) — a baseline content filter EKOS applies at every
//! point raw file content enters the pipeline (`ekos build`'s observation artifacts, `ekos
//! recover`'s direct file reads), so no analyzer or future connector can forget to sanitize its
//! own output before it lands in the append-only ledger, which has no delete path.
//!
//! Deliberately a fixed, transparent table of well-known secret *shapes* — the same "not
//! exhaustive, cheap, easy to extend by adding a row" spirit `dependency_analyzer.rs` (RFC 0019)
//! already uses — not a statistical/entropy-based scanner. It is a baseline floor, not a
//! guarantee of zero leakage: a secret that matches none of these shapes will not be caught.
//!
//! Two independent mechanisms:
//! - [`redact`]/[`redact_json`] replace a matched secret span with `[REDACTED:<label>]`, keeping
//!   the rest of the content's structure intact.
//! - [`is_excluded_path`] flags whole files (by name, e.g. `.env`, `*.pem`) that are almost
//!   entirely secret material with no useful non-secret structure worth keeping — callers should
//!   skip reading/observing these files at all rather than redact them.
//!
//! On by default and not fully disable-able by config (RFC 0043's explicit "global limitation"
//! requirement) — [`RedactionConfig`] can only *add* patterns/exclusions on top of the built-in
//! baseline, never remove from it.

use std::sync::OnceLock;

use regex::Regex;

/// One built-in detection rule: a compiled regex plus the short label used in the redaction
/// placeholder, so redacted content still says *what kind* of secret was there without ever
/// containing the secret itself.
struct SecretPattern {
    label: &'static str,
    regex_src: &'static str,
    /// When true, a match whose `value` capture group is a dotted chain of plain identifiers
    /// (e.g. `settings.azure_openai_api_key`) is left untouched instead of redacted — real
    /// second-order finding (see `looks_like_code_reference`'s doc comment): only
    /// `generic-assigned-secret` sets this, since its unprefixed vendor-agnostic shape is the one
    /// pattern here that can't tell "a literal secret value" from "a reference to a variable/
    /// attribute that merely has a secret-sounding name" (`api_key=settings.azure_openai_api_key`
    /// is real, common code, not a secret).
    skip_dotted_identifier_refs: bool,
}

const BUILTIN_SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        label: "aws-access-key-id",
        regex_src: r"AKIA[0-9A-Z]{16}",
        skip_dotted_identifier_refs: false,
    },
    SecretPattern {
        label: "github-token",
        regex_src: r"gh[pousr]_[A-Za-z0-9]{36,}",
        skip_dotted_identifier_refs: false,
    },
    SecretPattern {
        label: "slack-token",
        regex_src: r"xox[baprs]-[0-9A-Za-z-]{10,}",
        skip_dotted_identifier_refs: false,
    },
    SecretPattern {
        label: "google-api-key",
        regex_src: r"AIza[0-9A-Za-z\-_]{35}",
        skip_dotted_identifier_refs: false,
    },
    SecretPattern {
        label: "stripe-key",
        regex_src: r"sk_(live|test)_[0-9a-zA-Z]{16,}",
        skip_dotted_identifier_refs: false,
    },
    SecretPattern {
        label: "private-key-block",
        regex_src: r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        skip_dotted_identifier_refs: false,
    },
    SecretPattern {
        label: "jwt",
        regex_src: r"eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}",
        skip_dotted_identifier_refs: false,
    },
    // Generic "key/secret/password/token = value" assignment — catches project-specific secrets
    // that don't match a known vendor prefix. The whole match (including the key name) is
    // replaced; we don't need to preserve "password:" text, and over-redacting here is safer
    // than under-redacting — *except* for the one real false-positive class found live against a
    // real project (`pdf-reader`'s `services/ai_service.py`, 2026-08-24): a keyword argument like
    // `api_key=settings.azure_openai_api_key` is a reference to a config value, not a secret
    // literal, and the previous version of this regex would truncate its match at the first `.`
    // (outside the old char class) and splice in a colon-bearing `[REDACTED:...]` placeholder mid-
    // expression — corrupting the source badly enough that the whole file failed to parse and
    // silently dropped every real symbol/import it defined. `value` now includes `.` so a dotted
    // reference is captured whole, and `skip_dotted_identifier_refs` (see `redact`) leaves it
    // untouched entirely rather than redacting a non-secret and breaking the file either way.
    //
    // A second, independent real bug found live 2026-08-25/26 (this file's *own* test fixtures,
    // re-running `ekos recover` against EKOS's own repository): the old `['"]?` on each side of
    // `value` matched *independently* — a real value with no leading quote (`api_key=1.2.3.4-not-
    // an-identifier`, this file's own `redacts_ip_like_value_that_is_not_a_dotted_identifier`
    // test) could still have a trailing `['"]?` consume a real, syntactically-necessary closing
    // quote sitting right after it (e.g. inside `redact("api_key=1.2.3.4-not-an-identifier", ...)`)
    // — an *asymmetric* quote consumption the `regex` crate has no backreference support to
    // prevent directly (`(['"]?)...\1` isn't expressible: this crate is a non-backtracking DFA
    // engine). The replacement text never restores what it ate, so the file's own closing `"`
    // silently vanished, leaving every subsequent line swallowed into one unterminated string
    // literal — the same `RUST003`/"cannot parse string into token stream" failure mode as the
    // first bug, from a different regex mechanism. Fixed with an explicit alternation instead of
    // two independent optionals: `"value"` (both quotes, matched as one unit) or `'value'` (both
    // single quotes) or bare `value` (no quotes at all) — never one quote without its pair, so a
    // quote can only ever be consumed alongside its real matching partner.
    //
    // A third, independent real bug found in the same live run (EKOS's own
    // `crates/marketing/src/oauth1.rs` test fixtures): the label alternation had no word-boundary
    // guard at all, so `secret` matched as a bare *substring* inside a longer real identifier —
    // `api_secret: "consumer-secret".to_string()` matched starting mid-identifier at "secret"
    // (the `api_` prefix was never part of the match), leaving the redacted line reading
    // `api_[REDACTED:...].to_string()` — syntactically `api_` immediately followed by `[...]`,
    // which `syn` parses as an array-index expression on an identifier `api_`, not a struct-
    // literal field initializer, and fails with "expected identifier or integer". A first fix
    // (requiring a real `\b` immediately on both sides of the bare alternatives) overcorrected:
    // it stopped `api_secret`/`access_token_secret` from matching *at all*, since `\b` never
    // fires between two word characters (`_` and `s`) — real compound field names ending in
    // `secret`/`password`/... are exactly as real a target as `api_key`/`access_key` (already
    // explicit compounds in this same list) and must still redact. Fixed properly:
    // `(?:[A-Za-z0-9]+[_-])*` consumes zero or more real leading `word_`/`word-` segments as part
    // of the match itself (not asserted — a `\b` at the very start still guards against starting
    // mid-identifier), so `access_token_secret` matches its *whole* real identifier, never a
    // fragment of it either way.
    SecretPattern {
        label: "generic-assigned-secret",
        regex_src: r#"(?i)\b(?:[A-Za-z0-9]+[_-])*(?:api[_-]?key|secret|passwd|password|access[_-]?key|auth[_-]?token)\b\s*[:=]\s*(?:"(?P<value_dq>[A-Za-z0-9/+_\-.]{8,})"|'(?P<value_sq>[A-Za-z0-9/+_\-.]{8,})'|(?P<value_bare>[A-Za-z0-9/+_\-.]{8,}))"#,
        skip_dotted_identifier_refs: true,
    },
];

/// The one real captured value out of `generic-assigned-secret`'s three mutually-exclusive
/// quote-shape alternatives (`value_dq`/`value_sq`/`value_bare` — see that pattern's own comment
/// for why there are three instead of one `['"]?value['"]?`). Every other built-in pattern has no
/// named group at all, so this is only ever consulted when `skip_dotted_identifier_refs` is set.
fn captured_value<'a>(caps: &'a regex::Captures) -> Option<regex::Match<'a>> {
    caps.name("value_dq")
        .or_else(|| caps.name("value_sq"))
        .or_else(|| caps.name("value_bare"))
}

/// Files whose names match one of these globs are excluded from observation entirely — near-100%
/// secret material, no useful non-secret structure worth redacting-and-keeping.
const BUILTIN_EXCLUDED_GLOBS: &[&str] = &[
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "*.pfx",
    "*.p12",
    "id_rsa",
    "id_rsa.*",
    "id_ed25519",
    "id_ed25519.*",
    "*.ppk",
    "credentials",
    "credentials.json",
    ".npmrc",
    ".netrc",
    ".pgpass",
    "*.jks",
    "*.keystore",
];

fn compiled_builtin_patterns() -> &'static [(&'static str, Regex, bool)] {
    static COMPILED: OnceLock<Vec<(&'static str, Regex, bool)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        BUILTIN_SECRET_PATTERNS
            .iter()
            .map(|p| {
                (
                    p.label,
                    Regex::new(p.regex_src).expect("built-in redaction pattern must compile"),
                    p.skip_dotted_identifier_refs,
                )
            })
            .collect()
    })
}

/// True when `value` is a dotted chain of plain code identifiers (e.g.
/// `settings.azure_openai_api_key`, `os.environ`) — how source code refers to a *variable*,
/// never how a real literal secret value looks (a real key/token is one contiguous alphanumeric/
/// base64-ish run; it doesn't contain a `.` splitting it into valid-identifier segments). Used
/// only by patterns with `skip_dotted_identifier_refs` set, so this never weakens a pattern
/// matching an actual known secret shape (AWS/GitHub/Slack/... keys can't look like this).
fn looks_like_code_reference(value: &str) -> bool {
    value.contains('.')
        && value.split('.').all(|segment| {
            let mut chars = segment.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Additive-only extension of the built-in baseline (RFC 0043's `[security]` `ekos.toml`
/// section) — there is deliberately no way to disable or remove a built-in pattern/exclusion.
#[derive(Debug, Clone, Default)]
pub struct RedactionConfig {
    /// (label, regex source) pairs merged with the built-in secret-pattern table.
    pub extra_patterns: Vec<(String, String)>,
    /// Filename globs merged with the built-in fully-excluded-file table.
    pub extra_excluded_globs: Vec<String>,
}

/// Whether the file at `rel_path` should be excluded from observation entirely. Each glob is
/// checked against both the bare file name (so a name-only baseline pattern like `.env` matches
/// regardless of which directory the file lives in) and the full relative path (so a
/// config-supplied pattern can scope to a specific directory, e.g. `secrets/*.yaml`).
pub fn is_excluded_path(rel_path: &str, config: &RedactionConfig) -> bool {
    let file_name = std::path::Path::new(rel_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path);

    BUILTIN_EXCLUDED_GLOBS
        .iter()
        .copied()
        .chain(config.extra_excluded_globs.iter().map(String::as_str))
        .any(|pat| {
            glob::Pattern::new(pat)
                .map(|p| p.matches(file_name) || p.matches(rel_path))
                .unwrap_or(false)
        })
}

/// Replaces every match of every built-in pattern, plus every valid `config.extra_patterns`
/// regex, with `[REDACTED:<label>]`. An invalid regex in `extra_patterns` is skipped silently
/// (config-driven, so a typo there must not be able to disable the built-in baseline by erroring
/// out the whole call).
pub fn redact(content: &str, config: &RedactionConfig) -> String {
    let mut out = content.to_string();
    for (label, regex, skip_dotted_identifier_refs) in compiled_builtin_patterns() {
        out = redact_with_pattern(&out, label, regex, *skip_dotted_identifier_refs);
    }
    for (label, regex_src) in &config.extra_patterns {
        if let Ok(regex) = Regex::new(regex_src) {
            // Config-supplied patterns stay blanket-replace: they're arbitrary user regexes with
            // no guaranteed `value` capture group, unlike the built-in table above.
            out = redact_with_pattern(&out, label, &regex, false);
        }
    }
    out
}

fn redact_with_pattern(
    content: &str,
    label: &str,
    regex: &Regex,
    skip_dotted_identifier_refs: bool,
) -> String {
    if !skip_dotted_identifier_refs {
        return regex
            .replace_all(content, format!("[REDACTED:{label}]"))
            .into_owned();
    }
    regex
        .replace_all(content, |caps: &regex::Captures| {
            let whole = caps.get(0).expect("whole match always present");
            let Some(value) = captured_value(caps) else {
                // No named group at all (shouldn't happen for this pattern, but never panic on
                // a regex-shape assumption) — fall back to the old whole-match behavior.
                return format!("[REDACTED:{label}]");
            };
            if looks_like_code_reference(value.as_str()) {
                return whole.as_str().to_string();
            }
            // A real, third bug in this same pattern's blast radius, found live in the same run
            // (EKOS's own `crates/marketing/src/oauth1.rs` test fixtures): replacing the *whole*
            // match — including the real field name (`api_key`) and its separator — deletes
            // syntax that's structurally required wherever this text sits inside a real struct
            // literal (`api_key: "consumer-key"` → `[REDACTED:...]` leaves a bare expression
            // where `field_name: value` was required, which `syn` correctly refuses to parse).
            // Only the *value* span gets replaced now — everything else in the match (the real
            // field/env-var name, its `:`/`=` separator, and a real quote character on either
            // side, verbatim) stays untouched, so the result is always a drop-in replacement for
            // only the secret-shaped text itself, never the surrounding structure.
            format!(
                "{}[REDACTED:{label}]{}",
                &content[whole.start()..value.start()],
                &content[value.end()..whole.end()]
            )
        })
        .into_owned()
}

/// Recursively applies [`redact`] to every string value in a `serde_json::Value` — needed since
/// observation artifact content (`ObservationContent.data`) and harvested symbol lists are JSON,
/// not a single string.
pub fn redact_json(value: &mut serde_json::Value, config: &RedactionConfig) {
    match value {
        serde_json::Value::String(s) => *s = redact(s, config),
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json(item, config);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                redact_json(v, config);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RedactionConfig {
        RedactionConfig::default()
    }

    #[test]
    fn redacts_aws_access_key_id() {
        let out = redact("key = AKIAABCDEFGHIJKLMNOP end", &cfg());
        assert!(out.contains("[REDACTED:aws-access-key-id]"));
        assert!(!out.contains("AKIAABCDEFGHIJKLMNOP"));
        assert!(out.contains("key = "));
        assert!(out.contains(" end"));
    }

    #[test]
    fn redacts_github_token() {
        let token = format!("ghp_{}", "a".repeat(36));
        let out = redact(&format!("token: {token}"), &cfg());
        assert!(out.contains("[REDACTED:github-token]"));
        assert!(!out.contains(&token));
    }

    #[test]
    fn redacts_private_key_block() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBAK\n-----END RSA PRIVATE KEY-----";
        let out = redact(&format!("before\n{pem}\nafter"), &cfg());
        assert!(out.contains("[REDACTED:private-key-block]"));
        assert!(!out.contains("MIIBOgIBAAJBAK"));
        assert!(out.contains("before"));
        assert!(out.contains("after"));
    }

    #[test]
    fn redacts_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let out = redact(jwt, &cfg());
        assert!(out.contains("[REDACTED:jwt]"));
        assert!(!out.contains(jwt));
    }

    #[test]
    fn redacts_generic_password_assignment() {
        let out = redact(r#"password = "hunter2fake""#, &cfg());
        assert!(out.contains("[REDACTED:generic-assigned-secret]"));
        assert!(!out.contains("hunter2fake"));
    }

    /// Real bug, found live 2026-08-25/26 against EKOS's own `crates/marketing/src/oauth1.rs`
    /// test fixtures: redacting the *whole* `label: "value"` match (including the real field
    /// name and its separator, the original design) deletes syntax a real struct literal
    /// requires — `api_key: "consumer-key".to_string()` became a bare `[REDACTED:...]` where
    /// Rust needed `field_name: value`, and `syn` correctly refused to parse the result. Only the
    /// value itself must be replaced; the real field/env-var name, separator, and any real quote
    /// character must survive untouched.
    #[test]
    fn redaction_preserves_the_real_field_name_and_quotes_replacing_only_the_value() {
        let out = redact(r#"api_key: "consumer-key".to_string(),"#, &cfg());
        assert_eq!(
            out,
            r#"api_key: "[REDACTED:generic-assigned-secret]".to_string(),"#
        );
    }

    /// The word-boundary companion to the fix above, same real file: `secret` must not match as
    /// a bare *substring* of a longer real identifier like `api_secret`/`access_token_secret` —
    /// doing so silently dropped the identifier's own real prefix (`api_`/`access_token_`) from
    /// the output entirely, a second, independent way the same bug broke real struct-literal
    /// syntax.
    #[test]
    fn label_match_requires_a_real_word_boundary_not_a_substring_of_a_longer_identifier() {
        let out = redact(r#"api_secret: "consumer-secret".to_string(),"#, &cfg());
        assert_eq!(
            out,
            r#"api_secret: "[REDACTED:generic-assigned-secret]".to_string(),"#
        );

        let out2 = redact(
            r#"access_token_secret: "access-token-secret".to_string(),"#,
            &cfg(),
        );
        assert_eq!(
            out2,
            r#"access_token_secret: "[REDACTED:generic-assigned-secret]".to_string(),"#
        );
    }

    #[test]
    fn generic_pattern_leaves_a_dotted_code_reference_untouched() {
        // Real false positive, found live against a real project (`pdf-reader`'s
        // `services/ai_service.py`, 2026-08-24): this is a keyword argument passing a config
        // *reference*, not a secret literal. The old regex truncated its match at the `.` (outside
        // its char class) and spliced a colon-bearing `[REDACTED:...]` placeholder mid-expression,
        // corrupting the line badly enough that the whole file failed to parse — every real
        // function/import it declared was silently dropped from the compiled ledger.
        let line = "api_key=settings.azure_openai_api_key,";
        let out = redact(line, &cfg());
        assert_eq!(
            out, line,
            "a dotted identifier reference must be left untouched"
        );
    }

    #[test]
    fn generic_pattern_still_redacts_a_dotted_value_that_is_not_a_clean_identifier_chain() {
        // A dotted-looking value that isn't actually a valid identifier chain (e.g. a real
        // version-number-shaped or IP-shaped secret) must still be redacted — the exemption is
        // narrowly for genuine code references, not "any value containing a dot".
        let out = redact("api_key=1.2.3.4-not-an-identifier", &cfg());
        assert!(out.contains("[REDACTED:generic-assigned-secret]"));
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        let text = "fn main() { println!(\"hello world\"); }";
        assert_eq!(redact(text, &cfg()), text);
    }

    #[test]
    fn extra_pattern_from_config_is_additive() {
        let config = RedactionConfig {
            extra_patterns: vec![(
                "internal-token".to_string(),
                r"ITKN-[0-9a-f]{8}".to_string(),
            )],
            extra_excluded_globs: Vec::new(),
        };
        let out = redact("token=ITKN-deadbeef", &config);
        assert!(out.contains("[REDACTED:internal-token]"));
        // Baseline still fires alongside the extra pattern.
        let out2 = redact("key = AKIAABCDEFGHIJKLMNOP", &config);
        assert!(out2.contains("[REDACTED:aws-access-key-id]"));
    }

    #[test]
    fn invalid_extra_pattern_regex_is_skipped_not_fatal() {
        let config = RedactionConfig {
            extra_patterns: vec![("bad".to_string(), "(unclosed".to_string())],
            extra_excluded_globs: Vec::new(),
        };
        // Must not panic, and the baseline must still run.
        let out = redact("key = AKIAABCDEFGHIJKLMNOP", &config);
        assert!(out.contains("[REDACTED:aws-access-key-id]"));
    }

    #[test]
    fn is_excluded_path_matches_builtin_globs() {
        assert!(is_excluded_path(".env", &cfg()));
        assert!(is_excluded_path("project/.env.local", &cfg()));
        assert!(is_excluded_path("secrets/id_rsa", &cfg()));
        assert!(is_excluded_path("certs/server.pem", &cfg()));
        assert!(!is_excluded_path("src/main.rs", &cfg()));
        assert!(!is_excluded_path("ekos.toml", &cfg()));
    }

    #[test]
    fn is_excluded_path_extra_glob_is_additive() {
        let config = RedactionConfig {
            extra_patterns: Vec::new(),
            extra_excluded_globs: vec!["secrets/*.yaml".to_string()],
        };
        assert!(is_excluded_path("secrets/prod.yaml", &config));
        // Baseline still applies alongside the extra glob.
        assert!(is_excluded_path(".env", &config));
    }

    #[test]
    fn redact_json_recurses_into_arrays_and_objects() {
        let mut value = serde_json::json!({
            "excerpt": "key = AKIAABCDEFGHIJKLMNOP",
            "symbols": ["fn main", "AKIAABCDEFGHIJKLMNOP"],
            "nested": { "inner": "token: AKIAABCDEFGHIJKLMNOP" },
            "count": 3,
        });
        redact_json(&mut value, &cfg());
        assert!(
            value["excerpt"]
                .as_str()
                .unwrap()
                .contains("[REDACTED:aws-access-key-id]")
        );
        assert!(
            value["symbols"][1]
                .as_str()
                .unwrap()
                .contains("[REDACTED:aws-access-key-id]")
        );
        assert!(
            value["nested"]["inner"]
                .as_str()
                .unwrap()
                .contains("[REDACTED:aws-access-key-id]")
        );
        assert_eq!(value["count"], serde_json::json!(3));
    }
}
