//! `ekos sql check`, `ekos sql generate` and the `ekos_sql_check` MCP tool (RFC 0242/0243).
//!
//! `check` lints a query against the project's compiled schema (tables, columns, keys and foreign
//! keys from the ledger) and the dialect's documented behaviour; with a database
//! (`--database <name>` from `[[databases]]`, or `--sqlite <file>`) it also runs the probes
//! through that database plugin's read-only session. `generate` is RFC 0242's agent over the same
//! schema, the dialect's generation notes and, with a database, the same probes. Nothing writes:
//! every live probe passes the single-SELECT guard and runs in a read-only session.
//!
//! The drivers are blocking, their sessions are not `Send`, and the PostgreSQL driver panics
//! inside an async runtime, so a database session lives on its own plain thread
//! ([`ProbeWorker`]): it connects there, answers queries sent over a channel, and is dropped
//! there. The agent's runtime only waits on the channel.

use super::store::open_store_read_only;
use anyhow::{Context, Result, anyhow, bail};
use ekos_compiler_core::EkosConfig;
use ekos_compiler_core::config::{DatabaseConfig, DatabaseKind};
use ekos_ledger::KnowledgeReader;
use ekos_recovery::LlmProvider;
use ekos_sql_dialect_sdk::SqlDialectParser;
use ekos_sqlgen::agent::{AgentConfig, AgentOutcome, Toolbox};
use ekos_sqlgen::lint::lint;
use ekos_sqlgen::probe::Probe;
use ekos_sqlgen::{Finding, Schema};
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

/// Most tables a generation prompt shows; the agent's tools reach the rest.
const PROMPT_TABLES: usize = 40;
/// Rows of the result `check` prints.
const SHOWN_ROWS: usize = 5;

/// The dialect a check uses when neither `--dialect` nor a database names one: the project's
/// first SQL dialect rule, else `generic` (no dialect-dependent check fires).
pub fn dialect_name(config: &EkosConfig, asked: Option<&str>) -> String {
    asked
        .map(str::to_string)
        .or_else(|| {
            config
                .recover
                .sql
                .dialect_rules
                .first()
                .map(|r| r.dialect.clone())
        })
        .unwrap_or_else(|| "generic".into())
}

fn kind_dialect(kind: &DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::Oracle => "oracle",
        DatabaseKind::Postgres => "postgres",
        DatabaseKind::Clickhouse => "clickhouse",
    }
}

/// The schema the ledger holds (through the store, so a read scope applies), with what the
/// project's current Apache Ossie models state about it (RFC 0246): synonyms, keys, declared
/// joins, metrics. `ossie_files` is the `ossie/current.json` list.
pub fn schema_of(
    store: &dyn KnowledgeReader,
    ossie_files: Option<&std::collections::BTreeSet<String>>,
) -> Result<Schema> {
    let objects = store.all_objects()?;
    let relationships = store.all_relationships()?;
    let knowledge = ekos_ossie::knowledge::resolve(&objects, &relationships, ossie_files);
    Ok(Schema::from_ledger(&objects, &relationships).with_ossie(&knowledge))
}

/// [`schema_of`] for a workspace: its own Ossie file list.
fn schema_for(config: &EkosConfig, cwd: &Path, store: &dyn KnowledgeReader) -> Result<Schema> {
    schema_of(
        store,
        super::export::current_ossie_files(config, cwd).as_ref(),
    )
}

