use std::path::PathBuf;

use clap::{command, Parser};
use colored::Colorize;
use tokio::fs::read;
use wsvc::{fs::WsvcFsError, model::Repository, WsvcError};

/// wsvc is a simple version control system.
#[derive(Parser)]
#[command(name = "wsvc")]
#[command(bin_name = "wsvc")]
enum WsvcCli {
    /// record a snapshot of workspace.
    #[command(name = "commit")]
    Commit {
        /// commit message
        #[clap(short, long)]
        message: String,
        /// commit author
        #[clap(short, long)]
        author: Option<String>,
        /// optional workspace dir, if not configured, current dir will be used
        #[clap(short, long)]
        workspace: Option<String>,
        /// optional root dir where stores the repo data, if not configured, current dir or .wsvc will be used
        #[clap(short, long)]
        root: Option<String>,
    },
    /// checkout a commit.
    #[command(name = "checkout")]
    Checkout {
        /// the aim commit hash
        hash: Option<String>,
        /// optional workspace dir, if not configured, current dir will be used
        #[clap(short, long)]
        workspace: Option<String>,
        /// optional root dir where stores the repo data, if not configured, current dir or .wsvc will be used
        #[clap(short, long)]
        root: Option<String>,
    },
    /// init a repo in current dir.
    #[command(name = "init")]
    Init {
        /// whether init this repo as bare repo. if false (default), a .wsvc dir will be created to store the repo data
        #[clap(short, long)]
        bare: Option<bool>,
    },
    /// create a new wsvc project repo.
    #[command(name = "new")]
    New {
        /// the new repo dir that will be created
        name: String,
        /// whether init this repo as bare repo
        #[clap(short, long)]
        bare: Option<bool>,
    },
    Logs {
        #[clap(short, long)]
        root: Option<String>,
        #[clap(short, long)]
        skip: Option<usize>,
        #[clap(short, long)]
        limit: Option<usize>,
    },
}

pub async fn run() -> Result<(), WsvcError> {
    let cli = WsvcCli::parse();
    match cli {
        WsvcCli::Commit {
            message,
            author,
            workspace,
            root,
        } => commit(message, author, workspace, root).await,
        WsvcCli::Checkout {
            hash,
            workspace,
            root,
        } => checkout(hash, workspace, root).await,
        WsvcCli::Init { bare } => init(bare).await,
        WsvcCli::New { name, bare } => new(name, bare).await,
        WsvcCli::Logs { root, skip, limit } => logs(root, skip, limit).await,
    }
}

async fn commit(
    message: String,
    author: Option<String>,
    workspace: Option<String>,
    root: Option<String>,
) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .unwrap()
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
    repo.commit_record(
        &workspace,
        &author.unwrap_or(String::from("UNKNOWN")),
        &message,
    )
    .await?;
    Ok(())
}

async fn checkout(
    hash: Option<String>,
    workspace: Option<String>,
    root: Option<String>,
) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .unwrap()
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
                "no commit found for hash {}",
                hash
            )));
        }
        if records.len() > 1 {
            println!("{}", "More than one commit found:".bright_red());
            for record in records.iter() {
                println!(
                    "{}\nAt: {}\nMessage: {}\n",
                    format!("Record {:?} by {}", record.hash, record.author).bold(),
                    record.date,
                    record.message
                );
            }
            return Err(WsvcError::BadUsage(format!(
                "more than one commit found for hash {}",
                hash
            )));
        }
        repo.checkout_record(&&records[0].hash, &workspace).await?;
    } else {
        let head_hash = read(repo.path.join("HEAD"))
            .await
            .map_err(|err| WsvcFsError::Os(err))?;
        if String::from_utf8(head_hash.clone())? == "".to_owned() {
            return Err(WsvcError::BadUsage("no commit found for HEAD".to_owned()));
        }
        repo.checkout_record(
            &String::from_utf8(head_hash)?
                .try_into()
                .map_err(|err| WsvcFsError::InvalidHexString(err))?,
            &workspace,
        )
        .await?;
    }
    Ok(())
}

async fn init(bare: Option<bool>) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcFsError::Os(err))?;
    let bare = bare.unwrap_or(false);
    Repository::new(&pwd, bare).await?;
    Ok(())
}

async fn new(name: String, bare: Option<bool>) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir().map_err(|err| WsvcFsError::Os(err))?;
    let bare = bare.unwrap_or(false);
    Repository::new(&pwd.join(&name), bare).await?;
    Ok(())
}

async fn logs(
    root: Option<String>,
    skip: Option<usize>,
    limit: Option<usize>,
) -> Result<(), WsvcError> {
    let pwd = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root).await?;
    let skip = skip.unwrap_or(0);
    let limit = limit.unwrap_or(10);
    let mut records = repo.get_records().await?;
    records.sort_by(|a, b| b.date.cmp(&a.date));
    for record in records.iter().skip(skip).take(limit) {
        println!(
            "{}\nAt: {}\nMessage: {}\n",
            format!("Record {:?} by {}", record.hash, record.author).bold(),
            record.date,
            record.message
        );
    }
    Ok(())
}
