//! RFC 0252 Phase 1: `ekos profile run|show` — the analyst's signals, measured on the live
//! databases after a commit.
//!
//! `run` reads what the commit knows — the values code names per column (current `EnumMeaning`s
//! and unexplained-value gaps) and the live tables — and, per `[[databases]]` entry whose
//! `profile` is p1 or p2:
//!
//! - literal coverage and first/last month per code value (`ekos_observation_sdk::signals`);
//! - each table's grain: its declared primary key, or a levelwise key search.
//!
//! It writes `<state>/profile/current.json` (counts only; the only values in it are literals from
//! code) and one keyed `GrainCandidate` per table, then re-runs the semantics step so every
//! `EnumMeaning` carries `data_usage` and every column with values no code names gets a
//! `data_only_values` gap. Every query goes through the same read-only probe session as
//! `ekos sql check --database` ([`super::sql_check::ProbeWorker`]).

use super::sql_check::{ProbeWorker, Target};
use anyhow::{Context, Result, bail};
use ekos_common::redaction::{RedactionConfig, redact};
use ekos_compiler_core::{DatabaseConfig, DatabaseKind, DatabaseProfile, EkosConfig};
use ekos_kir::{
    KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
};
use ekos_ledger::KnowledgeStore;
use ekos_observation_sdk::profile::Flavor;
use ekos_observation_sdk::signals::{self, Coverage};
use ekos_semantic::business_semantics::{self, ENUM_MEANING, GAP};
use ekos_semantic::data_usage::{
    ColumnSignal, DataSignals, GrainSignal, HistorySignal, ImplicationSignal,
};
use ekos_sqlgen::probe::Probe;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The keyed kind a table's grain is written as.
pub const GRAIN: &str = "GrainCandidate";

/// Columns tried as key parts, per table.
const KEY_CANDIDATES: usize = 8;
/// Widest composite key searched (RFC 0252 `key_max_width`).
const KEY_MAX_WIDTH: usize = 3;
/// Key queries per table.
const KEY_MAX_QUERIES: usize = 24;

pub struct RunOpts<'a> {
    pub database: Option<&'a str>,
    /// Which signals: `literal`, `temporal`, `keys` (empty: all).
    pub signals: Vec<String>,
    /// Skip tables whose estimated rows exceed this (0: no limit).
    pub max_rows: i64,
    pub dead_after_months: i64,
    pub allow_writable_role: bool,
    pub json: bool,
}

pub fn current_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("profile").join("current.json")
}

/// The latest run's signals, if any.
pub fn load(config: &EkosConfig, cwd: &Path) -> Option<DataSignals> {
    let bytes = std::fs::read(current_path(config, cwd)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The profile dialect of a database kind.
pub(crate) fn flavor_of(kind: DatabaseKind) -> Flavor {
    flavor(kind)
}

/// The `GrainCandidate` ids the latest run wrote; `None` before the first run.
pub fn grain_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_slice(&std::fs::read(current_path(config, cwd)).ok()?).ok()?;
    Some(
        v["grain_ids"]
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
    )
}

/// The values code names for one column (case-insensitive table and column).
pub(crate) fn code_literals_for(items: &[KirObject], table: &str, column: &str) -> Vec<String> {
    code_literals(items)
        .remove(&(tail(table), column.to_lowercase()))
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn flavor(kind: DatabaseKind) -> Flavor {
    match kind {
        DatabaseKind::Oracle => Flavor::Oracle,
        DatabaseKind::Postgres => Flavor::Postgres,
        DatabaseKind::Clickhouse => Flavor::ClickHouse,
        DatabaseKind::Sqlserver => Flavor::SqlServer,
    }
}

fn s<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

fn tail(n: &str) -> String {
    n.rsplit('.').next().unwrap_or(n).to_lowercase()
}

/// Values code names, per (table, column), both lowercase: current `EnumMeaning`s and
/// unexplained-value gaps.
fn code_literals(items: &[KirObject]) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut out: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for o in items {
        let k = business_semantics::kind_name(o);
        let wanted =
            k == Some(ENUM_MEANING) || (k == Some(GAP) && s(o, "gap_type") == "unexplained_value");
        if !wanted || s(o, "value").is_empty() {
            continue;
        }
        out.entry((tail(s(o, "table")), s(o, "column").to_lowercase()))
            .or_default()
            .insert(s(o, "value").to_string());
    }
    out
}

/// The live tables of `database`, sorted by (owner, name).
fn live_tables<'a>(objects: &'a [KirObject], database: &str) -> Vec<&'a KirObject> {
    let mut v: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .filter(|o| s(o, "origin") == "live" && s(o, "database") == database)
        .collect();
    v.sort_by(|a, b| (s(a, "owner"), &a.name).cmp(&(s(b, "owner"), &b.name)));
    v.dedup_by(|a, b| a.id == b.id);
    v
}

