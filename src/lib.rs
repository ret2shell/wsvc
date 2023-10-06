use std::string::FromUtf8Error;
use thiserror::Error;
use toml::{de, ser};

pub mod fs;
pub mod model;
#[cfg(feature = "server")]
pub mod server;

/// Error type for wsvc
#[derive(Error, Debug)]
pub enum WsvcError {
    #[error("fs error: {0}")]
    FsError(#[from] fs::WsvcFsError),
    #[error("codec error: {0}")]
    CodecError(#[from] FromUtf8Error),
    #[error("bad usage: {0}")]
    BadUsage(String),
    #[error("repo error: {0}")]
    RepoError(String),
    #[error("config deserialize failed: {0}")]
    ConfigDeserializeFailed(#[from] de::Error),
    #[error("config serialize failed: {0}")]
    ConfigSerializeFailed(#[from] ser::Error),
    #[error("lack of config: {0}\n\ntips: {1}")]
    LackOfConfig(String, String),
    #[error("need configuring: {0}")]
    NeedConfiguring(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[cfg(feature = "cli")]
    #[error("network error: {0}")]
    NetworkError(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("data error: {0}")]
    DataError(String),
    #[error("repo without record")]
    EmptyRepoError,
}
