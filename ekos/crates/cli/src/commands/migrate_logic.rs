//! `ekos migrate lower` / `ekos migrate check-candidate` — RFC 0164 Phase 1.
//!
//! Both read the ledger and the workspace's own source files. Neither opens a database connection
//! and neither calls a model. Nothing is written: the lowering is printed for review, and what it
//! becomes on the target is still RFC 0160's guarded executor's business.
//!
//! The definition of a view or routine is read back from its file by the span `ekos recover`
//! recorded. That makes this a raw-content read, so it applies the same RFC 0043 exclusion and
//! redaction as recover does — the spans were measured on the redacted text.

use anyhow::{Context, Result, bail};
use ekos_compiler_core::config::EkosConfig;
use ekos_kir::{KirObject, ObjectKind};
use ekos_migrate_logic::lower::{LowerOptions, Lowering, lower_view};
use ekos_migrate_logic::triggers::TriggerOutcome;
use ekos_migrate_logic::{
    check_candidate, propose_trigger, recorded_gate, routine_atoms, routine_gate, sql_atoms,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn is_kind(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn text<'a>(o: &'a KirObject, key: &str) -> &'a str {
    o.properties
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

/// The object's definition, read from its file by the recorded span.
///
/// `Err` is a sentence for the report: a missing file, an excluded path, or a span that no longer
/// holds a definition of this object (the file changed since `ekos recover`).
fn definition(config: &EkosConfig, cwd: &Path, o: &KirObject) -> Result<String, String> {
    let path = text(o, "source_path");
    if path.is_empty() {
        return Err("no source path was recorded".into());
    }
    let redaction = config.redaction_config();
    if ekos_common::redaction::is_excluded_path(path, &redaction) {
        return Err(format!("{path} is excluded by the security rules"));
    }
    let raw = std::fs::read_to_string(cwd.join(path))
        .map_err(|e| format!("{path} could not be read: {e}"))?;
    let redacted = ekos_common::redaction::redact(&raw, &redaction);
    let span = |key: &str| {
        o.properties
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as usize)
    };
    let (Some(start), Some(end)) = (span("span_start"), span("span_end")) else {
        return Err("no source span was recorded".into());
    };
    let slice = redacted.get(start..end).ok_or_else(|| {
        format!("{path} is shorter than the recorded span — re-run `ekos recover`")
    })?;
    let head = slice.trim_start().to_ascii_lowercase();
    let bare = o.name.rsplit('.').next().unwrap_or(&o.name).to_lowercase();
    if !head.starts_with("create") || !slice.to_lowercase().contains(&bare) {
        return Err(format!(
            "{path} no longer holds this definition at the recorded span — re-run `ekos recover`"
        ));
    }
    Ok(slice.to_string())
}

fn location(o: &KirObject) -> String {
    match o.properties.get("line").and_then(serde_json::Value::as_u64) {
        Some(line) => format!("{}:{line}", text(o, "source_path")),
        None => text(o, "source_path").to_string(),
    }
}

fn sorted<'a>(objects: &'a [KirObject], kind: &str) -> Vec<&'a KirObject> {
    let mut out: Vec<&KirObject> = objects.iter().filter(|o| is_kind(o, kind)).collect();
    out.sort_by(|a, b| (location(a), &a.name).cmp(&(location(b), &b.name)));
    out
}

/// One row of the report, as JSON. The text rendering is derived from the same values.
fn view_row(config: &EkosConfig, cwd: &Path, o: &KirObject, options: &LowerOptions) -> Value {
    let mut row = json!({ "name": o.name, "source": location(o) });
    match definition(config, cwd, o) {
        Err(why) => {
            row["outcome"] = json!("unreadable");
            row["why"] = json!(why);
        }
        Ok(def) => match lower_view(&def, options) {
            Lowering::Lowered(l) => {
                // The lowering is checked against its own source before it is shown.
                let invented = sql_atoms(&def)
                    .map(|atoms| check_candidate(&atoms, &l.sql))
                    .ok();
                match invented {
                    Some(report) if report.accepted() => {
                        row["outcome"] = json!("lowered");
                        row["logic_hash"] = json!(logic_hash(&def, options, &l.sql));
                        row["sql"] = json!(l.sql);
                        row["notes"] = json!(l.notes);
                    }
                    other => {
                        row["outcome"] = json!("blocked");
                        row["blockers"] = json!([{
                            "code": "LOGIC.INVENTION",
                            "detail": match other {
                                Some(r) => format!(
                                    "the lowering states what the source does not: {}",
                                    r.unmapped.iter().map(|a| a.text.clone()).collect::<Vec<_>>().join(", ")
                                ),
                                None => "the source could not be reduced to atoms".to_string(),
                            },
                        }]);
                    }
                }
            }
            Lowering::Blocked { blockers } => {
                row["outcome"] = json!("blocked");
                row["blockers"] = json!(blockers);
            }
        },
    }
    row
}

/// A hash of the lowering that does not depend on the target database's name, so the statement
/// V5 ran (in a sandbox database of its own) and the statement recorded for a real target are
/// recognisably the same logic.
fn logic_hash(definition: &str, options: &LowerOptions, lowered: &str) -> String {
    let plain = match &options.database {
        None => lowered.to_string(),
        Some(_) => match lower_view(definition, &LowerOptions::default()) {
            Lowering::Lowered(l) => l.sql,
            Lowering::Blocked { .. } => lowered.to_string(),
        },
    };
    ekos_common::ContentHash::of(plain.as_bytes())
        .as_str()
        .to_string()
}

