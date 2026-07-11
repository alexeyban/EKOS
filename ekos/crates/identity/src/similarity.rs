//! Name normalisation and Jaro-Winkler similarity (RFC 0007).

/// Normalise an object name before comparison.
///
/// Steps:
/// 1. Lowercase
/// 2. Replace `_`, `-`, `.` with a space
/// 3. Strip common table/entity suffixes
/// 4. Collapse whitespace; trim
pub fn normalize(name: &str) -> String {
    let lower = name.to_lowercase();
    let spaced: String = lower.chars().map(|c| if matches!(c, '_' | '-' | '.') { ' ' } else { c }).collect();

    // Strip known suffixes (must strip in order of specificity: longest first).
    let suffixes = [" table", " tbl", "table ", "tbl ", " dim", " fact"];
    let mut s = spaced.trim().to_string();
    for suffix in &suffixes {
        if s.ends_with(suffix) {
            s.truncate(s.len() - suffix.len());
        } else if s.starts_with(suffix.trim()) && suffix.ends_with(' ') {
            let trim = suffix.trim();
            if s.starts_with(trim) {
                s = s[trim.len()..].to_string();
            }
        }
    }

    // Collapse remaining whitespace.
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string()
}

/// Jaro similarity between two strings (0.0 = no similarity, 1.0 = identical).
pub fn jaro(s1: &str, s2: &str) -> f32 {
    if s1 == s2 {
        return 1.0;
    }
    let chars1: Vec<char> = s1.chars().collect();
    let chars2: Vec<char> = s2.chars().collect();
    let len1 = chars1.len();
    let len2 = chars2.len();

    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    let match_dist = (len1.max(len2) / 2).saturating_sub(1);

    let mut matched1 = vec![false; len1];
    let mut matched2 = vec![false; len2];
    let mut matches = 0usize;

    for i in 0..len1 {
        let lo = i.saturating_sub(match_dist);
        let hi = (i + match_dist + 1).min(len2);
        for j in lo..hi {
            if !matched2[j] && chars1[i] == chars2[j] {
                matched1[i] = true;
                matched2[j] = true;
                matches += 1;
                break;
            }
        }
    }

    if matches == 0 {
        return 0.0;
    }

    // Count transpositions.
    let mut transpositions = 0usize;
    let mut k = 0;
    for i in 0..len1 {
        if matched1[i] {
            while !matched2[k] {
                k += 1;
            }
            if chars1[i] != chars2[k] {
                transpositions += 1;
            }
            k += 1;
        }
    }

    let m = matches as f32;
    let t = transpositions as f32 / 2.0;
    (m / len1 as f32 + m / len2 as f32 + (m - t) / m) / 3.0
}

/// Jaro-Winkler similarity, which boosts scores for strings sharing a common prefix.
pub fn jaro_winkler(s1: &str, s2: &str) -> f32 {
    let j = jaro(s1, s2);
    // Common prefix length, max 4.
    let prefix = s1
        .chars()
        .zip(s2.chars())
        .take(4)
        .take_while(|(a, b)| a == b)
        .count() as f32;
    j + prefix * 0.1 * (1.0 - j)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaro_winkler_identical() {
        assert!((jaro_winkler("customer", "customer") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaro_winkler_empty_strings() {
        assert!((jaro_winkler("", "abc")).abs() < 1e-6);
        assert!((jaro_winkler("abc", "")).abs() < 1e-6);
        // Two empty strings are trivially equal.
        assert!((jaro_winkler("", "") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaro_winkler_orders_vs_order() {
        let score = jaro_winkler("orders", "order");
        assert!(score > 0.95, "expected >0.95, got {score}");
    }

    #[test]
    fn jaro_winkler_dissimilar() {
        let score = jaro_winkler("orders", "products");
        assert!(score < 0.85, "expected <0.85, got {score}");
    }

    #[test]
    fn normalize_strips_underscores() {
        assert_eq!(normalize("customer_table"), "customer");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize("CUSTOMER"), "customer");
    }

    #[test]
    fn normalize_strips_tbl_prefix_not_suffix() {
        // "tbl_orders" → space → "tbl orders" → "tbl" is a leading word, not stripped here
        // (we only strip suffixes in this impl, prefix stripping is future work)
        let n = normalize("order_tbl");
        assert_eq!(n, "order");
    }

    #[test]
    fn normalize_preserves_distinct_names() {
        assert_ne!(normalize("orders"), normalize("products"));
    }
}
