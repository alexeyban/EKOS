//! The browser wizard behind `ekos setup` (RFC 0185 §3).
//!
//! axum on `127.0.0.1:<port>`; one embedded page. Every request must carry the random token
//! the URL was opened with (`?t=` for the page, `x-setup-token` for the API), so another page
//! in the same browser, or another user of the machine, cannot drive the installer. The server
//! stops when the person finishes, or after an hour without a request.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::setup::{self, Answers, Machine, RealEffects};

const PAGE: &str = include_str!("setup.html");
const IDLE: Duration = Duration::from_secs(3600);

#[derive(Clone)]
pub struct Wizard {
    token: String,
    machine: Machine,
    home: PathBuf,
    last_seen: Arc<Mutex<Instant>>,
    done: Arc<tokio::sync::Notify>,
}

impl Wizard {
    pub fn new(token: String, machine: Machine, home: PathBuf) -> Self {
        Self {
            token,
            machine,
            home,
            last_seen: Arc::new(Mutex::new(Instant::now())),
            done: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn allowed(&self, headers: &HeaderMap, query: Option<&str>) -> bool {
        *self.last_seen.lock().unwrap() = Instant::now();
        let given = headers
            .get("x-setup-token")
            .and_then(|v| v.to_str().ok())
            .or(query)
            .unwrap_or("");
        // Constant time is not needed against a 256-bit random token over loopback, but it
        // costs nothing.
        given.len() == self.token.len()
            && given
                .bytes()
                .zip(self.token.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
}

fn refused() -> Response {
    (StatusCode::FORBIDDEN, "this setup link is not valid").into_response()
}

fn bad(e: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": e.to_string()})),
    )
        .into_response()
}

#[derive(Deserialize)]
struct TokenQuery {
    t: Option<String>,
}

pub fn router(w: Wizard) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/api/state", get(state))
        .route("/api/check-path", post(check_path))
        .route("/api/check-issuer", post(check_issuer))
        .route("/api/ollama-models", get(ollama_models))
        .route("/api/check-model", post(check_model))
        .route("/api/plan", post(plan))
        .route("/api/apply", post(apply))
        .route("/api/finish", post(finish))
        .with_state(w)
}

async fn page(
    State(w): State<Wizard>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    if !w.allowed(&headers, q.t.as_deref()) {
        return refused();
    }
    Html(PAGE).into_response()
}

/// What is already set: the home's `console.toml`, so a second run starts from it.
pub fn current_answers(home: &Path) -> Answers {
    let mut a = Answers {
        home: home.display().to_string(),
        add_to_path: true,
        ..Default::default()
    };
    let path = home.join("control").join("console.toml");
    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(t) = text.parse::<toml::Table>()
    {
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let n = |k: &str| t.get(k).and_then(|v| v.as_integer());
        let c = &mut a.console;
        if let Some(v) = s("mode") {
            c.mode = v;
        }
        if let Some(v) = s("host") {
            c.host = v;
        }
        if let Some(v) = n("port").and_then(|p| u16::try_from(p).ok()) {
            c.port = v;
        }
        for (key, field) in [
            ("oidc_issuer", &mut c.oidc_issuer),
            ("oidc_client_id", &mut c.oidc_client_id),
            ("oidc_redirect_uri", &mut c.oidc_redirect_uri),
            ("owners", &mut c.owners),
        ] {
            if let Some(v) = s(key) {
                *field = v;
            }
        }
        if let Some(v) = t.get("production").and_then(|v| v.as_bool()) {
            c.production = v;
        }
        if let Some(v) = n("mcp_idle_seconds").and_then(|p| u32::try_from(p).ok()) {
            c.mcp_idle_seconds = v;
        }
        if let Some(v) = n("mcp_max_servers").and_then(|p| u32::try_from(p).ok()) {
            c.mcp_max_servers = v;
        }
    }
    let manifest = setup::read_manifest(home);
    if !manifest.program_dir.as_os_str().is_empty() {
        a.program_dir = manifest.program_dir.display().to_string();
    }
    a
}

/// `program` on `PATH`, as Windows or Unix would find it.
pub fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: Vec<String> = if cfg!(windows) {
        ["", ".exe", ".cmd", ".bat"]
            .iter()
            .map(|e| format!("{program}{e}"))
            .collect()
    } else {
        vec![program.to_string()]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

async fn state(State(w): State<Wizard>, headers: HeaderMap) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    let m = &w.machine;
    let projects = crate::home::list_projects(&w.home)
        .map(|ps| {
            ps.iter()
                .filter(|p| p.tenant == "default")
                .map(|p| p.project.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let runtime = super::console::find_runtime(&m.package.join("bin").join("ekos"), &|_| None)
        .is_ok()
        || m.package.join("console").join("app").is_dir();
    Json(serde_json::json!({
        "os": m.os,
        "answers": current_answers(&w.home),
        "installed": w.home.join("control").join("console.toml").is_file(),
        "package": m.package,
        "console_runtime": runtime,
        "projects": projects,
        "agents": {
            "claude_code": on_path("claude"),
            "claude_desktop": m.claude_desktop_config().parent().is_some_and(Path::is_dir),
            "vscode": m.vscode_mcp_config().parent().is_some_and(Path::is_dir),
        },
        "paths": {
            "claude_desktop": m.claude_desktop_config(),
            "vscode": m.vscode_mcp_config(),
        },
    }))
    .into_response()
}

#[derive(Deserialize)]
struct PathCheck {
    path: String,
}

/// Whether a directory can hold EKOS: it, or its nearest existing parent, is writable; and
/// the free space there.
pub fn check_dir(path: &Path) -> serde_json::Value {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        match probe.parent() {
            Some(p) => probe = p.to_path_buf(),
            None => break,
        }
    }
    let marker = probe.join(format!(".ekos-setup-check-{}", std::process::id()));
    let writable = std::fs::write(&marker, b"").is_ok();
    let _ = std::fs::remove_file(&marker);
    let free = fs4::available_space(&probe).ok();
    serde_json::json!({
        "exists": path.exists(),
        "writable": writable,
        "free_bytes": free,
        "absolute": path.is_absolute(),
    })
}

async fn check_path(
    State(w): State<Wizard>,
    headers: HeaderMap,
    Json(body): Json<PathCheck>,
) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    Json(check_dir(Path::new(body.path.trim()))).into_response()
}

#[derive(Deserialize)]
struct IssuerCheck {
    url: String,
}

async fn check_issuer(
    State(w): State<Wizard>,
    headers: HeaderMap,
    Json(body): Json<IssuerCheck>,
) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    let url = format!(
        "{}/.well-known/openid-configuration",
        body.url.trim().trim_end_matches('/')
    );
    let result = async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        let doc: serde_json::Value = client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::Ok(doc)
    }
    .await;
    Json(match result {
        Ok(doc) => serde_json::json!({
            "ok": true,
            "message": format!("found {}", doc["issuer"].as_str().unwrap_or("an issuer")),
        }),
        Err(e) => serde_json::json!({"ok": false, "message": format!("{url}: {e}")}),
    })
    .into_response()
}

fn ollama_base(base: &str) -> String {
    let b = if base.trim().is_empty() {
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".into())
    } else {
        base.trim().to_string()
    };
    let b = if b.starts_with("http") {
        b
    } else {
        format!("http://{b}")
    };
    b.trim_end_matches('/').to_string()
}

async fn fetch_json(url: &str, headers: &[(&str, String)]) -> anyhow::Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    Ok(req.send().await?.error_for_status()?.json().await?)
}

