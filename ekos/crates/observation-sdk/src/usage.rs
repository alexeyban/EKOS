//! RFC 0252 §3.5 — usage evidence: which statements touch which tables, as counts. Pure.
//!
//! A database that keeps statistics about its own workload says which tables are actually used,
//! and how (read or written), without anyone reading a line of application code. Each source is
//! one aggregate query, and **no statement text and no data value leaves the database**: table
//! names are matched against statement text *inside* the database (`LIKE` with the
//! catalog's own table names, which are already in the ledger), and what comes back per table is
//! a statement kind (a fixed vocabulary: `select`, `insert`, `update`, `delete`, `merge`,
//! `other`), counts and a month.
//!
//! | Source | Engine | What it counts | EKOS's own queries |
//! |---|---|---|---|
//! | [`UsageSource::QueryStore`] | SQL Server | statements and executions per kind, last month | left out: the session tags itself with `SET DATEFIRST` [`EKOS_DATEFIRST`], which Query Store records per query |
//! | [`UsageSource::ExecQueryStats`] | SQL Server | the same, from the plan cache (Query Store off) | left out by the same tag (a plan attribute) |
//! | [`UsageSource::SqlTrace`] | Informix | traced statements per kind (`sysmaster:syssqltrace`, tracing at level `med` or higher) | left out: statements by EKOS's own user |
//! | [`UsageSource::PartitionProfile`] | Informix | rows read, inserted, updated, deleted per table (`sysmaster:sysptprof`) | **included** in reads: a counter cannot tell who read |
//! | [`UsageSource::PgStatStatements`] | PostgreSQL | statements and calls per kind (`pg_stat_statements`, when installed) | left out: statements by EKOS's own role |
//! | [`UsageSource::TableStats`] | PostgreSQL | scans and rows inserted, updated, deleted (`pg_stat_user_tables`) | **included** in scans, as above |
//!
//! Writes are never EKOS's (every EKOS session is read-only), so a write count is clean evidence
//! of use on every source.

use crate::profile::Flavor;
use crate::signals::sql_string;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The `DATEFIRST` an EKOS SQL Server session sets, so its own statements can be told apart in
/// Query Store and the plan cache. No language defaults to 5 (Friday); EKOS's queries never
/// depend on the first day of the week.
pub const EKOS_DATEFIRST: u8 = 5;

/// Where usage evidence comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    QueryStore,
    ExecQueryStats,
    SqlTrace,
    PartitionProfile,
    PgStatStatements,
    TableStats,
}

impl UsageSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::QueryStore => "query_store",
            Self::ExecQueryStats => "dm_exec_query_stats",
            Self::SqlTrace => "syssqltrace",
            Self::PartitionProfile => "sysptprof",
            Self::PgStatStatements => "pg_stat_statements",
            Self::TableStats => "pg_stat_user_tables",
        }
    }

    /// What the counts cover, in words.
    pub fn scope(self) -> &'static str {
        match self {
            Self::QueryStore => "statements Query Store kept (its retention period)",
            Self::ExecQueryStats => "statements whose plans are in the plan cache now",
            Self::SqlTrace => "the statements in the SQL trace buffer (the most recent only)",
            Self::PartitionProfile => "row operations since the server started",
            Self::PgStatStatements => "statements pg_stat_statements kept since it was last reset",
            Self::TableStats => "scans and row operations since statistics were last reset",
        }
    }

    /// True when the counts include EKOS's own read queries (a counter cannot tell who read).
    pub fn includes_own_reads(self) -> bool {
        matches!(self, Self::PartitionProfile | Self::TableStats)
    }
}

/// The sources tried for an engine, in order. Every one that is available is read.
pub fn sources(flavor: Flavor) -> &'static [UsageSource] {
    match flavor {
        Flavor::SqlServer => &[UsageSource::QueryStore, UsageSource::ExecQueryStats],
        Flavor::Informix => &[UsageSource::SqlTrace, UsageSource::PartitionProfile],
        Flavor::Postgres => &[UsageSource::PgStatStatements, UsageSource::TableStats],
        // SQLite keeps no usage statistics; the benchmark reads its files, not a server.
        Flavor::Oracle | Flavor::ClickHouse | Flavor::Sqlite => &[],
    }
}

