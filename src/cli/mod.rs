use clap::{command, Parser};
use wsvc::WsvcError;

mod checkout;
mod commit;
mod config;
mod create;
mod logs;
mod transport;

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
    /// manage global/repository config
    Config {
        #[clap(subcommand)]
        subcmd: ConfigSubCmd,
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
        WsvcCli::Config { subcmd } => match subcmd {
            ConfigSubCmd::Get { key } => config::get(key).await,
            ConfigSubCmd::Set { key, value, global } => {
                config::set(key, value, global.unwrap_or(false)).await
            }
            ConfigSubCmd::Unset { key, global } => {
                config::unset(key, global.unwrap_or(false)).await
            }
        },
    }
}
