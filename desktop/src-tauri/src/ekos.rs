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
/// runtime; else `EKOS_BIN`; else where an installer or `ekos setup` puts it; else `ekos` from
/// `PATH` (a checkout). The installed places matter on macOS, where an app started from the Finder
/// gets a `PATH` without `/usr/local/bin`.
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
        let env = std::env::var_os("EKOS_BIN")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let program = beside
            .or(env)
            .or_else(|| installed_places().into_iter().find(|p| p.is_file()))
            .unwrap_or_else(|| PathBuf::from("ekos"));
        Self { program }
    }

    /// Runs `ekos console <args>` and returns its standard output; a non-zero exit is an error
    /// carrying the command's own message.
    fn console(&self, args: &[&str]) -> Result<String> {
        let mut cmd = Command::new(&self.program);
        cmd.arg("console").args(args).stdin(Stdio::null());
        no_window(&mut cmd);
        let out = cmd
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

/// Where the installers put `ekos` (RFC 0185, the .msi, .pkg, .deb and .rpm), then the copy
/// `ekos setup` makes from an archive in the default home (`~/.ekos/bin`).
fn installed_places() -> Vec<PathBuf> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    places_for(std::env::consts::OS, home, local_app_data)
}

/// [`installed_places`] for one system, from its environment; pure, so it is tested anywhere.
pub fn places_for(
    os: &str,
    home: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut places = Vec::new();
    let exe = if os == "windows" { "ekos.exe" } else { "ekos" };
    match os {
        "windows" => places
            .extend(local_app_data.map(|d| d.join("Programs").join("EKOS").join("bin").join(exe))),
        "macos" => places.push(PathBuf::from("/usr/local/ekos/bin/ekos")),
        _ => places.push(PathBuf::from("/opt/ekos/bin/ekos")),
    }
    places.extend(home.map(|h| h.join(".ekos").join("bin").join(exe)));
    places
}

/// On Windows the app has no console of its own (`windows_subsystem = "windows"`), so each
/// `ekos console …` it runs would open a terminal window for a moment without this flag.
#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

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
    fn the_installed_places_are_each_installers_then_the_setup_copy() {
        let home = Some(PathBuf::from("/home/p"));
        assert_eq!(
            places_for("linux", home.clone(), None),
            [
                PathBuf::from("/opt/ekos/bin/ekos"),
                PathBuf::from("/home/p/.ekos/bin/ekos")
            ]
        );
        assert_eq!(
            places_for("macos", home, None)[0],
            PathBuf::from("/usr/local/ekos/bin/ekos")
        );
        let win = places_for(
            "windows",
            Some(PathBuf::from("U")),
            Some(PathBuf::from("L")),
        );
        assert_eq!(
            win,
            [
                PathBuf::from("L")
                    .join("Programs")
                    .join("EKOS")
                    .join("bin")
                    .join("ekos.exe"),
                PathBuf::from("U")
                    .join(".ekos")
                    .join("bin")
                    .join("ekos.exe"),
            ]
        );
        assert!(places_for("windows", None, None).is_empty());
    }

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
