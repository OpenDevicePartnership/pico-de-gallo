# gallo-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server that
exposes a [Pico de Gallo](https://github.com/OpenDevicePartnership/pico-de-gallo)
USB bridge to AI coding agents. It wraps
[`pico-de-gallo-lib`](https://crates.io/crates/pico-de-gallo-lib) and presents
one MCP tool per peripheral operation (I²C, SPI, UART, GPIO, PWM, ADC, 1-Wire)
over stdio, so an agent can probe and drive real hardware while writing an
embedded driver.

## Install

```bash
cargo install gallo-mcp
```

## Run

```bash
gallo-mcp [--serial-number <SN>]
```

The server holds no persistent connection to the board. Each tool call opens
the device, runs the operation, and releases it when the call completes, so
the board stays free for the `gallo` CLI or other host processes between
calls. Each call re-opens and re-validates the device, so there is a small
fixed per-call connection cost. The server starts even with no board
attached; tools begin working as soon as a Pico de Gallo is plugged in. Logs
go to stderr; stdout carries the
MCP protocol.

## Use with an MCP client

Add it to your client's MCP configuration. For example, opencode
(`opencode.json`, safe to commit per-project):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "pico-de-gallo": { "type": "local", "command": ["gallo-mcp"], "enabled": true }
  }
}
```

## Security

Tools that write to a bus or actuate pins are annotated `destructiveHint`;
read-only tools are annotated `readOnlyHint`. `gallo-mcp` does **not** gate
writes itself — approval is delegated to the MCP client. Configure your
client's permission system (e.g. opencode `permission`) to prompt before
destructive tool calls. Under a client with no permission system, an agent can
actuate hardware without confirmation.

See the [book chapter](https://opendevicepartnership.github.io/pico-de-gallo/crates/mcp.html)
for the full tool catalog and details.
