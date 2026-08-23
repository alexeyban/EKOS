//! Convention-based Backend/Frontend/Database classification (RFC 0083, Phase 3 of the
//! source-decomposition plan) — the small, real, evidence-adjacent heuristic behind `## System
//! Decomposition`'s layer grouping. Deliberately not a new KIR kind or a compiler pass: the C4
//! Container-level question "which real layer does this file belong to" is answered from
//! structure already present (a `File` object's own path), the same "derive from what's already
//! compiled, don't invent new storage" principle `data_domains_section`'s schema-qualifier
//! grouping already established (RFC 0075).
//!
//! A convention is a guess, and a wrong guess with no escape hatch is worse than no guess — every
//! workspace can override the convention per-path via `[[architecture.system-decomposition.overrides]]`
//! in `ekos.toml`, checked first, same "first match wins" pattern
//! `sql_dialect_registry::resolve_dialect_name`'s `[[recover.sql.dialect-rules]]` already
//! established for exactly this shape of problem.

/// The three real layers `## System Decomposition` groups compiled objects into. `Database` is
/// never assigned by [`classify_path`] — real `Table` objects are already unambiguously database
/// objects by their own `ObjectKind`, no path heuristic needed; it exists here only so an
/// `ekos.toml` override can explicitly route a path (e.g. a `.sql` migrations directory) into the
/// same layer label for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Backend,
    Frontend,
    Database,
}

impl Layer {
    pub fn label(&self) -> &'static str {
        match self {
            Layer::Backend => "Backend",
            Layer::Frontend => "Frontend",
            Layer::Database => "Database",
        }
    }

    fn parse(s: &str) -> Option<Layer> {
        match s.to_ascii_lowercase().as_str() {
            "backend" => Some(Layer::Backend),
            "frontend" => Some(Layer::Frontend),
            "database" => Some(Layer::Database),
            _ => None,
        }
    }
}

/// One `[[architecture.system-decomposition.overrides]]` entry from `ekos.toml`.
#[derive(Debug, Clone)]
pub struct LayerOverride {
    pub path_glob: String,
    pub layer: String,
}

/// Server-side language extensions — real `Custom("*Module")`/`Custom("*Symbol")` analyzers exist
/// today for `ex`/`exs` (RFC 0081) and `rs`/`py` (pre-existing); the rest have no real EKOS
/// decomposition analyzer yet, but a `File` object with one of these extensions is still real,
/// evidenced backend source, worth counting honestly even before its own analyzer ships.
const BACKEND_EXTENSIONS: &[&str] = &[
    "ex", "exs", "rs", "py", "java", "kt", "go", "rb", "erl", "php", "cs",
];

/// Client-side language/asset extensions — `package.json` (RFC 0082) already gives real npm
/// `Technology` data for this layer; per-file decomposition is Phase 5's job
/// (`javascript_analyzer.rs`), not this classifier's.
const FRONTEND_EXTENSIONS: &[&str] = &[
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "vue", "svelte", "css", "scss", "sass", "less", "html",
];

/// Classifies one real compiled `File` object's path into a real layer, or `None` when neither
/// an override nor an extension gives a real signal — never guessed, an ambiguous/unknown
/// extension (`.md`, `.json`, `.toml`, `.yml`, ...) is honestly excluded from every layer rather
/// than forced into one. Overrides are checked first and win outright, same precedence
/// `resolve_dialect_name` already established for the equivalent SQL-dialect problem.
pub fn classify_path(path: &str, overrides: &[LayerOverride]) -> Option<Layer> {
    for o in overrides {
        match glob::Pattern::new(&o.path_glob) {
            Ok(pattern) if pattern.matches(path) => return Layer::parse(&o.layer),
            _ => continue, // no match, or a malformed glob — skip rather than fail the whole render
        }
    }

    if path.rsplit('/').next() == Some("package.json") {
        return Some(Layer::Frontend);
    }

    let ext = path.rsplit('.').next().unwrap_or("");
    if BACKEND_EXTENSIONS.contains(&ext) {
        return Some(Layer::Backend);
    }
    if FRONTEND_EXTENSIONS.contains(&ext) {
        return Some(Layer::Frontend);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_real_backend_and_frontend_extensions() {
        assert_eq!(
            classify_path("lib/plausible/auth.ex", &[]),
            Some(Layer::Backend)
        );
        assert_eq!(
            classify_path("crates/cli/src/main.rs", &[]),
            Some(Layer::Backend)
        );
        assert_eq!(
            classify_path("assets/js/components/Dashboard.tsx", &[]),
            Some(Layer::Frontend)
        );
        assert_eq!(
            classify_path("assets/css/dashboard.scss", &[]),
            Some(Layer::Frontend)
        );
    }

    #[test]
    fn package_json_is_always_a_real_frontend_signal() {
        assert_eq!(
            classify_path("tracker/package.json", &[]),
            Some(Layer::Frontend)
        );
    }

    #[test]
    fn an_ambiguous_extension_is_honestly_unclassified_not_guessed() {
        assert_eq!(classify_path("README.md", &[]), None);
        assert_eq!(classify_path("ekos.toml", &[]), None);
        assert_eq!(classify_path("data/schema.json", &[]), None);
    }

    #[test]
    fn an_override_wins_over_the_extension_convention() {
        let overrides = vec![LayerOverride {
            path_glob: "vendor/**/*.rs".to_string(),
            layer: "frontend".to_string(),
        }];
        assert_eq!(
            classify_path("vendor/embedded/widget.rs", &overrides),
            Some(Layer::Frontend),
            "an .rs file would normally be Backend, but the override must win"
        );
        assert_eq!(
            classify_path("crates/cli/src/main.rs", &overrides),
            Some(Layer::Backend),
            "a path the override glob doesn't match still falls through to the convention"
        );
    }

    #[test]
    fn a_malformed_override_glob_is_skipped_not_fatal() {
        let overrides = vec![LayerOverride {
            path_glob: "[".to_string(),
            layer: "frontend".to_string(),
        }];
        assert_eq!(
            classify_path("lib/plausible/auth.ex", &overrides),
            Some(Layer::Backend),
            "a broken glob must not crash classification — falls through to the real convention"
        );
    }
}
