//! Static findings: what can be said about a query from the schema and the dialect alone,
//! without running it and without the answer it should give.

use crate::analyze::{Analysis, CmpOp, NumKind, analyze};
use crate::schema::Schema;
use ekos_sql_dialect_sdk::DialectFacts;
use serde::{Deserialize, Serialize};
use sqlparser::dialect::Dialect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The query is certainly wrong (it cannot run, or names what does not exist).
    Error,
    /// The query may well mean something other than intended.
    Warn,
}

/// The context that would settle a finding — what an agent or a repair round looks up next.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fetch {
    DescribeTable {
        table: String,
    },
    FindValues {
        text: String,
        column: Option<String>,
    },
    JoinPath {
        from: String,
        to: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetch: Option<Fetch>,
}

impl Finding {
    fn new(code: &str, severity: Severity, message: String, fetch: Option<Fetch>) -> Finding {
        Finding {
            code: code.into(),
            severity,
            message,
            fetch,
        }
    }
}

/// Every lint code, in order.
pub const LINT_CODES: &[&str] = &[
    "L001", "L002", "L003", "L004", "L005", "L006", "L007", "L008", "L009", "L010", "L011", "E001",
];

/// Parses and lints `sql`. A statement the dialect's parser cannot read gives `E001` (a warning:
/// the parser's coverage of a dialect is incomplete, the engine is the judge of syntax) plus the
/// checks that need no parse; `Err` holds the parse error for callers that want it.
pub fn lint(
    sql: &str,
    dialect: &dyn Dialect,
    facts: &DialectFacts,
    schema: &Schema,
) -> Result<(Analysis, Vec<Finding>), (String, Vec<Finding>)> {
    let mut text = text_findings(sql, facts);
    match analyze(sql, dialect, schema) {
        Ok(a) => {
            text.extend(lint_analysis(&a, facts, schema));
            Ok((a, text))
        }
        Err(e) => {
            text.push(Finding::new(
                "E001",
                Severity::Warn,
                format!("the query does not parse as this dialect: {e}"),
                None,
            ));
            Err((e, text))
        }
    }
}

/// L011 (backticks where the dialect has none) — read from the text, since a parser for such a
/// dialect stops at the first backtick.
fn text_findings(sql: &str, facts: &DialectFacts) -> Vec<Finding> {
    let mut out = Vec::new();
    if facts.identifier_quote.is_some() && !facts.backtick_identifiers && has_backtick(sql) {
        let q = facts.identifier_quote.unwrap_or('"');
        let (open, close) = if q == '[' { ('[', ']') } else { (q, q) };
        out.push(Finding::new(
            "L011",
            Severity::Error,
            format!("this dialect does not quote names with backticks: write {open}name{close}"),
            None,
        ));
    }
    out
}

/// A backtick outside single-quoted strings.
fn has_backtick(sql: &str) -> bool {
    let mut in_string = false;
    for c in sql.chars() {
        match c {
            '\'' => in_string = !in_string,
            '`' if !in_string => return true,
            _ => {}
        }
    }
    false
}

pub fn lint_analysis(a: &Analysis, facts: &DialectFacts, schema: &Schema) -> Vec<Finding> {
    let mut out = Vec::new();
    unknown_names(a, schema, &mut out);
    joins(a, schema, &mut out);
    literals(a, schema, &mut out);
    divisions(a, facts, &mut out);
    sentinels(a, schema, &mut out);
    null_order(a, facts, schema, &mut out);
    dates(a, facts, schema, &mut out);
    dialect_forms(a, facts, schema, &mut out);
    out
}

