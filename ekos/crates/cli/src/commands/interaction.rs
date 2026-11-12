//! RFC 0256 — `ekos interaction matrix|findings`: who reads and writes each table, declared and
//! observed.
//!
//! The matrix itself is pure (`ekos_kir::interaction`). This module reads its inputs through the
//! caller's (possibly scoped, RFC 0181) store: the objects and relationships, the source file of
//! every `JavaMethod` that states SQL (from its evidence), and the current `ObservedAccess` facts.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::interaction::{self, Actor, Cell, DeclaredInput, Matrix, Observed, Op};
use ekos_kir::{KirId, KirObject, ObjectKind};
use ekos_ledger::KnowledgeReader;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// The `Custom` kind of one observed (database, role, table, operation) aggregate.
pub const OBSERVED_KIND: &str = "ObservedAccess";

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn s<'a>(o: &'a KirObject, k: &str) -> Option<&'a str> {
    o.properties.get(k).and_then(Value::as_str)
}

/// The source file of every `JavaMethod` that states SQL, from its first evidence record. A
/// method in test code is left out, so it is no actor.
fn method_paths(
    objects: &[KirObject],
    ledger: &dyn KnowledgeReader,
) -> Result<HashMap<KirId, String>> {
    let mut out = HashMap::new();
    for o in objects.iter().filter(|o| is_custom(o, "JavaMethod")) {
        let states_sql = o
            .properties
            .get("sql")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        // Test code is not a writer of the application's tables (the RFC 0212 rule).
        if states_sql
            && let Some(ev) = o.evidence.first()
            && let Some(e) = ledger.get_evidence(ev)?
            && !ekos_recovery::java_po::is_test_path(&e.location.path)
        {
            out.insert(o.id, e.location.path);
        }
    }
    Ok(out)
}

/// Adds the current `ObservedAccess` facts (RFC 0256 §3.2) to `m`. Returns how many were added.
/// Only facts the latest observation wrote count (`current`, from `<state>/interaction/current.json`);
/// with no observation yet, none does.
pub fn add_observed(
    m: &mut Matrix,
    objects: &[KirObject],
    current: Option<&BTreeSet<String>>,
) -> usize {
    let Some(current) = current else {
        return 0;
    };
    let mut n = 0;
    for o in objects
        .iter()
        .filter(|o| is_custom(o, OBSERVED_KIND) && current.contains(&o.id.to_string()))
    {
        let (Some(table), Some(role), Some(op)) = (
            s(o, "table"),
            s(o, "role"),
            s(o, "operation").and_then(Op::parse),
        ) else {
            continue;
        };
        let u = |k: &str| o.properties.get(k).and_then(Value::as_u64).unwrap_or(0);
        m.add_observed(
            table,
            role,
            op,
            Observed {
                calls: u("calls"),
                rows: u("rows"),
                window_start: s(o, "window_start").map(str::to_string),
                window_end: s(o, "window_end").unwrap_or_default().to_string(),
                truncated: o
                    .properties
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                first_observed: s(o, "first_observed").map(str::to_string),
            },
        );
        n += 1;
    }
    n
}

/// The matrix, declared and observed, through `ledger`.
pub fn build(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
) -> Result<(Matrix, usize)> {
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    build_from(config, cwd, ledger, &objects, &relationships)
}

/// [`build`] over objects and relationships already read from `ledger` (which still gives the
/// evidence of each Java method).
pub fn build_from(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    objects: &[KirObject],
    relationships: &[ekos_kir::KirRelationship],
) -> Result<(Matrix, usize)> {
    // Code removed from the workspace stays in the append-only ledger; only what the latest
    // compile holds declares access (a deleted job no longer explains a write).
    let current = current_code(config, cwd, objects);
    let objects: &[KirObject] = current.as_deref().unwrap_or(objects);
    let paths = method_paths(objects, ledger)?;
    let mut m = interaction::declared(
        objects,
        relationships,
        &DeclaredInput {
            method_paths: &paths,
            component_depth: config.interaction.component_depth,
        },
    );
    let observed = add_observed(&mut m, objects, current_ids(config, cwd).as_ref());
    retain_visible(&mut m, objects);
    Ok((m, observed))
}

/// The kinds whose objects state access: code that a deleted file takes away.
const CODE_KINDS: [&str; 6] = [
    "Procedure",
    "View",
    "Trigger",
    "JavaMethod",
    "TransformNode",
    "AppProcess",
];

/// `objects` without code objects the latest compile no longer holds (`<state>/ckm/model.json`);
/// `None` when there is no compiled model to compare with (every object counts, as before).
fn current_code(config: &EkosConfig, cwd: &Path, objects: &[KirObject]) -> Option<Vec<KirObject>> {
    let ids = compiled_ids(&config.ekos_dir(cwd).join("ckm").join("model.json"))?;
    Some(
        objects
            .iter()
            .filter(|o| match &o.kind {
                ObjectKind::Custom(k) if CODE_KINDS.contains(&k.as_str()) => ids.contains(&o.id),
                _ => true,
            })
            .cloned()
            .collect(),
    )
}

/// The ids of the latest compiled model's objects.
fn compiled_ids(model: &Path) -> Option<std::collections::HashSet<KirId>> {
    ekos_common::compress::resolve_auto(model)?;
    let m: ekos_semantic::CkModel = ekos_common::compress::read_json_auto(model).ok()?;
    Some(m.objects.into_iter().map(|o| o.id).collect())
}

/// RFC 0181: a row only for a relation this reader's store returned. Observed facts carry no link
/// to the table object, so without this a denied table would come back through them.
fn retain_visible(m: &mut Matrix, objects: &[KirObject]) {
    let visible: BTreeSet<String> = objects
        .iter()
        .filter(|o| {
            matches!(o.kind, ObjectKind::Table | ObjectKind::Dataset) || is_custom(o, "View")
        })
        .map(|o| interaction::table_key(&o.name))
        .collect();
    m.retain_tables(|t| visible.contains(t));
}

/// Keeps the rows `--table` and the cells `--actor` (`kind:name`, or a name) select.
pub fn filter(m: &mut Matrix, table: Option<&str>, actor: Option<&str>) {
    if let Some(t) = table {
        let key = interaction::table_key(t);
        m.retain_tables(|k| k == key);
    }
    if let Some(a) = actor {
        let a = a.to_lowercase();
        m.retain_actors(|x| x.key().to_lowercase() == a || x.name.to_lowercase() == a);
    }
}

fn ops(cell: &Cell) -> String {
    let mut parts: Vec<String> = Vec::new();
    for op in &cell.declared {
        let seen = cell.observed.get(op).map(|o| o.calls);
        parts.push(match seen {
            Some(c) => format!("{} (declared, observed {c})", op.as_str()),
            None => format!("{} (declared)", op.as_str()),
        });
    }
    for (op, o) in &cell.observed {
        if !cell.declared.contains(op) {
            parts.push(format!("{} (observed {})", op.as_str(), o.calls));
        }
    }
    parts.join(", ")
}

/// The matrix as JSON: one entry per table, one cell per actor.
pub fn to_json(m: &Matrix, observed: usize, unavailable: &BTreeMap<String, String>) -> Value {
    let tables: Vec<Value> = m
        .tables
        .iter()
        .map(|(t, row)| {
            let cells: Vec<Value> = row
                .iter()
                .map(|(a, c)| {
                    json!({"actor": a.key(), "kind": a.kind.as_str(), "name": a.name,
                           "declared": c.declared, "observed": c.observed, "via": c.via,
                           "writes": c.writes()})
                })
                .collect();
            json!({"table": t, "cells": cells})
        })
        .collect();
    json!({
        "tables": tables,
        "observed_facts": observed,
        "observed_note": observed_note(observed, unavailable),
        "no_usage_statistics": unavailable,
    })
}

