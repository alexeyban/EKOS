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

fn default_root() -> PathBuf { PathBuf::from(".") }
fn default_log_level() -> String { "info".into() }
fn default_log_format() -> String { "pretty".into() }

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self { root: default_root(), log_level: default_log_level(), log_format: default_log_format() }
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
        Self { paths: Vec::new(), ignore_patterns: default_ignore_patterns() }
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
}

#[allow(clippy::derivable_impls)]
impl Default for EkosConfig {
    fn default() -> Self {
        Self {
            workspace: WorkspaceConfig::default(),
            observe: ObserveConfig::default(),
            llm: LlmConfig::default(),
            ai: AiConfig::default(),
        }
    }
}

impl EkosConfig {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("invalid ekos.toml: {}", e))?;
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

    /// Absolute path to the .ekos/ metadata directory.
    pub fn ekos_dir(&self, cwd: &Path) -> PathBuf {
        cwd.join(".ekos")
    }

    /// Absolute path to the artifact cache.
    pub fn artifact_dir(&self, cwd: &Path) -> PathBuf {
        self.ekos_dir(cwd).join("artifacts")
    }

    /// Absolute path to the ledger database.
    pub fn ledger_path(&self, cwd: &Path) -> PathBuf {
        self.ekos_dir(cwd).join("ledger").join("ledger.db")
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
}
