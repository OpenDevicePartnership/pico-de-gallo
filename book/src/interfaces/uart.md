# UART

> **Hardware revision note:** UART requires **hw-rev2** firmware. On v1
> hardware, UART endpoints return `UartError::Unsupported`.

Pico de Gallo provides UART support through the RP2350's hardware **UART0**
peripheral. The TX pin is on **GPIO 0** and RX is on **GPIO 1**. The UART
is buffered and interrupt-driven, so reads and writes do not block the
firmware's main loop.

## Operations

| Operation | Description |
|-----------|-------------|
| **Read** | Reads up to N bytes from the receive buffer with an optional timeout |
| **Write** | Writes raw bytes to the transmit buffer |
| **Flush** | Flushes the transmit buffer, blocking until all bytes are sent |
| **Set Config** | Replaces the baud rate and framing configuration |
| **Get Config** | Returns the current UART configuration |

## Framing Configuration

The firmware initializes UART0 to **115200 8N1**: 115200 bits per second,
eight data bits, no parity, and one stop bit. At runtime, `set-config` can
change the baud rate and every supported framing parameter.

The RP2350 UART0 capability envelope is:

| Parameter | Values | Result |
|-----------|--------|--------|
| Data bits | 5, 6, 7, 8 | Supported |
| Data bits | 9 | Not available |
| Parity | None, odd, even | Supported |
| Parity | Mark, space | Supported as stick parity |
| Stop bits | 1, 2 | Supported |
| Stop bits | 0.5, 1.5 | Not available |
| Flow control | None | Supported |
| Flow control | RTS/CTS | Not available |
| Flow control | DTR/DSR | Not available |
| Flow control | RS-485 | Not available |

The mark parity setting always means a parity bit of 1; the space parity
setting always means 0. These modes are not exposed by embassy's `Parity`
enum, so the firmware configures the PL011 framing register directly.

> [!WARNING]
>
> **Reconfiguration is not atomic at the UART pins.** The device applies the
> baud divisor first and the framing second, and drains neither direction.
> There is a window in which the new divisor is active with the old framing.
> Quiesce transmit and receive traffic while changing the configuration.

> [!WARNING]
>
> **`set-config` replaces the complete configuration.** In the CLI, omitting
> a framing flag selects its 8N1 power-on value and overwrites the previous
> setting. A baud-only change must therefore repeat all framing flags.

For example, configure 9600 baud with seven data bits, even parity, and two
stop bits:

```bash
gallo uart set-config --baud-rate 9600 \
    --data-bits 7 --parity even --stop-bits 2
gallo uart get-config
```

`get-config` reports the last successfully requested configuration. This is
a software shadow, not a UART register read-back, so the reported baud rate
does not reflect divisor rounding.

## Loopback Example

The simplest way to verify UART operation is a loopback test: connect
**GPIO 0 (TX)** directly to **GPIO 1 (RX)** with a jumper wire. Everything
you write will be received back.

### CLI

```bash
# 1. Check the current configuration
gallo uart get-config

# 2. Set the complete configuration to 115200 8N1 (default)
gallo uart set-config --baud-rate 115200 \
    --data-bits 8 --parity none --stop-bits 1

# 3. Write "Hello" (ASCII bytes)
gallo uart write --bytes 0x48 0x65 0x6C 0x6C 0x6F

# 4. Read back 5 bytes with a 100ms timeout
gallo uart read --count 5 --timeout 100

# 5. Flush the transmit buffer
gallo uart flush
```

### Rust Library