/// What "observed" means for this project right now, said in words (RFC 0256 §3.2: an empty
/// observation is never shown as "nobody writes").
fn observed_note(observed: usize, unavailable: &BTreeMap<String, String>) -> String {
    let none: Vec<String> = unavailable
        .iter()
        .map(|(db, why)| format!("{db}: {why}"))
        .collect();
    let base = if observed == 0 && !none.is_empty() {
        "This source provides no usage statistics, so only declared access is shown — absence is \
         not evidence that nothing else writes."
    } else if observed == 0 {
        "No observed access recorded: the source provides no usage statistics, or they were not \
         read (`ekos interaction observe`). Only declared access is shown — absence is not \
         evidence that nothing else writes."
    } else {
        "Observed access is aggregated from the engine's statement statistics over a dated, \
         possibly truncated window: \"not observed\" is not \"never happens\"."
    };
    if none.is_empty() {
        base.to_string()
    } else {
        format!("{base} No usage statistics — {}", none.join("; "))
    }
}

fn print(m: &Matrix, observed: usize, unavailable: &BTreeMap<String, String>) {
    if m.tables.is_empty() {
        println!("No table access found.");
    }
    let mut writers: BTreeMap<usize, usize> = BTreeMap::new();
    for (t, row) in &m.tables {
        let n = row.values().filter(|c| c.writes()).count();
        *writers.entry(n).or_default() += 1;
        println!("{t}  ({} actor(s), {n} writing)", row.len());
        let mut cells: Vec<(&Actor, &Cell)> = row.iter().collect();
        cells.sort_by_key(|(a, c)| (!c.writes(), a.kind, a.name.clone()));
        for (a, c) in cells {
            println!("  {:<48} {}", a.key(), ops(c));
        }
    }
    let actors = m.actors();
    println!(
        "\n{} table(s), {} actor(s). {}",
        m.tables.len(),
        actors.len(),
        observed_note(observed, unavailable)
    );
}

/// `ekos interaction matrix [--table] [--actor] [--json]`.
pub fn matrix(
    config: &EkosConfig,
    cwd: &Path,
    table: Option<&str>,
    actor: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let (mut m, observed) = build(config, cwd, &*ledger)?;
    filter(&mut m, table, actor);
    let none = unavailable(config, cwd);
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&to_json(&m, observed, &none))?
        );
    } else {
        print(&m, observed, &none);
    }
    Ok(())
}

// ── Observed access (RFC 0256 §3.2) ──────────────────────────────────────────────────────────

/// The window and counters of one observation, per database.
pub const WINDOW_KIND: &str = "ObservationWindow";
/// A database role seen running statements.
pub const ROLE_KIND: &str = "DbRole";

/// What a server reports for a statement whose text the session may not read.
const HIDDEN_TEXT: &str = "<insufficient privilege>";

/// One `pg_stat_statements` row as read: the role, the normalized statement, calls and rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementRow {
    pub role: String,
    pub query: String,
    pub calls: u64,
    pub rows: u64,
}

/// Statements aggregated to (role, table, operation). No statement text survives it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Aggregate {
    /// (role, table, operation) → (calls, rows).
    pub cells: BTreeMap<(String, String, Op), (u64, u64)>,
    /// Calls per role whose statement the footprint could not attribute to a table.
    pub unparsed_calls: BTreeMap<String, u64>,
    /// Calls whose text the session was not allowed to read (needs `pg_read_all_stats`).
    pub hidden_calls: u64,
    /// Calls of the reading role itself (EKOS's own sessions), left out.
    pub own_calls: u64,
    /// Calls of schema statements (`CREATE`, `DROP`, `ALTER`, grants): they change no rows, so they
    /// are attributed to no table.
    pub ddl_calls: u64,
}

/// A schema statement: it reads and writes no rows. A ClickHouse mutation (`ALTER TABLE … UPDATE
/// | DELETE`) changes rows and is not one.
fn is_ddl(text: &str) -> bool {
    let upper = text.trim_start().to_ascii_uppercase();
    let first = upper.split_whitespace().next().unwrap_or("");
    match first {
        "CREATE" | "DROP" | "GRANT" | "REVOKE" | "COMMENT" | "RENAME" => true,
        "ALTER" => !(upper.contains(" UPDATE ") || upper.contains(" DELETE ")),
        _ => false,
    }
}

/// ClickHouse logs an `INSERT` whose rows came as data without them (`INSERT INTO t VALUES`,
/// `… FORMAT CSV`). A placeholder row makes it parse, so its target table is still found; the
/// text is never kept.
fn complete_insert(text: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = text.trim_end().trim_end_matches(';');
    let upper = trimmed.to_ascii_uppercase();
    if !upper.trim_start().starts_with("INSERT") {
        return text.into();
    }
    if upper.ends_with(" VALUES") {
        return format!("{trimmed} (NULL)").into();
    }
    if let Some(i) = upper.rfind(" FORMAT ")
        && !upper[i + 8..].trim().contains(' ')
        && !upper.contains(" SELECT ")
    {
        return format!("{} VALUES (NULL)", &trimmed[..i]).into();
    }
    text.into()
}

fn is_system(table: &str) -> bool {
    table.starts_with("pg_")
        || table == "information_schema"
        || table == "dual"
        || table.starts_with("v$")
        || table.starts_with("gv$")
        || table.starts_with("dba_")
        || table.starts_with("all_")
        || table.starts_with("user_")
        || table.starts_with("system.")
}

/// Aggregates statement rows: each statement's text is redacted, parsed for the tables it reads
/// and writes (RFC 0163's footprint), counted, and dropped. `own_role`'s statements and those
/// touching only system catalogs are left out.
pub fn aggregate(
    rows: &[StatementRow],
    own_role: &str,
    redaction: &ekos_common::redaction::RedactionConfig,
) -> Aggregate {
    aggregate_in(
        &sqlparser::dialect::PostgreSqlDialect {},
        rows,
        own_role,
        redaction,
    )
}

/// [`aggregate`] with the statements parsed in `dialect` (the engine they ran on).
pub fn aggregate_in(
    dialect: &dyn sqlparser::dialect::Dialect,
    rows: &[StatementRow],
    own_role: &str,
    redaction: &ekos_common::redaction::RedactionConfig,
) -> Aggregate {
    let mut a = Aggregate::default();
    for r in rows {
        if r.role.eq_ignore_ascii_case(own_role) {
            a.own_calls += r.calls;
            continue;
        }
        if r.query.trim() == HIDDEN_TEXT {
            a.hidden_calls += r.calls;
            continue;
        }
        if is_ddl(&r.query) {
            a.ddl_calls += r.calls;
            continue;
        }
        let text = ekos_common::redaction::redact(&r.query, redaction);
        let fp = ekos_recovery::plpgsql_footprint::statement_footprint_in(
            dialect,
            &complete_insert(&text),
        );
        let touched = touched_tables(&fp);
        if touched.is_empty() {
            if !fp.errors.is_empty() {
                *a.unparsed_calls.entry(r.role.clone()).or_default() += r.calls;
            }
            continue;
        }
        for (t, ops) in touched {
            for op in ops {
                let c = a.cells.entry((r.role.clone(), t.clone(), op)).or_default();
                c.0 += r.calls;
                c.1 += r.rows;
            }
        }
    }
    a
}

/// One statement's footprint as table → operations: inserts, updates, deletes and reads by name;
/// `writes` only for tables the split does not already name. System catalogs are left out.
fn touched_tables(
    fp: &ekos_recovery::plpgsql_footprint::Footprint,
) -> BTreeMap<String, BTreeSet<Op>> {
    let mut touched: BTreeMap<String, BTreeSet<Op>> = BTreeMap::new();
    let mut split = BTreeSet::new();
    for (set, op) in [
        (&fp.inserts, Op::Insert),
        (&fp.updates, Op::Update),
        (&fp.deletes, Op::Delete),
        (&fp.reads, Op::Read),
    ] {
        for t in set {
            let t = interaction::table_key(t);
            if op.is_write() {
                split.insert(t.clone());
            }
            touched.entry(t).or_default().insert(op);
        }
    }
    for t in &fp.writes {
        let t = interaction::table_key(t);
        if !split.contains(&t) {
            touched.entry(t).or_default().insert(Op::Write);
        }
    }
    touched.retain(|t, _| !is_system(t));
    touched
}

