//! `ekos migrate init|status` (RFC 0154 Phase 0) — the migration project model and state machine.
//!
//! Phase 0 is deliberately inert with respect to the outside world: nothing here opens a
//! connection to PostgreSQL or to a target, and nothing generates or executes SQL. It creates the
//! project and its connection *references* (aliases and secret variable names, never secrets) and
//! reports the state of its units. Discovery, profiling, mapping, execution and validation arrive
//! with RFCs 0155–0167.

use anyhow::{Result, bail};
use ekos_compiler_core::{EkosConfig, MigrateConnection};
use ekos_migrate::drift::{self, ColumnRef, TableRef, TableShape};
use ekos_migrate::profile_facts;
use ekos_migrate::project::{self, Unit};
use ekos_migrate::{ALL_STATES, ConnectionRef, Project, UnitState};
use ekos_pg_live::catalog::ObjectKind;
use ekos_pg_live::{PgSource, SessionPolicy, profile as pgprofile};
use std::path::Path;

/// One id per `ekos migrate` invocation, grouping every write that verb makes (RFC 0135 Part B).
fn new_run_id() -> String {
    format!("migrate-{}", uuid::Uuid::new_v4())
}

fn require_enabled(config: &EkosConfig) -> Result<()> {
    if !config.migrate.enabled {
        bail!(
            "EKOS Migrate is disabled. Enable it in ekos.toml:\n\n[migrate]\nenabled = true\n\n\
             Migrate is opt-in because it is the only part of EKOS that connects to systems \
             outside this workspace."
        );
    }
    Ok(())
}

fn active_project(config: &EkosConfig, explicit: Option<String>) -> Result<String> {
    explicit
        .or_else(|| config.migrate.project.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no migration project selected. Run `ekos migrate init --name <project> \
                 --source <dsn> --target <dsn>`, or pass --project <name>."
            )
        })
}

/// `ekos migrate init` — create the project and its two connection references.
///
/// Idempotent: ids are deterministic, so re-running against the same name re-appends identical
/// content rather than creating a second project in an append-only ledger that has no dedup.
pub fn init(
    config: &EkosConfig,
    cwd: &Path,
    name: String,
    source: String,
    target: String,
    source_secret_env: Option<String>,
    target_secret_env: Option<String>,
) -> Result<()> {
    require_enabled(config)?;

    let source = ConnectionRef::parse(&source)?
        .require_source()?
        .with_secret_env(source_secret_env);
    let target = ConnectionRef::parse(&target)?
        .require_target()?
        .with_secret_env(target_secret_env);

    let store = crate::commands::store::open_store(config, cwd)?;
    let project = Project {
        name: name.clone(),
        source: source.clone(),
        target: target.clone(),
        created_by: whoami(),
    };
    let id = project.create(store.as_ref(), &new_run_id())?;

    println!("Migration project '{name}' ready.");
    println!("  id      : {id}");
    println!("  source  : {source}");
    println!("  target  : {target}");
    for (role, conn) in [("source", &source), ("target", &target)] {
        match &conn.secret_env {
            Some(v) => println!("  {role} secret: ${v} (name only — the value is never stored)"),
            None => println!(
                "  {role} secret: not set. Pass --{role}-secret-env <VAR> when a password is needed."
            ),
        }
    }
    println!(
        "\nAdd this to ekos.toml so other verbs find it:\n\n[migrate]\nenabled = true\nproject = \"{name}\""
    );
    Ok(())
}

/// `ekos migrate status` — units by state, and what is blocking.
pub fn status(config: &EkosConfig, cwd: &Path, project: Option<String>) -> Result<()> {
    require_enabled(config)?;
    let name = active_project(config, project)?;
    let store = crate::commands::store::open_store(config, cwd)?;

    let Some(obj) = Project::load(store.as_ref(), &name)? else {
        bail!("no migration project named '{name}'. Run `ekos migrate init` first.");
    };
    let units = Unit::all_in(store.as_ref(), &name)?;

    println!("Migration project: {name}");
    for key in ["source", "target"] {
        if let Some(v) = obj.properties.get(key).and_then(|v| v.as_str()) {
            println!("  {key:<7}: {v}");
        }
    }

    if units.is_empty() {
        println!("\nNo migration units yet. `ekos migrate discover` arrives with RFC 0157.");
        return Ok(());
    }

    let mut counts = std::collections::BTreeMap::new();
    for (_, state) in &units {
        *counts.entry(*state).or_insert(0usize) += 1;
    }

    println!("\nUnits ({}):", units.len());
    // Ordered by the state machine, not by a hand-written list that drifts from it.
    for state in ALL_STATES {
        if let Some(n) = counts.get(&state) {
            println!("  {:<15} {n}", state.to_string());
        }
    }

    let complete = units.iter().filter(|(_, s)| s.is_complete()).count();
    println!(
        "\n{complete}/{} units validated or signed off.",
        units.len()
    );

    let blocked: Vec<&str> = units
        .iter()
        .filter(|(_, s)| *s == UnitState::Diverged)
        .map(|(o, _)| o.name.as_str())
        .collect();
    if !blocked.is_empty() {
        println!("\nDiverged, needs reconciliation:");
        for name in blocked {
            println!("  {name}");
        }
    }
    Ok(())
}

/// Resolve a connection alias through `[migrate.connections.<alias>]`.
///
/// The alias is what the ledger stores; the host, port and user live in config and the password
/// lives in an environment variable the config only *names*. A missing alias is an error with the
/// TOML to add, because guessing `localhost:5432` would silently point a migration at the wrong
/// database.
fn resolve_alias<'a>(config: &'a EkosConfig, alias: &str) -> Result<&'a MigrateConnection> {
    config.migrate.connections.get(alias).ok_or_else(|| {
        anyhow::anyhow!(
            "no connection named '{alias}'. Add it to ekos.toml:\n\n\
             [migrate.connections.{alias}]\nhost = \"…\"\nport = 5432\nuser = \"…\"\n\
             secret-env = \"MY_PASSWORD_VAR\""
        )
    })
}

/// Run database work on a dedicated OS thread, outside the tokio runtime.
///
/// **Why this exists.** `bin/ekos.rs` is `#[tokio::main]`, so every CLI command already runs inside
/// a runtime. The synchronous `postgres` crate builds its own runtime internally, and starting a
/// runtime from within one panics: *"Cannot start a runtime from within a runtime."*
///
/// RFC 0157 chose the synchronous driver on the reasoning that it avoids having to answer how a
/// chunk-parallel executor coexists with the non-`Sync` `KnowledgeStore`. That reasoning held; the
/// claim that it left *nothing* to arrange did not. The arrangement is this function, and it is the
/// same shape RFC 0160's chunk parallelism will use: database work happens on its own thread, and
/// **results are collected before anything touches the store**. The ledger handle never crosses the
/// boundary, so its thread-safety stays nobody else's problem.
fn off_runtime<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send,
    T: Send,
{
    std::thread::scope(|s| {
        s.spawn(f)
            .join()
            .map_err(|_| anyhow::anyhow!("the database worker thread panicked"))?
    })
}

/// Open the source described by a project's `source` DSN.
fn open_source(config: &EkosConfig, dsn: &str, run_id: &str) -> Result<PgSource> {
    let conn = ConnectionRef::parse(dsn)?.require_source()?;
    let settings = resolve_alias(config, &conn.alias)?;
    let conn = conn.with_secret_env(settings.secret_env.clone());
    let src = PgSource::connect(
        &conn,
        &settings.host,
        settings.port,
        &settings.user,
        run_id,
        &SessionPolicy::default(),
    )?;

    // RFC 0157: refuse before doing work, not halfway through. A run against a lagging replica
    // produces divergences that are really just lag.
    if let Some(lag) = src.guard_replica_lag(settings.max_replica_lag_seconds)? {
        println!(
            "  replica lag : {lag:.1}s (within {:.1}s)",
            settings.max_replica_lag_seconds
        );
    }
    Ok(src)
}

fn project_source(store: &dyn ekos_ledger::KnowledgeStore, name: &str) -> Result<String> {
    let obj = Project::load(store, name)?
        .ok_or_else(|| anyhow::anyhow!("no migration project named '{name}'"))?;
    obj.properties
        .get("source")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("project '{name}' has no source connection"))
}

/// `ekos migrate discover` — read the live catalog, create a unit per table, record drift.
pub fn discover(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    schemas: Vec<String>,
) -> Result<()> {
    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;
    let dsn = project_source(store.as_ref(), &name)?;

    println!("Discovering {dsn}");
    // Database work on its own thread; the store is not touched inside it.
    let snapshot = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        let snapshot = ekos_pg_live::introspect(&src, &schemas, &config.redaction_config())?;
        ekos_pg_live::reconcile(&src, &snapshot, &schemas)?;
        Ok(snapshot)
    })?;

    // One unit per table-shaped relation. Views and functions become units when RFC 0163/0164 can
    // do something with them; creating them now would put units in the state machine that nothing
    // can advance.
    let mut created = 0;
    let mut live_shapes: Vec<TableShape> = Vec::new();
    for kind in [ObjectKind::Table, ObjectKind::PartitionedTable] {
        for o in snapshot.of_kind(kind) {
            live_shapes.push(TableShape {
                table: TableRef::parse(&o.qualified_name),
                columns: columns_of(&snapshot, &o.qualified_name),
            });
            Unit {
                project: name.clone(),
                key: o.qualified_name.clone(),
                state: UnitState::Discovered,
                wave: None,
            }
            .create(store.as_ref(), &run_id)?;
            created += 1;
        }
    }

    // Drift against what the compiled ledger already knows from the repository's DDL (RFC 0146).
    let repo_shapes = repo_table_shapes(store.as_ref())?;
    let drifts = drift::reconcile(&live_shapes, &repo_shapes);
    profile_facts::write_drift(store.as_ref(), &name, &drifts, &run_id)?;

    println!("\nCatalog:");
    for (kind, count) in snapshot.counts() {
        println!("  {:<22} {count}", kind.as_str());
    }
    println!("\n{created} migration unit(s) discovered.");
    if repo_shapes.is_empty() {
        // Reporting "0 drift" from an empty comparison would be a clean bill of health nobody
        // earned — the same failure shape as a tier reporting green with no controls.
        println!(
            "\nDrift: not checked — this ledger has no compiled Table objects to compare against.\n\
             Run `ekos build && ekos recover && ekos compile && ekos commit` over the repository \
             that owns this schema first."
        );
    } else if drifts.is_empty() {
        println!("\nDrift: none. The live schema matches the repository's DDL.");
    } else {
        println!("\nDrift ({} finding(s), recorded as facts):", drifts.len());
        let mut by_kind: std::collections::BTreeMap<&str, usize> = Default::default();
        for d in &drifts {
            *by_kind.entry(d.detail.as_str()).or_default() += 1;
        }
        for (kind, n) in &by_kind {
            println!("  {kind:<22} {n}");
        }
        let structural = drifts.iter().filter(|d| d.detail.is_structural()).count();
        if structural > 0 {
            println!(
                "\n  {structural} of these change what a migration would produce. The first few:"
            );
            for d in drifts.iter().filter(|d| d.detail.is_structural()).take(5) {
                println!("    {} — {}", d.object, d.message);
            }
        }
    }
    Ok(())
}