/// A query returning one number, non-zero when `source` has something to read (Query Store on,
/// the SQL trace holding statements with tables). `None`: just try the usage query.
pub fn available_sql(source: UsageSource) -> Option<&'static str> {
    match source {
        // actual_state 1 = READ_ONLY, 2 = READ_WRITE.
        UsageSource::QueryStore => Some(
            "SELECT CASE WHEN actual_state IN (1, 2) THEN 1 ELSE 0 END FROM sys.database_query_store_options",
        ),
        UsageSource::SqlTrace => {
            Some("SELECT COUNT(*) FROM sysmaster:syssqltrace WHERE sql_numtables > 0")
        }
        UsageSource::PgStatStatements => {
            Some("SELECT count(*) FROM pg_extension WHERE extname = 'pg_stat_statements'")
        }
        _ => None,
    }
}

/// One table, as the catalog names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableName {
    pub owner: String,
    pub table: String,
}

/// The statement kinds, in the order a keyword search tries them.
const KINDS: &[&str] = &["select", "insert", "update", "delete", "merge"];

/// A `LIKE` pattern (T-SQL) matching `word` as a whole word in text padded with a space on both
/// sides: `[`, `%` and `_` are escaped as one-character classes.
fn tsql_word_pattern(word: &str) -> String {
    let mut p = String::from("%[^a-z0-9_@#$]");
    for ch in word.to_lowercase().chars() {
        match ch {
            '[' | '%' | '_' => {
                p.push('[');
                p.push(ch);
                p.push(']');
            }
            c => p.push(c),
        }
    }
    p.push_str("[^a-z0-9_@#$]%");
    p
}

/// A `LIKE` pattern (Informix, escape `\`) for `word` in a space-separated, space-padded name
/// list. `LIKE`, not `MATCHES`: the probe's guard parses with the generic grammar.
fn ifx_list_pattern(word: &str) -> String {
    let mut p = String::from("% ");
    for ch in word.to_lowercase().chars() {
        if matches!(ch, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(ch);
    }
    p.push_str(" %");
    p
}

/// A name put into a pattern or a literal list: letters, digits, `_`, `$`, `#` only. A table with
/// any other name is left out of usage evidence (its quoting differs between engines and
/// statement text; a wrong pattern would count the wrong table).
pub fn plain_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 128
        && n.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '#'))
}

fn plain_tables(tables: &[TableName]) -> Result<Vec<TableName>, String> {
    let v: Vec<TableName> = tables
        .iter()
        .filter(|t| plain_name(&t.owner) && plain_name(&t.table))
        .cloned()
        .collect();
    if v.is_empty() {
        return Err("no table with a plain name to count usage for".into());
    }
    Ok(v)
}

/// `owner, table, kind, statements, executions, last month` per table and statement kind
/// (SQL Server sources); `owner, table, c1…cn` per table (counter sources).
/// The current database is the one counted: `DB_ID()`, `current_database()`, `DBINFO('dbname')`.
pub fn usage_sql(source: UsageSource, tables: &[TableName]) -> Result<String, String> {
    let tables = &plain_tables(tables)?;
    match source {
        UsageSource::QueryStore | UsageSource::ExecQueryStats => Ok(tsql_usage(source, tables)),
        UsageSource::SqlTrace => Ok(sql_trace(tables)),
        UsageSource::PartitionProfile => Ok(partition_profile(tables)),
        UsageSource::PgStatStatements => Ok(pg_statements(tables)),
        UsageSource::TableStats => Ok(table_stats(tables)),
    }
}

