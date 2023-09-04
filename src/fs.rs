use thiserror::Error;

#[derive(Error, Debug)]
pub enum WsvcFsError {
    #[error("repo path is invalid (not exists or not a directory)")]
    InvalidRepoPath,
    #[error("unknown fs error")]
    Unknown,
}
