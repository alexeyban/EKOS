//! The `ekos` binary, as the shell runs it (RFC 0261 §2.1): the console's lifecycle is
//! `ekos console …`, never uvicorn or the control files directly.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// What `ekos console open --json` prints (RFC 0261 §2.3): the address and, in local mode, a
/// one-time code (`null` in team mode).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OpenTarget {
    pub url: String,
    #[serde(default)]
    pub code: Option<String>,
}

impl OpenTarget {
    /// The sign-in link (`/api/auth/local?code=…`); `None` in team mode.
    pub fn signin_url(&self) -> Option<String> {
        self.code
            .as_ref()
            .map(|c| format!("{}/api/auth/local?code={c}", self.url.trim_end_matches('/')))
    }
}

/// The `ekos` program (RFC 0261 §2.3): beside the app's own executable, as the console finds its
/// runtime; else `EKOS_BIN`; else `ekos` from `PATH` (a checkout).
#[derive(Debug, Clone)]
pub struct Ekos {
    program: PathBuf,
}

impl Ekos {
    pub fn from_env() -> Self {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join(ekos_file_name())))
            .filter(|p| p.is_file());
        let program = beside
            .or_else(|| {
                std::env::var_os("EKOS_BIN")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("ekos"));
        Self { program }
    }

    /// Runs `ekos console <args>` and returns its standard output; a non-zero exit is an error
    /// carrying the command's own message.
    fn console(&self, args: &[&str]) -> Result<String> {
        let out = Command::new(&self.program)
            .arg("console")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {}", self.program.display()))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let msg = stderr.trim();
            bail!(
                "`ekos console {}` failed: {}",
                args.join(" "),
                if msg.is_empty() { stdout.trim() } else { msg }
            );
        }
        Ok(stdout)
    }

    /// Whether the console answers now (`ekos console status`).
    pub fn is_running(&self) -> Result<bool> {
        Ok(status_says_running(&self.console(&["status"])?))
    }

    pub fn start(&self) -> Result<()> {
        self.console(&["start"]).map(drop)
    }

    pub fn stop(&self) -> Result<()> {
        self.console(&["stop"]).map(drop)
    }

    pub fn restart(&self) -> Result<()> {
        self.console(&["restart"]).map(drop)
    }

    /// The console's address and, in local mode, a fresh one-time sign-in link.
    pub fn open_target(&self) -> Result<OpenTarget> {
        parse_open_target(&self.console(&["open", "--json"])?)
    }

    /// Opens the console in the system browser, signed in with its own fresh code.
    pub fn open_in_browser(&self) -> Result<()> {
        self.console(&["open"]).map(drop)
    }

    /// The end of the console's log.
    pub fn log_tail(&self, lines: usize) -> Result<String> {
        self.console(&["logs", "--lines", &lines.to_string()])
    }
}

fn ekos_file_name() -> &'static str {
    if cfg!(windows) { "ekos.exe" } else { "ekos" }
}

/// `ekos console status` begins with "running at" only when the console answers.
pub fn status_says_running(stdout: &str) -> bool {
    stdout.trim_start().starts_with("running at ")
}

/// The JSON line of `open --json`; anything printed before it (e.g. by `--start`) is skipped.
pub fn parse_open_target(stdout: &str) -> Result<OpenTarget> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .context("`ekos console open --json` printed no JSON")?;
    serde_json::from_str(line).context("reading `ekos console open --json`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_answering_console_counts_as_running() {
        assert!(status_says_running(
            "running at http://127.0.0.1:8000 (local mode) since …\n"
        ));
        assert!(!status_says_running(
            "not answering at http://127.0.0.1:8000 (supervisor pid 1 …)"
        ));
        assert!(!status_says_running(
            "not running (configured for http://127.0.0.1:8000 in local mode)"
        ));
    }

    #[test]
    fn the_open_target_is_the_last_json_line() {
        let t = parse_open_target(
            "The console runs at http://127.0.0.1:8000\n\
             {\"url\":\"http://127.0.0.1:8000\",\"code\":\"ab\"}\n",
        )
        .unwrap();
        assert_eq!(
            t.signin_url().as_deref(),
            Some("http://127.0.0.1:8000/api/auth/local?code=ab")
        );
        let team = parse_open_target("{\"url\":\"http://h:1\",\"code\":null}").unwrap();
        assert_eq!(team.signin_url(), None);
        assert!(parse_open_target("nothing here").is_err());
    }
}