fn columns(t: &KirObject) -> &[Value] {
    t.properties
        .get("columns")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// Rows the catalog or a profile reported, if any.
fn known_rows(t: &KirObject) -> Option<i64> {
    t.properties
        .get("profile")
        .and_then(|p| p["row_count"].as_i64())
        .or_else(|| t.properties.get("num_rows").and_then(Value::as_i64))
}

fn number(p: &dyn Probe, sql: &str) -> Result<i64, String> {
    let r = p.preview(sql, 1)?;
    r.single_number()
        .and_then(|v| v.first().copied())
        .ok_or_else(|| "an aggregate query returned no number".to_string())
}

/// Measures one database. Errors per table are kept (redacted) and the run goes on.
struct Measure<'a> {
    db: &'a DatabaseConfig,
    flavor: Flavor,
    probe: &'a dyn Probe,
    redaction: &'a RedactionConfig,
    tier: &'static str,
    errors: Vec<Value>,
}

impl Measure<'_> {
    fn fail(&mut self, owner: &str, table: &str, what: &str, e: &str) {
        self.errors.push(json!({
            "database": self.db.name, "owner": owner, "table": table, "signal": what,
            "error": redact(e, self.redaction),
        }));
    }

    fn coverage(
        &mut self,
        t: &KirObject,
        column: &str,
        literals: &[String],
        temporal: bool,
    ) -> Option<ColumnSignal> {
        let (owner, table) = (s(t, "owner"), t.name.as_str());
        let time = temporal
            .then(|| signals::time_column(self.flavor, columns(t)))
            .flatten();
        let sql = signals::literal_coverage_sql(
            self.flavor,
            owner,
            table,
            column,
            literals,
            time.as_deref(),
        );
        let rows = sql.and_then(|q| self.probe.preview(&q, 10_000).map(|p| p.rows));
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                self.fail(owner, table, "literal", &e);
                return None;
            }
        };
        let mut cov: Coverage = signals::parse_coverage(literals, &rows);
        if cov.unexplained.rows > 0 {
            match signals::unexplained_distinct_sql(self.flavor, owner, table, column, literals)
                .and_then(|q| number(self.probe, &q))
            {
                Ok(n) => cov.unexplained_distinct = Some(n),
                Err(e) => self.fail(owner, table, "literal", &e),
            }
        }
        Some(ColumnSignal {
            database: self.db.name.clone(),
            owner: owner.to_string(),
            table: table.to_string(),
            column: column.to_string(),
            tier: self.tier.into(),
            time_column: time,
            coverage: cov,
        })
    }

    fn grain(&mut self, t: &KirObject) -> Option<GrainSignal> {
        let (owner, table) = (s(t, "owner"), t.name.as_str());
        let declared: Option<Vec<String>> = t
            .properties
            .get("primary_key")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty());
        let rows = match known_rows(t).filter(|_| declared.is_some()) {
            Some(n) => n,
            None => {
                match rows_sql(self.flavor, owner, table).and_then(|q| number(self.probe, &q)) {
                    Ok(n) => n,
                    Err(e) => {
                        self.fail(owner, table, "keys", &e);
                        return None;
                    }
                }
            }
        };
        let search = declared.is_none().then(|| {
            let cands = signals::key_candidates(self.flavor, columns(t), KEY_CANDIDATES);
            let (flavor, probe) = (self.flavor, self.probe);
            let mut ask = |set: &[String]| {
                signals::distinct_tuples_sql(flavor, owner, table, set)
                    .and_then(|q| number(probe, &q))
            };
            let mut r =
                signals::search_keys(&cands, rows, KEY_MAX_WIDTH, KEY_MAX_QUERIES, &mut ask);
            r.errors = r.errors.iter().map(|e| redact(e, self.redaction)).collect();
            r
        });
        Some(GrainSignal {
            database: self.db.name.clone(),
            owner: owner.to_string(),
            table: table.to_string(),
            rows,
            declared,
            search,
        })
    }
}

