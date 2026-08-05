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
    // MCP clients spawn this server with whatever environment they happen to
    // have, which in practice means no `RUST_LOG`. `from_default_env()` falls
    // back to a global ERROR floor in that case, so the pin warning below —
    // WARN — never reached the operator it exists to protect. Supply our own
    // fallback instead, used only when `RUST_LOG` is absent.
    //
    // `error,gallo_mcp=warn`, not a bare `warn`: a bare `warn` would also
    // enable warnings from rmcp, nusb and postcard-rpc onto the stderr the
    // client captures, and that noise has not been assessed. Keeping the
    // leading `error` preserves the global ERROR floor `from_default_env()`
    // already gave us, so this only ever *adds* the one warning rather than
    // trading it for the errors we used to surface. `gallo_mcp` has exactly
    // two tracing call sites, one `warn!` and one `debug!`, so at this level
    // the only thing it can newly emit is the pin warning itself.
    //
    // `try_from_default_env` errors only when `RUST_LOG` is unset or
    // unparseable, so a `RUST_LOG` that is set still wins outright.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error,gallo_mcp=warn")),
        )
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
