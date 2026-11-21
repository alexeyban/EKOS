//! Shared by the gated fixture tests: a fixture's projects, read the way the observer reads them.

use std::path::Path;

use ekos_talend::{PROJECT_MARKER, Project, ProjectFile, Role, read_project, role};

/// Every project under `root`, read the way the observer reads it: files whose role is kept.
pub fn projects(root: &Path) -> Vec<Project> {
    let mut out = Vec::new();
    for marker in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            !ekos_talend::SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == PROJECT_MARKER)
    {
        let dir = marker.path().parent().unwrap();
        let files: Vec<ProjectFile> = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let rel = e
                    .path()
                    .strip_prefix(dir)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                if role(&rel) == Role::Other {
                    return None;
                }
                Some(ProjectFile {
                    text: std::fs::read_to_string(e.path()).ok()?,
                    path: rel,
                })
            })
            .collect();
        let rel_dir = dir
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        out.push(read_project(&rel_dir, &files));
    }
    out
}
