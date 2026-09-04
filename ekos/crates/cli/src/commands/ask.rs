use super::recover::build_llm_provider;
use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_runtime::ai::ConversationTurn;
use ekos_runtime::reason::{render_evidence, render_plan};
use ekos_runtime::{AiRuntime, AiRuntimeConfig, Runtime};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Flags for [`run`]. `ekos ask` routes through the REASON planner (RFC 0123/0124) by default;
/// `classic` selects the pre-0123 `gather_context` path, and `stream` implies it.
pub struct AskOpts<'a> {
    pub json: bool,
    pub stream: bool,
    pub session: Option<&'a str>,
    pub classic: bool,
    pub explain: bool,
}

pub async fn run(config: &EkosConfig, cwd: &Path, question: &str, opts: AskOpts<'_>) -> Result<()> {
    let AskOpts {
        json,
        stream,
        session,
        mut classic,
        explain,
    } = opts;

    if json && stream {
        anyhow::bail!(
            "--stream is not compatible with --json — --json needs the complete structured result, not a partial one"
        );
    }
    if explain && classic {
        anyhow::bail!(
            "--explain is not compatible with --classic — the classic path has no compiled plan to show"
        );
    }
    if stream && !classic {
        if explain {
            anyhow::bail!("--stream is not compatible with --explain");
        }
        eprintln!(
            "note: --stream uses the classic retrieval path (REASON streaming is not supported)"
        );
        classic = true;
    }

    let session_path = session
        .map(|name| session_path(config, cwd, name))
        .transpose()?;
    let history = match &session_path {
        Some(path) => load_session(path)?,
        None => Vec::new(),
    };

    let ai_config = ai_config(config);
    let artifact_dir = config.artifact_dir(cwd);
    let llm = build_llm_provider(config, &artifact_dir);

    let ledger = open_store(config, cwd)?;
    let runtime = Runtime::over(&*ledger);
    let ai = AiRuntime::new(&runtime, llm, ai_config);

    // `--explain` (REASON only): assemble the plan + evidence once, for the text block and --json.
    let explain_data = if explain {
        Some((ai.plan(question)?, ai.gather_evidence(question)?))
    } else {
        None
    };

    let answer = if classic && stream {
        // Prints prose chunks live as they arrive. Known, accepted v1
        // limitation (RFC 0098): the trailing `{"cited_evidence": [...]}`
        // block the LLM emits as part of the same stream is printed raw
        // too — extract_citations can only strip it from the *full* text
        // (it finds the *last* `{` via rfind, which can't be resolved
        // mid-stream), so there's no way to know live whether a `{` is the
        // real citation block's start or just part of the prose. The
        // non-streaming path stays fully clean; this is the real, named
        // trade-off of live streaming a response with a structured trailer.
        let mut on_chunk = |chunk: String| {
            print!("{chunk}");
            let _ = std::io::stdout().flush();
        };
        let answer = ai
            .ask_stream_with_history(question, &history, &mut on_chunk)
            .await?;
        println!();
        answer
    } else if classic {
        ai.ask_with_history(question, &history).await?
    } else {
        ai.reason_with_history(question, &history).await?
    };

    if let Some((plan, evidence)) = &explain_data
        && !json
    {
        println!(
            "── plan ──\n{}\n── evidence ──\n{}",
            render_plan(plan),
            render_evidence(evidence)
        );
    }

    if let Some(path) = &session_path {
        let mut history = history;
        history.push(ConversationTurn {
            question: question.to_string(),
            answer: answer.answer.clone(),
        });
        save_session(path, &history)?;
    }

    if json {
        let mut out = serde_json::to_value(&answer)?;
        if let Some((plan, evidence)) = &explain_data {
            out["plan"] = serde_json::to_value(plan)?;
            out["evidence"] = serde_json::to_value(evidence)?;
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if !stream {
        println!("{}", answer.answer);
    }

    if !answer.evidence_refs.is_empty() {
        println!("\nSources:");
        for id in &answer.evidence_refs {
            if let Some(ev) = ledger.get_evidence(id)? {
                println!(
                    "  [{:.0}%] {} — \"{}\"",
                    ev.confidence * 100.0,
                    ev.location.path,
                    ev.fragment
                );
            }
        }
    }

    for diag in &answer.diagnostics {
        eprintln!("warning: {}", diag.message);
    }

    Ok(())
}

/// `.ekos/ask-sessions/<name>.json` (RFC 0099) — mirrors the existing
/// `.ekos/llm-cache/` convention (a plain, inspectable JSON file, not a new
/// ledger table). `name` becomes a path component, so it's validated to
/// `[A-Za-z0-9_-]+` before use — rejects anything that could escape the
/// `ask-sessions` directory (`..`, `/`, an absolute path, …) with a clear
/// error rather than silently sanitizing or, worse, writing outside it.
fn session_path(config: &EkosConfig, cwd: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "invalid --session name '{name}': must be non-empty and contain only letters, digits, '_', or '-'"
        );
    }
    Ok(config
        .ekos_dir(cwd)
        .join("ask-sessions")
        .join(format!("{name}.json")))
}

