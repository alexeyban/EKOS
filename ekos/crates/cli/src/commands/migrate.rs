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
        assert!(
            !mcp.contains("ekos_migrate::lifecycle"),
            "approval, execution outside the sandbox and sign-off must stay human-only (CLI)"
        );
        assert!(!mcp.contains("Actor::Human"));
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
