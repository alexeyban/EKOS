use async_trait::async_trait;
use ekos_artifact::{ArtifactStore, FileSystemArtifactStore};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use thiserror::Error;

use crate::{config::EkosConfig, diagnostics::DiagnosticSink};

/// Error produced by a single compiler pass.
#[derive(Debug, Error)]
pub enum PassError {
    #[error("pass failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl PassError {
    pub fn failed(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }
}

/// Shared context threaded through every compiler pass.
pub struct PassContext {
    pub config: Arc<EkosConfig>,
    pub diagnostics: DiagnosticSink,
    /// Current working directory (where ekos.toml lives).
    pub cwd: std::path::PathBuf,
    /// Content-addressable artifact store passes can read/write.
    pub artifact_store: Arc<dyn ArtifactStore>,
}

impl PassContext {
    pub fn new(config: Arc<EkosConfig>, cwd: std::path::PathBuf) -> Self {
        let artifact_dir = config.artifact_dir(&cwd);
        let store = Arc::new(FileSystemArtifactStore::new(artifact_dir));
        Self {
            config,
            diagnostics: DiagnosticSink::default(),
            cwd,
            artifact_store: store,
        }
    }

    pub fn with_artifact_store(mut self, store: Arc<dyn ArtifactStore>) -> Self {
        self.artifact_store = store;
        self
    }
}

/// The core compiler extension point. Every observation, analysis, and compilation
/// step implements this trait.
///
/// # Contract
/// - `run` must be deterministic: given the same inputs, produce the same outputs.
/// - `run` must not have hidden side effects beyond writing to `ctx`.
/// - `run` declares its dependencies; the PassManager enforces the order.
#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;

    /// Names of passes that must complete before this one runs.
    fn dependencies(&self) -> &[&str] {
        &[]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError>;
}

/// Error produced by the pass dependency graph analysis.
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("dependency cycle detected involving pass '{0}'")]
    CycleDetected(String),
    #[error("pass '{pass}' declares unknown dependency '{dep}'")]
    UnknownDependency { pass: String, dep: String },
}

/// Validates the pass dependency graph and returns a topological execution order.
pub struct PassManager {
    passes: Vec<Box<dyn CompilerPass>>,
}

impl PassManager {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn register(&mut self, pass: Box<dyn CompilerPass>) {
        self.passes.push(pass);
    }

    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.passes.len()
    }

    /// Returns pass names in a valid topological execution order.
    pub fn execution_order(&self) -> Result<Vec<String>, SchedulerError> {
        let known: HashSet<&str> = self.passes.iter().map(|p| p.name()).collect();

        // Validate that all declared dependencies actually exist.
        for pass in &self.passes {
            for &dep in pass.dependencies() {
                if !known.contains(dep) {
                    return Err(SchedulerError::UnknownDependency {
                        pass: pass.name().to_string(),
                        dep: dep.to_string(),
                    });
                }
            }
        }

        // Kahn's algorithm: build adjacency list and in-degree map.
        let mut in_degree: HashMap<&str, usize> =
            self.passes.iter().map(|p| (p.name(), 0)).collect();
        let mut adj: HashMap<&str, Vec<&str>> =
            self.passes.iter().map(|p| (p.name(), vec![])).collect();

        for pass in &self.passes {
            for &dep in pass.dependencies() {
                // dep → pass edge (dep must finish first)
                adj.get_mut(dep).unwrap().push(pass.name());
                *in_degree.get_mut(pass.name()).unwrap() += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|&(_, d)| *d == 0)
            .map(|(&n, _)| n)
            .collect();

        let mut order: Vec<String> = Vec::with_capacity(self.passes.len());
        while let Some(name) = queue.pop_front() {
            order.push(name.to_string());
            for &next in adj.get(name).unwrap() {
                let d = in_degree.get_mut(next).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(next);
                }
            }
        }

        if order.len() != self.passes.len() {
            let cycle_node = self
                .passes
                .iter()
                .find(|p| !order.contains(&p.name().to_string()))
                .map(|p| p.name().to_string())
                .unwrap_or_default();
            return Err(SchedulerError::CycleDetected(cycle_node));
        }

        Ok(order)
    }

    /// Execute all passes in dependency order, using the given failure mode.
    pub async fn run_all(
        &mut self,
        ctx: &mut PassContext,
        failure_mode: crate::scheduler::FailureMode,
    ) -> Result<crate::scheduler::ExecutionReport, SchedulerError> {
        use crate::scheduler::{ExecutionReport, FailureMode, PassOutcome};

        let order = self.execution_order()?;
        let mut report = ExecutionReport { outcomes: Vec::new() };

        for name in &order {
            let pass = self
                .passes
                .iter_mut()
                .find(|p| p.name() == name.as_str())
                .expect("execution_order only contains registered pass names");

            tracing::info!(pass = %name, "running pass");
            let result = pass.run(ctx).await;
            let failed = result.is_err();

            report.outcomes.push(PassOutcome { pass_name: name.clone(), result });

            if failed && matches!(failure_mode, FailureMode::FailFast) {
                break;
            }
        }

        Ok(report)
    }
}

