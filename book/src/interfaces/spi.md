# SPI

Pico de Gallo drives the RP2350's **SPI0** controller in
DMA-backed full-duplex mode.

| Signal     | RP2350 GPIO | Available on |
|------------|-------------|--------------|
| SCK        | GPIO 6      | v1.0+        |
| MOSI (TX)  | GPIO 7      | v1.0+        |
| MISO (RX)  | GPIO 4      | v1.0+        |
| SPI_CS net | GPIO 5      | v1.1+        |

> **Note.** GPIO 5 is physically routed as SPI_CS on v1.1, but firmware
> never claims or drives it on either revision. The only firmware-managed
> chip-select mechanism uses user GPIO indices in `0..num_gpios`, where
> `num_gpios` is device-reported and currently 4. Those indices map to
> RP2350 GPIO 8–11 and work with `spi/batch`, the fallible
> `spi_device(cs_pin)` HAL accessor, and equivalent host surfaces. Manually
> toggling a GPIO around separate SPI operations is also possible, but it
> does not hold chip-select atomically across the sequence as `spi/batch`
> does.

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
$ gallo spi --help
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

Mode is selected with a single `--mode` flag, defaulting to 0:

```console
$ gallo spi set-config -h
Set SPI bus parameters

Usage: gallo spi set-config [OPTIONS] --frequency <FREQUENCY>

Options:
      --frequency <FREQUENCY>  SPI frequency in Hz
      --mode <MODE>            SPI mode 0-3, the conventional (CPOL, CPHA) pairing [default: 0]
  -h, --help                   Print help (see more with '--help')
```

The default matches the firmware's power-on configuration, so setting
only the clock leaves the mode alone:

```console
$ gallo spi set-config --frequency 1000000
$ gallo spi get-config
SPI frequency: 1000000 Hz
SPI phase:     CaptureOnFirstTransition (CPHA=0)
SPI polarity:  IdleLow (CPOL=0)
```

Any other mode is one flag away, and out-of-range values are rejected
rather than silently masked:

```console
$ gallo spi set-config --frequency 1000000 --mode 3
$ gallo spi set-config --frequency 1000000 --mode 4
error: invalid value '4' for '--mode <MODE>': 4 is not in 0..=3
```

### Batch (Atomic Under CS)

A single transaction with chip-select held low for the duration:

```console
$ gallo spi batch --cs 0 --op write:0x9f --op read:3
Read data (3 bytes):
  0000: ef 40 18                                           .@.
```

The `--cs` flag picks which user GPIO drives chip-select. Firmware validates
the encoded operations first, then requires the index to be inside
`0..DeviceInfo::num_gpios`, not monitored for GPIO events, and not explicitly
configured as an input. These refusals occur before chip-select is driven:
an invalid index does not touch any pin, and the other refusals leave the
selected pin's direction, level, and pull unchanged.

For an accepted transaction, firmware configures the pin as an output, drives
it high, asserts it low for the batch, and deasserts it high after execution
even when an SPI operation fails. The prior direction and level are **not**
restored, but a pull configured through `gpio/set-config` is preserved.
Firmware predating this contract may instead reconfigure an explicit input
pin. See [Transaction Batching](./batching.md).

### Zephyr chip-select mapping

The Zephyr SPI controller driver reaches the same batch endpoint, but a child
node's `reg` is a chip-select *selector*, not a firmware GPIO index. It indexes
the controller's `cs-gpio-indices` array, whose elements are the firmware GPIO
indices this page describes:

```dts
&pdg_spi0 {
	status = "okay";
	cs-gpio-indices = <2 0>;
};
```

Here a child with `reg = <0>` drives firmware GPIO index 2 and a child with
`reg = <1>` drives index 0. There is no identity fallback.

The controller validates the mapping *before* it validates transfer buffers, so
a devicetree mistake never reaches the bus. A controller without
`cs-gpio-indices`, a selector beyond the array, or a mapped index at or beyond
the firmware-reported GPIO count each return `-EINVAL`. Firmware reporting zero
GPIOs returns `-ENODEV`. A mapped pin explicitly configured as an input returns
`-EACCES`; a pin under a live GPIO event subscription returns `-EBUSY`.

Each refusal is logged with the selector, the mapping length, the mapped index,
and the reported GPIO count. Stacked drivers collapse these into a generic
not-ready error — `jedec,spi-nor`, for instance, reports `-ENODEV` for any
transfer failure — so the controller's own log line is the only authoritative
diagnosis.

Duplicate indices are permitted, but every child mapped to one index selects
the same physical line. That is safe only when the hardware intentionally
shares selection; mapping physically distinct peripherals to one index selects
them simultaneously, and both may drive MISO, producing bus contention, invalid
returned bytes, and possible electrical over-drive.

Zephyr `cs-gpios` is rejected with `-ENOTSUP`: driving chip select from the
Zephyr side would split one atomic firmware batch across multiple USB
round-trips, losing the chip-select interval guarantee described above.
`zephyr/README.md` in the repository remains the detailed module guide.

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

## Host chip-select preflight

Every host surface checks the chip-select index against the GPIO count
the connected device reports in `device/info`, *before* the pin is driven
and before any `spi/batch` request is transmitted. The bound is therefore
runtime-authoritative: it comes from the board, not from a compile-time
constant.

The count is resolved lazily. The first call that needs it performs one
implicit validated `device/info` round-trip — bounded at 300 seconds —
and caches the result for the lifetime of the connection; handles cloned
from the same connection share that cache. A failed lookup is not cached,
so the next call retries.

The failure modes stay disjoint, which is the point:

- the index is at or beyond the reported count → invalid chip-select;
- the device reports zero GPIOs → its own distinct error, for every index;
- the count could not be established (transport failure, 300-second
  timeout, legacy firmware, schema mismatch) → a communications /
  compatibility error, **never** an invalid chip-select. Misreporting a
  metadata failure as a bad argument would send you hunting for a bug in
  your own code.

A refused chip-select drives no pin and transmits nothing, so the pin
keeps whatever direction you configured.

> **Schema-freeze caveat.** On this branch, a successful validation does
> not prove wire-*shape* compatibility: `DeviceInfo` changed while both
> host and firmware still report schema 0.6.1. Validation bounds how long
> you wait and checks the reported numbers; it cannot make those numbers
> trustworthy.

```rust,no_run
use embedded_hal::spi::{Operation, SpiDevice};
use pico_de_gallo_hal::Hal;

fn read_jedec(hal: &Hal) -> [u8; 3] {
    // `spi_device` returns a Result: the chip-select is validated against
    // the device-reported GPIO count before the pin is driven.
    let mut spi = hal.spi_device(0).expect("CS 0 is valid on this board");
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

| Variant            | Meaning                                              |
|--------------------|------------------------------------------------------|
| `BufferTooLong`    | Request exceeds firmware buffer limit                |
| `Other`            | Catch-all for firmware-reported SPI failure          |
| `InvalidCsPin`     | Chip-select index outside `0..DeviceInfo::num_gpios` |
| `CsPinUnavailable` | Chip-select pin is explicitly configured as an input |
| `CsPinMonitored`   | Chip-select pin is monitored for GPIO events         |

See [`appendix/status-codes.md`](../appendix/status-codes.md) for
the FFI mapping.