fn parser_for(dialect: &str) -> Result<Box<dyn SqlDialectParser>> {
    let mut registry = ekos_recovery::build_dialect_registry();
    if let Some(p) = registry.remove(dialect) {
        return Ok(p);
    }
    let mut known: Vec<&String> = registry.keys().collect();
    known.sort();
    bail!(
        "unknown dialect {dialect:?}; known: {}",
        known
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The dialect's rules for writing SQL, for whoever is about to write some.
pub fn notes(dialect: &str) -> Result<Vec<&'static str>> {
    Ok(parser_for(dialect)?.generation_notes().to_vec())
}

/// The static check's result as one JSON document.
pub fn check(schema: &Schema, sql: &str, dialect: &str) -> Result<Value> {
    let parser = parser_for(dialect)?;
    let sql = parser.preprocess(sql);
    let sqlparser_dialect = parser.sqlparser_dialect();
    let (parse_error, findings): (Option<String>, Vec<Finding>) =
        match lint(&sql, sqlparser_dialect.as_ref(), &parser.facts(), schema) {
            Ok((_, f)) => (None, f),
            Err((e, f)) => (Some(e), f),
        };
    Ok(report(schema, dialect, parse_error, findings, None, None))
}

fn report(
    schema: &Schema,
    dialect: &str,
    parse_error: Option<String>,
    findings: Vec<Finding>,
    probed: Option<Result<String, String>>,
    preview: Option<&ekos_sqlgen::probe::Preview>,
) -> Value {
    let notes = parser_for(dialect)
        .map(|p| p.generation_notes().to_vec())
        .unwrap_or_default();
    let note = match (&probed, schema.tables.is_empty()) {
        (_, true) => "the ledger holds no tables: nothing to check names against".to_string(),
        (Some(Ok(label)), _) => format!("checked against {label}, read-only"),
        (Some(Err(why)), _) => format!("static checks only — the database was not queried: {why}"),
        (None, _) => "static checks only: value spellings, row counts and results need a \
                      database (--database or --sqlite)"
            .into(),
    };
    let preview = preview.map(|p| {
        json!({
            "columns": p.columns,
            "rows": p.rows.iter().take(SHOWN_ROWS).collect::<Vec<_>>(),
            "more": p.truncated || p.rows.len() > SHOWN_ROWS,
        })
    });
    json!({
        "dialect": dialect,
        "tables_known": schema.tables.len(),
        "foreign_keys_known": schema.foreign_keys.len(),
        "parse_error": parse_error,
        "findings": findings,
        "result": preview,
        "note": note,
        "dialect_notes": notes,
    })
}

/// `ekos_sql_check {sql?, dialect?}`: findings for `sql`, and the dialect's notes; with no `sql`,
/// the notes alone (for an agent about to write a query).
pub fn agent_check(
    config: &EkosConfig,
    workspace: &Path,
    store: &dyn KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let dialect = dialect_name(config, args.get("dialect").and_then(Value::as_str));
    match args.get("sql").and_then(Value::as_str) {
        Some(sql) => check(&schema_for(config, workspace, store)?, sql, &dialect),
        None => Ok(json!({ "dialect": dialect, "dialect_notes": notes(&dialect)? })),
    }
}

/// Where probe queries may go.
#[derive(Debug, Clone)]
pub enum Target {
    None,
    Database(DatabaseConfig),
    Sqlite(PathBuf),
}

/// A connected probe and its label.
type Connected = (Box<dyn Probe>, String);

impl Target {
    pub fn pick(
        config: &EkosConfig,
        database: Option<&str>,
        sqlite: Option<&Path>,
    ) -> Result<Self> {
        match (database, sqlite) {
            (Some(_), Some(_)) => bail!("give --database or --sqlite, not both"),
            (Some(name), None) => config
                .databases
                .iter()
                .find(|d| d.name == name)
                .cloned()
                .map(Target::Database)
                .ok_or_else(|| {
                    anyhow!("no database {name:?} in this project (ekos database list)")
                }),
            (None, Some(path)) => Ok(Target::Sqlite(path.to_path_buf())),
            (None, None) => Ok(Target::None),
        }
    }

    /// The schema as this target sees it: a database's own live tables when it has any.
    fn narrow(&self, schema: Schema) -> Schema {
        match self {
            Target::Database(d) => schema.for_database(&d.name),
            _ => schema,
        }
    }

    /// The dialect the target implies, if any.
    fn dialect(&self) -> Option<&'static str> {
        match self {
            Target::None => None,
            Target::Database(d) => Some(kind_dialect(&d.kind)),
            Target::Sqlite(_) => Some("sqlite"),
        }
    }

    /// Connects (on the calling thread). `Ok(None)` without a target; `Err` says why the
    /// database will not be queried.
    fn connect(&self, allow_writable_role: bool) -> Result<Option<Connected>, String> {
        let run_id = format!("sql-{}", chrono::Utc::now().format("%Y%m%dT%H%M%S"));
        Ok(Some(match self {
            Target::None => return Ok(None),
            Target::Sqlite(path) => {
                let db = ekos_evals::bench::sqlexec::Db::open(path).map_err(|e| e.to_string())?;
                (
                    Box::new(SqliteFile(db)) as Box<dyn Probe>,
                    format!("sqlite:{}", path.display()),
                )
            }
            Target::Database(d) => connect_database(d, &run_id, allow_writable_role)?,
        }))
    }
}

