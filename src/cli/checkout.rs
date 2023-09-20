use std::path::PathBuf;

use colored::Colorize;
use wsvc::{fs::WsvcFsError, model::Repository, WsvcError};

pub async fn checkout(
    hash: Option<String>,
    workspace: Option<String>,
    root: Option<String>,
) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .map_err(|err| WsvcFsError::Os(err))?
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
    if let Some(hash) = hash {
        let hash = hash.to_ascii_lowercase();
        let records = repo.get_records().await?;
        let records = records
            .iter()
            .filter(|h| h.hash.0.to_hex().to_ascii_lowercase().starts_with(&hash))
            .collect::<Vec<_>>();
        if records.len() == 0 {
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
                    format!(
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
        repo.checkout_record(&&records[0].hash, &workspace).await?;
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
