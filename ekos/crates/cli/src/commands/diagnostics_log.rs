//! Persists a `PassContext`'s collected diagnostics to a real file under `.ekos/diagnostics/`
//! (RFC 0076) — closes a real, confirmed gap found live: `ekos compile` printed
//! `Warnings: 28434 (check logs)` against a real project, but every one of those 28,434 warnings
//! only ever logged at `tracing::debug!`, invisible at this project's own default
//! `log-level = "info"`, and was never persisted anywhere a user could actually go look. "Check
//! logs" pointed nowhere real.

use ekos_compiler_core::pass::PassContext;
use std::path::{Path, PathBuf};

/// Writes every diagnostic collected in `ctx` to `.ekos/diagnostics/<command>.log` (plain text,
/// one line per diagnostic, overwritten each run — this is "what happened last run", not an
/// accumulating history no one would ever prune). Returns `None` (writes nothing, removing any
/// stale file from an earlier run) when there are no diagnostics at all, so a clean run never
/// leaves behind a log that would misleadingly suggest a past problem still applies.
pub fn write_diagnostics_log(
    ekos_dir: &Path,
    command: &str,
    ctx: &PassContext,
) -> anyhow::Result<Option<PathBuf>> {
    let sink = ctx.diagnostics.lock().unwrap();
    let dir = ekos_dir.join("diagnostics");
    let path = dir.join(format!("{command}.log"));

    if sink.diagnostics().is_empty() {
        // Best-effort: an earlier run's now-stale log shouldn't linger and look current.
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }

    std::fs::create_dir_all(&dir)?;
    let mut out = String::new();
    for d in sink.diagnostics() {
        out.push_str(&format!("[{:?}] {}: {}\n", d.severity, d.code, d.message));
    }
    std::fs::write(&path, out)?;
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn ctx() -> PassContext {
        PassContext::new(
            Arc::new(EkosConfig::default()),
            std::path::PathBuf::from("."),
        )
    }

    #[test]
    fn writes_a_real_file_with_every_diagnostic_when_any_exist() {
        let dir = tempdir().unwrap();
        let ctx = ctx();
        ctx.diagnostics.lock().unwrap().warning("W001", "first");
        ctx.diagnostics.lock().unwrap().error("E001", "second");

        let path = write_diagnostics_log(dir.path(), "compile", &ctx)
            .unwrap()
            .expect("diagnostics exist, a file must be written");
        assert_eq!(path, dir.path().join("diagnostics/compile.log"));

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("W001: first"));
        assert!(content.contains("E001: second"));
    }

    #[test]
    fn writes_nothing_and_returns_none_when_there_are_no_diagnostics() {
        let dir = tempdir().unwrap();
        let ctx = ctx();
        let result = write_diagnostics_log(dir.path(), "compile", &ctx).unwrap();
        assert!(result.is_none());
        assert!(!dir.path().join("diagnostics/compile.log").exists());
    }

    #[test]
    fn a_clean_re_run_removes_a_stale_log_from_an_earlier_run() {
        let dir = tempdir().unwrap();
        let with_warning = ctx();
        with_warning
            .diagnostics
            .lock()
            .unwrap()
            .warning("W001", "first run had a problem");
        write_diagnostics_log(dir.path(), "compile", &with_warning).unwrap();
        assert!(dir.path().join("diagnostics/compile.log").exists());

        let clean = ctx();
        let result = write_diagnostics_log(dir.path(), "compile", &clean).unwrap();
        assert!(result.is_none());
        assert!(
            !dir.path().join("diagnostics/compile.log").exists(),
            "a clean re-run must not leave a stale log implying the old problem still applies"
        );
    }
}
