# `gallo` CLI

`gallo` is the fastest way to prove your board works, poke a device, and turn a
manual experiment into a repeatable command. It sits on top of
`pico-de-gallo-lib`, so the CLI and the Rust library speak the same protocol and
see the same capabilities.

Use it for:

- bring-up and smoke tests,
- one-off I<sup>2</sup>C / SPI / UART / GPIO / PWM / ADC / 1-Wire operations,
- shell scripting,
- discovering which board is which when several are plugged in.

## Top-level Help

```console
$ gallo -h
Access I2C/SPI devices through Pico De Gallo

Usage: gallo [OPTIONS] <COMMAND>

Commands:
  list     List all connected Pico de Gallo devices
  ping     Check device liveness with a round-trip echo
  version  Get firmware version
  i2c      I2C access methods
  spi      SPI access methods
  gpio     GPIO access methods
  uart     UART access methods
  pwm      PWM control methods
  adc      ADC access methods
  onewire  1-Wire bus access methods
  help     Print this message or the help of the given subcommand(s)

Options:
  -s, --serial-number <SERIAL_NUMBER>  Select a specific board by USB serial number
  -f, --format <FORMAT>                Output format for read data [default: hex] [possible values: hex, binary, ascii]
  -h, --help                           Print help (see more with '--help')
  -V, --version                        Print version
```

> [!TIP]
> `--help` prints the same thing with each option expanded — including the
> per-value descriptions for `--format`.

## Global Options

### `-s, --serial-number`

If more than one Pico de Gallo is attached, `gallo` would otherwise use the
first matching device the OS reports. Pass `-s` to make board selection
explicit.

```console
$ gallo list
Serial Number         Bus    Address
E6633861A34B8C24      2      14
E6633861A34B9F17      1      8
```

Then target one of those serials explicitly:

```bash
gallo -s E6633861A34B9F17 version
```

### `-f, --format hex|binary|ascii`

The global `-f` flag controls how read-style commands print data:

- `hex` — hexadecimal bytes,
- `binary` — raw bytes to stdout,
- `ascii` — printable characters, with non-printable bytes shown as `.`.

```console
$ gallo -f ascii uart read --count 5 --timeout 100
Hello
```

> [!TIP]
> `binary` is the right choice when you want to pipe the output into another
> program without pretty-printing in the way.

## Device Discovery Commands

### `list`

Lists every connected Pico de Gallo device the host can see.

```console
$ gallo list
Serial Number         Bus    Address
E6633861A34B8C24      2      14
```

### `version`

Queries the connected board for firmware, schema, hardware revision, runtime
GPIO count, build identity, and capabilities.

```console
$ gallo version
╭─────────────┬──────────────────────────────╮
│ Firmware    │ v0.12.0                      │
│ Schema      │ v0.8.0                       │
│ HW revision │ 2                            │
│ GPIOs       │ 4                            │
│ Build       │ firmware-v0.12.0-42-g1a2b3c4 │
╰─────────────┴──────────────────────────────╯
╭─────┬─────┬──────┬──────┬─────┬─────┬────────╮
│ I2C │ SPI │ UART │ GPIO │ PWM │ ADC │ 1-Wire │
├─────┼─────┼──────┼──────┼─────┼─────┼────────┤
│ ✓   │ ✓   │ ✓    │ ✓    │ ✓   │ ✓   │ ✓      │
╰─────┴─────┴──────┴──────┴─────┴─────┴────────╯
```

### `ping`

Round-trips a random `u32` through the firmware's `ping` endpoint and
checks that the same value comes back.

```console
$ gallo ping
Ping OK
```

This is the lowest-level check `gallo` offers. It exercises USB
enumeration, the postcard-rpc framing, and the firmware dispatch loop
without touching a peripheral, so it is the right first move when a
board enumerates but a peripheral command misbehaves.

The payload is randomised per invocation so that a stale, duplicated, or
default-initialised response cannot pass as a healthy round trip. If the
round trip completes but the value comes back wrong, `gallo` reports the
mismatch with both values rather than a generic transport error:

```console
$ gallo ping
Error: ping echo mismatch: sent 0x9f2c41ab, received 0x00000000
```

> [!NOTE]
> `ping` and `version` are the only device subcommands that skip the
> up-front schema-version check. A board whose schema does not match this
> `gallo` build should still be able to prove its USB path works — that is
> exactly the situation `ping` exists to diagnose. See
> [Verifying Your Device](../getting-started/verify.md).

## Peripheral Command Groups

### `i2c`

| Subcommand | Purpose |
|---|---|
| `scan` | Probe the bus for responding addresses |
| `read` | Read bytes from one target address |
| `write` | Write bytes to one target address |
| `write-read` | Write first, then read from the same target without releasing the bus |
| `set-config` | Set the I<sup>2</sup>C frequency |
| `get-config` | Show the active I<sup>2</sup>C frequency |
| `batch` | Execute multiple I2C operations as a single transaction |

See the [I<sup>2</sup>C chapter](../interfaces/i2c.md) and
[Transaction Batching](../interfaces/batching.md) for examples.

### `spi`

