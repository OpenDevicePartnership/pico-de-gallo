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
| **Set Config** | Updates the baud rate (and future line parameters) |
| **Get Config** | Returns the current UART configuration |

## Loopback Example

The simplest way to verify UART operation is a loopback test: connect
**GPIO 0 (TX)** directly to **GPIO 1 (RX)** with a jumper wire. Everything
you write will be received back.

### CLI

```bash
# 1. Check the current configuration
gallo uart get-config

# 2. Set baud rate to 115200 (default)
gallo uart set-config --baud-rate 115200

# 3. Write "Hello" (ASCII bytes)
gallo uart write --bytes 0x48 0x65 0x6C 0x6C 0x6F

# 4. Read back 5 bytes with a 100ms timeout
gallo uart read --count 5 --timeout 100

# 5. Flush the transmit buffer
gallo uart flush
```

### Rust Library

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

async fn uart_loopback(gallo: &PicoDeGallo) {
    // Configure baud rate
    gallo.uart_set_config(115_200).await.unwrap();

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

void uart_loopback(PicoDeGallo *gallo) {
    /* Configure baud rate */
    GalloStatus rc = gallo_uart_set_config(gallo, 115200);
    if (rc != GALLO_STATUS_OK) {
        fprintf(stderr, "set-config failed: %d\n", rc);
        return;
    }

    /* Read back current config */
    GalloUartConfigurationInfo info;
    rc = gallo_uart_get_config(gallo, &info);
    if (rc != GALLO_STATUS_OK) {
        fprintf(stderr, "get-config failed: %d\n", rc);
        return;
    }
    printf("Baud rate: %u\n", info.baud_rate);

    /* Write "Hello" */
    uint8_t tx[] = {0x48, 0x65, 0x6C, 0x6C, 0x6F};
    rc = gallo_uart_write(gallo, tx, sizeof(tx));
    if (rc != GALLO_STATUS_OK) {
        fprintf(stderr, "write failed: %d\n", rc);
        return;
    }

    /* Flush */
    gallo_uart_flush(gallo);

    /* Read back */
    uint8_t rx[5];
    uint16_t out_read;
    rc = gallo_uart_read(gallo, rx, sizeof(rx), 100, &out_read);
    if (rc != GALLO_STATUS_OK) {
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

```
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

A timeout of **0** performs a non-blocking read — it returns immediately
with whatever bytes are already in the receive buffer (possibly none):

```bash
# Non-blocking: return whatever is buffered right now
gallo uart read --count 64 --timeout 0
```

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

async fn drain_buffer(gallo: &PicoDeGallo) -> Vec<u8> {
    // timeout_ms = 0 → non-blocking
    gallo.uart_read(64, 0).await.unwrap()
}
```

## Error Handling

UART operations return `PicoDeGalloError<UartError>` on failure. The
`UartError` variants cover both protocol-level and configuration errors:

| Variant | Description |
|---------|-------------|
| `BufferTooLong` | Requested read/write exceeds the firmware buffer size |
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
| `uart_set_config` | `uart_set_config(baud_rate: u32) -> Result<(), PicoDeGalloError<UartError>>` |
| `uart_get_config` | `uart_get_config() -> Result<UartConfigurationInfo, PicoDeGalloError<UartError>>` |

> [!NOTE]
>
> `PicoDeGallo::new()` is **not** async. Only the peripheral methods
> listed above are async.

### FFI Functions

All FFI functions return a `GalloStatus` code:

```c
GalloStatus gallo_uart_read(PicoDeGallo *gallo,
                            uint8_t *buf, uint16_t buf_len,
                            uint32_t timeout_ms, uint16_t *out_read);

GalloStatus gallo_uart_write(PicoDeGallo *gallo,
                             const uint8_t *buf, uint16_t len);

GalloStatus gallo_uart_flush(PicoDeGallo *gallo);

GalloStatus gallo_uart_set_config(PicoDeGallo *gallo, uint32_t baud_rate);

GalloStatus gallo_uart_get_config(PicoDeGallo *gallo,
                                  GalloUartConfigurationInfo *out_info);
```

### CLI Commands

```
gallo uart read       --count <N> --timeout <MS>
gallo uart write      --bytes <BYTE>...
gallo uart flush
gallo uart set-config --baud-rate <RATE>
gallo uart get-config
```

## Pin Mapping

| Function | GPIO | RP2350 Peripheral |
|----------|------|-------------------|
| TX       | 0    | UART0 TX          |
| RX       | 1    | UART0 RX          |