fn connect_database(
    d: &DatabaseConfig,
    run_id: &str,
    allow_writable_role: bool,
) -> Result<Connected, String> {
    Ok(match d.kind {
        DatabaseKind::Postgres => {
            let c =
                ekos_plugin_postgres::PgConnection::parse(&d.connect, &d.user, &d.password_env)?;
            let p = ekos_plugin_postgres::probe::PgProbe::connect(&c, run_id, allow_writable_role)?;
            (Box::new(p), c.label())
        }
        DatabaseKind::Clickhouse => {
            let c = ekos_plugin_clickhouse::catalog::ChConnection::parse(
                &d.connect,
                &d.user,
                &d.password_env,
            )?;
            let label = c.label();
            (
                Box::new(ekos_plugin_clickhouse::probe::ChProbe::new(c)?),
                label,
            )
        }
        DatabaseKind::Oracle => {
            let c = ekos_plugin_oracle::OraConnection {
                connect_string: d.connect.clone(),
                user: d.user.clone(),
                password_env: d.password_env.clone(),
            };
            let label = c.label();
            (
                Box::new(ekos_plugin_oracle::probe::OraProbe::connect(&c, run_id)?),
                label,
            )
        }
    })
}

/// A local SQLite file, opened read-only (the bench's probe, with a 30-second deadline).
struct SqliteFile(ekos_evals::bench::sqlexec::Db);

impl Probe for SqliteFile {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<ekos_sqlgen::probe::Preview, String> {
        ekos_sqlgen::guard::single_select(sql, &sqlparser::dialect::SQLiteDialect {})?;
        ekos_evals::bench::sqlexec::SqliteProbe {
            db: &self.0,
            timeout: std::time::Duration::from_secs(30),
        }
        .preview(sql, max_rows)
    }
}

/// Lint plus, with a target, the probes on it.
pub fn check_on(
    schema: &Schema,
    sql: &str,
    dialect: &str,
    target: &Target,
    allow_writable_role: bool,
) -> Result<Value> {
    if matches!(target, Target::None) {
        return check(schema, sql, dialect);
    }
    let parser = parser_for(dialect)?;
    let sql = parser.preprocess(sql);
    let sqlparser_dialect = parser.sqlparser_dialect();
    let parse_error = lint(&sql, sqlparser_dialect.as_ref(), &parser.facts(), schema)
        .err()
        .map(|(e, _)| e);
    let (worker, probed) = ProbeWorker::start(target, allow_writable_role);
    let tb = Toolbox {
        schema,
        dialect: sqlparser_dialect.as_ref(),
        facts: parser.facts(),
        probe: worker.as_ref().map(|w| w as &dyn Probe),
        checks: None,
    };
    let (preview, findings) = tb.check(&sql);
    Ok(report(
        schema,
        dialect,
        parse_error,
        findings,
        probed,
        preview.as_ref(),
    ))
}

/// A connection attempt as (the probe, what to report about it).
#[allow(clippy::type_complexity)]
fn split(
    connected: Result<Option<Connected>, String>,
) -> (Option<Box<dyn Probe>>, Option<Result<String, String>>) {
    match connected {
        Ok(Some((p, label))) => (Some(p), Some(Ok(label))),
        Ok(None) => (None, None),
        Err(why) => (None, Some(Err(why))),
    }
}

type Request = (
    String,
    usize,
    mpsc::Sender<Result<ekos_sqlgen::probe::Preview, String>>,
);

/// A database session on its own thread, queried over a channel.
struct ProbeWorker {
    tx: mpsc::Sender<Request>,
}

impl ProbeWorker {
    /// Connects on a new thread. The probe is `None` without a target or when the database will
    /// not be queried; the second value says which, for the report.
    fn start(
        target: &Target,
        allow_writable_role: bool,
    ) -> (Option<ProbeWorker>, Option<Result<String, String>>) {
        if matches!(target, Target::None) {
            return (None, None);
        }
        let target = target.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::spawn(move || {
            let (probe, status) = split(target.connect(allow_writable_role));
            let _ = ready_tx.send(status);
            if let Some(p) = probe {
                // Ends when the worker's sender is dropped.
                for (sql, max_rows, reply) in rx {
                    let _ = reply.send(p.preview(&sql, max_rows));
                }
            }
        });
        match ready_rx.recv() {
            Ok(Some(Ok(label))) => (Some(ProbeWorker { tx }), Some(Ok(label))),
            Ok(status) => (None, status),
            Err(_) => (None, Some(Err("the database thread stopped".into()))),
        }
    }
}

