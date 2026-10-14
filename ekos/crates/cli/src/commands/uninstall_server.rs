//! The page behind `ekos uninstall --ui` (RFC 0188).
//!
//! Same shape as the setup wizard: axum on `127.0.0.1:<port>`, one embedded page, the random
//! token from the link on every request. The page is in this program's memory, so it keeps
//! answering while the program directory, the console and (with purge) the home are removed.
//! The server stops a few seconds after the removal, or after an hour without a request.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::setup::{self, Effects, Machine};
use super::setup_server::token_matches;

const PAGE: &str = include_str!("uninstall.html");
const IDLE: Duration = Duration::from_secs(3600);
/// What the person types to remove the program, and to remove the home with it.
pub const CONFIRM: &str = "uninstall";
pub const CONFIRM_PURGE: &str = "delete everything";

#[derive(Clone)]
pub struct UninstallPage {
    token: String,
    home: PathBuf,
    machine: Machine,
    effects: Arc<dyn Effects + Send + Sync>,
    last_seen: Arc<Mutex<Instant>>,
    done: Arc<tokio::sync::Notify>,
}

impl UninstallPage {
    pub fn new(
        token: String,
        home: PathBuf,
        machine: Machine,
        effects: Arc<dyn Effects + Send + Sync>,
    ) -> Self {
        Self {
            token,
            home,
            machine,
            effects,
            last_seen: Arc::new(Mutex::new(Instant::now())),
            done: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn allowed(&self, headers: &HeaderMap, query: Option<&str>) -> bool {
        *self.last_seen.lock().unwrap() = Instant::now();
        token_matches(&self.token, headers, query)
    }
}

fn refused() -> Response {
    (StatusCode::FORBIDDEN, "this uninstall link is not valid").into_response()
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

#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    purge: bool,
    #[serde(default)]
    confirm: String,
}

pub fn router(p: UninstallPage) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/api/plan", get(plan))
        .route("/api/uninstall", post(uninstall))
        .with_state(p)
}

async fn page(
    State(p): State<UninstallPage>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    if !p.allowed(&headers, q.t.as_deref()) {
        return refused();
    }
    Html(PAGE).into_response()
}

async fn plan(State(p): State<UninstallPage>, headers: HeaderMap) -> Response {
    if !p.allowed(&headers, None) {
        return refused();
    }
    let keep = setup::uninstall_plan(&p.home, false);
    let purge = setup::uninstall_plan(&p.home, true);
    Json(serde_json::json!({
        "home": keep.home,
        "program_dir": keep.program_dir,
        "installed": keep.installed,
        "steps": keep.steps,
        "purge_steps": purge.steps,
        "projects": keep.projects,
    }))
    .into_response()
}

async fn uninstall(
    State(p): State<UninstallPage>,
    headers: HeaderMap,
    Json(r): Json<Request>,
) -> Response {
    if !p.allowed(&headers, None) {
        return refused();
    }
    let expected = if r.purge { CONFIRM_PURGE } else { CONFIRM };
    if r.confirm.trim() != expected {
        return bad(format!("type \"{expected}\" to confirm"));
    }
    let (home, machine, effects) = (p.home.clone(), p.machine.clone(), p.effects.clone());
    let result = tokio::task::spawn_blocking(move || {
        setup::uninstall(&home, r.purge, &machine, effects.as_ref())
    })
    .await;
    match result {
        Ok(Ok(steps)) => {
            // Let the answer reach the page before the server stops — and stop before Windows'
            // delayed removal of `bin\` (3 s, `setup::uninstall`) finds this program in use.
            let done = p.done.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                done.notify_one();
            });
            Json(serde_json::json!({ "steps": steps })).into_response()
        }
        Ok(Err(e)) => bad(format!("{e:#}")),
        Err(e) => bad(e),
    }
}

/// The first line the server prints; `--detach` waits for it in the child's output.
pub const URL_PREFIX: &str = "EKOS uninstall: ";

