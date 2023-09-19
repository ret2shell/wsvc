#[cfg(feature = "cli")]
mod cli;

#[tokio::main]
async fn main() {
    cli::run().await;
}
