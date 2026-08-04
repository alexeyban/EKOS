//! RFC 5849 OAuth 1.0a request signing (RFC 0027) — required to `POST` to the X API as a
//! specific user without a browser-based OAuth 2.0 PKCE flow.
//!
//! Implemented from spec and unit-tested against the RFC 2202 HMAC-SHA1 test vector plus
//! internal determinism/sensitivity checks. **Not yet exercised against a live X account** —
//! no credentials are available in this environment (see RFC 0027's Open Questions).

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use rand::Rng;
use rand::distributions::Alphanumeric;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

/// RFC 3986 unreserved characters (`A-Z a-z 0-9 - . _ ~`) are the only bytes OAuth 1.0a
/// leaves un-encoded; every other ASCII punctuation/space character listed here must be
/// percent-encoded, and `percent_encoding` always encodes non-ASCII bytes regardless of set.
const OAUTH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub fn percent_encode(s: &str) -> String {
    utf8_percent_encode(s, OAUTH_ENCODE_SET).to_string()
}

#[derive(Debug, Clone)]
pub struct OauthCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
}

pub fn generate_nonce() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs()
}

/// `key=value` pairs, percent-encoded and sorted per RFC 5849 §3.4.1.3.2, joined with `&`.
fn normalized_param_string(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// RFC 5849 §3.4.1.1 signature base string. `params` must already exclude `oauth_signature`
/// itself and (for this crate's use — no query string / form-encoded body on `/2/tweets`)
/// contains only the `oauth_*` protocol parameters.
pub fn signature_base_string(method: &str, url: &str, params: &BTreeMap<String, String>) -> String {
    format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(url),
        percent_encode(&normalized_param_string(params))
    )
}

fn sign(
    creds: &OauthCredentials,
    method: &str,
    url: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let base_string = signature_base_string(method, url, params);
    let signing_key = format!(
        "{}&{}",
        percent_encode(&creds.api_secret),
        percent_encode(&creds.access_token_secret)
    );

    let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes())
        .expect("HMAC-SHA1 accepts a key of any length");
    mac.update(base_string.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Builds the full `Authorization: OAuth ...` header value for one request.
pub fn authorization_header(creds: &OauthCredentials, method: &str, url: &str) -> String {
    let nonce = generate_nonce();
    let timestamp = unix_timestamp();

    let mut params: BTreeMap<String, String> = BTreeMap::new();
    params.insert("oauth_consumer_key".to_string(), creds.api_key.clone());
    params.insert("oauth_nonce".to_string(), nonce);
    params.insert(
        "oauth_signature_method".to_string(),
        "HMAC-SHA1".to_string(),
    );
    params.insert("oauth_timestamp".to_string(), timestamp.to_string());
    params.insert("oauth_token".to_string(), creds.access_token.clone());
    params.insert("oauth_version".to_string(), "1.0".to_string());

    let signature = sign(creds, method, url, &params);
    params.insert("oauth_signature".to_string(), signature);

    let header_params = params
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("OAuth {header_params}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_leaves_unreserved_characters_alone() {
        assert_eq!(percent_encode("abcXYZ019-._~"), "abcXYZ019-._~");
    }

    #[test]
    fn percent_encode_escapes_reserved_characters() {
        assert_eq!(percent_encode("hello world!"), "hello%20world%21");
        assert_eq!(percent_encode("a=b&c"), "a%3Db%26c");
        assert_eq!(
            percent_encode("https://x.com/2/tweets"),
            "https%3A%2F%2Fx.com%2F2%2Ftweets"
        );
    }

    /// RFC 2202 test case 1 — validates the raw HMAC-SHA1 primitive independent of any
    /// OAuth-specific base-string construction, so a bug there can't hide a broken primitive.
    #[test]
    fn hmac_sha1_matches_rfc2202_test_vector_1() {
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let expected = "b617318655057264e28bc0b6fb378c8ef146be00";

        let mut mac = HmacSha1::new_from_slice(&key).unwrap();
        mac.update(data);
        let result = mac.finalize().into_bytes();
        let hex: String = result.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, expected);
    }

    fn creds() -> OauthCredentials {
        OauthCredentials {
            api_key: "consumer-key".to_string(),
            api_secret: "consumer-secret".to_string(),
            access_token: "access-token".to_string(),
            access_token_secret: "access-token-secret".to_string(),
        }
    }

    fn base_params() -> BTreeMap<String, String> {
        let mut p = BTreeMap::new();
        p.insert("oauth_consumer_key".to_string(), "consumer-key".to_string());
        p.insert("oauth_nonce".to_string(), "fixednonce123".to_string());
        p.insert(
            "oauth_signature_method".to_string(),
            "HMAC-SHA1".to_string(),
        );
        p.insert("oauth_timestamp".to_string(), "1700000000".to_string());
        p.insert("oauth_token".to_string(), "access-token".to_string());
        p.insert("oauth_version".to_string(), "1.0".to_string());
        p
    }

    #[test]
    fn signature_is_deterministic_for_identical_inputs() {
        let url = "https://api.twitter.com/2/tweets";
        let s1 = sign(&creds(), "POST", url, &base_params());
        let s2 = sign(&creds(), "POST", url, &base_params());
        assert_eq!(s1, s2);
    }

    #[test]
    fn signature_changes_when_any_single_input_changes() {
        let url = "https://api.twitter.com/2/tweets";
        let baseline = sign(&creds(), "POST", url, &base_params());

        let mut different_params = base_params();
        different_params.insert("oauth_nonce".to_string(), "differentnonce456".to_string());
        assert_ne!(sign(&creds(), "POST", url, &different_params), baseline);

        let mut different_creds = creds();
        different_creds.api_secret = "a-different-consumer-secret".to_string();
        assert_ne!(
            sign(&different_creds, "POST", url, &base_params()),
            baseline
        );

        assert_ne!(
            sign(
                &creds(),
                "POST",
                "https://api.twitter.com/2/other",
                &base_params()
            ),
            baseline
        );

        assert_ne!(sign(&creds(), "GET", url, &base_params()), baseline);
    }

    #[test]
    fn authorization_header_has_oauth_scheme_and_all_required_params() {
        let header = authorization_header(&creds(), "POST", "https://api.twitter.com/2/tweets");
        assert!(header.starts_with("OAuth "));
        for key in [
            "oauth_consumer_key",
            "oauth_nonce",
            "oauth_signature",
            "oauth_signature_method",
            "oauth_timestamp",
            "oauth_token",
            "oauth_version",
        ] {
            assert!(header.contains(key), "missing {key} in header: {header}");
        }
    }

    #[test]
    fn nonce_is_reasonably_unique_across_calls() {
        let a = generate_nonce();
        let b = generate_nonce();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
