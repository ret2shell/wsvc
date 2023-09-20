use std::{path::PathBuf, process::exit};

use clap::{command, Parser};
use tokio::fs::read;
use wsvc::model::Repository;

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

pub async fn run() {
    let cli = WsvcCli::parse();
    match cli {
        WsvcCli::Commit {
            message,
            author,
            workspace,
            root,
        } => {
            commit(message, author, workspace, root).await;
        }
        WsvcCli::Checkout {
            hash,
            workspace,
            root,
        } => {
            checkout(hash, workspace, root).await;
        }
        WsvcCli::Init { bare } => {
            init(bare).await;
        }
        WsvcCli::New { name, bare } => {
            new(name, bare).await;
        }
        WsvcCli::Logs { root, skip, limit } => {
            logs(root, skip, limit).await;
        }
    }
}

async fn commit(
    message: String,
    author: Option<String>,
    workspace: Option<String>,
    root: Option<String>,
) {
    let pwd = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let workspace = PathBuf::from(workspace.unwrap_or(pwd.clone()));
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root)
        .await
        .expect("failed to open repo");
    if repo.path == workspace {
        println!("workspace and repo path can not be the same");
        exit(-1);
    }
    repo.commit_record(
        &workspace,
        &author.unwrap_or(String::from("UNKNOWN")),
        &message,
    )
    .await
    .expect("failed to commit");
}

async fn checkout(hash: Option<String>, workspace: Option<String>, root: Option<String>) {
    let pwd = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let workspace = PathBuf::from(workspace.unwrap_or(pwd.clone()));
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root)
        .await
        .expect("failed to open repo");
    if repo.path == workspace {
        println!("workspace and repo path can not be the same");
        exit(-1);
    }
    if let Some(hash) = hash {
        let hash = hash.to_ascii_lowercase();
        let records = repo.get_records().await.expect("failed to get records");
        let records = records
            .iter()
            .filter(|h| h.hash.0.to_hex().to_ascii_lowercase().starts_with(&hash))
            .collect::<Vec<_>>();
        if records.len() == 0 {
            println!("no commit found");
            exit(-1);
        }
        if records.len() > 1 {
            println!("more than one commit found:");
            for record in records.iter() {
                println!(
                    "Record {:?} by {}\nAt: {}\nMessage: {}\n",
                    record.hash, record.author, record.date, record.message
                );
            }
            exit(-1);
        }
        repo.checkout_record(&&records[0].hash, &workspace)
            .await
            .expect("failed to checkout record");
    } else {
        let head_hash = read(repo.path.join("HEAD"))
            .await
            .expect("failed to read head");
        if String::from_utf8(head_hash.clone()).expect("failed to parse HEAD") == "".to_owned() {
            println!("no commit found");
            exit(-1);
        }
        repo.checkout_record(
            &String::from_utf8(head_hash)
                .expect("failed to parse HEAD")
                .try_into()
                .expect("failed to parse hash"),
            &workspace,
        )
        .await
        .expect("failed to checkout record");
    }
}

async fn init(bare: Option<bool>) {
    let pwd = std::env::current_dir().expect("failed to get current dir");
    let bare = bare.unwrap_or(false);
    Repository::new(&pwd, bare)
        .await
        .expect("failed to init repo");
}

async fn new(name: String, bare: Option<bool>) {
    let pwd = std::env::current_dir().expect("failed to get current dir");
    let bare = bare.unwrap_or(false);
    Repository::new(&pwd.join(&name), bare)
        .await
        .expect("failed to init repo");
}

async fn logs(root: Option<String>, skip: Option<usize>, limit: Option<usize>) {
    let pwd = std::env::current_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let root = root.unwrap_or(pwd);
    let repo = Repository::try_open(root)
        .await
        .expect("failed to open repo");
    let skip = skip.unwrap_or(0);
    let limit = limit.unwrap_or(10);
    let mut records = repo.get_records().await.expect("failed to get records");
    records.sort_by(|a, b| b.date.cmp(&a.date));
    for record in records.iter().skip(skip).take(limit) {
        println!(
            "Record {:?} by {}\nAt: {}\nMessage: {}\n",
            record.hash, record.author, record.date, record.message
        );
    }
}
