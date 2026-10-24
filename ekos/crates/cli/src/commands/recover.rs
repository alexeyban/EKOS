use crate::extension::{Extensions, RecoverContext};
use anyhow::Result;
use ekos_artifact::{ArtifactId, ArtifactStore, PackArtifactStore};
use ekos_common::run_log;
use ekos_compiler_core::{
    EkosConfig,
    pass::{CompilerPass, PassContext},
    scheduler::FailureMode,
};
use ekos_recovery::{
    ArchitectureReasoningPass, ArchitectureReasoningStats, CicdAnalyzerPass,
    ClickHouseAnalyzerPass, ConfluenceAnalyzerPass, CrateTopologyAnalyzerPass, CryptoAnalyzerPass,
    DbtAnalyzerPass, DependencyAnalyzerPass, DialectRule, DocumentSemanticsAnalyzerPass,
    DocumentSemanticsStats, ElixirAnalyzerPass, ElixirStats, GitAnalyzerPass, GitHubAnalyzerPass,
    GovernanceAnalyzerPass, JavaAnalyzerPass, JavaScriptAnalyzerPass, JavaScriptStats, JavaStats,
    LlmRouter, LocalDocAnalyzerPass, MockLlmProvider, OracleCatalogPass, OracleCatalogStats,
    PackageJsonAnalyzerPass, PentahoAnalyzerPass, PentahoStats, PerlAnalyzerPass, PerlStats,
    PlPgSqlAnalyzerPass, PlPgSqlStats, PlSqlAnalyzerPass, PlSqlStats, PythonAnalyzerPass,
    PythonStats, RequirementsAnalyzerPass, RustAnalyzerPass, RustStats, SchemaHistoryPass,
    SchemaHistoryStats, SqlAnalyzerPass, SqlTransformAnalyzerPass, SqlTransformStats, Task,
    TreasuryAnalyzerPass, TriggerAnalyzerPass, TriggerStats, ViewAnalyzerPass, ViewStats,
    build_dialect_registry, llm::LlmProvider, resolve_dialect_name,
};
use serde_json::json;
use std::collections::HashMap;
use std::{path::Path, sync::Arc};
use walkdir::WalkDir;

pub async fn run(config: &EkosConfig, cwd: &Path, parallel: bool) -> Result<()> {
    run_with(config, cwd, parallel, &Extensions::none()).await
}

/// [`run`] with RFC 0149 extensions: their passes run alongside the built-in analyzers and their
/// report lines join the summary.
/// RFC 0220: the Java analyzer's lines — parse, application model, embedded SQL, `PO` deletes.
fn log_java(s: &JavaStats) {
    // A pass served from cache fills no counters; zeros would read as a tree without Java.
    if s.files_parsed + s.files_partial + s.files_skipped == 0 {
        run_log::emit("java.parse", "unchanged (cached)", &json!({}));
        return;
    }
    run_log::emit(
        "java.parse",
        "parsed",
        &json!({
            "files": s.files_parsed + s.files_partial + s.files_skipped,
            "ok": s.files_parsed,
            "partial": s.files_partial,
            "skipped": s.files_skipped,
            "unclassified_members": s.unclassified_members,
            "types": s.types_total,
            "generated_types": s.generated_types,
            "methods": s.methods_total,
        }),
    );
    if s.model_tables > 0 {
        run_log::emit(
            "java.model",
            "application model",
            &json!({
                "tables": s.model_tables,
                "columns": s.model_columns,
                "references": s.model_references,
            }),
        );
    }
    if s.sql_statements > 0 {
        run_log::emit(
            "java.sql",
            "embedded SQL",
            &json!({
                "statements": s.sql_statements,
                "parsed": s.sql_parsed,
                "partial": s.sql_partial,
                "unparsed": s.sql_unparsed,
                "table_links": s.sql_links,
            }),
        );
    }
    let po = s.po_deletes + s.po_deletes_unresolved + s.po_deletes_in_tests;
    if po > 0 {
        run_log::emit(
            "java.po",
            "PO deletes",
            &json!({
                "attributed": s.po_deletes,
                "unresolved": s.po_deletes_unresolved,
                "in_tests": s.po_deletes_in_tests,
            }),
        );
    }
}

fn print_sql_transform_stats(handles: &[Arc<std::sync::Mutex<SqlTransformStats>>]) {
    if handles.is_empty() {
        return;
    }
    let mut nodes_total = 0usize;
    let mut nodes_mapped = 0usize;
    for handle in handles {
        let s = handle.lock().unwrap();
        nodes_total += s.nodes_total;
        nodes_mapped += s.nodes_mapped;
    }
    let coverage = if nodes_total == 0 {
        0.0
    } else {
        100.0 * nodes_mapped as f32 / nodes_total as f32
    };
    run_log::emit(
        "sql.transform",
        "transformation IR",
        &json!({ "nodes": nodes_total, "mapped": nodes_mapped }),
    );
    println!(
        "  Transformation IR nodes (SQL): {nodes_total} total, {coverage:.0}% mapped (non-Unmapped)"
    );
}

fn print_trigger_stats(handles: &[Arc<std::sync::Mutex<TriggerStats>>]) {
    if handles.is_empty() {
        return;
    }
    let (mut triggers, mut parsed) = (0usize, 0usize);
    for handle in handles {
        let s = handle.lock().unwrap();
        triggers += s.triggers;
        parsed += s.parsed;
    }
    if triggers > 0 {
        run_log::emit(
            "sql.triggers",
            "triggers",
            &json!({ "triggers": triggers, "parsed": parsed }),
        );
        println!(
            "  Triggers: {triggers} ({parsed} parsed, {} read from tokens) — classified at compile",
            triggers - parsed
        );
    }
}

fn print_view_stats(handles: &[Arc<std::sync::Mutex<ViewStats>>]) {
    if handles.is_empty() {
        return;
    }
    let (mut views, mut parsed) = (0usize, 0usize);
    for handle in handles {
        let s = handle.lock().unwrap();
        views += s.views;
        parsed += s.parsed;
    }
    if views > 0 {
        run_log::emit(
            "sql.views",
            "views",
            &json!({ "views": views, "parsed": parsed }),
        );
        println!(
            "  Views: {views} ({parsed} parsed, {} unparsed — still recorded)",
            views - parsed
        );
    }
}

fn print_plpgsql_stats(handles: &[Arc<std::sync::Mutex<PlPgSqlStats>>]) {
    if handles.is_empty() {
        return;
    }
    let mut total = PlPgSqlStats::default();
    let mut unlexable = 0usize;
    for handle in handles {
        let s = handle.lock().unwrap();
        total.routines += s.routines;
        total.plpgsql += s.plpgsql;
        total.complete += s.complete;
        total.partial += s.partial;
        total.statements += s.statements;
        total.unrecovered += s.unrecovered;
        unlexable += usize::from(s.lex_error.is_some());
    }
    if total.routines > 0 || unlexable > 0 {
        run_log::emit(
            "plpgsql.parse",
            "routines",
            &json!({
                "routines": total.routines,
                "plpgsql": total.plpgsql,
                "complete": total.complete,
                "partial": total.partial,
                "statements": total.statements,
                "unrecovered": total.unrecovered,
                "files_not_lexed": unlexable,
            }),
        );
        println!(
            "  PL/pgSQL routines: {} ({} plpgsql: {} complete, {} partial), {} statements, {} unrecovered{}",
            total.routines,
            total.plpgsql,
            total.complete,
            total.partial,
            total.statements,
            total.unrecovered,
            if unlexable > 0 {
                format!(", {unlexable} file(s) did not lex")
            } else {
                String::new()
            }
        );
    }
}

fn print_plsql_stats(handles: &[Arc<std::sync::Mutex<PlSqlStats>>]) {
    let mut t = PlSqlStats::default();
    for handle in handles {
        let s = handle.lock().unwrap();
        t.routines.routines += s.routines.routines;
        t.routines.complete += s.routines.complete;
        t.routines.partial += s.routines.partial;
        t.routines.statements += s.routines.statements;
        t.routines.unrecovered += s.routines.unrecovered;
        t.triggers += s.triggers;
        t.anonymous_blocks += s.anonymous_blocks;
        t.unterminated += s.unterminated;
        t.skipped += s.skipped;
    }
    if t.routines.routines == 0 && t.anonymous_blocks == 0 && t.skipped == 0 {
        return;
    }
    run_log::emit(
        "plsql.parse",
        "routines",
        &json!({
            "routines": t.routines.routines,
            "complete": t.routines.complete,
            "partial": t.routines.partial,
            "triggers": t.triggers,
            "statements": t.routines.statements,
            "unrecovered": t.routines.unrecovered,
            "anonymous_blocks": t.anonymous_blocks,
            "skipped": t.skipped,
            "unterminated": t.unterminated,
        }),
    );
    println!(
        "  PL/SQL routines: {} ({} complete, {} partial, {} triggers), {} statements, {} unrecovered; \
         {} anonymous block(s) not recovered, {} unit(s) skipped, {} without a closing /",
        t.routines.routines,
        t.routines.complete,
        t.routines.partial,
        t.triggers,
        t.routines.statements,
        t.routines.unrecovered,
        t.anonymous_blocks,
        t.skipped,
        t.unterminated
    );
}

fn print_oracle_catalog_stats(handle: Option<&Arc<std::sync::Mutex<OracleCatalogStats>>>) {
    let Some(handle) = handle else {
        return;
    };
    let s = handle.lock().unwrap().clone();
    // A pass served from cache does not run, so it fills no counters; zeros would read as an
    // empty database.
    if s.catalogs == 0 {
        run_log::emit("oracle.recover", "unchanged (cached)", &json!({}));
        println!("  Oracle catalog: unchanged since the last recover (cached)");
        return;
    }
    let r = &s.routines.routines;
    run_log::emit(
        "oracle.recover",
        "catalog objects",
        &json!({
            "schemas": s.catalogs,
            "tables": s.tables,
            "foreign_keys": s.foreign_keys,
            "foreign_keys_unresolved": s.foreign_keys_unresolved,
            "views": s.views,
            "views_unparsed": s.views_unparsed,
            "routines": r.routines,
            "routines_complete": r.complete,
            "routines_partial": r.partial,
            "statements": r.statements,
            "invalid_objects": s.invalid_objects,
            "tables_profiled": s.tables_profiled,
            "tables_profile_failed": s.tables_profile_failed,
            "stale_objects": s.stale_objects,
            "dictionary_tables": s.dictionary_tables,
        }),
    );
    println!(
        "  Oracle catalog: {} schema(s), {} tables, {} foreign keys ({} unresolved), {} views \
         ({} unread), {} routines ({} complete, {} partial), {} statements, {} invalid object(s)",
        s.catalogs,
        s.tables,
        s.foreign_keys,
        s.foreign_keys_unresolved,
        s.views,
        s.views_unparsed,
        r.routines,
        r.complete,
        r.partial,
        r.statements,
        s.invalid_objects
    );
    println!(
        "  Oracle profile:  tier {}; {} table(s) scanned, {} failed or skipped; log mode {}; {} \
         object(s) no longer in the database",
        s.profile_tiers.join(", "),
        s.tables_profiled,
        s.tables_profile_failed,
        s.log_modes.join(", "),
        s.stale_objects
    );
}

