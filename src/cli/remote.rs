use wsvc::{fs::WsvcFsError, model::Repository, WsvcError};

pub async fn remote_set(root: Option<String>, url: String) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .map_err(WsvcFsError::Os)?
        .to_str()
        .unwrap()
        .to_string();
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root).await?;
    repo.write_origin(url).await?;
    Ok(())
}