/// The columns of one table, from the catalog snapshot.
///
/// Column objects are named `schema.table.column`, so the table's own qualified name plus a dot is
/// the prefix — and the *rightmost* dot separates the column, because a schema or table name can
/// itself contain one.
fn columns_of(snapshot: &ekos_pg_live::CatalogSnapshot, table: &str) -> Vec<ColumnRef> {
    let prefix = format!("{table}.");
    snapshot
        .of_kind(ObjectKind::Column)
        .filter(|c| c.qualified_name.starts_with(&prefix))
        .map(|c| ColumnRef {
            name: c.qualified_name[prefix.len()..].to_string(),
            data_type: c
                .detail
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

/// Read the repository's own view of its tables out of the compiled ledger (RFC 0146).
///
/// `ekos_recovery`'s SQL analyzer stores a table's columns as a `columns` property — an array of
/// `{name, data_type}` — rather than as separate `Column` objects, so this reads that shape. A
/// `Table` with no `columns` property contributes a table with no columns, which reconciliation
/// treats as "nothing to compare" rather than "every column was deleted".
fn repo_table_shapes(store: &dyn ekos_ledger::KnowledgeStore) -> Result<Vec<TableShape>> {
    let mut out = Vec::new();
    for o in store.all_objects()? {
        if !matches!(o.kind, ekos_kir::ObjectKind::Table) {
            continue;
        }
        let columns = o
            .properties
            .get("columns")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        Some(ColumnRef {
                            name: c.get("name")?.as_str()?.to_string(),
                            data_type: c
                                .get("data_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(TableShape {
            table: TableRef::parse(&o.name),
            columns,
        });
    }
    Ok(out)
}

/// `ekos migrate profile` — profile every discovered unit at the requested tier.
pub fn profile(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    tier: String,
    unit: Option<String>,
) -> Result<()> {
    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;
    let dsn = project_source(store.as_ref(), &name)?;

    let units = Unit::all_in(store.as_ref(), &name)?;
    let targets: Vec<_> = units
        .iter()
        .filter(|(o, _)| unit.as_ref().is_none_or(|u| &o.name == u))
        .collect();
    if targets.is_empty() {
        bail!("no matching units. Run `ekos migrate discover` first.");
    }
    let names: Vec<String> = targets.iter().map(|(o, _)| o.name.clone()).collect();

    // Every database read first, on its own thread. Nothing here touches the ledger.
    let measured = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        let mut out = Vec::new();
        for name in &names {
            let table = profile_table_p0_at(&src, name, &tier)?;
            let mut cols =
                ekos_pg_live::profile::profile_columns_p0(&src, name, &config.redaction_config())?;
            if tier != "p0" {
                cols = ekos_pg_live::profile::profile_columns_p1(&src, name, cols, 10.0, 500)?;
            }
            out.push((table, cols));
        }
        Ok(out)
    })?;

    // Then the writes, on the main thread, with the ledger handle that never left it.
    let mut suppressed_columns = 0usize;
    for ((obj, state), (table, cols)) in targets.iter().zip(&measured) {
        suppressed_columns += cols.iter().filter(|c| c.values_suppressed).count();

        let table_fact = ekos_migrate::TableProfileFact::from(table);
        let col_facts: Vec<ekos_migrate::ColumnProfileFact> = cols.iter().map(Into::into).collect();
        profile_facts::write_table_profile(
            store.as_ref(),
            &name,
            &table_fact,
            &col_facts,
            &run_id,
        )?;

        println!(
            "  {:<40} {:>10} rows  {:>3} cols{}",
            obj.name,
            table.row_count,
            cols.len(),
            if table.row_count_is_exact {
                ""
            } else {
                " (est)"
            }
        );

        // Profiling is what `discovered → profiled` means; the transition is a fact like any other.
        if *state == UnitState::Discovered {
            project::transition(
                store.as_ref(),
                &obj.id,
                UnitState::Profiled,
                "policy",
                &format!("profiled at {tier}"),
                &run_id,
            )?;
        }
    }

    println!("\n{} unit(s) profiled at {tier}.", targets.len());
    if suppressed_columns > 0 {
        println!(
            "{suppressed_columns} column(s) classified as personal data: no bounds and no top-k \
             recorded for them, at any tier."
        );
    }
    Ok(())
}

fn profile_table_p0_at(src: &PgSource, table: &str, tier: &str) -> Result<pgprofile::TableProfile> {
    let mut p = pgprofile::profile_table_p0(src, table)?;
    if tier == "p2" {
        // P2 asks the planner first and refuses above budget rather than scanning (RFC 0157).
        match pgprofile::exact_row_count(src, table, 50_000_000.0)? {
            Ok(n) => {
                p.row_count = n;
                p.row_count_is_exact = true;
                p.tier = pgprofile::ProfileTier::P2;
            }
            Err(over) => bail!(
                "exact count of {table} would scan an estimated {:.0} rows, above the \
                 {:.0}-row budget. RFC 0161 turns this into an approval request; for now, \
                 profile at p0 or p1.",
                over.estimate.estimated_rows,
                over.budget_rows
            ),
        }
    }
    Ok(p)
}

/// `ekos migrate assess` — run the rule catalog against the live source, measure affected rows,
/// and record every finding as a fact.
pub fn assess(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    unit: Option<String>,
    measure: bool,
) -> Result<()> {
    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;
    let dsn = project_source(store.as_ref(), &name)?;

    let units = Unit::all_in(store.as_ref(), &name)?;
    let targets: Vec<String> = units
        .iter()
        .filter(|(o, _)| unit.as_ref().is_none_or(|u| &o.name == u))
        .map(|(o, _)| o.name.clone())
        .collect();
    if targets.is_empty() {
        bail!("no matching units. Run `ekos migrate discover` first.");
    }

    // Everything the rules need to decide, read from the profile facts already in the ledger plus
    // the live catalog. Then the measurements. All of it on the database thread.
    let findings = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        let mut out: Vec<ekos_migrate_dq::Finding> = Vec::new();
        for table in &targets {
            let profile = ekos_pg_live::profile::profile_table_p0(&src, table)?;
            let cols =
                ekos_pg_live::profile::profile_columns_p0(&src, table, &config.redaction_config())?;
            let cols = ekos_pg_live::profile::profile_columns_p1(&src, table, cols, 10.0, 500)?;

            out.extend(ekos_migrate_dq::evaluate_table(
                &ekos_migrate_dq::TableContext {
                    table: table.clone(),
                    row_count: profile.row_count,
                    has_updates: profile.has_updates(),
                    looks_static: profile.looks_static(),
                    unvalidated_constraints: unvalidated_constraints(&src, table)?,
                    primary_key_columns: primary_key_columns(&src, table)?,
                },
            ));

            for c in &cols {
                let column = c
                    .qualified_name
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let ctx = ekos_migrate_dq::ColumnContext {
                    table: table.clone(),
                    column,
                    data_type: c.data_type.clone(),
                    nullable: c.null_fraction > 0.0 || c.data_type.is_empty(),
                    null_fraction: Some(c.null_fraction),
                    distinct_estimate: c.distinct_estimate,
                    numeric_precision_used: c.numeric_precision_used,
                    numeric_scale_used: c.numeric_scale_used,
                    pii: c.values_suppressed,
                    row_count: profile.row_count,
                };
                for mut f in ekos_migrate_dq::evaluate_column(&ctx) {
                    // A measurement that fails is left unmeasured rather than recorded as zero:
                    // "we could not count" and "there are none" are different answers, and only one
                    // of them is a disposition.
                    if let (true, Some(sql)) = (measure, &f.evidence_sql) {
                        f.affected_rows = ekos_pg_live::profile::estimate_cost(&src, sql)
                            .ok()
                            .and(src.raw_query(sql).ok())
                            .and_then(|rows| rows.first()?.first()?.trim().parse::<i64>().ok());
                    }
                    out.push(f);
                }
            }
        }
        Ok(out)
    })?;

    let facts: Vec<ekos_migrate::FindingFact> = findings
        .iter()
        .map(|f| ekos_migrate::FindingFact {
            rule_id: f.rule_id.clone(),
            family: f.family.as_str().to_string(),
            severity: format!("{:?}", f.severity).to_lowercase(),
            target: f.target.map(|t| t.as_str().to_string()),
            lossiness: f.lossiness.map(|l| l.as_str().to_string()),
            object: f.object.clone(),
            message: f.message.clone(),
            affected_rows: f.affected_rows,
            evidence_sql: f.evidence_sql.clone(),
            blocks: f.blocks(),
        })
        .collect();
    profile_facts::write_findings(store.as_ref(), &name, &facts, &run_id)?;

    report_findings(&findings, measure);

    // RFC 0154's coverage promise, made visible. Dispositions arrive with RFC 0161, so today this
    // reports the denominator and what is unaccounted for rather than gating on it — but it reports
    // the real denominator, which is the part that stops "we handled what we thought of" from
    // looking like completeness.
    // Scoped to the schemas the assessed units live in. An unscoped introspection would put every
    // other schema in the database into the denominator, which makes the number honest about the
    // server and dishonest about the migration.
    let schemas: Vec<String> = targets
        .iter()
        .filter_map(|t| t.split_once('.').map(|(s, _)| s.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let catalog = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        Ok(ekos_pg_live::introspect(
            &src,
            &schemas,
            &config.redaction_config(),
        )?)
    })?;
    report_completeness(&catalog, &findings);
    assess_inferred_keys(
        config,
        store.as_ref(),
        &name,
        &dsn,
        &run_id,
        &targets,
        measure,
    )?;

    // Assessment is what `profiled → assessed` means. Units with a blocking finding stay put:
    // advancing them would say the findings had been dealt with.
    let blocked: std::collections::BTreeSet<&str> = findings
        .iter()
        .filter(|f| f.blocks() && !f.is_theoretical())
        .map(|f| f.object.as_str())
        .collect();
    for (obj, state) in &units {
        if *state != UnitState::Profiled || !targets.contains(&obj.name) {
            continue;
        }
        if blocked.iter().any(|b| b.starts_with(&obj.name)) {
            continue;
        }
        project::transition(
            store.as_ref(),
            &obj.id,
            UnitState::Assessed,
            "policy",
            "assessed with no blocking findings",
            &run_id,
        )?;
    }
    Ok(())
}