```rust,no_run
use pico_de_gallo_lib::{
    PicoDeGallo, UartDataBits, UartParity, UartStopBits,
};

async fn uart_loopback(gallo: &PicoDeGallo) {
    // Configure 115200 8N1
    gallo
        .uart_set_config(
            115_200,
            UartDataBits::Eight,
            UartParity::None,
            UartStopBits::One,
        )
        .await
        .unwrap();

    // Verify configuration
    let config = gallo.uart_get_config().await.unwrap();
    println!("Baud rate: {}", config.baud_rate);

    // Write "Hello"
    gallo.uart_write(&[0x48, 0x65, 0x6C, 0x6C, 0x6F]).await.unwrap();

    // Flush to ensure all bytes are transmitted
    gallo.uart_flush().await.unwrap();

    // Read back with 100ms timeout
    let data = gallo.uart_read(5, 100).await.unwrap();
    assert_eq!(&data, &[0x48, 0x65, 0x6C, 0x6C, 0x6F]);
    println!("Received: {:?}", String::from_utf8_lossy(&data));
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>
#include <string.h>

void uart_loopback(const PicoDeGallo *gallo) {
    /* Configure 115200 8N1 */
    Status rc = gallo_uart_set_config(
        gallo,
        115200,
        GalloUartDataBits_Eight,
        GalloUartParity_None,
        GalloUartStopBits_One);
    if (rc != Ok) {
        fprintf(stderr, "set-config failed: %d\n", rc);
        return;
    }

    /* Read back current config */
    uint32_t baud_rate;
    uint8_t data_bits;
    uint8_t parity;
    uint8_t stop_bits;
    rc = gallo_uart_get_config(
        gallo, &baud_rate, &data_bits, &parity, &stop_bits);
    if (rc != Ok) {
        fprintf(stderr, "get-config failed: %d\n", rc);
        return;
    }
    printf("Baud rate: %u\n", baud_rate);

    /* Write "Hello" */
    uint8_t tx[] = {0x48, 0x65, 0x6C, 0x6C, 0x6F};
    rc = gallo_uart_write(gallo, tx, sizeof(tx));
    if (rc != Ok) {
        fprintf(stderr, "write failed: %d\n", rc);
        return;
    }

    /* Flush */
    gallo_uart_flush(gallo);

    /* Read back */
    uint8_t rx[5];
    uint16_t out_read;
    rc = gallo_uart_read(gallo, rx, sizeof(rx), 100, &out_read);
    if (rc != Ok) {
        fprintf(stderr, "read failed: %d\n", rc);
        return;
    }

    printf("Received %u bytes: %.*s\n", out_read, out_read, rx);
}
```

### HAL

The HAL layer implements the standard `embedded_io` and `embedded_io_async`
traits, so the UART can be used with any driver that accepts generic
readers or writers.