/// How a role is shown under `[interaction] roles` (RFC 0256 §3.2): its name, or a salted hash.
/// A name a redaction rule matches is always hashed.
pub fn role_label(
    role: &str,
    mode: ekos_compiler_core::RoleNames,
    salt: &str,
    redaction: &ekos_common::redaction::RedactionConfig,
) -> String {
    let matched = ekos_common::redaction::redact(role, redaction) != role;
    if mode == ekos_compiler_core::RoleNames::Names && !matched {
        return role.to_string();
    }
    let h = ekos_common::ContentHash::of_str(&format!("{salt}:{role}"));
    format!("role-{}", &h.as_str()[..12])
}

const SQL_EXTENSION: &str =
    "SELECT count(*)::text FROM pg_extension WHERE extname = 'pg_stat_statements'";
const SQL_ME: &str = "SELECT current_user::text";
const SQL_STATEMENTS: &str = "SELECT r.rolname::text, s.query, s.calls::text, s.rows::text \
     FROM pg_stat_statements s JOIN pg_roles r ON r.oid = s.userid \
     WHERE s.dbid = (SELECT oid FROM pg_database WHERE datname = current_database())";
const SQL_INFO: &str = "SELECT stats_reset::text, dealloc::text FROM pg_stat_statements_info";
const SQL_TABLES: &str = "SELECT relname::text, n_tup_ins::text, n_tup_upd::text, n_tup_del::text \
     FROM pg_stat_user_tables";

/// The most statement rows one observation reads (`pg_stat_statements.max` defaults to 5,000).
const MAX_STATEMENTS: usize = 200_000;

/// What one database's statistics gave, before facts are written.
#[derive(Debug, Default)]
struct Reading {
    rows: Vec<StatementRow>,
    me: String,
    stats_reset: Option<String>,
    dealloc: u64,
    /// table → (inserted, updated, deleted) rows since the counters' reset.
    table_writes: BTreeMap<String, (u64, u64, u64)>,
    statements_truncated: bool,
    /// The view always loses entries (Oracle's shared pool ages cursors out): incomplete by
    /// construction, so `declared_not_observed` is never derived from it.
    always_partial: bool,
    /// Where the statistics came from, for the evidence (`pg_stat_statements`, `V$SQL`, …).
    view: &'static str,
    /// What the window start means on this engine.
    window_note: &'static str,
}

/// One database's usage statistics, or why there are none (RFC 0256 §3.2: every surface says
/// so, rather than show an empty list).
#[derive(Debug)]
enum Availability {
    Read(Reading),
    Unavailable(String),
}

/// The dialect statements of `kind` are parsed in.
fn statement_dialect(
    kind: &ekos_compiler_core::DatabaseKind,
) -> Box<dyn sqlparser::dialect::Dialect> {
    use ekos_compiler_core::DatabaseKind as K;
    match kind {
        K::Postgres => Box::new(sqlparser::dialect::PostgreSqlDialect {}),
        K::Clickhouse => Box::new(sqlparser::dialect::ClickHouseDialect {}),
        _ => Box::new(sqlparser::dialect::GenericDialect {}),
    }
}

/// Why an engine gives no per-role statement statistics EKOS reads.
fn no_usage_statistics(kind: &ekos_compiler_core::DatabaseKind) -> Option<&'static str> {
    use ekos_compiler_core::DatabaseKind as K;
    match kind {
        K::Sqlserver => Some(
            "SQL Server's statement statistics (sys.dm_exec_query_stats, Query Store) do not \
             record which login ran a statement: this source provides no usage statistics",
        ),
        K::Informix => Some(
            "Informix keeps no per-user statement statistics EKOS reads: this source provides \
             no usage statistics",
        ),
        _ => None,
    }
}

fn num(v: &Option<String>) -> u64 {
    v.as_deref().and_then(|x| x.parse().ok()).unwrap_or(0)
}

type Run<'a> = dyn Fn(&str, usize) -> std::result::Result<ekos_sqlgen::probe::Preview, String> + 'a;

fn first_value(run: &Run, sql: &str) -> std::result::Result<Option<String>, String> {
    Ok(run(sql, 1)?
        .rows
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next())
        .flatten())
}

fn statement_rows(run: &Run, sql: &str, r: &mut Reading) -> std::result::Result<(), String> {
    let st = run(sql, MAX_STATEMENTS)?;
    r.statements_truncated = st.truncated;
    for row in st.rows {
        let [role, query, calls, rows] = <[Option<String>; 4]>::try_from(row)
            .map_err(|_| format!("{}: unexpected columns", r.view))?;
        r.rows.push(StatementRow {
            role: role.unwrap_or_default(),
            query: query.unwrap_or_default(),
            calls: num(&calls),
            rows: num(&rows),
        });
    }
    Ok(())
}

/// PostgreSQL: `pg_stat_statements` (+ `_info`) and `pg_stat_user_tables`.
fn read_postgres(run: &Run) -> std::result::Result<Availability, String> {
    if first_value(run, SQL_EXTENSION)?.as_deref() != Some("1") {
        return Ok(Availability::Unavailable(
            "pg_stat_statements is not installed: this source provides no usage statistics".into(),
        ));
    }
    let mut r = Reading {
        me: first_value(run, SQL_ME)?.unwrap_or_default(),
        view: "pg_stat_statements",
        window_note: "the statistics' last reset",
        ..Default::default()
    };
    statement_rows(run, SQL_STATEMENTS, &mut r)?;
    // `pg_stat_statements_info` exists from PostgreSQL 14; without it the window start is unknown.
    if let Ok(info) = run(SQL_INFO, 1)
        && let Some(row) = info.rows.into_iter().next()
    {
        r.stats_reset = row.first().cloned().flatten();
        r.dealloc = num(&row.get(1).cloned().flatten());
    }
    for row in run(SQL_TABLES, MAX_STATEMENTS)?.rows {
        let get = |i: usize| row.get(i).cloned().flatten();
        if let Some(t) = get(0) {
            r.table_writes.insert(
                interaction::table_key(&t),
                (num(&get(1)), num(&get(2)), num(&get(3))),
            );
        }
    }
    Ok(Availability::Read(r))
}

const CH_HAS_LOG: &str =
    "SELECT toString(count()) FROM system.tables WHERE database = 'system' AND name = 'query_log'";
const CH_ME: &str = "SELECT currentUser()";
/// One row per (user, normalized statement) of the finished queries touching `owners` (the
/// `[[databases]]` entry's databases; the session's own when none is named). The text is a sample
/// of the group (its literals included) because `normalizeQuery`'s `?..` lists do not parse; it
/// is redacted, parsed for its footprint and dropped in process, like PostgreSQL's.
fn ch_statements(owners: &[String]) -> String {
    let scope = if owners.is_empty() {
        "has(databases, currentDatabase())".to_string()
    } else {
        let list: Vec<String> = owners
            .iter()
            .map(|o| format!("'{}'", o.replace('\\', "\\\\").replace('\'', "\\'")))
            .collect();
        format!("hasAny(databases, [{}])", list.join(", "))
    };
    format!(
        "SELECT user, substring(any(query), 1, 20000), toString(count()), \
         toString(sum(written_rows) + sum(result_rows)) FROM system.query_log \
         WHERE type = 'QueryFinish' AND {scope} GROUP BY user, normalized_query_hash"
    )
}
const CH_WINDOW: &str = "SELECT toString(min(event_time)) FROM system.query_log";

