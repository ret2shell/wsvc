use std::string::FromUtf8Error;

use thiserror::Error;

pub mod model;
pub mod fs;
#[cfg(feature = "server")]
pub mod server;

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
}