fn print_history_stats(handles: &[Arc<std::sync::Mutex<SchemaHistoryStats>>]) {
    let mut t = SchemaHistoryStats::default();
    for handle in handles {
        let s = handle.lock().unwrap();
        t.change_sets += s.change_sets;
        t.changes += s.changes;
        t.data_statements += s.data_statements;
        t.unrecognized += s.unrecognized;
        for (k, v) in &s.by_operation {
            *t.by_operation.entry(k.clone()).or_default() += v;
        }
    }
    if t.change_sets == 0 {
        return;
    }
    run_log::emit(
        "schema.history",
        "migration scripts",
        &json!({
            "change_sets": t.change_sets,
            "changes": t.changes,
            "data_statements": t.data_statements,
            "unrecognized": t.unrecognized,
            "by_operation": t.by_operation,
        }),
    );
    let top: Vec<String> = t
        .by_operation
        .iter()
        .map(|(op, n)| format!("{n} {op}"))
        .collect();
    println!(
        "  Schema history: {} migration script(s), {} change(s) ({}), {} data statement(s), {} unrecognized ALTER operation(s)",
        t.change_sets,
        t.changes,
        top.join(", "),
        t.data_statements,
        t.unrecognized
    );
}

/// How the run went: passes run and skipped, pass errors, and the diagnostics summary (RFC 0076,
/// written to `.ekos/diagnostics/recover.log` when there is anything to say). Fails when a pass
/// reported an error.
fn print_run_outcome(
    config: &EkosConfig,
    cwd: &Path,
    parallel: bool,
    report: &ekos_compiler_core::scheduler::ExecutionReport,
    ctx: &PassContext,
) -> Result<()> {
    let errors: Vec<_> = report.error_outcomes().collect();
    println!("  Passes run: {}", report.passes_run());
    if report.passes_skipped() > 0 {
        println!("  Passes skipped (cached): {}", report.passes_skipped());
    }
    if parallel {
        println!("  Mode: parallel");
    }
    if !errors.is_empty() {
        println!("  Errors ({}):", errors.len());
        for o in &errors {
            if let Err(e) = &o.result {
                println!("    {}: {e}", o.pass_name);
            }
        }
    }
    // RFC 0076: `recover` used to collect real per-pass warnings (SQL001-003, and others) into
    // `ctx.diagnostics` without ever summarising or persisting them anywhere — worse than
    // `compile`'s at-least-a-count-with-a-broken-pointer, this was pure silent loss. Same fix,
    // same shared helper: a real, `.ekos/diagnostics/recover.log` file when there's anything to
    // say, nothing left behind otherwise.
    if ctx.diagnostics.lock().unwrap().has_warnings() {
        let warning_count = ctx.diagnostics.lock().unwrap().warning_count();
        match super::diagnostics_log::write_diagnostics_log(&config.ekos_dir(cwd), "recover", ctx) {
            Ok(Some(path)) => {
                println!("  Warnings: {warning_count} (see {})", path.display())
            }
            Ok(None) => println!("  Warnings: {warning_count}"),
            Err(e) => println!("  Warnings: {warning_count} (could not write log: {e})"),
        }
    }
    if ctx.diagnostics.lock().unwrap().has_errors() {
        anyhow::bail!("recovery completed with errors");
    }
    Ok(())
}

/// One `  label: n` line of the report, left out when there is nothing to count.
fn count_line(label: &str, n: usize) {
    if n > 0 {
        println!("  {label}: {n}");
    }
}

/// The counters of every per-`.sql`-file pass, read once the passes have run.
#[derive(Default)]
struct SqlStatsHandles {
    transform: Vec<Arc<std::sync::Mutex<SqlTransformStats>>>,
    plpgsql: Vec<Arc<std::sync::Mutex<PlPgSqlStats>>>,
    plsql: Vec<Arc<std::sync::Mutex<PlSqlStats>>>,
    views: Vec<Arc<std::sync::Mutex<ViewStats>>>,
    triggers: Vec<Arc<std::sync::Mutex<TriggerStats>>>,
    history: Vec<Arc<std::sync::Mutex<SchemaHistoryStats>>>,
    /// RFC 0211: the live Oracle catalog pass (one per run, when any database was observed).
    oracle: Option<Arc<std::sync::Mutex<OracleCatalogStats>>>,
}

/// Everything `ekos recover` reports once its passes have run.
struct RecoverSummary {
    sql_count: usize,
    sql_stats: SqlStatsHandles,
    dep_file_count: usize,
    git_count: usize,
    crypto_batch_count: usize,
    github_item_count: usize,
    treasury_payment_count: usize,
    governance_proposal_count: usize,
    confluence_page_count: usize,
    clickhouse_table_count: usize,
    localdocs_count: usize,
    docsem_stats: Option<Arc<std::sync::Mutex<DocumentSemanticsStats>>>,
    pentaho_count: usize,
    pentaho_stats: Option<Arc<std::sync::Mutex<PentahoStats>>>,
    python_count: usize,
    python_stats: Option<Arc<std::sync::Mutex<PythonStats>>>,
    rust_count: usize,
    rust_stats: Option<Arc<std::sync::Mutex<RustStats>>>,
    elixir_count: usize,
    elixir_stats: Option<Arc<std::sync::Mutex<ElixirStats>>>,
    perl_count: usize,
    perl_stats: Option<Arc<std::sync::Mutex<PerlStats>>>,
    java_count: usize,
    java_stats: Option<Arc<std::sync::Mutex<JavaStats>>>,
    extension_reports: Vec<Box<dyn FnOnce() -> Vec<String> + Send>>,
    javascript_count: usize,
    javascript_stats: Option<Arc<std::sync::Mutex<JavaScriptStats>>>,
    package_json_count: usize,
    requirements_count: usize,
    archreasoning_stats: Option<Arc<std::sync::Mutex<ArchitectureReasoningStats>>>,
}

impl RecoverSummary {
    /// RFC 0220: the run-log lines of the counts [`Self::print`] reports.
    fn log(&self) {
        let counts = [
            ("sql_files", self.sql_count),
            ("git_commits", self.git_count),
            ("dependency_files", self.dep_file_count),
            ("crypto_batches", self.crypto_batch_count),
            ("github_items", self.github_item_count),
            ("treasury_payments", self.treasury_payment_count),
            ("governance_proposals", self.governance_proposal_count),
            ("confluence_pages", self.confluence_page_count),
            ("clickhouse_tables", self.clickhouse_table_count),
            ("local_documents", self.localdocs_count),
            ("pentaho_jobs", self.pentaho_count),
            ("python_files", self.python_count),
            ("rust_files", self.rust_count),
            ("elixir_files", self.elixir_count),
            ("perl_files", self.perl_count),
            ("java_files", self.java_count),
            ("javascript_files", self.javascript_count),
            ("package_json", self.package_json_count),
            ("requirements_txt", self.requirements_count),
        ];
        let analysed: serde_json::Map<String, serde_json::Value> = counts
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(k, n)| ((*k).to_string(), (*n).into()))
            .collect();
        run_log::emit("recover", "inputs analysed", &analysed);
        if let Some(stats) = &self.java_stats {
            log_java(&stats.lock().unwrap());
        }
    }

    /// Prints the report and the diagnostics summary. Fails when a pass reported an error.
    fn print(
        self,
        config: &EkosConfig,
        cwd: &Path,
        parallel: bool,
        report: &ekos_compiler_core::scheduler::ExecutionReport,
        ctx: &PassContext,
    ) -> Result<()> {
        let RecoverSummary {
            sql_count,
            sql_stats,
            dep_file_count,
            git_count,
            crypto_batch_count,
            github_item_count,
            treasury_payment_count,
            governance_proposal_count,
            confluence_page_count,
            clickhouse_table_count,
            localdocs_count,
            docsem_stats,
            pentaho_count,
            pentaho_stats,
            python_count,
            python_stats,
            rust_count,
            rust_stats,
            elixir_count,
            elixir_stats,
            perl_count,
            perl_stats,
            java_count,
            java_stats,
            extension_reports,
            javascript_count,
            javascript_stats,
            package_json_count,
            requirements_count,
            archreasoning_stats,
        } = self;

        println!("Recover complete.");
        println!("  SQL files analysed: {sql_count}");
        println!("  Git commits analysed: {git_count}");
        count_line("Crypto batches analysed", crypto_batch_count);
        count_line("Source files scanned for dependencies", dep_file_count);
        count_line("GitHub issues/PRs analysed", github_item_count);
        count_line("Treasury payments analysed", treasury_payment_count);
        count_line("Governance proposals analysed", governance_proposal_count);
        count_line("Confluence pages analysed", confluence_page_count);
        count_line("ClickHouse tables analysed", clickhouse_table_count);
        count_line("Local documents analysed", localdocs_count);
        if let Some(stats) = &docsem_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Document concepts extracted: {} sections processed, {} concepts, {} relationships",
                s.sections_processed, s.concepts, s.relationships
            );
        }
        count_line("Pentaho jobs analysed", pentaho_count);
        if let Some(stats) = &pentaho_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Transformation IR nodes (Pentaho): {} total, {:.0}% mapped (non-Unmapped)",
                s.nodes_total,
                s.coverage_percent()
            );
        }
        count_line("Python files analysed", python_count);
        if let Some(stats) = &python_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Transformation IR nodes (Python): {} total, {:.0}% mapped (non-Unmapped)",
                s.nodes_total,
                s.coverage_percent()
            );
        }
        count_line("Rust files analysed", rust_count);
        if let Some(stats) = &rust_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Rust symbols recovered: {} total, {} Calls edges",
                s.symbols_total, s.calls_total
            );
        }
        count_line("Elixir files analysed", elixir_count);
        if let Some(stats) = &elixir_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Elixir modules/symbols recovered: {} modules, {} symbols",
                s.modules_total, s.symbols_total
            );
        }
        count_line("Perl files analysed", perl_count);
        if let Some(stats) = &perl_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Perl packages/symbols recovered: {} packages, {} symbols",
                s.packages_total, s.symbols_total
            );
        }
        count_line("Java files analysed", java_count);
        if let Some(stats) = &java_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Java types/methods recovered: {} types ({} generated), {} methods; files {} parsed, {} partial ({} members unclassified), {} skipped",
                s.types_total,
                s.generated_types,
                s.methods_total,
                s.files_parsed,
                s.files_partial,
                s.unclassified_members,
                s.files_skipped
            );
            if s.model_tables > 0 {
                println!(
                    "  Application model (RFC 0209): {} tables, {} columns, {} code-declared references",
                    s.model_tables, s.model_columns, s.model_references
                );
            }
            if s.sql_statements > 0 {
                println!(
                    "  Embedded SQL: {} statements ({} parsed, {} partial, {} unparsed), {} method→table links",
                    s.sql_statements, s.sql_parsed, s.sql_partial, s.sql_unparsed, s.sql_links
                );
            }
            let po = s.po_deletes + s.po_deletes_unresolved + s.po_deletes_in_tests;
            if po > 0 {
                println!(
                    "  PO deletes (RFC 0212): {} attributed to a table, {} receiver not resolved, {} in test code",
                    s.po_deletes, s.po_deletes_unresolved, s.po_deletes_in_tests
                );
            }
        }
        for report in extension_reports {
            for line in report() {
                println!("  {line}");
            }
        }
        count_line("JavaScript/TypeScript files analysed", javascript_count);
        if let Some(stats) = &javascript_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  JS/TS modules/symbols recovered: {} modules, {} symbols ({} failed to parse)",
                s.modules_total, s.symbols_total, s.files_failed_to_parse
            );
        }
        count_line("package.json manifests analysed", package_json_count);
        count_line("requirements.txt manifests analysed", requirements_count);
        if let Some(stats) = &archreasoning_stats {
            let s = *stats.lock().unwrap();
            println!(
                "  Architecture reasoning: {} crates considered, {} role(s) assigned, {} rejected",
                s.crates_considered, s.roles_assigned, s.rejected_unknown_crate
            );
        }
        print_sql_transform_stats(&sql_stats.transform);
        print_trigger_stats(&sql_stats.triggers);
        print_view_stats(&sql_stats.views);
        print_plpgsql_stats(&sql_stats.plpgsql);
        print_plsql_stats(&sql_stats.plsql);
        print_history_stats(&sql_stats.history);
        print_oracle_catalog_stats(sql_stats.oracle.as_ref());
        print_run_outcome(config, cwd, parallel, report, ctx)
    }
}