impl Probe for ProbeWorker {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<ekos_sqlgen::probe::Preview, String> {
        let (reply, answer) = mpsc::channel();
        self.tx
            .send((sql.to_string(), max_rows, reply))
            .map_err(|_| "the database thread stopped".to_string())?;
        answer
            .recv()
            .map_err(|_| "the database thread stopped".to_string())?
    }
}

pub struct CheckOpts<'a> {
    pub file: Option<&'a PathBuf>,
    pub dialect: Option<&'a str>,
    pub database: Option<&'a str>,
    pub sqlite: Option<&'a Path>,
    pub allow_writable_role: bool,
    pub json: bool,
}

fn read_sql(file: Option<&PathBuf>) -> Result<String> {
    match file {
        Some(p) if p.as_os_str() != "-" => {
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))
        }
        _ => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
    }
}

fn pick_dialect(config: &EkosConfig, asked: Option<&str>, target: &Target) -> String {
    asked
        .map(str::to_string)
        .or_else(|| target.dialect().map(str::to_string))
        .unwrap_or_else(|| dialect_name(config, None))
}

/// `ekos sql check [FILE|-] [--dialect D] [--database NAME | --sqlite FILE] [--json]`.
pub fn run(config: &EkosConfig, cwd: &Path, o: CheckOpts<'_>) -> Result<()> {
    let sql = read_sql(o.file)?;
    let target = Target::pick(config, o.database, o.sqlite)?;
    let store = open_store_read_only(config, cwd)?;
    let schema = target.narrow(schema_for(config, cwd, store.as_ref())?);
    let dialect = pick_dialect(config, o.dialect, &target);
    let v = check_on(&schema, &sql, &dialect, &target, o.allow_writable_role)?;
    if o.json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print_check(&v);
    }
    Ok(())
}

fn print_check(v: &Value) {
    println!(
        "Checked against {} tables and {} foreign keys ({} dialect).",
        v["tables_known"],
        v["foreign_keys_known"],
        v["dialect"].as_str().unwrap_or("")
    );
    let findings = v["findings"].as_array().cloned().unwrap_or_default();
    if findings.is_empty() {
        println!("No problems found.");
    }
    for f in findings {
        println!(
            "{} {}: {}",
            f["severity"].as_str().unwrap_or(""),
            f["code"].as_str().unwrap_or(""),
            f["message"].as_str().unwrap_or("")
        );
    }
    if let Some(r) = v["result"].as_object() {
        print_result(r);
    }
    println!("{}", v["note"].as_str().unwrap_or(""));
}

fn print_result(r: &serde_json::Map<String, Value>) {
    println!("\nResult:");
    let cols: Vec<&str> = r["columns"]
        .as_array()
        .map(|c| c.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    println!("  {}", cols.join(" | "));
    for row in r["rows"].as_array().cloned().unwrap_or_default() {
        let cells: Vec<String> = row
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|c| c.as_str().map_or("NULL".to_string(), str::to_string))
            .collect();
        println!("  {}", cells.join(" | "));
    }
    if r["more"] == true {
        println!("  …");
    }
}

pub struct GenerateOpts<'a> {
    pub question: &'a str,
    pub dialect: Option<&'a str>,
    pub database: Option<&'a str>,
    pub sqlite: Option<&'a Path>,
    pub max_turns: usize,
    pub max_tokens: u32,
    pub allow_metered: bool,
    pub allow_writable_role: bool,
    pub json: bool,
}

/// The task prompt for one question.
pub fn generation_prompt(schema: &Schema, dialect: &str, question: &str) -> Result<String> {
    let tables = schema.relevant(question, PROMPT_TABLES);
    let shown = if tables.len() < schema.tables.len() {
        format!(
            " — {} of its {} tables, picked for the question; search_schema and describe_table \
             reach the rest",
            tables.len(),
            schema.tables.len()
        )
    } else {
        String::new()
    };
    let notes: String = notes(dialect)?.iter().map(|n| format!("- {n}\n")).collect();
    Ok(format!(
        "Task:\nWrite one SQL query for the {dialect} database described below that answers the \
         question.\n\nDatabase schema (compiled by EKOS from the project's sources{shown}):\n\
         {}\nHow to write SQL for {dialect}:\n{notes}\nQuestion:\n{question}\n\nInstructions:\n\
         - Use only the tables and columns shown, or ones the tools find; never invent a name.\n\
         - Return exactly the columns the question asks for, in the order it asks for them.\n\
         - Before counting or averaging, check each table's grain: when an entity appears in \
         several rows, count DISTINCT entities if the question is about entities.\n\
         - Join on the foreign keys listed (`joins:`) unless the question needs another \
         condition.\n{metrics}\
         - Give the SQL first, in one ```sql block, then explain it in at most three \
         sentences, naming any assumption about a value or a definition the schema does not \
         state.\n",
        schema.render(&tables),
        metrics = if schema.metrics.is_empty() {
            String::new()
        } else {
            "- When the question asks for a metric listed (`metric name = …`), compute it with \
             that definition, and say that you did.\n"
                .to_string()
        }
    ))
}