fn trigger_row(o: &KirObject) -> Value {
    let class = text(o, "classification");
    let mut row = json!({ "name": o.name, "source": location(o), "classification": class });
    match propose_trigger(class) {
        TriggerOutcome::Proposed(p) => {
            row["outcome"] = json!("proposed");
            row["shape"] = json!(p.shape);
            row["proposal"] = json!(p.proposal);
            row["behaviour_change"] = json!(p.behaviour_change);
        }
        TriggerOutcome::HumanDecision { why } => {
            row["outcome"] = json!("human_decision");
            row["why"] = json!(why);
        }
    }
    row
}

fn routine_row(o: &KirObject) -> Value {
    let language = text(o, "language").to_lowercase();
    let mut row = json!({ "name": o.name, "source": location(o), "language": language });
    if language != "plpgsql" {
        row["outcome"] = json!("out_of_scope");
        row["why"] = json!(format!(
            "a {} routine: only PL/pgSQL bodies are recovered statement by statement",
            if language.is_empty() {
                "non-PL/pgSQL"
            } else {
                &language
            }
        ));
        return row;
    }
    let props: serde_json::Map<String, Value> = o
        .properties
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    match recorded_gate(&props) {
        Ok(()) => row["outcome"] = json!("eligible"),
        Err(why) => {
            row["outcome"] = json!("ineligible");
            row["why"] = json!(why.describe());
        }
    }
    row
}

fn tally(rows: &[Value]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for r in rows {
        *out.entry(r["outcome"].as_str().unwrap_or_default().to_string())
            .or_default() += 1;
    }
    out
}

fn tally_line(rows: &[Value]) -> String {
    tally(rows)
        .iter()
        .map(|(k, n)| format!("{n} {}", k.replace('_', " ")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The proposals `--record` saves: one per lowered view, one per trigger with a proposed rule.
fn lowering_proposals(
    objects: &[KirObject],
    views: &[Value],
    triggers: &[Value],
) -> Vec<KirObject> {
    let mut proposals = Vec::new();
    for (source, row) in sorted(objects, "View").into_iter().zip(views) {
        if row["outcome"] == "lowered" {
            let mut body = json!({
                "target": "clickhouse",
                "sql": row["sql"],
                "sql_hash": ekos_common::ContentHash::of(
                    row["sql"].as_str().unwrap_or_default().as_bytes()
                ).as_str(),
                "logic_hash": row["logic_hash"],
                "notes": row["notes"],
            });
            // A V5 result already recorded for this same logic still holds; recording the
            // lowering again must not erase it. A changed lowering starts unexecuted.
            let earlier = objects.iter().find(|o| o.id == proposal_id(source));
            if let Some(v5) = earlier
                .filter(|o| o.properties.get("logic_hash") == Some(&row["logic_hash"]))
                .and_then(|o| o.properties.get("v5"))
            {
                body["v5"] = v5.clone();
            }
            proposals.push(proposal(source, "view", "lowering", body));
        }
    }
    for (source, row) in sorted(objects, "Trigger").into_iter().zip(triggers) {
        if row["outcome"] == "proposed" {
            proposals.push(proposal(
                source,
                "trigger",
                "rule",
                json!({
                    "classification": row["classification"],
                    "shape": row["shape"],
                    "proposal": row["proposal"],
                    "behaviour_change": row["behaviour_change"],
                }),
            ));
        }
    }
    proposals
}

/// One line per view with its notes or blockers, then the blockers counted by rule.
fn print_view_rows(views: &[Value], show_sql: bool) {
    let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
    for v in views {
        let name = v["name"].as_str().unwrap_or_default();
        let source = v["source"].as_str().unwrap_or_default();
        match v["outcome"].as_str() {
            Some("lowered") => {
                println!("  lowered   {name}  ({source})");
                for n in v["notes"].as_array().into_iter().flatten() {
                    println!("              · {}", n.as_str().unwrap_or_default());
                }
                if show_sql {
                    println!("              {}", v["sql"].as_str().unwrap_or_default());
                }
            }
            Some("blocked") => {
                println!("  blocked   {name}  ({source})");
                for b in v["blockers"].as_array().into_iter().flatten() {
                    let code = b["code"].as_str().unwrap_or_default();
                    *by_code.entry(code.to_string()).or_default() += 1;
                    println!(
                        "              {code}: {}",
                        b["detail"].as_str().unwrap_or_default()
                    );
                }
            }
            _ => println!(
                "  unreadable {name}  ({source}): {}",
                v["why"].as_str().unwrap_or_default()
            ),
        }
    }
    if !by_code.is_empty() {
        println!("  Blockers by rule:");
        for (code, n) in &by_code {
            println!("    {n:>4}  {code}");
        }
    }
}

fn print_trigger_rows(triggers: &[Value]) {
    for t in triggers {
        let name = t["name"].as_str().unwrap_or_default();
        let class = t["classification"].as_str().unwrap_or_default();
        match t["outcome"].as_str() {
            Some("proposed") => {
                println!(
                    "  {name} [{class}] → {}",
                    t["proposal"].as_str().unwrap_or_default()
                );
                println!(
                    "              changes: {}",
                    t["behaviour_change"].as_str().unwrap_or_default()
                );
            }
            _ => println!(
                "  {name} [{class}] → a person decides: {}",
                t["why"].as_str().unwrap_or_default()
            ),
        }
    }
}

/// `ekos migrate lower`.
pub fn lower(
    config: &EkosConfig,
    cwd: &Path,
    database: Option<String>,
    show_sql: bool,
    record: bool,
    as_json: bool,
) -> Result<()> {
    super::migrate::require_enabled(config)?;
    let store = super::store::open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let options = LowerOptions { database };

    let views: Vec<Value> = sorted(&objects, "View")
        .into_iter()
        .map(|o| view_row(config, cwd, o, &options))
        .collect();
    let triggers: Vec<Value> = sorted(&objects, "Trigger")
        .into_iter()
        .map(trigger_row)
        .collect();
    let routines: Vec<Value> = sorted(&objects, "Procedure")
        .into_iter()
        .map(routine_row)
        .collect();

    let recorded = if record {
        let proposals = lowering_proposals(&objects, &views, &triggers);
        Some(write_proposals(store.as_ref(), &proposals, "lower")?)
    } else {
        None
    };

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "views": views, "triggers": triggers, "routines": routines,
                "summary": {
                    "views": tally(&views), "triggers": tally(&triggers), "routines": tally(&routines),
                },
            }))?
        );
        return Ok(());
    }

    println!("Views: {} — {}", views.len(), tally_line(&views));
    print_view_rows(&views, show_sql);

    println!("\nTriggers: {} — {}", triggers.len(), tally_line(&triggers));
    print_trigger_rows(&triggers);

    println!("\nRoutines: {} — {}", routines.len(), tally_line(&routines));
    for r in routines.iter().filter(|r| r["outcome"] == "ineligible") {
        println!(
            "  ineligible {}  ({}): {}",
            r["name"].as_str().unwrap_or_default(),
            r["source"].as_str().unwrap_or_default(),
            r["why"].as_str().unwrap_or_default()
        );
    }
    match recorded {
        Some(n) => println!(
            "\n{n} proposal(s) recorded as MigrationProposal, status `proposed`. Nothing was run on \
             a target. `ekos migrate v5` executes the lowered views against their sources."
        ),
        None => println!(
            "\nNothing was written or run. A lowered view is a proposal: it is checked to parse on \
             the target and to state nothing its source does not. `--record` saves the proposals; \
             `ekos migrate v5` executes them against their sources on sandbox engines."
        ),
    }
    Ok(())
}

