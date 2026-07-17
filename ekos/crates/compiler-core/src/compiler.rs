use std::{path::PathBuf, sync::Arc};
use thiserror::Error;

use crate::{
    config::EkosConfig,
    pass::{CompilerPass, PassContext, SchedulerError},
    scheduler::{ExecutionReport, FailureMode, Scheduler},
};

#[derive(Debug, Error)]
pub enum CompilerError {
    #[error("dependency graph error: {0}")]
    Scheduler(#[from] SchedulerError),
    #[error("compilation failed with {0} error(s)")]
    Failed(usize),
}

pub struct Compiler {
    config: Arc<EkosConfig>,
    scheduler: Scheduler,
    cwd: PathBuf,
}

impl Compiler {
    pub fn new(config: EkosConfig, cwd: PathBuf) -> Self {
        Self {
            config: Arc::new(config),
            scheduler: Scheduler::new(FailureMode::Collect),
            cwd,
        }
    }

    pub fn with_failure_mode(mut self, mode: FailureMode) -> Self {
        self.scheduler.failure_mode = mode;
        self
    }

    pub fn register_pass(&mut self, pass: Box<dyn CompilerPass>) {
        self.scheduler.register(pass);
    }

    pub async fn run(&mut self) -> Result<ExecutionReport, CompilerError> {
        let mut ctx = PassContext::new(Arc::clone(&self.config), self.cwd.clone());
        let report = self.scheduler.run(&mut ctx).await?;

        if report.has_errors() {
            return Err(CompilerError::Failed(report.error_count()));
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::PassError;
    use async_trait::async_trait;

    struct NoopPass;

    #[async_trait]
    impl CompilerPass for NoopPass {
        fn name(&self) -> &str {
            "noop"
        }
        async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn zero_passes_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Compiler::new(EkosConfig::default(), dir.path().to_path_buf());
        assert!(c.run().await.is_ok());
    }

    #[tokio::test]
    async fn noop_pass_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Compiler::new(EkosConfig::default(), dir.path().to_path_buf());
        c.register_pass(Box::new(NoopPass));
        let report = c.run().await.unwrap();
        assert_eq!(report.passes_run(), 1);
    }
}
