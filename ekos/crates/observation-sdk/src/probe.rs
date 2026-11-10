//! RFC 0254 §3.5 — `ekos_probe`: aggregates-only exploration for agents, as a closed set of
//! templates. Pure: a [`Probe`] is checked against the table's columns ([`check`]) and rendered
//! per dialect ([`render`]); the caller runs it on a read-only session, then applies the PII and
//! small-cell rules to the answer ([`shape`]). An agent never writes SQL here: `where` is a closed
//! predicate vocabulary (column, operator, literal), type-checked against the column.

use crate::profile::{self, Family, Flavor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The templates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "template", rename_all = "snake_case")]
pub enum Probe {
    /// Top values with counts, null count, distinct count.
    Distribution {
        column: String,
        #[serde(default)]
        r#where: Vec<Cond>,
    },
    /// Counts per (x, y) pair, top first.
    Cooccurrence { x: String, y: String },
    /// Null rate of `column` per value of `by`.
    NullBy { column: String, by: String },
    /// RFC 0158's inclusion counts for one pair (child column of this table → parent).
    Orphans {
        column: String,
        parent_table: String,
        parent_column: String,
    },
    /// Counts per value per month or year.
    OverTime {
        column: String,
        time_column: String,
        #[serde(default = "month")]
        grain: String,
    },
    /// A row count.
    Count {
        #[serde(default)]
        r#where: Vec<Cond>,
    },
}

fn month() -> String {
    "month".into()
}

/// One condition of the closed predicate vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cond {
    pub column: String,
    /// `=`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `is_null`, `is_not_null`.
    pub op: String,
    #[serde(default)]
    pub value: Option<Value>,
}

/// No `!=`: its count is the total minus the `=` count, which undoes small-cell suppression.
const OPS: &[&str] = &["=", "<", "<=", ">", ">=", "in", "is_null", "is_not_null"];

/// A column as the catalog states it.
fn column<'a>(columns: &'a [Value], name: &str) -> Result<&'a Value, String> {
    columns
        .iter()
        .find(|c| {
            c["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| format!("no column {name:?} in this table"))
}

fn real_name(c: &Value) -> String {
    c["name"].as_str().unwrap_or("").to_string()
}

/// A literal checked against a column's family, rendered for `flavor`.
fn literal(flavor: Flavor, fam: Family, v: &Value) -> Result<String, String> {
    match (fam, v) {
        (Family::Number, Value::Number(n)) => Ok(n.to_string()),
        (Family::Number, Value::String(s)) if s.parse::<f64>().is_ok() => Ok(s.clone()),
        (Family::Text, Value::String(s)) => Ok(crate::signals::sql_string(flavor, s)),
        (Family::Text, Value::Number(n)) => Ok(crate::signals::sql_string(flavor, &n.to_string())),
        (Family::Date, Value::String(s))
            if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' =>
        {
            Ok(match flavor {
                Flavor::Oracle => format!("DATE '{}'", &s[..10]),
                _ => crate::signals::sql_string(flavor, &s[..10]),
            })
        }
        _ => Err(format!("{v} is not a value of a {fam:?} column")),
    }
}

fn condition(flavor: Flavor, columns: &[Value], c: &Cond) -> Result<String, String> {
    if !OPS.contains(&c.op.as_str()) {
        return Err(format!(
            "operator {:?} is not one of {}",
            c.op,
            OPS.join(", ")
        ));
    }
    let col = column(columns, &c.column)?;
    let fam = profile::family(flavor, col["data_type"].as_str().unwrap_or(""));
    if !matches!(fam, Family::Number | Family::Text | Family::Date) {
        return Err(format!("{} cannot be compared", c.column));
    }
    // A range over text lets a caller bisect a value prefix by prefix: equality only.
    if fam == Family::Text && matches!(c.op.as_str(), "<" | "<=" | ">" | ">=") {
        return Err(format!(
            "{} is text: only =, in, is_null and is_not_null",
            c.column
        ));
    }
    let id = profile::ident(flavor, &real_name(col))?;
    match c.op.as_str() {
        "is_null" => Ok(format!("{id} IS NULL")),
        "is_not_null" => Ok(format!("{id} IS NOT NULL")),
        "in" => {
            let vals = c
                .value
                .as_ref()
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty() && a.len() <= 50)
                .ok_or("`in` takes a list of 1 to 50 values")?;
            let lits = vals
                .iter()
                .map(|v| literal(flavor, fam, v))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{id} IN ({})", lits.join(", ")))
        }
        op => {
            let v = c.value.as_ref().ok_or("a comparison needs a value")?;
            Ok(format!("{id} {op} {}", literal(flavor, fam, v)?))
        }
    }
}

fn where_clause(flavor: Flavor, columns: &[Value], conds: &[Cond]) -> Result<String, String> {
    if conds.len() > 5 {
        return Err("at most 5 conditions".into());
    }
    let parts = conds
        .iter()
        .map(|c| condition(flavor, columns, c))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    })
}

