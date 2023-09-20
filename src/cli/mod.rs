use clap::{command, Parser};
use wsvc::WsvcError;

mod checkout;
mod create;
mod commit;
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
    }
}
