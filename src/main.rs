#[cfg(feature = "cli")]
mod cli;

#[cfg(feature = "cli")]
use colored::Colorize;

#[tokio::main]
async fn main() {
    match cli::run().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{}: {}", "error".bright_red().bold(), e);
            std::process::exit(-1);
        }
    }
}