fn rows_sql(flavor: Flavor, owner: &str, table: &str) -> Result<String, String> {
    let count = match flavor {
        Flavor::ClickHouse => "count()",
        _ => "COUNT(*)",
    };
    Ok(format!(
        "SELECT {count} FROM {}",
        ekos_observation_sdk::profile::table_ref(flavor, owner, table)?
    ))
}

fn tier_name(p: DatabaseProfile) -> Option<&'static str> {
    match p {
        DatabaseProfile::P0 => None,
        DatabaseProfile::P1 => Some("p1"),
        DatabaseProfile::P2 => Some("p2"),
    }
}

fn wants(opts: &RunOpts<'_>, signal: &str) -> bool {
    opts.signals.is_empty() || opts.signals.iter().any(|s| s == signal)
}

/// What one database run measured.
#[derive(Default)]
struct Measured {
    columns: Vec<ColumnSignal>,
    grain: Vec<GrainSignal>,
    implications: Vec<ImplicationSignal>,
    history: Vec<HistorySignal>,
    errors: Vec<Value>,
}

/// Coded columns with at most this many values are conditioned on (RFC `conditional_max_distinct`).
const CONDITIONAL_MAX_VALUES: usize = 32;
/// Columns tested for nulls per conditioned column (RFC `conditional_max_pairs`, per X).
const CONDITIONAL_MAX_TARGETS: usize = 40;

impl Measure<'_> {
    fn conditional(
        &mut self,
        t: &KirObject,
        column: &str,
        literals: &[String],
    ) -> Vec<ImplicationSignal> {
        let (owner, table) = (s(t, "owner"), t.name.as_str());
        if literals.len() > CONDITIONAL_MAX_VALUES
            || ekos_common::pii::classify_name(column).is_some()
        {
            return Vec::new();
        }
        let ys = signals::conditional_targets(columns(t), column, CONDITIONAL_MAX_TARGETS);
        if ys.is_empty() {
            return Vec::new();
        }
        let rows = signals::conditional_sql(self.flavor, owner, table, column, literals, &ys)
            .and_then(|q| self.probe.preview(&q, 10_000).map(|p| p.rows));
        match rows {
            Ok(rows) => signals::parse_implications(literals, &ys, &rows)
                .into_iter()
                .map(|i| ImplicationSignal {
                    database: self.db.name.clone(),
                    owner: owner.into(),
                    table: table.into(),
                    column: column.into(),
                    value: i.value,
                    then_column: i.then_column,
                    then: i.then,
                    support: i.support,
                })
                .collect(),
            Err(e) => {
                self.fail(owner, table, "conditional", &e);
                Vec::new()
            }
        }
    }

    fn history(&mut self, t: &KirObject) -> Vec<HistorySignal> {
        let (owner, table) = (s(t, "owner"), t.name.as_str());
        let mut out = Vec::new();
        for p in signals::history_patterns(self.flavor, columns(t)) {
            let r = signals::history_sql(self.flavor, owner, table, &p)
                .and_then(|q| self.probe.preview(&q, 1))
                .and_then(|r| r.single_number().ok_or_else(|| "no counts".to_string()));
            match r {
                Ok(n) if n.len() == 2 => out.push(HistorySignal {
                    database: self.db.name.clone(),
                    owner: owner.into(),
                    table: table.into(),
                    kind: p.kind,
                    columns: p.columns,
                    rows: n[0],
                    measured: n[1],
                }),
                Ok(_) => {}
                Err(e) => self.fail(owner, table, "history", &e),
            }
        }
        out
    }
}

