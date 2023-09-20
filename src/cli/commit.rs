use std::path::PathBuf;

use colored::Colorize;
use wsvc::{fs::WsvcFsError, model::Repository, WsvcError};

use super::config::get_config;

pub async fn commit(
    message: String,
    author: Option<String>,
    workspace: Option<String>,
    root: Option<String>,
) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .map_err(WsvcFsError::Os)?
        .to_str()
        .unwrap()
        .to_string();
    let workspace = PathBuf::from(workspace.unwrap_or(pwd.clone()));
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root).await?;
    if repo.path == workspace {
        return Err(WsvcError::BadUsage(
            "workspace and repo path can not be the same".to_owned(),
        ));
    }
    let record = repo
        .commit_record(
            &workspace,
            &author.unwrap_or(
                get_config()
                    .await?
                    .ok_or(WsvcError::LackOfConfig(
                        "commit.author".to_owned(),
                        "wsvc config set commit.author [--global]".to_owned(),
                    ))?
                    .commit
                    .ok_or(WsvcError::LackOfConfig(
                        "commit.author".to_owned(),
                        "wsvc config set commit.author [--global]".to_owned(),
                    ))?
                    .author
                    .ok_or(WsvcError::LackOfConfig(
                        "commit.author".to_owned(),
                        "wsvc config set commit.author [--global]".to_owned(),
                    ))?,
            ),
            &message,
        )
        .await?;
    let hash = record.hash.0.to_hex().to_string();
    println!("Committed record: {} ({})", hash[0..6].green().bold(), hash);
    Ok(())
}
