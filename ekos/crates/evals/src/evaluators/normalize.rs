//! Deterministic text normalisation for fact matching (RFC 0139 §2.1).
//!
//! The RFC 0138 ruler compared an expected fact to the answer with raw case-insensitive substring
//! containment. Measured on the real suite, that failed a correct answer for reasons that have
//! nothing to do with correctness: `"redaction"` missed an answer saying "redacted",
//! `"append-only"` missed "append only", `"dbt-gen"` missed `dbt_gen`. Roughly a third of the
//! zero-scoring scenarios hinged on wording rather than substance.
//!
//! This module fixes the mechanical part of that — separators and word endings — while staying
//! fully deterministic (RFC 0138's no-LLM-judge non-goal still holds). Genuine vocabulary
//! differences ("CKM" for "Canonical Knowledge Model") are *not* its job: those are declared per
//! scenario as `any_of` alternates, because guessing at synonyms is exactly the kind of implicit
//! judgement a deterministic ruler should refuse to make.
//!
//! **Symmetry, not linguistics.** Both sides go through the same folding, so an aggressive fold is
//! harmless where a lopsided one would not be: `"ekos"` folds to `"eko"`, and so does the answer's
//! `"ekos"`, so they still match. The only real risk is a false *positive* between two genuinely
//! different words that fold together — accepted deliberately, because the measured failure mode
//! is false negatives by a wide margin.

/// Split text into folded tokens.
///
/// Separators — `_ - . : / \` and whitespace — all become token boundaries, so `dbt-gen`,
/// `dbt_gen` and `dbt gen` are indistinguishable afterwards. Remaining non-alphanumerics are
/// dropped.
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| fold(&t.to_lowercase()))
        .collect()
}

/// Strip one common inflectional ending so `redaction`/`redacted`/`redacting` share a form.
///
/// Deliberately a short, explicit list rather than a real stemmer: every entry here is a word
/// ending that produced a measured false negative on the RFC 0138 suite, and a reader can predict
/// exactly what this does. Words of four characters or fewer are left alone — stripping them
/// collapses too many distinct short identifiers together.
fn fold(token: &str) -> String {
    if token.len() <= 4 || token.chars().all(|c| c.is_ascii_digit()) {
        return token.to_string();
    }
    // Order matters and is load-bearing: `-ion` must be tried before any longer ending that
    // contains it, or `redaction` folds to `red` instead of `redact` and stops matching
    // `redacted`/`redacts`. Caught by this module's own test rather than by reasoning.
    for suffix in ["ions", "ion", "ings", "ing", "ed", "es", "s"] {
        if let Some(stem) = token.strip_suffix(suffix)
            && stem.len() >= 3
        {
            return drop_trailing_e(stem);
        }
    }
    drop_trailing_e(token)
}

/// Drop one trailing `e` so a silent-`e` singular converges with its plural.
///
/// Without this, `tombstone` stays whole while `tombstones` folds to `tombston`, and the pair stops
/// matching — the asymmetry that broke the measured `tombstone` scenario. Applied on both the
/// stripped and unstripped paths so the two always land in the same place.
fn drop_trailing_e(token: &str) -> String {
    match token.strip_suffix('e') {
        Some(stem) if stem.len() >= 4 => stem.to_string(),
        _ => token.to_string(),
    }
}

/// Does `needle` occur as a contiguous run of tokens inside `haystack`?
///
/// Token-sequence containment rather than raw substring matching, so `"extend"` cannot be satisfied
/// by an unrelated word that merely contains those letters, while a multi-word fact like
/// `"Canonical Knowledge Model"` still matches across whatever whitespace the answer used.
pub fn contains_tokens(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// True when `fact` appears in `text` under this module's normalisation.
pub fn text_contains_fact(text: &str, fact: &str) -> bool {
    contains_tokens(&tokens(text), &tokens(fact))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every case here is a real false negative from the RFC 0138 suite.
    #[test]
    fn word_endings_no_longer_decide_correctness() {
        assert!(text_contains_fact(
            "the symbols were redacted by the pass",
            "redaction"
        ));
        assert!(text_contains_fact("it redacts secrets", "redaction"));
        assert!(text_contains_fact(
            "deletion is modelled with tombstones",
            "tombstone"
        ));
        assert!(text_contains_fact("the config extends it", "extend"));
    }

    #[test]
    fn separators_are_interchangeable() {
        assert!(text_contains_fact(
            "the ledger is append only",
            "append-only"
        ));
        assert!(text_contains_fact("rendered by dbt_gen", "dbt-gen"));
        assert!(text_contains_fact("see dbt gen", "dbt-gen"));
        assert!(text_contains_fact("in sql_analyzer", "sql analyzer"));
    }

    #[test]
    fn a_multi_word_fact_matches_across_whitespace() {
        assert!(text_contains_fact(
            "compiled into the  Canonical   Knowledge Model.",
            "Canonical Knowledge Model"
        ));
    }

    #[test]
    fn an_absent_fact_still_does_not_match() {
        assert!(!text_contains_fact(
            "it uses the recovery crate",
            "sql_analyzer"
        ));
        assert!(!text_contains_fact(
            "a runtime environment",
            "Canonical Knowledge Model"
        ));
    }

    #[test]
    fn token_boundaries_prevent_a_substring_false_positive() {
        // Raw `contains` would match "extend" inside "extended-warranty-department"; token
        // matching should not fire on an unrelated word that merely embeds the letters.
        assert!(!text_contains_fact("the superintendent said no", "extend"));
    }

    /// Real v1 false positives found by re-grading the R0 transcripts (RFC 0139 §2.1).
    ///
    /// These matter more than the false negatives that motivated this module: the old ruler was
    /// not merely too strict, it was *also* too loose, and in the direction that flatters the
    /// system. Each answer below scored full marks under substring matching while being wrong.
    #[test]
    fn a_fact_embedded_in_a_larger_identifier_is_not_a_match() {
        // arch-006 expects "runtime"; the answer only ever says "AiRuntime", and answers the
        // wrong question ("AiRuntime.kind = RustSymbol"). v1 scored it 1.0.
        assert!(!text_contains_fact(
            "The kind of AiRuntime is RustSymbol.",
            "runtime"
        ));
        // code-008 expects the crate "kir"; the answer claims the crate is "KirObject".
        assert!(!text_contains_fact(
            "the crate that owns the `KirObject` type is \"KirObject\"",
            "kir"
        ));
        // arch-002 expects the pipeline stage "compile"; the answer lists "ekos-compiler-core".
        assert!(!text_contains_fact(
            "1. ekos-compiler-core (12) 2. walkdir (4)",
            "compile"
        ));
    }

    #[test]
    fn short_tokens_and_numbers_are_left_alone() {
        // RFC numbers and editions must not be folded — "0043" and "2024" are exact answers.
        assert!(text_contains_fact("introduced in RFC 0043", "0043"));
        assert!(text_contains_fact("edition 2024", "2024"));
        assert!(!text_contains_fact("edition 2021", "2024"));
    }
}
