use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct EkosConfig {
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub observe: ObserveConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub document_semantics: DocumentSemanticsConfig,
    #[serde(default)]
    pub architecture_reasoning: ArchitectureReasoningConfig,
    #[serde(default)]
    pub llm_description: LlmDescriptionConfig,
    #[serde(default)]
    pub marketing: MarketingConfig,
    #[serde(default)]
    pub recover: RecoverConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub clickhouse: ClickHouseConfig,
    #[serde(default)]
    pub architecture: ArchitectureConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspaceConfig {
    #[serde(default = "default_root")]
    pub root: PathBuf,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}
fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "pretty".into()
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root: default_root(),
            log_level: default_log_level(),
            log_format: default_log_format(),
        }
    }
}

fn default_ignore_patterns() -> Vec<String> {
    [".ekos", ".git", "target", "node_modules"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ObserveConfig {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default = "default_ignore_patterns")]
    pub ignore_patterns: Vec<String>,
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            ignore_patterns: default_ignore_patterns(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub api_key_env: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AiConfig {
    pub model: Option<String>,
    pub max_matches: Option<u32>,
    pub neighborhood_depth: Option<u32>,
    pub max_tokens: Option<u32>,
    pub system_prompt: Option<String>,
    pub max_context_chars: Option<u32>,
}

/// Gating for RFC 0026's `DocumentSemanticsAnalyzerPass`. Opt-in because the
/// pass makes one LLM call per document section — thousands for a large corpus.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DocumentSemanticsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Safety valve for "opted in, then ran against a huge corpus by accident".
    pub max_sections: Option<u32>,
}

/// Gating for RFC 0065 Phase 2's `ArchitectureReasoningPass`. Opt-in, same reasoning as
/// `DocumentSemanticsConfig`: one batched LLM call per `recover` run (all crates in one prompt,
/// not one call per crate — RFC 0065 §42's cost discipline), but still a real network call a
/// workspace shouldn't pay for unless it asked to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ArchitectureReasoningConfig {
    #[serde(default)]
    pub enabled: bool,
}

/// Gating for RFC 0088's `describe_objects` post-`commit` step. Opt-in, same reasoning as
/// `ArchitectureReasoningConfig`: a real, potentially large LLM spend (~900 calls at the default
/// `scope = "modules"` against a real mid-size codebase, ~5x that at `scope = "all"`) a workspace
/// shouldn't pay for unless it asked to. `scope` defaults to the cheaper tier specifically so
/// enabling this once doesn't silently commit a user to the larger `"all"` spend without a second,
/// explicit choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LlmDescriptionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub scope: DescriptionScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DescriptionScope {
    /// `Module`/`Rollup`/`Crate` objects only — the cheaper, default tier.
    #[default]
    Modules,
    /// `Symbol` objects only (function/method-level) — no module-level overviews.
    Symbols,
    /// Both modules and symbols — the full, most expensive tier.
    All,
}

/// RFC 0027: marketing-agent config. `[marketing]` in `ekos.toml`, replacing the source
/// design doc's standalone `marketing/config.yaml` — this repo has exactly one config file
/// and one format, and this follows the same opt-in-table pattern as `[document-semantics]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MarketingConfig {
    /// Project GitHub URL, always included in generated tweets.
    #[serde(default = "default_github")]
    pub github: String,
    /// Hashtags offered to the tweet-generation prompt (at most 3 are used).
    #[serde(default = "default_hashtags")]
    pub hashtags: Vec<String>,
    #[serde(default)]
    pub twitter: TwitterConfig,
}

fn default_github() -> String {
    "https://github.com/alexeyban/EKOS".to_string()
}

