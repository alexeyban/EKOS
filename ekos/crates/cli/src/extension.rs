//! RFC 0149 — the extension seam.
//!
//! An out-of-tree crate implements [`EkosExtension`] to add observers to `ekos build`, passes to
//! `ekos recover`, a post-`commit` step and MCP tools, then builds its own binary with
//! [`crate::app::main_with`]. The public `ekos` binary runs with [`Extensions::none`].
//!
//! RFC 0191 adds commands of its own (each leaf with its catalogue class) and lets an extension
//! say which of its MCP tools a read scope can filter. RFC 0198 adds console pages: data that the
//! web console renders, built from the extension's own read commands. RFC 0200 lets one
//! extension offer a [`JavaDecompiler`] that another extension's command or pass can use.
//!
//! A private crate cannot be an optional dependency of a public one (Cargo resolves every git
//! dependency, optional or not, when it writes `Cargo.lock`), so the dependency points outward:
//! the extension depends on these public crates, never the other way round.

use crate::commands::catalogue::Class;
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

    /// RFC 0191: true when MCP tool `name` reads knowledge only through the store
    /// [`EkosExtension::call_mcp_tool`] is handed. A read scope filters that store, so a reader
    /// with restricted access may call the tool. Default: refused to such a reader.
    fn mcp_tool_is_scoped(&self, _name: &str) -> bool {
        false
    }

    /// RFC 0191: top-level subcommands this extension adds, e.g. `lakehouse` with subcommands of
    /// its own. A name that is already a command stops the binary at start-up.
    fn commands(&self) -> Vec<clap::Command> {
        Vec::new()
    }

    /// RFC 0191: the catalogue class (RFC 0186) of every leaf command [`EkosExtension::commands`]
    /// adds, by its path ("lakehouse status"), and whether it may call the configured model. A
    /// leaf without a row stops the binary at start-up.
    fn command_classes(&self) -> Vec<(&'static str, Class, bool)> {
        Vec::new()
    }

    /// RFC 0191: runs the command `name`, one of [`EkosExtension::commands`]. `matches` are that
    /// command's own matches; the global flags have already been applied to `ctx`.
    async fn run_command(
        &self,
        name: &str,
        _matches: &clap::ArgMatches,
        _ctx: &CommandContext<'_>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("extension {} has no command {name}", self.name())
    }

    /// RFC 0198: workspace pages for the web console. Each panel shows the JSON one of this
    /// binary's **read** commands prints; each action runs a catalogue command. Checked against
    /// the catalogue by `ekos extensions --json`.
    fn console_pages(&self) -> Vec<ConsolePage> {
        Vec::new()
    }

    /// RFC 0199: the binary this extension ships as — its name and version, shown by `--version`
    /// and `--help` as `<name> <version> (EKOS <ekos version>)`. `None`: the binary is `ekos`.
    fn binary(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    /// RFC 0200: a decompiler for compiled JVM classes this extension offers to the others (the
    /// private `ekos-binary` build). `None`: it offers none.
    fn java_decompiler(&self) -> Option<Arc<dyn JavaDecompiler>> {
        None
    }
}

/// RFC 0200: turns a compiled JVM class into Java-like source, one method at a time. The public
/// build has no implementation; a caller treats `None` from [`Extensions::java_decompiler`] as
/// "compiled code stays a gap".
pub trait JavaDecompiler: Send + Sync {
    /// Stable identifier, recorded with every translation that used it.
    fn name(&self) -> &'static str;

    /// Decompiles `class` (binary name, `org/example/MyStep`) from `archive`, the bytes of a jar
    /// or of one class file. Nothing is executed.
    fn decompile(&self, archive: &[u8], class: &str) -> anyhow::Result<DecompiledClass>;
}

/// One class a [`JavaDecompiler`] recovered.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecompiledClass {
    /// Binary name, `org/example/MyStep`.
    pub class: String,
    pub super_class: Option<String>,
    pub interfaces: Vec<String>,
    pub methods: Vec<DecompiledMethod>,
    /// What the decompiler could not read, in words.
    pub diagnostics: Vec<String>,
}

/// One method of a [`DecompiledClass`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecompiledMethod {
    pub name: String,
    /// JVM descriptor, `(Ljava/lang/Object;)Z`.
    pub descriptor: String,
    pub fidelity: DecompiledFidelity,
    /// Java-like source of the body: statements when `fidelity` is `Statements`, otherwise the
    /// signature and whatever structure was recovered.
    pub source: String,
    /// Parts of the body that were not recovered, each with its bytecode offset range in words.
    pub gaps: Vec<String>,
}

/// How much of a method body a [`JavaDecompiler`] recovered (RFC 0150's labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecompiledFidelity {
    Structural,
    ControlFlow,
    Statements,
}

/// RFC 0198: one workspace tab in the web console.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ConsolePage {
    /// URL segment and key; lowercase letters, digits and `-`.
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub panels: Vec<Panel>,
    pub actions: Vec<Action>,
}