// ── Proposals ────────────────────────────────────────────────────────────────

fn proposal_id(source: &KirObject) -> ekos_kir::KirId {
    ekos_kir::KirId(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("migration-proposal:{}", source.id).as_bytes(),
    ))
}

/// A `MigrationProposal` for `source`. Keyed by the source object, so recording again replaces
/// the proposal instead of adding a second one. It carries the source's evidence: a proposal is a
/// conclusion about those lines.
fn proposal(source: &KirObject, object_kind: &str, author: &str, body: Value) -> KirObject {
    let mut obj = KirObject::new(
        format!("proposal:{}", source.name),
        ObjectKind::Custom(ekos_migrate::kinds::PROPOSAL_KIND.into()),
    );
    obj.id = proposal_id(source);
    obj.evidence = source.evidence.clone();
    for (k, v) in [
        ("source_object", json!(source.id.to_string())),
        ("source_name", json!(source.name)),
        ("source_path", json!(text(source, "source_path"))),
        ("object_kind", json!(object_kind)),
        ("author", json!(author)),
        // Only a person moves a proposal on; nothing in this module writes another status.
        ("status", json!("proposed")),
    ] {
        obj.properties.insert(k.into(), v);
    }
    if let Value::Object(map) = body {
        obj.properties.extend(map);
    }
    obj
}

fn write_proposals(
    store: &dyn ekos_ledger::KnowledgeStore,
    proposals: &[KirObject],
    verb: &str,
) -> Result<usize> {
    let run_id = super::migrate::new_run_id();
    store.set_write_context(Some(ekos_migrate::project::write_context(verb, &run_id)));
    let result = proposals
        .iter()
        .try_for_each(|p| store.append_object(p).map(|_| ()));
    store.set_write_context(None);
    result?;
    Ok(proposals.len())
}

// ── V5 ───────────────────────────────────────────────────────────────────────

use ekos_migrate_logic::v5::{self, Sandbox, TableSpec, V5Outcome};
use ekos_migrate_validate::Dialect;

struct PgEngine(ekos_pg_live::PgSandbox);

impl Sandbox for PgEngine {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }
    fn label(&self) -> &str {
        self.0.label()
    }
    fn run(
        &self,
        setup: &[String],
        queries: &[String],
    ) -> std::result::Result<Vec<Vec<Vec<String>>>, String> {
        self.0.run(setup, queries).map_err(|e| e.to_string())
    }
}

/// ClickHouse over HTTP. Each run gets a database of its own, dropped afterwards whatever
/// happened in between.
struct ChEngine {
    url: String,
    label: String,
    run_id: String,
    seq: std::cell::Cell<usize>,
}

impl ChEngine {
    fn post(&self, database: Option<&str>, sql: &str) -> std::result::Result<String, String> {
        // SQL NULL as an empty cell, the way the PostgreSQL side renders it.
        let mut url = format!("{}&format_tsv_null_representation=", self.url);
        if let Some(db) = database {
            url.push_str(&format!("&database={db}"));
        }
        let out = std::process::Command::new("curl")
            .args(["-s", "--fail-with-body", &url, "--data-binary", sql])
            .output()
            .map_err(|e| e.to_string())?;
        let body = String::from_utf8_lossy(&out.stdout).to_string();
        if out.status.success() {
            Ok(body)
        } else {
            Err(body)
        }
    }
}

impl Sandbox for ChEngine {
    fn dialect(&self) -> Dialect {
        Dialect::ClickHouse
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn run(
        &self,
        setup: &[String],
        queries: &[String],
    ) -> std::result::Result<Vec<Vec<Vec<String>>>, String> {
        let n = self.seq.get();
        self.seq.set(n + 1);
        let db = format!("ekos_v5_{}_{n}", self.run_id);
        self.post(None, &format!("CREATE DATABASE {db}"))?;
        let result = (|| {
            for s in setup {
                self.post(Some(&db), s)?;
            }
            queries
                .iter()
                .map(|q| {
                    let body = self.post(Some(&db), &format!("{q} FORMAT TabSeparatedRaw"))?;
                    Ok(body
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| l.split('\t').map(str::to_string).collect())
                        .collect())
                })
                .collect()
        })();
        let _ = self.post(None, &format!("DROP DATABASE IF EXISTS {db}"));
        result
    }
}

