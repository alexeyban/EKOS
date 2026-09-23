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
pub mod coverage;
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
pub mod session;
pub mod simulate;
pub mod store;
pub mod treasury;

use ekos_compiler_core::EkosConfig;

/// Default log filter: the workspace's level for EKOS, but tantivy quieted to `warn`.
///
/// RFC 0142 — at `info` (the default), tantivy logs a line per segment file it creates or garbage-
/// collects, so a real `commit` buries its own output under dozens of `Deleted "52274…fieldnorm"`
/// lines about index bookkeeping the user did not ask about. Warnings and errors still come
/// through, and `EKOS_LOG` overrides the whole filter — `EKOS_LOG=info,tantivy=info` restores the
/// old behaviour for anyone debugging the index itself.
fn default_filter(level: &str) -> String {
    format!("{level},tantivy=warn")
}

pub fn init_logging(config: &EkosConfig) {
    let level = &config.workspace.log_level;
    let format =
        std::env::var("EKOS_LOG_FORMAT").unwrap_or_else(|_| config.workspace.log_format.clone());

    let builder = tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_env("EKOS_LOG")
            .unwrap_or_else(|_| default_filter(level).into()),
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
            assert_eq!(f, format!("{level},tantivy=warn"));
            tracing_subscriber::EnvFilter::builder()
                .parse(&f)
                .unwrap_or_else(|e| panic!("default filter {f:?} must parse strictly: {e}"));
        }
    }
}
