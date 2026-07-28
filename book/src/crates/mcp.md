# gallo-mcp — MCP server

`gallo-mcp` bridges a Pico de Gallo to AI coding agents. It runs a
[Model Context Protocol](https://modelcontextprotocol.io/) server over
stdio, wrapping `pico-de-gallo-lib` and exposing one tool per peripheral
operation across I<sup>2</sup>C, SPI, UART, GPIO, PWM, ADC, and 1-Wire.

The point is the on-your-laptop development loop: while an agent writes
an embedded driver, it can probe and drive real hardware through the same
board `gallo` talks to — reading a sensor register, scanning a bus,
toggling a pin — without cross-compiling or flashing anything.

Use it for:

- letting an AI agent explore an unfamiliar device interactively,
- generating and validating register-level driver code against real
  silicon,
- turning "what does this chip return?" into a tool call the agent can
  make itself.

## Install

```console
$ cargo install gallo-mcp
```

## Run

```console
$ gallo-mcp [--serial-number <SN>]
```

`gallo-mcp` speaks the MCP protocol on **stdout** and logs to **stderr**,
so it is meant to be launched by an MCP client rather than run
interactively.

- `-s, --serial-number <SN>` selects a specific board by USB serial when
  more than one is attached.
- **Per-call connection:** the server holds no persistent USB claim. Each
  tool call opens the board, runs, and releases it when the call completes,
  so the device is free for the `gallo` CLI or other host processes between
  calls. Each call re-opens and re-validates the board, so there is a small
  fixed per-call connection cost. The server starts even with no board
  attached and tools begin working as soon as a Pico de Gallo is present;
  you can plug the board in mid-session.

## Using it with an MCP client

Add `gallo-mcp` as a local (stdio) server in your client's config. These
files are safe to commit per-project, so the tools appear only in repos
that opt in.

### opencode (`opencode.json`)

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "pico-de-gallo": { "type": "local", "command": ["gallo-mcp"], "enabled": true }
  }
}
```

### Claude Code (`.mcp.json`)

```json
{ "mcpServers": { "pico-de-gallo": { "command": "gallo-mcp", "args": [] } } }
```

### Cursor (`.cursor/mcp.json`)

```json
{ "mcpServers": { "pico-de-gallo": { "command": "gallo-mcp", "args": [] } } }
```

## Byte conventions

Byte payloads go in as **hex strings** and come back as both hex and a
decimal array. Input accepts comma-separated or bare hex (`"0x00,0x10"`
or `"0010"`); reads return `{ "hex": ..., "bytes": ... }`.

```json
// i2c_write_read {"address":72,"data":"0x00","count":2}
{ "hex": "0x0B,0xCF", "bytes": [11, 207] }
```

## Tool catalog

35 tools, grouped by peripheral. Read-only tools carry the
`readOnlyHint` annotation; write/actuation tools carry `destructiveHint`.

### device (read-only)

| Tool | Description |
|---|---|
| `list_devices` | List connected Pico de Gallo boards |
| `status` | Report attach state and firmware/schema version |
| `device_info` | Firmware version, schema version, capabilities |
| `version` | Firmware version |
| `ping` | Echo a value (liveness check) |

### i2c

| Tool | Description | Kind |
|---|---|---|
| `i2c_read` | Read bytes from a target address | read-only |
| `i2c_write_read` | Write then read without releasing the bus | read-only |
| `i2c_scan` | Probe the bus for responding addresses | read-only |
| `i2c_get_config` | Show the active I<sup>2</sup>C frequency | read-only |
| `i2c_write` | Write bytes to a target address | destructive |
| `i2c_set_config` | Set the I<sup>2</sup>C frequency | destructive |
| `i2c_batch` | Execute several I<sup>2</sup>C operations in one transfer | destructive |

### spi

| Tool | Description | Kind |
|---|---|---|
| `spi_read` | Clock in bytes | read-only |
| `spi_transfer` | Full-duplex transfer | read-only |
| `spi_get_config` | Show the active SPI configuration | read-only |
| `spi_write` | Clock out bytes | destructive |
| `spi_flush` | Flush the SPI buffer | destructive |
| `spi_set_config` | Set frequency, phase, and polarity | destructive |
| `spi_batch` | Atomic multi-step transaction under chip-select | destructive |

### uart

| Tool | Description | Kind |
|---|---|---|
| `uart_read` | Read bytes with a timeout | read-only |
| `uart_get_config` | Show the active UART configuration | read-only |
| `uart_write` | Write raw bytes | destructive |
| `uart_flush` | Drain the transmit buffer | destructive |
| `uart_set_config` | Set baud rate | destructive |

### gpio

| Tool | Description | Kind |
|---|---|---|
| `gpio_get` | Read the current level of a pin | read-only |
| `gpio_wait_for_rising_edge_with_timeout` | Wait for a rising edge (bounded) | read-only |
| `gpio_wait_for_falling_edge_with_timeout` | Wait for a falling edge (bounded) | read-only |
| `gpio_wait_for_any_edge_with_timeout` | Wait for any edge (bounded) | read-only |
| `gpio_put` | Drive a pin high or low | destructive |
| `gpio_set_config` | Set direction and pull resistor | destructive |

### pwm

| Tool | Description | Kind |
|---|---|---|
| `pwm_get_duty_cycle` | Read current and maximum duty | read-only |
| `pwm_get_config` | Show the active PWM configuration | read-only |
| `pwm_set_duty_cycle` | Set a raw duty-cycle value | destructive |
| `pwm_enable` | Enable the slice behind a channel | destructive |
| `pwm_disable` | Disable the slice behind a channel | destructive |
| `pwm_set_config` | Set frequency and phase-correct mode | destructive |

### adc (read-only)

| Tool | Description |
|---|---|
| `adc_read` | Read one ADC sample |
| `adc_get_config` | Show resolution, reference, and channel count |

### onewire

| Tool | Description | Kind |
|---|---|---|
| `onewire_read` | Read raw bytes | read-only |
| `onewire_search` | Enumerate ROM IDs on the bus | read-only |
| `onewire_reset` | Reset the bus and report presence | destructive |
| `onewire_write` | Write raw bytes | destructive |
| `onewire_write_pullup` | Write, then hold the line high (parasitic power) | destructive |

## GPIO waits and v1 limits

GPIO edge waits are **timeout-bounded only**: each of the three wait
tools requires a non-zero `timeout_ms`. This release deliberately does
**not** expose infinite/no-timeout waits or push-based edge
subscriptions — a wait that never returns would stall the stdio session,
and event streaming is out of scope for v1.

## Security

`gallo-mcp` does **not** gate writes itself. Write approval is delegated
to the MCP client through tool annotations:

- read tools are marked `readOnlyHint`,
- write and actuation tools are marked `destructiveHint`.

A well-configured client uses those hints to prompt for confirmation
before a destructive tool call. **Configure your client's permissions
accordingly.**

> [!WARNING]
> Under a permission-less or blanket-allow client, an agent can actuate
> hardware — drive pins, write buses, change configuration — without
> confirmation. If the board is wired to anything you care about, run the
> server behind a client that honors `destructiveHint`.

## Validation

The server was validated on real hardware: a Pico de Gallo (serial
`5256657D8A5D7F03`, firmware v0.10.0, schema v0.6.0, HW rev2) with a
TMP108 temperature sensor on I<sup>2</sup>C.

Over stdio, `status` reports the attached board:

```json
{ "attached": true, "firmware_version": "0.10.0", "schema_major": 0, "schema_minor": 6 }
```

`i2c_scan` finds the sensor at address `0x48`:

```json
// i2c_scan {"include_reserved":false}
{ "addresses": ["0x48"], "raw": [72] }
```

And `i2c_write_read` reads its two temperature bytes:

```json
// i2c_write_read {"address":72,"data":"0x00","count":2}
{ "hex": "0x0B,0xCF", "bytes": [11, 207] }
```

Those bytes are **byte-for-byte identical** to the `gallo` CLI:

```console
$ gallo i2c write-read -a 0x48 -b 0x00 -c 2
0b cf
```

> [!NOTE]
> Decoding `0x0BCF` into degrees Celsius is left to the reader and the
> TMP108 datasheet — it is not asserted here. The point of this
> walkthrough is that the MCP round-trip returns exactly what the CLI
> does, so an agent driving the board through `gallo-mcp` sees the same
> truth you would at the shell.