The `embedded-io-06` Cargo feature remains enabled by default, while
`embedded-io-07` can be enabled additively. See
[Feature Flags](../crates/hal.md#feature-flags) for the details. The
snippets below are identical either way.

**Blocking** — `embedded_io::Read` + `embedded_io::Write`:

```rust,no_run
use embedded_io::{Read, Write};
use pico_de_gallo_hal::Hal;

fn uart_loopback_blocking(hal: &Hal) {
    let mut uart = hal.uart();

    // Write "Hello"
    uart.write_all(&[0x48, 0x65, 0x6C, 0x6C, 0x6F]).unwrap();
    uart.flush().unwrap();

    // Read back
    let mut buf = [0u8; 5];
    uart.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"Hello");
}
```

**Async** — `embedded_io_async::Read` + `embedded_io_async::Write`:

```rust,no_run
use embedded_io_async::{Read, Write};
use pico_de_gallo_hal::Hal;

async fn uart_loopback_async(hal: &Hal) {
    let mut uart = hal.uart();

    uart.write_all(&[0x48, 0x65, 0x6C, 0x6C, 0x6F]).await.unwrap();
    uart.flush().await.unwrap();

    let mut buf = [0u8; 5];
    uart.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"Hello");
}
```

## Connecting an External Device

To communicate with an external UART device (e.g., a GPS module or
microcontroller), connect:

```text
Pico de Gallo          External Device
──────────────         ───────────────
GPIO 0 (TX) ────────── RX
GPIO 1 (RX) ────────── TX
GND ────────────────── GND
```

> [!NOTE]
>
> Cross the TX/RX lines: the transmit pin of one device connects to the
> receive pin of the other.

## Non-blocking Read

A timeout of **0** performs a single non-blocking poll and returns whatever
bytes are already in the receive buffer (possibly none). This is a deliberate
exception to GPIO wait semantics. Non-zero values above the firmware's
30-minute ceiling are clamped to that ceiling:

```bash
# Non-blocking: return whatever is buffered right now
gallo uart read --count 64 --timeout 0
```

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

async fn drain_buffer(gallo: &PicoDeGallo) -> Vec<u8> {
    // timeout_ms = 0 → one non-blocking poll
    gallo.uart_read(64, 0).await.unwrap()
}
```

## Error Handling

UART direction determines the payload ceiling. `uart_read` may return at most
`MAX_RESPONSE_PAYLOAD` (1014 bytes), while `uart_write` may send at most
`MAX_TRANSFER_SIZE` (4096 bytes). Every host surface checks this before
transmitting and reports `BufferTooLong` for an over-ceiling call.

UART operations return `PicoDeGalloError<UartError>` on failure. The
`UartError` variants cover both protocol-level and configuration errors:

| Variant | Description |
|---------|-------------|
| `BufferTooLong` | Read exceeds the response ceiling or write exceeds the transfer ceiling |
| `Overrun` | Receive buffer overflowed before host read the data |
| `Break` | Break condition detected on the line |
| `Parity` | Parity check failed |
| `Framing` | Invalid stop bit detected |
| `InvalidBaudRate` | Requested baud rate is out of range or unsupported |
| `Other` | Catch-all for unexpected firmware errors |
| `Unsupported` | UART is not available on this hardware revision |

## API Reference

### Lib Methods

All methods are `async` and available on `PicoDeGallo`:

| Method | Signature |
|--------|-----------|
| `uart_read` | `uart_read(count: u16, timeout_ms: u32) -> Result<Vec<u8>, PicoDeGalloError<UartError>>` |
| `uart_write` | `uart_write(contents: &[u8]) -> Result<(), PicoDeGalloError<UartError>>` |
| `uart_flush` | `uart_flush() -> Result<(), PicoDeGalloError<UartError>>` |
| `uart_set_config` | `uart_set_config(baud_rate: u32, data_bits: UartDataBits, parity: UartParity, stop_bits: UartStopBits) -> Result<(), PicoDeGalloError<UartError>>` |
| `uart_get_config` | `uart_get_config() -> Result<UartConfigurationInfo, PicoDeGalloError<UartError>>` |

> [!NOTE]
>
> `PicoDeGallo::new()` is **not** async. Only the peripheral methods
> listed above are async.

### FFI Functions

All FFI functions return a `Status` code:

```c
Status gallo_uart_read(const PicoDeGallo *gallo,
                       uint8_t *buf, uint16_t buf_len,
                       uint32_t timeout_ms, uint16_t *out_read);

Status gallo_uart_write(const PicoDeGallo *gallo,
                        const uint8_t *buf, uint16_t len);

Status gallo_uart_flush(const PicoDeGallo *gallo);

Status gallo_uart_set_config(const PicoDeGallo *gallo,
                             uint32_t baud_rate,
                             uint8_t data_bits,
                             uint8_t parity,
                             uint8_t stop_bits);

Status gallo_uart_get_config(const PicoDeGallo *gallo,
                             uint32_t *out_baud_rate,
                             uint8_t *out_data_bits,
                             uint8_t *out_parity,
                             uint8_t *out_stop_bits);
```

### CLI Commands

```text
gallo uart read       --count <N> --timeout <MS>
gallo uart write      --bytes <BYTE>...
gallo uart flush
gallo uart set-config --baud-rate <BAUD_RATE> [--data-bits <5|6|7|8>] \
                      [--parity <none|odd|even|mark|space>] \
                      [--stop-bits <1|2>]
gallo uart get-config
gallo uart help
```

## Pin Mapping

| Function | GPIO | RP2350 Peripheral |
|----------|------|-------------------|
| TX       | 0    | UART0 TX          |
| RX       | 1    | UART0 RX          |
