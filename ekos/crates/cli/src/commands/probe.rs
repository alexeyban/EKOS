//! RFC 0254 §3.5: `ekos probe` and MCP `ekos_probe` — aggregates-only, read-only, budgeted
//! exploration of a live database for an agent forming a hypothesis.
//!
//! A probe is one of the closed templates in `ekos_observation_sdk::probe`, never SQL text. Every
//! call: the table is found through the caller's (scoped) store; a table larger than
//! `[probe] max-rows` is refused; the project's `[probe] daily-calls` is counted before the query
//! runs; personal columns give counts only and small cells hide their value; the answer is never
//! stored; the call is audited — template and parameters, never results — in
//! `<state>/probe/audit.jsonl`. The session is the same read-only probe session `ekos sql check`
//! uses.

use super::sql_check::{ProbeWorker, Target};
use anyhow::{Result, anyhow, bail};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::KnowledgeReader;
use ekos_observation_sdk::probe::{self, Probe};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

fn s<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

fn dir(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("probe")
}

fn columns(t: &KirObject) -> Vec<Value> {
    t.properties
        .get("columns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The live table `name` (optionally `owner.name`) of `database`, through the caller's store.
fn find_table<'a>(objects: &'a [KirObject], database: &str, name: &str) -> Result<&'a KirObject> {
    let (owner, table) = match name.rsplit_once('.') {
        Some((o, t)) => (Some(o), t),
        None => (None, name),
    };
    let m: Vec<&KirObject> = objects
        .iter()
        .filter(|o| {
            o.kind == ObjectKind::Table && s(o, "origin") == "live" && s(o, "database") == database
        })
        .filter(|o| o.name.eq_ignore_ascii_case(table))
        .filter(|o| owner.is_none_or(|w| s(o, "owner").eq_ignore_ascii_case(w)))
        .collect();
    match m.as_slice() {
        [one] => Ok(one),
        [] => bail!("no live table {name} in {database} (or it is outside your read scope)"),
        _ => bail!("{name} is in several schemas of {database}; write owner.table"),
    }
}

/// Counts today's call, refusing past the cap.
fn spend(config: &EkosConfig, cwd: &Path) -> Result<u32> {
    std::fs::create_dir_all(dir(config, cwd))?;
    let path = dir(config, cwd).join("usage.json");
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut v: Value = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}));
    let n = v[&today].as_u64().unwrap_or(0) as u32;
    if n >= config.probe.daily_calls {
        bail!(
            "the project's probe budget for today is spent ([probe] daily-calls = {})",
            config.probe.daily_calls
        );
    }
    v = json!({ today.clone(): n + 1 });
    std::fs::write(&path, serde_json::to_vec(&v)?)?;
    Ok(n + 1)
}

fn audit(
    config: &EkosConfig,
    cwd: &Path,
    who: &str,
    database: &str,
    table: &str,
    spec: &Value,
) -> Result<()> {
    std::fs::create_dir_all(dir(config, cwd))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir(config, cwd).join("audit.jsonl"))?;
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    writeln!(
        f,
        "{}",
        json!({"at": at, "who": who, "database": database, "table": table, "probe": spec})
    )?;
    Ok(())
}

/// Whether a column's values are withheld: a personal name, or a confirmed PII review decision.
fn personal(objects: &[KirObject], table: &str, column: &str) -> bool {
    ekos_common::pii::classify_name(column).is_some()
        || ekos_semantic::review::decisions(objects)
            .iter()
            .any(|((subj, item), d)| {
                subj == "pii"
                    && item.to_lowercase().ends_with(&format!(
                        "{}.{}",
                        table.to_lowercase(),
                        column.to_lowercase()
                    ))
                    && d.properties.get("decision").and_then(|x| x["pii"].as_str())
                        == Some("confirm")
            })
}