/// Resolve a sandbox DSN, refusing anything that is not marked as one.
///
/// The mark is the control. V5 writes — fixtures, a view — and the only connections it may write
/// to are ones somebody declared disposable in `ekos.toml`. A DSN that is some project's migration
/// source is refused even if it carries the mark.
fn sandbox_ref<'a>(
    config: &'a EkosConfig,
    dsn: &str,
    engine: ekos_migrate::EngineKind,
    project_sources: &[String],
) -> Result<(
    ekos_migrate::ConnectionRef,
    &'a ekos_compiler_core::config::MigrateConnection,
)> {
    let conn = ekos_migrate::ConnectionRef::parse(dsn)?;
    if conn.engine != engine {
        bail!("{dsn} is not a {engine} connection");
    }
    let settings = super::migrate::resolve_alias(config, &conn.alias)?;
    if !settings.sandbox {
        bail!(
            "connection '{alias}' is not marked as a sandbox. V5 loads fixtures and creates a view, \
             so it only runs on an engine declared disposable:\n\n\
             [migrate.connections.{alias}]\nsandbox = true\n\n\
             Never set that on a migration source.",
            alias = conn.alias
        );
    }
    if project_sources.iter().any(|s| s == dsn) {
        bail!("{dsn} is a migration project's source; V5 will not write to it");
    }
    let conn = conn.with_secret_env(settings.secret_env.clone());
    Ok((conn, settings))
}

/// The tables EKOS recovered from the repository, by bare name. A name two tables share is left
/// out: the fixture would be built from a guess.
fn table_specs(objects: &[KirObject]) -> Vec<TableSpec> {
    let mut by_name: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for o in objects.iter().filter(|o| o.kind == ObjectKind::Table) {
        let bare = o.name.rsplit('.').next().unwrap_or(&o.name).to_lowercase();
        by_name.entry(bare).or_default().push(o);
    }
    by_name
        .into_iter()
        .filter(|(_, v)| v.len() == 1)
        .filter_map(|(name, v)| {
            let columns: Vec<(String, String)> = v[0]
                .properties
                .get("columns")?
                .as_array()?
                .iter()
                .filter_map(|c| {
                    Some((
                        c.get("name")?.as_str()?.to_string(),
                        c.get("data_type")?.as_str()?.to_string(),
                    ))
                })
                .collect();
            (!columns.is_empty()).then_some(TableSpec { name, columns })
        })
        .collect()
}

fn v5_row(
    config: &EkosConfig,
    cwd: &Path,
    o: &KirObject,
    tables: &[TableSpec],
    source: &dyn Sandbox,
    target: &dyn Sandbox,
) -> (Value, Option<Value>) {
    let mut row = json!({ "name": o.name, "source": location(o) });
    let def = match definition(config, cwd, o) {
        Ok(def) => def,
        Err(why) => {
            row["v5"] = json!("not_run");
            row["why"] = json!(why);
            return (row, None);
        }
    };
    // Lowered without a target database: the sandbox database is the session's own.
    let lowered = match lower_view(&def, &LowerOptions::default()) {
        Lowering::Lowered(l) => l,
        Lowering::Blocked { .. } => {
            row["v5"] = json!("not_lowered");
            return (row, None);
        }
    };
    let outcome = match v5::plan(&def, &lowered.sql, tables) {
        Ok(plan) => v5::run(&plan, source, target),
        Err(outcome) => outcome,
    };
    let recorded = serde_json::to_value(&outcome).ok();
    match &outcome {
        V5Outcome::Ran(report) => {
            row["v5"] = json!(if report.passed { "passed" } else { "failed" });
            row["verdict"] = json!(report.verdict());
            row["report"] = json!(report);
        }
        V5Outcome::NotRun { why } => {
            row["v5"] = json!("not_run");
            row["why"] = json!(why);
        }
        V5Outcome::Error { engine, message } => {
            row["v5"] = json!("error");
            row["why"] = json!(format!("{engine}: {}", message.trim()));
        }
    }
    let proposal = recorded.map(|v| {
        json!({
            "target": "clickhouse",
            "sql": lowered.sql,
            "sql_hash": ekos_common::ContentHash::of(lowered.sql.as_bytes()).as_str(),
            "logic_hash": ekos_common::ContentHash::of(lowered.sql.as_bytes()).as_str(),
            "notes": lowered.notes,
            "v5": v,
        })
    });
    (row, proposal)
}

