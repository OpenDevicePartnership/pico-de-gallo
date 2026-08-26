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

## SPI chip-select preflight

`spi_batch` runs its steps in a fixed order: parse every operation
payload, connect exactly once, read the GPIO count from the `DeviceInfo`
that connection already validated, classify `cs`, then call the library
once. No second metadata query is issued.

Parsing precedes connecting on purpose. `connect` runs
`system_reset_subscriptions`, which tears down every GPIO subscription on
the board — including ones owned by other host processes — so a merely
malformed request must not reach it. A payload that fails to parse
returns `-32602` (invalid params) without opening the device at all.

Error codes stay disjoint:

- `-32602` — malformed payload, a `cs` at or beyond the reported GPIO
  count, or a device reporting zero GPIOs (its own distinct message).
  Nothing is transmitted and no pin is driven.
- `-32603` — the GPIO count could not be established: transport failure,
  a `device/info` timeout after 300 seconds, legacy firmware, or a schema
  mismatch. Never reported as an invalid argument.

## Choosing a board

Every tool that touches a device takes an optional `serial_number`, and every
device response is wrapped as `{ "serial_number": ..., "result": ... }`, naming
the board that served the call. (`list_devices` and `status` are not wrapped:
one opens no board, the other already reports a serial of its own.) With one
board attached you can omit the argument. With two or more, omitting it is an
error that lists the available serials rather than a silent guess:

```text
Multiple Pico de Gallo devices attached; `serial_number` is required.
Available: 5256657D8A5D7F03, 568E9AAEC72B0D49
```

`--serial-number` pins the server to one board, which then refuses any tool
call naming a different one — use it to scope an agent session to a single
board. A pinned server makes `serial_number` optional again however many
boards are attached. Call `list_devices` to see what is attached and whether a
serial is required. A pin that names no attached board is logged as a warning
at startup rather than refused, so that starting with no board attached keeps
working. That warning is on by default (on stderr, never on the JSON-RPC
stream); `RUST_LOG` still overrides verbosity when set.

Calls to different boards run concurrently; calls to the same board queue, so
a long `gpio_wait_*` holds only the board it addressed.

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
