//! The `/ask` adapter (RFC 0045 Design: "a thin adapter, not new business
//! logic"). Every step below reuses an existing, unmodified function from the
//! `ekos` CLI/library crate — this module only wires them together and
//! shapes the JSON response.

use anyhow::{Context, Result};
use ekos::commands::ask::ai_config;
use ekos::commands::recover::build_llm_provider;
use ekos::commands::store::open_store;
use ekos_runtime::{AiRuntime, Runtime};
use serde::Serialize;

use crate::catalog::RepoEntry;

#[derive(Debug, Serialize)]
pub struct Citation {
    pub confidence: f32,
    pub path: String,
    pub fragment: String,
}

#[derive(Debug, Serialize)]
pub struct AskResponse {
    pub answer: String,
    pub citations: Vec<Citation>,
    /// Non-fatal issues from `AiRuntime::ask` (e.g. a missing/truncated citation
    /// block) — surfaced so pre-vetting a demo question catches a hollow
    /// "answer with zero citations" instead of it looking like a clean success.
    pub diagnostics: Vec<String>,
}

/// Mirrors `crates/cli/src/commands/ask.rs::run` exactly — same config
/// resolution, same provider selection, same evidence-mapping pattern
/// (`ask.rs:29`) — just returning JSON instead of printing to stdout.
pub async fn answer_question(repo: &RepoEntry, question: &str) -> Result<AskResponse> {
    let config = repo.load_config();
    let ai_cfg = ai_config(&config);
    let artifact_dir = config.artifact_dir(&repo.workspace_dir);
    let llm = build_llm_provider(&config, &artifact_dir);

    let ledger = open_store(&config, &repo.workspace_dir)
        .with_context(|| format!("opening ledger for repo '{}'", repo.slug))?;
    let runtime = Runtime::over(&*ledger);
    let ai = AiRuntime::new(&runtime, llm, ai_cfg);

    let answer = ai.ask(question).await.context("AiRuntime::ask failed")?;

    let mut citations = Vec::new();
    for id in &answer.evidence_refs {
        if let Some(ev) = ledger.get_evidence(id)? {
            citations.push(Citation {
                confidence: ev.confidence,
                path: ev.location.path,
                fragment: ev.fragment,
            });
        }
    }

    Ok(AskResponse {
        answer: answer.answer,
        citations,
        diagnostics: answer
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect(),
    })
}
