//! Knowledge Recovery — compiler passes that lift raw observations into KIR.
//!
//! Phase 6. Requires RFC 0003 (KIR) and RFC 0008 (LLM policy).

pub mod anthropic;
pub mod cache;
pub mod confluence_analyzer;
pub mod crypto_analyzer;
pub mod dependency_analyzer;
pub mod document_semantics_analyzer;
pub mod git_analyzer;
pub mod github_analyzer;
pub mod llm;
pub mod llm_json;
pub mod local_docs_analyzer;
pub mod ollama;
pub mod pentaho_analyzer;
pub mod sql_analyzer;
pub mod sql_transform_analyzer;

pub use anthropic::AnthropicProvider;
pub use cache::CachedLlmProvider;
pub use confluence_analyzer::ConfluenceAnalyzerPass;
pub use crypto_analyzer::CryptoAnalyzerPass;
pub use dependency_analyzer::DependencyAnalyzerPass;
pub use document_semantics_analyzer::{DocumentSemanticsAnalyzerPass, DocumentSemanticsStats};
pub use git_analyzer::GitAnalyzerPass;
pub use github_analyzer::GitHubAnalyzerPass;
pub use llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, MockLlmProvider};
pub use llm_json::strip_json_fences;
pub use local_docs_analyzer::LocalDocAnalyzerPass;
pub use ollama::OllamaProvider;
pub use pentaho_analyzer::{PentahoAnalyzerPass, PentahoStats};
pub use sql_analyzer::{SqlAnalyzerPass, parse_ddl_structural};
pub use sql_transform_analyzer::{
    SqlTransformAnalyzerPass, SqlTransformStats, parse_sql_to_transform_graphs,
};
