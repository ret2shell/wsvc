use clap::{command, Parser};
use wsvc::WsvcError;

mod checkout;
mod commit;
mod create;
mod logs;
mod remote;
mod transport;

/// wsvc is a simple version control system.
#[derive(Parser)]
#[command(name = "wsvc")]
#[command(bin_name = "wsvc")]
enum WsvcCli {
    /// record a snapshot of workspace.
    Commit {
        /// commit message
        #[clap(short, long)]
        message: String,
        /// commit author
        #[clap(short, long)]
        author: String,
        /// optional workspace dir, if not configured, current dir will be used
        #[clap(short, long)]
        workspace: Option<String>,
        /// optional root dir where stores the repo data, if not configured, current dir or .wsvc will be used
        #[clap(short, long)]
        root: Option<String>,
    },
    /// checkout a commit.
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
    Init {
        /// whether init this repo as bare repo. if false (default), a .wsvc dir will be created to store the repo data
        #[clap(short, long)]
        bare: Option<bool>,
    },
    /// create a new wsvc project repo.
    New {
        /// the new repo dir that will be created
        name: String,
        /// whether init this repo as bare repo
        #[clap(short, long)]
        bare: Option<bool>,
    },
    /// show records list
    Logs {
        /// optional root dir where stores the repo data, if not configured, current dir or .wsvc will be used
        #[clap(short, long)]
        root: Option<String>,
        /// skip records
        #[clap(short, long)]
        skip: Option<usize>,
        /// limit records that are shown
        #[clap(short, long)]
        limit: Option<usize>,
    },
    /// clone a repository
    Clone {
        /// the remote repository url
        url: String,
        /// the local repository dir
        dir: Option<String>,
    },
    /// sync a repository with remote origin
    Sync,
    /// set remote origin
    Remote {
        /// optional root dir where stores the repo data, if not configured, current dir or .wsvc will be used
        #[clap(short, long)]
        root: Option<String>,
        /// remote origin url
        url: String,
    },
}

#[derive(Parser)]
enum ConfigSubCmd {
    /// get config
    Get {
        /// config key
        key: String,
    },
    /// set config
    Set {
        /// config key
        key: String,
        /// config value
        value: String,
        /// whether set global config
        #[clap(short, long, action = clap::ArgAction::SetTrue)]
        global: Option<bool>,
    },
    /// unset config
    Unset {
        /// config key
        key: String,
        /// whether unset global config
        #[clap(short, long, action = clap::ArgAction::SetTrue)]
        global: Option<bool>,
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
        } => commit::commit(message, author, workspace, root).await,
        WsvcCli::Checkout {
            hash,
            workspace,
            root,
        } => checkout::checkout(hash, workspace, root).await,
        WsvcCli::Init { bare } => create::init(bare).await,
        WsvcCli::New { name, bare } => create::new(name, bare).await,
        WsvcCli::Logs { root, skip, limit } => logs::logs(root, skip, limit).await,
        WsvcCli::Clone { url, dir } => transport::clone(url, dir).await,
        WsvcCli::Sync => transport::sync().await,
        WsvcCli::Remote { root, url } => remote::remote_set(root, url).await,
    }
}
