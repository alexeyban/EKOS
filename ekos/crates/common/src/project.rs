//! RFC 0079: the shared, tiny half of the multi-project path-collision fix — a recovery pass that
//! derives a deterministic `KirId` from a raw path string, and wants that id to stay
//! collision-safe across `[observe] paths` entries, qualifies the *hash input* (never the
//! displayed path/name — those must stay human-readable) with this function.
//!
//! `crates/cli/src/commands/build.rs` writes the matching `"project"` field onto every observed
//! artifact's `data` object (only present when `[observe] paths` has more than one entry — see
//! its own doc comment) at the single central choke point every connector's artifacts already
//! pass through for RFC 0043 redaction. Every downstream recovery pass that reads a `project`
//! field back from its own artifact's `data` and calls this function gets the exact same
//! qualification convention `build.rs`'s own `File`-object ids already established (RFC 0044) —
//! `"{project}:{path}"`, or the bare path unchanged when there's no project to qualify with.

/// Qualifies `path` for hashing when `project` is `Some` — `None`/absent reproduces the bare path
/// unchanged, so a single-project workspace's ids never change (no migration needed there, the
/// same guarantee RFC 0044's original `File`-object fix made).
pub fn project_qualify(path: &str, project: Option<&str>) -> String {
    match project {
        Some(p) if !p.is_empty() => format!("{p}:{path}"),
        _ => path.to_string(),
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
}
