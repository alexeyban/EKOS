//! RFC 0252 §3.3/§3.4 — value-domain overlap between columns, without moving a value out of the
//! database.
//!
//! A column's **signature** is a MinHash computed by the database itself: for each of
//! [`SEEDS`] salted MD5 digests of every distinct value (rendered as text), the minimum of each
//! of its four 32-bit slices — [`K`] minimums in all — plus the distinct count. Only those
//! minimums leave the database; the salt is the project's own (never in the ledger), so a
//! signature cannot be matched against a dictionary of known values by anyone without the salt.
//! MD5 is used because every engine has it under the same definition, so signatures from two
//! databases of one project compare (§3.4).
//!
//! Two signatures give an estimated Jaccard similarity and, with the distinct counts, an estimated
//! containment of the child's values in the parent's. LSH banding ([`bands`]) proposes pairs so
//! the search is not quadratic in columns. A proposal within one database is then verified exactly
//! by [`inclusion_sql`] (RFC 0158's inclusion check, rendered per dialect).

use crate::profile::{self, Family, Flavor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Salted digests per value.
pub const SEEDS: usize = 4;
/// Minimums per signature: four 32-bit slices of each digest.
pub const K: usize = SEEDS * 4;
/// Rows per LSH band (`K / ROWS_PER_BAND` bands).
pub const ROWS_PER_BAND: usize = 2;

/// A column's MinHash signature and distinct count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub distinct: i64,
    /// `K` lowercase 8-digit hex minimums; empty when the column holds no value.
    pub mins: Vec<String>,
    /// For a whole-number column, `max - min + 1`: the width of the range its values span (never
    /// a bound). A column whose distinct count fills most of its span is a dense sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<i64>,
}

impl Signature {
    /// Most of the span is used: a surrogate-key sequence. Containment in such a column is
    /// uninformative — any small whole number falls inside it.
    pub fn dense(&self) -> bool {
        self.span
            .is_some_and(|s| s > 0 && self.distinct as f64 >= 0.8 * s as f64)
    }
}

fn md5_hex(flavor: Flavor, expr: &str) -> String {
    match flavor {
        Flavor::Postgres => format!("md5({expr})"),
        Flavor::ClickHouse => format!("lower(hex(MD5({expr})))"),
        Flavor::Oracle => format!("LOWER(RAWTOHEX(STANDARD_HASH({expr}, 'MD5')))"),
    }
}

fn substr(flavor: Flavor, expr: &str, from: usize, len: usize) -> String {
    match flavor {
        Flavor::ClickHouse => format!("substring({expr}, {from}, {len})"),
        Flavor::Oracle => format!("SUBSTR({expr}, {from}, {len})"),
        Flavor::Postgres => format!("substr({expr}, {from}, {len})"),
    }
}

fn concat(flavor: Flavor, parts: &[String]) -> String {
    match flavor {
        Flavor::ClickHouse => format!("concat({})", parts.join(", ")),
        _ => parts.join(" || "),
    }
}

fn as_text(flavor: Flavor, col: &str) -> String {
    match flavor {
        Flavor::Oracle => format!("TO_CHAR({col})"),
        Flavor::Postgres => format!("{col}::text"),
        Flavor::ClickHouse => format!("toString({col})"),
    }
}

/// Whether a column's values can be compared as text across engines: whole numbers and text.
/// Dates and decimals render differently per engine.
pub fn comparable(flavor: Flavor, data_type: &str) -> bool {
    let t = data_type.to_ascii_lowercase();
    match profile::family(flavor, data_type) {
        Family::Text => true,
        Family::Number => {
            !(t.contains("numeric")
                || t.contains("decimal")
                || t.contains("float")
                || t.contains("double")
                || t.contains("real")
                || t.contains("money")
                || (t.starts_with("number(") && t.contains(',') && !t.ends_with(",0)")))
        }
        _ => false,
    }
}

