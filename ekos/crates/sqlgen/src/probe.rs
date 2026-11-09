//! Findings that need the database: the query's own result, and cheap side queries that test a
//! suspicion (a value's spelling, rows repeated by a join, a tie at the top). Every query a
//! probe runs is read-only and derived from the checked query or the schema — never from an
//! expected answer.

use crate::analyze::{Analysis, CmpOp};
use crate::lint::{Fetch, Finding, Severity};
use crate::schema::Schema;
use crate::{quote_ident, quote_literal};
use sqlparser::ast::{SelectItem, SetExpr};

/// Rows a probe shows of the checked query.
pub const PREVIEW_ROWS: usize = 20;

pub use ekos_sql_dialect_sdk::probe::{Preview, Probe};

/// Every probe code, in order.
pub const PROBE_CODES: &[&str] = &["E000", "P001", "P002", "P003", "P004", "P005"];

/// Runs the query and the side probes. `E000` (the query fails) ends the list.
pub fn probe(
    sql: &str,
    analysis: Option<&Analysis>,
    schema: &Schema,
    db: &dyn Probe,
) -> (Option<Preview>, Vec<Finding>) {
    let mut out = Vec::new();
    let preview = match db.preview(sql, PREVIEW_ROWS) {
        Ok(p) => p,
        Err(e) => {
            out.push(Finding {
                code: "E000".into(),
                severity: Severity::Error,
                message: format!("Running that query on the database failed: {e}"),
                fetch: None,
            });
            return (None, out);
        }
    };
    if preview.rows.is_empty() {
        out.push(Finding {
            code: "P001".into(),
            severity: Severity::Warn,
            message: "the query returns no rows; if the question expects rows, a filter, join or \
                      value spelling is likely wrong"
                .into(),
            fetch: None,
        });
    } else if preview.rows.len() == 1 && preview.rows[0].iter().all(Option::is_none) {
        out.push(Finding {
            code: "P002".into(),
            severity: Severity::Warn,
            message: "the query returns a single NULL; the rows it aggregates are likely \
                      filtered out or the column is empty for them"
                .into(),
            fetch: None,
        });
    }
    if let Some(a) = analysis {
        spellings(a, schema, db, &mut out);
        fan_out(a, schema, db, &mut out);
        ties(a, db, &mut out);
    }
    (Some(preview), out)
}

