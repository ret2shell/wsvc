use dirs::config_local_dir;
use serde::{Deserialize, Serialize};
use tokio::fs::{create_dir_all, read_to_string, write};
use wsvc::{fs::WsvcFsError, WsvcError};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    pub commit: Option<Commit>,
    pub auth: Option<Auth>,
}

impl Config {
    fn merge(&self, other: Config) -> Self {
        Self {
            commit: other.commit.or(self.commit.clone()),
            auth: other.auth.or(self.auth.clone()),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Commit {
    pub author: Option<String>,
    pub auto_record: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Auth {
    pub account: Option<String>,
    pub password: Option<String>,
}

pub async fn get_config() -> Result<Option<Config>, WsvcError> {
    let local_config = get_local_config().await?;
    let global_config = get_global_config().await?;

    if let Some(global_config) = global_config {
        if let Some(local_config) = local_config {
            Ok(Some(global_config.merge(local_config)))
        } else {
            Ok(Some(global_config))
        }
    } else {
        Ok(local_config)
    }
}

pub async fn get_local_config() -> Result<Option<Config>, WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let current_config = pwd.join(".wsvc/config.toml");
    let current_config = read_to_string(current_config)
        .await
        .unwrap_or("".to_owned());
    Ok(toml::from_str(&current_config).ok())
}

pub async fn get_global_config() -> Result<Option<Config>, WsvcError> {
    let env_config = config_local_dir().ok_or(WsvcError::FsError(WsvcFsError::UnknownPath(
        "$HOME/.config".to_owned(),
    )))?;
    let global_config = env_config.join("wsvc/config.toml");
    let global_config = read_to_string(global_config).await.unwrap_or("".to_owned());
    Ok(toml::from_str(&global_config).ok())
}

pub async fn set_local_config(config: Config) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    let current_config = pwd.join(".wsvc/config.toml");
    write(current_config, toml::to_string(&config)?)
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    Ok(())
}

pub async fn set_global_config(config: Config) -> Result<(), WsvcError> {
    let env_config = config_local_dir().ok_or(WsvcError::FsError(WsvcFsError::UnknownPath(
        "$HOME/.config".to_owned(),
    )))?;
    let global_config_path = env_config.join("wsvc");
    if !global_config_path.exists() {
        create_dir_all(&global_config_path)
            .await
            .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    }
    let global_config = global_config_path.join("config.toml");
    write(global_config, toml::to_string(&config)?)
        .await
        .map_err(|err| WsvcError::FsError(WsvcFsError::Os(err)))?;
    Ok(())
}

pub async fn get(key: String) -> Result<(), WsvcError> {
    let config = get_config().await?;
    match config {
        Some(config) => match key.as_str() {
            "commit.author" => {
                if let Some(commit) = config.commit {
                    if let Some(author) = commit.author {
                        println!("{}", author);
                    } else {
                        return Err(WsvcError::LackOfConfig(
                            "commit.author".to_owned(),
                            "wsvc config set commit.author [--global]".to_owned(),
                        ));
                    }
                } else {
                    return Err(WsvcError::LackOfConfig(
                        "commit.author".to_owned(),
                        "wsvc config set commit.author [--global]".to_owned(),
                    ));
                }
            }
            "commit.auto_record" => {
                if let Some(commit) = config.commit {
                    if let Some(auto_record) = commit.auto_record {
                        println!("{}", auto_record);
                    } else {
                        return Err(WsvcError::LackOfConfig(
                            "commit.auto_record".to_owned(),
                            "wsvc config set commit.auto_record [--global]".to_owned(),
                        ));
                    }
                } else {
                    return Err(WsvcError::LackOfConfig(
                        "commit.auto_record".to_owned(),
                        "wsvc config set commit.auto_record [--global]".to_owned(),
                    ));
                }
            }
            "auth.account" => {
                if let Some(auth) = config.auth {
                    if let Some(account) = auth.account {
                        println!("{}", account);
                    } else {
                        return Err(WsvcError::LackOfConfig(
                            "auth.account".to_owned(),
                            "wsvc config set auth.account [--global]".to_owned(),
                        ));
                    }
                } else {
                    return Err(WsvcError::LackOfConfig(
                        "auth.account".to_owned(),
                        "wsvc config set auth.account [--global]".to_owned(),
                    ));
                }
            }
            "auth.password" => {
                if let Some(auth) = config.auth {
                    if let Some(password) = auth.password {
                        println!("{}", password);
                    } else {
                        return Err(WsvcError::LackOfConfig(
                            "auth.password".to_owned(),
                            "wsvc config set auth.password [--global]".to_owned(),
                        ));
                    }
                } else {
                    return Err(WsvcError::LackOfConfig(
                        "auth.password".to_owned(),
                        "wsvc config set auth.password [--global]".to_owned(),
                    ));
                }
            }
            _ => return Err(WsvcError::BadUsage(format!("unknown config key: {}", key))),
        },
        None => {
            return Err(WsvcError::LackOfConfig(
                key,
                "wsvc config set commit.author [--global]".to_owned(),
            ))
        }
    }
    Ok(())
}

pub async fn set(key: String, value: String, global: bool) -> Result<(), WsvcError> {
    let config = if global {
        get_global_config().await?
    } else {
        get_local_config().await?
    };

    let mut config = config.unwrap_or_default();
    match key.as_str() {
        "commit.author" => {
            if config.commit.is_none() {
                config.commit = Some(Commit::default());
            }
            if let Some(commit) = config.commit.as_mut() {
                commit.author = Some(value);
            }
        }
        "commit.auto_record" => {
            if config.commit.is_none() {
                config.commit = Some(Commit::default());
            }
            if let Some(commit) = config.commit.as_mut() {
                commit.auto_record = Some(value.parse::<bool>().unwrap());
            }
        }
        "auth.account" => {
            if config.auth.is_none() {
                config.auth = Some(Auth::default());
            }
            if let Some(auth) = config.auth.as_mut() {
                auth.account = Some(value);
            }
        }
        "auth.password" => {
            if config.auth.is_none() {
                config.auth = Some(Auth::default());
            }
            if let Some(auth) = config.auth.as_mut() {
                auth.password = Some(value);
            }
        }
        _ => return Err(WsvcError::BadUsage(format!("unknown config key: {}", key))),
    }

    if global {
        set_global_config(config).await?;
    } else {
        set_local_config(config).await?;
    }
    Ok(())
}

pub async fn unset(key: String, global: bool) -> Result<(), WsvcError> {
    let config = if global {
        get_global_config().await?
    } else {
        get_local_config().await?
    };

    let mut config = config.unwrap_or_default();
    match key.as_str() {
        "commit.author" => {
            if config.commit.is_none() {
                config.commit = Some(Commit::default());
            }
            if let Some(commit) = config.commit.as_mut() {
                commit.author = None;
            }
        }
        "auth.account" => {
            if config.auth.is_none() {
                config.auth = Some(Auth::default());
            }
            if let Some(auth) = config.auth.as_mut() {
                auth.account = None;
            }
        }
        "auth.password" => {
            if config.auth.is_none() {
                config.auth = Some(Auth::default());
            }
            if let Some(auth) = config.auth.as_mut() {
                auth.password = None;
            }
        }
        _ => return Err(WsvcError::BadUsage(format!("unknown config key: {}", key))),
    }

    if global {
        set_global_config(config).await?;
    } else {
        set_local_config(config).await?;
    }
    Ok(())
}