impl Default for PassManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::FailureMode;

    struct NamedPass(&'static str, &'static [&'static str]);

    #[async_trait]
    impl CompilerPass for NamedPass {
        fn name(&self) -> &str { self.0 }
        fn dependencies(&self) -> &[&str] { self.1 }
        async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> { Ok(()) }
    }

    #[test]
    fn topological_order_a_b_c() {
        let mut pm = PassManager::new();
        pm.register(Box::new(NamedPass("C", &["B"])));
        pm.register(Box::new(NamedPass("A", &[])));
        pm.register(Box::new(NamedPass("B", &["A"])));
        let order = pm.execution_order().unwrap();
        assert!(order.iter().position(|n| n == "A") < order.iter().position(|n| n == "B"));
        assert!(order.iter().position(|n| n == "B") < order.iter().position(|n| n == "C"));
    }

    #[test]
    fn cycle_detected() {
        let mut pm = PassManager::new();
        pm.register(Box::new(NamedPass("A", &["B"])));
        pm.register(Box::new(NamedPass("B", &["A"])));
        assert!(matches!(pm.execution_order(), Err(SchedulerError::CycleDetected(_))));
    }

    #[test]
    fn unknown_dependency() {
        let mut pm = PassManager::new();
        pm.register(Box::new(NamedPass("A", &["ghost"])));
        assert!(matches!(
            pm.execution_order(),
            Err(SchedulerError::UnknownDependency { .. })
        ));
    }

    #[test]
    fn zero_passes_empty_order() {
        let pm = PassManager::new();
        assert_eq!(pm.execution_order().unwrap(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn collect_mode_runs_all_despite_failure() {
        struct FailingPass;
        #[async_trait]
        impl CompilerPass for FailingPass {
            fn name(&self) -> &str { "fail" }
            async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
                Err(PassError::failed("intentional"))
            }
        }

        let mut pm = PassManager::new();
        pm.register(Box::new(FailingPass));
        pm.register(Box::new(NamedPass("ok", &[])));

        let config = Arc::new(EkosConfig::default());
        let mut ctx = PassContext::new(config, std::path::PathBuf::from("."));
        let report = pm.run_all(&mut ctx, FailureMode::Collect).await.unwrap();
        assert_eq!(report.passes_run(), 2);
    }

    #[tokio::test]
    async fn fail_fast_stops_after_first_error() {
        struct FailingPass;
        #[async_trait]
        impl CompilerPass for FailingPass {
            fn name(&self) -> &str { "fail" }
            async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
                Err(PassError::failed("intentional"))
            }
        }

        // "must-run-after" depends on "fail", so it comes second in topological order.
        // In FailFast mode the scheduler stops after "fail" errors, so "must-run-after" is skipped.
        let mut pm = PassManager::new();
        pm.register(Box::new(FailingPass));
        pm.register(Box::new(NamedPass("must-run-after", &["fail"])));

        let config = Arc::new(EkosConfig::default());
        let mut ctx = PassContext::new(config, std::path::PathBuf::from("."));
        let report = pm.run_all(&mut ctx, FailureMode::FailFast).await.unwrap();
        assert_eq!(report.passes_run(), 1);
        assert!(report.has_errors());
    }
}