/// Serve the page until the removal is done or an hour passes without a request.
pub async fn serve(p: UninstallPage, port: u16, open_browser: bool) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .context("binding the uninstall page to 127.0.0.1")?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/?t={}", p.token);
    println!("{URL_PREFIX}{url}");
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    if open_browser && !super::console::open_browser(&url) {
        println!("Open that address in a browser on this machine.");
    }
    let done = p.done.clone();
    let last_seen = p.last_seen.clone();
    let stop = async move {
        loop {
            tokio::select! {
                _ = done.notified() => break,
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if last_seen.lock().unwrap().elapsed() > IDLE {
                        println!("EKOS uninstall: an hour without a request; stopping");
                        break;
                    }
                }
            }
        }
    };
    axum::serve(listener, router(p))
        .with_graceful_shutdown(stop)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use std::path::Path;
    use tower::ServiceExt;

    /// Records programs instead of running them.
    #[derive(Default)]
    struct Recorded(Mutex<Vec<String>>);

    impl Effects for Recorded {
        fn run_env(
            &self,
            program: &Path,
            _: &[String],
            _: &[(String, String)],
            _: Option<&str>,
        ) -> Result<String> {
            self.0.lock().unwrap().push(program.display().to_string());
            Ok(String::new())
        }
        fn open(&self, _: &str) -> bool {
            true
        }
    }

    fn page_for(home: &Path) -> UninstallPage {
        let m = Machine {
            os: setup::Os::Linux,
            user_home: home.parent().unwrap().join("user"),
            appdata: None,
            package: home.join("package"),
            shell: None,
        };
        UninstallPage::new(
            "tok".into(),
            home.to_path_buf(),
            m,
            Arc::new(Recorded::default()),
        )
    }

    fn installed_home(dir: &Path) -> PathBuf {
        let home = dir.join("home");
        for d in ["bin", "console/app", "control", "data/default/shop/state"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::fs::write(home.join("bin/ekos"), "bin").unwrap();
        std::fs::write(home.join("data/default/shop/state/f"), "1234567").unwrap();
        std::fs::write(home.join("control/install.json"), "{}").unwrap();
        home
    }

    async fn send(p: &UninstallPage, req: HttpRequest<Body>) -> (u16, serde_json::Value) {
        let resp = router(p.clone()).oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    fn post(token: &str, body: serde_json::Value) -> HttpRequest<Body> {
        HttpRequest::post("/api/uninstall")
            .header("x-setup-token", token)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn every_route_needs_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let p = page_for(&installed_home(dir.path()));
        let get = |uri: &str| HttpRequest::get(uri).body(Body::empty()).unwrap();
        assert_eq!(send(&p, get("/")).await.0, 403);
        assert_eq!(send(&p, get("/?t=nope")).await.0, 403);
        assert_eq!(send(&p, get("/api/plan")).await.0, 403);
        let body = serde_json::json!({"confirm": CONFIRM});
        assert_eq!(send(&p, post("nope", body)).await.0, 403);
        assert!(dir.path().join("home/bin").exists(), "nothing was removed");
        assert_eq!(send(&p, get("/?t=tok")).await.0, 200);
    }

    #[tokio::test]
    async fn the_plan_lists_steps_and_projects() {
        let dir = tempfile::tempdir().unwrap();
        let p = page_for(&installed_home(dir.path()));
        let req = HttpRequest::get("/api/plan")
            .header("x-setup-token", "tok")
            .body(Body::empty())
            .unwrap();
        let (status, v) = send(&p, req).await;
        assert_eq!(status, 200);
        assert_eq!(v["installed"], true);
        assert_eq!(v["projects"][0]["name"], "shop");
        let steps = v["steps"].as_array().unwrap();
        let purge = v["purge_steps"].as_array().unwrap();
        assert_eq!(purge.len(), steps.len() + 1);
        assert!(
            purge
                .last()
                .unwrap()
                .as_str()
                .unwrap()
                .contains("remove the home")
        );
    }

    #[tokio::test]
    async fn a_wrong_word_changes_nothing_and_the_right_one_removes() {
        let dir = tempfile::tempdir().unwrap();
        let home = installed_home(dir.path());
        let p = page_for(&home);
        let (status, v) = send(&p, post("tok", serde_json::json!({"confirm": "yes"}))).await;
        assert_eq!(status, 400, "{v}");
        // The word for keeping the home does not purge it.
        let wrong = serde_json::json!({"purge": true, "confirm": CONFIRM});
        assert_eq!(send(&p, post("tok", wrong)).await.0, 400);
        assert!(home.join("bin").exists() && home.join("console").exists());

        let (status, v) = send(&p, post("tok", serde_json::json!({"confirm": CONFIRM}))).await;
        assert_eq!(status, 200, "{v}");
        assert!(
            v["steps"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["ok"] == true),
            "{v}"
        );
        assert!(!home.join("bin").exists() && !home.join("console").exists());
        assert!(home.join("data/default/shop").is_dir(), "the home is kept");
    }

    #[tokio::test]
    async fn purge_removes_the_home() {
        let dir = tempfile::tempdir().unwrap();
        let home = installed_home(dir.path());
        let p = page_for(&home);
        let body = serde_json::json!({"purge": true, "confirm": CONFIRM_PURGE});
        let (status, v) = send(&p, post("tok", body)).await;
        assert_eq!(status, 200, "{v}");
        assert!(!home.exists());
    }
}