/// `SELECT distinct, min₁ … min_K` for one column. `salt` must be the project's; it is embedded
/// as a literal and never stored with the signature.
pub fn signature_sql(
    flavor: Flavor,
    owner: &str,
    table: &str,
    column: &str,
    salt: &str,
    numeric: bool,
) -> Result<String, String> {
    if salt.is_empty() || !salt.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("the salt must be non-empty ASCII letters and digits".into());
    }
    let tbl = profile::table_ref(flavor, owner, table)?;
    let col = profile::ident(flavor, column)?;
    let txt = as_text(flavor, &col);
    let digests: Vec<String> = (0..SEEDS)
        .map(|s| {
            format!(
                "{} AS h{s}",
                md5_hex(
                    flavor,
                    &concat(flavor, &[format!("'{salt}:{s}:'"), "v".into()])
                )
            )
        })
        .collect();
    let mins: Vec<String> = (0..SEEDS)
        .flat_map(|s| (0..4).map(move |q| (s, q)))
        .map(|(s, q)| format!("MIN({})", substr(flavor, &format!("h{s}"), 1 + q * 8, 8)))
        .collect();
    // The span is the only number derived from the values themselves, and it is a width.
    let span = if numeric {
        "MAX(n) - MIN(n) + 1"
    } else {
        "NULL"
    };
    Ok(format!(
        "SELECT COUNT(*), {span}, {} FROM (SELECT n, {} FROM (SELECT DISTINCT {txt} AS v, {col} AS n \
         FROM {tbl} WHERE {col} IS NOT NULL) d) x",
        mins.join(", "),
        digests.join(", ")
    ))
}

/// The answer of [`signature_sql`]: `distinct, span, min₁ … min_K`.
pub fn parse_signature(row: &[Option<String>]) -> Option<Signature> {
    let distinct = row.first()?.as_deref()?.trim().parse::<f64>().ok()? as i64;
    let span = row
        .get(1)?
        .as_deref()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map(|v| v as i64);
    let mins: Vec<String> = row[2.min(row.len())..]
        .iter()
        .map(|c| c.as_deref().unwrap_or("").trim().to_ascii_lowercase())
        .collect();
    if distinct == 0 {
        return Some(Signature {
            distinct,
            mins: Vec::new(),
            span,
        });
    }
    (mins.len() == K && mins.iter().all(|m| m.len() == 8)).then_some(Signature {
        distinct,
        mins,
        span,
    })
}

/// Estimated Jaccard similarity of two value sets.
pub fn jaccard(a: &Signature, b: &Signature) -> f64 {
    if a.mins.len() != K || b.mins.len() != K {
        return 0.0;
    }
    a.mins.iter().zip(&b.mins).filter(|(x, y)| x == y).count() as f64 / K as f64
}

/// Estimated share of `child`'s distinct values found among `parent`'s.
pub fn containment(child: &Signature, parent: &Signature) -> f64 {
    let j = jaccard(child, parent);
    if child.distinct <= 0 || j == 0.0 {
        return 0.0;
    }
    let inter = j * (child.distinct + parent.distinct) as f64 / (1.0 + j);
    (inter / child.distinct as f64).min(1.0)
}

/// The LSH band keys of a signature: equal keys in any band make a pair worth estimating.
pub fn bands(sig: &Signature) -> Vec<String> {
    sig.mins
        .chunks(ROWS_PER_BAND)
        .enumerate()
        .map(|(i, c)| format!("{i}:{}", c.join("")))
        .collect()
}