fn count(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::ClickHouse => "count()",
        Flavor::SqlServer => "COUNT_BIG(*)",
        _ => "COUNT(*)",
    }
}

fn as_text(flavor: Flavor, col: &str) -> String {
    match flavor {
        Flavor::Oracle => format!("TO_CHAR({col})"),
        Flavor::Postgres => format!("{col}::text"),
        Flavor::ClickHouse => format!("toString({col})"),
        Flavor::SqlServer => format!("CAST({col} AS nvarchar(4000))"),
        Flavor::Informix => format!("CAST({col} AS LVARCHAR(4000))"),
    }
}

fn bucket(flavor: Flavor, col: &str, grain: &str) -> Result<String, String> {
    let fmt = match grain {
        "month" => "month",
        "year" => "year",
        g => return Err(format!("grain {g:?}: month or year")),
    };
    Ok(match (flavor, fmt) {
        (Flavor::Postgres, "month") => format!("to_char({col}, 'YYYY-MM')"),
        (Flavor::Postgres, _) => format!("to_char({col}, 'YYYY')"),
        (Flavor::Oracle, "month") => format!("TO_CHAR({col}, 'YYYY-MM')"),
        (Flavor::Oracle, _) => format!("TO_CHAR({col}, 'YYYY')"),
        (Flavor::ClickHouse, "month") => format!("formatDateTime({col}, '%Y-%m')"),
        (Flavor::ClickHouse, _) => format!("formatDateTime({col}, '%Y')"),
        (Flavor::SqlServer, "month") => format!("CONVERT(char(7), {col}, 126)"),
        (Flavor::SqlServer, _) => format!("CONVERT(char(4), {col}, 126)"),
        (Flavor::Informix, "month") => format!("TO_CHAR({col}, '%Y-%m')"),
        (Flavor::Informix, _) => format!("TO_CHAR({col}, '%Y')"),
    })
}

/// Every column a probe filters on (a personal one is refused: filtering on it reveals values).
pub fn filter_columns(p: &Probe) -> Vec<&str> {
    match p {
        Probe::Distribution { r#where, .. } | Probe::Count { r#where } => {
            r#where.iter().map(|c| c.column.as_str()).collect()
        }
        _ => Vec::new(),
    }
}

/// The probe with every `where` literal replaced by a placeholder: what the audit keeps.
pub fn audit_form(p: &Probe) -> Probe {
    let mut q = p.clone();
    if let Probe::Distribution { r#where, .. } | Probe::Count { r#where } = &mut q {
        for c in r#where.iter_mut() {
            if c.value.is_some() {
                c.value = Some(json!("<literal>"));
            }
        }
    }
    q
}

/// The columns whose values a probe would return (for the PII rule).
pub fn value_columns(p: &Probe) -> Vec<&str> {
    match p {
        Probe::Distribution { column, .. } => vec![column],
        Probe::Cooccurrence { x, y } => vec![x, y],
        Probe::NullBy { by, .. } => vec![by],
        Probe::OverTime { column, .. } => vec![column],
        Probe::Orphans { .. } | Probe::Count { .. } => vec![],
    }
}

