# Transaction Batching

When talking to I<sup>2</sup>C or SPI devices, a single logical transaction
often requires **multiple bus operations** — for example, writing a
register address and then reading back its value. Without batching, each
operation is a separate USB round-trip:

```text
Host ──write──▸ USB ──▸ Firmware ──▸ I²C bus    (~1 ms)
Host ◂──ack──── USB ◂── Firmware ◂── I²C bus    (~1 ms)
Host ──read───▸ USB ──▸ Firmware ──▸ I²C bus    (~1 ms)
Host ◂──data─── USB ◂── Firmware ◂── I²C bus    (~1 ms)
                                            Total: ~4 ms
```

Transaction batching packs all operations into a **single USB transfer**
and preserves one bus-level transaction. For I<sup>2</sup>C, the firmware
sends one START before the first operation, repeated STARTs only when the
direction changes, and one STOP after the last operation. It returns all
results at once:

```text
Host ──[write, read]──▸ USB ──▸ Firmware ──▸ I²C bus    (~1 ms)
Host ◂──[data]──────── USB ◂── Firmware ◂── I²C bus    (~1 ms)
                                            Total: ~2 ms
```

For transactions with many operations, this is a **10–50× speedup** —
USB latency dominates, not bus time.

## Using Batched Transactions from the CLI

The `gallo` CLI exposes batch operations directly. Each `--op` flag
specifies one bus operation.

### I<sup>2</sup>C Batch

Write a register address, then read back 2 bytes:

```console
$ gallo i2c batch -a 0x48 --op write:0x00 --op read:2
Read data (2 bytes):
  0000: 19 80                                              ..
```

Send a two-byte EEPROM address and three data bytes as one gather write:

```console
$ gallo i2c batch -a 0x50 --op write:0x00,0x10 --op write:0xab,0xcd,0xef
Batch complete (no read data)
```

The adjacent writes above produce the byte sequence `00 10 ab cd ef`
under one address phase; there is no STOP or repeated START between them.
In general, the operations execute as a single I<sup>2</sup>C transaction:
the bus is not released until the batch completes, and each direction
change (write-to-read or read-to-write) produces a documented repeated
START and re-addressing. This requires firmware from schema 0.7 or newer.
Older firmware executes every operation as a separate transaction.

#### Available I<sup>2</sup>C operations

| Operation | Syntax | Description |
|-----------|--------|-------------|
| Read      | `read:N` | Read *N* bytes from the device |
| Write     | `write:B1,B2,...` | Write the given bytes (hex `0x..` or decimal) |

### SPI Batch

Read a JEDEC ID from a SPI flash (command `0x9F`, 3-byte response):

```console
$ gallo spi batch --cs 0 --op write:0x9f --op read:3
Read data (3 bytes):
  0000: ef 40 18                                           .@.
```

Full-duplex transfer followed by a delay:

```console
$ gallo spi batch --cs 1 --op transfer:0x01,0x02,0x03 --op delay:1000 --op read:4
Read data (7 bytes):
  0000: ff ff ff 00 00 00 00                               .......
```

The `--cs` flag specifies which GPIO pin is used as chip-select. Valid
values are `0..num_gpios`, where `num_gpios` is the count the connected
device reports in `device/info` — not a fixed range baked into the CLI.
`gallo` validates the firmware on startup and checks `--cs` against that
reported count *before* parsing the operations or transmitting anything,
so an out-of-range chip-select drives no pin. A device reporting zero
GPIOs produces its own distinct error rather than an out-of-range one.

The firmware asserts CS low before the first operation and deasserts it
after the last — all operations run atomically under chip-select.

#### Available SPI operations

| Operation | Syntax | Description |
|-----------|--------|-------------|
| Read      | `read:N` | Clock in *N* bytes (MISO only) |
| Write     | `write:B1,B2,...` | Clock out the given bytes (MOSI only) |
| Transfer  | `transfer:B1,B2,...` | Full-duplex: send on MOSI, receive same count on MISO |
| DelayNs   | `delay:NS` | Delay for *NS* nanoseconds (best-effort, firmware resolution) |

## Using the Lib Crate Directly

If you need batch transactions from Rust code, use the
`i2c_batch` and `spi_batch` methods on the `PicoDeGallo` client. These
accept typed operation slices (`&[I2cBatchOp]` / `&[SpiBatchOp]`)
directly — no manual encoding needed.

### I<sup>2</sup>C batch example

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, I2cBatchOp};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pg = PicoDeGallo::new();

    // Build a "write register pointer, then read 2 bytes" transaction
    let ops = [
        I2cBatchOp::Write { data: &[0x00] },       // pointer register
        I2cBatchOp::Read { len: 2 },                // read temperature
    ];

    let result = pg.i2c_batch(0x48, &ops).await?;
    let temp_raw = u16::from_be_bytes([result[0], result[1]]);
    let celsius = (temp_raw >> 4) as f32 * 0.0625;
    println!("Temperature: {celsius:.2} °C");

    Ok(())
}
```

### SPI batch example

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, SpiBatchOp};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pg = PicoDeGallo::new();

    // Read JEDEC ID: send command 0x9F, then read 3 bytes
    let ops = [
        SpiBatchOp::Write { data: &[0x9F] },
        SpiBatchOp::Read { len: 3 },
    ];

    let result = pg.spi_batch(0, &ops).await?;
    println!(
        "JEDEC ID: manufacturer=0x{:02x}, type=0x{:02x}, capacity=0x{:02x}",
        result[0], result[1], result[2]
    );

    Ok(())
}
```

