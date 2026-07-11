//! Knowledge Recovery — compiler passes that lift raw observations into KIR.
//!
//! Phase 6. Requires RFC 0003 (KIR) and RFC 0008 (LLM policy).

pub mod anthropic;
pub mod cache;
pub mod git_analyzer;
pub mod llm;
pub mod sql_analyzer;

pub use anthropic::AnthropicProvider;
pub use cache::CachedLlmProvider;
pub use git_analyzer::GitAnalyzerPass;
pub use llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, MockLlmProvider};
pub use sql_analyzer::{SqlAnalyzerPass, parse_ddl_structural};
