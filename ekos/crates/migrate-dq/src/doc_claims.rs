//! RFC 0158 `DQ.CONSIST.DOC` / RFC 0172 Phase 3 — what a column's documentation *claims*, in a
//! form the data can check.
//!
//! Only **checkable** claims are read: never null, unique, one of a fixed set, within a numeric
//! range. Free text that makes no such claim produces nothing — this is not a prose comparer. A
//! sentence that explicitly allows the opposite ("may be null", "optional", "unique per account")
//! suppresses the claim rather than being misread as one.
//!
//! Measurement is the caller's job ([`DocClaim::violation_sql`] is the query); like every rule in
//! this crate, the queries count rows and never return a value from the data.

use serde::{Deserialize, Serialize};

/// One checkable statement about a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Claim {
    NotNull,
    Unique,
    /// The documented set of values, as written (codes compare exactly).
    OneOf {
        values: Vec<String>,
    },
    /// Inclusive bounds unless `min_exclusive` (e.g. "positive" → > 0).
    Range {
        min: Option<f64>,
        max: Option<f64>,
        #[serde(default)]
        min_exclusive: bool,
    },
}

/// A claim plus the words it was read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocClaim {
    pub claim: Claim,
    /// The phrase that made the claim, for the report ("never null", "one of A, L, Q, I, E").
    pub phrase: String,
}

impl Claim {
    /// A short name for the claim, used in finding objects and conflict attributes.
    pub fn key(&self) -> &'static str {
        match self {
            Self::NotNull => "not_null",
            Self::Unique => "unique",
            Self::OneOf { .. } => "one_of",
            Self::Range { .. } => "range",
        }
    }

    /// How the claim reads in a report.
    pub fn describe(&self) -> String {
        match self {
            Self::NotNull => "never null".into(),
            Self::Unique => "unique".into(),
            Self::OneOf { values } => format!("one of {}", values.join(", ")),
            Self::Range {
                min,
                max,
                min_exclusive,
            } => match (min, max) {
                (Some(a), Some(b)) => format!("between {} and {}", num(*a), num(*b)),
                (Some(a), None) if *min_exclusive => format!("greater than {}", num(*a)),
                (Some(a), None) => format!("at least {}", num(*a)),
                (None, Some(b)) => format!("at most {}", num(*b)),
                (None, None) => "a range".into(),
            },
        }
    }

    /// The SQL that counts rows contradicting the claim, or `None` when it cannot be checked on
    /// this column (a range on a non-numeric type). Identifiers are quoted; documented values are
    /// SQL string literals with quotes doubled — never interpolated raw.
    pub fn violation_sql(&self, table: &str, column: &str, data_type: &str) -> Option<String> {
        let t = qualify(table);
        let c = ident(column);
        match self {
            Self::NotNull => Some(format!("SELECT count(*) FROM {t} WHERE {c} IS NULL")),
            Self::Unique => Some(format!(
                "SELECT COALESCE(sum(n - 1), 0) FROM (SELECT count(*) AS n FROM {t} \
                 WHERE {c} IS NOT NULL GROUP BY {c} HAVING count(*) > 1) d"
            )),
            Self::OneOf { values } if !values.is_empty() => {
                let list = values
                    .iter()
                    .map(|v| format!("'{}'", v.replace('\'', "''")))
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(format!(
                    "SELECT count(*) FROM {t} WHERE {c} IS NOT NULL AND {c}::text NOT IN ({list})"
                ))
            }
            Self::OneOf { .. } => None,
            Self::Range {
                min,
                max,
                min_exclusive,
            } => {
                if !is_numeric(data_type) {
                    return None;
                }
                let mut parts = Vec::new();
                if let Some(a) = min {
                    let op = if *min_exclusive { "<=" } else { "<" };
                    parts.push(format!("{c} {op} {}", num(*a)));
                }
                if let Some(b) = max {
                    parts.push(format!("{c} > {}", num(*b)));
                }
                (!parts.is_empty()).then(|| {
                    format!(
                        "SELECT count(*) FROM {t} WHERE {c} IS NOT NULL AND ({})",
                        parts.join(" OR ")
                    )
                })
            }
        }
    }
}

fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn qualify(table: &str) -> String {
    table.split('.').map(ident).collect::<Vec<_>>().join(".")
}

fn is_numeric(data_type: &str) -> bool {
    let t = data_type.to_ascii_lowercase();
    let base = t.split('(').next().unwrap_or("").trim();
    matches!(
        base,
        "smallint"
            | "integer"
            | "int"
            | "int2"
            | "int4"
            | "int8"
            | "bigint"
            | "numeric"
            | "decimal"
            | "real"
            | "double precision"
            | "float4"
            | "float8"
            | "money"
    )
}

