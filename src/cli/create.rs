use wsvc::{model::Repository, fs::WsvcFsError, WsvcError};


pub async fn init(bare: Option<bool>) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(WsvcFsError::Os)?;
    let bare = bare.unwrap_or(false);
    Repository::new(&pwd, bare).await?;
    Ok(())
}

pub async fn new(name: String, bare: Option<bool>) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(WsvcFsError::Os)?;
    let bare = bare.unwrap_or(false);
    Repository::new(&pwd.join(&name), bare).await?;
    Ok(())
}