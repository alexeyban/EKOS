pub mod architecture;
pub mod artifact;
pub mod ask;
pub mod branch;
pub mod build;
pub mod catalogue;
pub mod clean;
pub mod clickhouse;
pub mod cluster;
pub mod commit;
pub mod compile;
pub mod config;
pub mod conflicts;
pub mod console;
pub mod coverage;
pub mod dbt;
pub mod diagnostics_log;
pub mod diff;
pub mod docs;
pub mod docs_estate;
pub mod doctor;
pub mod ekl;
pub mod eval;
pub mod export;
pub mod extension_pages;
pub mod freshness;
pub mod graph;
pub mod home_backup;
pub mod identity;
pub mod import;
pub mod init;
pub mod ledger;
pub mod llm;
pub mod marketing;
pub mod mcp;
pub mod migrate;
pub mod migrate_logic;
pub mod project;
pub mod query;
pub mod query_log;
pub mod recover;
pub mod replay;
pub mod resolve;
pub mod review;
pub mod semantics;
pub mod session;
pub mod setup;
pub mod setup_server;
#[cfg(test)]
mod setup_tests;
pub mod simulate;
pub mod source;
pub mod store;
pub mod treasury;
pub mod uninstall_server;
pub mod workflow;

use ekos_compiler_core::EkosConfig;

/// Default log filter: the workspace's level for EKOS, but tantivy quieted to `warn`.
///
/// RFC 0142 — at `info` (the default), tantivy logs a line per segment file it creates or garbage-
/// collects, so a real `commit` buries its own output under dozens of `Deleted "52274…fieldnorm"`
/// lines about index bookkeeping the user did not ask about. Warnings and errors still come
/// through, and `EKOS_LOG` overrides the whole filter — `EKOS_LOG=info,tantivy=info` restores the
/// old behaviour for anyone debugging the index itself.
fn default_filter(level: &str) -> String {
    // RFC 0220: run-log events go to the run log, not the terminal (stdout has its own summary).
    format!("{level},tantivy=warn,ekos_run_log=off,ekos_model_call=off")
}

pub fn init_logging(config: &EkosConfig) {
    init_logging_with(config, None);
}

/// [`init_logging`], plus the command's run log when it has one (RFC 0220).
pub fn init_logging_with(config: &EkosConfig, run_log: Option<&crate::run_log::RunLog>) {
    use tracing_subscriber::prelude::*;
    let level = &config.workspace.log_level;
    let format =
        std::env::var("EKOS_LOG_FORMAT").unwrap_or_else(|_| config.workspace.log_format.clone());
    let filter = tracing_subscriber::EnvFilter::try_from_env("EKOS_LOG")
        .unwrap_or_else(|_| default_filter(level).into());
    let terminal = if format == "json" {
        tracing_subscriber::fmt::layer()
            .json()
            .with_filter(filter)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer().with_filter(filter).boxed()
    };
    tracing_subscriber::registry()
        .with(terminal)
        .with(run_log.map(|log| log.layer()))
        .init();
}

/// Logging for the MCP server: stdout carries JSON-RPC frames only, so all
/// diagnostics must go to stderr.
pub fn init_logging_stderr(config: &EkosConfig) {
    let level = &config.workspace.log_level;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("EKOS_LOG")
                .unwrap_or_else(|_| default_filter(level).into()),
        )
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use super::default_filter;

    /// RFC 0142 — the directive must actually *parse*, not just look right.
    ///
    /// `EnvFilter::from(&str)` (what `init_logging` reaches via `.into()`) is lossy: an
    /// unparseable directive is dropped silently, which here would mean the tantivy noise quietly
    /// coming back with nothing to indicate why. Parsing strictly in a test is the only thing that
    /// rules that out.
    #[test]
    fn the_default_filter_is_a_valid_directive_that_quiets_tantivy() {
        for level in ["info", "debug", "warn"] {
            let f = default_filter(level);
            assert_eq!(
                f,
                format!("{level},tantivy=warn,ekos_run_log=off,ekos_model_call=off")
            );
            tracing_subscriber::EnvFilter::builder()
                .parse(&f)
                .unwrap_or_else(|e| panic!("default filter {f:?} must parse strictly: {e}"));
        }
    }
}
#[cfg(test)]
mod review_guard;