/// Registers the SQL analyzer passes for every `.sql` file under the observed paths.
#[allow(clippy::too_many_arguments)]
async fn register_sql_passes(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    config: &EkosConfig,
    cwd: &Path,
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    llm: &TaskLlm<'_>,
    pass_manager: &mut ekos_compiler_core::pass::PassManager,
    sql_count: &mut usize,
    sql_stats: &mut SqlStatsHandles,
    dialect_registry: &ekos_recovery::DialectRegistry,
    dialect_rules: &[DialectRule],
    default_dialect: &str,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_sql = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(|e| e.eq_ignore_ascii_case("sql"))
                .unwrap_or(false);
            if !is_sql {
                continue;
            }

            // Workspace-relative, not base-relative: with multiple observe
            // paths, two projects can hold the same base-relative SQL path
            // (e.g. `schema.sql`), and pass names must be unique.
            let rel = path.strip_prefix(cwd).unwrap_or(path);
            let rel_str = rel.to_string_lossy().into_owned();

            // RFC 0043: never read a file matching the built-in/configured secret-file exclusion
            // list at all (bypasses the artifact store, so `build.rs`'s central redaction pass
            // never sees this content).
            if ekos_common::redaction::is_excluded_path(&rel_str, redaction_config) {
                tracing::debug!(path = %rel_str, "skipping: matched security exclusion pattern");
                continue;
            }

            let sql = match tokio::fs::read_to_string(path).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };
            let sql = ekos_common::redaction::redact(&sql, redaction_config);

            // Resolve which dialect this file parses with (RFC 0031): first matching
            // `[[recover.sql.dialect-rules]]` wins, else `default-dialect` ("generic" — the
            // ANSI baseline — if the whole `[recover.sql]` section is omitted).
            let dialect_name = resolve_dialect_name(dialect_rules, default_dialect, &rel_str);
            let dialect_parser = match dialect_registry.get(dialect_name) {
                Some(parser) => parser.as_ref(),
                None => {
                    tracing::warn!(
                        "unknown dialect {dialect_name:?} configured for {rel_str}; falling back to generic"
                    );
                    dialect_registry
                        .get("generic")
                        .expect("generic dialect always registered")
                        .as_ref()
                }
            };

            let pass = SqlAnalyzerPass::new(
                &rel_str,
                sql.clone(),
                llm.get(Task::SqlNaming),
                dialect_parser,
            )
            .with_max_tokens(config.llm.max_tokens);
            pass_manager.register(Box::new(pass));

            // ── Transformation IR extraction (RFC 0027 Phase 2) ───────────
            // Pure structural, no LLM — same SQL text, a different concern
            // (transformation logic, not schema/entities). Preprocessed the same way
            // `SqlAnalyzerPass` preprocesses internally (e.g. MySQL `DELIMITER` stripping),
            // so both passes see identically-normalized SQL text for this file.
            let preprocessed_sql = dialect_parser.preprocess(&sql);
            let transform_pass = SqlTransformAnalyzerPass::new(
                &rel_str,
                preprocessed_sql,
                dialect_name,
                dialect_parser.sqlparser_dialect(),
            );
            sql_stats.transform.push(transform_pass.stats_handle());
            pass_manager.register(Box::new(transform_pass));

            // ── PL/pgSQL procedural IR (RFC 0163) ─────────────────────────
            // The redacted text as written, not the dialect-preprocessed one: statement spans and
            // line numbers cite the file, and `preprocess` rewrites it.
            // The owning `File` object's id key — base-relative and project-qualified, exactly as
            // `build.rs` keys `File` objects (RFC 0079; stripping only `cwd` here is the mistake
            // `ekos_common::project`'s docs record twice).
            let file_key = ekos_common::project::project_qualify(
                &path
                    .strip_prefix(base)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                Some(&ob.key),
            );

            let file = SqlFile {
                rel_str: &rel_str,
                sql: &sql,
                dialect_name,
                dialect_parser,
                file_key: &file_key,
            };
            register_file_analysis_passes(&file, pass_manager, sql_stats);

            *sql_count += 1;
        }
    }
    Ok(())
}

/// One `.sql` file, read and redacted, with its resolved dialect.
struct SqlFile<'a> {
    rel_str: &'a str,
    /// The text as written (redacted), not the dialect-preprocessed one: spans and lines cite it.
    sql: &'a str,
    dialect_name: &'a str,
    dialect_parser: &'a dyn ekos_recovery::SqlDialectParser,
    /// The owning `File` object's id key.
    file_key: &'a str,
}

/// The per-file analysis passes beyond DDL and transformations: views, triggers, routines in
/// either procedural language, and schema history.
fn register_file_analysis_passes(
    f: &SqlFile<'_>,
    pass_manager: &mut ekos_compiler_core::pass::PassManager,
    sql_stats: &mut SqlStatsHandles,
) {
    // ── Views (RFC 0169) ──────────────────────────────────────────
    // Same file-as-written text, the file's resolved dialect, one view at a time.
    if ViewAnalyzerPass::applies_to(f.sql) {
        let view_pass = ViewAnalyzerPass::new(
            f.rel_str,
            f.sql.to_string(),
            f.dialect_name,
            f.dialect_parser.sqlparser_dialect(),
        )
        .with_file(f.file_key.to_string());
        sql_stats.views.push(view_pass.stats_handle());
        pass_manager.register(Box::new(view_pass));
    }

    // ── Triggers (RFC 0163) — classified later, against their functions ──
    // An Oracle trigger carries its body inline: the PL/SQL pass below reads it whole.
    let oracle = PlSqlAnalyzerPass::applies_to(f.dialect_name);
    if TriggerAnalyzerPass::applies_to(f.sql) && !oracle {
        let trigger_pass = TriggerAnalyzerPass::new(
            f.rel_str,
            f.sql.to_string(),
            f.dialect_name,
            f.dialect_parser.sqlparser_dialect(),
        )
        .with_file(f.file_key.to_string());
        sql_stats.triggers.push(trigger_pass.stats_handle());
        pass_manager.register(Box::new(trigger_pass));
    }

    if PlPgSqlAnalyzerPass::applies_to(f.dialect_name, f.sql) {
        let plpgsql_pass = PlPgSqlAnalyzerPass::new(f.rel_str, f.sql.to_string())
            .with_file(f.file_key.to_string());
        sql_stats.plpgsql.push(plpgsql_pass.stats_handle());
        pass_manager.register(Box::new(plpgsql_pass));
    }

    // ── PL/SQL procedural IR (RFC 0210) — the same objects, Oracle's language ──
    if oracle {
        let plsql_pass =
            PlSqlAnalyzerPass::new(f.rel_str, f.sql.to_string()).with_file(f.file_key.to_string());
        sql_stats.plsql.push(plsql_pass.stats_handle());
        pass_manager.register(Box::new(plsql_pass));
    }

    // ── Schema history (RFC 0210) — a `<timestamp>_<description>.sql` migration script ──
    if SchemaHistoryPass::applies_to(f.rel_str) {
        let history_pass = SchemaHistoryPass::new(f.rel_str, f.sql.to_string(), f.dialect_name)
            .with_file(f.file_key.to_string());
        sql_stats.history.push(history_pass.stats_handle());
        pass_manager.register(Box::new(history_pass));
    }
}

