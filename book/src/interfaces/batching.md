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
| Maximum size of an individual operation | None — bounded only by the totals below |
| **Total bytes a batch may return** | **1014 (`MAX_RESPONSE_PAYLOAD`)** |
| **Total bytes a batch may send** | **Bounded by the whole request frame: 5119 bytes (`MAX_REQUEST_FRAME`), header and encoding overhead included — see [The request-frame ceiling](#the-request-frame-ceiling)** |
| Protocol packet-buffer/argument bound | 4096 bytes (`MAX_TRANSFER_SIZE`) |
| Plain read/duplex endpoint bound | 1014 bytes (`MAX_RESPONSE_PAYLOAD`) |
| Plain write endpoint bound | 4096 bytes (`MAX_TRANSFER_SIZE`) |
| Direct I²C RPC measurements | Read: 1014 bytes after a 1-byte write; write: no failure through 4096 bytes |
| Batch measurements | Reads totalling 1014 bytes return in full; 1015 is refused with `BufferTooLong`, on both `i2c/batch` and `spi/batch`. A request frame of 5119 bytes is executed; 5120 is refused with `BufferTooLong` |
| Demonstrated SPI payload (send direction) | Shape-dependent and below 4096; no general ceiling is published |
| Zero-length I²C writes | Rejected with `ZeroLengthWrite` before bus access |

If a batch violates these constraints, the firmware returns an error
indicating which constraint was violated. Validation failures report the
exact zero-indexed operation; an I<sup>2</sup>C bus failure applies to the
atomic transaction as a whole and reports operation index 0. An
aggregate-length overflow is not attributable to any single operation, so it
reports index 0 as well.

`MAX_TRANSFER_SIZE` is a buffer and argument bound, not a guarantee that 4096
bytes of application data fit through postcard-rpc and COBS framing. Request
shape and response size both matter. The direct-RPC I²C measurements used
`i2c/write` and `i2c/write-read`; the batch row was measured separately,
through `i2c/batch` and `spi/batch`. See the measured I²C and SPI evidence in
[Troubleshooting](../appendix/troubleshooting.md#buffertoolong-22).

Issue #179 introduced the aggregate returned-data bound for batches. Issue
#158 applies the same directional rule to the plain endpoints and every host
surface: reads and full-duplex transfers use `MAX_RESPONSE_PAYLOAD`, while
send-only writes use `MAX_TRANSFER_SIZE`. Issue #186 completed the pair by
bounding a batch's aggregate *outgoing* bytes against `MAX_REQUEST_FRAME`. In
every case an over-ceiling call is refused before transmission.

#### The response ceiling

`MAX_RESPONSE_PAYLOAD` bounds what a batch may send **back**, not what the
firmware can buffer. For I<sup>2</sup>C only `Read` counts towards it; for SPI,
`Read` and `Transfer` both do, because a transfer returns as many bytes as it
sends. `Write` and `DelayNs` contribute nothing.

The number is a property of the host transport rather than of the device: a
postcard-rpc response frame is read into a 1024-byte buffer, of which 7 bytes
are the header, 1 is the `Result` discriminant, and 2 are the payload's length
prefix. The firmware enforces it anyway. Until this was fixed, a batch whose
reads totalled between 1015 and 4096 bytes was accepted and **executed** — for
I<sup>2</sup>C the whole transaction ran, including every `Write`; for SPI
chip-select was asserted and deasserted and the clock was driven — and only
*then* lost its response to truncation. The caller received
`Comms(Postcard(DeserializeUnexpectedEnd))`, which looks like a transport
fault and invites a retry that repeats every write.

Refusing up front costs the 1015–4096 range, which never worked anyway, and
buys the guarantee that a batch the device accepts is a batch whose result the
caller can actually see. Split larger reads across several batches.

#### The request-frame ceiling

`MAX_REQUEST_FRAME` (5119) bounds what a batch may send **out**, and it is the
only thing that does. A batch's `Write` (and SPI `Transfer`) bytes all travel
in one request frame, and that frame has to fit in the firmware's 5120-byte
receive buffer, less one byte that an exactly-filling frame cannot use.
[Wire Protocol](../internals/wire-protocol.md#the-request-frame-ceiling)
derives both numbers term by term.

Unlike the response ceiling, this one is **not** the firmware's to enforce. An
over-ceiling frame is discarded by postcard-rpc's receiver before any handler
runs, so the device cannot refuse what it never sees. The bound is applied
host-side, before transmission, in `check_i2c_batch_ops` and
`check_spi_batch_ops`; an over-ceiling batch is rejected with
`BufferTooLong` and `failed_op = 0`, the same aggregate-overflow convention the
response ceiling uses.

It bounds the **aggregate**, not any single operation. Two consequences worth
stating plainly:

- A batch of individually modest writes can still overrun it. The hardware
  measurement below tripped it with eight 635-byte writes, none of them
  anywhere near `MAX_TRANSFER_SIZE`.
- Conversely, a single batch `Write` *is* allowed past `MAX_TRANSFER_SIZE`,
  which a plain `i2c_write` would refuse. That is deliberate: a batch write
  streams straight out of the received frame and never enters the firmware's
  4096-byte scratch buffer, so the argument bound does not apply to it. The
  frame is its real limit.

Sizing a batch exactly means accounting for postcard overhead — a variant byte
and a length varint per operation, plus a header, a selector byte and two more
varints for the request. `pico-de-gallo-lib` exports
`i2c_batch_request_frame_len` and `spi_batch_request_frame_len` so callers can
compute it rather than model it. Callers who would rather not: keep a batch's
total payload comfortably under 5000 bytes, or split it.

##### Why the bound is slightly conservative

The frame's header is 13 bytes until the client has received its first reply
and 7 bytes afterwards, because postcard-rpc narrows the key width only once a
reply comes back (see
[Wire Protocol](../internals/wire-protocol.md#the-request-frame-ceiling)). The
host cannot ask which state it is in — the field is private to postcard-rpc —
so the bound assumes the wider header. On a connection that has already
received a reply, six more payload bytes would in fact have fitted.

That is the price of an answer that does not depend on connection state. The
alternative is worse: a bound computed from the narrow header would accept a
batch that is silently dropped whenever it happens to be the first request a
process sends.

##### Measurements

Measured through `pico-de-gallo-lib`'s `i2c_batch` on board
`49742081C885AC69` (hw-rev2), one `Write` operation against a non-responding
address, from a fresh process so the header is the wide one:

| Payload | Encoded ops | Request frame | Result before #186 | Result now |
|--------:|------------:|--------------:|---|---|
| 5099 | 5102 | 5119 | `NoAcknowledge` — the batch ran | unchanged |
| 5100 | 5103 | 5120 | `Timeout` after 5 s — the frame never arrived | `BufferTooLong`, immediately |

Re-measured on firmware `firmware-v0.11.0-88-gfcb38de3a1ff` for issue #186,
with the host guard deliberately stubbed out to exercise the device alone; the
edge was identical to the original `-79-gc3c6a3e07bec` run. The multi-operation
arm, eight writes against the same address, tripped at 635 bytes each and
passed at 634.

The pre-#186 failure was worse on SPI than on I²C. A batch carrying no
`DelayNs` operation is bounded by the firmware's 30-minute handler ceiling
rather than the ordinary call timeout, so an over-ceiling `spi/batch` left the
caller waiting over half an hour — confirmed still running after 90 seconds in
the same control arm — for a request the device had never received. Nothing was
driven in either case: the bus stayed idle and chip-select never moved, which
is why this is a diagnosability defect rather than a corruption one.
