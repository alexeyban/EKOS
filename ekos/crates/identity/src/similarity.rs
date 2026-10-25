//! Name normalisation and Jaro-Winkler similarity (RFC 0007).

use std::collections::HashSet;

use ekos_kir::KirObject;

/// Lowercased column-name set from a `KirObject`'s `properties["columns"]`,
/// or `None` if absent/empty. Two on-disk shapes exist and both must work:
/// SQL DDL tables store `[{"name": ..., "data_type": ...}]` (`sql_analyzer.rs`);
/// Transformation IR `Source`/`Sink` nodes store a plain string array
/// (`crates/semantic/src/transform_ir.rs`'s `TransformNode::properties()`).
/// Shared by `structural_score` (RFC 0007, same-kind dedup) and
/// `cross_system` (RFC 0029, cross-kind/cross-system matching).
pub fn column_names(obj: &KirObject) -> Option<HashSet<String>> {
    let cols = obj.properties.get("columns")?.as_array()?;
    let names: HashSet<String> = cols
        .iter()
        .filter_map(|c| {
            c.as_str()
                .or_else(|| c.get("name").and_then(|n| n.as_str()))
                .map(str::to_lowercase)
        })
        .collect();
    if names.is_empty() { None } else { Some(names) }
}

/// Lowercased, deduplicated set of cell text from a `KirObject`'s `properties["rows"]`
/// (`Vec<Vec<String>>`, as written by `local_docs_analyzer.rs` for a PDF/DOCX-extracted table) —
/// the real structural signal PDF-derived `Table` objects carry, distinct from SQL-derived
/// tables' `"columns"` property. `None` if absent/empty, matching `column_names`'s own convention
/// so `structural_score` can fall through the same way.
///
/// Without this, `structural_score` had no way to distinguish two different tables extracted from
/// the same PDF (same long name prefix, no `columns` property) and fell back to its blanket `1.0`
/// "no structural signal" default — the same over-merge failure shape RFC 0024/0060 already fixed
/// for other kinds, just never closed for PDF-sourced `Table` objects specifically (confirmed
/// live: 9 distinct tables from one real PDF collapsed into one canonical object at confidence
/// 0.99).
pub fn row_cell_tokens(obj: &KirObject) -> Option<HashSet<String>> {
    let rows = obj.properties.get("rows")?.as_array()?;
    let mut cells = HashSet::new();
    for row in rows {
        let Some(row_cells) = row.as_array() else {
            continue;
        };
        for cell in row_cells {
            if let Some(s) = cell.as_str() {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    cells.insert(trimmed.to_lowercase());
                }
            }
        }
    }
    if cells.is_empty() { None } else { Some(cells) }
}

/// Jaccard similarity (intersection over union) between two sets.
pub fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f32 / union as f32
}

/// Normalise an object name before comparison.
///
/// Steps:
/// 1. Lowercase
/// 2. Replace `_`, `-`, `.` with a space
/// 3. Strip common table/entity suffixes
/// 4. Collapse whitespace; trim
pub fn normalize(name: &str) -> String {
    let lower = name.to_lowercase();
    let spaced: String = lower
        .chars()
        .map(|c| if matches!(c, '_' | '-' | '.') { ' ' } else { c })
        .collect();

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
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// RFC 0224: a name's words — split on anything that is not a letter or digit and at camelCase
/// boundaries, lower-cased. `HR_ListType` → `hr list type`, `@docusaurus/core` → `docusaurus core`.
pub fn tokens(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// RFC 0224: whether two names can name the same thing word for word. Names that differ by a
/// whole word are a parent and its child or two siblings — `ASP_PROCESS` / `ASP_PROCESS_PARA`,
/// `HR_ListType` / `HR_ListLine`, `react` / `react-dom`, `@docusaurus/core` / `@docusaurus/utils` —
/// however much of the spelling they share. Compatible: the same words; the same letters split
/// differently (`anozimada` / `Anozi Mada`); or words that differ only as spelling variants of one
/// another (`customer` / `customers`, `cust` / `customer`).
pub fn token_compatible(a: &str, b: &str) -> bool {
    let ta = significant(tokens(a));
    let tb = significant(tokens(b));
    if ta.concat() == tb.concat() {
        return true;
    }
    let mut only_b = tb.clone();
    let mut only_a = Vec::new();
    for t in ta {
        match only_b.iter().position(|x| *x == t) {
            Some(i) => {
                only_b.remove(i);
            }
            None => only_a.push(t),
        }
    }
    if only_a.len() != only_b.len() {
        return false;
    }
    // Pair each remaining word with a close variant, best first.
    for t in only_a {
        let best = only_b
            .iter()
            .enumerate()
            .map(|(i, x)| (i, jaro_winkler(&t, x)))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        match best {
            Some((i, score)) if score >= TOKEN_VARIANT => {
                only_b.remove(i);
            }
            _ => return false,
        }
    }
    true
}

/// Without the words [`normalize`] also drops: `customer_table` is `customer`.
fn significant(mut t: Vec<String>) -> Vec<String> {
    const AFFIXES: [&str; 4] = ["table", "tbl", "dim", "fact"];
    if t.len() > 1 && AFFIXES.contains(&t[t.len() - 1].as_str()) {
        t.pop();
    }
    if t.len() > 1 && AFFIXES.contains(&t[0].as_str()) {
        t.remove(0);
    }
    t
}

/// The Jaro-Winkler score above which two words are spelling variants of each other
/// (`customer`/`customers` 0.98, `cust`/`customer` 0.90; `input`/`output` 0.73, `type`/`line` 0.5).
pub const TOKEN_VARIANT: f32 = 0.9;

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
    fn names_that_differ_by_a_whole_word_are_not_compatible() {
        for (a, b) in [
            ("ASP_PROCESS", "ASP_PROCESS_PARA"),
            ("HR_ListType", "HR_ListLine"),
            ("HR_ListType", "HR_List"),
            ("react", "react-dom"),
            ("@docusaurus/core", "@docusaurus/utils"),
            ("docusaurus-lunr-search", "docusaurus-plugin-llms"),
            ("WS_WebServiceFieldInput", "WS_WebServiceFieldOutput"),
            ("PP_Order_Node_Trl", "PP_Order_BOM_Trl"),
            ("Build Private Images GHCR", "Build Public Images GHCR"),
        ] {
            assert!(!token_compatible(a, b), "{a} / {b}");
        }
        for (a, b) in [
            ("anozimada", "Anozi Mada"),
            ("customer", "customers"),
            ("C_BPartner", "c_bpartner"),
            ("cust_mstr", "customer_master"),
            ("orders", "Orders"),
            ("customer_table", "customer"),
        ] {
            assert!(token_compatible(a, b), "{a} / {b}");
        }
        assert_eq!(tokens("HR_ListType"), ["hr", "list", "type"]);
        assert_eq!(tokens("@docusaurus/core"), ["docusaurus", "core"]);
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