| Subcommand | Purpose |
|---|---|
| `read` | Clock in bytes |
| `write` | Clock out bytes |
| `transfer` | Full-duplex SPI transfer |
| `write-read` | Half-duplex write followed by read |
| `set-config` | Set frequency and SPI mode (0–3) |
| `get-config` | Show the active SPI configuration |
| `batch` | Run atomic multi-step SPI transactions under chip-select |

> [!NOTE]
> The CLI refuses over-ceiling payloads locally, before transmitting. Data
> returned by the device is limited to `MAX_RESPONSE_PAYLOAD` (1014 bytes),
> while data sent to it is limited to `MAX_TRANSFER_SIZE` (4096 bytes).
> Full-duplex `spi transfer` is therefore limited to 1014 bytes even though
> `spi write` accepts 4096. A `batch` is bounded a third way: its aggregate
> outgoing bytes must fit one `MAX_REQUEST_FRAME` (5119-byte) request frame.
> The command reports `BufferTooLong`; see
> [troubleshooting](../appendix/troubleshooting.md#buffertoolong-22).

The read counts accepted by `i2c read -c`, `i2c write-read -c`, `spi read -c`,
and `spi write-read -c` are `u16` values. Values above 65535 are rejected by
argument parsing rather than wrapping during conversion; the lower
`MAX_RESPONSE_PAYLOAD` limit is then enforced before transmission.

`batch --cs <PIN>` accepts any `u8`. The pin is checked at run time
against the GPIO count the connected device reports — not against a fixed
range — before the operations are parsed and before anything is
transmitted, so an out-of-range chip-select drives no pin:

```text
invalid SPI chip-select pin 7; device reports 4 GPIOs (valid 0..4)
device reports num_gpios=0; no SPI chip-select pin is available
```

Every subcommand except `list` and `version` validates the firmware
before doing anything else, and that validation supplies the count. If it
fails — including a `device/info` timeout after 300 seconds — the error
appears under `firmware validation failed`, never as an invalid
chip-select.

See the [SPI chapter](../interfaces/spi.md) and
[Transaction Batching](../interfaces/batching.md).

### `gpio`

| Subcommand | Purpose |
|---|---|
| `get` | Read the current level of a pin |
| `put` | Drive a pin high or low - `put --pin <PIN> --level <high\|low>` |
| `set-config` | Set direction and pull resistor |
| `monitor` | Subscribe to edge events until you stop the process |

See the [GPIO chapter](../interfaces/gpio.md).

### `uart`

| Subcommand | Purpose |
|---|---|
| `read` | Read bytes with a timeout |
| `write` | Write raw bytes |
| `flush` | Wait for the transmit buffer to drain |
| `set-config` | Set baud rate, data bits, parity, and stop bits |
| `get-config` | Show the active UART configuration |

```console
Usage: gallo uart set-config [OPTIONS] --baud-rate <BAUD_RATE>

Options:
      --baud-rate <BAUD_RATE>   Baud rate in bits per second (e.g. 9600, 115200)
      --data-bits <DATA_BITS>   Data bits per character [default: 8]
                                [possible values: 5, 6, 7, 8]
      --parity <PARITY>         Parity mode [default: none]
                                [possible values: none, odd, even, mark, space]
      --stop-bits <STOP_BITS>   Stop bits [default: 1]
                                [possible values: 1, 2]
  -h, --help                    Print help
```

The framing options are long-only. `set-config` replaces the complete
configuration, so omitting them selects 8N1 and overwrites the active framing;
repeat all three when changing only the baud rate. Baud and framing are applied
together but not atomically: the divisor changes first and neither direction is
drained, so quiesce transmit and receive traffic while reconfiguring.

See the [UART chapter](../interfaces/uart.md).

### `pwm`

| Subcommand | Purpose |
|---|---|
| `set-duty` | Set a raw duty-cycle value |
| `get-duty` | Read current and maximum duty |
| `enable` | Enable the slice behind a channel |
| `disable` | Disable the slice behind a channel |
| `set-config` | Set frequency and phase-correct mode |
| `get-config` | Show the active PWM configuration |

See the [PWM chapter](../interfaces/pwm.md).

### `adc`

| Subcommand | Purpose |
|---|---|
| `read` | Read one ADC sample |
| `info` | Show ADC resolution, reference, and channel count |

See the [ADC chapter](../interfaces/adc.md).

### `onewire`

| Subcommand | Purpose |
|---|---|
| `reset` | Reset the bus and report presence |
| `read` | Read raw bytes |
| `write` | Write raw bytes |
| `write-pullup` | Write, then hold the line high for parasitic-power devices |
| `search` | Enumerate ROM IDs on the bus |

See the [1-Wire chapter](../interfaces/onewire.md).

## A Few Crisp Examples

```console
$ gallo ping
$ gallo i2c get-config
$ gallo spi get-config
$ gallo uart set-config --baud-rate 115200 --data-bits 8 \
    --parity none --stop-bits 1
$ gallo gpio monitor --pin 0 --edge rising
$ gallo adc read --channel 0
$ gallo onewire search
```

That is the right mental model for `gallo`: short commands, explicit arguments,
and results you can immediately paste into a shell script or lab notebook.