/// ClickHouse: `system.query_log`. Its window starts at the oldest entry the log keeps (its TTL);
/// there are no per-table write counters, so no `unattributed_writes`.
fn read_clickhouse(run: &Run, owners: &[String]) -> std::result::Result<Availability, String> {
    if first_value(run, CH_HAS_LOG)?.as_deref() != Some("1") {
        return Ok(Availability::Unavailable(
            "system.query_log is not enabled on this server: this source provides no usage \
             statistics"
                .into(),
        ));
    }
    let mut r = Reading {
        me: first_value(run, CH_ME)?.unwrap_or_default(),
        view: "system.query_log",
        window_note: "the oldest entry the query log keeps",
        ..Default::default()
    };
    statement_rows(run, &ch_statements(owners), &mut r)?;
    r.stats_reset = first_value(run, CH_WINDOW)?.map(|t| format!("{}Z", t.replace(' ', "T")));
    Ok(Availability::Read(r))
}

const ORA_ME: &str = "SELECT USER FROM DUAL";
/// One row per (user, statement) of the cursors in the shared pool: inserts, selects, updates,
/// deletes and merges (command types 2, 3, 6, 7, 189), by users Oracle does not maintain itself.
const ORA_STATEMENTS: &str = "SELECT u.USERNAME, DBMS_LOB.SUBSTR(s.SQL_FULLTEXT, 4000, 1), \
     TO_CHAR(SUM(s.EXECUTIONS)), TO_CHAR(SUM(s.ROWS_PROCESSED)) \
     FROM V$SQL s JOIN ALL_USERS u ON u.USER_ID = s.PARSING_USER_ID \
     WHERE s.COMMAND_TYPE IN (2, 3, 6, 7, 189) AND u.ORACLE_MAINTAINED = 'N' \
     GROUP BY u.USERNAME, s.SQL_ID, DBMS_LOB.SUBSTR(s.SQL_FULLTEXT, 4000, 1)";
const ORA_WINDOW: &str =
    "SELECT TO_CHAR(STARTUP_TIME, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') FROM V$INSTANCE";

/// Oracle: `V$SQL` (needs `SELECT_CATALOG_ROLE` or `SELECT ANY DICTIONARY`). The shared pool ages
/// cursors out, so an observation is partial by construction; the window starts at the
/// instance's start-up. `DBA_TAB_MODIFICATIONS` counts since the last statistics gathering, a
/// different window from the cursors', so it is not used as the cross-check (no
/// `unattributed_writes` on Oracle). A statement run inside a definer's-rights routine is
/// attributed to the user that parsed it.
fn read_oracle(run: &Run) -> std::result::Result<Availability, String> {
    let me = first_value(run, ORA_ME)?.unwrap_or_default();
    let mut r = Reading {
        me,
        always_partial: true,
        view: "V$SQL",
        window_note: "the instance's start-up; cursors aged out of the shared pool are missing",
        ..Default::default()
    };
    if let Err(e) = statement_rows(run, ORA_STATEMENTS, &mut r) {
        return Ok(Availability::Unavailable(format!(
            "V$SQL is not readable by this user (grant SELECT_CATALOG_ROLE): this source \
             provides no usage statistics ({e})"
        )));
    }
    r.stats_reset = first_value(run, ORA_WINDOW).ok().flatten();
    Ok(Availability::Read(r))
}

/// Reads one database's statement statistics in the read-only probe session.
fn read_database(
    db: &ekos_compiler_core::DatabaseConfig,
    allow_writable_role: bool,
) -> std::result::Result<Availability, String> {
    use ekos_compiler_core::DatabaseKind as K;
    use ekos_sqlgen::probe::Probe as _;
    if let Some(why) = no_usage_statistics(&db.kind) {
        return Ok(Availability::Unavailable(why.into()));
    }
    let (worker, status) = super::sql_check::ProbeWorker::start(
        &super::sql_check::Target::Database(db.clone()),
        allow_writable_role,
    );
    let Some(w) = worker else {
        return Err(match status {
            Some(Err(e)) => e,
            _ => "not connected".into(),
        });
    };
    let run = |sql: &str, n: usize| w.preview(sql, n);
    match db.kind {
        K::Postgres => read_postgres(&run),
        K::Clickhouse => read_clickhouse(&run, &db.owners),
        K::Oracle => read_oracle(&run),
        _ => Ok(Availability::Unavailable(
            "this engine provides no usage statistics".into(),
        )),
    }
}

fn kid(key: &str) -> KirId {
    KirId(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        key.as_bytes(),
    ))
}

fn custom_object(name: String, kind: &str, key: &str, props: Value) -> KirObject {
    let mut o = KirObject::new(name, ObjectKind::Custom(kind.into()));
    o.id = kid(key);
    if let Value::Object(m) = props {
        o.properties = m.into_iter().collect();
    }
    o
}

/// The facts of one reading: one `ObservedAccess` per (role, table, operation) and one
/// `ObservationWindow` for the database (the `DbRole` items are written by [`commit_step`]). Every one cites the same evidence, which says
/// what was read and that no statement text was kept.
/// RFC 0256 open question 2, answered cheaply: what an observed cell carries from the previous
/// observation of it. `first_observed` is the earliest observation that held the cell; the calls
/// since the previous observation are known only when the statistics were not reset in between
/// (same window start, calls not lower).
fn carried(
    previous: Option<&KirObject>,
    window_start: Option<&str>,
    calls: u64,
    at: &str,
) -> Value {
    let Some(p) = previous else {
        return json!({"first_observed": at});
    };
    let first = s(p, "first_observed")
        .or_else(|| s(p, "window_end"))
        .unwrap_or(at)
        .to_string();
    let prev_calls = p.properties.get("calls").and_then(Value::as_u64);
    let same_window = s(p, "window_start") == window_start && window_start.is_some();
    let since = match prev_calls {
        Some(n) if same_window && calls >= n => json!(calls - n),
        _ => Value::Null,
    };
    json!({"first_observed": first, "calls_since_previous": since,
           "previous_observation": s(p, "window_end")})
}

fn facts(
    database: &str,
    reading: &Reading,
    agg: &Aggregate,
    label: &dyn Fn(&str) -> String,
    at: &str,
    previous: &HashMap<KirId, &KirObject>,
) -> (Vec<KirObject>, ekos_kir::KirEvidence) {
    let mut ev = ekos_kir::KirEvidence::new(
        ekos_kir::SourceLocation {
            path: format!("observe://{database}/{}", reading.view),
            line: None,
            column: None,
        },
        format!(
            "{database}: statement statistics ({}) aggregated per role, table and operation at {at} \
             (RFC 0256); the window starts at {}; no statement text kept",
            reading.view, reading.window_note
        ),
    );
    ev.id = kid(&format!("observation:{database}:evidence"));
    let truncated = reading.dealloc > 0 || reading.statements_truncated || reading.always_partial;
    let mut out = Vec::new();
    for ((role, table, op), (calls, rows)) in &agg.cells {
        let shown = label(role);
        let key = format!("observed-access:{database}:{shown}:{table}:{}", op.as_str());
        let carry = carried(
            previous.get(&kid(&key)).copied(),
            reading.stats_reset.as_deref(),
            *calls,
            at,
        );
        let mut props = json!({"database": database, "role": shown, "table": table,
                   "operation": op.as_str(), "calls": calls, "rows": rows,
                   "window_start": reading.stats_reset, "window_end": at,
                   "truncated": truncated, "status": "observed"});
        if let (Value::Object(p), Value::Object(c)) = (&mut props, carry) {
            p.extend(c);
        }
        out.push(custom_object(
            format!("{shown} {} {table}", op.as_str()),
            OBSERVED_KIND,
            &key,
            props,
        ));
    }
    let unparsed: BTreeMap<String, u64> = agg
        .unparsed_calls
        .iter()
        .map(|(r, n)| (label(r), *n))
        .collect();
    let writes: BTreeMap<&String, Value> = reading
        .table_writes
        .iter()
        .map(|(t, (i, u, d))| (t, json!({"insert": i, "update": u, "delete": d})))
        .collect();
    out.push(custom_object(
        format!("observation of {database}"),
        WINDOW_KIND,
        &format!("observation-window:{database}"),
        json!({"database": database, "window_start": reading.stats_reset, "window_end": at,
               "truncated": truncated, "dealloc": reading.dealloc, "statements": reading.rows.len(),
               "view": reading.view, "window_note": reading.window_note,
               "unparsed_calls": unparsed, "hidden_calls": agg.hidden_calls,
               "table_writes": writes}),
    ));
    for o in &mut out {
        o.evidence = vec![ev.id];
    }
    (out, ev)
}