## Transparent Batching via the HAL Crate

The most powerful aspect of transaction batching is that **you don't
need to use it explicitly**. The HAL crate's `embedded-hal` trait
implementations use batch endpoints automatically.

When you call `I2c::transaction()` or `SpiDevice::transaction()` from
the HAL crate, the implementation encodes all operations into a single
batch request, sends it over USB, and unpacks the results — all
transparently. The firmware also preserves the corresponding bus-level
transaction: I<sup>2</sup>C operations follow the START, repeated-START,
and STOP rules above, while SPI operations remain under one chip-select
assertion.

This means any existing device driver that uses the standard
`embedded-hal` transaction API gets the bus-level transaction semantics
and the performance benefit for free:

```rust,no_run
use embedded_hal::i2c::I2c;
use embedded_hal::i2c::Operation;
use pico_de_gallo_hal::Hal;

fn read_tmp102(hal: &Hal) -> Result<f32, Box<dyn std::error::Error>> {
    let mut i2c = hal.i2c();
    let mut buf = [0u8; 2];

    // This is one USB round-trip and one I2C transaction
    i2c.transaction(
        0x48,
        &mut [
            Operation::Write(&[0x00]),   // set pointer to temperature register
            Operation::Read(&mut buf),   // read 2-byte temperature
        ],
    )?;

    let raw = u16::from_be_bytes(buf);
    Ok((raw >> 4) as f32 * 0.0625)
}
```

Similarly, `SpiDevice::transaction()` batches all SPI operations and
manages chip-select automatically:

```rust,no_run
use embedded_hal::spi::SpiDevice;
use embedded_hal::spi::Operation;
use pico_de_gallo_hal::Hal;

fn read_spi_jedec(hal: &Hal) -> Result<[u8; 3], Box<dyn std::error::Error>> {
    // `spi_device` returns a Result: the chip-select is checked against
    // the device-reported GPIO count before the pin is driven, so an
    // out-of-range pin is refused here rather than reconfigured.
    let mut spi = hal.spi_device(0)?;  // CS on GPIO 0
    let mut id = [0u8; 3];

    // One USB round-trip: CS asserted, command sent, ID read, CS deasserted
    spi.transaction(&mut [
        Operation::Write(&[0x9F]),
        Operation::Read(&mut id),
    ])?;

    Ok(id)
}
```

### Before vs. After

Consider a logical bus transaction containing three operations:

| Approach | USB Round-Trips | Approx. Latency |
|----------|-----------------|-----------------|
| Without batching (3 × write/read) | 6 | ~6 ms |
| With batching (1 × batch) | 2 | ~2 ms |

The latency improvement grows with the number of operations. Unlike
separate calls, the batch also preserves one bus-level transaction.

## Wire Format Details

For those interested in the protocol internals, batch operations use
postcard serialization. Each `I2cBatchOp` or `SpiBatchOp` is serialized
individually using `postcard::to_slice`, and the resulting bytes are
concatenated into the `ops` field. The firmware decodes them one at a
time using `postcard::take_from_bytes`.

### I<sup>2</sup>C operation encoding (postcard)

| Variant | Encoding |
|---------|----------|
| `Read { len }` | varint `0` (variant index) + varint `len` |
| `Write { data }` | varint `1` + varint data length + raw bytes |

### SPI operation encoding (postcard)

| Variant | Encoding |
|---------|----------|
| `Read { len }` | varint `0` + varint `len` |
| `Write { data }` | varint `1` + varint data length + raw bytes |
| `Transfer { data }` | varint `2` + varint data length + raw bytes |
| `DelayNs { ns }` | varint `3` + varint `ns` |

The `count` field in each batch request struct tells the firmware how
many operations to expect, providing an additional safety check during
decoding.

The response for both I<sup>2</sup>C and SPI is simply the concatenated
read (and transfer) data. The host already knows the expected lengths
from the request, so no framing is needed in the response.

### Limits

| Parameter | Value |
|-----------|-------|
| Maximum operations per batch | 64 (`MAX_BATCH_OPS`) |
| Maximum size of an individual operation | None — bounded only by the total request |
| Protocol packet-buffer/argument bound | 4096 bytes (`MAX_TRANSFER_SIZE`) |
| Direct I²C RPC measurements | Read: 1014 bytes after a 1-byte write; write: no failure through 4096 bytes |
| Demonstrated SPI payload | Shape-dependent and below 4096; no general ceiling is published |
| Zero-length I²C writes | Rejected with `ZeroLengthWrite` before bus access |

If a batch violates these constraints, the firmware returns an error
indicating which constraint was violated. Validation failures report the
exact zero-indexed operation; an I<sup>2</sup>C bus failure applies to the
atomic transaction as a whole and reports operation index 0.

`MAX_TRANSFER_SIZE` is a buffer and argument bound, not a guarantee that 4096
bytes of application data fit through postcard-rpc and COBS framing. Request
shape and response size both matter. The I²C measurements used the direct
`i2c/write` and `i2c/write-read` RPCs; `i2c/batch` was not measured. See the
measured I²C and SPI evidence in
[Troubleshooting](../appendix/troubleshooting.md#buffertoolong-22).