/// `ekos migrate v5` — run every lowered view against its source on two sandbox engines.
pub fn run_v5(
    config: &EkosConfig,
    cwd: &Path,
    source_sandbox: &str,
    target_sandbox: &str,
    record: bool,
    as_json: bool,
) -> Result<()> {
    super::migrate::require_enabled(config)?;
    let store = super::store::open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let project_sources: Vec<String> = objects
        .iter()
        .filter(|o| is_kind(o, ekos_migrate::kinds::PROJECT_KIND))
        .map(|o| text(o, "source").to_string())
        .collect();
    let (pg_ref, pg) = sandbox_ref(
        config,
        source_sandbox,
        ekos_migrate::EngineKind::Postgres,
        &project_sources,
    )?;
    let (ch_ref, ch) = sandbox_ref(
        config,
        target_sandbox,
        ekos_migrate::EngineKind::ClickHouse,
        &project_sources,
    )?;
    let tables = table_specs(&objects);
    let views = sorted(&objects, "View");
    let run_id = super::migrate::new_run_id();
    let short: String = run_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(20)
        .collect();

    // Database work on its own thread: the synchronous driver cannot start inside the runtime.
    let results: Vec<(Value, Option<Value>)> = super::migrate::off_runtime(|| {
        let source = PgEngine(ekos_pg_live::PgSandbox::connect(
            &pg_ref, &pg.host, pg.port, &pg.user, &run_id,
        )?);
        let password = ch
            .secret_env
            .as_ref()
            .and_then(|v| std::env::var(v).ok())
            .unwrap_or_default();
        let target = ChEngine {
            url: format!(
                "http://{}:{}/?user={}&password={}",
                ch.host, ch.port, ch.user, password
            ),
            label: format!("clickhouse-sandbox:{}", ch_ref.alias),
            run_id: short.to_lowercase(),
            seq: std::cell::Cell::new(0),
        };
        Ok(views
            .iter()
            .map(|o| v5_row(config, cwd, o, &tables, &source, &target))
            .collect())
    })?;

    let rows: Vec<&Value> = results.iter().map(|(r, _)| r).collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        *counts
            .entry(r["v5"].as_str().unwrap_or_default().to_string())
            .or_default() += 1;
    }
    let recorded = if record {
        let proposals: Vec<KirObject> = views
            .iter()
            .zip(&results)
            .filter_map(|(o, (_, body))| Some(proposal(o, "view", "lowering", body.clone()?)))
            .collect();
        Some(write_proposals(store.as_ref(), &proposals, "v5")?)
    } else {
        None
    };

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "views": rows, "summary": counts }))?
        );
    } else {
        let summary: Vec<String> = counts
            .iter()
            .map(|(k, n)| format!("{n} {}", k.replace('_', " ")))
            .collect();
        println!("V5 over {} view(s): {}", rows.len(), summary.join(", "));
        for r in &rows {
            let name = r["name"].as_str().unwrap_or_default();
            let source = r["source"].as_str().unwrap_or_default();
            match r["v5"].as_str() {
                Some("passed") | Some("failed") => {
                    println!(
                        "  {name}  ({source})\n      {}",
                        r["verdict"].as_str().unwrap_or_default()
                    );
                    for d in r["report"]["divergent"].as_array().into_iter().flatten() {
                        println!(
                            "      row {} — source {}×, target {}×",
                            d["row"], d["source"], d["target"]
                        );
                    }
                    for u in r["report"]["coverage"]["uncovered"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        println!(
                            "      not taken both ways: {}",
                            u.as_str().unwrap_or_default()
                        );
                    }
                }
                Some("not_lowered") => {
                    println!("  {name}  ({source})\n      not lowered, so not run")
                }
                _ => println!(
                    "  {name}  ({source})\n      {}: {}",
                    r["v5"].as_str().unwrap_or_default().replace('_', " "),
                    r["why"].as_str().unwrap_or_default()
                ),
            }
        }
        if let Some(n) = recorded {
            println!("\n{n} proposal(s) recorded with their V5 result.");
        }
        println!(
            "\nRows are compared as multisets on a generated fixture; row order is not checked. \
             A view that was not run has not passed."
        );
    }
    if counts.get("failed").copied().unwrap_or(0) > 0 {
        bail!("V5 failed for at least one view");
    }
    Ok(())
}

/// `ekos migrate check-candidate`.
pub fn check(
    config: &EkosConfig,
    cwd: &Path,
    source: &str,
    candidate: &Path,
    as_json: bool,
) -> Result<()> {
    super::migrate::require_enabled(config)?;
    let store = super::store::open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let wanted = source.to_lowercase();
    let matches: Vec<&KirObject> = objects
        .iter()
        .filter(|o| is_kind(o, "View") || is_kind(o, "Procedure"))
        .filter(|o| {
            let name = o.name.to_lowercase();
            name == wanted
                || name.rsplit('.').next() == Some(wanted.as_str())
                || format!("{}:{name}", text(o, "source_path").to_lowercase()) == wanted
        })
        .collect();
    let object = match matches.as_slice() {
        [] => bail!("no View or Procedure named {source:?} in the ledger"),
        [one] => *one,
        many => bail!(
            "{source:?} names {} objects; say which as <path>:<name>:\n{}",
            many.len(),
            many.iter()
                .map(|o| format!("  {}:{}", text(o, "source_path"), o.name))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let def = definition(config, cwd, object).map_err(|why| anyhow::anyhow!(why))?;
    let (atoms, unparsed) = if is_kind(object, "View") {
        (
            sql_atoms(&def).map_err(|e| anyhow::anyhow!("the view does not parse: {e}"))?,
            0,
        )
    } else {
        let ir = ekos_plpgsql::parse_function(&def)
            .map_err(|e| anyhow::anyhow!("the routine does not lex: {e:?}"))?;
        if let Err(why) = routine_gate(&ir) {
            bail!(
                "{} is not eligible for reconstruction: {}. A candidate for it cannot be checked, \
                 because the check has nothing to compare the missing part against.",
                object.name,
                why.describe()
            );
        }
        let found = routine_atoms(&ir);
        (found.atoms, found.unparsed)
    };
    let sql = std::fs::read_to_string(candidate)
        .with_context(|| format!("reading {}", candidate.display()))?;
    let report = check_candidate(&atoms, &sql);

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "source": object.name, "source_atoms": atoms.len(),
                "source_fragments_unparsed": unparsed,
                "accepted": report.accepted(), "report": report,
            }))?
        );
    } else {
        println!(
            "Source: {} ({}) — {} atom(s){}",
            object.name,
            location(object),
            atoms.len(),
            if unparsed > 0 {
                format!(
                    ", {unparsed} fragment(s) of the source did not parse and count for nothing"
                )
            } else {
                String::new()
            }
        );
        if let Some(e) = &report.parse_error {
            println!("REJECTED: the candidate does not parse as ClickHouse SQL: {e}");
        } else if report.unmapped.is_empty() {
            println!(
                "ACCEPTED: all {} atom(s) of the candidate are stated by the source.",
                report.checked
            );
        } else {
            println!(
                "REJECTED: {} of {} atom(s) are not stated by the source:",
                report.unmapped.len(),
                report.checked
            );
            for a in &report.unmapped {
                println!("  {:<9} {}", format!("{:?}", a.kind).to_lowercase(), a.text);
            }
        }
    }
    if !report.accepted() {
        bail!("candidate rejected");
    }
    Ok(())
}

