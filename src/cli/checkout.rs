use std::path::PathBuf;

use colored::Colorize;
use wsvc::{fs::WsvcFsError, model::Repository, WsvcError};

use super::config::{get_config, Commit};

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
    let tips = "wsvc can't keep current workspace changes when you checkout to record.\n\ntips: you must `wsvc config set commit.auto_record [true/false]` to determine whether auto commit changes when checkout, if it set to false, unsaved changes will be abandoned.";
    let mut auto_record = false;
    let mut commit = Commit::default();
    if let Some(config) = config {
        config
            .commit
            .and_then(|commit| commit.auto_record.map(|a| (commit, a)))
            .map(|(c, a)| {
                auto_record = a;
                commit = c;
            })
            .ok_or(WsvcError::NeedConfiguring(tips.to_owned()))?;
    } else {
        return Err(WsvcError::NeedConfiguring(tips.to_owned()));
    }
    if auto_record {
        let record = repo
            .commit_record(
                &workspace,
                format!("{} BACKUP", commit.author.unwrap_or("DEFAULT".to_owned())),
                "auto backup by checkout",
            )
            .await
            .ok();
        if let Some(record) = record {
            let hash = record.hash.0.to_hex().to_string();
            println!(
                "Auto-backup created a record: {} ({})",
                hash[0..6].green().bold(),
                hash
            );
        }
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
                    "Record {} ({})\nAt: {} Author: {}\nMessage: {}\n",
                    &hash_str[0..6].bold(),
                    hash_str.dimmed(),
                    record.date.naive_local().to_string().yellow(),
                    record.author.bright_blue(),
                    record.message
                );
            }
            return Err(WsvcError::BadUsage(format!(
                "more than one record found for hash {}",
                hash
            )));
        }
        let record = repo.checkout_record(&records[0].hash, &workspace).await?;
        let hash = record.hash.0.to_hex().to_string();
        println!(
            "Checked-out record: {} ({})",
            hash[0..6].green().bold(),
            hash
        );
    } else {
        let latest_hash = repo
            .get_latest_record()
            .await?
            .ok_or(WsvcError::BadUsage("no record found".to_owned()))?
            .hash;
        let record = repo.checkout_record(&latest_hash, &workspace).await?;
        let hash = record.hash.0.to_hex().to_string();
        println!(
            "Checked-out latest record: {} ({})",
            hash[0..6].green().bold(),
            hash
        );
    }
    Ok(())
}