fn default_hashtags() -> Vec<String> {
    ["Rust", "AI", "MCP"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl Default for MarketingConfig {
    fn default() -> Self {
        Self {
            github: default_github(),
            hashtags: default_hashtags(),
            twitter: TwitterConfig::default(),
        }
    }
}

/// RFC 0031: pluggable SQL dialect selection. `[recover.sql]` in `ekos.toml`. Omitting the
/// section entirely preserves pre-RFC-0031 behavior exactly — every `.sql` file parsed with
/// the ANSI/`GenericDialect` baseline, no per-path rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RecoverConfig {
    #[serde(default)]
    pub sql: SqlRecoverConfig,
}

fn default_sql_dialect() -> String {
    "generic".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SqlRecoverConfig {
    /// The ANSI-SQL baseline fallback — "first, follow generic ANSI SQL rules." Used for any
    /// `.sql` file that no `dialect-rules` entry matches.
    #[serde(default = "default_sql_dialect")]
    pub default_dialect: String,
    /// Checked in order; the first `path-glob` match wins. A real workspace can mix dialects
    /// by folder (e.g. a `Destination MySQL/` vs. `Source MSSQL/` split) — a single global
    /// dialect setting can't express that.
    #[serde(default)]
    pub dialect_rules: Vec<SqlDialectRuleConfig>,
}

impl Default for SqlRecoverConfig {
    fn default() -> Self {
        Self {
            default_dialect: default_sql_dialect(),
            dialect_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SqlDialectRuleConfig {
    pub path_glob: String,
    pub dialect: String,
}

/// RFC 0043: additive-only extension of the built-in secrets/PII redaction baseline
/// (`ekos_common::redaction`). `[security]` in `ekos.toml`. Deliberately no `enabled` flag — the
/// built-in baseline (AWS/GitHub/Slack/... token shapes, PEM key blocks, `.env`/`*.pem`/... file
/// exclusion) always runs; this section can only add patterns/exclusions on top of it, matching
/// the "global limitation, not an opt-in feature" requirement behind this RFC.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SecurityConfig {
    #[serde(default)]
    pub extra_patterns: Vec<SecretPatternConfig>,
    #[serde(default)]
    pub extra_excluded_globs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SecretPatternConfig {
    pub label: String,
    pub regex: String,
}

/// Publishing is off by default — `ekos marketing publish` without `enabled = true` always
/// behaves as `--dry-run`, so opting in requires an explicit, visible config change.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TwitterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub dry_run: bool,
}

/// RFC 0056: gating for the live ClickHouse NL-to-SQL query engine's MCP exposure. `[clickhouse]`
/// in `ekos.toml`. Off by default — omitting the section (or `enable-mcp-query`) means `ekos mcp
/// serve` never advertises the `ekos_clickhouse_query` tool, even though `ekos clickhouse ask`
/// always works from the CLI regardless of this flag. Every existing MCP tool reads only the
/// local ledger; this one hits a live external system, so — unlike `[document-semantics]`'s
/// LLM-cost gate — this flag exists purely to control blast radius to connected AI agents, not
/// cost.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ClickHouseConfig {
    #[serde(default)]
    pub enable_mcp_query: bool,
}

/// RFC 0083 (System Decomposition, Phase 3): `[architecture.system-decomposition]` in
/// `ekos.toml`. A convention-based path→layer classifier ([`ekos_docs_gen::classify_path`]) needs
/// a real escape hatch for the workspaces it guesses wrong on — same shape and same
/// first-match-wins precedence `[recover.sql.dialect-rules]` already established for the
/// equivalent SQL-dialect problem (`SqlRecoverConfig`/`SqlDialectRuleConfig` above).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ArchitectureConfig {
    #[serde(default)]
    pub system_decomposition: SystemDecompositionConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SystemDecompositionConfig {
    /// Checked in order; the first `path-glob` match wins over the built-in extension
    /// convention. `layer` is one of `backend`/`frontend`/`database` (case-insensitive).
    #[serde(default)]
    pub overrides: Vec<LayerOverrideConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LayerOverrideConfig {
    pub path_glob: String,
    pub layer: String,
}

/// RFC 0111 groundwork (`[storage]` in `ekos.toml`): a config-driven mapping from a named
/// "storage container" to an arbitrary local folder. Lets a workspace's whole `.ekos` tree be
/// redirected to a configured folder instead of the default `<cwd>/.ekos` — the mechanism used to
/// simulate RFC 0111's distributed storage containers (S3/ADLS buckets, eventually) locally today:
/// point two `ekos.toml`s at two different container folders and each behaves as an independent
/// storage location, provable without any cloud dependency. Same shape as `[architecture.
/// system-decomposition]`'s `overrides` list (`SystemDecompositionConfig`, above) — a Vec of named
/// entries, first/only match by name wins, nothing fancier.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StorageConfig {
    /// Name of the container this workspace's `.ekos` tree resolves into. `None` (the default)
    /// keeps today's behavior — `<cwd>/.ekos` — completely unaffected; existing workspaces with
    /// no `[storage]` section see no change at all.
    #[serde(default)]
    pub active_container: Option<String>,
    #[serde(default)]
    pub containers: Vec<StorageContainerConfig>,
    /// RFC 0111 §1 — the partition model (`[storage.partition]`). Defaults to "no partitioning"
    /// (`dimension` unset), which is today's single-`FactLedger` behavior, unaffected.
    #[serde(default)]
    pub partition: StoragePartitionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StorageContainerConfig {
    pub name: String,
    pub path: PathBuf,
}

/// RFC 0111 §1's `[storage.partition]` block. Held here as plain strings — `compiler-core` does
/// not depend on `ekos-ledger`, so `ekos_ledger::{PartitionDimension, TimeBucket}` translation is
/// the `cli` layer's job (the same split CLAUDE.md documents for `ArchitectureConfidence`). Time
/// bucket has a global default plus per-scope glob overrides, reusing the first-match-wins
/// `[[recover.sql.dialect-rules]]` shape rather than inventing a second override convention.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StoragePartitionConfig {
    /// `"source-scope"` | `"entity-kind"` | `"composite"`. `None` = no partitioning (default).
    #[serde(default)]
    pub dimension: Option<String>,
    /// Global default time bucket: `"daily"` | `"weekly"` | `"monthly"`. `None` → `"monthly"`.
    #[serde(default)]
    pub time_bucket: Option<String>,
    /// Checked in order; first `scope-glob` match wins over `time_bucket` for that scope.
    #[serde(default)]
    pub time_bucket_overrides: Vec<TimeBucketOverrideConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TimeBucketOverrideConfig {
    pub scope_glob: String,
    pub time_bucket: String,
}

impl StoragePartitionConfig {
    /// Whether `[storage.partition]` actually asks for partitioning. `false` (the default) means
    /// the single-`FactLedger` path is unchanged.
    pub fn is_enabled(&self) -> bool {
        self.dimension.is_some()
    }

    /// The global default time bucket string, `"monthly"` when unset. Per-scope
    /// `time_bucket_overrides` resolution (needs a glob matcher) is the wiring layer's job, not
    /// this data-only config struct's.
    pub fn default_time_bucket(&self) -> &str {
        self.time_bucket.as_deref().unwrap_or("monthly")
    }
}

impl StorageConfig {
    /// The active container's config, if `active-container` names one that actually exists in
    /// `containers`. A name set but not found logs a warning and falls back to `None` (→ default
    /// `<cwd>/.ekos` behavior) rather than erroring — consistent with `from_file_or_default`'s
    /// existing graceful-fallback convention for this crate's config loading.
    pub fn active_container(&self) -> Option<&StorageContainerConfig> {
        let name = self.active_container.as_ref()?;
        let found = self.containers.iter().find(|c| &c.name == name);
        if found.is_none() {
            tracing::warn!(
                "storage.active-container = \"{name}\" does not match any storage.containers \
                 entry; falling back to the default <workspace>/.ekos location"
            );
        }
        found
    }
}

#[allow(clippy::derivable_impls)]
impl Default for EkosConfig {
    fn default() -> Self {
        Self {
            workspace: WorkspaceConfig::default(),
            observe: ObserveConfig::default(),
            llm: LlmConfig::default(),
            ai: AiConfig::default(),
            document_semantics: DocumentSemanticsConfig::default(),
            architecture_reasoning: ArchitectureReasoningConfig::default(),
            llm_description: LlmDescriptionConfig::default(),
            marketing: MarketingConfig::default(),
            recover: RecoverConfig::default(),
            security: SecurityConfig::default(),
            clickhouse: ClickHouseConfig::default(),
            architecture: ArchitectureConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}

impl EkosConfig {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
        let config: Self =
            toml::from_str(&content).map_err(|e| anyhow::anyhow!("invalid ekos.toml: {}", e))?;
        Ok(config)
    }

    pub fn from_file_or_default(path: &Path) -> Self {
        if path.exists() {
            match Self::from_file(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("failed to load config, using defaults: {e}");
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }

    /// Absolute path to the .ekos/ metadata directory — the whole artifact/ledger/facts tree
    /// hangs off this one path. RFC 0111 groundwork: when `[storage] active-container` names a
    /// configured container, that container's folder is used directly as this root instead of
    /// `<cwd>/.ekos`, so the entire storage tree (artifacts, SQLite ledger or fact-engine facts)
    /// lives under the configured container folder. No `[storage]` section, or an unmatched
    /// `active-container`, behaves exactly as before this field existed.
    pub fn ekos_dir(&self, cwd: &Path) -> PathBuf {
        if let Some(container) = self.storage.active_container() {
            return container.path.clone();
        }
        cwd.join(".ekos")
    }

    /// RFC 0043: the `[security]` config translated into the additive-only
    /// `ekos_common::redaction::RedactionConfig` every raw-content entry point
    /// (`build.rs`, `recover.rs`) checks before persisting anything.
    pub fn redaction_config(&self) -> ekos_common::redaction::RedactionConfig {
        ekos_common::redaction::RedactionConfig {
            extra_patterns: self
                .security
                .extra_patterns
                .iter()
                .map(|p| (p.label.clone(), p.regex.clone()))
                .collect(),
            extra_excluded_globs: self.security.extra_excluded_globs.clone(),
        }
    }

    /// Absolute path to the artifact cache.
    pub fn artifact_dir(&self, cwd: &Path) -> PathBuf {
        self.ekos_dir(cwd).join("artifacts")
    }

    /// Absolute path to the directory holding the main ledger and any branches.
    pub fn ledger_dir(&self, cwd: &Path) -> PathBuf {
        self.ekos_dir(cwd).join("ledger")
    }

    /// Absolute path to the ledger database.
    pub fn ledger_path(&self, cwd: &Path) -> PathBuf {
        self.ledger_dir(cwd).join("ledger.db")
    }

    /// Absolute path to a named branch's ledger database (Phase 13).
    pub fn branch_ledger_path(&self, cwd: &Path, name: &str) -> PathBuf {
        self.ledger_dir(cwd).join(format!("{name}.db"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[workspace]
root = "/srv/enterprise"
log-level = "debug"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.workspace.root, PathBuf::from("/srv/enterprise"));
        assert_eq!(cfg.workspace.log_level, "debug");
    }

    #[test]
    fn default_config_is_valid() {
        let cfg = EkosConfig::default();
        assert_eq!(cfg.workspace.log_level, "info");
        assert!(!cfg.observe.ignore_patterns.is_empty());
    }

    /// RFC 0026: document-semantics extraction is opt-in, so a config that never
    /// mentions it must leave the pass disabled.
    #[test]
    fn document_semantics_defaults_to_disabled() {
        assert!(!EkosConfig::default().document_semantics.enabled);
        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert!(!cfg.document_semantics.enabled);
        assert!(cfg.document_semantics.max_sections.is_none());
    }

    #[test]
    fn document_semantics_parses_from_kebab_case_table() {
        let toml = r#"
[document-semantics]
enabled = true
max-sections = 500
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert!(cfg.document_semantics.enabled);
        assert_eq!(cfg.document_semantics.max_sections, Some(500));
    }

    /// RFC 0088: LLM-backed compile-time descriptions are opt-in, and default to the cheaper
    /// `"modules"` scope rather than `"all"` even once enabled — the whole point being that
    /// turning this on once must never silently commit a workspace to the ~5x larger per-symbol
    /// spend without a second, explicit `scope = "all"` choice.
    #[test]
    fn llm_description_defaults_to_disabled_at_modules_scope() {
        let cfg = EkosConfig::default();
        assert!(!cfg.llm_description.enabled);
        assert_eq!(cfg.llm_description.scope, DescriptionScope::Modules);
        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert!(!cfg.llm_description.enabled);
        assert_eq!(cfg.llm_description.scope, DescriptionScope::Modules);
    }

    #[test]
    fn llm_description_parses_from_kebab_case_table() {
        let toml = r#"
[llm-description]
enabled = true
scope = "all"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert!(cfg.llm_description.enabled);
        assert_eq!(cfg.llm_description.scope, DescriptionScope::All);
    }

    /// RFC 0027: publishing is off unless a config explicitly opts in, even if the
    /// `[marketing]` table is entirely absent from `ekos.toml`.
    #[test]
    fn marketing_defaults_to_disabled_with_sensible_defaults() {
        let cfg = EkosConfig::default();
        assert!(!cfg.marketing.twitter.enabled);
        assert!(!cfg.marketing.twitter.dry_run);
        assert_eq!(cfg.marketing.github, "https://github.com/alexeyban/EKOS");
        assert_eq!(cfg.marketing.hashtags, vec!["Rust", "AI", "MCP"]);

        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert!(!cfg.marketing.twitter.enabled);
    }

    /// RFC 0031: omitting `[recover.sql]` entirely must preserve pre-RFC-0031 behavior —
    /// every `.sql` file parsed as generic/ANSI, no dialect rules.
    #[test]
    fn sql_recover_defaults_to_generic_with_no_rules() {
        let cfg = EkosConfig::default();
        assert_eq!(cfg.recover.sql.default_dialect, "generic");
        assert!(cfg.recover.sql.dialect_rules.is_empty());

        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert_eq!(cfg.recover.sql.default_dialect, "generic");
        assert!(cfg.recover.sql.dialect_rules.is_empty());
    }

    #[test]
    fn sql_recover_parses_dialect_rules_from_kebab_case_table() {
        let toml = r#"
[recover.sql]
default-dialect = "generic"

[[recover.sql.dialect-rules]]
path-glob = "**/mysql/**/*.sql"
dialect = "mysql"

[[recover.sql.dialect-rules]]
path-glob = "**/postgres/**/*.sql"
dialect = "postgres"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.recover.sql.default_dialect, "generic");
        assert_eq!(cfg.recover.sql.dialect_rules.len(), 2);
        assert_eq!(
            cfg.recover.sql.dialect_rules[0].path_glob,
            "**/mysql/**/*.sql"
        );
        assert_eq!(cfg.recover.sql.dialect_rules[0].dialect, "mysql");
        assert_eq!(cfg.recover.sql.dialect_rules[1].dialect, "postgres");
    }

    /// RFC 0056: MCP exposure of the live ClickHouse query tool is off unless a config
    /// explicitly opts in, even if the `[clickhouse]` table is entirely absent from `ekos.toml`.
    #[test]
    fn clickhouse_mcp_query_defaults_to_disabled() {
        assert!(!EkosConfig::default().clickhouse.enable_mcp_query);
        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert!(!cfg.clickhouse.enable_mcp_query);
    }

    #[test]
    fn clickhouse_mcp_query_parses_from_kebab_case_table() {
        let cfg: EkosConfig = toml::from_str("[clickhouse]\nenable-mcp-query = true\n").unwrap();
        assert!(cfg.clickhouse.enable_mcp_query);
    }

    /// RFC 0083: omitting `[architecture.system-decomposition]` entirely must preserve the pure
    /// convention-based behavior — no overrides, matching every other opt-in-table pattern above.
    #[test]
    fn system_decomposition_defaults_to_no_overrides() {
        let cfg = EkosConfig::default();
        assert!(cfg.architecture.system_decomposition.overrides.is_empty());

        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert!(cfg.architecture.system_decomposition.overrides.is_empty());
    }

    #[test]
    fn system_decomposition_parses_overrides_from_kebab_case_table() {
        let toml = r#"
[[architecture.system-decomposition.overrides]]
path-glob = "vendor/**/*.rs"
layer = "frontend"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.architecture.system_decomposition.overrides.len(), 1);
        assert_eq!(
            cfg.architecture.system_decomposition.overrides[0].path_glob,
            "vendor/**/*.rs"
        );
        assert_eq!(
            cfg.architecture.system_decomposition.overrides[0].layer,
            "frontend"
        );
    }

    #[test]
    fn marketing_parses_from_kebab_case_table() {
        let toml = r#"
[marketing]
github = "https://github.com/example/repo"
hashtags = ["Foo", "Bar"]

[marketing.twitter]
enabled = true
dry-run = true
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert!(cfg.marketing.twitter.enabled);
        assert!(cfg.marketing.twitter.dry_run);
        assert_eq!(cfg.marketing.github, "https://github.com/example/repo");
        assert_eq!(cfg.marketing.hashtags, vec!["Foo", "Bar"]);
    }

    /// RFC 0111 groundwork: no `[storage]` section at all must leave `ekos_dir` byte-identical to
    /// its pre-existing behavior — the whole point of defaulting `active-container` to `None`.
    #[test]
    fn storage_defaults_to_no_container_redirect() {
        let cfg = EkosConfig::default();
        assert!(cfg.storage.active_container.is_none());
        assert!(cfg.storage.containers.is_empty());
        let cwd = PathBuf::from("/workspace");
        assert_eq!(cfg.ekos_dir(&cwd), cwd.join(".ekos"));

        let cfg: EkosConfig = toml::from_str("[workspace]\n").unwrap();
        assert_eq!(cfg.ekos_dir(&cwd), cwd.join(".ekos"));
    }

    #[test]
    fn storage_parses_containers_from_kebab_case_table() {
        let toml = r#"
[storage]
active-container = "container-a"

[[storage.containers]]
name = "container-a"
path = "/tmp/ekos-containers/a"

[[storage.containers]]
name = "container-b"
path = "/tmp/ekos-containers/b"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.storage.active_container.as_deref(), Some("container-a"));
        assert_eq!(cfg.storage.containers.len(), 2);
        assert_eq!(cfg.storage.containers[0].name, "container-a");
        assert_eq!(
            cfg.storage.containers[0].path,
            PathBuf::from("/tmp/ekos-containers/a")
        );
    }

    /// The core RFC 0111 groundwork claim: naming an active container redirects the *entire*
    /// `.ekos` tree to that container's folder, completely independent of `cwd` — this is the
    /// mechanism that lets two different folders stand in for two different storage containers.
    #[test]
    fn active_container_redirects_ekos_dir_independent_of_cwd() {
        let toml = r#"
[storage]
active-container = "container-a"

[[storage.containers]]
name = "container-a"
path = "/tmp/ekos-containers/a"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        let container_path = PathBuf::from("/tmp/ekos-containers/a");
        assert_eq!(
            cfg.ekos_dir(&PathBuf::from("/workspace/one")),
            container_path
        );
        assert_eq!(
            cfg.ekos_dir(&PathBuf::from("/workspace/two")),
            container_path
        );
        assert_eq!(
            cfg.ledger_dir(&PathBuf::from("/workspace/one")),
            container_path.join("ledger")
        );
        assert_eq!(
            cfg.ledger_path(&PathBuf::from("/workspace/one")),
            container_path.join("ledger").join("ledger.db")
        );
    }

    /// Two different container configs must resolve to two genuinely separate roots — the actual
    /// "simulate distributed storage with different folders" property this exists for.
    #[test]
    fn different_active_containers_resolve_to_different_roots() {
        let toml_a = r#"
[storage]
active-container = "a"
[[storage.containers]]
name = "a"
path = "/tmp/ekos-containers/a"
"#;
        let toml_b = r#"
[storage]
active-container = "b"
[[storage.containers]]
name = "b"
path = "/tmp/ekos-containers/b"
"#;
        let cfg_a: EkosConfig = toml::from_str(toml_a).unwrap();
        let cfg_b: EkosConfig = toml::from_str(toml_b).unwrap();
        let cwd = PathBuf::from("/workspace");
        assert_ne!(cfg_a.ekos_dir(&cwd), cfg_b.ekos_dir(&cwd));
        assert_eq!(
            cfg_a.ekos_dir(&cwd),
            PathBuf::from("/tmp/ekos-containers/a")
        );
        assert_eq!(
            cfg_b.ekos_dir(&cwd),
            PathBuf::from("/tmp/ekos-containers/b")
        );
    }

    /// An `active-container` name that doesn't match any declared container must fall back to
    /// default behavior, not error or panic — matches `from_file_or_default`'s existing
    /// graceful-fallback convention.
    #[test]
    fn unmatched_active_container_falls_back_to_default() {
        let toml = r#"
[storage]
active-container = "does-not-exist"

[[storage.containers]]
name = "container-a"
path = "/tmp/ekos-containers/a"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        assert!(cfg.storage.active_container().is_none());
        let cwd = PathBuf::from("/workspace");
        assert_eq!(cfg.ekos_dir(&cwd), cwd.join(".ekos"));
    }

    /// RFC 0111 §1: no `[storage.partition]` block → partitioning disabled, monthly default.
    #[test]
    fn storage_partition_defaults_to_disabled_monthly() {
        let cfg = EkosConfig::default();
        assert!(!cfg.storage.partition.is_enabled());
        assert_eq!(cfg.storage.partition.default_time_bucket(), "monthly");

        let cfg: EkosConfig = toml::from_str("[storage]\nactive-container = \"a\"\n").unwrap();
        assert!(!cfg.storage.partition.is_enabled());
    }

    #[test]
    fn storage_partition_parses_dimension_bucket_and_overrides() {
        let toml = r#"
[storage.partition]
dimension = "entity-kind"
time-bucket = "weekly"

[[storage.partition.time-bucket-overrides]]
scope-glob = "discord:*"
time-bucket = "daily"
"#;
        let cfg: EkosConfig = toml::from_str(toml).unwrap();
        let p = &cfg.storage.partition;
        assert!(p.is_enabled());
        assert_eq!(p.dimension.as_deref(), Some("entity-kind"));
        assert_eq!(p.default_time_bucket(), "weekly");
        assert_eq!(p.time_bucket_overrides.len(), 1);
        assert_eq!(p.time_bucket_overrides[0].scope_glob, "discord:*");
        assert_eq!(p.time_bucket_overrides[0].time_bucket, "daily");
    }
}
