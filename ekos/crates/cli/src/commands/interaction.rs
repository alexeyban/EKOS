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
    Ok((m, observed))
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
pub fn to_json(m: &Matrix, observed: usize) -> Value {
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
        "observed_note": observed_note(observed),
    })
}

/// What "observed" means for this project right now, said in words (RFC 0256 §3.2: an empty
/// observation is never shown as "nobody writes").
fn observed_note(observed: usize) -> &'static str {
    if observed == 0 {
        "No observed access recorded: the source provides no usage statistics, or they were not \
         read. Only declared access is shown — absence is not evidence that nothing else writes."
    } else {
        "Observed access is aggregated from the engine's statement statistics over a dated, \
         possibly truncated window: \"not observed\" is not \"never happens\"."
    }
}

fn print(m: &Matrix, observed: usize) {
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
        observed_note(observed)
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
    if json_out {
        println!("{}", serde_json::to_string_pretty(&to_json(&m, observed))?);
    } else {
        print(&m, observed);
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
}

fn is_system(table: &str) -> bool {
    table.starts_with("pg_") || table == "information_schema"
}

/// Aggregates statement rows: each statement's text is redacted, parsed for the tables it reads
/// and writes (RFC 0163's footprint), counted, and dropped. `own_role`'s statements and those
/// touching only system catalogs are left out.
pub fn aggregate(
    rows: &[StatementRow],
    own_role: &str,
    redaction: &ekos_common::redaction::RedactionConfig,
) -> Aggregate {
    let mut a = Aggregate::default();
    for r in rows {
        if r.role == own_role {
            a.own_calls += r.calls;
            continue;
        }
        if r.query.trim() == HIDDEN_TEXT {
            a.hidden_calls += r.calls;
            continue;
        }
        let text = ekos_common::redaction::redact(&r.query, redaction);
        let fp = ekos_recovery::plpgsql_footprint::statement_footprint_in(
            &sqlparser::dialect::PostgreSqlDialect {},
            &text,
        );
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
}

fn num(v: &Option<String>) -> u64 {
    v.as_deref().and_then(|x| x.parse().ok()).unwrap_or(0)
}

/// Reads one PostgreSQL database's statement and table statistics in the read-only session.
fn read_database(
    db: &ekos_compiler_core::DatabaseConfig,
    allow_writable_role: bool,
) -> std::result::Result<Option<Reading>, String> {
    use ekos_sqlgen::probe::Probe as _;
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
    let first = |sql: &str| -> std::result::Result<Option<String>, String> {
        Ok(w.preview(sql, 1)?
            .rows
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next())
            .flatten())
    };
    if first(SQL_EXTENSION)?.as_deref() != Some("1") {
        return Ok(None);
    }
    let mut r = Reading {
        me: first(SQL_ME)?.unwrap_or_default(),
        ..Default::default()
    };
    let st = w.preview(SQL_STATEMENTS, MAX_STATEMENTS)?;
    r.statements_truncated = st.truncated;
    for row in st.rows {
        let [role, query, calls, rows] = <[Option<String>; 4]>::try_from(row)
            .map_err(|_| "pg_stat_statements: unexpected columns".to_string())?;
        r.rows.push(StatementRow {
            role: role.unwrap_or_default(),
            query: query.unwrap_or_default(),
            calls: num(&calls),
            rows: num(&rows),
        });
    }
    // `pg_stat_statements_info` exists from PostgreSQL 14; without it the window start is unknown.
    if let Ok(info) = w.preview(SQL_INFO, 1)
        && let Some(row) = info.rows.into_iter().next()
    {
        r.stats_reset = row.first().cloned().flatten();
        r.dealloc = num(&row.get(1).cloned().flatten());
    }
    for row in w.preview(SQL_TABLES, MAX_STATEMENTS)?.rows {
        let get = |i: usize| row.get(i).cloned().flatten();
        if let Some(t) = get(0) {
            r.table_writes.insert(
                interaction::table_key(&t),
                (num(&get(1)), num(&get(2)), num(&get(3))),
            );
        }
    }
    Ok(Some(r))
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
fn facts(
    database: &str,
    reading: &Reading,
    agg: &Aggregate,
    label: &dyn Fn(&str) -> String,
    at: &str,
) -> (Vec<KirObject>, ekos_kir::KirEvidence) {
    let mut ev = ekos_kir::KirEvidence::new(
        ekos_kir::SourceLocation {
            path: format!("observe://{database}/pg_stat_statements"),
            line: None,
            column: None,
        },
        format!(
            "{database}: statement statistics aggregated per role, table and operation at {at} (RFC 0256); \
             no statement text kept"
        ),
    );
    ev.id = kid(&format!("observation:{database}:evidence"));
    let truncated = reading.dealloc > 0 || reading.statements_truncated;
    let mut out = Vec::new();
    for ((role, table, op), (calls, rows)) in &agg.cells {
        let shown = label(role);
        out.push(custom_object(
            format!("{shown} {} {table}", op.as_str()),
            OBSERVED_KIND,
            &format!("observed-access:{database}:{shown}:{table}:{}", op.as_str()),
            json!({"database": database, "role": shown, "table": table, "operation": op.as_str(),
                   "calls": calls, "rows": rows, "window_start": reading.stats_reset,
                   "window_end": at, "truncated": truncated, "status": "observed"}),
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
    for db in dbs {
        if db.kind != ekos_compiler_core::DatabaseKind::Postgres {
            report.push(json!({"database": db.name, "skipped": "this engine provides no usage statistics EKOS reads (RFC 0256: PostgreSQL first)"}));
            continue;
        }
        let reading = match tokio::task::block_in_place(|| read_database(db, allow_writable_role)) {
            Ok(Some(r)) => r,
            Ok(None) => {
                report.push(json!({"database": db.name, "skipped": "pg_stat_statements is not installed: this source provides no usage statistics"}));
                continue;
            }
            Err(e) => {
                report.push(json!({"database": db.name, "skipped": ekos_common::redaction::redact(&e, &redaction)}));
                continue;
            }
        };
        let agg = aggregate(&reading.rows, &reading.me, &redaction);
        let label = |r: &str| role_label(r, config.interaction.roles, &salt, &redaction);
        let (objects, ev) = facts(&db.name, &reading, &agg, &label, &at);
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
            "own_calls_left_out": agg.own_calls, "window_start": reading.stats_reset,
            "truncated": reading.dealloc > 0 || reading.statements_truncated,
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
    hypotheses: &BTreeMap<String, Actor>,
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
                       "proposed_actor": hypotheses.get(*role).map(Actor::key),
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
    hypotheses: &BTreeMap<String, Actor>,
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
    let mut links = hypotheses.clone();
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
    let hypotheses = interaction::link_hypotheses(&m);
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
    let found = interaction::findings(&m, &input);
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
}