/// Whether a walked entry is a directory named by one of the `[observe]` ignore patterns.
fn is_ignored_dir(entry: &walkdir::DirEntry, ignore: &[String]) -> bool {
    entry.file_type().is_dir() && {
        let name = entry.file_name().to_str().unwrap_or("");
        ignore.iter().any(|p| name == p.as_str())
    }
}

/// The extensions the dependency scan reads (RFC 0019).
const DEP_SCAN_EXTENSIONS: &[&str] = &["py", "js", "ts", "java", "go", "rb"];

/// Source files for dependency-fact extraction (RFC 0019), redacted.
#[allow(clippy::too_many_arguments)]
async fn collect_dependency_files(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    dep_files: &mut Vec<(String, String, String)>,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        let project_key = ob.key.clone();
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_candidate = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(|e| {
                    DEP_SCAN_EXTENSIONS
                        .iter()
                        .any(|ext| e.eq_ignore_ascii_case(ext))
                })
                .unwrap_or(false);
            if !is_candidate {
                continue;
            }
            let rel = path.strip_prefix(base).unwrap_or(path);
            if ekos_common::redaction::is_excluded_path(&rel.to_string_lossy(), redaction_config) {
                tracing::debug!(path = %rel.display(), "skipping: matched security exclusion pattern");
                continue;
            }

            let content = match tokio::fs::read_to_string(path).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };
            let content = ekos_common::redaction::redact(&content, redaction_config);
            dep_files.push((
                rel.to_string_lossy().replace('\\', "/"),
                content,
                project_key.clone(),
            ));
        }
    }
    Ok(())
}

/// Every `Cargo.toml` under the observed paths (RFC 0042), redacted.
#[allow(clippy::too_many_arguments)]
async fn collect_cargo_manifests(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    cargo_manifests: &mut Vec<(String, String, String)>,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        let project_key = ob.key.clone();
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.file_name() != "Cargo.toml" {
                continue;
            }
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(path);
            if ekos_common::redaction::is_excluded_path(&rel.to_string_lossy(), redaction_config) {
                tracing::debug!(path = %rel.display(), "skipping: matched security exclusion pattern");
                continue;
            }
            let content = match tokio::fs::read_to_string(path).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };
            let content = ekos_common::redaction::redact(&content, redaction_config);
            cargo_manifests.push((
                rel.to_string_lossy().replace('\\', "/"),
                content,
                project_key.clone(),
            ));
        }
    }
    Ok(())
}

/// Every `package.json` under the observed paths (RFC 0082), redacted.
#[allow(clippy::too_many_arguments)]
async fn collect_package_json(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    package_json_manifests: &mut Vec<(String, String, Option<String>)>,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        let project_key = {
            let key = ob.key.clone();
            if key.is_empty() { None } else { Some(key) }
        };
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.file_name() != "package.json" {
                continue;
            }
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(path);
            if ekos_common::redaction::is_excluded_path(&rel.to_string_lossy(), redaction_config) {
                tracing::debug!(path = %rel.display(), "skipping: matched security exclusion pattern");
                continue;
            }
            let content = match tokio::fs::read_to_string(path).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };
            let content = ekos_common::redaction::redact(&content, redaction_config);
            package_json_manifests.push((
                rel.to_string_lossy().replace('\\', "/"),
                content,
                project_key.clone(),
            ));
        }
    }
    Ok(())
}

/// Every `requirements.txt` under the observed paths, redacted.
#[allow(clippy::too_many_arguments)]
async fn collect_requirements(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    requirements_manifests: &mut Vec<(String, String, Option<String>)>,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        let project_key = {
            let key = ob.key.clone();
            if key.is_empty() { None } else { Some(key) }
        };
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.file_name() != "requirements.txt" {
                continue;
            }
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(path);
            if ekos_common::redaction::is_excluded_path(&rel.to_string_lossy(), redaction_config) {
                tracing::debug!(path = %rel.display(), "skipping: matched security exclusion pattern");
                continue;
            }
            let content = match tokio::fs::read_to_string(path).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };
            let content = ekos_common::redaction::redact(&content, redaction_config);
            requirements_manifests.push((
                rel.to_string_lossy().replace('\\', "/"),
                content,
                project_key.clone(),
            ));
        }
    }
    Ok(())
}

/// Every GitHub Actions workflow under the observed paths (RFC 0042), redacted.
#[allow(clippy::too_many_arguments)]
async fn collect_cicd_workflows(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    cicd_workflows: &mut Vec<(String, String, String)>,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        let project_key = ob.key.clone();
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_yaml = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
                .unwrap_or(false);
            let under_workflows = path
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .windows(2)
                .any(|w| w[0] == ".github" && w[1] == "workflows");
            if !is_yaml || !under_workflows {
                continue;
            }
            let rel = path.strip_prefix(base).unwrap_or(path);
            if ekos_common::redaction::is_excluded_path(&rel.to_string_lossy(), redaction_config) {
                tracing::debug!(path = %rel.display(), "skipping: matched security exclusion pattern");
                continue;
            }
            let content = match tokio::fs::read_to_string(path).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };
            let content = ekos_common::redaction::redact(&content, redaction_config);
            cicd_workflows.push((
                rel.to_string_lossy().replace('\\', "/"),
                content,
                project_key.clone(),
            ));
        }
    }
    Ok(())
}

/// The YAML and SQL files under a dbt project's `models/`, redacted, each with its path
/// relative to the project. Returned as (yml files, sql files).
async fn read_dbt_models(
    dbt_dir: &Path,
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut dbt_yml_files: Vec<(String, String)> = Vec::new();
    let mut dbt_sql_files: Vec<(String, String)> = Vec::new();
    for m_entry in WalkDir::new(dbt_dir.join("models"))
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e, ignore))
    {
        let Ok(m_entry) = m_entry else { continue };
        if !m_entry.file_type().is_file() {
            continue;
        }
        let path = m_entry.path();
        let ext = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_ascii_lowercase);
        let is_sql = ext.as_deref() == Some("sql");
        let is_yaml = matches!(ext.as_deref(), Some("yml") | Some("yaml"));
        if !is_sql && !is_yaml {
            continue;
        }
        let rel = path.strip_prefix(dbt_dir).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if ekos_common::redaction::is_excluded_path(&rel_str, redaction_config) {
            tracing::debug!(path = %rel_str, "skipping: matched security exclusion pattern");
            continue;
        }
        let content = match tokio::fs::read_to_string(path).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("cannot read {}: {e}", path.display());
                continue;
            }
        };
        let content = ekos_common::redaction::redact(&content, redaction_config);
        if is_sql {
            dbt_sql_files.push((rel_str, content));
        } else {
            dbt_yml_files.push((rel_str, content));
        }
    }
    (dbt_yml_files, dbt_sql_files)
}

/// Registers one dbt analyzer pass per dbt project found under the observed paths (RFC 0117).
#[allow(clippy::too_many_arguments)]
async fn register_dbt_passes(
    observe_paths: &[ekos_compiler_core::ObserveBase],
    ignore: &[String],
    redaction_config: &ekos_common::redaction::RedactionConfig,
    pass_manager: &mut ekos_compiler_core::pass::PassManager,
) -> Result<()> {
    for ob in observe_paths {
        let base = &ob.path;
        let project = ob.key.clone();
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e, ignore))
        {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.file_name() != "dbt_project.yml" {
                continue;
            }
            let Some(dbt_dir) = entry.path().parent().map(Path::to_path_buf) else {
                continue;
            };
            let dbt_dir_rel = dbt_dir.strip_prefix(base).unwrap_or(&dbt_dir);
            let dbt_root = ekos_common::project::project_qualify(
                &dbt_dir_rel.to_string_lossy().replace('\\', "/"),
                if project.is_empty() {
                    None
                } else {
                    Some(project.as_str())
                },
            );

            let (dbt_yml_files, dbt_sql_files) =
                read_dbt_models(&dbt_dir, ignore, redaction_config).await;

            if !dbt_sql_files.is_empty() || !dbt_yml_files.is_empty() {
                pass_manager.register(Box::new(DbtAnalyzerPass::new(
                    dbt_root,
                    dbt_yml_files,
                    dbt_sql_files,
                )));
            }
        }
    }
    Ok(())
}

type ExtensionReport = Box<dyn FnOnce() -> Vec<String> + Send>;

/// Registers every out-of-tree recover pass (RFC 0149) and returns each one's report callback.
fn register_extension_passes(
    ext: &Extensions,
    config: &EkosConfig,
    cwd: &Path,
    artifact_store: &dyn ArtifactStore,
    pass_manager: &mut ekos_compiler_core::pass::PassManager,
) -> Vec<ExtensionReport> {
    let mut extension_reports: Vec<ExtensionReport> = Vec::new();
    let ext_ctx = RecoverContext {
        config,
        cwd,
        project: cwd
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        artifact_store,
        extensions: ext,
    };
    for e in ext.iter() {
        for contribution in e.recovery_passes(&ext_ctx) {
            pass_manager.register(contribution.pass);
            extension_reports.push(contribution.report);
        }
    }
    extension_reports
}

/// Registers the local-document pass, and the document-semantics pass on top of it when that is
/// enabled; returns the latter's stats handle.
fn register_localdocs(
    config: &EkosConfig,
    cwd: &Path,
    llm: &TaskLlm<'_>,
    localdocs_artifact_ids: Vec<ArtifactId>,
    pass_manager: &mut ekos_compiler_core::pass::PassManager,
) -> Option<Arc<std::sync::Mutex<DocumentSemanticsStats>>> {
    if localdocs_artifact_ids.is_empty() {
        return None;
    }
    let localdocs_count = localdocs_artifact_ids.len();
    let mut docsem_stats = None;
    let localdocs_pass = LocalDocAnalyzerPass::new(
        cwd.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        localdocs_artifact_ids,
    )
    .with_ticket_keys(config.tickets.keys.clone());

    // ── LLM document-semantics extraction (RFC 0026) ─────────────────
    // Opt-in: one LLM call per document section, so it stays off unless
    // asked for. RFC 0197: its own task route (`[llm.tasks.doc-semantics]`).
    if should_register_document_semantics(config, localdocs_count) {
        let docsem_pass =
            DocumentSemanticsAnalyzerPass::new(localdocs_pass.name(), llm.get(Task::DocSemantics))
                .with_max_sections(config.document_semantics.max_sections)
                .with_max_tokens(config.llm.for_task(Task::DocSemantics.as_str()).max_tokens);
        docsem_stats = Some(docsem_pass.stats_handle());
        pass_manager.register(Box::new(docsem_pass));
    }

    pass_manager.register(Box::new(localdocs_pass));
    docsem_stats
}

