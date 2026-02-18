use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sysfs read failed: {path}: {source}")]
    SysfsRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("sysfs write failed: {path}: {source}")]
    SysfsWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("parse error for {path}: {detail}")]
    Parse { path: PathBuf, detail: String },

    #[error("hardware detection failed: {0}")]
    Detection(String),

    #[error("not running as root (required for {operation})")]
    NotRoot { operation: String },

    #[error("conflicting service detected: {0}")]
    ConflictingService(String),

    #[error("state file error: {0}")]
    State(String),

    #[error("bootloader config error: {0}")]
    Bootloader(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
