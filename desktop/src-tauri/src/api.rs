//! The shell's own session with the console (RFC 0261 §2.1): signed in with a one-time code of
//! its own, in a cookie jar the webview never sees. It reads projects and runs, and writes one
//! thing only: a run's cancellation, when the person chooses "Cancel the run and quit".

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;

/// A project as `GET /api/workspaces` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
}

/// A run as `GET /api/runs` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Run {
    pub id: String,
    pub workspace_id: String,
    pub command: String,
    pub status: String,
}

/// The session is gone (console restarted, cookie expired): sign in again.
#[derive(Debug)]
pub struct SignedOut;

impl std::fmt::Display for SignedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the shell's console session ended")
    }
}

impl std::error::Error for SignedOut {}

pub struct Api {
    base: String,
    http: Client,
}

impl Api {
    /// Signs in through `signin_url` (one-time code, RFC 0184) and keeps the session cookie.
    pub fn sign_in(base: &str, signin_url: &str) -> Result<Self> {
        let http = Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(10))
            .build()?;
        let resp = http.get(signin_url).send().context("signing in")?;
        if !resp.status().is_success() {
            bail!("signing in: the console answered {}", resp.status());
        }
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            http,
        })
    }

    fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let resp = self.http.get(format!("{}{path}", self.base)).send()?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(SignedOut.into());
        }
        Ok(resp.error_for_status()?.json()?)
    }

    pub fn projects(&self) -> Result<Vec<Project>> {
        self.get("/api/workspaces")
    }

    /// The newest runs, every status.
    pub fn recent_runs(&self) -> Result<Vec<Run>> {
        self.get("/api/runs?limit=100")
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/runs/{id}/cancel", self.base))
            .send()?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(SignedOut.into());
        }
        resp.error_for_status()?;
        Ok(())
    }
}

/// Whether a run has not ended yet.
pub fn is_active(run: &Run) -> bool {
    matches!(run.status.as_str(), "queued" | "running")
}

/// Which ended runs are news. A run that starts and ends between two polls is never seen
/// active, so "left the active set" would miss it: every ended run not seen before is announced,
/// except on the first poll, which only records what had ended before the shell started.
#[derive(Debug, Default)]
pub struct RunWatch {
    seen_ended: BTreeSet<String>,
    primed: bool,
}

impl RunWatch {
    pub fn observe<'a>(&mut self, recent: &'a [Run]) -> Vec<&'a Run> {
        let mut news = Vec::new();
        for run in recent.iter().filter(|r| !is_active(r)) {
            if self.seen_ended.insert(run.id.clone()) && self.primed {
                news.push(run);
            }
        }
        self.primed = true;
        news
    }
}

/// The notification for an ended run: title and body (design: "Init and compile finished",
/// "idempiere-erp · all 7 stages done").
pub fn ended_message(run: &Run, project: Option<&str>) -> (String, String) {
    let verb = match run.status.as_str() {
        "succeeded" => "finished",
        "failed" => "failed",
        "cancelled" => "was cancelled",
        "timed_out" => "timed out",
        "interrupted" => "was interrupted",
        other => other,
    };
    let title = format!("{} {verb}", command_words(&run.command));
    let body = format!(
        "{} · {}",
        project.unwrap_or(&run.workspace_id),
        run.status.replace('_', " ")
    );
    (title, body)
}

/// A catalogue command name as words: `init-and-compile` → "Init and compile".
pub fn command_words(command: &str) -> String {
    capitalised(&command.replace('-', " "))
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The tray's status line.
pub fn status_line(active: usize, quitting: bool) -> String {
    let runs = match active {
        0 => "no runs".to_string(),
        1 => "1 run in progress".to_string(),
        n => format!("{n} runs in progress"),
    };
    if quitting {
        format!("Quitting when the run ends · {runs}")
    } else {
        format!("Running · {runs}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, status: &str) -> Run {
        Run {
            id: id.into(),
            workspace_id: "w1".into(),
            command: "init and compile".into(),
            status: status.into(),
        }
    }

    #[test]
    fn every_run_that_ended_since_the_last_poll_is_news_once() {
        let mut watch = RunWatch::default();
        // First poll: what had ended before the shell started is not announced.
        assert!(
            watch
                .observe(&[run("old", "succeeded"), run("a", "running")])
                .is_empty()
        );
        // "b" started and ended between two polls; "a" ended.
        let now = [
            run("b", "failed"),
            run("a", "succeeded"),
            run("old", "succeeded"),
        ];
        let news: Vec<&str> = watch.observe(&now).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(news, vec!["b", "a"]);
        assert!(watch.observe(&now).is_empty(), "announced once");
    }

    #[test]
    fn the_notification_names_the_command_project_and_outcome() {
        let (t, b) = ended_message(&run("a", "succeeded"), Some("idempiere-erp"));
        let mut dashed = run("a", "succeeded");
        dashed.command = "init-and-compile".into();
        assert_eq!(ended_message(&dashed, None).0, "Init and compile finished");
        assert_eq!(t, "Init and compile finished");
        assert_eq!(b, "idempiere-erp · succeeded");
        let (t, b) = ended_message(&run("a", "timed_out"), None);
        assert_eq!(t, "Init and compile timed out");
        assert_eq!(b, "w1 · timed out");
    }

    #[test]
    fn the_status_line_counts_runs() {
        assert_eq!(status_line(0, false), "Running · no runs");
        assert_eq!(status_line(1, false), "Running · 1 run in progress");
        assert_eq!(
            status_line(2, true),
            "Quitting when the run ends · 2 runs in progress"
        );
    }
}
