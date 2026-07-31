//! Error types and process exit codes for mcp2cli.
//!
//! Exit codes: `2` = usage/args, `1` = runtime/tool failure.

use std::fmt;

use thiserror::Error;

/// Process exit code for CLI failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Runtime = 1,
    Usage = 2,
}

impl From<ExitCode> for i32 {
    fn from(value: ExitCode) -> Self {
        value as i32
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Usage(String),

    #[error("{0}")]
    Runtime(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
}

impl Error {
    pub fn usage(msg: impl Into<String>) -> Self {
        Self::Usage(msg.into())
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => ExitCode::Usage.into(),
            _ => ExitCode::Runtime.into(),
        }
    }
}

/// Display with anyhow-style `:#` context via Debug for nested sources.
pub struct Report<'a>(pub &'a Error);

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
