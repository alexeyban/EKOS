use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub location: Option<SourceLocation>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, code: code.into(), message: message.into(), location: None }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, code: code.into(), message: message.into(), location: None }
    }

    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Info, code: code.into(), message: message.into(), location: None }
    }

    pub fn at(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }
}

/// Collects diagnostics emitted during a compilation run.
#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSink {
    pub fn emit(&mut self, d: Diagnostic) {
        tracing::debug!(severity = ?d.severity, code = %d.code, "{}", d.message);
        self.diagnostics.push(d);
    }

    pub fn error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.emit(Diagnostic::error(code, message));
    }

    pub fn warning(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.emit(Diagnostic::warning(code, message));
    }

    pub fn info(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.emit(Diagnostic::info(code, message));
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }

    pub fn has_warnings(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Warning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_collects_and_filters() {
        let mut sink = DiagnosticSink::default();
        sink.warning("W001", "minor issue");
        sink.warning("W002", "another warning");
        sink.error("E001", "fatal problem");

        assert!(!sink.has_errors() == false);
        assert!(sink.has_errors());
        assert_eq!(sink.errors().count(), 1);
        assert_eq!(sink.diagnostics().len(), 3);
    }
}