/// Registers the crate-topology pass, and the architecture-reasoning pass on top of it when that
/// is enabled; returns the latter's stats handle.
fn register_crate_topology(
    config: &EkosConfig,
    cwd: &Path,
    llm: &TaskLlm<'_>,
    cargo_manifests: Vec<(String, String, String)>,
    pass_manager: &mut ekos_compiler_core::pass::PassManager,
) -> Option<Arc<std::sync::Mutex<ArchitectureReasoningStats>>> {
    if cargo_manifests.is_empty() {
        return None;
    }
    let mut archreasoning_stats = None;
    let crate_count = cargo_manifests
        .iter()
        .filter(|(_, content, _)| content.contains("[package]"))
        .count();
    let crate_topology_pass = CrateTopologyAnalyzerPass::new(
        cwd.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        cargo_manifests,
    );

    // ── LLM architecture reasoning (RFC 0065 Phase 2) ─────────────────
    // Opt-in: one batched LLM call per `recover` run, so it stays off unless asked for.
    // RFC 0197: its own task route (`[llm.tasks.architecture]`).
    if should_register_architecture_reasoning(config, crate_count) {
        let archreasoning_pass =
            ArchitectureReasoningPass::new(crate_topology_pass.name(), llm.get(Task::Architecture));
        archreasoning_stats = Some(archreasoning_pass.stats_handle());
        pass_manager.register(Box::new(archreasoning_pass));
    }

    pass_manager.register(Box::new(crate_topology_pass));
    archreasoning_stats
}

