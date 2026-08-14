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

- `-s, --serial-number <SN>` **pins** the server to one board. A pinned
  server cannot address any other board: a tool call naming a different
  serial is refused. This is the way to scope an agent session to a
  single board. If the pinned serial is not among the attached boards at
  startup, the server logs a warning naming the serials it did find, and
  starts anyway — see below.
- **Per-call connection:** the server holds no persistent USB claim. Each
  tool call opens the board, runs, and releases it when the call completes,
  so the device is free for the `gallo` CLI or other host processes between
  calls. Each call re-opens and re-validates the board, so there is a small
  fixed per-call connection cost. The server starts even with no board
  attached and tools begin working as soon as a Pico de Gallo is present;
  you can plug the board in mid-session.

The pin is checked once at startup, and an unusable one is a **warning**
rather than a startup failure — starting with no board attached has to
keep working, so the server cannot treat "pin not attached" as fatal:

```text
WARN gallo_mcp: This server is pinned to serial number 'BOGUSSERIAL' (--serial-number), which is not attached.
Available: 5256657D8A5D7F03
```

Nothing is logged when the pin resolves, when no pin was given, or when
no board is attached at all — that last case is indistinguishable from
"not plugged in yet", which is supported. So the warning firing means a
board *is* attached and the pin does not match it, which in practice
means a typo. Without it a mistyped pin starts a server that looks
healthy and then fails every device call for the rest of the session.

This warning is on **by default** — it has to be, because MCP clients
launch the server with whatever environment they have and rarely set
`RUST_LOG`. Logs go to stderr, so they never disturb the JSON-RPC stream
on stdout. `RUST_LOG` still controls verbosity when you set it, and
overrides the default entirely: `RUST_LOG=error` silences the warning,
`RUST_LOG=gallo_mcp=debug` adds the per-call board-lock tracing.

## Choosing a board

Every tool except `list_devices` takes an optional `serial_number`, and
every response that came from a board names the board it came from:

```json
// i2c_scan {"serial_number":"5256657D8A5D7F03"}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "addresses": ["0x48"], "raw": [72] }
}
```

That `{ "serial_number", "result" }` envelope wraps every device tool
response but two. `list_devices` opens no board at all, and `status`
reports its serial as a top-level field of its own result rather than
nesting a serial under a serial.

The envelope's `serial_number` is `null` in exactly one case: the sole
attached board reports no USB serial number. The field is always
present, so `null` there means "a board that has no serial answered" —
never "this response was not enveloped". `status`'s `available` list
carries `null` entries for the same reason.

How the target is chosen:

| Boards attached | `serial_number` | Result |
|---|---|---|
| 0 | — | error: no device attached |
| 1 | omitted | that board |
| 1 | given | that board, if it matches |
| ≥2 | omitted | **error**, listing the available serials |
| ≥2 | given | the named board, if exactly one matches |

With one board attached nothing changes — omit `serial_number` and it
just works. With two or more, omitting it is an error rather than a
guess:

```text
Multiple Pico de Gallo devices attached; `serial_number` is required.
Available: 5256657D8A5D7F03, 568E9AAEC72B0D49
```

That is deliberate. Guessing turns a recoverable mistake into a
confident wrong answer with no signal that anything went wrong; the
error names the serials, so the next call succeeds.

Two boards can also report the *same* serial, which is refused for the
same reason — naming it no longer identifies a board:

```text
2 attached Pico de Gallo devices report serial number '0000000000000000'; they cannot be told apart. Detach all but one and retry.
```

That is reachable rather than hypothetical. The firmware derives the USB
serial from the RP2350 chip ID and falls back to all-zeros when the OTP
read fails, which its own comment notes happens on some dev boards, so
two such boards collide. No argument fixes it; detach one.

A server started with `-s` is **pinned**, and the two `≥2` rows above no
longer apply: `serial_number` is optional again however many boards are
attached. An omitted argument uses the pinned board, a matching one is
accepted, and a different one is refused:

```text
This server is pinned to serial number '5256657D8A5D7F03' (--serial-number); it cannot address '568E9AAEC72B0D49'. Omit serial_number, or pass '5256657D8A5D7F03'.
```

Device state is **per board** — bus configuration, GPIO direction, PWM
enable, and 1-Wire search progress all live on the board you addressed —
so a follow-up call must repeat the `serial_number` of the call that set
it up.

`list_devices` tells you which case you are in without connecting:

```json
// list_devices {}
{
  "devices": [
    { "serial_number": "5256657D8A5D7F03", "manufacturer": null,
      "product": "Pico de Gallo", "pinned": false, "default_target": false },
    { "serial_number": "568E9AAEC72B0D49", "manufacturer": null,
      "product": "Pico de Gallo", "pinned": false, "default_target": false }
  ],
  "pinned": null,
  "serial_number_required": true,
  "note": "2 devices attached and this server is not pinned; pass serial_number on every device tool call."
}
```

`note` is present only when `serial_number_required` is true.

`status` never errors, so it stays answerable even when the target is
ambiguous:

```json
// status {}
{
  "attached": true,
  "serial_number": null,
  "ambiguous": true,
  "available": ["5256657D8A5D7F03", "568E9AAEC72B0D49"],
  "pinned": null,
  "reason": "Multiple Pico de Gallo devices attached; `serial_number` is required.\nAvailable: 5256657D8A5D7F03, 568E9AAEC72B0D49",
  "firmware_version": null,
  "schema_major": null,
  "schema_minor": null
}
```

