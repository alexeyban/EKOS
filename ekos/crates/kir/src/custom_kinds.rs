//! RFC 0135 Part D — the canonical registry of every `ObjectKind::Custom(_)` the compiler
//! pipeline (`recover` / `compile` / `commit`) emits, and whether each is *structurally keyed*.
//!
//! A **structurally-keyed** kind's every instance is self-identified by a structural key — a file
//! path, a manifest directory, a `(source, index)` pair, a `(subject, predicate, object)` triple.
//! No two distinct instances can ever be the same real-world entity, so identity resolution MUST
//! exclude them from merge candidacy: a shared name prefix plus `structural_score`'s same-kind
//! `1.0` fallback (no `columns` property to compare) otherwise collapses a whole book / crate
//! graph / module tree into one canonical object. That over-merge (RFC 0024 / 0027 / 0038 / 0041
//! / 0042 / 0081 / 0085) was hit and re-diagnosed live roughly a dozen times, each time a new
//! analyzer shipped without touching the hand-maintained exclusion list this module replaces.
//!
//! `identity::DefaultResolver` derives its exclusion set from `structurally_keyed == true` here —
//! there is no second list. A test in `ekos-identity` enumerates every `ObjectKind::Custom("…")`
//! string literal in `crates/recovery/src` and `crates/semantic/src` and asserts each has an
//! entry below, so a new kind fails CI rather than a generated entity page weeks later.

/// One row of [`REGISTRY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomKind {
    /// The exact string inside `ObjectKind::Custom(_)`.
    pub name: &'static str,
    /// `true` → every instance has a structural identity; exclude from merge candidacy.
    /// `false` → two instances can legitimately be the same entity; merging is allowed (usually
    /// with its own name/threshold guard).
    pub structurally_keyed: bool,
    /// The structural key (keyed kinds) or why merging is legitimate (the rest).
    pub note: &'static str,
}