/// The JSON a read command prints, laid out by `view`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Panel {
    pub id: &'static str,
    pub title: &'static str,
    /// The full argv after the binary, e.g. `["lakehouse", "status", "--json"]`: a catalogue
    /// command of class read and its flags. No values come from the page or the person.
    pub command: Vec<&'static str>,
    pub view: View,
    /// Shown when the view has nothing to show (no rows, a missing field).
    pub empty: &'static str,
}

/// How a panel lays out its command's JSON. Pointers are RFC 6901 JSON pointers.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum View {
    /// `rows` points at an array; each column points into one element.
    Table {
        rows: &'static str,
        columns: Vec<Column>,
    },
    /// One object: label → pointer.
    Fields { fields: Vec<Column> },
    /// A string, shown preformatted.
    Text { pointer: &'static str },
}

/// A table column, or one of `Fields`' fields.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Column {
    pub key: &'static str,
    pub label: &'static str,
    /// Show the value as a status badge (its tone comes from the value, RFC 0198 §1).
    pub badge: bool,
}

impl Column {
    pub fn new(key: &'static str, label: &'static str) -> Self {
        Self {
            key,
            label,
            badge: false,
        }
    }

    pub fn badge(key: &'static str, label: &'static str) -> Self {
        Self {
            key,
            label,
            badge: true,
        }
    }
}

/// A button that runs a catalogue command as a job, with no arguments of its own.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Action {
    pub label: &'static str,
    /// The command's catalogue path, e.g. `["lakehouse", "sync"]`.
    pub command: Vec<&'static str>,
}

/// What an extension command gets: the same setup a built-in command has once `main_with` has
/// applied `--config`, `--project` and `--tenant` (RFC 0178).
pub struct CommandContext<'a> {
    pub config: &'a EkosConfig,
    /// The workspace: the project's sources for a home project, else the current directory.
    pub cwd: &'a Path,
    /// The `ekos.toml` the config came from (it may not exist).
    pub config_path: &'a Path,
    /// Where the command was typed. A path argument is relative to this, even after a project
    /// moved the process into its sources.
    pub invoked_from: &'a Path,
    /// Every extension of this binary (RFC 0200), e.g. to ask for a [`JavaDecompiler`].
    pub extensions: &'a Extensions,
}

/// What `ekos recover` hands an extension when it asks for passes.
pub struct RecoverContext<'a> {
    pub config: &'a EkosConfig,
    pub cwd: &'a Path,
    /// The workspace directory's own name, the project qualifier every built-in analyzer pass is
    /// constructed with.
    pub project: String,
    pub artifact_store: &'a dyn ArtifactStore,
    /// Every extension of this binary (RFC 0200), e.g. to ask for a [`JavaDecompiler`].
    pub extensions: &'a Extensions,
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

    /// RFC 0191: whether an extension says its MCP tool `name` may be served to a restricted
    /// reader.
    pub fn mcp_tool_is_scoped(&self, name: &str) -> bool {
        self.0.iter().any(|e| e.mcp_tool_is_scoped(name))
    }

    /// RFC 0200: the first [`JavaDecompiler`] an extension offers.
    pub fn java_decompiler(&self) -> Option<Arc<dyn JavaDecompiler>> {
        self.0.iter().find_map(|e| e.java_decompiler())
    }

    /// The extension that added top-level command `name`.
    pub fn command_owner(&self, name: &str) -> Option<&Arc<dyn EkosExtension>> {
        self.0
            .iter()
            .find(|e| e.commands().iter().any(|c| c.get_name() == name))
    }

    /// Every extension's command classes, for the catalogue.
    pub fn command_classes(&self) -> Vec<(&'static str, Class, bool)> {
        self.0.iter().flat_map(|e| e.command_classes()).collect()
    }

    /// `root` plus every extension's commands. Fails when a name is taken or an extension leaf
    /// has no class: either would be a broken build, so it stops before anything runs.
    pub fn extend_cli(&self, mut root: clap::Command) -> anyhow::Result<clap::Command> {
        if let Some((name, version)) = self.0.iter().find_map(|e| e.binary()) {
            // Built once per process; clap keeps borrowed strings.
            let full: &'static str = Box::leak(
                format!("{version} (EKOS {})", env!("CARGO_PKG_VERSION")).into_boxed_str(),
            );
            root = root.name(name).bin_name(name).version(full);
        }
        for e in self.0.iter() {
            let classes = e.command_classes();
            for c in e.commands() {
                let name = c.get_name().to_string();
                if root.find_subcommand(&name).is_some() {
                    anyhow::bail!(
                        "extension {} adds a command `{name}`, which already exists",
                        e.name()
                    );
                }
                let wrapped = clap::Command::new("root").subcommand(c.clone());
                for (path, _) in crate::commands::catalogue::leaves(&wrapped) {
                    let path = path.join(" ");
                    if !classes.iter().any(|(p, _, _)| *p == path) {
                        anyhow::bail!(
                            "extension {} adds `{path}` without a class in command_classes() \
                             (decide who may run it from the web, RFC 0186)",
                            e.name()
                        );
                    }
                }
                root = root.subcommand(c);
            }
        }
        Ok(root)
    }
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|e| e.name()))
            .finish()
    }
}