/// One database: coverage and implications per coded column, grain and history per table.
fn measure_database(
    db: &DatabaseConfig,
    objects: &[KirObject],
    literals: &BTreeMap<(String, String), BTreeSet<String>>,
    opts: &RunOpts<'_>,
    redaction: &RedactionConfig,
    report: &mut Vec<Value>,
) -> Measured {
    let Some(tier) = tier_name(db.profile) else {
        report.push(json!({"database": db.name, "skipped": "profile = p0 measures no signal; set p1 or p2"}));
        return Measured::default();
    };
    let target = Target::Database(db.clone());
    let (worker, status) = ProbeWorker::start(&target, opts.allow_writable_role);
    let Some(worker) = worker else {
        let why = match status {
            Some(Err(e)) => redact(&e, redaction),
            _ => "not connected".into(),
        };
        report.push(json!({"database": db.name, "skipped": why}));
        return Measured::default();
    };
    let mut m = Measure {
        db,
        flavor: flavor(db.kind),
        probe: &worker,
        redaction,
        tier,
        errors: Vec::new(),
    };
    let mut out = Measured::default();
    let mut skipped = 0usize;
    for t in live_tables(objects, &db.name) {
        if opts.max_rows > 0 && known_rows(t).is_some_and(|n| n > opts.max_rows) {
            skipped += 1;
            continue;
        }
        for c in columns(t) {
            let Some(name) = c["name"].as_str() else {
                continue;
            };
            let Some(lits) = literals.get(&(tail(&t.name), name.to_lowercase())) else {
                continue;
            };
            let lits: Vec<String> = lits.iter().cloned().collect();
            if wants(opts, "literal") {
                out.columns
                    .extend(m.coverage(t, name, &lits, wants(opts, "temporal")));
            }
            if wants(opts, "conditional") {
                out.implications.extend(m.conditional(t, name, &lits));
            }
        }
        if wants(opts, "keys") {
            out.grain.extend(m.grain(t));
        }
        if wants(opts, "history") {
            out.history.extend(m.history(t));
        }
    }
    report.push(json!({
        "database": db.name, "tier": tier, "columns": out.columns.len(), "tables": out.grain.len(),
        "implications": out.implications.len(), "history_patterns": out.history.len(),
        "skipped_large": skipped, "errors": m.errors.len(),
    }));
    out.errors = m.errors;
    out
}

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

/// The `GrainCandidate` of one table, with its evidence (no counts in the evidence text).
fn grain_object(g: &GrainSignal, table_id: KirId, measured_at: &str) -> (KirObject, KirEvidence) {
    let key = format!("grain-candidate:{table_id}");
    let (source, keys): (&str, Vec<Vec<String>>) = match (&g.declared, &g.search) {
        (Some(pk), _) => ("declared", vec![pk.clone()]),
        (None, Some(r)) if !r.keys.is_empty() => ("measured", r.keys.clone()),
        _ => ("none", Vec::new()),
    };
    let mut o = KirObject::new(
        format!("grain of {}.{}", g.owner, g.table),
        ObjectKind::Custom(GRAIN.into()),
    );
    o.id = kid(&key);
    for (k, v) in [
        (
            "status",
            json!(if source == "declared" {
                "declared"
            } else {
                "hypothesis"
            }),
        ),
        ("database", json!(g.database)),
        ("owner", json!(g.owner)),
        ("table", json!(g.table)),
        ("source", json!(source)),
        ("key", json!(keys.first())),
        ("keys", json!(keys)),
        ("rows", json!(g.rows)),
        ("tested", json!(g.search.as_ref().map(|r| r.tested))),
        (
            "exhausted",
            json!(g.search.as_ref().is_some_and(|r| r.exhausted)),
        ),
        ("exact", json!(true)),
        ("measured_at", json!(measured_at)),
    ] {
        o.properties.insert(k.into(), v);
    }
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: format!("profile://{}/{}.{}", g.database, g.owner, g.table),
            line: None,
            column: None,
        },
        match source {
            "declared" => format!(
                "{}.{}: primary key declared in the catalog",
                g.owner, g.table
            ),
            _ => format!(
                "{}.{}: distinct non-null tuples counted per column set (RFC 0252 key search)",
                g.owner, g.table
            ),
        },
    );
    ev.id = kid(&format!("{key}:evidence"));
    o.evidence = vec![ev.id];
    (o, ev)
}

