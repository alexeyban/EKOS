//! `ekos migrate init|status` (RFC 0154 Phase 0) — the migration project model and state machine.
//!
//! Phase 0 is deliberately inert with respect to the outside world: nothing here opens a
//! connection to PostgreSQL or to a target, and nothing generates or executes SQL. It creates the
//! project and its connection *references* (aliases and secret variable names, never secrets) and
//! reports the state of its units. Discovery, profiling, mapping, execution and validation arrive
//! with RFCs 0155–0167.

use anyhow::{Result, bail};
use ekos_compiler_core::EkosConfig;
use ekos_migrate::project::Unit;
use ekos_migrate::{ALL_STATES, ConnectionRef, Project, UnitState};
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