// ── Extraction ──────────────────────────────────────────────────────────────────────────────

const ALLOWS_NULL: [&str; 6] = [
    "may be null",
    "can be null",
    "nullable",
    "optional",
    "if any",
    "null when",
];

const NOT_NULL: [&str; 9] = [
    "never null",
    "not null",
    "must not be null",
    "cannot be null",
    "can't be null",
    "is required",
    "always set",
    "always present",
    "mandatory",
];

/// "unique per account", "unique within a batch" — a composite claim this column alone cannot test.
const SCOPED_UNIQUE: [&str; 6] = [
    "unique per",
    "unique within",
    "unique for each",
    "unique together",
    "unique combination",
    "unique in combination",
];

fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Every checkable claim in `text`, at most one per kind.
pub fn extract(text: &str) -> Vec<DocClaim> {
    let lower = text.to_lowercase();
    let mut out: Vec<DocClaim> = Vec::new();

    if !ALLOWS_NULL.iter().any(|p| lower.contains(p))
        && let Some(p) = NOT_NULL.iter().find(|p| has_word(&lower, p))
    {
        out.push(DocClaim {
            claim: Claim::NotNull,
            phrase: (*p).to_string(),
        });
    }

    if !SCOPED_UNIQUE.iter().any(|p| lower.contains(p))
        && !lower.contains("not unique")
        && !lower.contains("non-unique")
        && (has_word(&lower, "unique") || lower.contains("no duplicates"))
    {
        out.push(DocClaim {
            claim: Claim::Unique,
            phrase: if lower.contains("no duplicates") {
                "no duplicates".into()
            } else {
                "unique".into()
            },
        });
    }

    if let Some(values) = legend(text).or_else(|| one_of(text)) {
        out.push(DocClaim {
            phrase: format!("one of {}", values.join(", ")),
            claim: Claim::OneOf { values },
        });
    }

    if let Some(d) = range(&lower) {
        out.push(d);
    }
    out
}

/// `A=asset,L=liability,Q=Equity` → `[A, L, Q]`. Two or more short `code=label` pairs only; one
/// `=` in prose is not a legend.
fn legend(text: &str) -> Option<Vec<String>> {
    let mut codes = Vec::new();
    for part in text.split([',', ';', '\n']) {
        let Some((code, label)) = part.split_once('=') else {
            continue;
        };
        let code = code.trim().trim_matches(['\'', '"']);
        let label = label.trim();
        let ok = !code.is_empty()
            && code.len() <= 12
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && !label.is_empty();
        if ok {
            codes.push(code.to_string());
        }
    }
    (codes.len() >= 2).then_some(codes)
}

/// `one of A, B or C` / `values: a, b, c` / `either X or Y`.
fn one_of(text: &str) -> Option<Vec<String>> {
    let lower = text.to_lowercase();
    let start = ["one of:", "one of", "values:", "allowed values", "either "]
        .iter()
        .find_map(|m| lower.find(m).map(|i| i + m.len()))?;
    let rest = &text[start..];
    let rest = rest.trim_start_matches([':', ' ']);
    let end = rest.find(['.', ';', '\n', '(']).unwrap_or(rest.len());
    let list = &rest[..end];
    let values: Vec<String> = list
        .split(',')
        .flat_map(|p| p.split(" or "))
        .flat_map(|p| p.split(" and "))
        .map(|v| v.trim().trim_matches(['\'', '"', '`', ' ']).to_string())
        .filter(|v| !v.is_empty() && v.len() <= 40 && v.split_whitespace().count() <= 3)
        .collect();
    (values.len() >= 2).then_some(values)
}

fn number_after(s: &str) -> Option<(f64, usize)> {
    let t = s.trim_start();
    let skipped = s.len() - t.len();
    let end = t
        .char_indices()
        .take_while(|(i, c)| c.is_ascii_digit() || *c == '.' || (*i == 0 && *c == '-'))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    t[..end]
        .trim_end_matches('.')
        .parse::<f64>()
        .ok()
        .map(|v| (v, skipped + end))
}