/// `<state>/interaction/current.json`: the observed facts of the latest observation per database.
pub fn current_path(config: &EkosConfig, cwd: &Path) -> std::path::PathBuf {
    config
        .ekos_dir(cwd)
        .join("interaction")
        .join("current.json")
}

/// Databases the latest observation found without usage statistics, with the reason (RFC 0256
/// §3.2): surfaces say so instead of showing an empty list. Empty when every database observed
/// gave statistics, or nothing was observed yet.
pub fn unavailable(config: &EkosConfig, cwd: &Path) -> BTreeMap<String, String> {
    let Some(v) = std::fs::read(current_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    else {
        return BTreeMap::new();
    };
    v["databases"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(db, d)| Some((db.clone(), d["unavailable"].as_str()?.to_string())))
        .collect()
}

/// The ids of the current observed facts (`None`: no observation yet).
pub fn current_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_slice(&std::fs::read(current_path(config, cwd)).ok()?).ok()?;
    Some(
        v["databases"]
            .as_object()?
            .values()
            .filter_map(|d| d["ids"].as_array())
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
    )
}

/// `ekos interaction observe [--database] [--allow-writable-role] [--json]`.
pub fn observe(
    config: &EkosConfig,
    cwd: &Path,
    database: Option<&str>,
    allow_writable_role: bool,
    json_out: bool,
) -> Result<()> {
    if config.interaction.roles == ekos_compiler_core::RoleNames::Off {
        anyhow::bail!("[interaction] roles = \"off\": observed access is not read");
    }
    let dbs: Vec<_> = config
        .databases
        .iter()
        .filter(|d| database.is_none_or(|n| n == d.name))
        .collect();
    if dbs.is_empty() {
        anyhow::bail!(
            "no [[databases]] entry{}",
            database.map(|d| format!(" named {d}")).unwrap_or_default()
        );
    }
    let redaction = config.redaction_config();
    let salt = super::relationships::salt(config, cwd)?;
    let ledger = super::store::open_store(config, cwd)?;
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let path = current_path(config, cwd);
    let mut manifest: Value = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({"databases": {}}));
    let mut report = Vec::new();
    let latest_objects = ledger.all_objects()?;
    let previous: HashMap<KirId, &KirObject> = latest_objects
        .iter()
        .filter(|o| is_custom(o, OBSERVED_KIND))
        .map(|o| (o.id, o))
        .collect();
    for db in dbs {
        let reading = match tokio::task::block_in_place(|| read_database(db, allow_writable_role)) {
            Ok(Availability::Read(r)) => r,
            Ok(Availability::Unavailable(why)) => {
                // Recorded, so every surface says this source gives no usage statistics; the
                // database's earlier facts stop being current.
                manifest["databases"][&db.name] = json!({"ids": [], "at": at, "unavailable": why});
                report.push(json!({"database": db.name, "skipped": why}));
                continue;
            }
            Err(e) => {
                // A failed read leaves the previous observation as it was.
                report.push(json!({"database": db.name, "skipped": ekos_common::redaction::redact(&e, &redaction)}));
                continue;
            }
        };
        let dialect = statement_dialect(&db.kind);
        let agg = aggregate_in(dialect.as_ref(), &reading.rows, &reading.me, &redaction);
        let label = |r: &str| role_label(r, config.interaction.roles, &salt, &redaction);
        let (objects, ev) = facts(&db.name, &reading, &agg, &label, &at, &previous);
        ledger.append_evidence(&ev)?;
        for o in &objects {
            ledger.append_object(o)?;
        }
        let ids: Vec<String> = objects.iter().map(|o| o.id.to_string()).collect();
        manifest["databases"][&db.name] = json!({"ids": ids, "at": at});
        let roles: BTreeSet<&str> = agg.cells.keys().map(|(r, _, _)| r.as_str()).collect();
        report.push(json!({
            "database": db.name, "statements": reading.rows.len(), "cells": agg.cells.len(),
            "roles": roles.len(),
            "unparsed_calls": agg.unparsed_calls.values().sum::<u64>(), "hidden_calls": agg.hidden_calls,
            "ddl_calls": agg.ddl_calls,
            "own_calls_left_out": agg.own_calls, "window_start": reading.stats_reset,
            "view": reading.view,
            "truncated": reading.dealloc > 0 || reading.statements_truncated || reading.always_partial,
        }));
    }
    std::fs::create_dir_all(path.parent().expect("a state directory"))?;
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    // §3.4: the roles and findings now, not at the next commit; then the questions they raise.
    let step = commit_step(config, cwd, &*ledger)?;
    let questions = super::questions::commit_step(config, cwd, &*ledger)?;
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"databases": report, "step": step}))?
        );
    } else {
        for r in &report {
            println!("{}", serde_json::to_string(r)?);
        }
        if let Some(st) = &step {
            println!("{}", summary_line(st));
        }
        if let Some(q) = &questions {
            println!("{}", super::questions::summary_line(q));
        }
    }
    Ok(())
}

// ── Roles, findings and the commit step (RFC 0256 §3.3–3.4) ───────────────────────────────────

/// A derived finding.
pub const FINDING_KIND: &str = "InteractionFinding";

/// `<state>/interaction/findings.json`: the `DbRole` items and findings of the latest step.
fn findings_path(config: &EkosConfig, cwd: &Path) -> std::path::PathBuf {
    config
        .ekos_dir(cwd)
        .join("interaction")
        .join("findings.json")
}

fn manifest_ids(config: &EkosConfig, cwd: &Path, key: &str) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_slice(&std::fs::read(findings_path(config, cwd)).ok()?).ok()?;
    Some(
        v[key]
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
    )
}

/// The current `DbRole` review items.
pub fn current_role_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    manifest_ids(config, cwd, "roles")
}

/// The current findings.
pub fn current_finding_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    manifest_ids(config, cwd, "findings")
}

/// PostgreSQL prints `2026-11-11 14:10:50.54464+00`; EKOS writes RFC 3339.
fn parse_time(t: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(t)
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_str(&format!("{t}00"), "%Y-%m-%d %H:%M:%S%.f%z").ok()
        })
        .or_else(|| chrono::DateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S%.f%z").ok())
}

/// The current `ObservationWindow`s, with the observed days computed.
fn windows(objects: &[KirObject], current: &BTreeSet<String>) -> Vec<interaction::Window> {
    objects
        .iter()
        .filter(|o| is_custom(o, WINDOW_KIND) && current.contains(&o.id.to_string()))
        .map(|o| {
            let days = s(o, "window_start")
                .and_then(parse_time)
                .zip(s(o, "window_end").and_then(parse_time))
                .map(|(a, b)| (b - a).num_seconds() as f64 / 86_400.0);
            let table_writes = o
                .properties
                .get("table_writes")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(t, w)| {
                    let n = |k: &str| w[k].as_u64().unwrap_or(0);
                    (t.clone(), (n("insert"), n("update"), n("delete")))
                })
                .collect();
            interaction::Window {
                database: s(o, "database").unwrap_or_default().to_string(),
                days,
                truncated: o
                    .properties
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                table_writes,
            }
        })
        .collect()
}