/// The SQL of a probe over `owner.table` (whose catalog columns are `columns`); `parent_owner` and
/// `parent_columns` only for `orphans`. Every identifier is checked against the catalog.
pub fn render(
    flavor: Flavor,
    owner: &str,
    table: &str,
    columns: &[Value],
    p: &Probe,
    parent: Option<(&str, &str, &[Value])>,
) -> Result<String, String> {
    let tbl = profile::table_ref(flavor, owner, table)?;
    let col = |n: &str| -> Result<String, String> {
        profile::ident(flavor, &real_name(column(columns, n)?))
    };
    let n = count(flavor);
    Ok(match p {
        Probe::Count { r#where } => format!(
            "SELECT {n} FROM {tbl}{}",
            where_clause(flavor, columns, r#where)?
        ),
        Probe::Distribution { column: c, r#where } => {
            let c = col(c)?;
            let t = as_text(flavor, &c);
            format!(
                "SELECT {t}, {n} FROM {tbl}{} GROUP BY {t} ORDER BY {n} DESC",
                where_clause(flavor, columns, r#where)?
            )
        }
        Probe::Cooccurrence { x, y } => {
            let (x, y) = (as_text(flavor, &col(x)?), as_text(flavor, &col(y)?));
            format!("SELECT {x}, {y}, {n} FROM {tbl} GROUP BY {x}, {y} ORDER BY {n} DESC")
        }
        Probe::NullBy { column: c, by } => {
            let (c, b) = (col(c)?, as_text(flavor, &col(by)?));
            format!(
                "SELECT {b}, {n}, COUNT(CASE WHEN {c} IS NULL THEN 1 END) FROM {tbl} GROUP BY {b} ORDER BY {n} DESC"
            )
        }
        Probe::OverTime {
            column: c,
            time_column,
            grain,
        } => {
            let tc = column(columns, time_column)?;
            if profile::family(flavor, tc["data_type"].as_str().unwrap_or("")) != Family::Date {
                return Err(format!("{time_column} is not a date or time column"));
            }
            let b = bucket(flavor, &profile::ident(flavor, &real_name(tc))?, grain)?;
            let c = as_text(flavor, &col(c)?);
            format!("SELECT {b}, {c}, {n} FROM {tbl} GROUP BY {b}, {c} ORDER BY 1, {n} DESC")
        }
        Probe::Orphans {
            column: c,
            parent_table,
            parent_column,
        } => {
            let (powner, ptable, pcols) = parent.ok_or("orphans needs the parent table")?;
            if !ptable.eq_ignore_ascii_case(parent_table) {
                return Err("the parent table does not match".into());
            }
            let pc = real_name(column(pcols, parent_column)?);
            let cc = real_name(column(columns, c)?);
            crate::overlap::inclusion_sql(flavor, (owner, table, &cc), (powner, ptable, &pc))?
        }
    })
}

/// The answer as the caller may show it: a personal column's values withheld (counts only), a
/// value held by fewer than `min_cell` rows shown as `<min_cell` without it, every value redacted.
/// `value_cells` is how many leading cells of a row are values (the rest are counts).
pub fn shape(
    rows: &[Vec<Option<String>>],
    value_cells: usize,
    personal: &[bool],
    min_cell: i64,
    redaction: &ekos_common::redaction::RedactionConfig,
) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let n = r
                .get(value_cells)
                .cloned()
                .flatten()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(0.0) as i64;
            let small = value_cells > 0 && n < min_cell;
            let values: Vec<Value> = (0..value_cells)
                .map(|i| {
                    if small {
                        json!(format!("<{min_cell} rows"))
                    } else if personal.get(i).copied().unwrap_or(false) {
                        json!("(personal: withheld)")
                    } else {
                        r.get(i)
                            .cloned()
                            .flatten()
                            .map(|v| json!(ekos_common::redaction::redact(&v, redaction)))
                            .unwrap_or(Value::Null)
                    }
                })
                .collect();
            let counts: Vec<Value> = r[value_cells.min(r.len())..]
                .iter()
                .map(|c| {
                    c.as_deref()
                        .and_then(|v| v.trim().parse::<f64>().ok())
                        .map_or(Value::Null, |f| {
                            // With a filter, an exact small count says whether a row exists.
                            let n = f as i64;
                            // 0 too: "none" against "a few" answers a yes/no question.
                            if n < min_cell {
                                json!(format!("<{min_cell}"))
                            } else {
                                json!(n)
                            }
                        })
                })
                .collect();
            json!({"values": values, "counts": counts})
        })
        .collect()
}