fn report_findings(findings: &[ekos_migrate_dq::Finding], measured: bool) {
    if findings.is_empty() {
        println!("No findings.");
        return;
    }
    let blocking: Vec<_> = findings
        .iter()
        .filter(|f| f.blocks() && !f.is_theoretical())
        .collect();
    let theoretical = findings.iter().filter(|f| f.is_theoretical()).count();

    println!("{} finding(s):\n", findings.len());
    for f in findings {
        let rows = match (f.affected_rows, measured) {
            (Some(n), _) => format!("{n} rows"),
            // The distinction the whole design rests on: a rule nobody ran is not a rule that
            // found nothing.
            (None, true) => "not measurable".into(),
            (None, false) => "not measured".into(),
        };
        let mark = if f.blocks() && !f.is_theoretical() {
            "BLOCK"
        } else {
            "     "
        };
        println!("  {mark} {:<34} {:<14} {}", f.rule_id, rows, f.object);
        println!("        {}", f.message);
    }

    println!();
    if theoretical > 0 {
        println!(
            "{theoretical} rule(s) matched but affect zero rows — the cheapest disposition there is."
        );
    }
    if blocking.is_empty() {
        println!("Nothing blocking.");
    } else {
        println!(
            "{} blocking finding(s) need a disposition before these units can advance.",
            blocking.len()
        );
    }
}

/// `ekos migrate review` — list pending requests, or raise one for a unit.
pub fn review(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    unit: Option<String>,
    environment: String,
) -> Result<()> {
    use ekos_migrate_approval as ap;

    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;

    let Some(unit) = unit else {
        let requests = load_approvals(store.as_ref(), &name)?;
        if requests.is_empty() {
            println!("No approval requests. Raise one with `ekos migrate review --unit <unit>`.");
            return Ok(());
        }
        println!("{} request(s):", requests.len());
        for r in &requests {
            println!(
                "  {:<28} {:<4} {}",
                r.id,
                r.risk.class.as_str(),
                status_word(&r.status)
            );
            println!("        {}", r.risk.summary());
        }
        return Ok(());
    };

    // Raising a request is not approving one, so an agent may do it — and the requester is recorded
    // precisely so that whoever approves cannot be the same identity.
    let dsn = project_source(store.as_ref(), &name)?;
    let target_db = ConnectionRef::parse(&project_target(store.as_ref(), &name)?)?.database;
    let env: EnvArg = environment
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let policy = ap::load_policy(&cwd.join(&config.migrate.policy))?;
    let blast = blast_radius(store.as_ref(), &unit);

    // A request covers **every** artifact the load will execute, not just the DDL. RFC 0161 says
    // "the artifact ids and their content hashes", plural, and the first version froze only the DDL
    // — which approved the create and then refused the very first chunk. One decision, one set of
    // statements.
    let artifacts = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        plan_artifacts(config, &src, &unit, &target_db, env.0, &run_id)
    })?;
    let artifact = artifacts
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{unit} produced no artifacts to approve"))?;

    // The evidence: every profile and finding fact the decision rests on, with its content hash
    // frozen. If any of it changes, the request dies rather than being silently re-validated.
    let evidence = ap::EvidenceSnapshot::of(evidence_for(store.as_ref(), &name, &unit)?);

    let risk = ap::assess(
        &ap::ActionFacts {
            statement_class: ekos_migrate_target_clickhouse::batch_class(&artifact.sql)?
                .as_str()
                .to_string(),
            environment: env.0.as_str().to_string(),
            lossiness: None,
            blast_radius: blast,
            affected_rows: None,
        },
        &policy.policy.thresholds,
    );

    let request = ap::ApprovalRequest {
        id: format!("REQ:{unit}:{}", env.0.as_str()),
        artifacts: artifacts
            .iter()
            .map(|a| ap::EvidenceRef {
                fact_id: a.id.clone(),
                content_hash: a.hash.clone(),
            })
            .collect(),
        risk: risk.clone(),
        evidence,
        requester: format!("cli:{}", whoami()),
        status: ap::RequestStatus::Pending,
        evidence_shown: false,
        typed_confirmation: None,
    };

    profile_facts::write_approval(
        store.as_ref(),
        &name,
        &request.id,
        &serde_json::to_value(&request)?,
        &run_id,
    )?;

    println!("Raised {}", request.id);
    println!("  risk     : {}", risk.summary());
    println!(
        "  needs    : {} distinct approver(s)",
        risk.class.approvers_required()
    );
    if risk.class.requires_evidence_review() {
        println!("  evidence : must be rendered and that recorded (pass --show-evidence)");
    }
    if risk.class.requires_typed_confirmation() {
        println!("  confirm  : the approver must type `{unit}` exactly");
    }
    println!(
        "  covers   : {} artifact(s) — the DDL and every chunk, so one decision covers the load",
        request.artifacts.len()
    );
    println!(
        "  evidence : {} fact(s) frozen",
        request.evidence.refs.len()
    );
    println!(
        "  requester: {} — whoever approves must be someone else",
        request.requester
    );
    println!(
        "\nApprove with `ekos migrate approve {} --as <you>`.",
        request.id
    );
    Ok(())
}

/// What a human decided, and the evidence they were shown.
pub struct Decision {
    pub request_id: String,
    pub subjects: Vec<String>,
    /// `Some` rejects with this reason; `None` approves.
    pub reject_reason: Option<String>,
    pub typed_confirmation: Option<String>,
    pub evidence_shown: bool,
}

/// `ekos migrate approve` / `reject`.
pub fn decide(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    decision: Decision,
) -> Result<()> {
    let Decision {
        request_id,
        subjects,
        reject_reason,
        typed_confirmation: confirm,
        evidence_shown: show_evidence,
    } = decision;
    use ekos_migrate_approval as ap;

    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;

    let mut request = load_approvals(store.as_ref(), &name)?
        .into_iter()
        .find(|r| r.id == request_id)
        .ok_or_else(|| {
            anyhow::anyhow!("no request {request_id}. `ekos migrate review` lists them.")
        })?;

    let unit = request_id
        .strip_prefix("REQ:")
        .and_then(|r| r.rsplit_once(':').map(|(u, _)| u.to_string()))
        .unwrap_or_default();

    if let Some(reason) = reject_reason {
        let who = subjects.first().cloned().unwrap_or_else(whoami);
        ap::lifecycle::reject(&mut request, ap::Actor::Human, &who, &reason)?;
        println!("Rejected {request_id}: {reason}");
    } else {
        if subjects.is_empty() {
            bail!("who is approving? Pass --as <subject> (twice for an R4 action).");
        }
        request.evidence_shown = show_evidence;
        // The evidence is re-hashed from the ledger *now*, never trusted from the request.
        let current = current_hashes(store.as_ref(), &name)?;
        ap::lifecycle::approve(
            &mut request,
            ap::Actor::Human,
            &subjects,
            &unit,
            confirm.as_deref(),
            &|id| current.get(id).cloned(),
            &chrono::Utc::now().to_rfc3339(),
        )?;
        println!("Approved {request_id} ({})", request.risk.class.as_str());
    }

    profile_facts::write_approval(
        store.as_ref(),
        &name,
        &request.id,
        &serde_json::to_value(&request)?,
        &run_id,
    )?;
    Ok(())
}

fn status_word(s: &ekos_migrate_approval::RequestStatus) -> &'static str {
    use ekos_migrate_approval::RequestStatus as S;
    match s {
        S::Pending => "pending",
        S::Approved { .. } => "approved",
        S::Rejected { .. } => "rejected",
        S::Dead { .. } => "dead (evidence changed)",
    }
}

/// A newtype so `--env` parses once, in one place.
struct EnvArg(ekos_migrate_target_clickhouse::Environment);

impl std::str::FromStr for EnvArg {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        use ekos_migrate_target_clickhouse::Environment as E;
        Ok(EnvArg(match s {
            "sandbox" => E::Sandbox,
            "staging" => E::Staging,
            "production" | "prod" => E::Production,
            other => return Err(format!("unknown environment '{other}'")),
        }))
    }
}

/// The facts a decision about `unit` rests on: its profiles and its findings.
fn evidence_for(
    store: &dyn ekos_ledger::KnowledgeStore,
    project: &str,
    unit: &str,
) -> Result<Vec<ekos_migrate_approval::EvidenceRef>> {
    Ok(current_hashes(store, project)?
        .into_iter()
        .filter(|(id, _)| id.contains(unit))
        .map(
            |(fact_id, content_hash)| ekos_migrate_approval::EvidenceRef {
                fact_id,
                content_hash,
            },
        )
        .collect())
}

/// Every migration fact's current content hash, keyed by name.
///
/// Keyed by *name* rather than KirId so the snapshot survives a re-profile that writes a new version
/// of the same logical fact — which is exactly the change that must invalidate an approval.
fn current_hashes(
    store: &dyn ekos_ledger::KnowledgeStore,
    project: &str,
) -> Result<std::collections::BTreeMap<String, String>> {
    let mut out = std::collections::BTreeMap::new();
    for o in store.all_objects()? {
        let ekos_kir::ObjectKind::Custom(kind) = &o.kind else {
            continue;
        };
        if !matches!(
            kind.as_str(),
            k if k == ekos_migrate::kinds::TABLE_PROFILE_KIND
                || k == ekos_migrate::kinds::COLUMN_PROFILE_KIND
                || k == ekos_migrate::kinds::FINDING_KIND
        ) {
            continue;
        }
        if o.properties.get("project").and_then(|v| v.as_str()) != Some(project) {
            continue;
        }
        // **Sorted** before hashing. `KirObject::properties` is a `HashMap`, and serializing one
        // gives a different key order on every read — so a hash taken over it is not a content hash
        // at all, and an evidence snapshot compared against it reports *every* fact as changed
        // immediately after being frozen. Caught the first time a request was raised and approved
        // back to back.
        let sorted: std::collections::BTreeMap<&String, &serde_json::Value> =
            o.properties.iter().collect();
        let body = serde_json::to_string(&sorted).unwrap_or_default();
        out.insert(
            format!("{kind}:{}", o.name),
            ekos_common::ContentHash::of_str(&body).as_str().to_string(),
        );
    }
    Ok(out)
}