/// One probe. `args`: `database`, `table`, and the template's fields (`template`, `column`, …).
pub fn run(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    args: &Value,
    who: &str,
) -> Result<Value> {
    let objects = ledger.all_objects()?;
    let database = match args.get("database").and_then(Value::as_str) {
        Some(d) => d.to_string(),
        None if config.databases.len() == 1 => config.databases[0].name.clone(),
        None => bail!(
            "`database` is required: one of {}",
            config
                .databases
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let db = config
        .databases
        .iter()
        .find(|d| d.name == database)
        .ok_or_else(|| anyhow!("no database {database:?}"))?;
    let table_name = args
        .get("table")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("`table` is required"))?;
    let t = find_table(&objects, &database, table_name)?;
    let p: Probe =
        serde_json::from_value(args.clone()).map_err(|e| anyhow!("not a probe template: {e}"))?;
    let rows_known = t
        .properties
        .get("profile")
        .and_then(|p| p["row_count"].as_i64())
        .or_else(|| t.properties.get("num_rows").and_then(Value::as_i64));
    if rows_known.is_some_and(|n| n > config.probe.max_rows) {
        bail!(
            "{} has about {} rows, over [probe] max-rows ({})",
            t.name,
            rows_known.unwrap_or(0),
            config.probe.max_rows
        );
    }
    if let Some(c) = probe::filter_columns(&p)
        .into_iter()
        .find(|c| personal(&objects, &t.name, c))
    {
        bail!("{c} holds personal data: it cannot be filtered on (counts only, unfiltered)");
    }
    let parent = match &p {
        Probe::Orphans { parent_table, .. } => Some(find_table(&objects, &database, parent_table)?),
        _ => None,
    };
    let parent_cols = parent.map(columns);
    let flavor = super::profile::flavor_of(db.kind);
    let cols = columns(t);
    let sql = probe::render(
        flavor,
        s(t, "owner"),
        &t.name,
        &cols,
        &p,
        parent
            .zip(parent_cols.as_deref())
            .map(|(pt, pc)| (s(pt, "owner"), pt.name.as_str(), pc)),
    )
    .map_err(|e| anyhow!(e))?;
    let used = spend(config, cwd)?;
    let spec = serde_json::to_value(&p)?;
    // The audit keeps the template and columns, never a literal: a filter value can be personal.
    audit(
        config,
        cwd,
        who,
        &database,
        &t.name,
        &serde_json::to_value(probe::audit_form(&p))?,
    )?;
    let redaction = config.redaction_config();
    let rows = tokio::task::block_in_place(|| {
        let (worker, status) = ProbeWorker::start(&Target::Database(db.clone()), false);
        let worker = worker.ok_or_else(|| match status {
            Some(Err(e)) => anyhow!(ekos_common::redaction::redact(&e, &redaction)),
            _ => anyhow!("not connected"),
        })?;
        use ekos_sqlgen::probe::Probe as _;
        worker
            .preview(&sql, 50)
            .map(|r| r.rows)
            .map_err(|e| anyhow!(ekos_common::redaction::redact(&e, &redaction)))
    })?;
    let personal: Vec<bool> = probe::value_columns(&p)
        .iter()
        .map(|c| personal(&objects, &t.name, c))
        .collect();
    let shaped = probe::shape(
        &rows,
        probe::value_cells(&p),
        &personal,
        config.probe.min_cell,
        &redaction,
    );
    Ok(json!({
        "database": database, "table": format!("{}.{}", s(t, "owner"), t.name), "probe": spec,
        "rows": shaped, "calls_today": used, "daily_calls": config.probe.daily_calls,
        "note": "Live aggregates, not stored. Personal columns give counts only; values in fewer than [probe] min-cell rows are hidden. Cite a probe in a proposal so the person's Save re-measures it.",
    }))
}

/// The aggregate of a probe as stored evidence when a person saves a proposal that cited it: the
/// counts only — every value cell is dropped, so a stored measurement holds no source value.
pub fn evidence_text(result: &Value) -> String {
    let counts: Vec<Value> = result["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| r["counts"].clone())
        .collect();
    json!({
        "measured": "ekos_probe (RFC 0254 §3.5), re-run on save",
        "database": result["database"], "table": result["table"], "probe": result["probe"],
        "counts": counts,
    })
    .to_string()
}

/// `ekos probe --spec '<json>'`.
pub fn cli(config: &EkosConfig, cwd: &Path, spec: &str, by: &str) -> Result<()> {
    let args: Value = serde_json::from_str(spec).map_err(|e| anyhow!("--spec is not JSON: {e}"))?;
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let v = run(config, cwd, &*ledger, &args, by)?;
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_evidence_keeps_counts_and_drops_values() {
        let r = json!({"database": "erp", "table": "public.orders", "probe": {"template": "distribution"},
                       "rows": [{"values": ["SECRET-VALUE"], "counts": [12]}]});
        let e = evidence_text(&r);
        assert!(e.contains("12") && !e.contains("SECRET-VALUE"), "{e}");
    }

    /// RFC 0254 §3.5: the `source-explorer` agent has no write tool.
    #[test]
    fn the_source_explorer_agent_cannot_write() {
        let def = include_str!("../../../../../.claude/agents/source-explorer.md");
        let tools = def
            .lines()
            .find_map(|l| l.strip_prefix("tools:"))
            .expect("a tools line");
        for writer in [
            "ekos_identity_review",
            "ekos_architecture_review",
            "ekos_session_note",
            "ekos_clickhouse_query",
            "ekos_sql_generate",
            "Write",
            "Edit",
            "Bash",
        ] {
            assert!(
                !tools.split(',').any(|t| t.trim().ends_with(writer)),
                "{writer} in {tools}"
            );
        }
        assert!(tools.contains("ekos_probe"));
    }

    #[test]
    fn the_daily_budget_is_counted_before_running() {
        let t = tempfile::tempdir().unwrap();
        let mut c = EkosConfig::default();
        c.probe.daily_calls = 2;
        assert_eq!(spend(&c, t.path()).unwrap(), 1);
        assert_eq!(spend(&c, t.path()).unwrap(), 2);
        assert!(spend(&c, t.path()).is_err());
    }
}