/// `ekos profile run`.
pub async fn run(config: &EkosConfig, cwd: &Path, opts: RunOpts<'_>) -> Result<()> {
    let dbs: Vec<&DatabaseConfig> = config
        .databases
        .iter()
        .filter(|d| opts.database.is_none_or(|n| n == d.name))
        .collect();
    if dbs.is_empty() {
        bail!(match opts.database {
            Some(n) => format!("no database {n:?} in this project (ekos database list)"),
            None => "this project has no [[databases]] to profile (ekos database add)".into(),
        });
    }
    let ledger = super::store::open_store(config, cwd)?;
    let objects = ledger.all_objects()?;
    let items = super::semantics::items_in(config, cwd, &*ledger)?;
    let literals = code_literals(&items);
    let redaction = config.redaction_config();
    let measured_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // Probes are synchronous and may block: one database at a time, off the async runtime.
    let mut all = Measured::default();
    let mut report = Vec::new();
    for db in dbs {
        let m = tokio::task::block_in_place(|| {
            measure_database(db, &objects, &literals, &opts, &redaction, &mut report)
        });
        all.columns.extend(m.columns);
        all.grain.extend(m.grain);
        all.implications.extend(m.implications);
        all.history.extend(m.history);
        all.errors.extend(m.errors);
    }
    let errors = all.errors;
    let signals = DataSignals {
        rfc: "0252".into(),
        measured_at: measured_at.clone(),
        dead_after_months: opts.dead_after_months,
        columns: all.columns,
        grain: all.grain,
        implications: all.implications,
        history: all.history,
    };
    let grain_ids = write_grain(&*ledger, &objects, &signals)?;
    // §3.2(e): history evidence on the current write behaviour, now rather than at the next commit.
    let mut behaviour: Vec<KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == ekos_semantic::write_behaviour::KIND))
        .cloned()
        .collect();
    if ekos_semantic::data_usage::attach_history(&mut behaviour, &signals) > 0 {
        for o in behaviour
            .iter()
            .filter(|o| o.properties.contains_key("data_history"))
        {
            ledger.append_object(o)?;
        }
    }
    let path = current_path(config, cwd);
    std::fs::create_dir_all(path.parent().expect("a state directory"))?;
    let mut doc = serde_json::to_value(&signals)?;
    doc["grain_ids"] = json!(grain_ids);
    doc["errors"] = json!(errors);
    doc["report"] = json!(report);
    std::fs::write(&path, serde_json::to_vec_pretty(&doc)?)
        .with_context(|| format!("writing {}", path.display()))?;
    // Fold into the meanings now, not at the next commit.
    let folded = super::semantics::commit_step(config, cwd, &*ledger, true).await?;
    // RFC 0254: the questions and code-vs-data findings the new measurements raise.
    let questions = super::questions::commit_step(config, cwd, &*ledger)?;
    ekos_common::run_log::emit(
        "profile",
        "profile run finished",
        &json!({
            "columns": signals.columns.len(), "tables": signals.grain.len(), "errors": errors.len(),
        }),
    );
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary(&signals, &doc, folded.as_ref().map(|f| &f.0)))?
        );
    } else {
        print_summary(&signals, &doc, folded.as_ref().map(|f| &f.0), &path);
        if let Some(q) = &questions {
            println!("{}", super::questions::summary_line(q));
        }
    }
    Ok(())
}