/// How many leading cells of a probe's rows are values.
pub fn value_cells(p: &Probe) -> usize {
    match p {
        Probe::Count { .. } | Probe::Orphans { .. } => 0,
        Probe::Distribution { .. } | Probe::NullBy { .. } => 1,
        Probe::Cooccurrence { .. } | Probe::OverTime { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<Value> {
        serde_json::from_value(json!([
            {"name": "status", "data_type": "integer"},
            {"name": "email", "data_type": "text"},
            {"name": "created_at", "data_type": "timestamp"},
            {"name": "note", "data_type": "text"},
        ]))
        .unwrap()
    }

    fn p(v: Value) -> Probe {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_where_that_does_not_type_check_is_refused() {
        let bad = p(
            json!({"template": "count", "where": [{"column": "status", "op": "=", "value": "seven"}]}),
        );
        assert!(render(Flavor::Postgres, "s", "t", &cols(), &bad, None).is_err());
        let sql_text = p(
            json!({"template": "count", "where": [{"column": "status", "op": "; drop", "value": 1}]}),
        );
        assert!(render(Flavor::Postgres, "s", "t", &cols(), &sql_text, None).is_err());
        let unknown = p(json!({"template": "distribution", "column": "nope"}));
        assert!(render(Flavor::Postgres, "s", "t", &cols(), &unknown, None).is_err());
        let ok = p(
            json!({"template": "count", "where": [{"column": "status", "op": "in", "value": [1, 2]}]}),
        );
        assert_eq!(
            render(Flavor::Postgres, "s", "t", &cols(), &ok, None).unwrap(),
            "SELECT COUNT(*) FROM \"s\".\"t\" WHERE \"status\" IN (1, 2)"
        );
    }

    #[test]
    fn a_complement_and_a_zero_reveal_nothing() {
        let complement = p(
            json!({"template": "count", "where": [{"column": "status", "op": "!=", "value": 1}]}),
        );
        assert!(render(Flavor::Postgres, "s", "t", &cols(), &complement, None).is_err());
        let none = shape(&[vec![Some("0".into())]], 0, &[], 5, &Default::default());
        assert_eq!(none[0]["counts"][0], "<5", "zero and a few look the same");
    }

    #[test]
    fn a_text_literal_is_quoted() {
        let q = p(
            json!({"template": "count", "where": [{"column": "note", "op": "=", "value": "x' OR '1'='1"}]}),
        );
        let sql = render(Flavor::Postgres, "s", "t", &cols(), &q, None).unwrap();
        assert!(sql.ends_with("\"note\" = 'x'' OR ''1''=''1'"), "{sql}");
    }

    #[test]
    fn over_time_needs_a_date_column() {
        let q = p(json!({"template": "over_time", "column": "status", "time_column": "note"}));
        assert!(render(Flavor::Postgres, "s", "t", &cols(), &q, None).is_err());
        let q = p(
            json!({"template": "over_time", "column": "status", "time_column": "created_at", "grain": "year"}),
        );
        assert!(
            render(Flavor::Oracle, "S", "T", &cols(), &q, None)
                .unwrap()
                .contains("'YYYY'")
        );
    }

    #[test]
    fn personal_values_are_withheld_and_small_cells_hidden() {
        let rows = vec![
            vec![Some("a@x.org".into()), Some("12".into())],
            vec![Some("b@y.org".into()), Some("2".into())],
        ];
        let v = shape(&rows, 1, &[true], 5, &Default::default());
        assert_eq!(v[0]["values"][0], "(personal: withheld)");
        assert_eq!(v[0]["counts"][0], 12);
        assert_eq!(v[1]["values"][0], "<5 rows");
        let v = shape(&rows, 1, &[false], 5, &Default::default());
        assert_eq!(v[0]["values"][0], "a@x.org");
        let one = shape(&[vec![Some("1".into())]], 0, &[], 5, &Default::default());
        assert_eq!(
            one[0]["counts"][0], "<5",
            "a filtered count of one row is hidden"
        );
    }
}