/// The exact inclusion check for one pair in one database: `population, orphans` — non-null child
/// values, and those with no parent row.
pub fn inclusion_sql(
    flavor: Flavor,
    child: (&str, &str, &str),
    parent: (&str, &str, &str),
) -> Result<String, String> {
    let ct = profile::table_ref(flavor, child.0, child.1)?;
    let cc = profile::ident(flavor, child.2)?;
    let pt = profile::table_ref(flavor, parent.0, parent.1)?;
    let pc = profile::ident(flavor, parent.2)?;
    let (cnt, settings) = match flavor {
        // A LEFT JOIN fills a missing row with the type's default in ClickHouse unless told not to.
        Flavor::ClickHouse => ("count()", " SETTINGS join_use_nulls = 1"),
        _ => ("COUNT(*)", ""),
    };
    // Both sides as text: a whole-number column may reference a text code (and engines refuse
    // to compare the two types), and text is how the signatures compared them.
    let (ck, pk) = (as_text(flavor, &format!("c.{cc}")), as_text(flavor, &pc));
    Ok(format!(
        "SELECT {cnt}, SUM(CASE WHEN p.k IS NULL THEN 1 ELSE 0 END) FROM {ct} c \
         LEFT JOIN (SELECT DISTINCT {pk} AS k FROM {pt}) p ON {ck} = p.k \
         WHERE c.{cc} IS NOT NULL{settings}"
    ))
}

/// Names too common to say anything when they are equal (`id` = `id`).
const GENERIC: &[&str] = &[
    "id",
    "code",
    "name",
    "key",
    "no",
    "nr",
    "num",
    "number",
    "type",
    "status",
    "value",
    "description",
    "uuid",
    "guid",
    "label",
    "class",
    "kind",
];

/// Whether two column names suggest a reference (a tie-breaker only, never a seed): the same
/// specific name (`cust_no` = `cust_no`), or the child named after the parent's table
/// (`customer_id`, `customer` → `customer.id`, `customers.id`).
pub fn name_affinity(child_column: &str, parent_table: &str, parent_column: &str) -> bool {
    let c = child_column.to_ascii_lowercase();
    let pt = parent_table.to_ascii_lowercase();
    let pc = parent_column.to_ascii_lowercase();
    if c == pc && !GENERIC.contains(&c.as_str()) {
        return true;
    }
    let stem = ["_id", "_no", "_ref", "_code", "_key", "id"]
        .iter()
        .find_map(|suf| c.strip_suffix(suf))
        .unwrap_or(&c)
        .trim_end_matches('_');
    !stem.is_empty() && (stem == pt || format!("{stem}s") == pt || stem == pt.trim_end_matches('s'))
}

