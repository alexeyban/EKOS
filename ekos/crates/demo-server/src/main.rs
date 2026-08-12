//! RFC 0045 — hosted demo server binary. Read-only: serves pre-rendered
//! static docs for a fixed two-repo catalog plus one `POST /ask` endpoint.
//!
//! Usage: `demo-server [catalog.toml]` (default `catalog.toml` in cwd).
//! `ekos.toml`'s `[llm]` for every non-Ollama repo in the catalog must
//! resolve to a real, working API key — checked here at startup, not
//! discovered as a blank answer during a live demo (RFC 0045, "Live-LLM-
//! endpoint guardrails").

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Query, State},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use ekos_demo_server::{ask, catalog::Catalog, page::html_document};
use ekos_recovery::{AnthropicProvider, OpenAiProvider};
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tower_http::services::ServeDir;

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const RATE_LIMIT_MAX_REQUESTS: u32 = 10;

struct RateLimiter {
    hits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Simple per-IP fixed-window cap — deliberately not general DoS
    /// hardening, just enough that one ~20-person peer demo can't run up an
    /// unbounded live-LLM bill (RFC 0045).
    fn allow(&self, ip: IpAddr) -> bool {
        let mut hits = self.hits.lock().unwrap();
        let now = Instant::now();
        let entry = hits.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) > RATE_LIMIT_WINDOW {
            *entry = (now, 0);
        }
        if entry.1 >= RATE_LIMIT_MAX_REQUESTS {
            return false;
        }
        entry.1 += 1;
        true
    }
}