/// L009 (row-limit form), L010 (`ILIKE`), L011 (`"name"` read as a string).
fn dialect_forms(a: &Analysis, facts: &DialectFacts, schema: &Schema, out: &mut Vec<Finding>) {
    use ekos_sql_dialect_sdk::RowLimit as R;
    let accepted: &[&str] = match facts.row_limit {
        R::Unknown => &["LIMIT", "FETCH", "TOP"],
        R::Limit => &["LIMIT"],
        R::FetchFirst => &["FETCH"],
        R::LimitOrFetch => &["LIMIT", "FETCH"],
        R::TopOrFetch => &["TOP", "FETCH"],
    };
    let wanted = match facts.row_limit {
        R::Limit | R::LimitOrFetch => "LIMIT n",
        R::FetchFirst => "FETCH FIRST n ROWS ONLY",
        R::TopOrFetch => "SELECT TOP (n) … or OFFSET 0 ROWS FETCH NEXT n ROWS ONLY",
        R::Unknown => "",
    };
    for used in a.row_limits.iter().filter(|u| !accepted.contains(u)) {
        out.push(Finding::new(
            "L009",
            Severity::Error,
            format!("this dialect has no {used}: limit rows with {wanted}"),
            None,
        ));
    }
    if a.ilike && !facts.has_ilike && facts.row_limit != R::Unknown {
        out.push(Finding::new(
            "L010",
            Severity::Error,
            "this dialect has no ILIKE: compare lower(col) LIKE lower('…'), or use the \
             dialect's case-insensitive form"
                .into(),
            None,
        ));
    }
    if facts.double_quote_is_string {
        let names = |s: &str| {
            schema.table(s).is_some() || schema.tables.iter().any(|t| t.column(s).is_some())
        };
        for s in a.double_quoted_strings.iter().filter(|s| names(s)) {
            out.push(Finding::new(
                "L011",
                Severity::Error,
                format!(
                    "\"{s}\" is a text value in this dialect, not the name {s}: quote names with \
                     backticks (`{s}`)"
                ),
                None,
            ));
        }
    }
}