fn range(lower: &str) -> Option<DocClaim> {
    let mk = |min, max, min_exclusive, phrase: &str| {
        Some(DocClaim {
            claim: Claim::Range {
                min,
                max,
                min_exclusive,
            },
            phrase: phrase.to_string(),
        })
    };
    for (marker, sep) in [("between ", " and "), ("from ", " to ")] {
        for (i, _) in lower.match_indices(marker) {
            let after = &lower[i + marker.len()..];
            if let Some((a, used)) = number_after(after)
                && let Some(rest) = after[used..].strip_prefix(sep)
                && let Some((b, _)) = number_after(rest)
                && a <= b
            {
                return mk(
                    Some(a),
                    Some(b),
                    false,
                    &format!("between {} and {}", num(a), num(b)),
                );
            }
        }
    }
    if has_word(lower, "non-negative") || lower.contains("never negative") {
        return mk(Some(0.0), None, false, "non-negative");
    }
    if (has_word(lower, "positive") && !lower.contains("positive or"))
        || lower.contains("greater than zero")
    {
        return mk(Some(0.0), None, true, "positive");
    }
    for (marker, is_min) in [("at least ", true), ("at most ", false)] {
        if let Some(i) = lower.find(marker)
            && let Some((v, _)) = number_after(&lower[i + marker.len()..])
        {
            return if is_min {
                mk(Some(v), None, false, &format!("at least {}", num(v)))
            } else {
                mk(None, Some(v), false, &format!("at most {}", num(v)))
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<&'static str> {
        extract(text).iter().map(|d| d.claim.key()).collect()
    }

    #[test]
    fn not_null_claims_and_their_negations() {
        assert_eq!(kinds("The invoice number. Never null."), vec!["not_null"]);
        assert_eq!(
            kinds("Required: is required for posted invoices"),
            vec!["not_null"]
        );
        assert!(kinds("May be null for drafts; never null once posted").is_empty());
        assert!(kinds("Optional reference to a parent").is_empty());
        assert!(
            kinds("annotation").is_empty(),
            "`not` inside a word is not a claim"
        );
    }

    #[test]
    fn unique_claims_skip_scoped_ones() {
        assert_eq!(kinds("Unique invoice number"), vec!["unique"]);
        assert_eq!(
            kinds("Customer code, no duplicates allowed"),
            vec!["unique"]
        );
        assert!(kinds("Unique per account").is_empty());
        assert!(kinds("Not unique: several lines may share it").is_empty());
        assert!(kinds("uniqueness is enforced elsewhere").is_empty());
    }

    #[test]
    fn a_legend_or_an_enumeration_is_a_fixed_set() {
        let d = extract("A=asset,L=liability,Q=Equity,I=Income,E=expense");
        assert_eq!(
            d[0].claim,
            Claim::OneOf {
                values: vec!["A".into(), "L".into(), "Q".into(), "I".into(), "E".into()]
            }
        );
        let d = extract("Status: one of 'open', 'closed' or 'void'.");
        assert_eq!(
            d[0].claim,
            Claim::OneOf {
                values: vec!["open".into(), "closed".into(), "void".into()]
            }
        );
        assert!(
            extract("Set to x=1 when the import ran").is_empty(),
            "one `=` is not a legend"
        );
    }

    #[test]
    fn numeric_ranges() {
        let r = |t: &str| extract(t).into_iter().find(|d| d.claim.key() == "range");
        assert_eq!(
            r("Discount percentage between 0 and 100").unwrap().claim,
            Claim::Range {
                min: Some(0.0),
                max: Some(100.0),
                min_exclusive: false
            }
        );
        assert_eq!(r("Quantity, always positive").unwrap().phrase, "positive");
        assert_eq!(r("Amount; never negative").unwrap().phrase, "non-negative");
        assert_eq!(r("at least 1 line per order").unwrap().phrase, "at least 1");
        assert!(r("Positive or negative adjustment").is_none());
        assert!(r("Free text notes").is_none());
    }

    #[test]
    fn violation_queries_count_rows_and_quote_everything() {
        let sql = Claim::OneOf {
            values: vec!["A".into(), "O'Brien".into()],
        }
        .violation_sql("public.acc", "cat\"x", "char(1)")
        .unwrap();
        assert_eq!(
            sql,
            "SELECT count(*) FROM \"public\".\"acc\" WHERE \"cat\"\"x\" IS NOT NULL AND \"cat\"\"x\"::text NOT IN ('A', 'O''Brien')"
        );
        assert!(
            Claim::NotNull
                .violation_sql("t", "c", "text")
                .unwrap()
                .contains("IS NULL")
        );
        let range = Claim::Range {
            min: Some(0.0),
            max: None,
            min_exclusive: true,
        };
        assert_eq!(
            range.violation_sql("t", "q", "integer").unwrap(),
            "SELECT count(*) FROM \"t\" WHERE \"q\" IS NOT NULL AND (\"q\" <= 0)"
        );
        assert!(
            range.violation_sql("t", "q", "text").is_none(),
            "no range check on text"
        );
    }
}
