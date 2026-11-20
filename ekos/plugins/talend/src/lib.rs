//! Talend project observer (RFC 0262 §Seams).
//!
//! A Talend project is a directory holding `talend.project`. For every project under the
//! workspace, the observer emits one artifact per file the model reads (jobs, their
//! `.properties`, contexts, connections, schemas, custom routines, `talend.project`), carrying the
//! raw text, the project root and the file's role. Talend's own SQL templates and system routines,
//! and items in folders v1 does not read, are recorded by path only, so the model can count them.
//! Generated code (`poms/`, `temp/`, `target/`) and screenshots are never captured.
//!
//! No XML is parsed here: the analyzer reads the artifacts into `ekos_talend::Project`. Secrets are
//! removed from the text by `ekos build`'s redaction pass (RFC 0043) before the artifacts are
//! stored, and the model never keeps a password value either way.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{
    ObservationPackage, ObserveError, Observer, ScanContext, walk_observed,
};
use ekos_talend::{PROJECT_MARKER, Role, SKIPPED_DIRS, role};
use sha2::{Digest, Sha256};

/// The connector name every Talend artifact carries.
pub const CONNECTOR: &str = "talend";

#[derive(Debug, Default)]
pub struct TalendObserver;

impl TalendObserver {
    pub fn new() -> Self {
        Self
    }
}

/// Project roots (workspace-relative, `""` for the workspace itself) among the observed paths.
fn project_roots(paths: &[String]) -> Vec<String> {
    let mut roots: Vec<String> = paths
        .iter()
        .filter_map(|p| {
            let root = if p == PROJECT_MARKER {
                ""
            } else {
                p.strip_suffix(&format!("/{PROJECT_MARKER}"))?
            };
            let generated = root.split('/').any(|part| SKIPPED_DIRS.contains(&part));
            (!generated).then(|| root.to_string())
        })
        .collect();
    // Deepest first, so a file goes to the innermost project holding it.
    roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
    roots
}

/// The project holding `path`, and `path` relative to it.
fn locate<'a>(roots: &'a [String], path: &'a str) -> Option<(&'a str, &'a str)> {
    roots.iter().find_map(|root| {
        let rel = if root.is_empty() {
            path
        } else {
            path.strip_prefix(root.as_str())?.strip_prefix('/')?
        };
        Some((root.as_str(), rel))
    })
}

fn sha256(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

#[async_trait]
impl Observer for TalendObserver {
    fn name(&self) -> &str {
        CONNECTOR
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let mut pkg = ObservationPackage::new(CONNECTOR, ctx.workspace_root.display().to_string());
        let mut paths: Vec<String> = Vec::new();
        walk_observed(ctx, |rel, _| paths.push(rel.to_string()), |_| {});
        paths.sort();
        let roots = project_roots(&paths);
        if roots.is_empty() {
            return Ok(pkg);
        }
        for path in &paths {
            let Some((root, rel)) = locate(&roots, path) else {
                continue;
            };
            let file_role = role(rel);
            if file_role == Role::Other {
                continue;
            }
            let text = if file_role.keeps_text() {
                match std::fs::read_to_string(ctx.workspace_root.join(path)) {
                    Ok(t) => Some(t),
                    Err(err) => {
                        tracing::warn!("talend observer: cannot read {path}: {err}");
                        pkg.meta.error_count += 1;
                        continue;
                    }
                }
            } else {
                None
            };
            let data = serde_json::json!({
                "path": path,
                "project_root": root,
                "project_path": rel,
                "role": file_role.as_str(),
                "size_bytes": text.as_ref().map_or(0, String::len),
                "content_sha256": text.as_deref().map(sha256),
                "text": text,
            });
            pkg.push(
                ObservationArtifact::new(CONNECTOR, path, data).with_producer("ekos-plugin-talend"),
            );
        }
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, text: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn roles(pkg: &ObservationPackage) -> Vec<(String, String, bool)> {
        let mut out: Vec<_> = pkg
            .artifacts
            .iter()
            .map(|a| {
                let d = &a.content.data;
                (
                    d["path"].as_str().unwrap().to_string(),
                    d["role"].as_str().unwrap().to_string(),
                    d["text"].is_string(),
                )
            })
            .collect();
        out.sort();
        out
    }

    #[tokio::test]
    async fn a_project_is_captured_by_role_and_generated_code_is_not() {
        let dir = TempDir::new().unwrap();
        let p = "etl/DEMO";
        write(dir.path(), &format!("{p}/talend.project"), "<x/>");
        write(dir.path(), &format!("{p}/process/ODS/j_0.1.item"), "<j/>");
        write(
            dir.path(),
            &format!("{p}/process/ODS/j_0.1.properties"),
            "<p/>",
        );
        write(
            dir.path(),
            &format!("{p}/process/ODS/j_0.1.screenshot"),
            "png",
        );
        write(
            dir.path(),
            &format!("{p}/code/routines/system/Numeric_0.1.item"),
            "java",
        );
        write(
            dir.path(),
            &format!("{p}/poms/jobs/process/j/src/Main.java"),
            "class X {}",
        );
        write(dir.path(), &format!("{p}/poms/jobs/talend.project"), "<x/>");
        write(dir.path(), "readme.md", "not talend");

        let pkg = TalendObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert_eq!(
            roles(&pkg),
            vec![
                (
                    format!("{p}/code/routines/system/Numeric_0.1.item"),
                    "system_routine".into(),
                    false
                ),
                (format!("{p}/process/ODS/j_0.1.item"), "job".into(), true),
                (
                    format!("{p}/process/ODS/j_0.1.properties"),
                    "properties".into(),
                    true
                ),
                (format!("{p}/talend.project"), "project".into(), true),
            ]
        );
        let job = pkg
            .artifacts
            .iter()
            .find(|a| a.content.data["role"] == "job")
            .unwrap();
        assert_eq!(job.content.data["project_root"], p);
        assert_eq!(job.content.data["project_path"], "process/ODS/j_0.1.item");
    }

    #[tokio::test]
    async fn a_workspace_without_a_talend_project_yields_nothing() {
        let dir = TempDir::new().unwrap();
        // An `.item` file outside any project is not Talend's to claim.
        write(dir.path(), "process/x_0.1.item", "<j/>");
        let pkg = TalendObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn the_workspace_itself_can_be_the_project() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "talend.project", "<x/>");
        write(dir.path(), "context/c_0.1.item", "<c/>");
        let pkg = TalendObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert_eq!(pkg.artifacts.len(), 2);
        assert!(
            pkg.artifacts
                .iter()
                .all(|a| a.content.data["project_root"] == "")
        );
    }

    #[test]
    fn the_innermost_project_wins() {
        let roots = project_roots(&["a/talend.project".into(), "a/b/talend.project".into()]);
        assert_eq!(
            locate(&roots, "a/b/process/x_0.1.item"),
            Some(("a/b", "process/x_0.1.item"))
        );
        assert_eq!(
            locate(&roots, "a/process/x_0.1.item"),
            Some(("a", "process/x_0.1.item"))
        );
        assert_eq!(locate(&roots, "ab/process/x_0.1.item"), None);
    }
}