struct AppState {
    catalog: Catalog,
    limiter: RateLimiter,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let catalog_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "catalog.toml".to_string());
    let catalog_path = std::path::Path::new(&catalog_path);

    // Load a `.env` next to the catalog file if present — same pattern as
    // `marketing/.env` (never overrides a variable already set in the real
    // environment, dotenvy's default). Lets an operator hand this process an
    // OPENAI_API_KEY/ANTHROPIC_API_KEY via a gitignored file instead of
    // depending on shell `export` state surviving into whatever process
    // actually launches this binary.
    if let Some(dir) = catalog_path.parent() {
        dotenvy::from_path(dir.join(".env")).ok();
    }

    let catalog = Catalog::load(catalog_path)
        .with_context(|| format!("loading catalog from {}", catalog_path.display()))?;

    check_llm_keys_or_exit(&catalog);

    let state = Arc::new(AppState {
        catalog,
        limiter: RateLimiter::new(),
    });

    let mut app = Router::new()
        .route("/", get(index))
        .route("/ask", post(handle_ask));

    for repo in &state.catalog.repos {
        let route_path = format!("/docs/{}", repo.slug);
        app = app.nest_service(&route_path, ServeDir::new(&repo.html_dir));
    }

    let app = app.with_state(state);

    let addr: SocketAddr = std::env::var("DEMO_SERVER_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;
    tracing::info!(%addr, "demo-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Fail loudly at boot rather than silently degrading to `MockLlmProvider`
/// at request time (RFC 0045's explicit reason `/ask` needs guardrails at
/// all — `build_llm_provider`'s own fallback is fine for an internal
/// recovery pass, wrong for a public demo endpoint).
fn check_llm_keys_or_exit(catalog: &Catalog) {
    if let Some((slug, key_env)) = first_missing_key(catalog) {
        eprintln!(
            "demo-server: refusing to start — repo '{slug}' needs a working {key_env} \
             (checked at boot, not at request time, so this never surfaces as a blank \
             live-demo answer)"
        );
        std::process::exit(1);
    }
}

/// Pure check, separated from `check_llm_keys_or_exit`'s process exit so it's unit-testable:
/// the slug + resolved key-env-var name of the first catalog repo whose configured provider
/// needs a key that isn't present, or `None` if every repo is ready.
fn first_missing_key(catalog: &Catalog) -> Option<(String, String)> {
    for repo in &catalog.repos {
        let config = repo.load_config();
        if config.llm.provider.as_deref() == Some("ollama") {
            continue; // no key required, per build_llm_provider's own logic
        }
        let key_env = config
            .llm
            .api_key_env
            .as_deref()
            .unwrap_or("ANTHROPIC_API_KEY");
        // Mirrors build_llm_provider's own provider selection (RFC 0021/0046) — check whichever
        // provider ekos.toml actually configured, not always Anthropic.
        let key_present = if config.llm.provider.as_deref() == Some("openai") {
            OpenAiProvider::from_env_var(key_env).is_ok()
        } else {
            AnthropicProvider::from_env_var(key_env).is_ok()
        };
        if !key_present {
            return Some((repo.slug.clone(), key_env.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_demo_server::catalog::RepoEntry;

    fn repo(slug: &str, workspace_dir: std::path::PathBuf) -> RepoEntry {
        RepoEntry {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            tagline: String::new(),
            workspace_dir,
            html_dir: std::path::PathBuf::new(),
            llm_provider: None,
            llm_api_key_env: None,
            ai_max_tokens: None,
        }
    }

    /// RFC 0045 Testing: "server started with ANTHROPIC_API_KEY unset must
    /// exit with a startup error, not serve requests" — the pure half of
    /// that check, without actually exiting the test process. RFC 0046 adds
    /// the OpenAI-provider case. Every case lives in one test function: even
    /// mutating *different* env-var keys concurrently from separate `#[test]`
    /// threads is a real race against the platform's environment table (the
    /// reason `set_var`/`remove_var` are `unsafe` at all, not just same-key
    /// collisions) — so every env-var-touching assertion in this file runs
    /// sequentially, in this one function, on purpose.
    #[test]
    fn first_missing_key_reflects_whether_a_working_key_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = Catalog {
            repos: vec![repo("demo", dir.path().to_path_buf())],
        };

        // SAFETY (test-only): no other test in this crate reads or writes
        // this env var, and every assertion below runs sequentially in one
        // test, never concurrently with another test's env mutation.
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        // No ekos.toml written — from_file_or_default falls back to
        // defaults, which default to the Anthropic provider needing
        // ANTHROPIC_API_KEY.
        assert_eq!(
            first_missing_key(&catalog),
            Some(("demo".to_string(), "ANTHROPIC_API_KEY".to_string()))
        );

        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", "test-key-value");
        }
        assert_eq!(first_missing_key(&catalog), None);

        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }

        // RFC 0046: the same check must follow the catalog's llm_provider
        // override, not always assume Anthropic.
        let openai_dir = tempfile::tempdir().unwrap();
        let openai_repo = RepoEntry {
            llm_provider: Some("openai".to_string()),
            llm_api_key_env: Some("OPENAI_API_KEY".to_string()),
            ..repo("demo-openai", openai_dir.path().to_path_buf())
        };
        let openai_catalog = Catalog {
            repos: vec![openai_repo],
        };

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
        assert_eq!(
            first_missing_key(&openai_catalog),
            Some(("demo-openai".to_string(), "OPENAI_API_KEY".to_string()))
        );

        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test-key-value");
        }
        assert_eq!(first_missing_key(&openai_catalog), None);

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }
}

async fn index(State(state): State<Arc<AppState>>) -> Html<String> {
    let mut cards = String::new();
    for repo in &state.catalog.repos {
        cards.push_str(&format!(
            "<div class=\"repo-card\"><h3>{}</h3><p class=\"muted\">{}</p>\
             <p><a href=\"/docs/{}/Architecture.html\">Browse docs</a></p></div>",
            repo.display_name, repo.tagline, repo.slug
        ));
    }
    let body = format!(
        r#"<main>
<h1>EKOS demo</h1>
<p class="muted">Pick a repo, browse its compiled architecture docs (zero LLM calls), or ask a question and see the cited answer.</p>
<div class="repo-picker">{cards}</div>
<h2>Ask a question</h2>
<form id="ask-form">
  <select id="repo-select">{options}</select>
  <input id="question" type="text" placeholder="What depends on X?" size="50">
  <button type="submit">Ask</button>
</form>
<div id="ask-answer"></div>
<script>
document.getElementById('ask-form').addEventListener('submit', async (e) => {{
  e.preventDefault();
  const repo = document.getElementById('repo-select').value;
  const question = document.getElementById('question').value;
  const out = document.getElementById('ask-answer');
  out.textContent = 'Asking...';
  const res = await fetch('/ask?repo=' + encodeURIComponent(repo), {{
    method: 'POST',
    headers: {{'Content-Type': 'application/json'}},
    body: JSON.stringify({{question}})
  }});
  const data = await res.json();
  if (!res.ok) {{ out.textContent = 'Error: ' + (data.error || res.statusText); return; }}
  let text = data.answer + '\n\nSources:\n';
  for (const c of data.citations) {{
    text += `  [${{Math.round(c.confidence * 100)}}%] ${{c.path}} - "${{c.fragment}}"\n`;
  }}
  out.textContent = text;
}});
</script>
</main>"#,
        options = state
            .catalog
            .repos
            .iter()
            .map(|r| format!("<option value=\"{}\">{}</option>", r.slug, r.display_name))
            .collect::<String>()
    );
    Html(html_document("EKOS demo", &body))
}

#[derive(Deserialize)]
struct AskQuery {
    repo: String,
}

#[derive(Deserialize)]
struct AskBody {
    question: String,
}

async fn handle_ask(
    State(state): State<Arc<AppState>>,
    Query(q): Query<AskQuery>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    axum::Json(body): axum::Json<AskBody>,
) -> impl IntoResponse {
    if !state.limiter.allow(addr.ip()) {
        return error_response(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded — please wait a moment before asking again",
        );
    }

    let Some(repo) = state.catalog.find(&q.repo).cloned() else {
        return error_response(axum::http::StatusCode::NOT_FOUND, "unknown repo");
    };
    let question = body.question.clone();

    // `Runtime`/`KnowledgeStore` (crates/runtime, crates/ledger) were built for the CLI's
    // single-threaded execution and the MCP server's one-message-at-a-time stdio loop —
    // neither needed `Send`/`Sync`, so `dyn KnowledgeStore` isn't. Rather than retrofit
    // thread-safety across that whole stack (out of RFC 0045's scope), each `/ask` request
    // runs on its own blocking thread with its own throwaway single-threaded runtime, so the
    // non-Send ledger/runtime state never needs to cross an axum-visible `.await` boundary.
    let outcome = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build one-shot runtime for /ask");
        rt.block_on(ask::answer_question(&repo, &question))
    })
    .await;

    match outcome {
        Ok(Ok(answer)) => (axum::http::StatusCode::OK, axum::Json(answer)).into_response(),
        Ok(Err(e)) => {
            // `?e` (Debug), not `%e` (Display) — anyhow's Display only shows the outer
            // `.context(...)` message; Debug prints the full "Caused by:" chain, which is
            // what actually pinpoints a failure (rate limits, malformed responses, etc.).
            tracing::error!(error = ?e, repo = %q.repo, "ask failed");
            error_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "ask failed")
        }
        Err(join_err) => {
            tracing::error!(error = %join_err, repo = %q.repo, "ask task panicked");
            error_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "ask failed")
        }
    }
}

fn error_response(status: axum::http::StatusCode, message: &str) -> axum::response::Response {
    (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
}