`ambiguous` answers "would a call that omits `serial_number` be
*ambiguous*?" — not "would it fail?". A bare call also fails with no
board attached, and when the server is pinned to a board that is not
attached; `ambiguous` is `false` in both. It stays true even when this
particular `status` call named a board. `reason` is present only when
no board was reached.

## Concurrency

The connection lock is keyed on the **board**, not on the server. Calls
to different boards run concurrently; calls to the same board queue. A
long-running tool — a `gpio_wait_*` sitting on its full `timeout_ms` —
holds only the board it addressed, so traffic to every other board keeps
flowing.

`list_devices` connects to nothing and always answers immediately.
`status` does open the board it names, so it queues behind a call that is
still holding that board.

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
or `"0010"`); reads return `{ "hex": ..., "bytes": ... }` inside the
response envelope.

```json
// i2c_write_read {"address":72,"data":"0x00","count":2}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "hex": "0x0B,0x8E", "bytes": [11, 142] }
}
```

## Tool catalog

43 tools, grouped by peripheral. Read-only tools carry the
`readOnlyHint` annotation; write/actuation tools carry `destructiveHint`.
Every tool except `list_devices` accepts an optional `serial_number`.

### device (read-only)

| Tool | Description |
|---|---|
| `list_devices` | List connected Pico de Gallo boards |
| `status` | Which board is reachable, why not when none is, plus firmware/schema version |
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

`spi_batch` takes `cs` as a `u8` and runs its steps in a fixed order:
parse every operation payload, connect exactly once, read the GPIO count
from the `DeviceInfo` that connection already validated, classify `cs`,
then call the library once. No second metadata query is issued.

Parsing comes first on purpose. `connect` runs
`system_reset_subscriptions`, which tears down every GPIO subscription on
the board — including ones owned by other host processes — so a malformed
request must not reach it. A payload that fails to parse returns
`-32602` (invalid params) without connecting.

An out-of-range `cs` and a device reporting zero GPIOs are also `-32602`,
with distinct messages, and nothing is transmitted. A failure to
establish the count — transport, 300-second `device/info` timeout, legacy
firmware, or schema mismatch — is `-32603` (internal error), never
`-32602`: it is not a complaint about your argument.

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

`uart_read`'s `timeout_ms` is **not** covered by that rule: there `0` is
legal and means a non-blocking poll that returns whatever is already
buffered. The asymmetry is deliberate — for an edge wait, `0` would mean
an unbounded wait.

`uart_read` is also one of the twelve tools that hard-fail on a
`hw-rev1` board: that revision supports only I<sup>2</sup>C, SPI, GPIO,
and PWM, so every `uart_*`, `adc_*`, and `onewire_*` call returns
`Unsupported` there. Two boards on one bench can be different
revisions, so call `device_info` per board rather than assuming they
are interchangeable.

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

### Every call resets that board's GPIO subscriptions

Opening a board tears down **every** GPIO edge subscription on it,
including ones owned by other host processes. A `gallo` CLI session or a
user program watching a pin loses that watch the moment a tool call
touches the same board.

That is the documented host protocol rather than a bug — it is how a
host recovers pins stranded by a previous host that died mid-watch — but
two things about it are easy to miss:

- **`readOnlyHint` does not cover it.** `status`, `device_info`,
  `version`, `ping`, and `i2c_scan` are annotated read-only and appear
  in the read-only rows of the catalog below, yet each opens a board and
  so each resets its subscriptions. A client gating on `destructiveHint`
  will not prompt for any of them.
- **Per-call selection widened the blast radius.** It used to reach only
  the single board the server was bound to. Now any call can name any
  attached board, so on a multi-board bench a call carrying
  `serial_number` for board B disturbs board B — even if every previous
  call in the session went to board A.

`list_devices` is the one exception: it opens no board, so it disturbs
nothing.

If a long-lived watch matters, pin the server with `-s` to keep it away
from the board running the watch.

## Validation

The server was validated on real hardware: a Pico de Gallo (serial
`5256657D8A5D7F03`, firmware v0.10.1, schema v0.6.1, HW rev2) with a
TMP108 temperature sensor on I<sup>2</sup>C.

Over stdio, `status` reports the attached board:

```json
// status {}
{
  "attached": true,
  "serial_number": "5256657D8A5D7F03",
  "ambiguous": false,
  "available": ["5256657D8A5D7F03"],
  "pinned": null,
  "firmware_version": "0.10.1",
  "schema_major": 0,
  "schema_minor": 6
}
```

`i2c_scan` finds the sensor at address `0x48`:

```json
// i2c_scan {"include_reserved":false}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "addresses": ["0x48"], "raw": [72] }
}
```

And `i2c_write_read` reads its two temperature bytes:

```json
// i2c_write_read {"address":72,"data":"0x00","count":2}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "hex": "0x0B,0x8E", "bytes": [11, 142] }
}
```

Those bytes are **byte-for-byte identical** to the `gallo` CLI:

```console
$ gallo i2c write-read -a 0x48 -b 0x00 -c 2
0b 8e
```

Board selection was validated on the same bench with a second board
attached (`568E9AAEC72B0D49`, bare bus): a bare `i2c_scan` is refused
rather than answered from an arbitrary board, and naming either serial
reaches that board. See [Choosing a board](#choosing-a-board).

> [!NOTE]
> Decoding `0x0B8E` into degrees Celsius is left to the reader and the
> TMP108 datasheet — it is not asserted here. The point of this
> walkthrough is that the MCP round-trip returns exactly what the CLI
> does, so an agent driving the board through `gallo-mcp` sees the same
> truth you would at the shell.
