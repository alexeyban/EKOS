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
        self.outcomes.len()
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
}
