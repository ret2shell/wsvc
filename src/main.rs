#[cfg(feature = "cli")]
mod cli;

#[cfg(feature = "cli")]
use colored::Colorize;

#[tokio::main]
async fn main() {
    match cli::run().await {
        Ok(_) => {},
        Err(e) => {
            eprintln!("{}", e.to_string().red());
            std::process::exit(-1);
        }
    }    
}
