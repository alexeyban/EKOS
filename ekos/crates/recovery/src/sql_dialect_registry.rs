//! SQL dialect registry (RFC 0031) — resolves which `SqlDialectParser` a given `.sql` file
//! should be parsed with, and holds the compiled-in set of dialect crates.
//!
//! A new dialect is added the same way a new `Observer` plugin is added elsewhere in this
//! codebase: write a crate implementing `SqlDialectParser`, add it as a workspace member +
//! dependency, add one line to [`build_dialect_registry`]. This is a compile-time registry, not
//! a dynamic-loading mechanism — see RFC 0031's Non-goals.

use std::collections::HashMap;

use ekos_plugin_sql_dialect_mysql::MySqlDialectParser;
use ekos_plugin_sql_dialect_postgres::PostgresDialectParser;
use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{Dialect, GenericDialect};

/// The ANSI-SQL baseline — "first, follow generic ANSI SQL rules." This is the same
/// `GenericDialect` `SqlAnalyzerPass`/`SqlTransformAnalyzerPass` always used before RFC 0031,
/// now made an explicit, named, registry entry rather than an unconditional hardcoded default.
pub struct GenericDialectParser;

impl SqlDialectParser for GenericDialectParser {
    fn name(&self) -> &str {
        "generic"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(GenericDialect {})
    }
}

/// One `[[recover.sql.dialect-rules]]` entry from `ekos.toml`: files whose path matches
/// `path_glob` are parsed with `dialect` (a registry key). Rules are checked in order; the
/// first match wins.
#[derive(Debug, Clone)]
pub struct DialectRule {
    pub path_glob: String,
    pub dialect: String,
}

/// Builds the compiled-in dialect registry. Registering a new dialect crate means adding one
/// line here (plus the workspace member + dependency) — see this module's top-level doc.
pub fn build_dialect_registry() -> HashMap<String, Box<dyn SqlDialectParser>> {
    let mut registry: HashMap<String, Box<dyn SqlDialectParser>> = HashMap::new();
    registry.insert("generic".to_string(), Box::new(GenericDialectParser));
    registry.insert("mysql".to_string(), Box::new(MySqlDialectParser));
    registry.insert("postgres".to_string(), Box::new(PostgresDialectParser));
    registry
}

/// Resolves the dialect for one file: the first `rule` whose `path_glob` matches `rel_path`
/// wins; otherwise falls back to `default_dialect`. Returns `None` only if the resolved name
/// isn't in `registry` (a config error — an unknown dialect name was configured), in which case
/// callers should fall back to `"generic"` explicitly rather than silently guessing.
pub fn resolve_dialect_name<'a>(
    rules: &'a [DialectRule],
    default_dialect: &'a str,
    rel_path: &str,
) -> &'a str {
    for rule in rules {
        match glob::Pattern::new(&rule.path_glob) {
            Ok(pattern) if pattern.matches(rel_path) => return rule.dialect.as_str(),
            Ok(_) => continue,
            Err(_) => continue, // malformed glob — skip rather than fail the whole recover run
        }
    }
    default_dialect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_generic_mysql_and_postgres() {
        let registry = build_dialect_registry();
        assert!(registry.contains_key("generic"));
        assert!(registry.contains_key("mysql"));
        assert!(registry.contains_key("postgres"));
        assert_eq!(registry.get("mysql").unwrap().name(), "mysql");
        assert_eq!(registry.get("postgres").unwrap().name(), "postgres");
        assert_eq!(registry.get("generic").unwrap().name(), "generic");
    }

    #[test]
    fn resolve_dialect_name_falls_back_to_default_with_no_rules() {
        let resolved = resolve_dialect_name(&[], "generic", "DB Scripts/anything.sql");
        assert_eq!(resolved, "generic");
    }

    #[test]
    fn resolve_dialect_name_matches_first_rule_by_path_glob() {
        let rules = vec![
            DialectRule {
                path_glob: "**/mysql/**/*.sql".to_string(),
                dialect: "mysql".to_string(),
            },
            DialectRule {
                path_glob: "**/postgres/**/*.sql".to_string(),
                dialect: "postgres".to_string(),
            },
        ];

        assert_eq!(
            resolve_dialect_name(&rules, "generic", "db/mysql/create.sql"),
            "mysql"
        );
        assert_eq!(
            resolve_dialect_name(&rules, "generic", "db/postgres/create.sql"),
            "postgres"
        );
        assert_eq!(
            resolve_dialect_name(&rules, "generic", "db/other/create.sql"),
            "generic"
        );
    }

    /// The real, motivating case from this session's testing: a single workspace with
    /// dialect-mixed folders (AdventureWorks' "Destination MySQL" vs "Source MSSQL").
    #[test]
    fn resolve_dialect_name_handles_mixed_dialect_workspace() {
        let rules = vec![
            DialectRule {
                path_glob: "*MySQL*/*.sql".to_string(),
                dialect: "mysql".to_string(),
            },
            DialectRule {
                path_glob: "*MSSQL*/*.sql".to_string(),
                dialect: "generic".to_string(),
            },
        ];
        assert_eq!(
            resolve_dialect_name(
                &rules,
                "generic",
                "Destination MySQL/create.eae_data_management_mmjja.sql"
            ),
            "mysql"
        );
        assert_eq!(
            resolve_dialect_name(
                &rules,
                "generic",
                "Source MSSQL/transactional.testing.scenarios.sql"
            ),
            "generic"
        );
    }
}