/// P003: `col = 'lit'` matches nothing, while a case-insensitive or containing match does.
fn spellings(a: &Analysis, schema: &Schema, db: &dyn Probe, out: &mut Vec<Finding>) {
    let mut seen = std::collections::BTreeSet::new();
    for cmp in &a.comparisons {
        if cmp.op == CmpOp::Range
            || cmp.literal.trim().is_empty()
            || !seen.insert((cmp.col.clone(), cmp.literal.clone()))
        {
            continue;
        }
        let (t, c) = &cmp.col;
        if schema.column_values(t, c).is_some() {
            continue; // L004 already knows every value of this column
        }
        // The table as the query names it (schema-qualified where it was), else quoted.
        let qt = a
            .written
            .get(&t.to_lowercase())
            .cloned()
            .unwrap_or_else(|| quote_ident(t));
        let (qc, lit) = (quote_ident(c), quote_literal(&cmp.literal));
        let like = quote_literal(&format!("%{}%", cmp.literal));
        let count = |cond: String| format!("SUM(CASE WHEN {cond} THEN 1 ELSE 0 END)");
        let sql = format!(
            "SELECT {}, {}, {} FROM {qt}",
            count(format!("{qc} = {lit}")),
            count(format!("LOWER({qc}) = LOWER({lit})")),
            count(format!("{qc} LIKE {like}"))
        );
        let Some(counts) = db.preview(&sql, 1).ok().and_then(|p| p.single_number()) else {
            continue;
        };
        let [exact, folded, contains] = counts[..] else {
            continue;
        };
        if exact > 0 || (folded == 0 && contains == 0) {
            continue;
        }
        let sample = db
            .preview(
                &format!("SELECT DISTINCT {qc} FROM {qt} WHERE {qc} LIKE {like}"),
                5,
            )
            .map(|p| {
                p.rows
                    .iter()
                    .filter_map(|r| r.first().cloned().flatten())
                    .map(|v| quote_literal(&v))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push(Finding {
            code: "P003".into(),
            severity: Severity::Warn,
            message: format!(
                "{t}.{c} = {lit} matches no row, but {folded} rows match ignoring case and \
                 {contains} contain it (e.g. {sample}); match the stored spelling, or use LIKE \
                 if the value is part of a longer text"
            ),
            fetch: Some(Fetch::FindValues {
                text: cmp.literal.clone(),
                column: Some(format!("{t}.{c}")),
            }),
        });
    }
}

/// P004: an aggregate over a join that repeats rows of the first table.
fn fan_out(a: &Analysis, schema: &Schema, db: &dyn Probe, out: &mut Vec<Finding>) {
    let top = &a.top;
    if !top.aggregate || !top.joined || top.distinct || top.group_by || top.count_distinct {
        return;
    }
    let (Some((written, table)), Some(q)) = (&top.first, &a.query) else {
        return;
    };
    let Some(pk) = schema.table(table).and_then(|t| t.single_pk()) else {
        return;
    };
    let SetExpr::Select(s) = q.body.as_ref() else {
        return;
    };
    let from: Vec<String> = s.from.iter().map(|f| f.to_string()).collect();
    let mut sql = String::new();
    if let Some(with) = &q.with {
        sql.push_str(&format!("{with} "));
    }
    sql.push_str(&format!(
        "SELECT COUNT(*), COUNT(DISTINCT {}.{}) FROM {}",
        written,
        quote_ident(&pk.name),
        from.join(", ")
    ));
    if let Some(w) = &s.selection {
        sql.push_str(&format!(" WHERE {w}"));
    }
    let Some(counts) = db.preview(&sql, 1).ok().and_then(|p| p.single_number()) else {
        return;
    };
    let [rows, entities] = counts[..] else {
        return;
    };
    if rows > entities && entities > 0 {
        out.push(Finding {
            code: "P004".into(),
            severity: Severity::Warn,
            message: format!(
                "the joins give {rows} rows for {entities} distinct {table} rows, so COUNT/SUM/AVG \
                 counts each {table} row several times; if the question is about {table}, use \
                 COUNT(DISTINCT …) or aggregate before joining"
            ),
            fetch: None,
        });
    }
}

/// P005: `ORDER BY … LIMIT 1` where the first two rows tie on the sort key.
fn ties(a: &Analysis, db: &dyn Probe, out: &mut Vec<Finding>) {
    let Some(q) = &a.query else {
        return;
    };
    if a.top.limit != Some(1) || !a.top.has_order || a.top.aggregate {
        return;
    }
    let Some(order) = &q.order_by else {
        return;
    };
    // No row limit of its own (its form differs by dialect): the probe reads the first two rows.
    let mut probe = (**q).clone();
    probe.limit = None;
    probe.fetch = None;
    if let SetExpr::Select(s) = probe.body.as_mut() {
        s.projection = order
            .exprs
            .iter()
            .map(|o| SelectItem::UnnamedExpr(o.expr.clone()))
            .collect();
    } else {
        return;
    }
    let Ok(p) = db.preview(&probe.to_string(), 2) else {
        return;
    };
    if p.rows.len() == 2 && p.rows[0] == p.rows[1] {
        let key: Vec<String> = p.rows[0]
            .iter()
            .map(|c| c.clone().unwrap_or_else(|| "NULL".into()))
            .collect();
        out.push(Finding {
            code: "P005".into(),
            severity: Severity::Warn,
            message: format!(
                "the first two rows tie on the ORDER BY key ({}), so LIMIT 1 picks one of them \
                 arbitrarily; if the question wants every row with the top value, compare with a \
                 MAX/MIN subquery instead",
                key.join(", ")
            ),
            fetch: None,
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::analyze::analyze;
    use crate::schema::tests::fixture;
    use rusqlite::Connection;
    use sqlparser::dialect::SQLiteDialect;

    /// The fixture schema as a real in-memory SQLite database.
    pub struct Mem(pub Connection);

    impl Mem {
        pub fn new() -> Mem {
            let c = Connection::open_in_memory().unwrap();
            c.execute_batch(
                "CREATE TABLE cards (id INTEGER PRIMARY KEY, name TEXT, types TEXT, power INTEGER,
                   height_cm INTEGER, released TEXT, rarity TEXT, colour TEXT);
                 INSERT INTO cards VALUES
                   (1,'Fireball','Instant',3,0,'2017-01-21 00:00:00','common','Berlin, Germany'),
                   (2,'Goyf','Creature',4,180,'2018-02-01 00:00:00','rare','Germany'),
                   (3,'Bear','Creature',4,150,'2019-03-01 00:00:00','common','France');
                 CREATE TABLE rulings (id INTEGER PRIMARY KEY, card_id INTEGER, text TEXT);
                 INSERT INTO rulings VALUES (1,1,'a'),(2,1,'b'),(3,2,'c');
                 CREATE TABLE sets (code TEXT PRIMARY KEY, name TEXT);",
            )
            .unwrap();
            Mem(c)
        }
    }

    impl Probe for Mem {
        fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
            let mut stmt = self.0.prepare(sql).map_err(|e| e.to_string())?;
            let columns: Vec<String> = stmt.column_names().map(|s| s.to_string()).collect();
            let n = columns.len();
            let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
            let mut out = Preview {
                columns,
                ..Default::default()
            };
            while let Some(r) = rows.next().map_err(|e| e.to_string())? {
                if out.rows.len() == max_rows {
                    out.truncated = true;
                    break;
                }
                let row = (0..n)
                    .map(|i| {
                        use rusqlite::types::ValueRef as V;
                        match r.get_ref(i).unwrap() {
                            V::Null => None,
                            V::Integer(x) => Some(x.to_string()),
                            V::Real(x) => Some(x.to_string()),
                            V::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
                            V::Blob(_) => Some("<blob>".into()),
                        }
                    })
                    .collect();
                out.rows.push(row);
            }
            Ok(out)
        }
    }

    fn schema() -> Schema {
        let mut s = fixture();
        s.tables[0].columns.push(crate::Column {
            name: "colour".into(),
            data_type: "TEXT".into(),
            ..Default::default()
        });
        s
    }

    fn codes(sql: &str) -> Vec<String> {
        let s = schema();
        let a = analyze(sql, &SQLiteDialect {}, &s).ok();
        probe(sql, a.as_ref(), &s, &Mem::new())
            .1
            .into_iter()
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn e000_p001_p002_from_the_result() {
        assert_eq!(codes("SELECT nope FROM cards"), vec!["E000"]);
        assert_eq!(codes("SELECT id FROM cards WHERE power > 99"), vec!["P001"]);
        assert_eq!(
            codes("SELECT MAX(power) FROM cards WHERE power > 99"),
            vec!["P002"]
        );
        assert!(codes("SELECT id FROM cards").is_empty());
    }

    #[test]
    fn p003_spelling_and_containment() {
        let s = schema();
        let sql = "SELECT id FROM cards WHERE colour = 'germany'";
        let a = analyze(sql, &SQLiteDialect {}, &s).ok();
        let (_, f) = probe(sql, a.as_ref(), &s, &Mem::new());
        let found: Vec<&str> = f.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(found, vec!["P001", "P003"]);
        assert!(
            f[1].message
                .contains("1 rows match ignoring case and 2 contain it")
        );
        assert!(codes("SELECT id FROM cards WHERE colour = 'Germany'").is_empty());
    }

    #[test]
    fn p004_fan_out_only_without_distinct() {
        assert_eq!(
            codes("SELECT COUNT(c.id) FROM cards c JOIN rulings r ON r.card_id = c.id"),
            vec!["P004"]
        );
        assert!(
            codes("SELECT COUNT(DISTINCT c.id) FROM cards c JOIN rulings r ON r.card_id = c.id")
                .is_empty()
        );
    }

    #[test]
    fn p005_tie_at_the_top() {
        assert_eq!(
            codes("SELECT name FROM cards ORDER BY power DESC LIMIT 1"),
            vec!["P005"]
        );
        assert!(codes("SELECT name FROM cards ORDER BY power ASC LIMIT 1").is_empty());
    }
}
