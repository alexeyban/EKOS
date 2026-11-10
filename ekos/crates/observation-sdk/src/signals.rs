//! RFC 0252 §3.2 — the profile signals an analyst asks for, as SQL text and answer parsing. Pure.
//!
//! - **Literal coverage** (a): how many rows hold each value the code mentions, and how many hold
//!   a value no code mentions. A data value never leaves the database: the query returns a value
//!   only when it equals a literal the caller passed in — a literal recovered from code, already in
//!   the ledger — and `NULL` for every other value, so the unexplained rows come back as counts.
//! - **Temporal profile** (b): the first and last month each literal was seen, on a time column
//!   chosen by name ([`time_column`]).
//! - **Candidate keys** (d): the distinct-tuple count of a column set, searched levelwise
//!   ([`search_keys`]): a superset of a key is never tested.
//!
//! Every query is an aggregate over one table, rendered for one [`Flavor`]; identifiers go through
//! [`profile::ident`], literals through [`sql_string`].

use crate::profile::{self, Family, Flavor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// At most this many literals in one `IN` list (Oracle refuses more than 1000).
pub const MAX_LITERALS: usize = 500;

/// A literal as recovered from code (`'A'`, `7`, `-1`) → the text a database renders the value as.
pub fn literal_text(v: &str) -> String {
    let t = v.trim();
    if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
        t[1..t.len() - 1].replace("''", "'")
    } else {
        t.to_string()
    }
}

/// A SQL string literal for `flavor`.
pub fn sql_string(flavor: Flavor, s: &str) -> String {
    let body = s.replace('\'', "''");
    match flavor {
        // ClickHouse also reads backslash escapes inside a string literal.
        Flavor::ClickHouse => format!("'{}'", body.replace('\\', "\\\\")),
        _ => format!("'{body}'"),
    }
}

/// A column's value as text, so one `IN` list of text literals compares with any column type.
fn as_text(flavor: Flavor, col: &str) -> String {
    match flavor {
        Flavor::Oracle => format!("TO_CHAR({col})"),
        Flavor::Postgres => format!("{col}::text"),
        Flavor::SqlServer => format!("CAST({col} AS nvarchar(4000))"),
        Flavor::ClickHouse => format!("toString({col})"),
    }
}

/// `expr`'s month, `YYYY-MM`.
fn month(flavor: Flavor, expr: &str) -> String {
    match flavor {
        Flavor::Oracle => format!("TO_CHAR({expr}, 'YYYY-MM')"),
        Flavor::Postgres => format!("to_char({expr}, 'YYYY-MM')"),
        Flavor::SqlServer => format!("CONVERT(char(7), {expr}, 126)"),
        Flavor::ClickHouse => format!("formatDateTime({expr}, '%Y-%m')"),
    }
}

fn count(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::ClickHouse => "count()",
        Flavor::Oracle => "COUNT(*)",
        Flavor::Postgres => "count(*)",
        Flavor::SqlServer => "COUNT_BIG(*)",
    }
}

/// The distinct literal texts, capped at [`MAX_LITERALS`], sorted.
fn literal_texts(literals: &[String]) -> Vec<String> {
    let set: BTreeSet<String> = literals.iter().map(|l| literal_text(l)).collect();
    set.into_iter().take(MAX_LITERALS).collect()
}

fn in_list(flavor: Flavor, texts: &[String]) -> String {
    texts
        .iter()
        .map(|t| sql_string(flavor, t))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One row per literal the column holds plus one row (value `NULL`) for all other values:
/// `value, rows, first month, last month`. The month cells are `NULL` without a time column.
pub fn literal_coverage_sql(
    flavor: Flavor,
    owner: &str,
    table: &str,
    column: &str,
    literals: &[String],
    time_column: Option<&str>,
) -> Result<String, String> {
    let texts = literal_texts(literals);
    if texts.is_empty() {
        return Err("no literal to count".into());
    }
    let tbl = profile::table_ref(flavor, owner, table)?;
    let col = profile::ident(flavor, column)?;
    let txt = as_text(flavor, &col);
    let bucket = format!(
        "CASE WHEN {txt} IN ({}) THEN {txt} END",
        in_list(flavor, &texts)
    );
    let (first, last) = match time_column {
        Some(t) => {
            let t = profile::ident(flavor, t)?;
            (
                month(flavor, &format!("MIN({t})")),
                month(flavor, &format!("MAX({t})")),
            )
        }
        None => ("NULL".to_string(), "NULL".to_string()),
    };
    Ok(format!(
        "SELECT {bucket}, {}, {first}, {last} FROM {tbl} WHERE {col} IS NOT NULL GROUP BY {bucket}",
        count(flavor)
    ))
}

/// How many distinct values the column holds that no literal names.
pub fn unexplained_distinct_sql(
    flavor: Flavor,
    owner: &str,
    table: &str,
    column: &str,
    literals: &[String],
) -> Result<String, String> {
    let texts = literal_texts(literals);
    let tbl = profile::table_ref(flavor, owner, table)?;
    let col = profile::ident(flavor, column)?;
    let txt = as_text(flavor, &col);
    let not_in = if texts.is_empty() {
        String::new()
    } else {
        format!(" AND {txt} NOT IN ({})", in_list(flavor, &texts))
    };
    Ok(format!(
        "SELECT COUNT(DISTINCT {txt}) FROM {tbl} WHERE {col} IS NOT NULL{not_in}"
    ))
}

/// Rows, first and last month of one value group.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub rows: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
}

