use async_trait::async_trait;
use ekos_artifact::{ArtifactStore, FileSystemArtifactStore};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
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
///
/// Cheap to `Clone` — every field is an `Arc` (or a small `PathBuf`) — so
/// concurrently scheduled passes (Phase 13 — Optimizer) can each get their own
/// owned context while still writing into the same diagnostics sink and
/// artifact store.
#[derive(Clone)]
pub struct PassContext {
    pub config: Arc<EkosConfig>,
    pub diagnostics: Arc<Mutex<DiagnosticSink>>,
    /// Current working directory (where ekos.toml lives).
    pub cwd: std::path::PathBuf,
    /// Content-addressable artifact store passes can read/write.
    pub artifact_store: Arc<dyn ArtifactStore>,
}

impl PassContext {
    pub fn new(config: Arc<EkosConfig>, cwd: std::path::PathBuf) -> Self {
        let artifact_dir = config.artifact_dir(&cwd);
        // RFC 0015: packed segments by default; fall back to the loose-file
        // layout if the segment scan fails (e.g. unreadable directory) so a
        // damaged store degrades instead of aborting construction.
        let store: Arc<dyn ArtifactStore> =
            match ekos_artifact::PackArtifactStore::open(&artifact_dir) {
                Ok(pack) => Arc::new(pack),
                Err(e) => {
                    tracing::warn!("pack store unavailable ({e}); using loose files");
                    Arc::new(FileSystemArtifactStore::new(artifact_dir))
                }
            };
        Self {
            config,
            diagnostics: Arc::new(Mutex::new(DiagnosticSink::default())),
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

    /// Version string for this pass's logic (Phase 13 — Optimizer). Bump it
    /// manually whenever a change to `run`'s behavior means previously cached
    /// output should no longer be reused, even if inputs are unchanged.
    fn version(&self) -> &str {
        "v1"
    }

    /// Opaque identifiers for whatever this pass reads as input (e.g. a
    /// content hash of source text, or the artifact ids it consumes) — used
    /// for cache invalidation (Phase 13). A pass with nothing to identify
    /// (always recomputes) can leave this empty.
    fn cache_inputs(&self) -> Vec<String> {
        Vec::new()
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
    #[error("duplicate pass name '{0}' — pass names must be unique")]
    DuplicatePassName(String),
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

    /// Pass names key the dependency graph, the cache manifest, and the
    /// execution report — duplicates would silently collapse graph nodes and
    /// then surface as a bogus "cycle detected" error.
    fn check_unique_names(&self) -> Result<(), SchedulerError> {
        let mut seen: HashSet<&str> = HashSet::with_capacity(self.passes.len());
        for pass in &self.passes {
            if !seen.insert(pass.name()) {
                return Err(SchedulerError::DuplicatePassName(pass.name().to_string()));
            }
        }
        Ok(())
    }

    /// Returns pass names in a valid topological execution order.
    pub fn execution_order(&self) -> Result<Vec<String>, SchedulerError> {
        self.check_unique_names()?;
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
    ///
    /// Before running each pass, checks `should_recompute` against a manifest
    /// of that pass's last known `{version, config_hash, cache_inputs}`
    /// (Phase 13 — Optimizer); if nothing has changed, the pass is skipped and
    /// reported as `PassOutcome::skipped`.
    pub async fn run_all(
        &mut self,
        ctx: &mut PassContext,
        failure_mode: crate::scheduler::FailureMode,
    ) -> Result<crate::scheduler::ExecutionReport, SchedulerError> {
        use crate::scheduler::{ExecutionReport, FailureMode, PassOutcome};

        let order = self.execution_order()?;
        let mut report = ExecutionReport {
            outcomes: Vec::new(),
        };
        let manifest_dir = ctx.config.artifact_dir(&ctx.cwd).join("pass-manifests");
        let cfg_hash = crate::cache::config_hash(
            &serde_json::to_value(ctx.config.as_ref()).unwrap_or_default(),
        );

        for name in &order {
            let pass = self
                .passes
                .iter_mut()
                .find(|p| p.name() == name.as_str())
                .expect("execution_order only contains registered pass names");

            if !crate::cache::should_recompute(pass.as_ref(), &cfg_hash, &manifest_dir) {
                tracing::info!(pass = %name, "skipping pass (cached)");
                report.outcomes.push(PassOutcome::skipped(name.clone()));
                continue;
            }

            tracing::info!(pass = %name, "running pass");
            let result = pass.run(ctx).await;
            let failed = result.is_err();

            if !failed {
                crate::cache::record_manifest(pass.as_ref(), &cfg_hash, &manifest_dir);
            }

            report.outcomes.push(PassOutcome::ran(name.clone(), result));

            if failed && matches!(failure_mode, FailureMode::FailFast) {
                break;
            }
        }

        Ok(report)
    }

    /// Groups the dependency DAG into levels via iterated Kahn layering: level 0
    /// is every pass with no dependencies, level 1 is every pass whose
    /// dependencies are all in level 0, and so on. Passes within one level have
    /// no path between them in the DAG, so they can run concurrently.
    pub fn execution_levels(&self) -> Result<Vec<Vec<String>>, SchedulerError> {
        self.check_unique_names()?;
        let known: HashSet<&str> = self.passes.iter().map(|p| p.name()).collect();
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

        let mut adj: HashMap<&str, Vec<&str>> =
            self.passes.iter().map(|p| (p.name(), vec![])).collect();
        let mut remaining: HashMap<&str, usize> =
            self.passes.iter().map(|p| (p.name(), 0)).collect();
        for pass in &self.passes {
            for &dep in pass.dependencies() {
                adj.get_mut(dep).unwrap().push(pass.name());
                *remaining.get_mut(pass.name()).unwrap() += 1;
            }
        }

        let mut levels: Vec<Vec<String>> = Vec::new();
        let mut done = 0usize;
        loop {
            let level: Vec<&str> = remaining
                .iter()
                .filter(|&(_, &d)| d == 0)
                .map(|(&n, _)| n)
                .collect();
            if level.is_empty() {
                break;
            }
            for &n in &level {
                remaining.remove(n);
                done += 1;
                for &next in adj.get(n).unwrap() {
                    if let Some(d) = remaining.get_mut(next) {
                        *d -= 1;
                    }
                }
            }
            let mut sorted: Vec<String> = level.into_iter().map(String::from).collect();
            sorted.sort();
            levels.push(sorted);
        }

        if done != self.passes.len() {
            let placed: HashSet<&str> = levels.iter().flatten().map(|s| s.as_str()).collect();
            let cycle_node = self
                .passes
                .iter()
                .find(|p| !placed.contains(p.name()))
                .map(|p| p.name().to_string())
                .unwrap_or_default();
            return Err(SchedulerError::CycleDetected(cycle_node));
        }

        Ok(levels)
    }

    /// Execute passes level-by-level, running every pass within a level
    /// concurrently (Phase 13 — Optimizer). Each spawned pass gets its own
    /// cloned `PassContext` — cheap, since every field is an `Arc` or small
    /// value — so all clones still write into the same diagnostics sink and
    /// artifact store despite each pass needing an exclusive `&mut self`.
    pub async fn run_all_parallel(
        &mut self,
        ctx: &PassContext,
        failure_mode: crate::scheduler::FailureMode,
    ) -> Result<crate::scheduler::ExecutionReport, SchedulerError> {
        use crate::scheduler::{ExecutionReport, FailureMode, PassOutcome};

        let levels = self.execution_levels()?;
        let mut passes = std::mem::take(&mut self.passes);
        let mut report = ExecutionReport {
            outcomes: Vec::new(),
        };
        let mut failed_overall = false;

        for level in &levels {
            if failed_overall && matches!(failure_mode, FailureMode::FailFast) {
                break;
            }

            let mut level_passes: Vec<(String, Box<dyn CompilerPass>)> = Vec::new();
            for name in level {
                if let Some(pos) = passes.iter().position(|p| p.name() == name.as_str()) {
                    level_passes.push((name.clone(), passes.remove(pos)));
                }
            }

            let mut handles = Vec::new();
            for (name, mut pass) in level_passes {
                let mut pass_ctx = ctx.clone();
                handles.push(tokio::spawn(async move {
                    tracing::info!(pass = %name, "running pass (parallel)");
                    let result = pass.run(&mut pass_ctx).await;
                    (name, result)
                }));
            }

            for handle in handles {
                let (name, result) = handle.await.expect("pass task panicked");
                let failed = result.is_err();
                report.outcomes.push(PassOutcome::ran(name, result));
                if failed && matches!(failure_mode, FailureMode::FailFast) {
                    failed_overall = true;
                }
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
        fn name(&self) -> &str {
            self.0
        }
        fn dependencies(&self) -> &[&str] {
            self.1
        }
        async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
            Ok(())
        }
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
        assert!(matches!(
            pm.execution_order(),
            Err(SchedulerError::CycleDetected(_))
        ));
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

    /// Regression: duplicate pass names used to collapse into one graph node
    /// and surface as `CycleDetected("")` — first hit by `ekos recover` over a
    /// multi-project workspace where two projects held the same relative SQL
    /// path. They must be diagnosed as duplicates, in both scheduling modes.
    #[test]
    fn duplicate_pass_names_are_diagnosed_not_reported_as_cycle() {
        let mut pm = PassManager::new();
        pm.register(Box::new(NamedPass("sql-analyzer:schema.sql", &[])));
        pm.register(Box::new(NamedPass("sql-analyzer:schema.sql", &[])));
        assert!(matches!(
            pm.execution_order(),
            Err(SchedulerError::DuplicatePassName(name)) if name == "sql-analyzer:schema.sql"
        ));
        assert!(matches!(
            pm.execution_levels(),
            Err(SchedulerError::DuplicatePassName(_))
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
            fn name(&self) -> &str {
                "fail"
            }
            async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
                Err(PassError::failed("intentional"))
            }
        }

        let mut pm = PassManager::new();
        pm.register(Box::new(FailingPass));
        pm.register(Box::new(NamedPass("ok", &[])));

        let config = Arc::new(EkosConfig::default());
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = PassContext::new(config, dir.path().to_path_buf());
        let report = pm.run_all(&mut ctx, FailureMode::Collect).await.unwrap();
        assert_eq!(report.passes_run(), 2);
    }

    #[tokio::test]
    async fn fail_fast_stops_after_first_error() {
        struct FailingPass;
        #[async_trait]
        impl CompilerPass for FailingPass {
            fn name(&self) -> &str {
                "fail"
            }
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
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = PassContext::new(config, dir.path().to_path_buf());
        let report = pm.run_all(&mut ctx, FailureMode::FailFast).await.unwrap();
        assert_eq!(report.passes_run(), 1);
        assert!(report.has_errors());
    }

    #[test]
    fn execution_levels_groups_independent_passes_together() {
        let mut pm = PassManager::new();
        pm.register(Box::new(NamedPass("A", &[])));
        pm.register(Box::new(NamedPass("B", &[])));
        pm.register(Box::new(NamedPass("C", &["A", "B"])));
        let levels = pm.execution_levels().unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], vec!["A".to_string(), "B".to_string()]);
        assert_eq!(levels[1], vec!["C".to_string()]);
    }

    #[test]
    fn execution_levels_detects_cycle() {
        let mut pm = PassManager::new();
        pm.register(Box::new(NamedPass("A", &["B"])));
        pm.register(Box::new(NamedPass("B", &["A"])));
        assert!(matches!(
            pm.execution_levels(),
            Err(SchedulerError::CycleDetected(_))
        ));
    }

    #[tokio::test]
    async fn run_all_parallel_overlaps_independent_passes() {
        use std::time::Instant;

        struct TimedPass {
            name: &'static str,
            start: Arc<std::sync::Mutex<Option<Instant>>>,
        }

        #[async_trait]
        impl CompilerPass for TimedPass {
            fn name(&self) -> &str {
                self.name
            }
            async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
                *self.start.lock().unwrap() = Some(Instant::now());
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                Ok(())
            }
        }

        let starts: Vec<Arc<std::sync::Mutex<Option<Instant>>>> = (0..3)
            .map(|_| Arc::new(std::sync::Mutex::new(None)))
            .collect();

        let mut pm = PassManager::new();
        pm.register(Box::new(TimedPass {
            name: "p1",
            start: starts[0].clone(),
        }));
        pm.register(Box::new(TimedPass {
            name: "p2",
            start: starts[1].clone(),
        }));
        pm.register(Box::new(TimedPass {
            name: "p3",
            start: starts[2].clone(),
        }));

        let config = Arc::new(EkosConfig::default());
        let dir = tempfile::tempdir().unwrap();
        let ctx = PassContext::new(config, dir.path().to_path_buf());
        let report = pm
            .run_all_parallel(&ctx, FailureMode::Collect)
            .await
            .unwrap();
        assert_eq!(report.passes_run(), 3);
        assert!(!report.has_errors());

        let times: Vec<Instant> = starts.iter().map(|s| s.lock().unwrap().unwrap()).collect();
        let earliest = times.iter().min().unwrap();
        for t in &times {
            assert!(
                t.duration_since(*earliest).as_millis() < 100,
                "independent passes should start within 100ms of each other"
            );
        }
    }
}
