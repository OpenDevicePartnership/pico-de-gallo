//! `gallo-mcp` binary entry point.

use clap::Parser;
use gallo_mcp::GalloMcp;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

/// MCP server bridging AI agents to a Pico de Gallo USB device.
#[derive(Debug, Parser)]
#[command(name = "gallo-mcp", version, about)]
struct Cli {
    /// Connect to the device with this USB serial number (default: first match).
    #[arg(short, long)]
    serial_number: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let service = GalloMcp::new(cli.serial_number.as_deref());

    let server = service.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