/// The models a local Ollama server has: what the wizard offers for a chat that costs nothing.
async fn ollama_models(State(w): State<Wizard>, headers: HeaderMap) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    let url = format!("{}/api/tags", ollama_base(""));
    let names: Vec<String> = match fetch_json(&url, &[]).await {
        Ok(v) => v["models"]
            .as_array()
            .map(|ms| {
                ms.iter()
                    .filter_map(|m| m["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    Json(serde_json::json!({"models": names})).into_response()
}

#[derive(Deserialize)]
struct ModelCheck {
    provider: String,
    model: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
}

/// RFC 0187: whether the chat model can be reached, **without a paid call**: each provider's
/// list of models (free), and whether the chosen one is in it.
pub async fn model_check(c: &ModelCheckInput<'_>) -> (bool, String) {
    let (url, headers): (String, Vec<(&str, String)>) = match c.provider {
        "ollama" => (format!("{}/api/tags", ollama_base(c.base_url)), vec![]),
        "anthropic" => (
            format!(
                "{}/v1/models",
                if c.base_url.trim().is_empty() {
                    "https://api.anthropic.com"
                } else {
                    c.base_url.trim().trim_end_matches('/')
                }
            ),
            vec![
                ("x-api-key", c.api_key.to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
        ),
        "openai" => (
            format!(
                "{}/models",
                if c.base_url.trim().is_empty() {
                    "https://api.openai.com/v1"
                } else {
                    c.base_url.trim().trim_end_matches('/')
                }
            ),
            if c.api_key.is_empty() {
                vec![]
            } else {
                vec![("authorization", format!("Bearer {}", c.api_key))]
            },
        ),
        other => return (false, format!("unknown provider {other:?}")),
    };
    match fetch_json(&url, &headers).await {
        Ok(v) => {
            let list = v["models"].as_array().or_else(|| v["data"].as_array());
            let names: Vec<&str> = list
                .map(|l| {
                    l.iter()
                        .filter_map(|m| m["name"].as_str().or_else(|| m["id"].as_str()))
                        .collect()
                })
                .unwrap_or_default();
            if names.contains(&c.model) {
                (true, format!("{} answers and has {}", c.provider, c.model))
            } else {
                (
                    false,
                    format!(
                        "{} answers but has no model {:?}{}",
                        c.provider,
                        c.model,
                        if names.is_empty() {
                            String::new()
                        } else {
                            format!(" (it has: {})", names.join(", "))
                        }
                    ),
                )
            }
        }
        Err(e) => (false, format!("{url}: {e}")),
    }
}

pub struct ModelCheckInput<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub api_key: &'a str,
}

async fn check_model(
    State(w): State<Wizard>,
    headers: HeaderMap,
    Json(body): Json<ModelCheck>,
) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    let (ok, message) = model_check(&ModelCheckInput {
        provider: &body.provider,
        model: &body.model,
        base_url: &body.base_url,
        api_key: &body.api_key,
    })
    .await;
    Json(serde_json::json!({"ok": ok, "message": message})).into_response()
}

async fn plan(State(w): State<Wizard>, headers: HeaderMap, Json(a): Json<Answers>) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    match setup::plan(&a, &w.machine) {
        Ok(actions) => Json(serde_json::json!({
            "actions": actions.iter().map(|x| x.describe()).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => bad(e),
    }
}

async fn apply(State(w): State<Wizard>, headers: HeaderMap, Json(a): Json<Answers>) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    let machine = w.machine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let actions = setup::plan(&a, &machine)?;
        let steps = setup::apply(&actions, &a, &machine, &RealEffects)?;
        let layout = setup::Layout::of(&a, &machine);
        let conf = super::console::server_conf(&layout.home, &|_| None)?;
        anyhow::Ok((steps, super::console::url(&conf)))
    })
    .await;
    match result {
        Ok(Ok((steps, url))) => {
            Json(serde_json::json!({"steps": steps, "console_url": url})).into_response()
        }
        Ok(Err(e)) => bad(format!("{e:#}")),
        Err(e) => bad(e),
    }
}

async fn finish(State(w): State<Wizard>, headers: HeaderMap) -> Response {
    if !w.allowed(&headers, None) {
        return refused();
    }
    w.done.notify_one();
    Json(serde_json::json!({"ok": true})).into_response()
}

/// Serve the wizard until it is finished or idle for an hour. Returns once stopped.
pub async fn serve(w: Wizard, port: u16, open_browser: bool) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .context("binding the setup page to 127.0.0.1")?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/?t={}", w.token);
    println!("EKOS setup: {url}");
    if open_browser && !super::console::open_browser(&url) {
        println!("Open that address in a browser on this machine.");
    }
    let done = w.done.clone();
    let last_seen = w.last_seen.clone();
    let stop = async move {
        loop {
            tokio::select! {
                _ = done.notified() => break,
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if last_seen.lock().unwrap().elapsed() > IDLE {
                        println!("EKOS setup: an hour without a request; stopping");
                        break;
                    }
                }
            }
        }
    };
    axum::serve(listener, router(w))
        .with_graceful_shutdown(stop)
        .await?;
    println!("EKOS setup finished.");
    Ok(())
}

pub fn new_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn wizard(dir: &Path) -> Wizard {
        let m = Machine {
            os: setup::Os::Linux,
            user_home: dir.join("user"),
            appdata: None,
            package: dir.join("package"),
            shell: None,
        };
        Wizard::new("tok".into(), m, dir.join("home"))
    }

    async fn status(w: &Wizard, req: Request<Body>) -> u16 {
        router(w.clone())
            .oneshot(req)
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    #[tokio::test]
    async fn every_route_needs_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let w = wizard(dir.path());
        let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();
        assert_eq!(status(&w, get("/")).await, 403);
        assert_eq!(status(&w, get("/?t=wrong")).await, 403);
        assert_eq!(status(&w, get("/?t=tok")).await, 200);
        assert_eq!(status(&w, get("/api/state")).await, 403);
        let with = Request::get("/api/state")
            .header("x-setup-token", "tok")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(&w, with).await, 200);
        let apply = Request::post("/api/apply")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status(&w, apply).await, 403);
    }

    #[tokio::test]
    async fn a_plan_is_shown_before_anything_happens() {
        let dir = tempfile::tempdir().unwrap();
        let w = wizard(dir.path());
        let body = serde_json::json!({"console": {"mode": "team", "production": true}});
        let req = Request::post("/api/plan")
            .header("x-setup-token", "tok")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(status(&w, req).await, 400);
        let req = Request::post("/api/plan")
            .header("x-setup-token", "tok")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = router(w.clone()).oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["actions"].as_array().unwrap().len() >= 2);
        assert!(!dir.path().join("home").exists(), "planning wrote nothing");
    }

    #[test]
    fn the_answers_start_from_what_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("control")).unwrap();
        std::fs::write(
            home.join("control/console.toml"),
            "mode = \"team\"\nport = 9100\nowners = \"a@b.c\"\nproduction = true\n",
        )
        .unwrap();
        let a = current_answers(home);
        assert_eq!(a.console.mode, "team");
        assert_eq!(a.console.port, 9100);
        assert_eq!(a.console.owners, "a@b.c");
        assert!(a.console.production);
        assert!(a.console.oidc_client_secret.is_empty());
    }

    #[tokio::test]
    async fn a_model_check_reads_the_model_list_and_calls_nothing_paid() {
        // A stand-in for Ollama's /api/tags.
        let app = Router::new().route(
            "/api/tags",
            get(|| async { Json(serde_json::json!({"models": [{"name": "qwen2.5:1.5b"}]})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let check = |model: &'static str| ModelCheckInput {
            provider: "ollama",
            model,
            base_url: "",
            api_key: "",
        };
        let (ok, msg) = model_check(&ModelCheckInput {
            base_url: &base,
            ..check("qwen2.5:1.5b")
        })
        .await;
        assert!(ok, "{msg}");
        let (ok, msg) = model_check(&ModelCheckInput {
            base_url: &base,
            ..check("llama9")
        })
        .await;
        assert!(!ok && msg.contains("qwen2.5:1.5b"), "{msg}");
        let (ok, _) = model_check(&ModelCheckInput {
            provider: "nope",
            ..check("x")
        })
        .await;
        assert!(!ok);
    }

    #[test]
    fn a_directory_check() {
        let dir = tempfile::tempdir().unwrap();
        let v = check_dir(&dir.path().join("not/yet"));
        assert_eq!(v["exists"], false);
        assert_eq!(v["writable"], true);
        assert!(v["free_bytes"].as_u64().unwrap() > 0);
    }
}