/// One `DbRole` review item per role seen writing: what it writes (signed), the system proposed
/// for it, and (shown only) its calls and the tables where its writes are unexplained.
fn role_objects(
    m: &Matrix,
    databases: &BTreeMap<String, String>,
    hypotheses: &BTreeMap<String, interaction::Link>,
    unexplained: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<KirObject> {
    let mut writes: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut calls: BTreeMap<&str, u64> = BTreeMap::new();
    for (t, row) in &m.tables {
        for (a, c) in row
            .iter()
            .filter(|(a, _)| a.kind == interaction::ActorKind::DbRole)
        {
            *calls.entry(&a.name).or_default() += c.observed.values().map(|o| o.calls).sum::<u64>();
            for (op, o) in &c.observed {
                if op.is_write() && o.calls > 0 {
                    writes
                        .entry(&a.name)
                        .or_default()
                        .insert(format!("{t}:{}", op.as_str()));
                }
            }
        }
    }
    calls
        .keys()
        .map(|role| {
            let db = databases.get(*role).map(String::as_str).unwrap_or("");
            custom_object(
                (*role).to_string(),
                ROLE_KIND,
                &format!("db-role:{db}:{role}"),
                json!({"database": db, "role": role,
                       "writes": writes.get(role).cloned().unwrap_or_default(),
                       "proposed_actor": hypotheses.get(*role).map(|l| l.actor.key()),
                       "link_basis": hypotheses.get(*role).map(|l| l.basis.as_str()),
                       "calls": calls[role],
                       "unexplained": unexplained.get(*role).cloned().unwrap_or_default()}),
            )
        })
        .collect()
}

/// The people's decisions on the role items: roles accounted for (confirmed, still valid), and the
/// links that hold (a confirmed proposal; a rejected one is dropped).
fn decided(
    objects: &[KirObject],
    relationships: &[ekos_kir::KirRelationship],
    roles: &[KirObject],
    hypotheses: &BTreeMap<String, interaction::Link>,
) -> (BTreeSet<String>, BTreeMap<String, Actor>) {
    use ekos_semantic::review::{self, Inputs, ScopeRules, Subject};
    let ids: BTreeSet<String> = roles.iter().map(|o| o.id.to_string()).collect();
    let mut all: Vec<KirObject> = objects
        .iter()
        .filter(|o| !ids.contains(&o.id.to_string()))
        .cloned()
        .collect();
    all.extend(roles.iter().cloned());
    let rules = ScopeRules::default();
    let items = review::items(
        Subject::Interaction,
        &Inputs {
            objects: &all,
            relationships,
            conflicts_current: None,
            semantics_current: None,
            identity_current: None,
            relationships_current: None,
            interaction_current: Some(&ids),
            rules: &rules,
        },
    );
    let mut accounted = BTreeSet::new();
    let mut links: BTreeMap<String, Actor> = hypotheses
        .iter()
        .map(|(r, l)| (r.clone(), l.actor.clone()))
        .collect();
    for it in items.iter().filter(|i| !i.outdated) {
        let role = it.label.clone();
        match it
            .decision
            .as_ref()
            .and_then(|d| d.get("link"))
            .and_then(Value::as_str)
        {
            Some("confirm") => {
                accounted.insert(role);
            }
            Some("reject") => {
                links.remove(&role);
            }
            _ => {}
        }
    }
    (accounted, links)
}

fn finding_object(f: &interaction::Finding, evidence: KirId) -> KirObject {
    let mut o = custom_object(
        format!("{} on {}", f.kind.as_str(), f.table),
        FINDING_KIND,
        &format!("interaction-finding:{}", f.key()),
        json!({"finding": f.kind.as_str(), "table": f.table, "status": "open",
               "actor": f.actor.as_ref().map(Actor::key),
               "operations": f.operations, "detail": f.detail}),
    );
    o.evidence = vec![evidence];
    o
}

/// Appends `o` unless the ledger's latest version already says the same.
fn append_if_changed(
    ledger: &dyn ekos_ledger::KnowledgeStore,
    latest: &HashMap<KirId, &KirObject>,
    o: &KirObject,
) -> Result<bool> {
    if latest
        .get(&o.id)
        .is_some_and(|p| p.properties == o.properties && p.name == o.name)
    {
        return Ok(false);
    }
    ledger.append_object(o)?;
    Ok(true)
}

/// What one step derived.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StepSummary {
    pub roles: usize,
    pub accounted: usize,
    pub findings: BTreeMap<&'static str, usize>,
}

/// One line for the terminal.
pub fn summary_line(s: &StepSummary) -> String {
    let parts: Vec<String> = s.findings.iter().map(|(k, n)| format!("{n} {k}")).collect();
    format!(
        "Interaction: {} writing role(s), {} accounted for; findings: {} — `ekos interaction findings`",
        s.roles,
        s.accounted,
        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(", ")
        }
    )
}

/// RFC 0256 §3.4, run at the end of `ekos commit` and of `ekos interaction observe`: role items,
/// findings and their manifest. Nothing happens before a first observation — no finding is ever
/// derived from absence.
pub fn commit_step(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn ekos_ledger::KnowledgeStore,
) -> Result<Option<StepSummary>> {
    let Some(current) = current_ids(config, cwd) else {
        return Ok(None);
    };
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    let (m, _) = build_from(config, cwd, ledger, &objects, &relationships)?;
    let windows = windows(&objects, &current);
    let databases: BTreeMap<String, String> = objects
        .iter()
        .filter(|o| is_custom(o, OBSERVED_KIND) && current.contains(&o.id.to_string()))
        .filter_map(|o| Some((s(o, "role")?.to_string(), s(o, "database")?.to_string())))
        .collect();
    // §3.3: a role that is a Kettle connection's user is proposed as that job first. Users are
    // shown as roles are (hashed when roles are), so they compare with the matrix's role names.
    let redaction = config.redaction_config();
    let salt = super::relationships::salt(config, cwd)?;
    let users: BTreeMap<String, BTreeSet<Actor>> = interaction::connection_users(&objects)
        .into_iter()
        .map(|(u, a)| {
            (
                role_label(&u, config.interaction.roles, &salt, &redaction),
                a,
            )
        })
        .collect();
    let hypotheses = interaction::link_hypotheses_with(&m, &users);
    // First pass without decisions, to find each role's item as it stands, then the decisions on
    // those items, then the findings that remain.
    let draft = role_objects(&m, &databases, &hypotheses, &BTreeMap::new());
    let (accounted, links) = decided(&objects, &relationships, &draft, &hypotheses);
    let input = interaction::FindingInput {
        windows: &windows,
        accounted: &accounted,
        links: &links,
        quiet_after_days: config.interaction.quiet_after_days,
    };
    let mut found = interaction::findings(&m, &input);
    // §3.4: who only reads each table — the list of who a change breaks, no question.
    found.extend(interaction::read_only_consumers(&m));
    let mut unexplained: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in found
        .iter()
        .filter(|f| f.kind == interaction::FindingKind::UnexplainedWriter)
    {
        if let Some(a) = &f.actor {
            unexplained
                .entry(a.name.clone())
                .or_default()
                .insert(f.table.clone());
        }
    }
    let roles: Vec<KirObject> = role_objects(&m, &databases, &hypotheses, &unexplained)
        .into_iter()
        .filter(|o| {
            o.properties
                .get("writes")
                .and_then(Value::as_array)
                .is_some_and(|w| !w.is_empty())
        })
        .collect();
    let latest: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let evidence = windows
        .first()
        .map(|w| kid(&format!("observation:{}:evidence", w.database)));
    for o in &roles {
        let mut o = o.clone();
        o.evidence = evidence.into_iter().collect();
        append_if_changed(ledger, &latest, &o)?;
    }
    let mut finding_ids = Vec::new();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    if let Some(ev) = evidence {
        for f in &found {
            let o = finding_object(f, ev);
            append_if_changed(ledger, &latest, &o)?;
            finding_ids.push(o.id.to_string());
            *counts.entry(f.kind.as_str()).or_default() += 1;
        }
    }
    let path = findings_path(config, cwd);
    std::fs::create_dir_all(path.parent().expect("a state directory"))?;
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "roles": roles.iter().map(|o| o.id.to_string()).collect::<Vec<_>>(),
            "findings": finding_ids,
        }))?,
    )?;
    Ok(Some(StepSummary {
        roles: roles.len(),
        accounted: accounted.len(),
        findings: counts,
    }))
}