/// L001: tables and columns that do not exist.
fn unknown_names(a: &Analysis, schema: &Schema, out: &mut Vec<Finding>) {
    for t in &a.unknown_tables {
        let near = nearest(t, schema.tables.iter().map(|t| t.name.as_str()));
        out.push(Finding::new(
            "L001",
            Severity::Error,
            format!("table {t} does not exist{}", did_you_mean(&near)),
            None,
        ));
    }
    for u in &a.unresolved {
        let shown = match &u.qualifier {
            Some(q) => format!("{q}.{}", u.column),
            None => u.column.clone(),
        };
        if u.double_quoted {
            out.push(Finding::new(
                "L001",
                Severity::Error,
                format!(
                    "\"{}\" is in double quotes but is no column; SQL reads double quotes as a \
                     name — write a text value in single quotes: '{}'",
                    u.column,
                    u.column.replace('\'', "''")
                ),
                None,
            ));
            continue;
        }
        let (near, fetch) = match &u.table {
            Some(t) => (
                nearest(
                    &u.column,
                    schema
                        .table(t)
                        .map(|t| {
                            t.columns
                                .iter()
                                .map(|c| c.name.as_str())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                        .into_iter(),
                ),
                Some(Fetch::DescribeTable { table: t.clone() }),
            ),
            None => {
                let all: Vec<String> = schema
                    .tables
                    .iter()
                    .flat_map(|t| {
                        t.columns
                            .iter()
                            .map(move |c| format!("{}.{}", t.name, c.name))
                    })
                    .collect();
                let near = nearest(&u.column, all.iter().map(String::as_str));
                (near, None)
            }
        };
        let place = match (&u.qualifier, &u.table) {
            (Some(_), Some(t)) => format!(" (table {t} has no such column)"),
            (Some(q), None) => format!(" ({q} is no table or alias in scope)"),
            _ => String::new(),
        };
        out.push(Finding::new(
            "L001",
            Severity::Error,
            format!(
                "column {shown} does not exist{place}{}",
                did_you_mean(&near)
            ),
            fetch,
        ));
    }
}

/// L002: a join on columns that are neither a declared foreign key nor a key on either side.
fn joins(a: &Analysis, schema: &Schema, out: &mut Vec<Finding>) {
    if schema.foreign_keys.is_empty() {
        return;
    }
    for (x, y) in &a.joins {
        if schema.is_foreign_key((&x.0, &x.1), (&y.0, &y.1)) {
            continue;
        }
        let keyed = |c: &(String, String)| {
            schema
                .column(&c.0, &c.1)
                .is_some_and(|col| col.primary_key || col.stats.as_ref().is_some_and(|s| s.unique))
        };
        if keyed(x) || keyed(y) {
            continue;
        }
        let known = schema
            .join_path(&x.0, &y.0)
            .filter(|p| !p.is_empty())
            .map(|p| format!("; the foreign keys join them by {}", p.join(" AND ")))
            .unwrap_or_default();
        out.push(Finding::new(
            "L002",
            Severity::Warn,
            format!(
                "join {}.{} = {}.{} is no foreign key and neither side is a key, so each row \
                 may match many{known}",
                x.0, x.1, y.0, y.1
            ),
            Some(Fetch::JoinPath {
                from: x.0.clone(),
                to: y.0.clone(),
            }),
        ));
    }
}

/// L003 (`=` on a list column) and L004 (a literal the column does not hold).
fn literals(a: &Analysis, schema: &Schema, out: &mut Vec<Finding>) {
    for cmp in &a.comparisons {
        // `= ''` is a test for a missing value, not a value spelling.
        if cmp.op == CmpOp::Range || cmp.literal.trim().is_empty() {
            continue;
        }
        let (t, c) = &cmp.col;
        let list = schema
            .column(t, c)
            .and_then(|col| col.stats.as_ref())
            .is_some_and(|s| s.list_valued);
        if list {
            out.push(Finding::new(
                "L003",
                Severity::Warn,
                format!(
                    "{t}.{c} holds comma-separated lists; = '{0}' matches only rows whose whole \
                     list is '{0}' — use {c} LIKE '%{0}%' to match one element",
                    cmp.literal.replace('\'', "''")
                ),
                None,
            ));
            continue;
        }
        let Some(values) = schema.column_values(t, c) else {
            continue;
        };
        if values.contains(&cmp.literal) {
            continue;
        }
        let lower = cmp.literal.to_lowercase();
        let mut near: Vec<String> = values
            .iter()
            .filter(|v| {
                let lv = v.to_lowercase();
                lv == lower || lv.contains(&lower) || (lv.len() >= 3 && lower.contains(&lv))
            })
            .take(5)
            .map(|v| format!("'{}'", v.replace('\'', "''")))
            .collect();
        let elsewhere: Vec<String> = schema
            .find_values(&cmp.literal, None)
            .into_iter()
            .filter(|h| !h.contains(&format!(" in {t}.{c}")))
            .take(3)
            .collect();
        let mut msg = format!(
            "{t}.{c} never holds '{}' ({} distinct values)",
            cmp.literal.replace('\'', "''"),
            values.len()
        );
        if !near.is_empty() {
            near.dedup();
            msg.push_str(&format!("; it holds {}", near.join(", ")));
        }
        if !elsewhere.is_empty() {
            msg.push_str(&format!("; the text occurs as {}", elsewhere.join("; ")));
        }
        out.push(Finding::new(
            "L004",
            Severity::Warn,
            msg,
            Some(Fetch::FindValues {
                text: cmp.literal.clone(),
                column: None,
            }),
        ));
    }
}

/// L005: integer division where the engine truncates.
fn divisions(a: &Analysis, facts: &DialectFacts, out: &mut Vec<Finding>) {
    if !facts.integer_division {
        return;
    }
    for (l, r, text) in &a.divisions {
        if *l == NumKind::Int && *r == NumKind::Int {
            out.push(Finding::new(
                "L005",
                Severity::Warn,
                format!(
                    "{text} divides two integers, which truncates (7 / 2 = 3); for a ratio or \
                     percentage multiply the numerator by 1.0 or cast it to a non-integer type"
                ),
                None,
            ));
        }
    }
}

/// L006: an aggregate over a column that uses a value for "unknown".
fn sentinels(a: &Analysis, schema: &Schema, out: &mut Vec<Finding>) {
    for (f, (t, c)) in &a.aggregates {
        if f == "max" || a.filtered.contains(&(t.clone(), c.clone())) {
            continue;
        }
        let Some(s) = schema.column(t, c).and_then(|col| col.stats.as_ref()) else {
            continue;
        };
        if let Some((v, n)) = s.sentinels.first() {
            out.push(Finding::new(
                "L006",
                Severity::Warn,
                format!(
                    "{}({t}.{c}) includes {n} rows holding {v}, which in this column likely \
                     means unknown; filter {c} <> {v} (or > 0) if they are not real values",
                    f.to_uppercase()
                ),
                None,
            ));
        }
    }
}

/// L007: `ORDER BY col LIMIT n` where missing values sort first.
fn null_order(a: &Analysis, facts: &DialectFacts, schema: &Schema, out: &mut Vec<Finding>) {
    if !facts.nulls_first_ascending {
        return;
    }
    for o in &a.order_limits {
        if !o.ascending || o.explicit_nulls || a.filtered.contains(&o.col) {
            continue;
        }
        let (t, c) = &o.col;
        let Some(col) = schema.column(t, c) else {
            continue;
        };
        if col.has_missing() {
            out.push(Finding::new(
                "L007",
                Severity::Warn,
                format!(
                    "ORDER BY {t}.{c} ascending puts its NULL or empty values first, so LIMIT \
                     {} returns them; add {c} IS NOT NULL (and {c} <> '' for text)",
                    o.limit
                ),
                None,
            ));
        }
    }
}

/// L008: a date literal written in another format than the column stores.
fn dates(a: &Analysis, facts: &DialectFacts, schema: &Schema, out: &mut Vec<Finding>) {
    if !facts.dates_are_text {
        return;
    }
    for cmp in &a.comparisons {
        let lit = shape(&cmp.literal);
        if !looks_like_date(&lit) {
            continue;
        }
        let (t, c) = &cmp.col;
        let Some(stats) = schema.column(t, c).and_then(|col| col.stats.as_ref()) else {
            continue;
        };
        let stored: Vec<String> = stats
            .examples
            .iter()
            .map(|e| shape(e))
            .filter(|s| looks_like_date(s))
            .collect();
        let Some(example) = stats.examples.iter().find(|e| looks_like_date(&shape(e))) else {
            continue;
        };
        let fits = stored.iter().any(|s| match cmp.op {
            CmpOp::Eq | CmpOp::In => *s == lit,
            CmpOp::Range => s.starts_with(&lit) || lit.starts_with(s.as_str()),
        });
        if !fits {
            out.push(Finding::new(
                "L008",
                Severity::Warn,
                format!(
                    "{t}.{c} stores dates as text like '{example}', but the query compares it \
                     with '{}'; text comparison needs the stored format (or use substr/strftime \
                     on the column)",
                    cmp.literal
                ),
                None,
            ));
        }
    }
}

/// Digits → `9`, letters → `a`: `'2017-01-21 00:00'` → `9999-99-99 99:99`.
fn shape(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_digit() {
                '9'
            } else if c.is_alphabetic() {
                'a'
            } else {
                c
            }
        })
        .collect()
}

