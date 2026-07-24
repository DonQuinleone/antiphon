use std::fmt;
use std::path::PathBuf;

/// A configuration failure, attributed to a file and where
/// possible a line, with the nearest valid key when the
/// problem is an unknown key.
#[derive(Debug)]
pub struct ConfigError {
    pub file: PathBuf,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}", self.file.display())?;
        if let Some(line) = self.line {
            write!(out, ":{line}")?;
        }
        write!(out, ": {}", self.message)?;
        if let Some(key) = &self.suggestion {
            write!(out, " (nearest valid key: `{key}`)")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}