/// How many objects depend on the unit being changed.
///
/// RFC 0161's blast radius, and the number EKOS has that a schema-only migration tool does not: the
/// compiled CKM already knows which views, functions, ETL steps and application files reference this
/// table. A change touching one consumer and a change touching forty are genuinely different
/// actions, and this is what tells them apart.
fn blast_radius(store: &dyn ekos_ledger::KnowledgeStore, unit: &str) -> usize {
    let bare = unit.rsplit('.').next().unwrap_or(unit).to_ascii_lowercase();
    let Ok(objects) = store.all_objects() else {
        return 0;
    };
    let Some(target) = objects.iter().find(|o| {
        matches!(o.kind, ekos_kir::ObjectKind::Table)
            && o.name.to_ascii_lowercase().ends_with(&bare)
    }) else {
        // No compiled Table object means no impact graph to consult — which reads as zero, and the
        // assessment's own "not measured" language is what keeps that honest.
        return 0;
    };
    store
        .relationships_for(&target.id)
        .map(|rels| {
            rels.into_iter()
                .filter(|r| r.to == target.id)
                .map(|r| r.from.to_string())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        })
        .unwrap_or(0)
}

/// Approval requests recorded for this project.
fn load_approvals(
    store: &dyn ekos_ledger::KnowledgeStore,
    project: &str,
) -> Result<Vec<ekos_migrate_approval::ApprovalRequest>> {
    let mut out = Vec::new();
    for o in store.all_objects()? {
        if !matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == ekos_migrate::kinds::APPROVAL_KIND)
        {
            continue;
        }
        if o.properties.get("project").and_then(|v| v.as_str()) != Some(project) {
            continue;
        }
        if let Some(body) = o.properties.get("request")
            && let Ok(r) = serde_json::from_value(body.clone())
        {
            out.push(r);
        }
    }
    Ok(out)
}

/// A ClickHouse HTTP client for the executor. Read and write share one path so the classifier
/// gates both.
struct ChClient {
    url: String,
}

impl ChClient {
    /// The target resolves through `[migrate.connections.<alias>]`, exactly like the source: the
    /// DSN names an alias, the alias names a host, and the password lives in the environment
    /// variable the config only *names*.
    fn from_alias(config: &EkosConfig, alias: &str) -> Result<Self> {
        let settings = resolve_alias(config, alias)?;
        let password = settings
            .secret_env
            .as_ref()
            .and_then(|v| std::env::var(v).ok())
            .unwrap_or_default();
        Ok(Self {
            url: format!(
                "http://{}:{}/?user={}&password={}",
                settings.host, settings.port, settings.user, password
            ),
        })
    }

    fn run(&self, sql: &str) -> Result<String> {
        let out = std::process::Command::new("curl")
            .args(["-s", "--fail-with-body", &self.url, "--data-binary", sql])
            .output()?;
        let body = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.status.success() {
            anyhow::bail!("clickhouse: {body}");
        }
        Ok(body)
    }
}

impl ekos_migrate_validate::EngineReader for ChClient {
    fn dialect(&self) -> ekos_migrate_validate::Dialect {
        ekos_migrate_validate::Dialect::ClickHouse
    }
    fn label(&self) -> &str {
        // RFC 0156's independent-oracle rule in its smallest honest form: the label records *which
        // path* read the target, which is what makes the rule auditable rather than aspirational.
        "clickhouse:reader"
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ekos_migrate_validate::ReadError> {
        // TabSeparatedRaw, because the default format escapes backslashes on output and the
        // canonical form is full of them (devlog_207).
        let body = self
            .run(&format!("{sql} FORMAT TabSeparatedRaw"))
            .map_err(|e| ekos_migrate_validate::ReadError::Query {
                engine: "clickhouse".into(),
                message: e.to_string(),
            })?;
        Ok(body
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.split('\t').map(str::to_string).collect())
            .collect())
    }
}

/// `ekos migrate validate` — run the RFC 0156 tiers over a loaded unit.
pub fn validate(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    unit: String,
    tier: String,
) -> Result<()> {
    use ekos_migrate_target_clickhouse as ch;
    use ekos_migrate_validate as v;

    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;
    let dsn = project_source(store.as_ref(), &name)?;
    let target_ref = ConnectionRef::parse(&project_target(store.as_ref(), &name)?)?;
    let target_db = target_ref.database.clone();
    let bare = unit.rsplit('.').next().unwrap_or(&unit).to_string();

    let target = ChClient::from_alias(config, &target_ref.alias)?;

    // The plan needs the same column list on both sides, rendered per dialect. It is built from the
    // mapping so the two sides agree on order — RFC 0155 joins columns in the *approved target*
    // order, not the source catalog's.
    let outcome = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        let lsn = src.current_lsn()?;
        let cols =
            ekos_pg_live::profile::profile_columns_p0(&src, &unit, &config.redaction_config())?;
        let nullability = column_nullability(&src, &unit)?;
        let pk = primary_key_columns(&src, &unit)?;
        let key = pk
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{unit} has no primary key to bucket on"))?;

        let mut source_columns = Vec::new();
        let mut target_columns = Vec::new();
        let mut names = Vec::new();
        for c in &cols {
            let column = c.qualified_name.rsplit('.').next().unwrap_or_default();
            let mapping = ch::map_column(
                column,
                &c.data_type,
                nullability.get(column).copied().unwrap_or(true),
                &ch::ColumnEvidence::default(),
            );
            let Some(rule) = column_rule_for(&c.data_type, &mapping.target_type) else {
                // A column whose canonical form has no rule is skipped and named, never silently
                // folded into the hash on one side only.
                println!("  skipping {column}: no canonical rule for {}", c.data_type);
                continue;
            };
            source_columns.push(v::canon_expr(v::Dialect::Postgres, column, rule));
            target_columns.push(v::canon_expr(v::Dialect::ClickHouse, column, rule));
            names.push(column.to_string());
        }

        let (schema, table) = unit.split_once('.').unwrap_or(("public", &unit));
        let plan = v::UnitPlan {
            unit: unit.clone(),
            source_table: format!("\"{schema}\".\"{table}\""),
            target_table: format!("`{target_db}`.`{bare}`"),
            source_pk: v::canon_expr(v::Dialect::Postgres, &key, v::ColumnRule::Int),
            target_pk: v::canon_expr(v::Dialect::ClickHouse, &key, v::ColumnRule::Int),
            source_columns,
            target_columns,
            buckets: 64,
        };

        let mut outcomes = Vec::new();
        outcomes.push(v::tiers::run_v1(&plan, &src, &target)?);
        if tier != "v1" {
            outcomes.push(v::tiers::run_v2(&plan, &src, &target, &names)?);
        }
        if tier == "v3" || tier == "v4" {
            outcomes.push(v::tiers::run_v3(&plan, &src, &target)?);
        }
        Ok((outcomes, lsn, plan))
    })?;
    let (outcomes, lsn, plan) = outcome;

    println!("{unit} -> {target_db}.{bare}");
    println!("  source LSN: {lsn}");
    let mut all_passed = true;
    for o in &outcomes {
        println!("  {}", o.verdict());
        println!("    read via {} and {}", o.source_path, o.target_path);
        for d in &o.divergences {
            println!("    {} — {}", d.locus, d.detail);
        }
        all_passed &= o.passed();
    }

    if all_passed {
        println!(
            "\nEvery tier run passed. Note: no planted controls were run, so this says the \
                  tiers found nothing — not that they would have."
        );
        if let Some((obj, _)) = units_named(store.as_ref(), &name, &unit)? {
            let _ = project::transition(
                store.as_ref(),
                &obj.id,
                UnitState::Validated,
                "policy",
                &format!("validated at {tier} against source LSN {lsn}"),
                &run_id,
            );
        }
    } else {
        println!("\nValidation failed. Bisect the failed buckets with RFC 0156's V4 path.");
    }
    let _ = plan;
    Ok(())
}

/// Which RFC 0155 canonical rule applies to a source type.
fn column_rule_for(
    source_type: &str,
    _target_type: &str,
) -> Option<ekos_migrate_validate::ColumnRule> {
    use ekos_migrate_validate::ColumnRule as R;
    let base = source_type
        .split_once('(')
        .map(|(h, _)| h)
        .unwrap_or(source_type)
        .trim();
    Some(match base {
        "bigint" | "integer" | "smallint" => R::Int,
        "boolean" => R::Bool,
        "text" | "character varying" => R::Text,
        "character" => R::Char,
        "numeric" | "decimal" => {
            let scale = source_type
                .split_once(',')
                .and_then(|(_, s)| s.trim_end_matches(')').trim().parse().ok())
                .unwrap_or(0);
            R::Decimal(scale)
        }
        "timestamp with time zone" => R::TimestampUtc,
        "timestamp without time zone" => R::TimestampNaive,
        "date" => R::Date,
        "uuid" => R::Uuid,
        "bytea" => R::Bytes,
        "inet" | "cidr" => R::Inet,
        // Floats and JSON are excluded from hashing by RFC 0155, and anything unrecognized is
        // skipped rather than guessed at.
        _ => return None,
    })
}