/// Appends one `GrainCandidate` per measured table; returns their ids.
fn write_grain(
    ledger: &dyn KnowledgeStore,
    objects: &[KirObject],
    signals: &DataSignals,
) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for g in &signals.grain {
        let Some(t) = objects.iter().find(|o| {
            o.kind == ObjectKind::Table
                && s(o, "origin") == "live"
                && s(o, "database") == g.database
                && s(o, "owner") == g.owner
                && o.name == g.table
        }) else {
            continue;
        };
        let (o, ev) = grain_object(g, t.id, &signals.measured_at);
        ledger.append_evidence(&ev)?;
        ledger.append_object(&o)?;
        ledger.append_relationship(&KirRelationship::deterministic(
            RelationshipKind::Custom(business_semantics::DESCRIBES.into()),
            o.id,
            t.id,
            "",
        ))?;
        ids.push(o.id.to_string());
    }
    Ok(ids)
}

fn summary(
    signals: &DataSignals,
    doc: &Value,
    folded: Option<&ekos_semantic::business_semantics::SemanticsStats>,
) -> Value {
    let (dead, never, data_only): (usize, usize, usize) = folded
        .map(|s| (s.dead_codes, s.never_seen_codes, s.data_only_gaps))
        .unwrap_or_default();
    json!({
        "measured_at": signals.measured_at,
        "columns": signals.columns.len(),
        "tables": signals.grain.len(),
        "grain": {
            "declared": signals.grain.iter().filter(|g| g.declared.is_some()).count(),
            "measured": signals.grain.iter().filter(|g| g.search.as_ref().is_some_and(|r| !r.keys.is_empty())).count(),
            "none": signals.grain.iter().filter(|g| g.declared.is_none() && g.search.as_ref().is_none_or(|r| r.keys.is_empty())).count(),
        },
        "code_values_measured": folded.map(|s| s.data_values_measured),
        "implications": signals.implications.len(),
        "history_patterns": signals.history.len(),
        "dead_codes": dead,
        "never_seen_codes": never,
        "data_only_gaps": data_only,
        "semantics": folded.is_some(),
        "databases": doc["report"],
        "errors": doc["errors"],
    })
}

fn print_summary(
    signals: &DataSignals,
    doc: &Value,
    folded: Option<&ekos_semantic::business_semantics::SemanticsStats>,
    path: &Path,
) {
    let v = summary(signals, doc, folded);
    for d in v["databases"].as_array().into_iter().flatten() {
        match d.get("skipped") {
            Some(why) => println!(
                "{}: skipped — {}",
                d["database"].as_str().unwrap_or(""),
                why.as_str().unwrap_or("")
            ),
            None => println!(
                "{} ({}): {} coded column(s), {} table(s), {} skipped as too large, {} error(s)",
                d["database"].as_str().unwrap_or(""),
                d["tier"].as_str().unwrap_or(""),
                d["columns"],
                d["tables"],
                d["skipped_large"],
                d["errors"]
            ),
        }
    }
    println!(
        "Grain: {} declared, {} found by key search, {} without a key.",
        v["grain"]["declared"], v["grain"]["measured"], v["grain"]["none"]
    );
    println!(
        "Implications: {} (a code value that always or never comes with another column set); history patterns: {}.",
        v["implications"], v["history_patterns"]
    );
    if v["semantics"] == true {
        println!(
            "Code values: {} measured, {} dead, {} never seen in data; {} column(s) hold values no code names.",
            v["code_values_measured"], v["dead_codes"], v["never_seen_codes"], v["data_only_gaps"]
        );
    } else {
        println!(
            "[semantics] is off: the signals are in {} but no meaning carries them.",
            path.display()
        );
    }
    for e in v["errors"].as_array().into_iter().flatten().take(10) {
        println!(
            "  error {}.{} ({}): {}",
            e["owner"].as_str().unwrap_or(""),
            e["table"].as_str().unwrap_or(""),
            e["signal"].as_str().unwrap_or(""),
            e["error"].as_str().unwrap_or("")
        );
    }
}