/// Every `ObjectKind::Custom(_)` produced by a `recover` / `compile` / `commit` pass.
///
/// Simulation (`ekos_simulation`), EKL demo fixtures, and render-time throwaway objects are out
/// of scope — they never reach `DefaultResolver`.
pub const REGISTRY: &[CustomKind] = &[
    // ── structurally keyed → excluded from merge ──────────────────────────────
    CustomKind {
        name: "Section",
        structurally_keyed: true,
        note: "(document, page/chunk index) — RFC 0024; devlog 27's 8,624→120 over-merge",
    },
    CustomKind {
        name: "Page",
        structurally_keyed: true,
        note: "(Confluence space, page id) — confluence_analyzer; same shape as Section",
    },
    CustomKind {
        name: "Document",
        structurally_keyed: true,
        note: "RFC 0079-qualified path — localdocs/confluence; two real README.md exact-name merge",
    },
    CustomKind {
        name: "TransformNode",
        structurally_keyed: true,
        note: "(source path, node index) — RFC 0027; transform_ir::lower_to_kir",
    },
    CustomKind {
        name: "RustModule",
        structurally_keyed: true,
        note: "(file path, qualified name) — RFC 0041",
    },
    CustomKind {
        name: "RustSymbol",
        structurally_keyed: true,
        note: "(file path, qualified name) — RFC 0041",
    },
    CustomKind {
        name: "PythonModule",
        structurally_keyed: true,
        note: "(file path, qualified name) — RFC 0038/0040",
    },
    CustomKind {
        name: "PythonSymbol",
        structurally_keyed: true,
        note: "(file path, qualified name) — RFC 0038/0040",
    },
    CustomKind {
        name: "ElixirModule",
        structurally_keyed: true,
        note: "qualified module name — RFC 0081",
    },
    CustomKind {
        name: "ElixirSymbol",
        structurally_keyed: true,
        note: "(owning module id, qualified name) — RFC 0081",
    },
    CustomKind {
        name: "JsModule",
        structurally_keyed: true,
        note: "qualified module name — RFC 0085",
    },
    CustomKind {
        name: "JsSymbol",
        structurally_keyed: true,
        note: "(owning module id, qualified name) — RFC 0085",
    },
    CustomKind {
        name: "Crate",
        structurally_keyed: true,
        note: "manifest directory — RFC 0042; 39→1 over-merge on the ekos-* prefix",
    },
    CustomKind {
        name: "Claim",
        structurally_keyed: true,
        note: "(subject, predicate, object) triple — RFC 0065",
    },
    CustomKind {
        name: "ArchitectureGap",
        structurally_keyed: true,
        note: "(crate, unresolved dependency name) — RFC 0065",
    },
    CustomKind {
        name: "Risk",
        structurally_keyed: true,
        note: "the object it is a risk for — RFC 0094; one deterministic Risk per source object",
    },
    CustomKind {
        name: "Rollup",
        structurally_keyed: true,
        note: "directory path — RFC 0044; one hierarchical rollup per directory",
    },
    CustomKind {
        name: "ProjectSummary",
        structurally_keyed: true,
        note: "the workspace/project — RFC 0088; exactly one per project",
    },
    // ── legitimately mergeable → NOT excluded ─────────────────────────────────
    CustomKind {
        name: "Concept",
        structurally_keyed: false,
        note: "RFC 0026 — cross-document mentions of one concept should merge; guarded by \
               MIN_CONCEPT_NAME_WORDS/CHARS + the stricter CONCEPT_MERGE_THRESHOLD",
    },
    CustomKind {
        name: "Technology",
        structurally_keyed: false,
        note: "RFC 0042 — deterministic id from the technology name, so cross-analyzer instances \
               share an id and never reach the merge path at all",
    },
    CustomKind {
        name: "Issue",
        structurally_keyed: false,
        note: "RFC 0020 — name normalized to the bare title (strip `owner/repo#num: `); distinct \
               titles do not collide, identical ones legitimately merge",
    },
    CustomKind {
        name: "PullRequest",
        structurally_keyed: false,
        note: "RFC 0020 — see Issue",
    },
];

/// The [`CustomKind`] row for `name`, if it is a registered compiler-pipeline kind.
pub fn lookup(name: &str) -> Option<&'static CustomKind> {
    REGISTRY.iter().find(|k| k.name == name)
}

/// Whether `name` names a registered *structurally-keyed* `Custom` kind — the predicate
/// `DefaultResolver` uses to exclude a kind from merge candidacy.
///
/// An **unknown** kind returns `false` (it is not excluded). The `ekos-identity` coverage test
/// guarantees no `recover`/`compile`/`commit` kind is ever unknown, so in practice this is only
/// `false` for the genuinely-mergeable kinds and for out-of-scope kinds (simulation, fixtures)
/// that never reach the resolver.
pub fn is_structurally_keyed(name: &str) -> bool {
    lookup(name).is_some_and(|k| k.structurally_keyed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for k in REGISTRY {
            assert!(seen.insert(k.name), "duplicate registry entry: {}", k.name);
        }
    }

    #[test]
    fn the_historically_over_merged_kinds_are_all_keyed() {
        for name in [
            "Section",
            "TransformNode",
            "RustSymbol",
            "RustModule",
            "PythonSymbol",
            "PythonModule",
            "Crate",
            "Claim",
            "ArchitectureGap",
            "ElixirModule",
            "ElixirSymbol",
            "JsModule",
            "JsSymbol",
            "Document",
        ] {
            assert!(
                is_structurally_keyed(name),
                "{name} is a documented over-merge case and must stay excluded"
            );
        }
    }

    #[test]
    fn mergeable_kinds_are_not_keyed() {
        for name in ["Concept", "Technology", "Issue", "PullRequest"] {
            assert!(!is_structurally_keyed(name), "{name} must stay mergeable");
        }
        assert!(!is_structurally_keyed("NotAKindAtAll"));
    }
}