/// `ekos migrate load` — create the target table and copy the data, chunk by chunk.
///
/// Every statement goes through the RFC 0160 gate before it runs: parsed, classified, checked for an
/// inline credential, and — outside a sandbox — matched against an approval by artifact id, hash and
/// environment.
pub fn load(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    unit: String,
    environment: String,
    chunk_rows: i64,
    dry_run: bool,
) -> Result<()> {
    use ekos_migrate_target_clickhouse as ch;

    require_enabled(config)?;
    // The flag overrides the config for this run. `review` reads the config, so overriding it here
    // deliberately changes the artifact set and invalidates an approval raised without the override
    // — which the hash check then catches, rather than quietly loading a different set of chunks.
    let mut config = config.clone();
    if chunk_rows > 0 {
        config.migrate.chunk_rows = chunk_rows;
    }
    let config = &config;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;
    let dsn = project_source(store.as_ref(), &name)?;
    let target_ref = ConnectionRef::parse(&project_target(store.as_ref(), &name)?)?;
    let target_db = target_ref.database.clone();

    let env: ch::Environment = match environment.as_str() {
        "sandbox" => ch::Environment::Sandbox,
        "staging" => ch::Environment::Staging,
        "production" | "prod" => ch::Environment::Production,
        other => bail!("unknown environment '{other}' (sandbox | staging | production)"),
    };

    // The same planner `review` used, so an approval covers the statements that actually run.
    let artifacts = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        plan_artifacts(config, &src, &unit, &target_db, env, &run_id)
    })?;
    let chunks = artifacts.len().saturating_sub(1);
    let bare = unit.rsplit('.').next().unwrap_or(&unit).to_string();

    // RFC 0161: compute the risk from the situation, not the category, and gate anything above R1
    // on a real approval whose evidence still matches. `blast_radius` is the number EKOS has and a
    // schema-only migration tool does not.
    let policy = ekos_migrate_approval::load_policy(&cwd.join(&config.migrate.policy))?;
    let blast = blast_radius(store.as_ref(), &unit);
    let approvals = load_approvals(store.as_ref(), &name)?;

    println!(
        "{unit} -> {target_db}.{bare} ({}, {} chunk(s))",
        env.as_str(),
        chunks
    );
    if !policy.from_file {
        println!(
            "  policy   : defaults ({} not found). Worth saying in a report: \"the default policy \
             allowed it\" is a different statement from \"our policy allowed it\".",
            config.migrate.policy.display()
        );
    }

    for a in &artifacts {
        let risk = ekos_migrate_approval::assess(
            &ekos_migrate_approval::ActionFacts {
                statement_class: ch::batch_class(&a.sql)?.as_str().to_string(),
                environment: env.as_str().to_string(),
                lossiness: None,
                blast_radius: blast,
                affected_rows: None,
            },
            &policy.policy.thresholds,
        );

        // The two gates compose rather than duplicate: RFC 0161 decides *whether* an approval is
        // needed and finds it, RFC 0160 checks that the approval matches this artifact's hash and
        // environment. Neither is sufficient alone — a valid approval for a different statement is
        // exactly what the hash check exists to catch.
        let approval = if risk.class.is_automatic() {
            None
        } else {
            match approvals.iter().find(|r| r.authorizes(&a.id, &a.hash)) {
                Some(req) => Some(ch::Approval {
                    artifact_id: a.id.clone(),
                    artifact_hash: a.hash.clone(),
                    environment: env,
                    approver: match &req.status {
                        ekos_migrate_approval::RequestStatus::Approved { approvers, .. } => {
                            approvers.join(", ")
                        }
                        _ => String::new(),
                    },
                }),
                None => bail!(
                    "{} is {} and has no matching approval.\n  {}\n\nRaise one with \
                     `ekos migrate review --unit {unit} --env {}`, then approve it. Approving is \
                     human-only and has no MCP equivalent, by design.",
                    a.id,
                    risk.class.as_str(),
                    risk.summary(),
                    env.as_str()
                ),
            }
        };

        match ch::authorize(a, env, approval.as_ref()) {
            Ok(class) => {
                if dry_run {
                    println!("  [{}] {} ({})", class.as_str(), a.id, &a.hash[..12]);
                } else {
                    let client = ChClient::from_alias(config, &target_ref.alias)?;
                    client.run(&a.sql)?;
                    println!("  [{}] {} ok", class.as_str(), a.id);
                }
            }
            Err(e) => bail!("{} refused: {e}", a.id),
        }
    }

    if dry_run {
        println!(
            "\nDry run: nothing executed. {} artifact(s) passed the gate.",
            artifacts.len()
        );
        return Ok(());
    }

    // Loading is what `mapped -> loaded` means.
    if let Some((obj, _)) = units_named(store.as_ref(), &name, &unit)? {
        let _ = project::transition(
            store.as_ref(),
            &obj.id,
            UnitState::Loaded,
            "policy",
            &format!("loaded into {} in {}", target_db, env.as_str()),
            &run_id,
        );
    }
    println!("\nLoaded. Validate with `ekos migrate validate --unit {unit}`.");
    Ok(())
}

fn units_named(
    store: &dyn ekos_ledger::KnowledgeStore,
    project: &str,
    unit: &str,
) -> Result<Option<(ekos_kir::KirObject, UnitState)>> {
    Ok(Unit::all_in(store, project)?
        .into_iter()
        .find(|(o, _)| o.name == unit))
}

/// Every artifact a load of `unit` will execute, in order.
///
/// Shared by `review` and `load` on purpose: if the two built the set separately they could disagree,
/// and an approval covering a different set of statements than the one that runs is precisely what
/// the hash check exists to catch — better not to create the opportunity.
fn plan_artifacts(
    config: &EkosConfig,
    src: &PgSource,
    unit: &str,
    target_db: &str,
    _env: ekos_migrate_target_clickhouse::Environment,
    run_id: &str,
) -> Result<Vec<ekos_migrate_target_clickhouse::Artifact>> {
    use ekos_migrate_target_clickhouse as ch;

    let ddl = generate_ddl_for(config, src, unit, target_db, run_id)?;
    let pk = primary_key_columns(src, unit)?;
    let key = pk.first().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "{unit} has no primary key, so the load cannot be chunked by key range. RFC 0160's \
             ctid fallback is not implemented yet."
        )
    })?;
    let (schema, table) = unit.split_once('.').unwrap_or(("public", unit));
    let rows = src.raw_query(&format!(
        "SELECT COALESCE(min(\"{k}\"), 0), COALESCE(max(\"{k}\"), -1) FROM \"{s}\".\"{t}\"",
        k = key.replace('"', "\"\""),
        s = schema.replace('"', "\"\""),
        t = table.replace('"', "\"\"")
    ))?;
    let lo: i64 = rows[0][0].trim().parse().unwrap_or(0);
    let hi: i64 = rows[0][1].trim().parse().unwrap_or(-1);

    let bare = unit.rsplit('.').next().unwrap_or(unit);
    let mut out = vec![ch::Artifact::new(format!("{unit}:ddl"), ddl)];
    for c in ch::plan_chunks(lo, hi, config.migrate.chunk_rows) {
        out.push(ch::Artifact::new(
            format!("{unit}:chunk:{}", c.index),
            ch::chunk_insert(
                target_db,
                bare,
                schema,
                table,
                &config.migrate.source_named_collection,
                &key,
                &c,
            ),
        ));
    }
    Ok(out)
}

/// Re-derive the DDL for one unit. Kept separate from `map` so a load never depends on someone
/// having run `map --emit` first and kept the file.
fn generate_ddl_for(
    config: &EkosConfig,
    src: &PgSource,
    unit: &str,
    target_db: &str,
    _run_id: &str,
) -> Result<String> {
    use ekos_migrate_target_clickhouse as ch;

    let profile = ekos_pg_live::profile::profile_table_p0(src, unit)?;
    let cols = ekos_pg_live::profile::profile_columns_p0(src, unit, &config.redaction_config())?;
    let cols = ekos_pg_live::profile::profile_columns_p1(src, unit, cols, 10.0, 500)?;
    let nullability = column_nullability(src, unit)?;
    let pk = primary_key_columns(src, unit)?;
    let shapes = ekos_pg_live::workload::harvest(src, &config.redaction_config(), 2000)?;
    let (filters, _) = ekos_pg_live::workload::analyze(&shapes);

    let mappings: Vec<ch::Mapping> = cols
        .iter()
        .map(|c| {
            let column = c.qualified_name.rsplit('.').next().unwrap_or_default();
            ch::map_column(
                column,
                &c.data_type,
                nullability.get(column).copied().unwrap_or(true),
                &ch::ColumnEvidence {
                    null_fraction: Some(c.null_fraction),
                    distinct: ekos_pg_live::profile::distinct_count(
                        c.distinct_estimate,
                        profile.row_count,
                    ),
                    row_count: profile.row_count,
                    numeric_precision_used: c.numeric_precision_used,
                    numeric_scale_used: c.numeric_scale_used,
                    growing: c.monotonic == Some(true),
                    profile_ref: Some(format!("profile:{}", c.qualified_name)),
                },
            )
        })
        .collect();
    let design_columns: Vec<ch::DesignColumn> = cols
        .iter()
        .zip(&mappings)
        .map(|(c, m)| ch::DesignColumn {
            name: m.column.clone(),
            target_type: m.target_type.clone(),
            distinct: ekos_pg_live::profile::distinct_count(c.distinct_estimate, profile.row_count),
            monotonic: c.monotonic,
        })
        .collect();
    let design = ch::design(
        unit,
        &ch::TableEvidence {
            row_count: profile.row_count,
            has_updates: profile.has_updates(),
            update_time_column: None,
            primary_key: pk,
            filter_columns: ekos_pg_live::workload::filters_for(&filters, unit),
            time_column: None,
        },
        &design_columns,
    );
    Ok(ch::create_table(&design, &mappings, target_db)?)
}

