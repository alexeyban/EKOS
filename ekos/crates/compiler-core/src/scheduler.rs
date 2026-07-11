use crate::pass::PassError;

#[derive(Debug, Clone, Copy, Default)]
pub enum FailureMode {
    /// Stop after the first failing pass.
    #[default]
    FailFast,
    /// Run all passes, collecting errors.
    Collect,
}

#[derive(Debug)]
pub struct PassOutcome {
    pub pass_name: String,
    pub result: Result<(), PassError>,
    /// `true` if this pass was skipped because its cached output is still
    /// valid (Phase 13 — `should_recompute` returned `false`).
    pub skipped: bool,
}

impl PassOutcome {
    pub fn ran(pass_name: String, result: Result<(), PassError>) -> Self {
        Self { pass_name, result, skipped: false }
    }

    pub fn skipped(pass_name: String) -> Self {
        Self { pass_name, result: Ok(()), skipped: true }
    }
}

#[derive(Debug)]
pub struct ExecutionReport {
    pub outcomes: Vec<PassOutcome>,
}

impl ExecutionReport {
    pub fn has_errors(&self) -> bool {
        self.outcomes.iter().any(|o| o.result.is_err())
    }

    pub fn passes_run(&self) -> usize {
        self.outcomes.iter().filter(|o| !o.skipped).count()
    }

    pub fn passes_skipped(&self) -> usize {
        self.outcomes.iter().filter(|o| o.skipped).count()
    }

    pub fn error_count(&self) -> usize {
        self.outcomes.iter().filter(|o| o.result.is_err()).count()
    }

    pub fn error_outcomes(&self) -> impl Iterator<Item = &PassOutcome> {
        self.outcomes.iter().filter(|o| o.result.is_err())
    }
}

/// Wraps a PassManager with a configurable failure mode.
pub struct Scheduler {
    pub failure_mode: FailureMode,
    pub manager: crate::pass::PassManager,
}

impl Scheduler {
    pub fn new(failure_mode: FailureMode) -> Self {
        Self { failure_mode, manager: crate::pass::PassManager::new() }
    }

    pub fn register(&mut self, pass: Box<dyn crate::pass::CompilerPass>) {
        self.manager.register(pass);
    }

    pub async fn run(
        &mut self,
        ctx: &mut crate::pass::PassContext,
    ) -> Result<ExecutionReport, crate::pass::SchedulerError> {
        self.manager.run_all(ctx, self.failure_mode).await
    }

    /// Like `run`, but executes DAG-independent passes concurrently (Phase 13).
    pub async fn run_parallel(
        &mut self,
        ctx: &crate::pass::PassContext,
    ) -> Result<ExecutionReport, crate::pass::SchedulerError> {
        self.manager.run_all_parallel(ctx, self.failure_mode).await
    }
}
