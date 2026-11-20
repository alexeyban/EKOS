//! `ekos-login.exe`: the Microsoft Store package's startup task (RFC 0262).
//!
//! A Store app cannot add a scheduled task that points into its own folder, because every update
//! moves that folder. So the package declares a startup task, which Windows runs at each login,
//! and this program decides whether to act:
//! - **Asked for.** `ekos setup` writes `%LOCALAPPDATA%\EKOS.DEV\start-at-login` when the person
//!   chose "start the console at login". The file holds the EKOS home. `ekos uninstall` removes it.
//! - **Not asked for.** Without the file this program exits at once and starts nothing.
//! - **No window.** It runs `ekos.exe --home <home> console start` from beside itself, with no
//!   window, then exits.
//!
//! The path matches `MSIX_LOGIN_FILE` in `ekos/crates/cli/src/commands/setup.rs`. Only the standard
//! library is used, so the program stays small.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// `%LOCALAPPDATA%\EKOS.DEV\start-at-login`, under `local_appdata`.
fn login_file(local_appdata: &Path) -> PathBuf {
    local_appdata.join("EKOS.DEV").join("start-at-login")
}

/// The EKOS home the file names: its first non-empty line.
fn home_from(text: &str) -> Option<PathBuf> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
}

fn main() -> ExitCode {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return ExitCode::SUCCESS;
    };
    let Some(home) = std::fs::read_to_string(login_file(Path::new(&local)))
        .ok()
        .and_then(|t| home_from(&t))
    else {
        return ExitCode::SUCCESS;
    };
    let ekos = std::env::current_exe().ok().and_then(|e| {
        e.parent()
            .map(|d| d.join(if cfg!(windows) { "ekos.exe" } else { "ekos" }))
    });
    let Some(ekos) = ekos.filter(|p| p.is_file()) else {
        return ExitCode::FAILURE;
    };
    let mut cmd = Command::new(ekos);
    cmd.arg("--home")
        .arg(home)
        .args(["console", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_where_setup_writes_it() {
        assert_eq!(
            login_file(Path::new("L")),
            Path::new("L").join("EKOS.DEV").join("start-at-login")
        );
    }

    #[test]
    fn the_home_is_the_first_non_empty_line() {
        assert_eq!(
            home_from("\n  C:\\Users\\a\\.ekos  \r\n"),
            Some(PathBuf::from("C:\\Users\\a\\.ekos"))
        );
        assert_eq!(home_from("  \n"), None);
    }
}
