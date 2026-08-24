use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("daemon not running")]
    DaemonNotRunning,
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error("{0}")]
    Other(String),
}