pub async fn run_with(
    config: &EkosConfig,
    cwd: &Path,
    parallel: bool,
    ext: &Extensions,
) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);
    // Shared with the pass context below — two pack stores over the same
    // segments would go stale on each other's appends (RFC 0015).
    let artifact_store: Arc<dyn ArtifactStore> = Arc::new(
        PackArtifactStore::open(&artifact_dir)
            .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?,
    );

    // ── LLM provider selection (RFC 0197: one router, one provider per task, built on use) ──
    let router = llm_router(config, &artifact_dir);
    let llm = TaskLlm::new(&router);

    // ── Build PassContext ─────────────────────────────────────────────────
    let mut pass_manager = ekos_compiler_core::pass::PassManager::new();

    // ── SQL files ─────────────────────────────────────────────────────────
    let observe_paths = config.observe_bases(cwd);

    let ignore = &config.observe.ignore_patterns;
    // RFC 0043: shared by every direct-file-read block below (SQL scan, dependency-scan,
    // crate-topology, CI/CD) — these bypass `build.rs`'s artifact-store redaction pass entirely,
    // so each needs its own exclusion check + redaction before content reaches an analyzer pass.
    let redaction_config = config.redaction_config();
    let mut sql_count = 0usize;
    let mut sql_stats = SqlStatsHandles::default();

    // ── SQL dialect resolution (RFC 0031) ─────────────────────────────────
    // Registry is compile-time (mirrors the Observer plugin pattern) — a new dialect means a
    // new crate implementing `SqlDialectParser`, registered here, not a runtime plugin load.
    let dialect_registry = build_dialect_registry();
    let dialect_rules: Vec<DialectRule> = config
        .recover
        .sql
        .dialect_rules
        .iter()
        .map(|r| DialectRule {
            path_glob: r.path_glob.clone(),
            dialect: r.dialect.clone(),
        })
        .collect();
    let default_dialect = &config.recover.sql.default_dialect;

    register_sql_passes(
        &observe_paths,
        config,
        cwd,
        ignore,
        &redaction_config,
        &llm,
        &mut pass_manager,
        &mut sql_count,
        &mut sql_stats,
        &dialect_registry,
        &dialect_rules,
        default_dialect,
    )
    .await?;

    // ── Source files for dependency-fact extraction (RFC 0019) ───────────
    // Pattern/regex-based only (no AST/call-graph) — one pass batching every
    // matched file so technology objects dedup within the batch before
    // append_object's content-addressed idempotency takes over across runs.
    // (relative path — to `base`, not `cwd`, matching `build.rs`'s own `File`-object convention
    // exactly — raw file content, RFC 0079 project qualifier). Found live, 2026-08-24: this used
    // to be a bare 2-tuple with no project qualifier at all, so every `DependsOn` edge
    // `dependency_analyzer.rs` emitted pointed at a `file_kir_id` that never matched the real,
    // project-qualified `File` object id `build.rs` actually wrote — silently orphaned
    // relationships (`SEM002: ... has unknown from-id`) and a `## Technology Inventory` that
    // could detect a technology but never resolve which file used it.
    let mut dep_files: Vec<(String, String, String)> = Vec::new();
    collect_dependency_files(&observe_paths, ignore, &redaction_config, &mut dep_files).await?;
    let dep_file_count = dep_files.len();
    if !dep_files.is_empty() {
        let dep_pass = DependencyAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            dep_files,
        );
        pass_manager.register(Box::new(dep_pass));
    }

    // ── Git commit artifacts ─────────────────────────────────────────────
    let (commit_ids, repo_id) = collect_git_artifact_ids(&*artifact_store);
    let git_count = commit_ids.len();
    if !commit_ids.is_empty() {
        let git_pass = GitAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            commit_ids,
            repo_id,
        )
        .with_ticket_keys(config.tickets.keys.clone());
        pass_manager.register(Box::new(git_pass));
    }

    // ── Crypto export artifacts (RFC 0017) ───────────────────────────────
    let crypto_artifact_ids = collect_crypto_artifact_ids(&*artifact_store);
    let crypto_batch_count = crypto_artifact_ids.len();
    if !crypto_artifact_ids.is_empty() {
        let crypto_pass = CryptoAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            crypto_artifact_ids,
        );
        pass_manager.register(Box::new(crypto_pass));
    }

    // ── GitHub issue/PR artifacts (RFC 0020) ─────────────────────────────
    let github_artifact_ids = collect_github_artifact_ids(&*artifact_store);
    let github_item_count = github_artifact_ids.len();
    if !github_artifact_ids.is_empty() {
        let github_pass = GitHubAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            github_artifact_ids,
        );
        pass_manager.register(Box::new(github_pass));
    }

    // ── DAO treasury payments + governance proposals (RFC 0032) ───────────
    let workspace_name = cwd
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let treasury_artifact_ids = collect_artifact_ids_for_connector(&*artifact_store, "treasury");
    let treasury_payment_count = treasury_artifact_ids.len();
    if !treasury_artifact_ids.is_empty() {
        pass_manager.register(Box::new(TreasuryAnalyzerPass::new(
            workspace_name.as_str(),
            treasury_artifact_ids,
        )));
    }
    let governance_artifact_ids =
        collect_artifact_ids_for_connector(&*artifact_store, "governance");
    let governance_proposal_count = governance_artifact_ids.len();
    if !governance_artifact_ids.is_empty() {
        pass_manager.register(Box::new(GovernanceAnalyzerPass::new(
            workspace_name.as_str(),
            governance_artifact_ids,
        )));
    }

    // ── Confluence page artifacts (RFC 0022) ─────────────────────────────
    let confluence_artifact_ids = collect_confluence_artifact_ids(&*artifact_store);
    let confluence_page_count = confluence_artifact_ids.len();
    if !confluence_artifact_ids.is_empty() {
        let confluence_pass = ConfluenceAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            confluence_artifact_ids,
        );
        pass_manager.register(Box::new(confluence_pass));
    }

    // ── ClickHouse table artifacts (RFC 0056) ────────────────────────────
    let clickhouse_artifact_ids = collect_clickhouse_artifact_ids(&*artifact_store);
    let clickhouse_table_count = clickhouse_artifact_ids.len();
    if !clickhouse_artifact_ids.is_empty() {
        let clickhouse_pass = ClickHouseAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            clickhouse_artifact_ids,
        );
        pass_manager.register(Box::new(clickhouse_pass));
    }

    // ── Live Oracle catalog artifacts (RFC 0211) ─────────────────────────
    let oracle_artifact_ids = collect_artifact_ids_for_connector(&*artifact_store, "oracle");
    if !oracle_artifact_ids.is_empty() {
        let stats = Arc::new(std::sync::Mutex::new(OracleCatalogStats::default()));
        sql_stats.oracle = Some(Arc::clone(&stats));
        pass_manager.register(Box::new(OracleCatalogPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            oracle_artifact_ids,
            stats,
        )));
    }

    // ── Local document artifacts (RFC 0023) ──────────────────────────────
    let localdocs_artifact_ids = collect_localdocs_artifact_ids(&*artifact_store);
    let localdocs_count = localdocs_artifact_ids.len();
    let docsem_stats =
        register_localdocs(config, cwd, &llm, localdocs_artifact_ids, &mut pass_manager);

    // ── Pentaho .ktr/.kjb artifacts (RFC 0027 Phase 3) ────────────────────
    let pentaho_artifact_ids = collect_pentaho_artifact_ids(&*artifact_store);
    let pentaho_count = pentaho_artifact_ids.len();
    let mut pentaho_stats: Option<Arc<std::sync::Mutex<PentahoStats>>> = None;
    if !pentaho_artifact_ids.is_empty() {
        let pentaho_pass = PentahoAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            pentaho_artifact_ids,
        );
        pentaho_stats = Some(pentaho_pass.stats_handle());
        pass_manager.register(Box::new(pentaho_pass));
    }

    // ── Python .py artifacts (RFC 0038/0040 Phase 2) ──────────────────────
    let python_artifact_ids = collect_python_artifact_ids(&*artifact_store);
    let python_count = python_artifact_ids.len();
    let mut python_stats: Option<Arc<std::sync::Mutex<PythonStats>>> = None;
    if !python_artifact_ids.is_empty() {
        let python_pass = PythonAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            python_artifact_ids,
        );
        python_stats = Some(python_pass.stats_handle());
        pass_manager.register(Box::new(python_pass));
    }

    // ── Rust .rs artifacts (RFC 0041) ──────────────────────────────────────
    let rust_artifact_ids = collect_rust_artifact_ids(&*artifact_store);
    let rust_count = rust_artifact_ids.len();
    let mut rust_stats: Option<Arc<std::sync::Mutex<RustStats>>> = None;
    if !rust_artifact_ids.is_empty() {
        let rust_pass = RustAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            rust_artifact_ids,
        );
        rust_stats = Some(rust_pass.stats_handle());
        pass_manager.register(Box::new(rust_pass));
    }

    // ── Elixir .ex/.exs artifacts (RFC 0081) ────────────────────────────────
    let elixir_artifact_ids = collect_elixir_artifact_ids(&*artifact_store);
    let elixir_count = elixir_artifact_ids.len();
    let mut elixir_stats: Option<Arc<std::sync::Mutex<ElixirStats>>> = None;
    if !elixir_artifact_ids.is_empty() {
        let elixir_pass = ElixirAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            elixir_artifact_ids,
        );
        elixir_stats = Some(elixir_pass.stats_handle());
        pass_manager.register(Box::new(elixir_pass));
    }

    // ── Perl .pl/.pm/.t/.psgi/.cgi artifacts (RFC 0147) ─────────────────────
    let perl_artifact_ids = collect_perl_artifact_ids(&*artifact_store);
    let perl_count = perl_artifact_ids.len();
    let mut perl_stats: Option<Arc<std::sync::Mutex<PerlStats>>> = None;
    if !perl_artifact_ids.is_empty() {
        let perl_pass = PerlAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            perl_artifact_ids,
        );
        perl_stats = Some(perl_pass.stats_handle());
        pass_manager.register(Box::new(perl_pass));
    }

    // ── Java .java artifacts (RFC 0209) ─────────────────────────────────────
    let java_artifact_ids = collect_artifact_ids_for_connector(&*artifact_store, "java");
    let java_count = java_artifact_ids.len();
    let mut java_stats: Option<Arc<std::sync::Mutex<JavaStats>>> = None;
    if !java_artifact_ids.is_empty() {
        let java_pass = JavaAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            java_artifact_ids,
        );
        java_stats = Some(java_pass.stats_handle());
        pass_manager.register(Box::new(java_pass));
    }

    // ── Out-of-tree passes (RFC 0149) ───────────────────────────────────────
    let extension_reports =
        register_extension_passes(ext, config, cwd, &*artifact_store, &mut pass_manager);

    // ── JavaScript/TypeScript artifacts (RFC 0085) ──────────────────────────
    let javascript_artifact_ids = collect_javascript_artifact_ids(&*artifact_store);
    let javascript_count = javascript_artifact_ids.len();
    let mut javascript_stats: Option<Arc<std::sync::Mutex<JavaScriptStats>>> = None;
    if !javascript_artifact_ids.is_empty() {
        let javascript_pass = JavaScriptAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            javascript_artifact_ids,
        );
        javascript_stats = Some(javascript_pass.stats_handle());
        pass_manager.register(Box::new(javascript_pass));
    }

    // ── Cargo.toml crate/workspace topology (RFC 0042) ────────────────────
    // (relative manifest path — to `base`, not `cwd`, matching `build.rs`'s own `File`-object
    // convention — raw content, RFC 0079 project qualifier). Found live, 2026-08-25: this used to
    // be a bare 2-tuple with no project qualifier at all — the same class of bug already fixed for
    // `dependency_analyzer.rs`/`package_json_analyzer.rs` (`devlog_101`), not yet mirrored here.
    // Internal path-dependency resolution (`normalize_rel_path`, `workspace_deps`) stays correct
    // under this change: a single real Cargo workspace is never split across two different
    // `[observe] paths` entries in practice, so every manifest within one project's own `run()`
    // call still resolves against a consistent, self-relative directory namespace.
    let mut cargo_manifests: Vec<(String, String, String)> = Vec::new();
    collect_cargo_manifests(
        &observe_paths,
        ignore,
        &redaction_config,
        &mut cargo_manifests,
    )
    .await?;

    // ── package.json dependency extraction (RFC 0082) ──────────────────────
    // RFC 0079's own qualification convention, computed fresh here — this collection loop reads
    // files directly (the same second raw-content entry point `crate_topology_analyzer.rs`'s own
    // collection above uses), so it has no `build.rs`-populated `data.project` field to read back.
    // Found live, 2026-08-24: this used the *old*, since-corrected `observe_paths.len() > 1`
    // condition (fixed in `build.rs` itself 2026-08-23, never propagated here) and a `cwd`-
    // relative path where `build.rs`'s own `File`-object ids use a `base`-relative one — two
    // independent mismatches, either one alone enough to make this pass's `DependsOn` edges
    // silently point at a `File` id that never resolves for any single non-`"."` `[observe]
    // paths` entry (this doc comment's own "matches build.rs exactly" claim was never actually
    // true for that real, common shape until both are fixed here).
    let mut package_json_manifests: Vec<(String, String, Option<String>)> = Vec::new();
    collect_package_json(
        &observe_paths,
        ignore,
        &redaction_config,
        &mut package_json_manifests,
    )
    .await?;
    let package_json_count = package_json_manifests.len();
    if !package_json_manifests.is_empty() {
        let package_json_pass = PackageJsonAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            package_json_manifests,
        );
        pass_manager.register(Box::new(package_json_pass));
    }

    // ── requirements.txt dependency extraction ──────────────────────────────
    // Same collection pattern as package.json above — real gap found running the full pipeline
    // against a real external project (`pdf-reader`): every Technology Inventory / System Context
    // view was blind to all of a real FastAPI backend's declared pip dependencies, even though the
    // Python source itself was fully analyzed by `python_analyzer.rs`.
    let mut requirements_manifests: Vec<(String, String, Option<String>)> = Vec::new();
    collect_requirements(
        &observe_paths,
        ignore,
        &redaction_config,
        &mut requirements_manifests,
    )
    .await?;
    let requirements_count = requirements_manifests.len();
    if !requirements_manifests.is_empty() {
        let requirements_pass = RequirementsAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            requirements_manifests,
        );
        pass_manager.register(Box::new(requirements_pass));
    }

    let archreasoning_stats =
        register_crate_topology(config, cwd, &llm, cargo_manifests, &mut pass_manager);

    // ── GitHub Actions CI/CD workflows (RFC 0042) ─────────────────────────
    // Same fix as `cargo_manifests` above — base-relative path + RFC 0079 project qualifier,
    // previously a bare 2-tuple with no project awareness at all.
    let mut cicd_workflows: Vec<(String, String, String)> = Vec::new();
    collect_cicd_workflows(
        &observe_paths,
        ignore,
        &redaction_config,
        &mut cicd_workflows,
    )
    .await?;
    if !cicd_workflows.is_empty() {
        let cicd_pass = CicdAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            cicd_workflows,
        );
        pass_manager.register(Box::new(cicd_pass));
    }

    // ── dbt project(s) (RFC 0117) ───────────────────────────────────────────
    // Static extraction only: no live warehouse connection, no `manifest.json`/`catalog.json`
    // (both live under `dbt/target/`, confirmed gitignored on a real inspected project) — dbt
    // itself can point at any database, so the only stable, version-controlled source of truth is
    // dbt's own checked-in project files (model `.sql` bodies + `schema.yml`/`sources.yml`-shaped
    // YAML under `models/`). Gated on finding a `dbt_project.yml` marker, the same
    // "walk `observe_paths` for a marker file directly" pattern `crate_topology_analyzer`
    // (`Cargo.toml`) and `cicd_analyzer` (`.github/workflows/*.yml`) already use — one
    // `DbtAnalyzerPass` per discovered project, so a monorepo with more than one dbt project
    // never mixes their model namespaces.
    register_dbt_passes(&observe_paths, ignore, &redaction_config, &mut pass_manager).await?;

    if pass_manager.is_empty() {
        println!(
            "Nothing to recover (no SQL files, git artifacts, crypto batches, dependency-scan source files, GitHub items, treasury payments, governance proposals, Confluence pages, ClickHouse tables, local documents, Pentaho jobs, Python files, Rust files, Cargo manifests, CI/CD workflows, or dbt projects found)."
        );
        return Ok(());
    }

    // ── Run passes ────────────────────────────────────────────────────────
    let mut ctx = PassContext::new(Arc::new(config.clone()), cwd.to_path_buf())
        .with_artifact_store(artifact_store);
    let report = if parallel {
        pass_manager
            .run_all_parallel(&ctx, FailureMode::Collect)
            .await
            .map_err(|e| anyhow::anyhow!("scheduler error: {e}"))?
    } else {
        pass_manager
            .run_all(&mut ctx, FailureMode::Collect)
            .await
            .map_err(|e| anyhow::anyhow!("scheduler error: {e}"))?
    };

    let summary = RecoverSummary {
        sql_count,
        sql_stats,
        dep_file_count,
        git_count,
        crypto_batch_count,
        github_item_count,
        treasury_payment_count,
        governance_proposal_count,
        confluence_page_count,
        clickhouse_table_count,
        localdocs_count,
        docsem_stats,
        pentaho_count,
        pentaho_stats,
        python_count,
        python_stats,
        rust_count,
        rust_stats,
        elixir_count,
        elixir_stats,
        perl_count,
        perl_stats,
        java_count,
        java_stats,
        extension_reports,
        javascript_count,
        javascript_stats,
        package_json_count,
        requirements_count,
        archreasoning_stats,
    };
    summary.log();
    summary.print(config, cwd, parallel, &report, &ctx)
}

