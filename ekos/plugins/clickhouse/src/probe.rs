//! RFC 0243: checking SQL against a live ClickHouse server — the text-to-SQL probes run through
//! the catalog's HTTP client with `readonly=1` on every request, which a query cannot lift.
//! The text must also be one SELECT (`guard::single_select`) before anything is sent.

use crate::catalog::{ChConnection, Http};
use ekos_sql_dialect_sdk::RowLimit;
use ekos_sql_dialect_sdk::guard::{single_select, wrap_rows};
use ekos_sql_dialect_sdk::probe::{Preview, Probe};
use serde_json::Value;
use sqlparser::dialect::ClickHouseDialect;

pub struct ChProbe {
    conn: ChConnection,
}

impl ChProbe {
    /// `Err` when the password variable is not set.
    pub fn new(conn: ChConnection) -> Result<ChProbe, String> {
        Http::new(&conn)?;
        Ok(ChProbe { conn })
    }
}

impl Probe for ChProbe {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        single_select(sql, &ClickHouseDialect {})?;
        let query = format!(
            "{} FORMAT JSONCompact",
            wrap_rows(sql, RowLimit::Limit, max_rows + 1)
        );
        // The HTTP client is async; a scoped thread with its own runtime keeps this callable
        // from inside another runtime (the agent's) as well as from plain threads.
        let conn = &self.conn;
        let body = std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                rt.block_on(async { Http::new(conn)?.send(&query, true).await })
            })
            .join()
            .map_err(|_| "the probe thread panicked".to_string())?
        })?;
        parse_json_compact(&body, max_rows)
    }
}

/// ClickHouse's `JSONCompact` output as a preview: `null` stays NULL, a quoted 64-bit integer
/// stays its digits.
pub fn parse_json_compact(body: &str, max_rows: usize) -> Result<Preview, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
    let columns = v["meta"]
        .as_array()
        .map(|m| {
            m.iter()
                .map(|c| c["name"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default();
    let data = v["data"].as_array().cloned().unwrap_or_default();
    let truncated = data.len() > max_rows;
    let rows = data
        .iter()
        .take(max_rows)
        .map(|r| {
            r.as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| match c {
                            Value::Null => None,
                            Value::String(s) => Some(s.clone()),
                            other => Some(other.to_string()),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    Ok(Preview {
        columns,
        rows,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_compact_keeps_nulls_and_cuts_at_the_row_limit() {
        let body = r#"{"meta":[{"name":"n","type":"UInt64"},{"name":"s","type":"Nullable(String)"}],
                       "data":[["18446744073709551615",null],["2","b"],["3","c"]],"rows":3}"#;
        let p = parse_json_compact(body, 2).unwrap();
        assert_eq!(p.columns, vec!["n", "s"]);
        assert_eq!(p.rows[0], vec![Some("18446744073709551615".into()), None]);
        assert_eq!(p.rows.len(), 2);
        assert!(p.truncated);
    }

    #[test]
    fn a_write_is_refused_before_anything_is_sent() {
        let conn =
            ChConnection::parse("http://127.0.0.1:1", "u", "EKOS_TEST_UNSET_VAR_0243").unwrap();
        let probe = ChProbe { conn };
        let e = probe.preview("INSERT INTO t VALUES (1)", 5).unwrap_err();
        assert!(e.starts_with("not sent"), "{e}");
    }
}