/// `ekos migrate map` — choose target types and a table design for each unit, and emit DDL.
///
/// Everything it decides is derived from what the profiler measured, and everything it cannot
/// derive it says it cannot derive. The DDL carries its own reasoning in comments, so a human
/// approving it is not asked to go and find a report somewhere else.
pub fn map(
    config: &EkosConfig,
    cwd: &Path,
    project: Option<String>,
    unit: Option<String>,
    emit: Option<std::path::PathBuf>,
) -> Result<()> {
    use ekos_migrate_target_clickhouse as ch;

    require_enabled(config)?;
    let name = active_project(config, project)?;
    let run_id = new_run_id();
    let store = crate::commands::store::open_store(config, cwd)?;
    let dsn = project_source(store.as_ref(), &name)?;
    let target_db = ConnectionRef::parse(&project_target(store.as_ref(), &name)?)?.database;

    let units = Unit::all_in(store.as_ref(), &name)?;
    let targets: Vec<String> = units
        .iter()
        .filter(|(o, _)| unit.as_ref().is_none_or(|u| &o.name == u))
        .map(|(o, _)| o.name.clone())
        .collect();
    if targets.is_empty() {
        bail!("no matching units. Run `ekos migrate discover` first.");
    }

    let (measured, shape_count, filter_counts) = off_runtime(|| {
        let src = open_source(config, &dsn, &run_id)?;
        // The workload is what turns an ORDER BY from a default into a derivation.
        let shapes = ekos_pg_live::workload::harvest(&src, &config.redaction_config(), 2000)?;
        let (filters, _) = ekos_pg_live::workload::analyze(&shapes);
        let mut out = Vec::new();
        for table in &targets {
            let profile = ekos_pg_live::profile::profile_table_p0(&src, table)?;
            let cols =
                ekos_pg_live::profile::profile_columns_p0(&src, table, &config.redaction_config())?;
            let cols = ekos_pg_live::profile::profile_columns_p1(&src, table, cols, 10.0, 500)?;
            let nullability = column_nullability(&src, table)?;
            let pk = primary_key_columns(&src, table)?;
            out.push((table.clone(), profile, cols, nullability, pk));
        }
        Ok((out, shapes.len(), filters))
    })?;

    if shape_count == 0 {
        println!(
            "No workload evidence: pg_stat_statements is not installed, or holds nothing. Every \
             ORDER BY below falls back to the primary key and says so."
        );
    } else {
        println!("Workload: {shape_count} query shape(s) read from pg_stat_statements.");
    }

    let mut emitted = Vec::new();
    for (table, profile, cols, nullability, pk) in &measured {
        let mappings: Vec<ch::Mapping> = cols
            .iter()
            .map(|c| {
                let column = c.qualified_name.rsplit('.').next().unwrap_or_default();
                let distinct =
                    ekos_pg_live::profile::distinct_count(c.distinct_estimate, profile.row_count);
                let evidence = ch::ColumnEvidence {
                    null_fraction: Some(c.null_fraction),
                    distinct,
                    row_count: profile.row_count,
                    numeric_precision_used: c.numeric_precision_used,
                    numeric_scale_used: c.numeric_scale_used,
                    // The guard that stops the profiler's biggest win from becoming its biggest
                    // mistake: a monotonic column's domain keeps growing, so no measurement of its
                    // past can make a narrowing safe.
                    growing: c.monotonic == Some(true),
                    profile_ref: Some(format!("profile:{}", c.qualified_name)),
                };
                ch::map_column(
                    column,
                    &c.data_type,
                    nullability.get(column).copied().unwrap_or(true),
                    &evidence,
                )
            })
            .collect();

        let design_columns: Vec<ch::DesignColumn> = cols
            .iter()
            .zip(&mappings)
            .map(|(c, m)| ch::DesignColumn {
                name: m.column.clone(),
                target_type: m.target_type.clone(),
                distinct: ekos_pg_live::profile::distinct_count(
                    c.distinct_estimate,
                    profile.row_count,
                ),
                monotonic: c.monotonic,
            })
            .collect();

        let evidence = ch::TableEvidence {
            row_count: profile.row_count,
            has_updates: profile.has_updates(),
            update_time_column: cols
                .iter()
                .map(|c| c.qualified_name.rsplit('.').next().unwrap_or_default())
                .find(|n| matches!(*n, "updated_at" | "modified_at" | "last_modified"))
                .map(str::to_string),
            primary_key: pk.clone(),
            filter_columns: ekos_pg_live::workload::filters_for(&filter_counts, table),
            // A partition column needs a distinct-month estimate, which needs a scan the mapper
            // does not take. Left absent rather than guessed: an unpartitioned table is a safe
            // default and a wrongly-partitioned one is not.
            time_column: None,
        };

        let d = ch::design(table, &evidence, &design_columns);
        report_mapping(table, &mappings, &d);

        match ch::create_table(&d, &mappings, &target_db) {
            Ok(sql) => emitted.push(format!("{}\n{sql};\n", ch::rationale_comment(&d))),
            Err(e) => println!("  DDL not emitted: {e}"),
        }
    }

    if let Some(path) = emit {
        std::fs::write(&path, emitted.join("\n"))?;
        println!(
            "\nDDL for {} table(s) written to {}",
            emitted.len(),
            path.display()
        );
    } else if !emitted.is_empty() {
        println!("\nRe-run with --emit <path> to write the DDL.");
    }
    Ok(())
}

fn report_mapping(
    table: &str,
    mappings: &[ekos_migrate_target_clickhouse::Mapping],
    design: &ekos_migrate_target_clickhouse::TargetDesign,
) {
    use ekos_migrate_target_clickhouse::Lossiness;
    println!("\n{table}");
    println!("  engine   : {}", design.engine.render());
    println!("             {}", design.engine_rationale);
    println!("  order by : {}", design.order_by.join(", "));
    println!("             {}", design.order_by_rationale);
    if let Some(p) = &design.partition_by {
        println!("  partition: {p}");
    }
    println!("  columns  :");
    for m in mappings {
        let mark = match m.lossiness {
            Lossiness::Lossy => "LOSSY",
            Lossiness::NarrowingSafe => "narrow",
            _ => "      ",
        };
        println!(
            "    {mark} {:<22} {:<28} -> {}",
            m.column, m.source_type, m.target_type
        );
        if m.lossiness != Lossiness::Exact {
            println!("           {}", m.rationale);
        }
    }
    let lossy = mappings
        .iter()
        .filter(|m| m.lossiness.needs_approval())
        .count();
    if lossy > 0 {
        println!("  {lossy} lossy mapping(s) need an R3 approval before this design is used.");
    }
    for f in &design.findings {
        println!("  NEEDS A DECISION: {f}");
    }
}

fn project_target(store: &dyn ekos_ledger::KnowledgeStore, name: &str) -> Result<String> {
    let obj = Project::load(store, name)?
        .ok_or_else(|| anyhow::anyhow!("no migration project named '{name}'"))?;
    obj.properties
        .get("target")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("project '{name}' has no target connection"))
}

/// Which columns the source declares nullable.
fn column_nullability(
    src: &PgSource,
    table: &str,
) -> Result<std::collections::BTreeMap<String, bool>> {
    let (schema, name) = table.split_once('.').unwrap_or(("public", table));
    let rows = src.raw_query(&format!(
        "SELECT a.attname, NOT a.attnotnull FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND a.attnum > 0 AND NOT a.attisdropped",
        schema.replace('\'', "''"),
        name.replace('\'', "''")
    ))?;
    Ok(rows
        .into_iter()
        .filter(|r| r.len() >= 2)
        .map(|r| (r[0].clone(), r[1] == "t"))
        .collect())
}