/// One coded column measured: counts only, and literals that came from code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// Rows where the column is not null.
    pub non_null: i64,
    /// Per literal **as the ledger writes it** (`'A'`, `7`): its rows; 0 when the data never holds it.
    pub literals: BTreeMap<String, Usage>,
    /// Rows holding a value no literal names.
    pub unexplained: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unexplained_distinct: Option<i64>,
    /// The latest month any non-null row of the column was seen (the reference for "dead").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
}

fn cell(row: &[Option<String>], i: usize) -> Option<String> {
    row.get(i)
        .cloned()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The answer of [`literal_coverage_sql`] for `literals` (as the ledger writes them).
pub fn parse_coverage(literals: &[String], rows: &[Vec<Option<String>>]) -> Coverage {
    let mut by_text: BTreeMap<String, Usage> = BTreeMap::new();
    let mut out = Coverage::default();
    for r in rows {
        let n = cell(r, 1)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0) as i64;
        let u = Usage {
            rows: n,
            first_seen: cell(r, 2),
            last_seen: cell(r, 3),
        };
        out.non_null += n;
        out.latest = out.latest.max(u.last_seen.clone());
        match row_value(r) {
            // A value in a NULL group is the "everything else" bucket; never a data value.
            None => out.unexplained = u,
            Some(v) => {
                by_text.insert(v, u);
            }
        }
    }
    for l in literals {
        let u = by_text.get(&literal_text(l)).cloned().unwrap_or_default();
        out.literals.insert(l.clone(), u);
    }
    out
}

fn row_value(r: &[Option<String>]) -> Option<String> {
    r.first().cloned().flatten()
}

/// Months between `from` and `to` (`YYYY-MM`), `None` when either does not parse.
pub fn months_between(from: &str, to: &str) -> Option<i64> {
    let ym = |s: &str| -> Option<i64> {
        let (y, m) = s.get(..7)?.split_once('-')?;
        Some(y.parse::<i64>().ok()? * 12 + m.parse::<i64>().ok()?)
    };
    Some(ym(to)? - ym(from)?)
}

/// Name fragments of a creation or event time, strongest first. An update time is last: it moves
/// every row forward and hides when a code was last *written*.
const TIME_NAMES: &[&[&str]] = &[
    &[
        "created",
        "creation",
        "createdon",
        "date_created",
        "create_date",
    ],
    &["inserted", "insert_date", "entered", "entry_date"],
    &[
        "transdate",
        "trans_date",
        "dateacct",
        "datetrx",
        "dateordered",
        "dateinvoiced",
    ],
    &[
        "posting_date",
        "document_date",
        "order_date",
        "invoice_date",
        "event_time",
    ],
    &["updated", "modified", "last_update", "changed"],
];

/// The column that dates a row, chosen by name among date/time columns; `None` when no name fits.
pub fn time_column(flavor: Flavor, columns: &[Value]) -> Option<String> {
    let dated: Vec<&str> = columns
        .iter()
        .filter(|c| profile::family(flavor, c["data_type"].as_str().unwrap_or("")) == Family::Date)
        .filter_map(|c| c["name"].as_str())
        .filter(|n| ekos_common::pii::classify_name(n).is_none())
        .collect();
    for group in TIME_NAMES {
        let mut hits: Vec<&str> = dated
            .iter()
            .copied()
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                group
                    .iter()
                    .any(|g| l == *g || l.starts_with(g) || l.ends_with(g))
            })
            .collect();
        hits.sort();
        if let Some(h) = hits.first() {
            return Some((*h).to_string());
        }
    }
    None
}

// ── Candidate keys ─────────────────────────────────────────────────────────────────────────────