/// Every artifact for `connector_name` currently in the store, deduplicated by `target` — the
/// real fix `collect_crypto_artifact_ids` alone used to carry (see git blame), now shared so the
/// same real bug can't recur independently in each collector again. Found live 2026-08-25/26
/// re-running `ekos build`/`recover` against EKOS's own repository, right after fixing the
/// upstream cause: `build.rs`'s artifact-id-staleness fix (recomputing an `ObservationArtifact`'s
/// content-addressed id from its *redacted* content, not the raw pre-redaction content the
/// observer originally computed it from) means a single real target can legitimately now have
/// more than one artifact in the store — an old, permanently-stale one plus a fresh, correctly
/// re-redacted one — and every collector *except* `collect_crypto_artifact_ids` blindly passed
/// every historical version through, re-processing the same real file once per version and
/// re-raising a parse warning on the stale, permanently-broken copy forever, on every single
/// future `recover` run, no matter how many times the underlying bug got fixed. Picks whichever
/// `store.list()` iteration order happens to insert last for a given target — the same accepted
/// imprecision the crypto version already shipped with; "some real, current version" beats
/// "every historical one."
pub fn collect_artifact_ids_for_connector(
    store: &dyn ArtifactStore,
    connector_name: &str,
) -> Vec<ArtifactId> {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };

    // Keyed by target, valued by (id, created_at) so a real duplicate resolves to the *newest*
    // one deterministically — plain "whichever `store.list()` happens to insert last" (this
    // function's own first version, and the still-standalone `collect_crypto_artifact_ids` this
    // replaced) has no defined relationship to recency at all: `PackArtifactStore::list()`'s
    // order comes from segment/offset position, not time, so it silently picked the *stale* one
    // for exactly the real files this fix exists for on a live run against EKOS's own repo.
    // `ArtifactMeta.created_at` is a real RFC3339 timestamp every artifact already carries;
    // lexicographic string comparison on a fixed-width RFC3339 string is a correct recency
    // ordering, no timestamp parsing needed.
    let mut by_target: HashMap<String, (ArtifactId, String)> = HashMap::new();
    for id in all_ids {
        if let Ok(Some(json)) = store.read(&id)
            && json["connector_name"].as_str() == Some(connector_name)
        {
            let target = json["target"].as_str().unwrap_or_default().to_string();
            let created_at = json["meta"]["created_at"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            match by_target.get(&target) {
                Some((_, existing_created_at)) if existing_created_at >= &created_at => {}
                _ => {
                    by_target.insert(target, (id, created_at));
                }
            }
        }
    }
    let mut ids: Vec<ArtifactId> = by_target.into_values().map(|(id, _)| id).collect();
    ids.sort_by_key(std::string::ToString::to_string);
    ids
}

/// Collect ArtifactIds for all git commit and repo artifacts currently in the store.
fn collect_git_artifact_ids(store: &dyn ArtifactStore) -> (Vec<ArtifactId>, Option<ArtifactId>) {
    let mut ids = collect_artifact_ids_for_connector(store, "git");
    // `target == "repo"` is the one artifact per real git connector that isn't keyed by commit
    // sha — pull it out into its own slot rather than the commit-id list.
    let repo_id = ids
        .iter()
        .position(|id| {
            store
                .read(id)
                .ok()
                .flatten()
                .is_some_and(|json| json["target"].as_str() == Some("repo"))
        })
        .map(|i| ids.remove(i));
    (ids, repo_id)
}

/// Collect ArtifactIds for every crypto export-batch artifact currently in the store.
fn collect_crypto_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "crypto")
}

/// Collect ArtifactIds for every GitHub issue/PR artifact currently in the store (RFC 0020).
fn collect_github_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "github")
}

/// Collect ArtifactIds for every ClickHouse table artifact currently in the store (RFC 0056).
fn collect_clickhouse_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "clickhouse")
}

/// Collect ArtifactIds for every Confluence page artifact currently in the store (RFC 0022).
fn collect_confluence_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "confluence")
}

/// Collect ArtifactIds for every local-document artifact currently in the store (RFC 0023).
fn collect_localdocs_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "localdocs")
}

/// Collect ArtifactIds for every Pentaho `.ktr`/`.kjb` artifact currently in
/// the store (RFC 0027 Phase 3).
fn collect_pentaho_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "pentaho")
}

/// Collect ArtifactIds for every Python `.py` artifact currently in the store (RFC 0038/0040
/// Phase 2).
fn collect_python_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "python")
}

/// Collect ArtifactIds for every Rust `.rs` artifact currently in the store (RFC 0041).
fn collect_rust_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "rust")
}

fn collect_perl_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "perl")
}

fn collect_elixir_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "elixir")
}

/// Collect ArtifactIds for every JavaScript/TypeScript artifact currently in the store (RFC 0085).
fn collect_javascript_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    collect_artifact_ids_for_connector(store, "javascript")
}

/// RFC 0026's opt-in gate: the document-semantics pass costs one LLM call per
/// document section, so it registers only when explicitly enabled *and* there is
/// local-document output for it to read.
fn should_register_document_semantics(config: &EkosConfig, localdocs_count: usize) -> bool {
    config.document_semantics.enabled && localdocs_count > 0
}

/// RFC 0065 Phase 2's opt-in gate: the architecture-reasoning pass costs one (batched) LLM call
/// per `recover` run, so — same shape as `should_register_document_semantics` — it registers only
/// when explicitly enabled *and* there is crate-topology output for it to read.
fn should_register_architecture_reasoning(config: &EkosConfig, crate_count: usize) -> bool {
    config.architecture_reasoning.enabled && crate_count > 0
}

/// The API-key environment variable name `build_llm_provider` falls back to when `[llm]
/// api-key-env` isn't set explicitly, per provider.
///
/// Found live while adding RFC 0138 Phase 4's hard-fail-on-mock check: this used to be one
/// literal `"ANTHROPIC_API_KEY"` computed before branching on provider, reused unchanged for the
/// `openai` branch too. A workspace configured with `[llm] provider = "openai"` and a real
/// `OPENAI_API_KEY` set, but no explicit `api-key-env` override, silently degraded to the stub
/// `MockLlmProvider` — `OpenAiProvider::from_env_var("ANTHROPIC_API_KEY")` checked the wrong
/// variable — and warned about the wrong key ever being missing.
pub(crate) fn default_key_env(provider: Option<&str>) -> &'static str {
    ekos_recovery::route::default_key_env(provider)
}

/// The API-key environment variable name `build_llm_provider` will actually check for this
/// config: `[llm] api-key-env` if set, else [`default_key_env`] for the configured provider.
///
/// `pub` so a caller that needs to explain *why* provider selection fell back to the stub
/// `MockLlmProvider` — `ekos eval`'s hard-fail (RFC 0138 Phase 4) — can name the real variable
/// instead of duplicating this resolution logic or guessing at "ANTHROPIC_API_KEY" regardless of
/// provider.
pub fn resolved_key_env(config: &EkosConfig) -> &str {
    config
        .llm
        .api_key_env
        .as_deref()
        .unwrap_or_else(|| default_key_env(config.llm.provider.as_deref()))
}

/// Choose LLM provider (RFC 0021, RFC 0046): `[llm] provider = "ollama"` in
/// `ekos.toml` routes to a local Ollama daemon (no key required —
/// unreachability surfaces as an ordinary error on first use, not here);
/// `provider = "openai"` routes to OpenAI (key + cache, mock if unset,
/// mirroring the Anthropic path exactly); anything else tries Anthropic with
/// cache if an API key is present, mock otherwise.
///
/// `pub` (not `pub(crate)`) because RFC 0045's demo server reuses this exact
/// selection logic as a library dependency rather than duplicating it — see
/// `ekos/crates/demo-server`.
/// RFC 0197: each task's provider, built the first time a pass asks for it — so a disabled pass
/// never builds one, and a missing key warns once per task, not per file.
pub(crate) struct TaskLlm<'a> {
    router: &'a LlmRouter,
    built: std::sync::Mutex<HashMap<Task, Arc<dyn LlmProvider>>>,
}

impl<'a> TaskLlm<'a> {
    pub(crate) fn new(router: &'a LlmRouter) -> Self {
        Self {
            router,
            built: Default::default(),
        }
    }

    pub(crate) fn get(&self, task: Task) -> Arc<dyn LlmProvider> {
        self.built
            .lock()
            .expect("task llm lock")
            .entry(task)
            .or_insert_with(|| provider_or_mock(self.router, task))
            .clone()
    }
}

pub fn build_llm_provider(config: &EkosConfig, artifact_dir: &Path) -> Arc<dyn LlmProvider> {
    build_llm_provider_for(config, Task::Default, artifact_dir)
}

/// RFC 0197: [`build_llm_provider`] for one task's route (`[llm.tasks.<task>]` over `[llm]`).
pub fn build_llm_provider_for(
    config: &EkosConfig,
    task: Task,
    artifact_dir: &Path,
) -> Arc<dyn LlmProvider> {
    provider_or_mock(&llm_router(config, artifact_dir), task)
}

/// RFC 0197: the router for one command — every task it serves shares `[llm] budget-usd`.
/// State (`llm-cache/`, `llm-usage.jsonl`) lives beside the artifact directory.
pub fn llm_router(config: &EkosConfig, artifact_dir: &Path) -> LlmRouter {
    LlmRouter::new(&config.llm, artifact_dir.parent().unwrap_or(artifact_dir))
}

/// `router`'s provider for `task`, or the structural-only mock when it cannot be built.
pub fn provider_or_mock(router: &LlmRouter, task: Task) -> Arc<dyn LlmProvider> {
    match router.provider(task) {
        Ok(provider) => {
            tracing::info!(
                "LLM route {task}: {} via {}",
                router.route(task).models.join(" → "),
                router.route(task).provider
            );
            provider
        }
        Err(err) => {
            let err = anyhow::anyhow!("{} not set", key_name(&err, router, task));
            tracing::warn!("{err} — using structural analysis only (LLM enrichment skipped)");
            Arc::new(MockLlmProvider::new(
                r#"{"entities":[],"relationships":[]}"#,
            ))
        }
    }
}

fn key_name(err: &ekos_recovery::LlmError, router: &LlmRouter, task: Task) -> String {
    match err {
        ekos_recovery::LlmError::NoApiKey(k) => k.clone(),
        _ => router
            .route(task)
            .key_env
            .unwrap_or_else(|| err.to_string()),
    }
}

/// The same provider selection as [`build_llm_provider`], without its mock fallback: a missing
/// key is an error naming the variable [`resolved_key_env`] resolved.
///
/// For the opt-in paths that have no honest degraded mode — `docs generate --prose` and
/// `ekos marketing publish`. Each of those used to carry its own copy of this selection, and both
/// copies only knew `ollama` and Anthropic: with `[llm] provider = "openai"` they built an
/// `AnthropicProvider` from the OpenAI-compatible key and failed on the first request. One
/// selection, called from every site, is what keeps a new provider from reaching only some of
/// them (RFC 0197: `commit.rs`'s own copy went the same way).
pub fn build_llm_provider_strict(
    config: &EkosConfig,
    artifact_dir: &Path,
) -> Result<Arc<dyn LlmProvider>> {
    build_llm_provider_strict_for(config, Task::Default, artifact_dir)
}

