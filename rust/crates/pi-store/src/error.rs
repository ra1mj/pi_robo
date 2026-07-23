use std::path::Path;

/// Stable failure categories emitted by data and resource boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreErrorCategory {
    Io,
    InvalidJson,
    InvalidShape,
    InvalidPath,
    UnsupportedVersion,
    Authentication,
    UnsupportedOauth,
    CommandFailed,
    Timeout,
    Cancelled,
    OutputLimit,
    StaleSession,
}

/// Structured storage/configuration failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreError {
    pub category: StoreErrorCategory,
    pub message: String,
    pub path: Option<String>,
    pub line: Option<usize>,
}

impl StoreError {
    #[must_use]
    pub fn new(category: StoreErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            path: None,
            line: None,
        }
    }

    #[must_use]
    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().display().to_string());
        self
    }

    #[must_use]
    pub const fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    pub fn io(error: std::io::Error, path: impl AsRef<Path>) -> Self {
        Self::new(StoreErrorCategory::Io, error.to_string()).with_path(path)
    }

    pub(crate) fn json(error: serde_json::Error, path: impl AsRef<Path>) -> Self {
        let line = error.line();
        Self::new(StoreErrorCategory::InvalidJson, error.to_string())
            .with_path(path)
            .with_line(line)
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(path) = &self.path {
            write!(formatter, " ({path}")?;
            if let Some(line) = self.line {
                write!(formatter, ":{line}")?;
            }
            formatter.write_str(")")?;
        }
        Ok(())
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    pub path: Option<String>,
    pub line: Option<usize>,
}

impl StoreDiagnostic {
    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            path: None,
            line: None,
        }
    }

    #[must_use]
    pub fn error(error: &StoreError) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            message: error.message.clone(),
            path: error.path.clone(),
            line: error.line,
        }
    }

    #[must_use]
    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().display().to_string());
        self
    }
}