/// `ekos sql generate "<question>"`.
pub async fn generate(config: &EkosConfig, cwd: &Path, o: GenerateOpts<'_>) -> Result<()> {
    super::migrate_logic::model_call_allowed(config, o.allow_metered)?;
    let target = Target::pick(config, o.database, o.sqlite)?;
    let schema = {
        let store = open_store_read_only(config, cwd)?;
        target.narrow(schema_for(config, cwd, store.as_ref())?)
    };
    if schema.tables.is_empty() {
        bail!("the ledger holds no tables to write SQL against — run the pipeline first");
    }
    let dialect = pick_dialect(config, o.dialect, &target);
    let prompt = generation_prompt(&schema, &dialect, o.question)?;
    let llm = super::recover::build_llm_provider_strict_for(
        config,
        ekos_recovery::Task::NlSql,
        &config.artifact_dir(cwd),
    )?;
    let cfg = AgentConfig {
        max_turns: o.max_turns,
        max_tokens: o.max_tokens,
        prompt_version: "ekos-sql-generate-v1",
        tools: true,
        verify: true,
    };
    let job = Job {
        schema: &schema,
        dialect: &dialect,
        prompt: &prompt,
        target: &target,
        allow_writable_role: o.allow_writable_role,
        cfg: &cfg,
    };
    let (outcome, probed) = std::thread::scope(|s| {
        s.spawn(|| job.run(llm))
            .join()
            .map_err(|_| anyhow!("the generation thread panicked"))?
    })?;
    print_generated(&outcome, &dialect, &probed, o.json)
}

/// One generation, run on its own thread.
struct Job<'a> {
    schema: &'a Schema,
    dialect: &'a str,
    prompt: &'a str,
    target: &'a Target,
    allow_writable_role: bool,
    cfg: &'a AgentConfig,
}

impl Job<'_> {
    /// Connect, then run the agent on a runtime local to this thread.
    fn run(
        &self,
        llm: Arc<dyn LlmProvider>,
    ) -> Result<(AgentOutcome, Option<Result<String, String>>)> {
        let parser = parser_for(self.dialect)?;
        let sqlparser_dialect = parser.sqlparser_dialect();
        let (worker, probed) = ProbeWorker::start(self.target, self.allow_writable_role);
        let tb = Toolbox {
            schema: self.schema,
            dialect: sqlparser_dialect.as_ref(),
            facts: parser.facts(),
            probe: worker.as_ref().map(|w| w as &dyn Probe),
            checks: None,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let outcome = rt
            .block_on(ekos_sqlgen::agent::run(
                llm.as_ref(),
                self.prompt,
                &tb,
                self.cfg,
            ))
            .map_err(|e| anyhow!("the model call failed: {e}"))?;
        Ok((outcome, probed))
    }
}

