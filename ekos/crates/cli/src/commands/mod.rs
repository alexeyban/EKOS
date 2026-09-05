pub mod architecture;
pub mod artifact;
pub mod ask;
pub mod branch;
pub mod build;
pub mod clean;
pub mod clickhouse;
pub mod cluster;
pub mod commit;
pub mod compile;
pub mod config;
pub mod dbt;
pub mod diagnostics_log;
pub mod diff;
pub mod docs;
pub mod doctor;
pub mod ekl;
pub mod eval;
pub mod graph;
pub mod identity;
pub mod init;
pub mod ledger;
pub mod marketing;
pub mod mcp;
pub mod query;
pub mod query_log;
pub mod recover;
pub mod replay;
pub mod resolve;
pub mod simulate;
pub mod store;

use ekos_compiler_core::EkosConfig;

pub fn init_logging(config: &EkosConfig) {
    let level = &config.workspace.log_level;
    let format =
        std::env::var("EKOS_LOG_FORMAT").unwrap_or_else(|_| config.workspace.log_format.clone());

    let builder = tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_env("EKOS_LOG")
            .unwrap_or_else(|_| level.as_str().into()),
    );

    if format == "json" {
        builder.json().init();
    } else {
        builder.init();
    }
}

/// Logging for the MCP server: stdout carries JSON-RPC frames only, so all
/// diagnostics must go to stderr.
pub fn init_logging_stderr(config: &EkosConfig) {
    let level = &config.workspace.log_level;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("EKOS_LOG")
                .unwrap_or_else(|_| level.as_str().into()),
        )
        .with_writer(std::io::stderr)
        .init();
}