/// `ekos profile show [TABLE]`: what the latest run measured, as text or JSON.
pub fn show(config: &EkosConfig, cwd: &Path, table: Option<&str>, json_out: bool) -> Result<()> {
    let Some(sig) = load(config, cwd) else {
        bail!("no profile run yet (ekos profile run)");
    };
    let want =
        |t: &str| table.is_none_or(|w| t.eq_ignore_ascii_case(w) || tail(w) == t.to_lowercase());
    let cols: Vec<&ColumnSignal> = sig.columns.iter().filter(|c| want(&c.table)).collect();
    let grain: Vec<&GrainSignal> = sig.grain.iter().filter(|g| want(&g.table)).collect();
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "measured_at": sig.measured_at, "dead_after_months": sig.dead_after_months,
                "columns": cols, "grain": grain,
            }))?
        );
        return Ok(());
    }
    println!(
        "Measured {} (dead after {} months).",
        sig.measured_at, sig.dead_after_months
    );
    for g in grain {
        let key = match (&g.declared, &g.search) {
            (Some(k), _) => format!("declared key ({})", k.join(", ")),
            (None, Some(r)) if !r.keys.is_empty() => format!(
                "key found: ({}) after {} test(s)",
                r.keys[0].join(", "),
                r.tested
            ),
            (None, Some(r)) => format!("no key among {} tested column set(s)", r.tested),
            _ => "not searched".into(),
        };
        println!(
            "{}.{}.{} — {} rows, {key}",
            g.database, g.owner, g.table, g.rows
        );
    }
    for c in cols {
        println!(
            "{}.{}.{}.{} — {} non-null row(s){}",
            c.database,
            c.owner,
            c.table,
            c.column,
            c.coverage.non_null,
            c.time_column
                .as_ref()
                .map(|t| format!(", dated by {t}"))
                .unwrap_or_default()
        );
        for (lit, u) in &c.coverage.literals {
            println!(
                "    {lit:>12}  {:>10} row(s)  {} → {}",
                u.rows,
                u.first_seen.as_deref().unwrap_or("-"),
                u.last_seen.as_deref().unwrap_or("-")
            );
        }
        if c.coverage.unexplained.rows > 0 {
            println!(
                "    {:>12}  {:>10} row(s) in {} value(s) no code names",
                "(other)",
                c.coverage.unexplained.rows,
                c.coverage
                    .unexplained_distinct
                    .map_or("?".into(), |d| d.to_string())
            );
        }
    }
    Ok(())
}