fn print_generated(
    o: &AgentOutcome,
    dialect: &str,
    probed: &Option<Result<String, String>>,
    json: bool,
) -> Result<()> {
    let checked = match probed {
        Some(Ok(label)) => format!("checked against {label}, read-only"),
        Some(Err(why)) => format!("static checks only — the database was not queried: {why}"),
        None => "static checks only (no --database or --sqlite)".into(),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "sql": o.sql,
                "dialect": dialect,
                "turns": o.turns,
                "tools": o.tools,
                "findings": o.findings,
                "open_findings": o.open_findings,
                "checked": checked,
                "input_tokens": o.calls.iter().map(|c| c.input_tokens).sum::<u64>(),
                "output_tokens": o.calls.iter().map(|c| c.output_tokens).sum::<u64>(),
            }))?
        );
        return Ok(());
    }
    match &o.sql {
        Some(sql) => println!("{sql}\n"),
        None => println!("The model gave no SQL.\n"),
    }
    let tools = if o.tools.is_empty() {
        "none".to_string()
    } else {
        o.tools.join(", ")
    };
    println!(
        "{dialect} · {} replies · tools: {tools} · {checked}",
        o.turns
    );
    if !o.open_findings.is_empty() {
        println!(
            "Still open on the final SQL: {} — run `ekos sql check` on it for the details.",
            o.open_findings.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_sqlgen::{Column, ForeignKey, Table};

    fn schema() -> Schema {
        let col = |n: &str, t: &str, pk: bool| Column {
            name: n.into(),
            data_type: t.into(),
            primary_key: pk,
            ..Default::default()
        };
        Schema {
            tables: vec![
                Table {
                    name: "customers".into(),
                    columns: vec![col("id", "integer", true), col("city", "text", false)],
                    ..Default::default()
                },
                Table {
                    name: "orders".into(),
                    columns: vec![
                        col("id", "integer", true),
                        col("customer_id", "integer", false),
                        col("amount", "integer", false),
                    ],
                    ..Default::default()
                },
            ],
            foreign_keys: vec![ForeignKey {
                from_table: "orders".into(),
                from_column: "customer_id".into(),
                to_table: "customers".into(),
                to_column: "id".into(),
            }],
            ..Default::default()
        }
    }

    fn codes(v: &Value) -> Vec<&str> {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["code"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn unknown_names_and_integer_division_by_dialect() {
        let sql = "SELECT SUM(o.amount) / COUNT(o.id), c.town FROM orders o \
                   JOIN customers c ON c.id = o.customer_id";
        let v = check(&schema(), sql, "postgres").unwrap();
        assert_eq!(codes(&v), vec!["L001", "L005"]);
        assert!(!v["dialect_notes"].as_array().unwrap().is_empty());
        let v = check(&schema(), sql, "generic").unwrap();
        assert_eq!(codes(&v), vec!["L001"], "generic claims no division rule");
        assert!(check(&schema(), sql, "nope").is_err());
    }

    #[test]
    fn oracle_wants_fetch_first_and_mysql_backticks() {
        let v = check(&schema(), "SELECT city FROM customers LIMIT 5", "oracle").unwrap();
        assert_eq!(codes(&v), vec!["L009"]);
        let v = check(&schema(), "SELECT \"city\" FROM customers", "mysql").unwrap();
        assert_eq!(codes(&v), vec!["L011"]);
    }

    #[test]
    fn dialect_defaults_to_the_projects_first_rule() {
        let mut config = EkosConfig::default();
        assert_eq!(dialect_name(&config, None), "generic");
        config
            .recover
            .sql
            .dialect_rules
            .push(ekos_compiler_core::config::SqlDialectRuleConfig {
                path_glob: "**/*.sql".into(),
                dialect: "postgres".into(),
            });
        assert_eq!(dialect_name(&config, None), "postgres");
        assert_eq!(dialect_name(&config, Some("sqlite")), "sqlite");
    }

    /// A real SQLite file: the probes run, a write never does.
    #[test]
    fn a_sqlite_file_is_probed_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shop.sqlite");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, city TEXT);
             INSERT INTO customers VALUES (1, 'Paris'), (2, 'Lyon');",
        )
        .unwrap();
        drop(c);
        let target = Target::Sqlite(path.clone());
        let v = check_on(
            &schema(),
            "SELECT city FROM customers WHERE city = 'paris'",
            "sqlite",
            &target,
            false,
        )
        .unwrap();
        assert_eq!(codes(&v), vec!["P001", "P003"]);
        assert!(
            v["note"]
                .as_str()
                .unwrap()
                .starts_with("checked against sqlite:")
        );
        let v = check_on(&schema(), "DELETE FROM customers", "sqlite", &target, false).unwrap();
        assert!(
            v["findings"].to_string().contains("not sent"),
            "the guard refuses before the engine sees it: {v}"
        );
        let c = rusqlite::Connection::open(&path).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn the_prompt_holds_schema_notes_and_question() {
        let p =
            generation_prompt(&schema(), "oracle", "How many customers live in Paris?").unwrap();
        assert!(p.contains("table customers") && p.contains("joins: orders.customer_id"));
        assert!(p.contains("FETCH FIRST n ROWS ONLY"));
        assert!(p.contains("How many customers live in Paris?"));
    }
}