fn load_session(path: &Path) -> Result<Vec<ConversationTurn>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_session(path: &Path, history: &[ConversationTurn]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(history)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Shared with `docs.rs`'s `--prose` tier (RFC 0035 Phase 5) and RFC 0045's demo server —
/// same `[ai]` config resolution, so prose generation and the hosted `/ask` endpoint both
/// honor the same model/provider settings `ekos ask` already does.
pub fn ai_config(config: &EkosConfig) -> AiRuntimeConfig {
    let default = AiRuntimeConfig::default();
    AiRuntimeConfig {
        model: config.ai.model.clone().unwrap_or(default.model),
        max_matches: config.ai.max_matches.unwrap_or(default.max_matches),
        neighborhood_depth: config
            .ai
            .neighborhood_depth
            .unwrap_or(default.neighborhood_depth),
        max_tokens: config.ai.max_tokens.unwrap_or(default.max_tokens),
        system_prompt: config
            .ai
            .system_prompt
            .clone()
            .unwrap_or(default.system_prompt),
        max_context_chars: config
            .ai
            .max_context_chars
            .unwrap_or(default.max_context_chars),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::config::LlmConfig;
    use tempfile::tempdir;

    /// `ekos ask` must honor `config.llm.provider` the same way `ekos recover`
    /// does — it calls the exact same `build_llm_provider` rather than
    /// constructing `AnthropicProvider` itself, so this proves the shared
    /// selection logic is reachable from `ask.rs`, not duplicated/diverged.
    #[test]
    fn ask_selects_ollama_provider_when_configured() {
        let dir = tempdir().unwrap();
        let config = EkosConfig {
            llm: LlmConfig {
                provider: Some("ollama".to_string()),
                api_key_env: None,
                model: None,
            },
            ..Default::default()
        };
        let artifact_dir = config.artifact_dir(dir.path());
        let provider = build_llm_provider(&config, &artifact_dir);
        assert_eq!(provider.model_name(), "llama3.1:8b");
    }

    #[tokio::test]
    async fn stream_and_json_together_is_rejected_before_touching_the_ledger() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        // No `ekos init`/`ekos build` in this workspace at all — if the
        // --stream/--json check didn't run before opening the store, this
        // would fail with a *different*, misleading "no ledger" error
        // instead of the real, actionable one.
        let err = run(
            &config,
            dir.path(),
            "anything",
            AskOpts {
                json: true,
                stream: true,
                session: None,
                classic: false,
                explain: false,
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("--stream is not compatible with --json"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn explain_with_classic_is_rejected_before_touching_the_ledger() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let err = run(
            &config,
            dir.path(),
            "anything",
            AskOpts {
                json: false,
                stream: false,
                session: None,
                classic: true,
                explain: true,
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("--explain is not compatible with --classic"),
            "got: {err}"
        );
    }

    // ── RFC 0099: session storage ────────────────────────────────────────

    #[test]
    fn session_path_rejects_names_that_could_escape_the_sessions_directory() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        for bad in ["", "../escape", "a/b", "a b", "a.b", "/abs/path"] {
            assert!(
                session_path(&config, dir.path(), bad).is_err(),
                "expected '{bad}' to be rejected"
            );
        }
    }

    #[test]
    fn session_path_accepts_letters_digits_underscore_and_dash() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        for good in ["session1", "my-session", "my_session", "ABC123"] {
            assert!(
                session_path(&config, dir.path(), good).is_ok(),
                "expected '{good}' to be accepted"
            );
        }
    }

    #[test]
    fn session_path_lands_under_ekos_ask_sessions() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let path = session_path(&config, dir.path(), "demo").unwrap();
        assert_eq!(path, dir.path().join(".ekos/ask-sessions/demo.json"));
    }

    #[test]
    fn load_session_on_a_missing_file_returns_empty_history() {
        let dir = tempdir().unwrap();
        let history = load_session(&dir.path().join("does-not-exist.json")).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn save_then_load_session_round_trips_the_full_history() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ask-sessions/demo.json");
        let history = vec![
            ConversationTurn {
                question: "what tables exist?".to_string(),
                answer: "orders and customers.".to_string(),
            },
            ConversationTurn {
                question: "which one has more columns?".to_string(),
                answer: "orders.".to_string(),
            },
        ];

        save_session(&path, &history).unwrap();
        let loaded = load_session(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].question, "what tables exist?");
        assert_eq!(loaded[1].answer, "orders.");
    }
}