/// Infer undeclared foreign keys from real code joins, then measure whether they hold.
///
/// A candidate is a hypothesis: the code says two columns reference each other, and an inclusion
/// check says whether the data agrees. Both halves are needed — a join in a view proves a developer
/// believed it, not that the values line up.
///
/// Takes the caller's already-open `store`: the fact ledger allows exactly one writable process, so
/// opening a second handle here deadlocks against the first. The error is clear when it happens
/// ("another writable process already holds the ledger's write lock") but the cause is not, because
/// the second opener is inside the same process.
fn assess_inferred_keys(
    config: &EkosConfig,
    store: &dyn ekos_ledger::KnowledgeStore,
    project: &str,
    dsn: &str,
    run_id: &str,
    targets: &[String],
    measure: bool,
) -> Result<()> {
    let mut observations = harvest_join_observations(store)?;

    // The second seed: joins that exist only in queries the running application issues, and never
    // in the repository. `pg_stat_statements` is the only place they are visible.
    let workload_joins = off_runtime(|| {
        let src = open_source(config, dsn, run_id)?;
        let shapes = ekos_pg_live::workload::harvest(&src, &config.redaction_config(), 2000)?;
        Ok(ekos_pg_live::workload::analyze(&shapes).1)
    })?;
    let from_workload = workload_joins.len();
    observations.extend(
        workload_joins
            .into_iter()
            .map(|j| ekos_migrate_dq::JoinObservation {
                left_table: j.left_table,
                left_column: j.left_column,
                right_table: j.right_table,
                right_column: j.right_column,
                source: "pg_stat_statements".into(),
            }),
    );
    if from_workload > 0 {
        // Before scoping: this counts every join in the workload, including ones against tables
        // outside the migration and against the system catalogs. Saying "joins not in the
        // repository" here would over-claim.
        println!(
            "\n{from_workload} join predicate(s) harvested from the live workload, before scoping \
             to the tables under migration."
        );
    }

    if observations.is_empty() {
        println!(
            "\nInferred keys: no join predicates found. `ekos recover` compiles views, SQL and ETL \
             into the Transformation IR, and pg_stat_statements holds what the application runs — \
             without either, there is nothing to infer from."
        );
        return Ok(());
    }

    // Only candidates whose *both* sides are tables under migration: a join against something out
    // of scope is real but not this migration's problem.
    let in_scope: std::collections::BTreeSet<String> =
        targets.iter().map(|t| bare_table(t)).collect();
    let qualify =
        |bare: &str| -> Option<String> { targets.iter().find(|t| bare_table(t) == bare).cloned() };

    // Scoped to the schemas under migration. Both queries return **bare** table names, because
    // join predicates recovered from code carry whatever alias the query used and cannot be
    // qualified — so an unscoped query lets `archive.orders` suppress a genuine finding about
    // `public.orders`. Observed: a declared FK in an unrelated fixture schema silently hid the one
    // this fixture was built to find.
    let schemas: Vec<String> = targets
        .iter()
        .filter_map(|t| t.split_once('.').map(|(s, _)| s.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let (declared, keyed) = off_runtime(|| {
        let src = open_source(config, dsn, run_id)?;
        Ok((
            declared_foreign_keys(&src, &schemas)?,
            keyed_columns(&src, &schemas)?,
        ))
    })?;

    let candidates: Vec<_> = ekos_migrate_dq::infer::candidates(&observations, &declared, &keyed)
        .into_iter()
        .filter(|c| in_scope.contains(&c.child_table) && in_scope.contains(&c.parent_table))
        .collect();

    if candidates.is_empty() {
        println!("\nInferred keys: none beyond what the schema already declares.");
        return Ok(());
    }

    println!(
        "\nInferred keys ({} candidate(s) from real code joins):",
        candidates.len()
    );
    if !measure {
        for c in &candidates {
            println!(
                "  {}.{} -> {}.{}  seen in {} place(s), not measured",
                c.child_table, c.child_column, c.parent_table, c.parent_column, c.observations
            );
        }
        return Ok(());
    }

    let measured = off_runtime(|| {
        let src = open_source(config, dsn, run_id)?;
        let mut out = Vec::new();
        for c in &candidates {
            // Resolve the bare names against the live catalog before touching the database.
            let (Some(child), Some(parent)) = (qualify(&c.child_table), qualify(&c.parent_table))
            else {
                continue;
            };
            let resolved = ekos_migrate_dq::FkCandidate {
                child_table: child,
                parent_table: parent,
                ..c.clone()
            };
            let count = |sql: &str| -> Option<i64> {
                src.raw_query(sql)
                    .ok()
                    .and_then(|r| r.first()?.first()?.trim().parse().ok())
            };
            let result = match (
                count(&resolved.orphan_sql()),
                count(&resolved.population_sql()),
            ) {
                (Some(orphans), Some(population)) => Some(ekos_migrate_dq::InclusionResult {
                    orphans,
                    population,
                }),
                // A measurement that errors stays unmeasured. The usual reason is a type mismatch
                // between the two columns — which is itself evidence the join was never a key.
                _ => None,
            };
            out.push((resolved, result));
        }
        Ok(out)
    })?;

    let mut facts = Vec::new();
    for (c, result) in &measured {
        let verdict = result
            .as_ref()
            .map(ekos_migrate_dq::InclusionResult::verdict);
        let rate = result.as_ref().and_then(|r| r.inclusion_rate());
        let detail = match (&verdict, rate) {
            (Some(v), Some(r)) => format!("{} ({:.2}% of values match)", v.as_str(), r * 100.0),
            (Some(v), None) => v.as_str().to_string(),
            (None, _) => "not measurable".to_string(),
        };
        println!(
            "  {}.{} -> {}.{}  seen in {} place(s) ({:?})  {}",
            c.child_table,
            c.child_column,
            c.parent_table,
            c.parent_column,
            c.observations,
            c.direction,
            detail
        );
        if let Some(r) = result {
            println!(
                "        {} orphan(s) of {} non-null values; joined in: {}",
                r.orphans,
                r.population,
                c.sources.join(", ")
            );
        }

        if verdict.is_some_and(ekos_migrate_dq::Verdict::is_relationship) {
            let orphans = result.as_ref().map(|r| r.orphans).unwrap_or_default();
            facts.push(ekos_migrate::FindingFact {
                rule_id: "DQ.REFINT.002".into(),
                family: "referential_integrity".into(),
                severity: "blocking".into(),
                target: None,
                lossiness: None,
                object: format!("{}.{}", c.child_table, c.child_column),
                message: format!(
                    "undeclared foreign key to {}.{}, inferred from joins in {}. \
                     {orphans} orphan row(s). The schema does not declare it, so a schema-only \
                     migration would not order the load by it — and the target enforces nothing, \
                     so whatever the application was maintaining is now maintained by nothing.",
                    c.parent_table,
                    c.parent_column,
                    c.sources.join(", ")
                ),
                affected_rows: Some(orphans),
                evidence_sql: Some(c.orphan_sql()),
                blocks: true,
            });
        }
    }

    if !facts.is_empty() {
        profile_facts::write_findings(store, project, &facts, run_id)?;
        println!(
            "\n{} inferred relationship(s) recorded as findings.",
            facts.len()
        );
    }
    Ok(())
}

/// A `schema IN (…)` fragment, or a predicate that matches nothing when the list is empty.
///
/// An empty list must not silently become "every schema": that is how the unscoped version of these
/// queries let an unrelated schema suppress a real finding.
fn schema_in(column: &str, schemas: &[String]) -> String {
    if schemas.is_empty() {
        return "false".into();
    }
    let list = schemas
        .iter()
        .map(|s| format!("'{}'", s.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{column} IN ({list})")
}

/// Foreign keys the schema already declares, so inference does not re-report them.
fn declared_foreign_keys(
    src: &PgSource,
    schemas: &[String],
) -> Result<Vec<(String, String, String, String)>> {
    let rows = src.raw_query(&format!(
        "SELECT c.relname, a.attname, pc.relname, pa.attname \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_class pc ON pc.oid = con.confrelid \
         JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[1] \
         JOIN pg_attribute pa ON pa.attrelid = con.confrelid AND pa.attnum = con.confkey[1] \
         WHERE con.contype = 'f' AND {}",
        schema_in("n.nspname", schemas)
    ))?;
    Ok(rows
        .into_iter()
        .filter(|r| r.len() >= 4)
        .map(|r| (r[0].clone(), r[1].clone(), r[2].clone(), r[3].clone()))
        .collect())
}

/// Columns backed by a primary key or unique constraint. A foreign key points at one of these, so
/// this is what decides a candidate's direction without touching the data.
fn keyed_columns(src: &PgSource, schemas: &[String]) -> Result<Vec<(String, String)>> {
    let rows = src.raw_query(&format!(
        "SELECT c.relname, a.attname FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(con.conkey) \
         WHERE con.contype IN ('p', 'u') AND {}",
        schema_in("n.nspname", schemas)
    ))?;
    Ok(rows
        .into_iter()
        .filter(|r| r.len() >= 2)
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect())
}

/// Harvest every `JOIN … ON a.x = b.y` the compiler has already recovered from the estate.
///
/// This is the part a schema-only migration tool cannot do. `ekos recover` has already compiled
/// views, SQL and ETL into the Transformation IR (RFC 0027), and every join in it is a developer's
/// claim that two columns reference each other — a claim nobody wrote into the schema.
///
/// The IR lowers each node to a `Custom("TransformNode")` object named `<source path>:<index>`,
/// with a `Join` node carrying `keys` plus the *node indices* of its operands. Those indices are
/// only meaningful inside one graph, so resolution walks upstream through `FeedsInto` edges from
/// each operand until it reaches a `Source` node, which carries the real `object_name`. An operand
/// that does not reach one is skipped rather than guessed at.
fn harvest_join_observations(
    store: &dyn ekos_ledger::KnowledgeStore,
) -> Result<Vec<ekos_migrate_dq::JoinObservation>> {
    use std::collections::HashMap;

    let objects = store.all_objects()?;
    let nodes: HashMap<ekos_kir::KirId, &ekos_kir::KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == "TransformNode"))
        .map(|o| (o.id, o))
        .collect();
    if nodes.is_empty() {
        return Ok(Vec::new());
    }

    // `<source path>:<index>` → the object, so a join's operand indices can be resolved within its
    // own graph and never across graphs.
    let by_slot: HashMap<(String, u64), &ekos_kir::KirObject> = nodes
        .values()
        .filter_map(|o| {
            let (path, idx) = o.name.rsplit_once(':')?;
            Some(((path.to_string(), idx.parse().ok()?), *o))
        })
        .collect();

    let mut out = Vec::new();
    for o in nodes.values() {
        if o.properties.get("node_type").and_then(|v| v.as_str()) != Some("Join") {
            continue;
        }
        let Some((path, _)) = o.name.rsplit_once(':') else {
            continue;
        };
        let operand = |key: &str| -> Option<String> {
            let idx = o.properties.get(key)?.as_u64()?;
            let start = by_slot.get(&(path.to_string(), idx))?;
            source_object_name(start, &by_slot, path, store)
        };
        let (Some(left_table), Some(right_table)) = (operand("left"), operand("right")) else {
            continue;
        };

        let Some(keys) = o.properties.get("keys").and_then(|v| v.as_array()) else {
            continue;
        };
        for k in keys {
            // `keys` is a list of `[left_column, right_column]` pairs.
            let Some(pair) = k.as_array() else { continue };
            let (Some(lc), Some(rc)) = (
                pair.first().and_then(|v| v.as_str()),
                pair.get(1).and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            out.push(ekos_migrate_dq::JoinObservation {
                left_table: bare_table(&left_table),
                left_column: bare_column(lc),
                right_table: bare_table(&right_table),
                right_column: bare_column(rc),
                source: path.to_string(),
            });
        }
    }
    Ok(out)
}

/// Walk upstream from a node until a `Source` is reached, and return its `object_name`.
///
/// Bounded: a malformed graph with a cycle would otherwise loop, and a deep pipeline is not more
/// informative than a shallow one about which table a join operand came from.
fn source_object_name(
    start: &ekos_kir::KirObject,
    by_slot: &std::collections::HashMap<(String, u64), &ekos_kir::KirObject>,
    path: &str,
    store: &dyn ekos_ledger::KnowledgeStore,
) -> Option<String> {
    let mut current = start;
    for _ in 0..8 {
        if current.properties.get("node_type").and_then(|v| v.as_str()) == Some("Source") {
            return current
                .properties
                .get("object_name")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
        // Follow the one `FeedsInto` edge that points *into* this node.
        let upstream = store
            .relationships_for(&current.id)
            .ok()?
            .into_iter()
            .find(|r| {
                r.to == current.id
                    && matches!(&r.kind, ekos_kir::RelationshipKind::Custom(k) if k == "FeedsInto")
            })?;
        current = by_slot
            .values()
            .find(|o| o.id == upstream.from)
            .filter(|o| o.name.starts_with(path))?;
    }
    None
}

/// `dbo.cust_mstr` → `cust_mstr`, and `c.customer_id` → `customer_id`.
///
/// Join predicates in recovered SQL are written against whatever alias the query used, and an alias
/// is not a table. Reducing both sides to a bare name is what lets a candidate be matched against
/// the live catalog at all; the cost is that two same-named tables in different schemas collapse,
/// which the caller resolves against the live catalog.
fn bare_table(raw: &str) -> String {
    raw.rsplit('.').next().unwrap_or(raw).to_lowercase()
}

fn bare_column(raw: &str) -> String {
    raw.rsplit('.').next().unwrap_or(raw).to_lowercase()
}

/// Report how much of the source is accounted for, per RFC 0158's completeness check.
fn report_completeness(
    catalog: &ekos_pg_live::CatalogSnapshot,
    findings: &[ekos_migrate_dq::Finding],
) {
    use ekos_migrate_dq::completeness::{self, SourceObject};

    // Columns are covered by their table; counting them separately would make the denominator
    // dominated by a number nobody dispositions individually.
    let objects: Vec<SourceObject> = catalog
        .objects
        .iter()
        .filter(|o| o.kind != ObjectKind::Column && o.kind != ObjectKind::Schema)
        .map(|o| SourceObject {
            qualified_name: o.qualified_name.clone(),
            kind: o.kind.as_str().to_string(),
            no_target_equivalent: o.kind.has_no_target_equivalent(),
        })
        .collect();

    // A finding is not an accounting — it is the *start* of one. What counts today is that the
    // object has been looked at by a rule; a real `Accounted::Dispositioned` needs RFC 0161.
    let mut accounted = std::collections::BTreeMap::new();
    for f in findings {
        let owner = f
            .object
            .rsplit_once('.')
            .map_or(f.object.as_str(), |(t, _)| t);
        accounted.insert(
            owner.to_string(),
            completeness::Accounted::Translated {
                evidence: f.rule_id.clone(),
            },
        );
    }

    let report = completeness::check(&objects, &accounted);
    println!("\nCoverage (RFC 0158): {}", report.summary());
    if !report.passes() {
        let no_equivalent: usize = objects
            .iter()
            .filter(|o| {
                o.no_target_equivalent
                    && report.gaps.values().any(|v| v.contains(&o.qualified_name))
            })
            .count();
        if no_equivalent > 0 {
            println!(
                "  {no_equivalent} of the unclassified have no target equivalent at all (triggers, \
                 RLS policies, procedures). Those can only ever be dispositioned, never translated."
            );
        }
        println!(
            "  Dispositions arrive with RFC 0161. Until then this is the denominator, not a gate."
        );
    }
}

/// Constraints that are declared and were never validated. Their existence is not a guarantee.
fn unvalidated_constraints(src: &PgSource, table: &str) -> Result<Vec<String>> {
    let (schema, name) = table.split_once('.').unwrap_or(("public", table));
    let rows = src.raw_query(&format!(
        "SELECT con.conname FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND NOT con.convalidated",
        schema.replace('\'', "''"),
        name.replace('\'', "''")
    ))?;
    Ok(rows.into_iter().filter_map(|mut r| r.pop()).collect())
}

fn primary_key_columns(src: &PgSource, table: &str) -> Result<Vec<String>> {
    let (schema, name) = table.split_once('.').unwrap_or(("public", table));
    let rows = src.raw_query(&format!(
        "SELECT a.attname FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(con.conkey) \
         WHERE n.nspname = '{}' AND c.relname = '{}' AND con.contype = 'p'",
        schema.replace('\'', "''"),
        name.replace('\'', "''")
    ))?;
    Ok(rows.into_iter().filter_map(|mut r| r.pop()).collect())
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> EkosConfig {
        let mut c = EkosConfig::default();
        c.migrate.enabled = true;
        c
    }

    #[test]
    fn every_verb_refuses_while_migrate_is_disabled() {
        let d = tempfile::tempdir().unwrap();
        let off = EkosConfig::default();
        assert!(status(&off, d.path(), Some("p".into())).is_err());
        assert!(
            init(
                &off,
                d.path(),
                "p".into(),
                "postgres://pg/db".into(),
                "clickhouse://ch/db".into(),
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn a_dsn_carrying_a_password_is_refused_before_anything_is_written() {
        let d = tempfile::tempdir().unwrap();
        let err = init(
            &enabled(),
            d.path(),
            "p".into(),
            "postgres://user:pw@pg/db".into(),
            "clickhouse://ch/db".into(),
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("credentials must never appear"),
            "got: {err}"
        );
        // Nothing was created: the refusal happens before the store is opened.
        assert!(!d.path().join(".ekos").exists());
    }

    #[test]
    fn a_target_engine_cannot_be_used_as_a_source() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            init(
                &enabled(),
                d.path(),
                "p".into(),
                "clickhouse://ch/db".into(),
                "clickhouse://ch/db2".into(),
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn status_without_a_project_says_what_to_run() {
        let d = tempfile::tempdir().unwrap();
        let err = status(&enabled(), d.path(), None).unwrap_err();
        assert!(err.to_string().contains("ekos migrate init"), "got: {err}");
    }

    /// RFC 0154 Phase 0 exit criterion, copied from RFC 0151's
    /// `no_mcp_code_can_reach_the_lifecycle_module` (`commands/session.rs`). Crude, and it works:
    /// it is why session memory's isolation survived a live agent test. An absent capability is
    /// verifiable; a correct permission check is only probable.
    #[test]
    fn no_mcp_code_can_reach_the_migration_lifecycle() {
        let mcp = include_str!("mcp.rs");
        for forbidden in [
            "ekos_migrate::lifecycle",
            // RFC 0161's decision path. Added when the approval crate landed: a second lifecycle
            // module is a second way in, and the guard has to name both or it only protects the one
            // somebody remembered.
            "ekos_migrate_approval::lifecycle",
            "ApprovalRequest",
            "Actor::Human",
        ] {
            assert!(
                !mcp.contains(forbidden),
                "{forbidden} must stay out of the MCP surface: approving, executing outside a \
                 sandbox and signing off are human-only (RFC 0154/0161)"
            );
        }
    }

    /// The `Actor` enum must never grow an `Agent` variant: an absent variant cannot be
    /// constructed by a future caller who has not read RFC 0154.
    #[test]
    fn the_lifecycle_actor_has_no_agent_variant() {
        let src = include_str!("../../../migrate/src/lifecycle.rs");
        let body = src
            .split("pub enum Actor {")
            .nth(1)
            .expect("Actor enum not found")
            .split('}')
            .next()
            .unwrap();
        assert_eq!(body.trim(), "Human,", "Actor gained a variant: {body:?}");
    }

    /// The regression test for "Cannot start a runtime from within a runtime".
    ///
    /// `bin/ekos.rs` is `#[tokio::main]`, and the synchronous `postgres` crate builds its own
    /// runtime internally. Calling it directly from a command panics — which no ordinary `#[test]`
    /// can catch, because a plain test has no runtime. This one deliberately has one.
    ///
    /// It connects to a closed port: the assertion is that the failure is an ordinary connection
    /// error rather than a panic, which is exactly the difference `off_runtime` makes.
    #[tokio::test]
    async fn database_work_survives_being_called_from_inside_the_runtime() {
        let result = off_runtime(|| {
            let conn = ConnectionRef::parse("postgres://local/nope").unwrap();
            // Port 1 is reserved and nothing listens there.
            ekos_pg_live::PgSource::connect(
                &conn,
                "127.0.0.1",
                1,
                "nobody",
                "run-test",
                &ekos_pg_live::SessionPolicy::default(),
            )
            .map(|_| ())
            .map_err(anyhow::Error::from)
        });
        let err = match result {
            Ok(()) => panic!("connecting to a closed port must fail"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("cannot connect"),
            "expected a connection error, got: {err}"
        );
    }

    /// A repository `Table` fact, shaped exactly as `ekos_recovery`'s SQL analyzer writes one:
    /// a bare `CREATE TABLE` name and a `columns` property of `{name, data_type}`.
    fn repo_table(name: &str, cols: &[(&str, &str)]) -> ekos_kir::KirObject {
        let mut o = ekos_kir::KirObject::new(name, ekos_kir::ObjectKind::Table);
        o.properties.insert(
            "columns".into(),
            serde_json::json!(
                cols.iter()
                    .map(|(n, d)| serde_json::json!({ "name": n, "data_type": d }))
                    .collect::<Vec<_>>()
            ),
        );
        o
    }

    /// The whole point of reading `columns`: a `Table` fact with no such property must not read as
    /// "every column was deleted".
    #[test]
    fn a_repo_table_without_a_columns_property_contributes_no_column_drift() {
        let d = tempfile::tempdir().unwrap();
        let cfg = enabled();
        let store = crate::commands::store::open_store(&cfg, d.path()).unwrap();
        let bare = ekos_kir::KirObject::new("orders", ekos_kir::ObjectKind::Table);
        store.append_object(&bare).unwrap();

        let shapes = repo_table_shapes(store.as_ref()).unwrap();
        assert_eq!(shapes.len(), 1);
        assert!(shapes[0].columns.is_empty());

        let live = vec![TableShape {
            table: TableRef::parse("public.orders"),
            columns: vec![ColumnRef {
                name: "id".into(),
                data_type: "bigint".into(),
            }],
        }];
        let drifts = drift::reconcile(&live, &shapes);
        assert!(
            drifts
                .iter()
                .all(|x| x.detail != drift::DriftDetail::ColumnRepoOnly),
            "no columns to compare must not read as deleted columns: {drifts:?}"
        );
    }

    #[test]
    fn repo_table_shapes_reads_the_sql_analyzers_columns_property() {
        let d = tempfile::tempdir().unwrap();
        let cfg = enabled();
        let store = crate::commands::store::open_store(&cfg, d.path()).unwrap();
        store
            .append_object(&repo_table(
                "orders",
                &[("id", "BIGINT"), ("total", "NUMERIC(12,2)")],
            ))
            .unwrap();
        // A non-Table object must be ignored.
        store
            .append_object(&ekos_kir::KirObject::new(
                "some.file.rs",
                ekos_kir::ObjectKind::File,
            ))
            .unwrap();

        let shapes = repo_table_shapes(store.as_ref()).unwrap();
        assert_eq!(shapes.len(), 1, "only Table objects contribute");
        assert_eq!(shapes[0].table.name, "orders");
        assert_eq!(shapes[0].columns.len(), 2);
        assert_eq!(shapes[0].columns[1].data_type, "NUMERIC(12,2)");
    }

    /// The bug this guards: unscoped, a declared foreign key in *any* schema suppressed an
    /// inferred one in the schema under migration, because join predicates recovered from code
    /// carry bare table names and cannot be qualified. Observed live — a fixture in an unrelated
    /// schema silently hid the finding the test was built to produce.
    #[test]
    fn an_empty_schema_list_matches_nothing_rather_than_everything() {
        assert_eq!(schema_in("n.nspname", &[]), "false");
        let one = schema_in("n.nspname", &["public".into()]);
        assert_eq!(one, "n.nspname IN ('public')");
        let quoted = schema_in("n.nspname", &["o'brien".into()]);
        assert!(
            quoted.contains("'o''brien'"),
            "a quote must be doubled: {quoted}"
        );
    }

    #[test]
    fn join_operands_are_reduced_to_bare_names() {
        // Recovered SQL writes whatever the query used: a schema qualifier, an alias, or neither.
        assert_eq!(bare_table("dbo.cust_mstr"), "cust_mstr");
        assert_eq!(bare_table("ekos_fk.orders"), "orders");
        assert_eq!(bare_table("Orders"), "orders");
        assert_eq!(bare_column("o.customer_id"), "customer_id");
        assert_eq!(bare_column("customer_id"), "customer_id");
    }

    /// A ledger with no Transformation IR yields no candidates, and must say why rather than
    /// reporting "no inferred keys" — which would read as a clean bill of health.
    #[test]
    fn a_ledger_without_transform_nodes_harvests_nothing() {
        let d = tempfile::tempdir().unwrap();
        let cfg = enabled();
        let store = crate::commands::store::open_store(&cfg, d.path()).unwrap();
        store
            .append_object(&ekos_kir::KirObject::new(
                "orders",
                ekos_kir::ObjectKind::Table,
            ))
            .unwrap();
        assert!(
            harvest_join_observations(store.as_ref())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn init_then_status_round_trips() {
        let d = tempfile::tempdir().unwrap();
        init(
            &enabled(),
            d.path(),
            "ledgersmb".into(),
            "postgres://pg-prod/ledgersmb".into(),
            "clickhouse://ch-dev/ledgersmb".into(),
            Some("PG_PW".into()),
            None,
        )
        .unwrap();
        status(&enabled(), d.path(), Some("ledgersmb".into())).unwrap();
        assert!(status(&enabled(), d.path(), Some("nope".into())).is_err());
    }
}