// ── Reconstruction ───────────────────────────────────────────────────────────

use ekos_migrate_logic::reconstruct::{self, Reconstruction};

/// Whether a run with this configuration may call its model without being told to.
///
/// A local Ollama model costs nothing. Anything else is billed somewhere, so it needs the flag —
/// per run, never a config default somebody set once (RFC 0164's open question, answered).
fn model_call_allowed(config: &EkosConfig, allow_metered: bool) -> Result<()> {
    let local = config.llm.provider.as_deref() == Some("ollama");
    if local || allow_metered {
        return Ok(());
    }
    bail!(
        "the configured model ({}) is not local, so a call may be billed. Re-run with \
         --allow-metered to send this one routine to it, or set `[llm] provider = \"ollama\"`.",
        config.llm.provider.as_deref().unwrap_or("anthropic")
    )
}

/// One routine through the three fences: gate, model, anti-invention. Split from the command so
/// the fences are tested with a provider that answers what the test says.
async fn reconstruct_one(
    llm: &dyn ekos_recovery::LlmProvider,
    definition: &str,
    max_tokens: u32,
) -> Result<(Reconstruction, Option<String>)> {
    let ir = ekos_plpgsql::parse_function(definition)
        .map_err(|e| anyhow::anyhow!("the routine does not lex: {e:?}"))?;
    let user = match reconstruct::prompt(definition, &ir) {
        Ok(user) => user,
        // Refused before any model is asked.
        Err(why) => {
            return Ok((
                Reconstruction::Ineligible {
                    why: why.describe(),
                },
                None,
            ));
        }
    };
    let answer = llm
        .complete(&ekos_recovery::LlmRequest {
            system: reconstruct::SYSTEM,
            user: &user,
            prompt_version: reconstruct::PROMPT_VERSION,
            max_tokens,
            history: &[],
        })
        .await
        .map_err(|e| anyhow::anyhow!("the model call failed: {e}"))?;
    let prompt_hash = ekos_common::ContentHash::of(
        format!(
            "{}\n{}\n{user}",
            reconstruct::PROMPT_VERSION,
            reconstruct::SYSTEM
        )
        .as_bytes(),
    )
    .as_str()
    .to_string();
    Ok((reconstruct::judge(&ir, &answer.content), Some(prompt_hash)))
}

