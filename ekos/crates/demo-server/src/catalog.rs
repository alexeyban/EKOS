//! RFC 0045 — the fixed, two-repo demo catalog. Deliberately not general
//! self-serve ingestion (see the RFC's Non-goals): visitors pick one of a
//! small, pre-baked set of repos, never an arbitrary URL.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct RepoEntry {
    /// URL-safe identifier, e.g. `"ekos"` — used as the `?repo=` query value.
    pub slug: String,
    /// Human-readable name shown in the repo picker.
    pub display_name: String,
    /// One-line framing shown under the name (the demo's "why this repo" note).
    pub tagline: String,
    /// Directory containing this repo's `ekos.toml` and `.ekos/` ledger,
    /// already baked (`init`/`build`/`recover`/`resolve`/`compile`/`commit`
    /// already run) — the live server only ever opens this read-only.
    pub workspace_dir: PathBuf,
    /// Directory of pre-rendered static HTML (produced by `prerender`, see
    /// `src/bin/prerender.rs`) served as-is, with zero server-side templating.
    pub html_dir: PathBuf,
    /// Optional `[llm]` overrides applied on top of `workspace_dir/ekos.toml`
    /// (RFC 0046) — lets the demo catalog force a provider (e.g. `"openai"`)
    /// for the live demo without editing the underlying repo's real
    /// `ekos.toml`, which may be a genuine project (EKOS-self) whose own
    /// default provider this demo has no business changing.
    #[serde(default)]
    pub llm_provider: Option<String>,
    #[serde(default)]
    pub llm_api_key_env: Option<String>,
    /// Overrides `[ai] max_tokens` (default 1024, `AiRuntimeConfig::default()`) — the demo's
    /// live-LLM answers were observed running out of budget before completing their trailing
    /// `cited_evidence` JSON block, which `AiRuntime`'s citation parser then silently drops
    /// (see `ai.rs`'s `extract_citations` doc comment) — a real answer with an empty citation
    /// list looks like a clean success but defeats the demo's core "evidence-cited, not an
    /// unverified guess" claim. Raising the budget here, not in the underlying repo's real
    /// `ekos.toml`, for the same reason the `llm_*` overrides above exist.
    #[serde(default)]
    pub ai_max_tokens: Option<u32>,
}

impl RepoEntry {
    /// Load `workspace_dir/ekos.toml`, then apply this entry's `[llm]`/`[ai]`
    /// overrides on top, if any.
    pub fn load_config(&self) -> ekos_compiler_core::EkosConfig {
        let mut config = ekos_compiler_core::EkosConfig::from_file_or_default(
            &self.workspace_dir.join("ekos.toml"),
        );
        if let Some(provider) = &self.llm_provider {
            config.llm.provider = Some(provider.clone());
        }
        if let Some(key_env) = &self.llm_api_key_env {
            config.llm.api_key_env = Some(key_env.clone());
        }
        if let Some(max_tokens) = self.ai_max_tokens {
            config.ai.max_tokens = Some(max_tokens);
        }
        config
    }
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogFile {
    #[serde(rename = "repo", default)]
    repos: Vec<RepoEntry>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub repos: Vec<RepoEntry>,
}

impl Catalog {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading catalog file {}", path.display()))?;
        let parsed: CatalogFile = toml::from_str(&raw)
            .with_context(|| format!("parsing catalog file {}", path.display()))?;
        if parsed.repos.is_empty() {
            anyhow::bail!("catalog {} defines zero repos", path.display());
        }
        Ok(Self {
            repos: parsed.repos,
        })
    }

    pub fn find(&self, slug: &str) -> Option<&RepoEntry> {
        self.repos.iter().find(|r| r.slug == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_valid_two_repo_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.toml");
        std::fs::write(
            &path,
            r#"
[[repo]]
slug = "ekos"
display_name = "EKOS"
tagline = "self-hosted"
workspace_dir = "/tmp/ekos"
html_dir = "/tmp/ekos-html"

[[repo]]
slug = "fd"
display_name = "fd"
tagline = "external"
workspace_dir = "/tmp/fd"
html_dir = "/tmp/fd-html"
"#,
        )
        .unwrap();

        let catalog = Catalog::load(&path).unwrap();
        assert_eq!(catalog.repos.len(), 2);
        assert_eq!(catalog.find("fd").unwrap().display_name, "fd");
        assert!(catalog.find("nope").is_none());
    }

    #[test]
    fn rejects_a_catalog_with_zero_repos() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.toml");
        std::fs::write(&path, "").unwrap();

        assert!(
            Catalog::load(&path)
                .unwrap_err()
                .to_string()
                .contains("zero repos")
        );
    }

    #[test]
    fn errors_clearly_when_the_catalog_file_is_missing() {
        let missing = std::path::Path::new("/nonexistent/catalog.toml");
        assert!(Catalog::load(missing).is_err());
    }
}
