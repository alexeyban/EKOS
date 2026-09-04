//! Knowledge Recovery — compiler passes that lift raw observations into KIR.
//!
//! Phase 6. Requires RFC 0003 (KIR) and RFC 0008 (LLM policy).

pub mod anthropic;
pub mod architecture_diff;
pub mod architecture_drift;
pub mod architecture_evaluator;
pub mod architecture_reasoning;
pub mod cache;
pub mod cicd_analyzer;
pub mod clickhouse_analyzer;
pub mod confluence_analyzer;
pub mod crate_topology_analyzer;
pub mod crypto_analyzer;
pub mod dbt_analyzer;
pub mod dependency_analyzer;
pub mod document_semantics_analyzer;
pub mod elixir_analyzer;
pub mod embed;
pub mod git_analyzer;
pub mod github_analyzer;
pub mod javascript_analyzer;
pub mod llm;
pub mod llm_description;
pub mod llm_json;
pub mod local_docs_analyzer;
pub mod ollama;
pub mod openai;
pub mod package_json_analyzer;
pub mod pentaho_analyzer;
pub mod python_analyzer;
pub mod requirements_analyzer;
pub mod rust_analyzer;
pub mod sql_analyzer;
pub mod sql_dialect_registry;
pub mod sql_transform_analyzer;
mod statement_repair;

pub use anthropic::AnthropicProvider;
pub use architecture_diff::{ArchitectureDiff, RoleChange, diff_architecture};
pub use architecture_drift::{DriftFinding, drift_from_history};
pub use architecture_evaluator::{
    EvaluationIssue, EvaluationIssueType, EvaluationReport, IssueSeverity,
    crates_missing_classification, evaluate_architecture,
};
pub use architecture_reasoning::{
    ArchitectureReasoningPass, ArchitectureReasoningStats, read_crate_doc_comment,
    role_claim_kir_id,
};
pub use cache::CachedLlmProvider;
pub use cicd_analyzer::CicdAnalyzerPass;
pub use clickhouse_analyzer::ClickHouseAnalyzerPass;
pub use confluence_analyzer::ConfluenceAnalyzerPass;
pub use crate_topology_analyzer::CrateTopologyAnalyzerPass;
pub use crypto_analyzer::CryptoAnalyzerPass;
pub use dbt_analyzer::DbtAnalyzerPass;
pub use dependency_analyzer::DependencyAnalyzerPass;
pub use document_semantics_analyzer::{DocumentSemanticsAnalyzerPass, DocumentSemanticsStats};
pub use elixir_analyzer::{ElixirAnalyzerPass, ElixirStats};
pub use embed::{
    CachedEmbeddingProvider, EmbedStats, EmbeddingProvider, MockEmbeddingProvider,
    OllamaEmbeddingProvider, OpenAiEmbeddingProvider, cosine, embed_objects, l2_normalize,
};
pub use git_analyzer::GitAnalyzerPass;
pub use github_analyzer::GitHubAnalyzerPass;
pub use javascript_analyzer::{JavaScriptAnalyzerPass, JavaScriptStats};
pub use llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, MockLlmProvider};
pub use llm_description::{
    DescriptionScope, DescriptionStats, describe_objects, describe_project, estimate_call_counts,
};
pub use llm_json::strip_json_fences;
pub use local_docs_analyzer::LocalDocAnalyzerPass;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use package_json_analyzer::PackageJsonAnalyzerPass;
pub use pentaho_analyzer::{PentahoAnalyzerPass, PentahoStats};
pub use python_analyzer::{PythonAnalyzerPass, PythonStats};
pub use requirements_analyzer::RequirementsAnalyzerPass;
pub use rust_analyzer::{RustAnalyzerPass, RustStats};
pub use sql_analyzer::{SqlAnalyzerPass, parse_ddl_structural};
pub use sql_dialect_registry::{
    DialectRule, GenericDialectParser, build_dialect_registry, resolve_dialect_name,
};
pub use sql_transform_analyzer::{
    SqlTransformAnalyzerPass, SqlTransformStats, parse_sql_to_transform_graphs,
};

#[cfg(test)]
mod relationship_determinism_guard {
    //! RFC 0135 Part C — every persisted `KirRelationship` an analyzer emits must get a
    //! deterministic id (`KirRelationship::deterministic`, or a `rel.id = <helper>` right after
    //! `KirRelationship::new`). A random id lets logically-identical relationships pile up as
    //! duplicate rows across `recover`/`commit` cycles (RFC 0070/0072). This test fails if a new
    //! bare `KirRelationship::new(` slips into production code in this crate.

    /// Remove `#[cfg(test)] mod … { … }` blocks (brace-matched) so only production code is scanned.
    pub(crate) fn strip_test_modules(src: &str) -> String {
        let mut out = String::new();
        let mut rest = src;
        while let Some(pos) = rest.find("#[cfg(test)]") {
            let (before, after) = rest.split_at(pos);
            out.push_str(before);
            // find the opening brace of the following `mod … {`
            let Some(brace) = after.find('{') else {
                out.push_str(after);
                return out;
            };
            let mut depth = 1usize;
            let mut idx = brace + 1;
            let bytes = after.as_bytes();
            while idx < bytes.len() && depth > 0 {
                match bytes[idx] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                idx += 1;
            }
            rest = &after[idx..];
        }
        out.push_str(rest);
        out
    }

    pub(crate) fn offenders(dir: &std::path::Path) -> Vec<String> {
        let mut bad = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = strip_test_modules(&std::fs::read_to_string(&path).unwrap());
            let mut from = 0;
            while let Some(rel) = src[from..].find("KirRelationship::new(") {
                let at = from + rel;
                let window = &src[at..(at + 600).min(src.len())];
                if !window.contains(".id =") && !window.contains(".id=") {
                    let line = src[..at].matches('\n').count() + 1;
                    bad.push(format!(
                        "{}:{}",
                        path.file_name().unwrap().to_string_lossy(),
                        line
                    ));
                }
                from = at + 1;
            }
        }
        bad
    }

    #[test]
    fn no_bare_relationship_new_in_production_code() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let bad = offenders(&dir);
        assert!(
            bad.is_empty(),
            "bare `KirRelationship::new(` in production code — use `KirRelationship::deterministic` \
             (RFC 0135 Part C), or assign `rel.id = <deterministic helper>` right after: {bad:?}"
        );
    }
}
