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
}

const BUILTIN_SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        label: "aws-access-key-id",
        regex_src: r"AKIA[0-9A-Z]{16}",
    },
    SecretPattern {
        label: "github-token",
        regex_src: r"gh[pousr]_[A-Za-z0-9]{36,}",
    },
    SecretPattern {
        label: "slack-token",
        regex_src: r"xox[baprs]-[0-9A-Za-z-]{10,}",
    },
    SecretPattern {
        label: "google-api-key",
        regex_src: r"AIza[0-9A-Za-z\-_]{35}",
    },
    SecretPattern {
        label: "stripe-key",
        regex_src: r"sk_(live|test)_[0-9a-zA-Z]{16,}",
    },
    SecretPattern {
        label: "private-key-block",
        regex_src: r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
    },
    SecretPattern {
        label: "jwt",
        regex_src: r"eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}",
    },
    // Generic "key/secret/password/token = value" assignment — catches project-specific secrets
    // that don't match a known vendor prefix. The whole match (including the key name) is
    // replaced; we don't need to preserve "password:" text, and over-redacting here is safer
    // than under-redacting.
    SecretPattern {
        label: "generic-assigned-secret",
        regex_src: r#"(?i)(api[_-]?key|secret|passwd|password|access[_-]?key|auth[_-]?token)\s*[:=]\s*['"]?[A-Za-z0-9/+_\-]{8,}['"]?"#,
    },
];

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

fn compiled_builtin_patterns() -> &'static [(&'static str, Regex)] {
    static COMPILED: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        BUILTIN_SECRET_PATTERNS
            .iter()
            .map(|p| {
                (
                    p.label,
                    Regex::new(p.regex_src).expect("built-in redaction pattern must compile"),
                )
            })
            .collect()
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
    for (label, regex) in compiled_builtin_patterns() {
        out = regex
            .replace_all(&out, format!("[REDACTED:{label}]"))
            .into_owned();
    }
    for (label, regex_src) in &config.extra_patterns {
        if let Ok(regex) = Regex::new(regex_src) {
            out = regex
                .replace_all(&out, format!("[REDACTED:{label}]"))
                .into_owned();
        }
    }
    out
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
