use clap::Parser;
use color_eyre::Result;
use gallo::Cli;

fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse().run()
}