/// Rows whose `columns` are all non-null, counted as distinct tuples.
pub fn distinct_tuples_sql(
    flavor: Flavor,
    owner: &str,
    table: &str,
    columns: &[String],
) -> Result<String, String> {
    if columns.is_empty() {
        return Err("no column".into());
    }
    let tbl = profile::table_ref(flavor, owner, table)?;
    let cols = columns
        .iter()
        .map(|c| profile::ident(flavor, c))
        .collect::<Result<Vec<_>, _>>()?;
    let not_null = cols
        .iter()
        .map(|c| format!("{c} IS NOT NULL"))
        .collect::<Vec<_>>()
        .join(" AND ");
    Ok(format!(
        "SELECT {} FROM (SELECT DISTINCT {} FROM {tbl} WHERE {not_null}) d",
        count(flavor),
        cols.join(", ")
    ))
}

/// Columns worth testing as part of a key: non-null, countable, not personal, not long text;
/// highest distinct count first, at most `limit`.
pub fn key_candidates(flavor: Flavor, columns: &[Value], limit: usize) -> Vec<String> {
    let mut scored: Vec<(i64, String)> = columns
        .iter()
        .filter_map(|c| {
            let name = c["name"].as_str()?;
            let fam = profile::family(flavor, c["data_type"].as_str().unwrap_or(""));
            if !matches!(fam, Family::Number | Family::Date | Family::Text) {
                return None;
            }
            if ekos_common::pii::classify_name(name).is_some() {
                return None;
            }
            let p = &c["profile"];
            let non_null = c["not_null"] == true || p["nulls"].as_i64() == Some(0);
            if !non_null {
                return None;
            }
            if fam == Family::Text && p["max_length"].as_i64().is_some_and(|l| l > 100) {
                return None;
            }
            Some((p["distinct"].as_i64().unwrap_or(-1), name.to_string()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, n)| n).collect()
}

/// What a key search found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySearch {
    /// Minimal keys found, in the order found.
    pub keys: Vec<Vec<String>>,
    /// Column sets tested (one query each).
    pub tested: usize,
    /// The query budget ran out before every set up to the width was tested.
    pub exhausted: bool,
    /// Failures, as engine messages (redacted by the caller).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Levelwise search for minimal keys among `candidates`: a set is a key when its distinct
/// non-null tuples equal `rows`. A superset of a found key is never tested. `distinct` runs one
/// [`distinct_tuples_sql`] and returns its count.
pub fn search_keys(
    candidates: &[String],
    rows: i64,
    max_width: usize,
    max_queries: usize,
    distinct: &mut dyn FnMut(&[String]) -> Result<i64, String>,
) -> KeySearch {
    let mut out = KeySearch::default();
    if rows <= 0 {
        return out;
    }
    for width in 1..=max_width.min(candidates.len()) {
        for set in combinations(candidates, width) {
            if out.keys.iter().any(|k| k.iter().all(|c| set.contains(c))) {
                continue;
            }
            if out.tested >= max_queries {
                out.exhausted = true;
                return out;
            }
            out.tested += 1;
            match distinct(&set) {
                Ok(n) if n == rows => out.keys.push(set),
                Ok(_) => {}
                Err(e) => out.errors.push(e),
            }
        }
    }
    out
}

/// Every `k`-subset of `items`, in lexicographic index order.
fn combinations(items: &[String], k: usize) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut idx: Vec<usize> = (0..k).collect();
    if k == 0 || k > items.len() {
        return out;
    }
    loop {
        out.push(idx.iter().map(|&i| items[i].clone()).collect());
        let mut i = k;
        while i > 0 && idx[i - 1] == items.len() - k + i - 1 {
            i -= 1;
        }
        if i == 0 {
            return out;
        }
        idx[i - 1] += 1;
        for j in i..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lits(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn coverage_sql_returns_only_literals_or_null_in_every_dialect() {
        for f in [
            Flavor::Postgres,
            Flavor::Oracle,
            Flavor::ClickHouse,
            Flavor::SqlServer,
        ] {
            let sql = literal_coverage_sql(
                f,
                "public",
                "orders",
                "status",
                &lits(&["1", "'A'", "'it''s'"]),
                Some("created_at"),
            )
            .unwrap();
            // The value column is a CASE that yields a literal or NULL — never the raw column.
            assert!(sql.starts_with("SELECT CASE WHEN"), "{sql}");
            assert!(sql.contains("THEN"), "{sql}");
            assert!(!sql.contains("ELSE"), "{sql}");
            assert!(sql.contains("'it''s'"), "{sql}");
            assert!(sql.contains("'A'") && sql.contains("'1'"), "{sql}");
        }
    }

    #[test]
    fn a_hostile_literal_is_quoted_not_executed() {
        let sql = literal_coverage_sql(
            Flavor::ClickHouse,
            "db",
            "t",
            "c",
            &lits(&["'x\\' OR 1=1 --'"]),
            None,
        )
        .unwrap();
        assert!(sql.contains("'x\\\\'' OR 1=1 --'"), "{sql}");
    }

    #[test]
    fn parse_keeps_the_ledger_spelling_and_counts_unseen_literals_as_zero() {
        let rows = vec![
            vec![
                Some("1".into()),
                Some("10".into()),
                Some("2020-01".into()),
                Some("2026-10".into()),
            ],
            vec![
                Some("A".into()),
                Some("5".into()),
                Some("2021-03".into()),
                Some("2022-04".into()),
            ],
            vec![
                None,
                Some("4".into()),
                Some("2024-01".into()),
                Some("2026-11".into()),
            ],
        ];
        let c = parse_coverage(&lits(&["1", "'A'", "3"]), &rows);
        assert_eq!(c.non_null, 19);
        assert_eq!(c.literals["1"].rows, 10);
        assert_eq!(c.literals["'A'"].last_seen.as_deref(), Some("2022-04"));
        assert_eq!(c.literals["3"].rows, 0);
        assert_eq!(c.unexplained.rows, 4);
        assert_eq!(c.latest.as_deref(), Some("2026-11"));
    }

    #[test]
    fn months_between_counts_calendar_months() {
        assert_eq!(months_between("2024-05", "2026-11"), Some(30));
        assert_eq!(months_between("x", "2026-11"), None);
    }

    #[test]
    fn time_column_prefers_creation_over_update_and_skips_birth_dates() {
        let cols = json!([
            {"name": "updated_at", "data_type": "timestamp"},
            {"name": "date_of_birth", "data_type": "date"},
            {"name": "created_at", "data_type": "timestamp with time zone"},
            {"name": "status", "data_type": "integer"},
        ]);
        assert_eq!(
            time_column(Flavor::Postgres, cols.as_array().unwrap()).as_deref(),
            Some("created_at")
        );
        let only_birth = json!([{"name": "date_of_birth", "data_type": "date"}]);
        assert_eq!(
            time_column(Flavor::Postgres, only_birth.as_array().unwrap()),
            None
        );
    }

    #[test]
    fn composite_key_found_and_no_superset_is_tested() {
        // (order_id, line_no) is the key; order_id alone is not.
        let cands = lits(&["order_id", "line_no", "sku"]);
        let mut asked: Vec<Vec<String>> = Vec::new();
        let r = search_keys(&cands, 100, 3, 50, &mut |set| {
            asked.push(set.to_vec());
            Ok(
                if set.contains(&"order_id".to_string()) && set.contains(&"line_no".to_string()) {
                    100
                } else {
                    40
                },
            )
        });
        assert_eq!(r.keys, vec![lits(&["order_id", "line_no"])]);
        assert!(
            !asked.iter().any(|s| s.len() == 3),
            "the superset (order_id, line_no, sku) was tested: {asked:?}"
        );
        // 3 singles + 3 pairs; the only triple contains the key.
        assert_eq!(r.tested, 6);
    }

    #[test]
    fn the_query_budget_holds() {
        let cands = lits(&["a", "b", "c", "d", "e"]);
        let r = search_keys(&cands, 10, 3, 4, &mut |_| Ok(1));
        assert_eq!(r.tested, 4);
        assert!(r.exhausted);
    }

    #[test]
    fn key_candidates_skip_nullable_personal_and_long_text() {
        let cols = json!([
            {"name": "id", "data_type": "integer", "not_null": true, "profile": {"distinct": 100}},
            {"name": "email", "data_type": "text", "not_null": true},
            {"name": "note", "data_type": "text", "not_null": true, "profile": {"max_length": 900}},
            {"name": "parent", "data_type": "integer"},
            {"name": "line_no", "data_type": "integer", "profile": {"nulls": 0, "distinct": 5}},
        ]);
        assert_eq!(
            key_candidates(Flavor::Postgres, cols.as_array().unwrap(), 8),
            lits(&["id", "line_no"])
        );
    }

    #[test]
    fn distinct_tuples_sql_counts_non_null_tuples() {
        let sql = distinct_tuples_sql(
            Flavor::Oracle,
            "ADEMPIERE",
            "C_ORDERLINE",
            &lits(&["C_ORDER_ID", "LINE"]),
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT COUNT(*) FROM (SELECT DISTINCT \"C_ORDER_ID\", \"LINE\" FROM \"ADEMPIERE\".\"C_ORDERLINE\" \
             WHERE \"C_ORDER_ID\" IS NOT NULL AND \"LINE\" IS NOT NULL) d"
        );
    }
}
