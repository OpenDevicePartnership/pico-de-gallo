use clap::Parser;
use color_eyre::Result;
use pico_de_gallo_tool::Cli;

fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse().run()
}