/// Columns of a table worth a signature: comparable type, not personal, not long text.
pub fn signature_columns(flavor: Flavor, columns: &[Value]) -> Vec<String> {
    columns
        .iter()
        .filter_map(|c| {
            let name = c["name"].as_str()?;
            let ty = c["data_type"].as_str().unwrap_or("");
            if !comparable(flavor, ty) || ekos_common::pii::classify_name(name).is_some() {
                return None;
            }
            let p = &c["profile"];
            if p["max_length"].as_i64().is_some_and(|l| l > 64) {
                return None;
            }
            if ty.to_ascii_lowercase().contains("bool") {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(d: i64, mins: &[&str]) -> Signature {
        let mut m: Vec<String> = mins.iter().map(|s| s.to_string()).collect();
        m.resize(K, "ffffffff".into());
        Signature {
            distinct: d,
            mins: m,
            span: None,
        }
    }

    #[test]
    fn signature_sql_never_returns_a_value_in_any_dialect() {
        for f in [Flavor::Postgres, Flavor::Oracle, Flavor::ClickHouse] {
            let sql = signature_sql(f, "s", "t", "c", "Ab12", true).unwrap();
            // The outer SELECT is aggregates only: COUNT, the span and MIN over digest slices.
            let outer = sql.split(" FROM (").next().unwrap();
            assert!(
                outer.starts_with("SELECT COUNT(*), MAX(n) - MIN(n) + 1, MIN("),
                "{outer}"
            );
            assert_eq!(outer.matches("MIN(").count(), K + 1, "{outer}");
            assert!(
                !outer.contains('"') && !outer.contains('`'),
                "no column in the outer select: {outer}"
            );
        }
    }

    #[test]
    fn a_salt_that_could_break_out_is_refused() {
        assert!(signature_sql(Flavor::Postgres, "s", "t", "c", "x'; drop", false).is_err());
        assert!(signature_sql(Flavor::Postgres, "s", "t", "c", "", false).is_err());
    }

    #[test]
    fn containment_of_a_subset_is_high_and_of_a_stranger_zero() {
        let parent = sig(100, &["00000001", "00000002", "00000003", "00000004"]);
        // Every min equal: Jaccard 1 → containment 1.
        assert_eq!(containment(&parent.clone(), &parent), 1.0);
        let stranger = Signature {
            distinct: 50,
            mins: vec!["aaaaaaaa".into(); K],
            span: None,
        };
        assert_eq!(containment(&stranger, &parent), 0.0);
        // A child half the size, Jaccard 0.5 → |∩| = 0.5·150/1.5 = 50 → containment 1.
        let mut half = parent.clone();
        half.distinct = 50;
        for m in half.mins.iter_mut().take(K / 2) {
            *m = "bbbbbbbb".into();
        }
        assert!((containment(&half, &parent) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn equal_band_keys_propose_a_pair() {
        let a = sig(10, &["00000001", "00000002"]);
        let mut b = a.clone();
        b.mins[5] = "99999999".into();
        let shared = bands(&a).iter().filter(|k| bands(&b).contains(k)).count();
        assert_eq!(shared, K / ROWS_PER_BAND - 1);
    }

    #[test]
    fn parse_refuses_a_partial_answer() {
        let mut row = vec![Some("3".to_string()), Some("3".to_string())];
        row.extend((0..K).map(|_| Some("0a0b0c0d".to_string())));
        assert_eq!(parse_signature(&row).unwrap().mins.len(), K);
        row.pop();
        assert!(parse_signature(&row).is_none());
        assert_eq!(
            parse_signature(&[Some("0".into()), None])
                .unwrap()
                .mins
                .len(),
            0
        );
    }

    #[test]
    fn inclusion_sql_counts_orphans_with_a_left_join() {
        let sql = inclusion_sql(
            Flavor::ClickHouse,
            ("db", "invoice", "cust_ref"),
            ("db", "customer", "id"),
        )
        .unwrap();
        assert!(sql.ends_with("SETTINGS join_use_nulls = 1"), "{sql}");
        let sql = inclusion_sql(
            Flavor::Oracle,
            ("S", "INVOICE", "CUST_REF"),
            ("S", "CUSTOMER", "ID"),
        )
        .unwrap();
        assert!(
            sql.contains(
                "LEFT JOIN (SELECT DISTINCT TO_CHAR(\"ID\") AS k FROM \"S\".\"CUSTOMER\") p"
            ),
            "{sql}"
        );
    }

    #[test]
    fn a_sequence_filling_its_span_is_dense() {
        let mut s = sig(100, &[]);
        s.span = Some(110);
        assert!(s.dense());
        s.span = Some(10_000);
        assert!(!s.dense());
    }

    #[test]
    fn decimals_and_dates_are_not_compared_across_engines() {
        assert!(comparable(Flavor::Postgres, "integer"));
        assert!(comparable(Flavor::Postgres, "character varying(20)"));
        assert!(!comparable(Flavor::Postgres, "numeric(12,2)"));
        assert!(!comparable(Flavor::Postgres, "date"));
        assert!(comparable(Flavor::Oracle, "NUMBER(10,0)"));
        assert!(!comparable(Flavor::Oracle, "NUMBER(12,2)"));
    }

    #[test]
    fn name_affinity_is_a_tie_breaker() {
        assert!(name_affinity("customer_id", "customer", "id"));
        assert!(name_affinity("customer", "customers", "id"));
        assert!(name_affinity("cust_no", "x", "cust_no"));
        assert!(
            !name_affinity("id", "x", "id"),
            "a generic name says nothing"
        );
        assert!(
            !name_affinity("entity_credit_account", "entity", "id"),
            "a prefix is not a name"
        );
        assert!(name_affinity(
            "entity_credit_account",
            "entity_credit_account",
            "id"
        ));
        assert!(!name_affinity("amount", "customer", "id"));
    }
}
