//! `gallo-mcp` binary entry point.

use clap::Parser;
use gallo_mcp::GalloMcp;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

/// MCP server bridging AI agents to a Pico de Gallo USB device.
#[derive(Debug, Parser)]
#[command(name = "gallo-mcp", version, about)]
struct Cli {
    /// Pin the server to the board with this USB serial number
    ///
    /// Tool calls that omit serial_number use the pinned board, and a call
    /// naming a different board is refused.
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
    // Before serving: a mistyped --serial-number otherwise starts a server
    // that works right up until the first device call and then fails every
    // one of them, with nothing on stderr to say why.
    service.warn_if_pin_unresolvable();

    let server = service.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