fn looks_like_date(shape: &str) -> bool {
    [
        "9999-99",
        "9999/99",
        "99/99/9999",
        "99-99-9999",
        "99.99.9999",
    ]
    .iter()
    .any(|p| shape.starts_with(p))
}

/// Up to three names within a small edit distance, case-folded.
fn nearest<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Vec<String> {
    let target = name.to_lowercase();
    let short = |s: &str| s.rsplit('.').next().unwrap_or(s).to_lowercase();
    let mut scored: Vec<(usize, String)> = candidates
        .map(|c| (distance(&target, &short(c)), c.to_string()))
        .filter(|(d, c)| *d <= (target.len() / 3).max(2) || short(c).contains(&target))
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|s| s.1).collect()
}

fn did_you_mean(near: &[String]) -> String {
    if near.is_empty() {
        String::new()
    } else {
        format!("; did you mean {}?", near.join(", "))
    }
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::fixture;
    use sqlparser::dialect::SQLiteDialect;

    const SQLITE: DialectFacts = DialectFacts {
        integer_division: true,
        nulls_first_ascending: true,
        like_case_insensitive: true,
        has_ilike: false,
        dates_are_text: true,
        row_limit: ekos_sql_dialect_sdk::RowLimit::Limit,
        identifier_quote: Some('"'),
        double_quote_is_string: false,
        backtick_identifiers: true,
    };

    fn codes(sql: &str) -> Vec<String> {
        let (_, f) = lint(sql, &SQLiteDialect {}, &SQLITE, &fixture()).unwrap();
        f.into_iter().map(|f| f.code).collect()
    }

    fn messages(sql: &str) -> Vec<String> {
        let (_, f) = lint(sql, &SQLiteDialect {}, &SQLITE, &fixture()).unwrap();
        f.into_iter().map(|f| f.message).collect()
    }

    #[test]
    fn l001_unknown_names_with_suggestions() {
        let m = messages("SELECT c.nme FROM cards c JOIN rulngs r ON r.card_id = c.id");
        assert!(
            m.iter()
                .any(|m| m.contains("table rulngs") && m.contains("rulings"))
        );
        assert!(
            m.iter()
                .any(|m| m.contains("column c.nme") && m.contains("name"))
        );
        assert!(codes("SELECT name FROM cards").is_empty());
        let m = messages("SELECT id FROM cards WHERE name = \"Fireball\"");
        assert!(m[0].contains("'Fireball'"));
    }

    #[test]
    fn l002_join_off_the_keys() {
        assert!(codes("SELECT 1 FROM cards c JOIN rulings r ON r.card_id = c.id").is_empty());
        assert_eq!(
            codes("SELECT 1 FROM cards c JOIN rulings r ON r.text = c.name"),
            vec!["L002"]
        );
        assert!(codes("SELECT 1 FROM cards c JOIN rulings r ON r.id = c.power").is_empty());
    }

    #[test]
    fn l003_equals_on_a_list_column() {
        assert_eq!(
            codes("SELECT id FROM cards WHERE types = 'Creature'"),
            vec!["L003"]
        );
        assert!(codes("SELECT id FROM cards WHERE types LIKE '%Creature%'").is_empty());
    }

    #[test]
    fn l004_literal_not_held_offers_spellings_and_other_columns() {
        let m = messages("SELECT id FROM cards WHERE rarity = 'Rare'");
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("'rare'"), "{}", m[0]);
        assert!(m[0].contains("sets.name"), "{}", m[0]);
        assert!(codes("SELECT id FROM cards WHERE rarity = 'rare'").is_empty());
        assert!(codes("SELECT id FROM cards WHERE name = 'Anything'").is_empty());
        assert!(codes("SELECT id FROM cards WHERE rarity = '' OR rarity IN ('', ' ')").is_empty());
    }

    #[test]
    fn l005_integer_division_only_where_it_truncates() {
        assert_eq!(
            codes("SELECT SUM(CASE WHEN power > 2 THEN 1 ELSE 0 END) * 100 / COUNT(*) FROM cards"),
            vec!["L005"]
        );
        assert!(codes("SELECT CAST(COUNT(*) AS REAL) / COUNT(id) FROM cards").is_empty());
        assert_eq!(
            codes("SELECT COUNT(*) * 100 / (SELECT COUNT(*) FROM rulings) FROM cards"),
            vec!["L005"],
            "a scalar subquery has its column's kind"
        );
        let (_, f) = lint(
            "SELECT COUNT(*) / COUNT(id) FROM cards",
            &SQLiteDialect {},
            &DialectFacts::default(),
            &fixture(),
        )
        .unwrap();
        assert!(f.is_empty());
    }

    #[test]
    fn l006_aggregate_over_a_sentinel_column_unless_filtered() {
        assert_eq!(codes("SELECT AVG(height_cm) FROM cards"), vec!["L006"]);
        assert!(codes("SELECT AVG(height_cm) FROM cards WHERE height_cm > 0").is_empty());
        assert!(codes("SELECT AVG(power) FROM cards").is_empty());
    }

    #[test]
    fn l007_ascending_limit_on_a_nullable_column() {
        assert_eq!(
            codes("SELECT name FROM cards ORDER BY power LIMIT 1"),
            vec!["L007"]
        );
        assert!(codes("SELECT name FROM cards ORDER BY power DESC LIMIT 1").is_empty());
        assert!(
            codes("SELECT name FROM cards WHERE power IS NOT NULL ORDER BY power LIMIT 1")
                .is_empty()
        );
    }

    fn dialect_codes(sql: &str, facts: DialectFacts, dialect: &dyn Dialect) -> Vec<String> {
        let f = match lint(sql, dialect, &facts, &fixture()) {
            Ok((_, f)) | Err((_, f)) => f,
        };
        f.into_iter().map(|f| f.code).collect()
    }

    #[test]
    fn l009_row_limit_form_by_dialect() {
        use ekos_sql_dialect_sdk::RowLimit;
        use sqlparser::dialect::GenericDialect;
        let oracle = DialectFacts {
            row_limit: RowLimit::FetchFirst,
            identifier_quote: Some('"'),
            ..DialectFacts::GENERIC
        };
        assert_eq!(
            dialect_codes("SELECT name FROM cards LIMIT 5", oracle, &GenericDialect {}),
            vec!["L009"]
        );
        assert!(
            dialect_codes(
                "SELECT name FROM cards FETCH FIRST 5 ROWS ONLY",
                oracle,
                &GenericDialect {}
            )
            .is_empty()
        );
        let mssql = DialectFacts {
            row_limit: RowLimit::TopOrFetch,
            ..DialectFacts::GENERIC
        };
        assert!(
            dialect_codes("SELECT TOP 5 name FROM cards", mssql, &GenericDialect {}).is_empty()
        );
        assert_eq!(
            dialect_codes("SELECT TOP 5 name FROM cards", SQLITE, &SQLiteDialect {}),
            vec!["L009"]
        );
    }

    #[test]
    fn l010_ilike_and_l011_quoting() {
        use ekos_sql_dialect_sdk::RowLimit;
        use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect};
        let trino = DialectFacts {
            row_limit: RowLimit::LimitOrFetch,
            identifier_quote: Some('"'),
            ..DialectFacts::GENERIC
        };
        assert_eq!(
            dialect_codes(
                "SELECT name FROM cards WHERE name ILIKE 'a%'",
                trino,
                &GenericDialect {}
            ),
            vec!["L010"]
        );
        let pg = DialectFacts {
            has_ilike: true,
            ..trino
        };
        assert!(
            dialect_codes(
                "SELECT name FROM cards WHERE name ILIKE 'a%'",
                pg,
                &PostgreSqlDialect {}
            )
            .is_empty()
        );
        let codes = dialect_codes("SELECT `name` FROM cards", pg, &PostgreSqlDialect {});
        assert_eq!(
            codes,
            vec!["L011", "E001"],
            "backticks are reported, then the parse fails"
        );
        assert!(dialect_codes("SELECT 'a`b' FROM cards", pg, &PostgreSqlDialect {}).is_empty());
        let mysql = DialectFacts {
            row_limit: RowLimit::Limit,
            identifier_quote: Some('`'),
            double_quote_is_string: true,
            backtick_identifiers: true,
            ..DialectFacts::GENERIC
        };
        assert_eq!(
            dialect_codes("SELECT \"name\" FROM cards", mysql, &MySqlDialect {}),
            vec!["L011"]
        );
        assert!(
            dialect_codes(
                "SELECT id FROM cards WHERE rarity = \"rare\"",
                mysql,
                &MySqlDialect {}
            )
            .is_empty()
        );
    }

    #[test]
    fn l008_date_literal_in_another_format() {
        assert_eq!(
            codes("SELECT id FROM cards WHERE released = '2017/01/21'"),
            vec!["L008"]
        );
        assert!(codes("SELECT id FROM cards WHERE released >= '2017-01-01'").is_empty());
        assert_eq!(
            codes("SELECT id FROM cards WHERE released = '2017-01-21'"),
            vec!["L008"],
            "= needs the full stored format"
        );
    }
}