/// RFC 0197: [`build_llm_provider_strict`] for one task's route.
pub fn build_llm_provider_strict_for(
    config: &EkosConfig,
    task: Task,
    artifact_dir: &Path,
) -> Result<Arc<dyn LlmProvider>> {
    let router = llm_router(config, artifact_dir);
    router.provider(task).map_err(|err| match err {
        ekos_recovery::LlmError::NoApiKey(k) => anyhow::anyhow!("{k} not set"),
        other => anyhow::anyhow!("{other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_artifact::ObservationArtifact;
    use ekos_compiler_core::config::{DocumentSemanticsConfig, LlmConfig};
    use tempfile::tempdir;

    /// Real bug, found live 2026-08-25/26: every `collect_*_artifact_ids` function except
    /// `collect_crypto_artifact_ids` passed *every* historical artifact for a real target
    /// through, not just the current one — harmless when a target only ever has one real
    /// artifact, but a real, silent problem once `build.rs`'s artifact-id-staleness fix
    /// (recomputing an id from redacted, not raw, content) means the same real target can
    /// legitimately gain a second artifact over time. The stale one (from before some redaction
    /// fix) never goes away and gets reprocessed — and re-warned-about, if it's now unparseable —
    /// forever.
    #[test]
    fn collect_artifact_ids_for_connector_dedupes_by_target() {
        let dir = tempdir().unwrap();
        let store = PackArtifactStore::open(dir.path()).unwrap();

        // Two real artifacts for the *same* target — different `data`, so different real
        // content-addressed ids, exactly the shape a stale-then-fixed redaction re-write leaves
        // behind. `created_at` set explicitly (not relying on wall-clock ordering between two
        // back-to-back `Utc::now()` calls) so the test pins down *which* one must win, not just
        // that dedup happens at all — `PackArtifactStore::list()`'s own order has no relationship
        // to recency, which is exactly the bug this test guards against.
        let mut old =
            ObservationArtifact::new("rust", "secret.rs", serde_json::json!({"source": "old"}));
        old.meta.created_at = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let mut fresh =
            ObservationArtifact::new("rust", "secret.rs", serde_json::json!({"source": "fresh"}));
        fresh.meta.created_at = chrono::DateTime::from_timestamp(2_000, 0).unwrap();
        assert_ne!(
            old.id, fresh.id,
            "different data must produce different real ids"
        );
        store
            .write(&old.id, &serde_json::to_value(&old).unwrap())
            .unwrap();
        store
            .write(&fresh.id, &serde_json::to_value(&fresh).unwrap())
            .unwrap();

        // A real, unrelated target for the same connector must still come through untouched.
        let other =
            ObservationArtifact::new("rust", "other.rs", serde_json::json!({"source": "x"}));
        store
            .write(&other.id, &serde_json::to_value(&other).unwrap())
            .unwrap();

        let ids = collect_artifact_ids_for_connector(&store, "rust");
        assert_eq!(
            ids.len(),
            2,
            "one id per real target (secret.rs, other.rs), not one per historical artifact"
        );
        assert!(ids.contains(&other.id));
        assert!(
            ids.contains(&fresh.id),
            "the newer of the two secret.rs versions must be selected, not the stale one"
        );
        assert!(
            !ids.contains(&old.id),
            "the stale version must not survive dedup"
        );
    }

    #[test]
    fn collect_git_artifact_ids_splits_repo_from_commits_and_dedupes_each() {
        let dir = tempdir().unwrap();
        let store = PackArtifactStore::open(dir.path()).unwrap();

        let repo = ObservationArtifact::new("git", "repo", serde_json::json!({"remote": "x"}));
        let mut commit_a_stale =
            ObservationArtifact::new("git", "sha-a", serde_json::json!({"message": "old"}));
        commit_a_stale.meta.created_at = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        // Same target (commit sha), different content, later timestamp — the same real
        // duplicate shape a stale-then-fixed redaction re-write leaves behind.
        let mut commit_a_fresh =
            ObservationArtifact::new("git", "sha-a", serde_json::json!({"message": "one"}));
        commit_a_fresh.meta.created_at = chrono::DateTime::from_timestamp(2_000, 0).unwrap();
        let commit_b =
            ObservationArtifact::new("git", "sha-b", serde_json::json!({"message": "two"}));
        for a in [&repo, &commit_a_fresh, &commit_a_stale, &commit_b] {
            store
                .write(&a.id, &serde_json::to_value(a).unwrap())
                .unwrap();
        }

        let (commit_ids, repo_id) = collect_git_artifact_ids(&store);
        assert_eq!(repo_id, Some(repo.id));
        assert_eq!(
            commit_ids.len(),
            2,
            "one id per real commit sha, not per historical artifact"
        );
        assert!(commit_ids.contains(&commit_b.id));
        assert!(
            commit_ids.contains(&commit_a_fresh.id),
            "the newer of the two sha-a versions must be selected"
        );
        assert!(!commit_ids.contains(&commit_a_stale.id));
    }

    /// RFC 0026 acceptance criterion: zero LLM calls unless the user opted in.
    /// A config that never mentions `[document-semantics]` must leave the pass
    /// unregistered even when there are local documents to analyse.
    #[test]
    fn document_semantics_pass_not_registered_when_config_absent() {
        let config = EkosConfig::default();
        assert!(!config.document_semantics.enabled);
        assert!(!should_register_document_semantics(&config, 12));
    }

    #[test]
    fn document_semantics_pass_not_registered_when_explicitly_disabled() {
        let config = EkosConfig {
            document_semantics: DocumentSemanticsConfig {
                enabled: false,
                max_sections: Some(10),
            },
            ..Default::default()
        };
        assert!(!should_register_document_semantics(&config, 12));
    }

    /// Enabled but nothing to read is still no registration — the pass would
    /// otherwise declare a dependency on a `LocalDocAnalyzerPass` that was never
    /// registered, which the scheduler rejects as an unknown dependency.
    #[test]
    fn document_semantics_pass_not_registered_without_local_documents() {
        let mut config = EkosConfig::default();
        config.document_semantics.enabled = true;
        assert!(!should_register_document_semantics(&config, 0));
    }

    #[test]
    fn document_semantics_pass_registered_when_enabled_with_local_documents() {
        let mut config = EkosConfig::default();
        config.document_semantics.enabled = true;
        assert!(should_register_document_semantics(&config, 1));
    }

    /// RFC 0065 Phase 2, same acceptance shape as document-semantics above: zero LLM calls
    /// unless the user opted in, and no registration with nothing to read.
    #[test]
    fn architecture_reasoning_pass_not_registered_when_config_absent() {
        let config = EkosConfig::default();
        assert!(!config.architecture_reasoning.enabled);
        assert!(!should_register_architecture_reasoning(&config, 44));
    }

    #[test]
    fn architecture_reasoning_pass_not_registered_without_crates() {
        let mut config = EkosConfig::default();
        config.architecture_reasoning.enabled = true;
        assert!(!should_register_architecture_reasoning(&config, 0));
    }

    #[test]
    fn architecture_reasoning_pass_registered_when_enabled_with_crates() {
        let mut config = EkosConfig::default();
        config.architecture_reasoning.enabled = true;
        assert!(should_register_architecture_reasoning(&config, 44));
    }

    /// RFC 0021: `provider = "ollama"` must route here without ever
    /// attempting a network call — `model_name()` alone proves which
    /// provider was actually selected.
    #[test]
    fn ollama_provider_selected_when_configured() {
        let dir = tempdir().unwrap();
        let config = EkosConfig {
            llm: LlmConfig {
                provider: Some("ollama".to_string()),
                api_key_env: None,
                model: None,
                ..Default::default()
            },
            ..Default::default()
        };
        let provider = build_llm_provider(&config, dir.path());
        assert_eq!(provider.model_name(), "llama3.1:8b");
    }

    /// Anything other than "ollama" falls through to the existing
    /// Anthropic/Mock chain untouched.
    #[test]
    fn non_ollama_provider_falls_back_to_existing_chain() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let provider = build_llm_provider(&config, dir.path());
        // Without ANTHROPIC_API_KEY set in the test environment this lands
        // on the mock; either way it must not be the Ollama default model.
        assert_ne!(provider.model_name(), "llama3.1:8b");
    }

    /// `docs generate --prose` and `marketing publish` used to carry private copies of provider
    /// selection that only knew `ollama` and Anthropic, so an `openai` workspace built an
    /// `AnthropicProvider` from the wrong key. The strict builder is now their only path; it must
    /// route `openai` to its own key and fail naming it, never fall back to a mock.
    #[test]
    fn strict_builder_routes_openai_and_errors_instead_of_mocking() {
        let dir = tempdir().unwrap();
        let var = "EKOS_TEST_STRICT_BUILDER_KEY_THAT_IS_NEVER_SET";
        for provider in ["openai", "anthropic"] {
            let config = EkosConfig {
                llm: LlmConfig {
                    provider: Some(provider.to_string()),
                    api_key_env: Some(var.to_string()),
                    ..Default::default()
                },
                ..Default::default()
            };
            let err = match build_llm_provider_strict(&config, dir.path()) {
                Ok(p) => panic!("{provider}: expected an error, got {}", p.model_name()),
                Err(err) => err.to_string(),
            };
            assert!(err.contains(var), "{provider}: {err}");
        }
    }

    /// Real bug, found live 2026-09-14 while adding RFC 0138 Phase 4's hard-fail-on-mock check:
    /// `key_env` used to be one literal `"ANTHROPIC_API_KEY"` computed before branching on
    /// provider, reused unchanged for the `openai` branch too — so an `openai`-configured
    /// workspace with a real `OPENAI_API_KEY` set but no explicit `api-key-env` override checked
    /// the wrong variable and silently degraded to the mock.
    #[test]
    fn openai_provider_defaults_to_its_own_key_env_not_anthropics() {
        let config = EkosConfig {
            llm: LlmConfig {
                provider: Some("openai".to_string()),
                api_key_env: None,
                model: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(resolved_key_env(&config), "OPENAI_API_KEY");
    }

    #[test]
    fn an_explicit_api_key_env_override_always_wins() {
        let config = EkosConfig {
            llm: LlmConfig {
                provider: Some("openai".to_string()),
                api_key_env: Some("MY_CUSTOM_KEY".to_string()),
                model: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(resolved_key_env(&config), "MY_CUSTOM_KEY");
    }

    #[test]
    fn non_openai_providers_still_default_to_anthropics_key_env() {
        let config = EkosConfig::default();
        assert_eq!(resolved_key_env(&config), "ANTHROPIC_API_KEY");
    }
}
