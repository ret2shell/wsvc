use std::string::FromUtf8Error;
use toml::{de, ser};
use thiserror::Error;

pub mod model;
pub mod fs;
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
}
