//! RFC 0149 — the extension seam.
//!
//! An out-of-tree crate implements [`EkosExtension`] to add observers to `ekos build`, passes to
//! `ekos recover`, a post-`commit` step and MCP tools, then builds its own binary with
//! [`crate::app::main_with`]. The public `ekos` binary runs with [`Extensions::none`].
//!
//! A private crate cannot be an optional dependency of a public one (Cargo resolves every git
//! dependency, optional or not, when it writes `Cargo.lock`), so the dependency points outward:
//! the extension depends on these public crates, never the other way round.

use async_trait::async_trait;
use ekos_artifact::{ArtifactId, ArtifactStore};
use ekos_compiler_core::{EkosConfig, pass::CompilerPass};
use ekos_ledger::KnowledgeStore;
use ekos_observation_sdk::Observer;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

/// One out-of-tree capability plugged into the CLI pipeline. Every hook has a no-op default, so
/// an extension implements only the stages it takes part in.
///
/// `?Send`: [`CommitContext`] carries `&dyn KnowledgeStore`, which is not `Sync`, so a real
/// `after_commit` future cannot be `Send`. `commit` already holds the store across `.await`s and
/// is only ever driven by `block_on`, never spawned, so nothing here needs `Send` futures.
/// Implementations use `#[async_trait(?Send)]` too.
#[async_trait(?Send)]
pub trait EkosExtension: Send + Sync {
    /// Stable identifier, shown in diagnostics.
    fn name(&self) -> &'static str;

    /// Folded into `ekos build`'s fingerprint cache key (RFC 0135 Part A). Bump it whenever this
    /// extension's observer output changes, for the same reason `PIPELINE_LOGIC_VERSION` exists:
    /// otherwise an unchanged source tree keeps serving artifacts the old logic produced.
    fn logic_version(&self) -> u32 {
        0
    }

    /// Observers appended after the built-in ones in `ekos build`.
    fn observers(&self, _config: &EkosConfig) -> Vec<Box<dyn Observer>> {
        Vec::new()
    }

    /// Passes for `ekos recover`, built from the artifacts the observers wrote.
    fn recovery_passes(&self, _ctx: &RecoverContext<'_>) -> Vec<RecoveryContribution> {
        Vec::new()
    }

    /// Runs in `ekos commit` after `[llm-description]` and before `[embeddings]`, over the fully
    /// committed ledger. Returns summary lines for `commit`'s report.
    async fn after_commit(&self, _ctx: &CommitContext<'_>) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Extra MCP tool definitions, appended to `tools/list`.
    fn mcp_tools(&self, _config: &EkosConfig) -> Vec<Value> {
        Vec::new()
    }

    /// `Some` if this extension owns the tool `name`. Read-only: `ledger` is the same cached store
    /// every built-in read tool uses.
    fn call_mcp_tool(
        &self,
        _name: &str,
        _args: &Value,
        _ledger: &dyn KnowledgeStore,
    ) -> Option<anyhow::Result<Value>> {
        None
    }
}

/// What `ekos recover` hands an extension when it asks for passes.
pub struct RecoverContext<'a> {
    pub config: &'a EkosConfig,
    pub cwd: &'a Path,
    /// The workspace directory's own name, the project qualifier every built-in analyzer pass is
    /// constructed with.
    pub project: String,
    pub artifact_store: &'a dyn ArtifactStore,
}

impl RecoverContext<'_> {
    /// The newest artifact per target written by the observer named `connector` — the same
    /// deduplication every built-in pass uses.
    pub fn artifact_ids_for_connector(&self, connector: &str) -> Vec<ArtifactId> {
        crate::commands::recover::collect_artifact_ids_for_connector(self.artifact_store, connector)
    }
}

/// A recovery pass plus the report it prints once the pass manager has run.
pub struct RecoveryContribution {
    pub pass: Box<dyn CompilerPass>,
    /// Called after every pass has run; its lines are printed in `recover`'s summary. Built-in
    /// passes report counts they accumulated behind a stats handle, and this is how an extension
    /// pass does the same.
    pub report: Box<dyn FnOnce() -> Vec<String> + Send>,
}

/// What `ekos commit` hands an extension's post-commit step.
pub struct CommitContext<'a> {
    pub config: &'a EkosConfig,
    pub cwd: &'a Path,
    pub ledger: &'a dyn KnowledgeStore,
    /// `--yes`: spend confirmations are pre-approved.
    pub yes: bool,
}

/// The set of extensions a binary runs with. Cheap to clone; passed explicitly (dependency
/// injection), never held in global state.
#[derive(Clone, Default)]
pub struct Extensions(Arc<[Arc<dyn EkosExtension>]>);

impl Extensions {
    /// No extensions: the public `ekos` binary.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn new(extensions: Vec<Arc<dyn EkosExtension>>) -> Self {
        Self(extensions.into())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn EkosExtension>> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Combined logic version for the build fingerprint: 0 with no extensions, so a public build's
    /// cache keys are exactly what they were before RFC 0149.
    pub fn logic_version(&self) -> u32 {
        self.0.iter().fold(0u32, |acc, e| {
            acc.wrapping_mul(31)
                .wrapping_add(e.logic_version())
                .wrapping_add(e.name().len() as u32)
        })
    }

    /// First extension that owns MCP tool `name`.
    pub fn call_mcp_tool(
        &self,
        name: &str,
        args: &Value,
        ledger: &dyn KnowledgeStore,
    ) -> Option<anyhow::Result<Value>> {
        self.0
            .iter()
            .find_map(|e| e.call_mcp_tool(name, args, ledger))
    }
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|e| e.name()))
            .finish()
    }
}
