use std::path::PathBuf;

use colored::Colorize;
use wsvc::{fs::WsvcFsError, model::Repository, WsvcError};

use super::config::get_config;

pub async fn checkout(
    hash: Option<String>,
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
    let config = get_config().await?;
    if let Some(config) = config {
        if let Some(commit) = config.commit {
            if let Some(auto_record) = commit.auto_record {
                if auto_record {
                    let _ = repo.commit_record(
                        &workspace,
                        &commit.author.unwrap_or("BACKUP".to_owned()),
                        "AUTO BACKUP",
                    )
                    .await.ok();
                }
            } else {
                return Err(WsvcError::NeedConfiguring("wsvc can't keep current workspace changes when you checkout to record.\n\ntips: you must `wsvc config set commit.auto_record [true/false]` to determine whether auto commit changes when checkout, if it set to false, unsaved changes will be abandoned.".to_owned()));
            }
        } else {
            return Err(WsvcError::NeedConfiguring("wsvc can't keep current workspace changes when you checkout to record.\n\ntips: you must `wsvc config set commit.auto_record [true/false]` to determine whether auto commit changes when checkout, if it set to false, unsaved changes will be abandoned.".to_owned()));
        }
    } else {
        return Err(WsvcError::NeedConfiguring("wsvc can't keep current workspace changes when you checkout to record.\n\ntips: you must `wsvc config set commit.auto_record [true/false]` to determine whether auto commit changes when checkout, if it set to false, unsaved changes will be abandoned.".to_owned()));
    }
    if let Some(hash) = hash {
        let hash = hash.to_ascii_lowercase();
        let records = repo.get_records().await?;
        let records = records
            .iter()
            .filter(|h| h.hash.0.to_hex().to_ascii_lowercase().starts_with(&hash))
            .collect::<Vec<_>>();
        if records.is_empty() {
            return Err(WsvcError::BadUsage(format!(
                "no record found for hash {}",
                hash
            )));
        }
        if records.len() > 1 {
            println!("{}", "More than one record found:".bright_red());
            for record in records.iter() {
                let hash_str = record.hash.0.to_hex().to_ascii_lowercase();
                println!(
                    "{} At: {}\nMessage: {}\n",
                    format_args!(
                        "Record {} ({})\nAuthor: {}",
                        &hash_str[0..6].bold(),
                        hash_str.dimmed(),
                        record.author.bright_blue()
                    ),
                    record.date.naive_local().to_string().yellow(),
                    record.message
                );
            }
            return Err(WsvcError::BadUsage(format!(
                "more than one record found for hash {}",
                hash
            )));
        }
        repo.checkout_record(&records[0].hash, &workspace).await?;
    } else {
        let latest_hash = repo
            .get_latest_record()
            .await?
            .ok_or(WsvcError::BadUsage("no record found".to_owned()))?
            .hash;
        repo.checkout_record(&latest_hash, &workspace).await?;
    }
    Ok(())
}
