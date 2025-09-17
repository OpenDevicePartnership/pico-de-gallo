use clap::Parser;
use color_eyre::Result;
use gallo::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse().run().await
}
