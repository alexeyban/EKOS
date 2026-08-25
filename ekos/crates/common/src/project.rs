//! RFC 0079: the shared, tiny half of the multi-project path-collision fix — a recovery pass that
//! derives a deterministic `KirId` from a raw path string, and wants that id to stay
//! collision-safe across `[observe] paths` entries, qualifies the *hash input* (never the
//! displayed path/name — those must stay human-readable) with this function.
//!
//! `crates/cli/src/commands/build.rs` writes the matching `"project"` field onto every observed
//! artifact's `data` object (present whenever [`project_key_for_base`] returns non-empty for that
//! artifact's own `[observe] paths` entry — see that function's own doc comment for the exact
//! condition) at the single central choke point every connector's artifacts already pass through
//! for RFC 0043 redaction. Every downstream recovery pass that reads a `project` field back from
//! its own artifact's `data` and calls [`project_qualify`] gets the exact same qualification
//! convention `build.rs`'s own `File`-object ids already established (RFC 0044) —
//! `"{project}:{path}"`, or the bare path unchanged when there's no project to qualify with.
//!
//! **`path` must be relative to the same `[observe] paths` *entry* the `project` value came
//! from, not to the workspace root** — `build.rs`'s own `File` objects use `rel_str` (the
//! observer's `content.target`, base-relative: `"ai.py"`, not `"backend/app/api/ai.py"`) as
//! `path` here. A raw-content collection loop that instead strips only the workspace root
//! (`path.strip_prefix(cwd)`) produces a real, different id even when `project` itself is
//! correct — found live, 2026-08-24 (`dependency_analyzer.rs`, and `package_json_analyzer.rs`
//! despite its own doc comment's claim of matching `build.rs` exactly — never actually verified
//! against a real single-non-`"."`-entry workspace until this bug was found against one).

use std::path::Path;

/// Qualifies `path` for hashing when `project` is `Some` — `None`/absent reproduces the bare path
/// unchanged, so a single-project workspace's ids never change (no migration needed there, the
/// same guarantee RFC 0044's original `File`-object fix made).
pub fn project_qualify(path: &str, project: Option<&str>) -> String {
    match project {
        Some(p) if !p.is_empty() => format!("{p}:{path}"),
        _ => path.to_string(),
    }
}

/// The real qualifier for one `[observe] paths` entry (`base`), matching `build.rs`'s own
/// corrected rule exactly (fixed there 2026-08-23, RFC 0088's live verification): `base != cwd`,
/// not `observe_paths.len() > 1`. The entry-count check was wrong — a workspace with exactly
/// *one* `[observe] paths` entry that isn't `"."` (a real, common shape: `paths = ["src"]`, or a
/// single scoped subdirectory) still needs qualification, since `base` and `cwd` genuinely
/// differ; only the true `paths = ["."]` case (`base == cwd`) needs none, for byte-identical ids
/// on every already-existing single-project ledger.
///
/// Returns the empty string for "no qualification needed" (matching [`project_qualify`]'s own
/// `Some("")`/`None` equivalence) rather than `Option`, so every call site's existing
/// `if project_key.is_empty()` checks keep working unchanged.
pub fn project_key_for_base(base: &Path, cwd: &Path) -> String {
    if base != cwd {
        base.strip_prefix(cwd)
            .unwrap_or(base)
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_project_reproduces_the_bare_path_unchanged() {
        assert_eq!(project_qualify("src/main.rs", None), "src/main.rs");
        assert_eq!(project_qualify("src/main.rs", Some("")), "src/main.rs");
    }

    #[test]
    fn a_project_qualifies_the_path() {
        assert_eq!(
            project_qualify("src/main.rs", Some("service-a")),
            "service-a:src/main.rs"
        );
    }

    #[test]
    fn two_different_projects_with_the_same_relative_path_qualify_differently() {
        assert_ne!(
            project_qualify("src/main.rs", Some("service-a")),
            project_qualify("src/main.rs", Some("service-b")),
        );
    }

    #[test]
    fn dot_observe_path_needs_no_qualification() {
        let cwd = Path::new("/workspace");
        assert_eq!(project_key_for_base(Path::new("/workspace"), cwd), "");
    }

    #[test]
    fn a_single_non_dot_observe_path_still_needs_qualification() {
        // The real bug this function fixes: entry-count-based checks wrongly treated this case
        // (exactly one `[observe] paths` entry, but not `"."`) as needing none.
        let cwd = Path::new("/workspace");
        assert_eq!(
            project_key_for_base(Path::new("/workspace/backend/app/api"), cwd),
            "backend/app/api"
        );
    }
}