/// `ekos migrate reconstruct` — ask a model for a target implementation of one routine, and keep
/// the answer only if it survives the gate and the anti-invention check.
pub async fn run_reconstruct(
    config: &EkosConfig,
    cwd: &Path,
    source: &str,
    allow_metered: bool,
    record: bool,
    as_json: bool,
) -> Result<()> {
    super::migrate::require_enabled(config)?;
    let store = super::store::open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let wanted = source.to_lowercase();
    let matches: Vec<&KirObject> = objects
        .iter()
        .filter(|o| is_kind(o, "Procedure"))
        .filter(|o| {
            let name = o.name.to_lowercase();
            name == wanted || format!("{}:{name}", text(o, "source_path").to_lowercase()) == wanted
        })
        .collect();
    let object = match matches.as_slice() {
        [] => bail!("no Procedure named {source:?} in the ledger"),
        [one] => *one,
        many => bail!(
            "{source:?} names {} routines; say which as <path>:<name>:\n{}",
            many.len(),
            many.iter()
                .map(|o| format!("  {}:{}", text(o, "source_path"), o.name))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let def = definition(config, cwd, object).map_err(|why| anyhow::anyhow!(why))?;

    // The gate is checked on its own first, so an ineligible routine costs no model call and
    // needs no spending decision.
    let ir = ekos_plpgsql::parse_function(&def)
        .map_err(|e| anyhow::anyhow!("the routine does not lex: {e:?}"))?;
    let (outcome, prompt_hash, model) = match routine_gate(&ir) {
        Err(why) => (
            Reconstruction::Ineligible {
                why: why.describe(),
            },
            None,
            None,
        ),
        Ok(()) => {
            model_call_allowed(config, allow_metered)?;
            let llm = super::recover::build_llm_provider_strict_for(
                config,
                ekos_recovery::Task::Reconstruct,
                &config.artifact_dir(cwd),
            )?;
            let (outcome, hash) =
                reconstruct_one(llm.as_ref(), &def, config.llm.max_tokens.unwrap_or(4096)).await?;
            (outcome, hash, Some(llm.model_name().to_string()))
        }
    };

    let recorded = match (&outcome, record) {
        (
            Reconstruction::Proposed {
                candidate, checked, ..
            },
            true,
        ) => {
            let p = proposal(
                object,
                "routine",
                "llm",
                json!({
                    "target": "clickhouse",
                    "sql": candidate,
                    "sql_hash": ekos_common::ContentHash::of(candidate.as_bytes()).as_str(),
                    "model": model,
                    "prompt_version": reconstruct::PROMPT_VERSION,
                    "prompt_hash": prompt_hash,
                    "atoms_checked": checked,
                    "v5": { "v5": "not_run", "why": "differential execution of routines is not built" },
                }),
            );
            write_proposals(store.as_ref(), &[p], "reconstruct")?;
            true
        }
        _ => false,
    };

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "source": object.name, "model": model, "prompt_hash": prompt_hash,
                "recorded": recorded, "result": outcome,
            }))?
        );
    } else {
        println!("Routine: {} ({})", object.name, location(object));
        if let Some(m) = &model {
            println!("Model:   {m}");
        }
        match &outcome {
            Reconstruction::Ineligible { why } => {
                println!("NOT SENT: {why}. A person rewrites this routine; no model was asked.")
            }
            Reconstruction::NoCandidate { why } => println!("NO CANDIDATE: {why}."),
            Reconstruction::Rejected { candidate, report } => {
                match &report.parse_error {
                    Some(e) => {
                        println!("REJECTED: the answer does not parse as ClickHouse SQL: {e}")
                    }
                    None => {
                        println!(
                            "REJECTED: {} of {} atom(s) are not stated by the routine:",
                            report.unmapped.len(),
                            report.checked
                        );
                        for a in &report.unmapped {
                            println!("  {:<9} {}", format!("{:?}", a.kind).to_lowercase(), a.text);
                        }
                    }
                }
                println!("\n{candidate}");
            }
            Reconstruction::Proposed {
                candidate,
                checked,
                source_fragments_unparsed,
            } => {
                println!(
                    "PROPOSED: all {checked} atom(s) are stated by the routine{}.",
                    if *source_fragments_unparsed > 0 {
                        format!(
                            " ({source_fragments_unparsed} source fragment(s) did not parse and vouch for nothing)"
                        )
                    } else {
                        String::new()
                    }
                );
                println!(
                    "It has not been executed. The check finds what was added, not what was left \
                     out: a dropped branch passes it. Review it against the routine.\n\n{candidate}"
                );
                if recorded {
                    println!("\nRecorded as a MigrationProposal (author llm, status proposed).");
                }
            }
        }
    }
    if matches!(outcome, Reconstruction::Rejected { .. }) {
        bail!("candidate rejected");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(name: &str, path: &str, start: usize, end: usize) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("View".into()))
            .with_property("source_path", json!(path))
            .with_property("span_start", json!(start))
            .with_property("span_end", json!(end))
            .with_property("line", json!(1))
    }

    #[test]
    fn a_view_is_read_back_by_its_span_and_lowered() {
        let dir = tempfile::tempdir().unwrap();
        let sql =
            "-- header\nCREATE VIEW open_items AS SELECT id FROM items WHERE closed = false;\n";
        std::fs::write(dir.path().join("v.sql"), sql).unwrap();
        let start = sql.find("CREATE").unwrap();
        let o = view("open_items", "v.sql", start, sql.len() - 1);
        let row = view_row(
            &EkosConfig::default(),
            dir.path(),
            &o,
            &LowerOptions::default(),
        );
        assert_eq!(row["outcome"], "lowered", "{row}");
        assert_eq!(
            row["sql"],
            "CREATE VIEW `open_items` AS SELECT `id` FROM `items` WHERE (`closed` = false)"
        );
    }

    #[test]
    fn a_span_that_no_longer_holds_the_view_is_reported_not_lowered() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("v.sql"),
            "CREATE VIEW other AS SELECT 1;\n-- padding padding",
        )
        .unwrap();
        let o = view("open_items", "v.sql", 0, 30);
        let row = view_row(
            &EkosConfig::default(),
            dir.path(),
            &o,
            &LowerOptions::default(),
        );
        assert_eq!(row["outcome"], "unreadable");
        assert!(row["why"].as_str().unwrap().contains("re-run"), "{row}");
        // A missing file and a path the security rules exclude are reported the same way.
        let gone = view("v", "missing.sql", 0, 5);
        assert_eq!(
            view_row(
                &EkosConfig::default(),
                dir.path(),
                &gone,
                &LowerOptions::default()
            )["outcome"],
            "unreadable"
        );
        let secret = view("v", ".env", 0, 5);
        let row = view_row(
            &EkosConfig::default(),
            dir.path(),
            &secret,
            &LowerOptions::default(),
        );
        assert!(row["why"].as_str().unwrap().contains("excluded"), "{row}");
    }

    fn config_with(sandbox: bool) -> EkosConfig {
        let mut config = EkosConfig::default();
        config.migrate.connections.insert(
            "box".into(),
            toml::from_str(&format!(
                "host = \"localhost\"\nuser = \"u\"\nsandbox = {sandbox}"
            ))
            .unwrap(),
        );
        config
    }

    #[test]
    fn v5_only_writes_to_a_connection_marked_as_a_sandbox() {
        let pg = ekos_migrate::EngineKind::Postgres;
        let err = sandbox_ref(&config_with(false), "postgres://box/db", pg, &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("not marked as a sandbox"), "{err}");
        assert!(sandbox_ref(&config_with(true), "postgres://box/db", pg, &[]).is_ok());
        // The mark does not help a DSN that is a project's source, or one of the wrong engine.
        let err = sandbox_ref(
            &config_with(true),
            "postgres://box/db",
            pg,
            &["postgres://box/db".into()],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("migration project's source"), "{err}");
        assert!(sandbox_ref(&config_with(true), "clickhouse://box/db", pg, &[]).is_err());
        assert!(sandbox_ref(&config_with(true), "postgres://other/db", pg, &[]).is_err());
    }

    #[test]
    fn fixture_tables_come_from_unambiguous_recovered_tables_only() {
        let table = |name: &str| {
            KirObject::new(name, ObjectKind::Table).with_property(
                "columns",
                json!([{"name": "id", "data_type": "INT"}, {"name": "note", "data_type": "TEXT"}]),
            )
        };
        let specs = table_specs(&[
            table("public.invoice"),
            table("line"),
            table("lsmb13.line"),
            KirObject::new("empty", ObjectKind::Table),
        ]);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "invoice");
        assert_eq!(specs[0].columns[0], ("id".to_string(), "INT".to_string()));
    }

    #[test]
    fn the_logic_hash_does_not_depend_on_the_target_database() {
        let def = "CREATE VIEW v AS SELECT id FROM t WHERE closed = false";
        let lower = |database: Option<&str>| {
            let options = LowerOptions {
                database: database.map(str::to_string),
            };
            let Lowering::Lowered(l) = lower_view(def, &options) else {
                panic!("blocked");
            };
            (logic_hash(def, &options, &l.sql), l.sql)
        };
        let (plain_hash, plain_sql) = lower(None);
        let (named_hash, named_sql) = lower(Some("lsmb"));
        assert_ne!(plain_sql, named_sql);
        assert_eq!(plain_hash, named_hash);
        // A different view is different logic.
        let other = "CREATE VIEW v AS SELECT id FROM t WHERE closed = true";
        let Lowering::Lowered(l) = lower_view(other, &LowerOptions::default()) else {
            panic!("blocked");
        };
        assert_ne!(
            logic_hash(other, &LowerOptions::default(), &l.sql),
            plain_hash
        );
    }

    #[test]
    fn a_proposal_is_keyed_by_its_source_and_starts_proposed() {
        let mut source = view("open_items", "v.sql", 0, 10);
        source
            .evidence
            .push(ekos_kir::KirId(uuid::Uuid::from_u128(7)));
        let a = proposal(&source, "view", "lowering", json!({"sql": "CREATE VIEW …"}));
        let b = proposal(
            &source,
            "view",
            "lowering",
            json!({"sql": "CREATE VIEW … v2"}),
        );
        assert_eq!(a.id, b.id, "recording again must replace, not add");
        assert_eq!(a.properties["status"], "proposed");
        assert_eq!(a.properties["author"], "lowering");
        assert_eq!(a.properties["source_object"], source.id.to_string());
        assert_eq!(a.evidence, source.evidence);
        assert!(matches!(&a.kind, ObjectKind::Custom(k) if k == "MigrationProposal"));
    }

    #[test]
    fn triggers_and_routines_report_their_outcome() {
        let t = KirObject::new("audit_trg", ObjectKind::Custom("Trigger".into()))
            .with_property("classification", json!("Audit"));
        assert_eq!(trigger_row(&t)["outcome"], "proposed");
        let m = KirObject::new("mixed_trg", ObjectKind::Custom("Trigger".into()))
            .with_property("classification", json!("Mixed"));
        assert_eq!(trigger_row(&m)["outcome"], "human_decision");

        let routine = |language: &str, fidelity: &str, dynamic: u64| {
            KirObject::new("f", ObjectKind::Custom("Procedure".into()))
                .with_property("language", json!(language))
                .with_property("fidelity", json!(fidelity))
                .with_property("dynamic_sql_sites", json!(dynamic))
        };
        assert_eq!(
            routine_row(&routine("plpgsql", "statements", 0))["outcome"],
            "eligible"
        );
        assert_eq!(
            routine_row(&routine("plpgsql", "partial", 0))["outcome"],
            "ineligible"
        );
        assert_eq!(
            routine_row(&routine("plpgsql", "statements", 1))["outcome"],
            "ineligible"
        );
        assert_eq!(
            routine_row(&routine("sql", "signature", 0))["outcome"],
            "out_of_scope"
        );
    }

    const ROUTINE: &str = "CREATE FUNCTION open_total(in_customer int) RETURNS numeric AS $$\n\
        DECLARE t numeric;\nBEGIN\n\
        SELECT sum(amount) INTO t FROM invoice WHERE customer_id = in_customer;\n\
        RETURN t;\nEND; $$ LANGUAGE plpgsql;";

    #[tokio::test]
    async fn a_model_answer_is_kept_only_if_it_invents_nothing() {
        let faithful = ekos_recovery::MockLlmProvider::new(
            "```sql\nSELECT sum(amount) FROM invoice WHERE customer_id = in_customer\n```",
        );
        let (outcome, hash) = reconstruct_one(&faithful, ROUTINE, 512).await.unwrap();
        assert!(
            matches!(outcome, Reconstruction::Proposed { .. }),
            "{outcome:#?}"
        );
        assert!(hash.is_some());

        let inventing = ekos_recovery::MockLlmProvider::new(
            "```sql\nSELECT sum(amount) FROM invoice WHERE customer_id = in_customer AND void = 0\n```",
        );
        let (outcome, _) = reconstruct_one(&inventing, ROUTINE, 512).await.unwrap();
        assert!(
            matches!(outcome, Reconstruction::Rejected { .. }),
            "{outcome:#?}"
        );
    }

    /// A provider that fails the test if it is ever asked.
    struct NeverAsked;

    #[async_trait::async_trait]
    impl ekos_recovery::LlmProvider for NeverAsked {
        fn model_name(&self) -> &str {
            "never"
        }
        async fn complete(
            &self,
            _req: &ekos_recovery::LlmRequest<'_>,
        ) -> std::result::Result<ekos_recovery::LlmResponse, ekos_recovery::LlmError> {
            panic!("an ineligible routine reached the model");
        }
    }

    #[tokio::test]
    async fn an_ineligible_routine_never_reaches_the_model() {
        let dynamic = "CREATE FUNCTION f(t text) RETURNS int AS $$ BEGIN \
                       EXECUTE 'DELETE FROM ' || t; RETURN 1; END; $$ LANGUAGE plpgsql;";
        let (outcome, hash) = reconstruct_one(&NeverAsked, dynamic, 512).await.unwrap();
        assert!(matches!(outcome, Reconstruction::Ineligible { .. }));
        assert!(hash.is_none());
    }

    #[test]
    fn a_model_that_may_be_billed_needs_the_flag() {
        let mut config = EkosConfig::default();
        config.llm.provider = Some("openai".into());
        let err = model_call_allowed(&config, false).unwrap_err().to_string();
        assert!(err.contains("--allow-metered"), "{err}");
        assert!(model_call_allowed(&config, true).is_ok());
        config.llm.provider = None;
        assert!(model_call_allowed(&config, false).is_err());
        config.llm.provider = Some("ollama".into());
        assert!(model_call_allowed(&config, false).is_ok());
    }
}
