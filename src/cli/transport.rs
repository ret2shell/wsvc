use wsvc::WsvcError;

pub async fn clone(_url: String, _dir: Option<String>) -> Result<(), WsvcError> {
    Ok(())
}

pub async fn sync() -> Result<(), WsvcError> {
    Ok(())
}
