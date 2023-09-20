use colored::Colorize;
use wsvc::{model::Repository, WsvcError, fs::WsvcFsError};

pub async fn logs(
    root: Option<String>,
    skip: Option<usize>,
    limit: Option<usize>,
) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .map_err(WsvcFsError::Os)?
        .to_str()
        .unwrap()
        .to_string();
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root).await?;
    let skip = skip.unwrap_or(0);
    let limit = limit.unwrap_or(10);
    let mut records = repo.get_records().await?;
    records.sort_by(|a, b| b.date.cmp(&a.date));
    let head_record = repo.get_head_record().await?;
    let latest_record = repo.get_latest_record().await?;
    let head_hash = head_record.map(|r| r.hash).unwrap_or_default();
    let latest_hash = latest_record.map(|r| r.hash).unwrap_or_default();
    for record in records.iter().skip(skip).take(limit) {
        let hash_str = record.hash.0.to_hex().to_ascii_lowercase();
        let cursor = if head_hash == record.hash || latest_hash == record.hash {
            format!(
                "<== {}{}",
                if head_hash == record.hash {
                    "[HEAD]".bright_green().bold()
                } else {
                    "".clear()
                },
                if latest_hash == record.hash {
                    "[LATEST]".bright_blue().bold()
                } else {
                    "".clear()
                }
            )
        } else {
            "".to_owned()
        };
        println!(
            "Record {} ({}) {}\nAt: {} Author: {}\nMessage: {}\n",
            &hash_str[0..6].bold(),
            hash_str.dimmed(),
            cursor,
            record.date.naive_local().to_string().yellow(),
            record.author.bright_blue(),
            record.message
        );
    }
    Ok(())
}