fn tsql_usage(source: UsageSource, tables: &[TableName]) -> String {
    let s = |v: &str| format!("N{}", sql_string(Flavor::SqlServer, v));
    let values = tables
        .iter()
        .map(|t| {
            format!(
                "({}, {}, {})",
                s(&t.owner),
                s(&t.table),
                s(&tsql_word_pattern(&t.table))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let kinds = KINDS
        .iter()
        .map(|k| format!("('{k}', PATINDEX({}, x.txt))", s(&tsql_word_pattern(k))))
        .collect::<Vec<_>>()
        .join(", ");
    let (from, stmts, execs, last) = match source {
        UsageSource::QueryStore => (
            "sys.query_store_query_text AS qt \
             JOIN sys.query_store_query AS q ON q.query_text_id = qt.query_text_id \
             JOIN sys.query_context_settings AS cs ON cs.context_settings_id = q.context_settings_id \
             JOIN sys.query_store_plan AS p ON p.query_id = q.query_id \
             JOIN sys.query_store_runtime_stats AS rs ON rs.plan_id = p.plan_id \
             CROSS APPLY (SELECT LOWER(N' ' + qt.query_sql_text + N' ') AS txt) AS x"
                .to_string(),
            "COUNT_BIG(DISTINCT q.query_id)",
            "SUM(rs.count_executions)",
            "MAX(rs.last_execution_time)",
        ),
        _ => (
            "sys.dm_exec_query_stats AS qs \
             CROSS APPLY sys.dm_exec_sql_text(qs.sql_handle) AS st \
             CROSS APPLY (SELECT CONVERT(int, a.value) AS date_first FROM sys.dm_exec_plan_attributes(qs.plan_handle) AS a WHERE a.attribute = N'date_first') AS cs \
             CROSS APPLY (SELECT CONVERT(int, a.value) AS dbid FROM sys.dm_exec_plan_attributes(qs.plan_handle) AS a WHERE a.attribute = N'dbid') AS pa \
             CROSS APPLY (SELECT LOWER(N' ' + SUBSTRING(st.text, qs.statement_start_offset / 2 + 1, \
             (CASE qs.statement_end_offset WHEN -1 THEN DATALENGTH(st.text) ELSE qs.statement_end_offset END \
             - qs.statement_start_offset) / 2 + 1) + N' ') AS txt) AS x"
                .to_string(),
            "COUNT_BIG(DISTINCT qs.query_hash)",
            "SUM(qs.execution_count)",
            "MAX(qs.last_execution_time)",
        ),
    };
    let only_this_db = if source == UsageSource::ExecQueryStats {
        " AND pa.dbid = DB_ID()"
    } else {
        ""
    };
    format!(
        "SELECT t.o, t.n, COALESCE(k.kind, 'other'), {stmts}, {execs}, CONVERT(char(7), {last}, 126) \
         FROM {from} \
         JOIN (VALUES {values}) AS t(o, n, p) ON x.txt LIKE t.p \
         OUTER APPLY (SELECT TOP 1 v.kind FROM (VALUES {kinds}) AS v(kind, pos) WHERE v.pos > 0 ORDER BY v.pos) AS k \
         WHERE cs.date_first <> {EKOS_DATEFIRST}{only_this_db} \
         GROUP BY t.o, t.n, COALESCE(k.kind, 'other')"
    )
}

fn sql_trace(tables: &[TableName]) -> String {
    let s = |v: &str| sql_string(Flavor::Informix, v);
    // One branch per table: Informix has no VALUES list to join.
    let kind = "CASE TRIM(sql_stmtname) WHEN 'SELECT' THEN 'select' WHEN 'INSERT' THEN 'insert' \
                WHEN 'UPDATE' THEN 'update' WHEN 'DELETE' THEN 'delete' WHEN 'MERGE' THEN 'merge' \
                ELSE 'other' END";
    let branches = tables
        .iter()
        .map(|t| {
            format!(
                "SELECT {} AS o, {} AS n, {kind} AS kind, COUNT(DISTINCT sql_stmthash) AS statements, \
                 COUNT(*) AS executions, MAX(sql_finishtime) AS last \
                 FROM sysmaster:syssqltrace \
                 WHERE TRIM(sql_database) = DBINFO('dbname') AND ' ' || LOWER(TRIM(sql_tablelist)) || ' ' LIKE {} ESCAPE '\\' \
                 AND sql_uid <> (SELECT uid FROM sysmaster:syssessions WHERE sid = DBINFO('sessionid')) \
                 GROUP BY 1, 2, 3",
                s(&t.owner),
                s(&t.table),
                s(&ifx_list_pattern(&t.table))
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    format!(
        "SELECT o, n, kind, statements, executions, \
         TO_CHAR(DBINFO('utc_to_datetime', last), '%Y-%m') FROM ({branches}) AS u"
    )
}

fn partition_profile(tables: &[TableName]) -> String {
    let s = |v: &str| sql_string(Flavor::Informix, v);
    let pairs = tables
        .iter()
        .map(|t| {
            format!(
                "(t.owner = {} AND t.tabname = {})",
                s(&t.owner),
                s(&t.table)
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!(
        "SELECT TRIM(t.owner), TRIM(t.tabname), SUM(p.isreads), SUM(p.iswrites), SUM(p.isrewrites), \
         SUM(p.isdeletes), SUM(p.seqscans) \
         FROM sysmaster:sysptprof AS p, systables AS t \
         WHERE p.dbsname = DBINFO('dbname') AND p.tabname = t.tabname AND ({pairs}) GROUP BY 1, 2"
    )
}

/// `name` as a POSIX regular expression matching itself (plain names only: `$` is the one special).
fn regex_word(name: &str) -> String {
    format!(
        "[^a-z0-9_]{}[^a-z0-9_]",
        name.to_lowercase().replace('$', "\\$")
    )
}

fn pg_statements(tables: &[TableName]) -> String {
    let s = |v: &str| sql_string(Flavor::Postgres, v);
    let values = tables
        .iter()
        .map(|t| {
            format!(
                "({}, {}, {})",
                s(&t.owner),
                s(&t.table),
                s(&regex_word(&t.table))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let kinds = KINDS.join("|");
    format!(
        "SELECT t.o, t.n, COALESCE(substring(x.txt FROM '[^a-z0-9_]({kinds})[^a-z0-9_]'), 'other'), \
         count(DISTINCT st.queryid), sum(st.calls), NULL::text \
         FROM pg_stat_statements AS st \
         CROSS JOIN LATERAL (SELECT ' ' || lower(st.query) || ' ' AS txt) AS x \
         JOIN (VALUES {values}) AS t(o, n, p) ON x.txt ~ t.p \
         WHERE st.dbid = (SELECT oid FROM pg_database WHERE datname = current_database()) \
         AND st.userid <> (SELECT oid FROM pg_roles WHERE rolname = current_user) \
         GROUP BY 1, 2, 3"
    )
}

fn table_stats(tables: &[TableName]) -> String {
    let s = |v: &str| sql_string(Flavor::Postgres, v);
    let pairs = tables
        .iter()
        .map(|t| format!("({}, {})", s(&t.owner), s(&t.table)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT schemaname, relname, seq_scan, COALESCE(idx_scan, 0), n_tup_ins, n_tup_upd, n_tup_del \
         FROM pg_stat_user_tables WHERE (schemaname, relname) IN ({pairs})"
    )
}

/// One count about a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageCount {
    /// `select`, `insert`, `update`, `delete`, `merge`, `other` (statement sources); `reads`,
    /// `inserts`, `updates`, `deletes`, `seq_scans`, `index_scans` (counter sources).
    pub kind: String,
    /// `executions` (statement sources), `rows` or `scans` (counters).
    pub unit: String,
    pub count: i64,
    /// Distinct statements (statement sources).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statements: Option<i64>,
}

impl UsageCount {
    /// A write: no EKOS session ever writes, so this is the application's.
    pub fn is_write(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "insert" | "update" | "delete" | "merge" | "inserts" | "updates" | "deletes"
        )
    }
}

/// What one source says about one table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableUsage {
    pub owner: String,
    pub table: String,
    pub source: String,
    pub counts: Vec<UsageCount>,
    /// The latest month a statement touching the table ran (statement sources).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
    /// Another listed table has the same name under another owner: statement text names a table
    /// without saying whose, so these counts may be the other's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ambiguous: bool,
}

fn num(r: &[Option<String>], i: usize) -> i64 {
    r.get(i)
        .cloned()
        .flatten()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0) as i64
}

fn text(r: &[Option<String>], i: usize) -> Option<String> {
    r.get(i)
        .cloned()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The answer of [`usage_sql`] as per-table usage. Only tables in `tables` are kept (the owner and
/// table cells come back from the query's own literals), sorted by (owner, table).
pub fn parse_usage(
    source: UsageSource,
    tables: &[TableName],
    rows: &[Vec<Option<String>>],
) -> Vec<TableUsage> {
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for t in tables {
        *by_name.entry(t.table.to_lowercase()).or_default() += 1;
    }
    let known = |o: &str, n: &str| {
        tables
            .iter()
            .find(|t| t.owner.eq_ignore_ascii_case(o) && t.table.eq_ignore_ascii_case(n))
    };
    let mut out: BTreeMap<TableName, TableUsage> = BTreeMap::new();
    for r in rows {
        let (Some(o), Some(n)) = (text(r, 0), text(r, 1)) else {
            continue;
        };
        let Some(t) = known(&o, &n) else { continue };
        let entry = out.entry(t.clone()).or_insert_with(|| TableUsage {
            owner: t.owner.clone(),
            table: t.table.clone(),
            source: source.name().into(),
            counts: Vec::new(),
            last_used: None,
            ambiguous: by_name.get(&t.table.to_lowercase()).is_some_and(|c| *c > 1)
                && !matches!(
                    source,
                    UsageSource::PartitionProfile | UsageSource::TableStats
                ),
        });
        match source {
            UsageSource::QueryStore
            | UsageSource::ExecQueryStats
            | UsageSource::SqlTrace
            | UsageSource::PgStatStatements => {
                let kind = text(r, 2).unwrap_or_else(|| "other".into());
                // A kind outside the vocabulary cannot come from the planned SQL; never keep it.
                let kind = if KINDS.contains(&kind.as_str()) {
                    kind
                } else {
                    "other".into()
                };
                entry.counts.push(UsageCount {
                    kind,
                    unit: "executions".into(),
                    count: num(r, 4),
                    statements: Some(num(r, 3)),
                });
                entry.last_used = entry.last_used.clone().max(text(r, 5));
            }
            UsageSource::PartitionProfile => {
                for (i, kind, unit) in [
                    (2, "reads", "rows"),
                    (3, "inserts", "rows"),
                    (4, "updates", "rows"),
                    (5, "deletes", "rows"),
                    (6, "seq_scans", "scans"),
                ] {
                    push_counter(entry, kind, unit, num(r, i));
                }
            }
            UsageSource::TableStats => {
                for (i, kind, unit) in [
                    (2, "seq_scans", "scans"),
                    (3, "index_scans", "scans"),
                    (4, "inserts", "rows"),
                    (5, "updates", "rows"),
                    (6, "deletes", "rows"),
                ] {
                    push_counter(entry, kind, unit, num(r, i));
                }
            }
        }
    }
    let mut v: Vec<TableUsage> = out.into_values().collect();
    for u in &mut v {
        u.counts.sort_by(|a, b| a.kind.cmp(&b.kind));
    }
    v
}

fn push_counter(entry: &mut TableUsage, kind: &str, unit: &str, n: i64) {
    match entry.counts.iter_mut().find(|c| c.kind == kind) {
        Some(c) => c.count += n,
        None => entry.counts.push(UsageCount {
            kind: kind.into(),
            unit: unit.into(),
            count: n,
            statements: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> Vec<TableName> {
        [
            ("sales", "orders"),
            ("sales", "order_lines"),
            ("hr", "orders"),
        ]
        .iter()
        .map(|(o, t)| TableName {
            owner: (*o).into(),
            table: (*t).into(),
        })
        .collect()
    }

    #[test]
    fn a_pattern_escapes_like_wildcards_and_matches_whole_words() {
        assert_eq!(
            tsql_word_pattern("order_lines"),
            "%[^a-z0-9_@#$]order[_]lines[^a-z0-9_@#$]%"
        );
        assert_eq!(ifx_list_pattern("order_lines"), "% order\\_lines %");
    }

    #[test]
    fn statement_text_never_leaves_sql_server() {
        for src in [UsageSource::QueryStore, UsageSource::ExecQueryStats] {
            let sql = usage_sql(src, &tables()).unwrap();
            // The select list is names from the VALUES list, a kind, counts and a month.
            let select = &sql[..sql.find(" FROM ").unwrap()];
            assert!(
                !select.contains("txt") && !select.contains("query_sql_text"),
                "{select}"
            );
            assert!(!select.contains("st.text"), "{select}");
            assert!(
                sql.contains(&format!("cs.date_first <> {EKOS_DATEFIRST}")),
                "{sql}"
            );
            assert!(
                sql.contains("N'%[^a-z0-9_@#$]order[_]lines[^a-z0-9_@#$]%'"),
                "{sql}"
            );
        }
        assert!(
            usage_sql(UsageSource::ExecQueryStats, &tables())
                .unwrap()
                .contains("pa.dbid = DB_ID()")
        );
    }

    #[test]
    fn the_trace_query_returns_names_kinds_and_counts_only() {
        let sql = usage_sql(UsageSource::SqlTrace, &tables()).unwrap();
        let outer = &sql[..sql.find(" FROM (").unwrap()];
        assert!(
            !outer.contains("sql_statement") && !sql.contains("sql_statement"),
            "{sql}"
        );
        assert!(
            sql.contains("LIKE '% order\\_lines %' ESCAPE '\\'"),
            "{sql}"
        );
        assert!(sql.contains("sql_uid <>"), "{sql}");
        assert_eq!(sql.matches("UNION ALL").count(), 2);
    }

    #[test]
    fn a_hostile_name_is_refused() {
        let bad = vec![TableName {
            owner: "s".into(),
            table: "x' OR 1=1 --".into(),
        }];
        assert!(usage_sql(UsageSource::QueryStore, &bad).is_err());
        assert!(usage_sql(UsageSource::TableStats, &[]).is_err());
    }

    #[test]
    fn statement_rows_parse_with_ambiguity_and_unknown_kinds_folded() {
        let row = |o: &str, n: &str, k: &str, s: &str, e: &str, m: Option<&str>| {
            vec![
                Some(o.to_string()),
                Some(n.to_string()),
                Some(k.to_string()),
                Some(s.to_string()),
                Some(e.to_string()),
                m.map(str::to_string),
            ]
        };
        let rows = vec![
            row("sales", "orders", "select", "2", "40", Some("2026-10")),
            row("sales", "orders", "update", "1", "3", Some("2026-11")),
            row("sales", "order_lines", "weird", "1", "1", None),
            row("nobody", "else", "select", "1", "1", None),
        ];
        let u = parse_usage(UsageSource::QueryStore, &tables(), &rows);
        assert_eq!(u.len(), 2);
        let orders = u.iter().find(|t| t.table == "orders").unwrap();
        assert!(orders.ambiguous, "hr.orders shares the name");
        assert_eq!(orders.last_used.as_deref(), Some("2026-11"));
        assert_eq!(
            orders
                .counts
                .iter()
                .map(UsageCount::is_write)
                .filter(|w| *w)
                .count(),
            1
        );
        let lines = u.iter().find(|t| t.table == "order_lines").unwrap();
        assert_eq!(lines.counts[0].kind, "other");
        assert!(!lines.ambiguous);
    }

    #[test]
    fn counters_parse_per_table() {
        let rows = vec![vec![
            Some("public".into()),
            Some("orders".into()),
            Some("12".into()),
            Some("5".into()),
            Some("7".into()),
            Some("1".into()),
            Some("0".into()),
        ]];
        let t = vec![TableName {
            owner: "public".into(),
            table: "orders".into(),
        }];
        let u = parse_usage(UsageSource::TableStats, &t, &rows);
        assert_eq!(u[0].counts.len(), 5);
        let ins = u[0].counts.iter().find(|c| c.kind == "inserts").unwrap();
        assert_eq!((ins.count, ins.unit.as_str()), (7, "rows"));
        assert!(UsageSource::TableStats.includes_own_reads());
        assert!(!UsageSource::QueryStore.includes_own_reads());
        let sql = usage_sql(UsageSource::TableStats, &t).unwrap();
        assert!(sql.contains("IN (('public', 'orders'))"), "{sql}");
        assert!(sources(Flavor::Oracle).is_empty());
        let st = usage_sql(UsageSource::PgStatStatements, &t).unwrap();
        let select = &st[..st.find(" FROM pg_stat_statements").unwrap()];
        assert!(
            !select.contains("st.query,") && select.contains("substring(x.txt"),
            "{st}"
        );
        assert!(
            st.contains("st.userid <> (SELECT oid FROM pg_roles WHERE rolname = current_user)")
        );
        assert_eq!(regex_word("a$b"), "[^a-z0-9_]a\\$b[^a-z0-9_]");
    }
}
