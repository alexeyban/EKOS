//! Git repository observer plugin.
//!
//! Shells out to `git` (must be on PATH) and emits one `ObservationArtifact` per
//! commit plus a single repository-metadata artifact. No writes to the workspace.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use tokio::process::Command;

/// Runs `git` CLI commands against the workspace root.
///
/// Emits:
/// - One `ObservationArtifact` with `target = "repo"` carrying HEAD branch, remotes,
///   and contributors.
/// - One `ObservationArtifact` per commit (up to `max_commits`, default 500) carrying
///   SHA, author, date, message, and changed-file list.
pub struct GitObserver {
    /// Maximum number of commits to collect per run. Default: 500.
    pub max_commits: usize,
}

impl GitObserver {
    pub fn new() -> Self {
        Self { max_commits: 500 }
    }

    pub fn with_max_commits(mut self, n: usize) -> Self {
        self.max_commits = n;
        self
    }
}

impl Default for GitObserver {
    fn default() -> Self {
        Self::new()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// git helpers
// ────────────────────────────────────────────────────────────────────────────

async fn git_output(cwd: &std::path::Path, args: &[&str]) -> Result<String, ObserveError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| ObserveError::connector(format!("git exec error: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ObserveError::connector(format!(
            "git {}: {stderr}",
            args.first().unwrap_or(&"<?>")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn is_git_repo(cwd: &std::path::Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(cwd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ────────────────────────────────────────────────────────────────────────────
// Observer impl
// ────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl Observer for GitObserver {
    fn name(&self) -> &str {
        "git"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("git", &target);

        if !is_git_repo(root).await {
            tracing::debug!(
                "git observer: {} is not a git repo, skipping",
                root.display()
            );
            return Ok(pkg);
        }

        // ── Repository metadata artifact ──────────────────────────────────
        let head_branch = git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .await
            .unwrap_or_else(|_| "unknown".into())
            .trim()
            .to_string();

        let remotes_raw = git_output(root, &["remote", "-v"])
            .await
            .unwrap_or_default();
        let remotes: Vec<serde_json::Value> = remotes_raw
            .lines()
            .filter(|l| l.contains("(fetch)"))
            .map(|l| {
                let mut parts = l.split_whitespace();
                let name = parts.next().unwrap_or("").to_string();
                let url = parts.next().unwrap_or("").to_string();
                serde_json::json!({"name": name, "url": url})
            })
            .collect();

        // shortlog: "  <N>\t<Name>" per contributor
        let shortlog = git_output(root, &["shortlog", "-sn", "--no-merges", "HEAD"])
            .await
            .unwrap_or_default();
        let contributors: Vec<serde_json::Value> = shortlog
            .lines()
            .filter_map(|l| {
                let (count_str, name) = l.trim().split_once('\t')?;
                let commits: u64 = count_str.trim().parse().ok()?;
                Some(serde_json::json!({"name": name.trim(), "commits": commits}))
            })
            .collect();

        let repo_data = serde_json::json!({
            "head_branch": head_branch,
            "remotes": remotes,
            "contributors": contributors,
        });
        pkg.push(
            ObservationArtifact::new("git", "repo", repo_data).with_producer("ekos-plugin-git"),
        );

        // ── Per-commit artifacts ──────────────────────────────────────────
        // Format: SHA\x1FAuthor Name\x1FAuthor Email\x1FISO-date\x1FSubject
        let log_format = "%H\x1f%an\x1f%ae\x1f%aI\x1f%s";
        let log_raw = git_output(
            root,
            &[
                "log",
                &format!("--max-count={}", self.max_commits),
                &format!("--format={log_format}"),
                "HEAD",
            ],
        )
        .await
        .unwrap_or_default();

        for line in log_raw.lines() {
            let parts: Vec<&str> = line.splitn(5, '\x1f').collect();
            if parts.len() < 5 {
                continue;
            }
            let (sha, author_name, author_email, date, subject) =
                (parts[0], parts[1], parts[2], parts[3], parts[4]);

            // Collect changed files for this commit.
            let files_raw = git_output(
                root,
                &["diff-tree", "--no-commit-id", "-r", "--name-only", sha],
            )
            .await
            .unwrap_or_default();
            let files: Vec<&str> = files_raw.lines().collect();

            // Stat insertions/deletions.
            let stat_raw = git_output(root, &["show", "--stat", "--format=", sha])
                .await
                .unwrap_or_default();
            let (insertions, deletions) = parse_stat_summary(&stat_raw);

            let commit_data = serde_json::json!({
                "sha": sha,
                "author_name": author_name,
                "author_email": author_email,
                "date": date,
                "message": subject,
                "files_changed": files,
                "insertions": insertions,
                "deletions": deletions,
            });

            pkg.push(
                ObservationArtifact::new("git", sha, commit_data).with_producer("ekos-plugin-git"),
            );
        }

        Ok(pkg)
    }
}

/// Parse the last summary line of `git show --stat`: "3 files changed, 5 insertions(+), 2 deletions(-)"
fn parse_stat_summary(stat: &str) -> (u64, u64) {
    let last = stat.trim().lines().last().unwrap_or("");
    let ins = last
        .split(',')
        .find(|s| s.contains("insertion"))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(0);
    let del = last
        .split(',')
        .find(|s| s.contains("deletion"))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(0);
    (ins, del)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as SyncCommand;
    use tempfile::TempDir;

    /// Initialise a throwaway git repo with one commit so tests don't depend on the host repo.
    fn make_git_repo(dir: &TempDir) {
        let d = dir.path();
        let run = |args: &[&str]| {
            SyncCommand::new("git")
                .args(args)
                .current_dir(d)
                .output()
                .unwrap();
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(d.join("README.md"), b"# hello").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "initial commit"]);
    }

    #[tokio::test]
    async fn non_git_dir_returns_empty_package() {
        let dir = TempDir::new().unwrap();
        let ctx = ScanContext::new(dir.path());
        let pkg = GitObserver::new().scan(&ctx).await.unwrap();
        assert!(
            pkg.is_empty(),
            "non-git directory should produce no artifacts"
        );
    }

    #[tokio::test]
    async fn git_repo_produces_repo_and_commit_artifacts() {
        let dir = TempDir::new().unwrap();
        make_git_repo(&dir);
        let ctx = ScanContext::new(dir.path());
        let pkg = GitObserver::new().scan(&ctx).await.unwrap();
        // At minimum: 1 repo artifact + 1 commit artifact
        assert!(
            pkg.len() >= 2,
            "expected repo + commit artifact, got {}",
            pkg.len()
        );

        let repo_artifact = pkg.artifacts.iter().find(|a| a.content.target == "repo");
        assert!(
            repo_artifact.is_some(),
            "must have a 'repo' metadata artifact"
        );
        let repo_data = &repo_artifact.unwrap().content.data;
        assert_eq!(repo_data["head_branch"], "main");
    }

    #[tokio::test]
    async fn commit_artifacts_are_stable() {
        let dir = TempDir::new().unwrap();
        make_git_repo(&dir);
        let ctx = ScanContext::new(dir.path());
        let obs = GitObserver::new();
        let ids1: Vec<_> = obs
            .scan(&ctx)
            .await
            .unwrap()
            .artifacts
            .iter()
            .map(|a| a.id.clone())
            .collect();
        let ids2: Vec<_> = obs
            .scan(&ctx)
            .await
            .unwrap()
            .artifacts
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert_eq!(
            ids1, ids2,
            "same repo state must produce identical artifact IDs"
        );
    }

    #[test]
    fn parse_stat_extracts_numbers() {
        let stat = " 2 files changed, 7 insertions(+), 3 deletions(-)";
        assert_eq!(parse_stat_summary(stat), (7, 3));
    }

    #[test]
    fn parse_stat_insertions_only() {
        let stat = " 1 file changed, 2 insertions(+)";
        assert_eq!(parse_stat_summary(stat), (2, 0));
    }
}