/// MCP `ekos_profile` (read-only): one table's measured profile — what the catalog and the build
/// measured (counts and non-personal bounds), its grain, how often each value code names occurs
/// and what else the column holds (counts), and the inferred references touching it.
///
/// Scope: the table is looked up through `ledger`, which is the caller's scoped store; the
/// latest run's file entries are returned only for tables that store returned.
pub fn agent_profile(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn ekos_ledger::KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let table = args
        .get("table")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("`table` is required"))?;
    let column = args
        .get("column")
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let database = args.get("database").and_then(Value::as_str);
    let want = tail(table);
    let objects = ledger.all_objects()?;
    let tables: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && s(o, "origin") == "live")
        .filter(|o| o.name.to_lowercase() == want)
        .filter(|o| database.is_none_or(|d| s(o, "database") == d))
        .collect();
    let signals = load(config, cwd);
    let rel_current = super::relationships::current_ids(config, cwd);
    let mut out = Vec::new();
    for t in tables {
        let (db, owner) = (s(t, "database"), s(t, "owner"));
        let cols: Vec<Value> = columns(t)
            .iter()
            .filter(|c| {
                column
                    .as_deref()
                    .is_none_or(|w| c["name"].as_str().is_some_and(|n| n.to_lowercase() == w))
            })
            .map(|c| json!({"name": c["name"], "data_type": c["data_type"], "profile": c["profile"]}))
            .collect();
        let on_table = |d: &str, o: &str, n: &str| d == db && o == owner && n == t.name;
        let coverage: Vec<&ColumnSignal> = signals
            .iter()
            .flat_map(|sg| sg.columns.iter())
            .filter(|c| on_table(&c.database, &c.owner, &c.table))
            .filter(|c| {
                column
                    .as_deref()
                    .is_none_or(|w| c.column.to_lowercase() == w)
            })
            .collect();
        let grain: Option<&GrainSignal> = signals
            .iter()
            .flat_map(|sg| sg.grain.iter())
            .find(|g| on_table(&g.database, &g.owner, &g.table));
        let references: Vec<Value> = objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == super::relationships::KIND))
            .filter(|o| rel_current.as_ref().is_none_or(|c| c.contains(&o.id.to_string())))
            .filter(|o| s(o, "database") == db)
            .filter(|o| {
                (s(o, "child_owner") == owner && s(o, "child_table") == t.name)
                    || (s(o, "parent_owner") == owner && s(o, "parent_table") == t.name)
            })
            .map(|o| json!({
                "reference": o.name, "seed": o.properties.get("seed"), "verdict": o.properties.get("verdict"),
                "orphans": o.properties.get("orphans"), "population": o.properties.get("population"),
                "status": "hypothesis — a person decides it with `ekos review relationships`",
            }))
            .collect();
        out.push(json!({
            "database": db, "owner": owner, "table": t.name,
            "rows": known_rows(t),
            "table_profile": t.properties.get("profile"),
            "columns": cols,
            "grain": grain,
            "code_values": coverage,
            "references": references,
        }));
    }
    Ok(json!({
        "measured_at": signals.as_ref().map(|s| s.measured_at.clone()),
        "dead_after_months": signals.as_ref().map(|s| s.dead_after_months),
        "tables": out,
        "note": if out.is_empty() { "no live table by that name (only tables read from [[databases]] have profiles)" } else { "counts only: no data value is ever stored; the only values shown are ones code names" },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_come_from_enum_meanings_and_value_gaps_only() {
        let mut items = Vec::new();
        for (kind, gap, value) in [
            (ENUM_MEANING, "", "1"),
            (ENUM_MEANING, "", "'A'"),
            (GAP, "unexplained_value", "9"),
            (GAP, "undocumented_concept", ""),
        ] {
            let mut o = KirObject::new("x", business_semantics::object_kind(kind));
            o.properties.insert("table".into(), json!("public.Orders"));
            o.properties.insert("column".into(), json!("Status"));
            o.properties.insert("value".into(), json!(value));
            o.properties.insert("gap_type".into(), json!(gap));
            items.push(o);
        }
        let l = code_literals(&items);
        assert_eq!(
            l[&("orders".to_string(), "status".to_string())],
            ["'A'", "1", "9"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn the_grain_evidence_carries_no_count() {
        let g = GrainSignal {
            database: "erp".into(),
            owner: "public".into(),
            table: "orderline".into(),
            rows: 1234,
            declared: None,
            search: Some(signals::KeySearch {
                keys: vec![vec!["order_id".into(), "line_no".into()]],
                tested: 5,
                ..Default::default()
            }),
        };
        let (o, ev) = grain_object(&g, kid("t"), "2026-11-10T00:00:00Z");
        assert_eq!(o.properties["source"], "measured");
        assert_eq!(o.properties["key"], json!(["order_id", "line_no"]));
        assert!(!ev.fragment.contains("1234"));
        assert_eq!(
            o.id,
            grain_object(&g, kid("t"), "later").0.id,
            "keyed by table"
        );
    }
}