/// The current findings, through `ledger` (a restricted reader sees only tables in scope).
pub fn current_findings(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
) -> Result<Vec<KirObject>> {
    let Some(ids) = current_finding_ids(config, cwd) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<KirObject> = ledger
        .all_objects()?
        .into_iter()
        .filter(|o| is_custom(o, FINDING_KIND) && ids.contains(&o.id.to_string()))
        .collect();
    out.sort_by(|a, b| (s(a, "finding"), s(a, "table")).cmp(&(s(b, "finding"), s(b, "table"))));
    Ok(out)
}

fn finding_words(o: &KirObject) -> String {
    let actor = s(o, "actor").unwrap_or("-");
    let ops: Vec<&str> = o
        .properties
        .get("operations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let detail = o.properties.get("detail").cloned().unwrap_or(Value::Null);
    format!(
        "{:<26} {:<24} {:<36} {:<16} {}",
        s(o, "finding").unwrap_or(""),
        s(o, "table").unwrap_or(""),
        actor,
        ops.join(","),
        detail
    )
}

/// `ekos interaction findings [--json]`.
pub fn findings(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let list = current_findings(config, cwd, &*ledger)?;
    if json_out {
        let v: Vec<Value> = list
            .iter()
            .map(|o| {
                json!({"id": o.id.to_string(), "finding": s(o, "finding"), "table": s(o, "table"),
                            "actor": s(o, "actor"), "operations": o.properties.get("operations"),
                            "detail": o.properties.get("detail")})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    for (db, why) in unavailable(config, cwd) {
        println!("{db}: {why} — no finding is derived from its absence.");
    }
    if current_ids(config, cwd).is_none() {
        println!(
            "No observation yet (`ekos interaction observe`): without usage statistics no finding \
             is derived — absence of an observation is not evidence."
        );
        return Ok(());
    }
    if list.is_empty() {
        println!("No findings.");
    }
    for o in &list {
        println!("{}", finding_words(o));
    }
    Ok(())
}

/// Tables an MCP answer lists at most without a `table` filter.
const AGENT_TABLES: usize = 50;

/// MCP `ekos_interaction` (read-only, RFC 0256 §3.5): the matrix and the current findings through
/// the caller's store — a restricted reader sees only tables its scope returns.
pub fn agent_matrix(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let (mut m, observed) = build(config, cwd, ledger)?;
    filter(
        &mut m,
        args.get("table").and_then(Value::as_str),
        args.get("actor").and_then(Value::as_str),
    );
    let total = m.tables.len();
    if args.get("table").is_none() && total > AGENT_TABLES {
        let keep: BTreeSet<String> = m.tables.keys().take(AGENT_TABLES).cloned().collect();
        m.retain_tables(|t| keep.contains(t));
    }
    let tables: BTreeSet<&String> = m.tables.keys().collect();
    let findings: Vec<Value> = current_findings(config, cwd, ledger)?
        .iter()
        .filter(|o| s(o, "table").is_some_and(|t| tables.contains(&t.to_string())))
        .map(|o| json!({"finding": s(o, "finding"), "table": s(o, "table"), "actor": s(o, "actor"),
                        "operations": o.properties.get("operations"), "detail": o.properties.get("detail")}))
        .collect();
    let mut v = to_json(&m, observed, &unavailable(config, cwd));
    v["findings"] = json!(findings);
    v["tables_total"] = json!(total);
    if m.tables.len() < total {
        v["truncated"] = json!(format!(
            "{} of {total} tables shown; pass `table` for one",
            m.tables.len()
        ));
    }
    v["note"] = json!(
        "Declared access is stated by code (routines, Java components, Kettle jobs); observed access \
         is a database role's aggregated statement statistics over a dated window. A finding is \
         derived, not a decision: only a person accounts for a role (`ekos review interaction`)."
    );
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_common::redaction::RedactionConfig;
    use ekos_compiler_core::RoleNames;

    fn row(role: &str, query: &str, calls: u64) -> StatementRow {
        StatementRow {
            role: role.into(),
            query: query.into(),
            calls,
            rows: calls,
        }
    }

    #[test]
    fn oracle_and_clickhouse_statements_are_parsed_in_their_own_dialect() {
        let none = RedactionConfig::default();
        // Oracle: upper-case names, positional and named bind variables, the reading user left out.
        let ora = [
            row(
                "ETL_NIGHTLY",
                "UPDATE ADEMPIERE.C_BPARTNER SET NAME = :1 WHERE C_BPARTNER_ID = :2",
                4,
            ),
            row(
                "APP",
                "INSERT INTO C_ORDER (C_ORDER_ID, DOCSTATUS) VALUES (:B1, :B2)",
                9,
            ),
            row(
                "APP",
                "SELECT DOCSTATUS FROM C_ORDER WHERE C_ORDER_ID = :id",
                2,
            ),
            row("EKOS_READER", "SELECT COUNT(*) FROM C_ORDER", 1),
            row("APP", "SELECT 1 FROM DUAL", 50),
        ];
        let generic = sqlparser::dialect::GenericDialect {};
        let a = aggregate_in(&generic, &ora, "ekos_reader", &none);
        assert_eq!(
            a.cells[&("ETL_NIGHTLY".into(), "c_bpartner".into(), Op::Update)],
            (4, 4)
        );
        assert_eq!(
            a.cells[&("APP".into(), "c_order".into(), Op::Insert)],
            (9, 9)
        );
        assert_eq!(a.cells[&("APP".into(), "c_order".into(), Op::Read)], (2, 2));
        assert_eq!(
            a.own_calls, 1,
            "the reading user's own statements, whatever their case"
        );
        assert!(!a.cells.keys().any(|(_, t, _)| t == "dual"));
        // ClickHouse: the sampled statement of a group, literals included, parsed and dropped; an
        // insert logged without its rows still names its table; schema statements count apart.
        let ch = [
            row("loader", "INSERT INTO rfc.events VALUES ", 5),
            row("loader", "INSERT INTO rfc.events (id, kind) FORMAT CSV", 2),
            row(
                "ops",
                "CREATE TABLE rfc.kinds (kind String) ENGINE = MergeTree ORDER BY kind",
                1,
            ),
            row("ops", "ALTER TABLE rfc.kinds UPDATE kind = 'x' WHERE 1", 1),
            row(
                "loader",
                "INSERT INTO events (id, kind) VALUES (1, 'a'), (2, 'b')",
                12,
            ),
            row(
                "bi",
                "SELECT kind, count() FROM default.events GROUP BY kind",
                3,
            ),
        ];
        let a = aggregate_in(
            &sqlparser::dialect::ClickHouseDialect {},
            &ch,
            "ekos",
            &none,
        );
        assert_eq!(
            a.cells[&("loader".into(), "events".into(), Op::Insert)],
            (19, 19)
        );
        assert_eq!(a.ddl_calls, 1, "the CREATE, not the mutation");
        assert_eq!(a.cells[&("bi".into(), "events".into(), Op::Read)], (3, 3));
    }

    #[test]
    fn an_observed_cell_keeps_when_it_was_first_seen_and_counts_since_the_last_read() {
        let first = carried(None, Some("2026-11-01"), 5, "2026-11-10T00:00:00Z");
        assert_eq!(first["first_observed"], "2026-11-10T00:00:00Z");
        let mut prev = KirObject::new("p", ObjectKind::Custom(OBSERVED_KIND.into()));
        prev.properties = json!({"first_observed": "2026-11-10T00:00:00Z", "calls": 5,
                                 "window_start": "2026-11-01", "window_end": "2026-11-10T00:00:00Z"})
        .as_object()
        .unwrap()
        .clone()
        .into_iter()
        .collect();
        let next = carried(Some(&prev), Some("2026-11-01"), 8, "2026-11-12T00:00:00Z");
        assert_eq!(next["first_observed"], "2026-11-10T00:00:00Z");
        assert_eq!(next["calls_since_previous"], 3);
        assert_eq!(next["previous_observation"], "2026-11-10T00:00:00Z");
        // A reset in between: the first sighting stays, the delta is unknown, never negative.
        let reset = carried(Some(&prev), Some("2026-11-11"), 2, "2026-11-12T00:00:00Z");
        assert_eq!(reset["first_observed"], "2026-11-10T00:00:00Z");
        assert!(reset["calls_since_previous"].is_null());
    }

    #[test]
    fn the_clickhouse_query_log_is_read_for_the_named_databases_with_quotes_escaped() {
        assert!(ch_statements(&[]).contains("has(databases, currentDatabase())"));
        let q = ch_statements(&["shop".into(), "o'k".into()]);
        assert!(q.contains("hasAny(databases, ['shop', 'o\\'k'])"), "{q}");
        ekos_sqlgen::guard::single_select(&q, &sqlparser::dialect::ClickHouseDialect {}).unwrap();
    }

    #[test]
    fn code_the_latest_compile_no_longer_holds_declares_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let mut kept = KirObject::new("load.ktr:0", ObjectKind::Custom("TransformNode".into()));
        kept.id = kid("kept");
        let mut gone = KirObject::new("old.ktr:0", ObjectKind::Custom("TransformNode".into()));
        gone.id = kid("gone");
        let table = KirObject::new("customers", ObjectKind::Table);
        let all = vec![kept.clone(), gone.clone(), table.clone()];
        assert!(
            current_code(&config, dir.path(), &all).is_none(),
            "no model: everything counts"
        );
        let ckm = config.ekos_dir(dir.path()).join("ckm");
        std::fs::create_dir_all(&ckm).unwrap();
        let model = json!({"version": 1, "compiled_at": "2026-11-12T00:00:00Z", "relationships": [],
            "evidence_index": {}, "objects": [{"id": kept.id, "name": kept.name, "kind": "TransformNode"}]});
        std::fs::write(ckm.join("model.json"), serde_json::to_vec(&model).unwrap()).unwrap();
        let names: Vec<String> = current_code(&config, dir.path(), &all)
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(
            names,
            ["load.ktr:0", "customers"],
            "the table is not code: kept"
        );
    }

    #[test]
    fn a_source_without_usage_statistics_is_said_so_on_every_surface() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let path = current_path(&config, dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let why = no_usage_statistics(&ekos_compiler_core::DatabaseKind::Sqlserver).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(
                &json!({"databases": {"erp": {"ids": [], "at": "t", "unavailable": why}}}),
            )
            .unwrap(),
        )
        .unwrap();
        let none = unavailable(&config, dir.path());
        assert_eq!(none["erp"], why);
        // An observation exists (so findings run) but holds no fact: nothing is derived from it.
        assert_eq!(current_ids(&config, dir.path()), Some(BTreeSet::new()));
        let v = to_json(&Matrix::default(), 0, &none);
        assert_eq!(v["no_usage_statistics"]["erp"], why);
        let note = v["observed_note"].as_str().unwrap();
        assert!(note.contains("provides no usage statistics") && note.contains("erp:"));
        assert!(no_usage_statistics(&ekos_compiler_core::DatabaseKind::Postgres).is_none());
    }

    #[test]
    fn the_engine_view_and_its_window_are_in_the_evidence_and_an_oracle_reading_is_partial() {
        let reading = Reading {
            view: "V$SQL",
            window_note: "the instance's start-up",
            always_partial: true,
            ..Default::default()
        };
        let mut agg = Aggregate::default();
        agg.cells
            .insert(("ETL".into(), "c_order".into(), Op::Update), (2, 2));
        let (objects, ev) = facts(
            "idem",
            &reading,
            &agg,
            &|r: &str| r.to_string(),
            "2026-11-12T00:00:00Z",
            &HashMap::new(),
        );
        assert_eq!(ev.location.path, "observe://idem/V$SQL");
        assert!(objects.iter().all(|o| o.properties["truncated"] == true));
        assert_eq!(
            objects[0].properties["first_observed"],
            "2026-11-12T00:00:00Z"
        );
    }

    #[test]
    fn statements_become_role_table_operation_counts() {
        let rows = [
            row("app", "INSERT INTO public.orders VALUES ($1, $2)", 20),
            row("app", "UPDATE orders SET status = $1 WHERE id = $2", 5),
            row("etl", "UPDATE customers SET status = $1", 3),
            row(
                "bi",
                "SELECT count(*) FROM orders o JOIN customers c ON c.id = o.cid",
                7,
            ),
            row("ekos_ro", "SELECT * FROM orders", 9),
            row("bi", "SELECT * FROM pg_class", 4),
            row("dba", "THIS IS NOT SQL", 2),
            row("app", HIDDEN_TEXT, 6),
        ];
        let a = aggregate(&rows, "ekos_ro", &RedactionConfig::default());
        let get = |r: &str, t: &str, op: Op| a.cells.get(&(r.into(), t.into(), op)).map(|c| c.0);
        assert_eq!(get("app", "orders", Op::Insert), Some(20));
        assert_eq!(get("app", "orders", Op::Update), Some(5));
        assert_eq!(get("etl", "customers", Op::Update), Some(3));
        assert_eq!(get("bi", "orders", Op::Read), Some(7));
        assert_eq!(get("bi", "customers", Op::Read), Some(7));
        assert_eq!(a.own_calls, 9, "EKOS's own sessions never appear");
        assert!(
            a.cells
                .keys()
                .all(|(r, t, _)| r != "ekos_ro" && !t.starts_with("pg_"))
        );
        assert_eq!(a.unparsed_calls.get("dba"), Some(&2));
        assert_eq!(a.hidden_calls, 6);
    }

    #[test]
    fn hashed_roles_stay_distinct_and_hide_the_name() {
        let r = RedactionConfig::default();
        let a = role_label("alice", RoleNames::Hashed, "salt", &r);
        let b = role_label("bob", RoleNames::Hashed, "salt", &r);
        assert!(a.starts_with("role-") && !a.contains("alice"));
        assert_ne!(a, b);
        assert_eq!(a, role_label("alice", RoleNames::Hashed, "salt", &r));
        assert_eq!(role_label("app", RoleNames::Names, "salt", &r), "app");
    }

    #[test]
    fn an_observation_that_is_not_current_is_not_counted() {
        let mut o = KirObject::new("x", ObjectKind::Custom(OBSERVED_KIND.into()));
        for (k, v) in [
            ("table", "orders"),
            ("role", "app"),
            ("operation", "update"),
        ] {
            o.properties.insert(k.into(), json!(v));
        }
        let mut m = Matrix::default();
        assert_eq!(add_observed(&mut m, std::slice::from_ref(&o), None), 0);
        assert_eq!(
            add_observed(&mut m, std::slice::from_ref(&o), Some(&BTreeSet::new())),
            0
        );
        let cur = BTreeSet::from([o.id.to_string()]);
        assert_eq!(add_observed(&mut m, &[o], Some(&cur)), 1);
    }

    #[test]
    fn observed_access_to_a_table_the_reader_cannot_see_is_dropped() {
        let mut m = Matrix::default();
        let seen = Observed {
            calls: 3,
            ..Default::default()
        };
        m.add_observed("customers", "etl", Op::Update, seen.clone());
        m.add_observed("orders", "etl", Op::Update, seen);
        // The scoped store returned `orders` only.
        let visible = [KirObject::new("orders", ObjectKind::Table)];
        retain_visible(&mut m, &visible);
        assert_eq!(m.tables.keys().collect::<Vec<_>>(), ["orders"]);
    }
}
