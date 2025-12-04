//! Typed error handling for deadmod.
//!
//! Provides structured errors that library consumers can match on,
//! with full context about what went wrong and where.

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for deadmod operations.
///
/// This provides typed errors that library consumers can match on,
/// unlike opaque `anyhow::Error` types.
#[derive(Error, Debug)]
pub enum DeadmodError {
    /// I/O error when reading/writing files
    #[error("I/O error at {path}: {message}")]
    Io {
        path: PathBuf,
        message: String,
        #[source]
        source: Option<std::io::Error>,
    },

    /// Syntax error when parsing Rust source
    #[error("Parse error in {path}: {message}")]
    Parse {
        path: PathBuf,
        message: String,
        /// Line number (1-indexed) if available
        line: Option<usize>,
        /// Column number (1-indexed) if available
        column: Option<usize>,
    },

    /// Cache-related errors
    #[error("Cache error: {message}")]
    Cache { message: String },

    /// Configuration file errors
    #[error("Config error at {path}: {message}")]
    Config { path: PathBuf, message: String },

    /// Workspace/crate structure errors
    #[error("Workspace error at {path}: {message}")]
    Workspace { path: PathBuf, message: String },

    /// Fix operation errors
    #[error("Fix error: {message}")]
    Fix { message: String },

    /// Invalid argument provided
    #[error("Invalid argument: {message}")]
    InvalidArgument { message: String },

    /// Path traversal or security error
    #[error("Security error: {message}")]
    Security { message: String },

    /// Generic internal error
    #[error("Internal error: {message}")]
    Internal { message: String },
}

impl DeadmodError {
    /// Create an I/O error with path context.
    pub fn io(path: impl Into<PathBuf>, err: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            message: err.to_string(),
            source: Some(err),
        }
    }

    /// Create a parse error with location.
    pub fn parse(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Parse {
            path: path.into(),
            message: message.into(),
            line: None,
            column: None,
        }
    }

    /// Create a parse error with line/column info.
    pub fn parse_at(
        path: impl Into<PathBuf>,
        message: impl Into<String>,
        line: usize,
        column: usize,
    ) -> Self {
        Self::Parse {
            path: path.into(),
            message: message.into(),
            line: Some(line),
            column: Some(column),
        }
    }

    /// Create a cache error.
    pub fn cache(message: impl Into<String>) -> Self {
        Self::Cache {
            message: message.into(),
        }
    }

    /// Create a config error.
    pub fn config(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Config {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create a workspace error.
    pub fn workspace(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Workspace {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create a fix error.
    pub fn fix(message: impl Into<String>) -> Self {
        Self::Fix {
            message: message.into(),
        }
    }

    /// Create a security error.
    pub fn security(message: impl Into<String>) -> Self {
        Self::Security {
            message: message.into(),
        }
    }

    /// Check if this is a recoverable error (can continue analysis).
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Parse { .. } | Self::Cache { .. } | Self::Config { .. }
        )
    }

    /// Get the path associated with this error, if any.
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Io { path, .. } => Some(path),
            Self::Parse { path, .. } => Some(path),
            Self::Config { path, .. } => Some(path),
            Self::Workspace { path, .. } => Some(path),
            _ => None,
        }
    }
}

/// Convenience type alias for deadmod results.
pub type DeadmodResult<T> = Result<T, DeadmodError>;

/// Extension trait for converting std::io::Error with path context.
pub trait IoResultExt<T> {
    /// Add path context to an I/O error.
    fn with_path(self, path: impl Into<PathBuf>) -> DeadmodResult<T>;
}

impl<T> IoResultExt<T> for std::io::Result<T> {
    fn with_path(self, path: impl Into<PathBuf>) -> DeadmodResult<T> {
        self.map_err(|e| DeadmodError::io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error() {
        let err = DeadmodError::io(
            PathBuf::from("/test/file.rs"),
            std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        );
        assert!(matches!(err, DeadmodError::Io { .. }));
        assert_eq!(err.path(), Some(&PathBuf::from("/test/file.rs")));
        assert!(err.to_string().contains("/test/file.rs"));
    }

    #[test]
    fn test_parse_error_with_location() {
        let err = DeadmodError::parse_at("/src/lib.rs", "unexpected token", 10, 5);
        if let DeadmodError::Parse { line, column, .. } = &err {
            assert_eq!(*line, Some(10));
            assert_eq!(*column, Some(5));
        } else {
            panic!("Expected Parse error");
        }
    }

    #[test]
    fn test_is_recoverable() {
        assert!(DeadmodError::parse("/test.rs", "error").is_recoverable());
        assert!(DeadmodError::cache("stale").is_recoverable());
        assert!(!DeadmodError::security("path traversal").is_recoverable());
    }

    #[test]
    fn test_io_result_ext() {
        let result: std::io::Result<()> =
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        let deadmod_result = result.with_path("/missing/file.rs");
        assert!(deadmod_result.is_err());
    }
}
