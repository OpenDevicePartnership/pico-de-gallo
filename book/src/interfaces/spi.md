# SPI

Pico de Gallo drives the RP2350's **SPI0** controller in
DMA-backed full-duplex mode.

| Signal     | RP2350 GPIO | Available on |
|------------|-------------|--------------|
| SCK        | GPIO 6      | v1.0+        |
| MOSI (TX)  | GPIO 7      | v1.0+        |
| MISO (RX)  | GPIO 4      | v1.0+        |
| CS         | GPIO 5      | v1.1+        |

> [!NOTE]
>
> On v1.0 the dedicated CS line isn't routed to any header. You
> can still drive chip-select from any of the user GPIO pins
> (0–3) via the `spi_device(cs_pin)` HAL accessor or by toggling
> a GPIO manually around the SPI ops.

## Operations

| Operation | Description |
|-----------|-------------|
| **Read**     | Clock in N bytes (MISO only) |
| **Write**    | Clock out bytes (MOSI only) |
| **Transfer** | Full-duplex: simultaneous TX and RX |
| **Flush**    | Wait for any in-flight transactions to complete |
| **Batch**    | Sequence of ops under a single chip-select |
| **Set Config** | Change frequency / CPHA / CPOL at runtime |
| **Get Config** | Query the current configuration |

## SPI Mode

SPI mode is the (CPOL, CPHA) tuple. Mode is set via
`set-config` / `spi_set_config()`:

| Mode | CPOL | CPHA | Idle clock | Sample edge |
|------|------|------|------------|-------------|
| 0    | 0    | 0    | low        | rising      |
| 1    | 0    | 1    | low        | falling     |
| 2    | 1    | 0    | high       | falling     |
| 3    | 1    | 1    | high       | rising      |

The firmware defaults to mode 0.

## CLI

```console
$ gallo spi help
SPI access methods

Usage: gallo spi <COMMAND>

Commands:
  read        Read bytes through SPI bus
  write       Write bytes through SPI bus
  transfer    Full-duplex SPI transfer (simultaneous write and read)
  write-read  Write bytes followed by read bytes (half-duplex)
  set-config  Set SPI bus parameters
  get-config  Query the current SPI bus configuration
  batch       Execute multiple SPI operations atomically under chip-select
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### Read / Write / Transfer

```console
$ gallo spi read --count 4
00 00 00 00

$ gallo spi write --bytes 0x9f

$ gallo spi transfer --bytes 0x01 0x02 0x03 0x04
00 00 00 00
```

`transfer` clocks out the given bytes on MOSI and simultaneously
clocks in the same number of bytes on MISO — true full-duplex.

### Config

Mode is selected by two independent flags, and **both are opt-in**:

```console
$ gallo spi set-config --help
Set SPI bus parameters

Usage: gallo spi set-config [OPTIONS] --frequency <FREQUENCY>

Options:
      --frequency <FREQUENCY>  SPI frequency in Hz
      --first-transition       SPI phase first transition (CPHA=0)
      --idle-low               SPI polarity idle low (CPOL=0)
  -h, --help                   Print help
```

Omitting `--first-transition` gives CPHA=1 and omitting `--idle-low`
gives CPOL=1, so passing neither selects **mode 3**. Mode 0 needs both:

```console
$ gallo spi set-config --frequency 1000000 --first-transition --idle-low
$ gallo spi get-config
SPI frequency: 1000000 Hz
SPI phase:     CaptureOnFirstTransition (CPHA=0)
SPI polarity:  IdleLow (CPOL=0)
```

> [!WARNING]
>
> The firmware boots in mode 0, but `set-config` defaults to mode 3.
> `gallo spi set-config --frequency 1000000` therefore changes the mode
> as a side effect of setting the clock. Pass the mode flags explicitly
> whenever you call `set-config`.

### Batch (Atomic Under CS)

A single transaction with chip-select held low for the duration:

```console
$ gallo spi batch --cs 0 --op write:0x9f --op read:3
Read data (3 bytes):
  0000: ef 40 18                                           .@.
```

The `--cs` flag picks which user GPIO (0–3) drives chip-select.
See [Transaction Batching](./batching.md).

## Rust Library

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, SpiBatchOp, SpiPhase, SpiPolarity};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pg = PicoDeGallo::new();

    // Mode 0: sample on the first transition, clock idles low.
    pg.spi_set_config(
        1_000_000,
        SpiPhase::CaptureOnFirstTransition,
        SpiPolarity::IdleLow,
    )
    .await?;

    // Read JEDEC ID under CS on GPIO 0
    let ops = [
        SpiBatchOp::Write { data: &[0x9F] },
        SpiBatchOp::Read { len: 3 },
    ];
    let result = pg.spi_batch(0, &ops).await?;
    println!(
        "JEDEC: mfr=0x{:02x} type=0x{:02x} cap=0x{:02x}",
        result[0], result[1], result[2]
    );
    Ok(())
}
```

## HAL

The HAL provides two flavours of SPI access:

- **`hal.spi()`** — a raw `embedded_hal::spi::SpiBus` /
  `embedded_hal_async::spi::SpiBus` implementor. You manage
  chip-select yourself.
- **`hal.spi_device(cs_pin)`** — an `SpiDevice` that automatically
  drives the given GPIO as chip-select around every transaction.

```rust,no_run
use embedded_hal::spi::{Operation, SpiDevice};
use pico_de_gallo_hal::Hal;

fn read_jedec(hal: &Hal) -> [u8; 3] {
    let mut spi = hal.spi_device(0);
    let mut id = [0u8; 3];

    // One transaction; CS asserted for the whole thing; batched into
    // one USB round-trip transparently.
    spi.transaction(&mut [
        Operation::Write(&[0x9F]),
        Operation::Read(&mut id),
    ])
    .unwrap();
    id
}
```

## C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

void read_jedec(PicoDeGallo *gallo) {
    /* mode 0, 1 MHz */
    gallo_spi_set_config(gallo, 1000000, /*phase=*/false, /*polarity=*/false);

    uint8_t cmd[] = {0x9F};
    gallo_spi_write(gallo, cmd, 1);

    uint8_t id[3];
    gallo_spi_read(gallo, id, sizeof(id));
    printf("JEDEC: %02x %02x %02x\n", id[0], id[1], id[2]);
}
```

For atomic chip-select transactions, batch operations are
available — see the `gallo_spi_batch_*` family in the generated
`pico_de_gallo.h`.

## Python

```python
from pyco_de_gallo import PycoDeGallo, SpiPhase, SpiPolarity

pg = PycoDeGallo()
# Mode 0: sample on the first transition, clock idles low.
pg.spi_set_config(
    1_000_000, SpiPhase.CaptureOnFirstTransition, SpiPolarity.IdleLow
)

pg.spi_write(bytes([0x9F]))
id_bytes = pg.spi_read(3)
print("JEDEC:", id_bytes.hex())
```

## Error Handling

| Variant         | Meaning                                       |
|-----------------|-----------------------------------------------|
| `BufferTooLong` | Request exceeds firmware buffer limit         |
| `Unsupported`   | Returned by firmware builds without SPI       |
| `Other`         | Catch-all for firmware-reported SPI failure   |

See [`appendix/status-codes.md`](../appendix/status-codes.md) for
the FFI mapping.
