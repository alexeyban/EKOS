pub mod cache;
pub mod compiler;
pub mod config;
pub mod diagnostics;
pub mod pass;
pub mod scheduler;

pub use cache::{config_hash, record_manifest, should_recompute};
pub use compiler::{Compiler, CompilerError};
pub use config::EkosConfig;
pub use diagnostics::{Diagnostic, DiagnosticSink, Severity, SourceLocation as DiagnosticLocation};
pub use pass::{CompilerPass, PassContext, PassError, PassManager, SchedulerError};
pub use scheduler::{ExecutionReport, FailureMode, PassOutcome, Scheduler};
