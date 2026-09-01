# I²C

Pico de Gallo provides a single I²C bus on the RP2350's hardware
**I²C1** controller. SDA is on **GPIO 2** and SCL on **GPIO 3**.
The v1.1 PCB includes on-board 4.7 kΩ pull-ups; on v1.0 you must
supply your own.

## Operations

| Operation | Description |
|-----------|-------------|
| **Read**       | Read N bytes from a device at the given address |
| **Write**      | Write bytes to a device at the given address |
| **Write-Read** | Write then read on the same target (repeated start, no STOP between) |
| **Scan**       | Probe every address on the bus |
| **Batch**      | One I²C transaction; repeated START on direction change; final STOP only |
| **Set Config** | Change the bus clock frequency at runtime |
| **Get Config** | Query the current bus configuration |

## Bus Frequencies

| Variant      | Value     | Standard name |
|--------------|-----------|---------------|
| `Standard`   | 100 kHz   | I²C Standard mode |
| `Fast`       | 400 kHz   | I²C Fast mode |
| `FastPlus`   | 1 MHz     | I²C Fast-mode Plus |

The firmware defaults to Standard mode.

## CLI

```console
$ gallo i2c --help
I2C access methods

Usage: gallo i2c <COMMAND>

Commands:
  scan        Scan I2C bus for existing devices
  read        Read bytes through the I2C bus from device at given address
  write       Write bytes through I2C bus to device at given address
  write-read  Write bytes follwed by read bytes
  set-config  Set I2C bus parameters
  get-config  Query the current I2C bus configuration
  batch       Execute multiple I2C operations in a single USB transfer
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### Scanning

> [!WARNING]
>
> The RP235x I²C controller doesn't expose a pure address-probe
> primitive, so `gallo i2c scan` does a 1-byte **read** at each
> address. Devices that ACK a read are reported as present. A
> handful of peripherals may end up in an unexpected state after
> being probed this way — usually a power cycle clears it.

```console
$ gallo i2c scan
╭────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────╮
│    │  0 │  1 │  2 │  3 │  4 │  5 │  6 │  7 │  8 │  9 │  a │  b │  c │  d │  e │  f │
├────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┤
│ 0  │ RR │ RR │ RR │ RR │ RR │ RR │ RR │ RR │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 1  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 2  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 3  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 4  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ 48 │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 5  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 6  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ 68 │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 7  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ RR │ RR │ RR │ RR │ RR │ RR │ RR │ RR │
╰────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────╯
```

`RR` marks reserved I²C addresses. Pass `-r` (`--include-reserved`)
to probe them anyway.

### Read / Write / Write-Read

```console
$ gallo i2c read --address 0x48 --count 2
6b 15

$ gallo i2c write --address 0x48 --bytes 0x01 0xe0 0xa0

$ gallo i2c write-read --address 0x48 --bytes 0x00 --count 2
6b 15
```

Read output supports `-f hex` (default), `-f binary`, and
`-f ascii`.

### Config

```console
$ gallo i2c set-config --frequency fast
$ gallo i2c get-config
Frequency: Fast (400 kHz)
```

### Batch

One USB round-trip carries one multi-operation I²C transaction. A START
and address precede the first operation. Adjacent operations in the same
direction run back to back without a STOP or repeated START, so adjacent
writes form one gather write. A direction change emits a documented
repeated START and re-addressing, and only the final operation is followed
by a STOP.

This bus framing requires firmware from schema 0.7 or newer. Older
firmware executes each operation as a separate transaction.

```console
$ gallo i2c batch -a 0x48 --op write:0x00 --op read:2
Read data (2 bytes):
  0000: 19 80                                              ..
```

See [Transaction Batching](./batching.md) for the full mechanism.

## Measured Transfer Limits

On the measured schema-0.7 setup, an I²C read of **1014 bytes** after a
one-byte write returned exactly the requested length. The 1015-byte probe
failed in about 99 ms; larger tested lengths returned the same host-side
response decode error, `Postcard(DeserializeUnexpectedEnd)`, with increasing
latency, reaching 391 ms at 4096. No truncation was found at the checked lengths
of 1, 64, 256, 512, 1000, 1013 and 1014 bytes.

No failing write request length was observed through **4096 bytes**. Every probe
crossed USB intact, was decoded, initiated a bus transaction and returned the
expected address NACK. The target address was unpopulated, so no payload byte
was clocked: this verifies request framing at 4096, not successful bus-level
clocking of a 4096-byte payload. The probe could go no higher because 4096 is
the wire-representable `MAX_TRANSFER_SIZE`.

No hang was found at any tested I²C length, but that does not prove that no I²C
hang window exists. The combined write/read frontier also remains unmeasured
beyond a one-byte write, because the available TMP102 rejects longer writes.
Treating the read and write bounds as independent is an **inference from
mechanism, not measurement**: a shared roughly 1015-byte framing budget is hard
to reconcile with a 4096-byte write request being delivered, decoded and
initiating a bus transaction, while the observed failure occurs during response
decoding and write data travels in the request.

The Zephyr driver contains these observations locally by rejecting reads above
1014 bytes and writes above 4096 bytes with `-EMSGSIZE`. This containment does
not protect the `gallo` CLI, Rust library, C FFI, Python bindings, or MCP tools;
those callers can still request larger reads and encounter the response decode
error. See [troubleshooting](../appendix/troubleshooting.md#buffertoolong-22).

## Rust Library

All `PicoDeGallo` methods are `async`. `PicoDeGallo::new()` is
**not** async.

```rust,no_run
use pico_de_gallo_lib::{I2cBatchOp, I2cFrequency, PicoDeGallo};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pg = PicoDeGallo::new();

    pg.i2c_set_config(I2cFrequency::Fast).await?;

    // Plain write-read
    let data = pg.i2c_write_read(0x48, &[0x00], 2).await?;
    let raw = u16::from_be_bytes([data[0], data[1]]);
    println!("raw = 0x{raw:04x}");

    // Same bus framing as i2c_write_read above: write, repeated START, read, STOP.
    let ops = [
        I2cBatchOp::Write { data: &[0x00] },
        I2cBatchOp::Read { len: 2 },
    ];
    let _ = pg.i2c_batch(0x48, &ops).await?;
    Ok(())
}
```

## HAL

The HAL exposes the bus as an
[`embedded_hal::i2c::I2c`] / [`embedded_hal_async::i2c::I2c`]
implementor — so any driver written against those traits Just
Works:

```rust,no_run
use embedded_hal::i2c::I2c;
use pico_de_gallo_hal::Hal;

fn read_tmp102(hal: &Hal) {
    let mut i2c = hal.i2c();
    let mut buf = [0u8; 2];
    i2c.write_read(0x48, &[0x00], &mut buf).unwrap();
    let raw = u16::from_be_bytes(buf);
    let celsius = (raw >> 4) as f32 * 0.0625;
    println!("Temperature: {celsius:.2} °C");
}
```

`I2c::transaction()` sends all operations in one USB round-trip and,
with schema 0.7 or newer firmware, executes them as one I²C transaction.
Adjacent same-direction operations run without an intervening STOP; a
direction change emits a documented repeated START. See
[Transaction Batching](./batching.md).

## C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

void read_tmp102(PicoDeGallo *gallo) {
    uint8_t tx[] = {0x00};
    uint8_t rx[2];
    Status s = gallo_i2c_write_read(gallo, 0x48, tx, 1, rx, 2);
    if (s != Ok) { fprintf(stderr, "write-read failed: %d\n", s); return; }
    uint16_t raw = ((uint16_t)rx[0] << 8) | rx[1];
    printf("raw = 0x%04x\n", raw);
}
```

I²C frequency is passed as `uint8_t`: `0 = Standard`, `1 = Fast`,
`2 = FastPlus`. See [`crates/ffi.md`](../crates/ffi.md).

## Python

```python
from pyco_de_gallo import PycoDeGallo, I2cFrequency

pg = PycoDeGallo()
pg.i2c_set_config(I2cFrequency.Fast)

data = pg.i2c_write_read(0x48, [0x00], 2)
raw = (data[0] << 8) | data[1]
print(f"raw = 0x{raw:04x}")
```

## Error Handling

I²C operations return `PicoDeGalloError<I2cError>` on the Rust
side; FFI returns negative `Status` values:

| Variant              | Meaning                                  |
|----------------------|------------------------------------------|
| `Bus`                | Unexpected condition on the I²C bus      |
| `NoAcknowledge`      | Target did not acknowledge               |
| `ArbitrationLoss`    | Lost arbitration to another master       |
| `Overrun`            | Data overrun on read                     |
| `BufferTooLong`      | Request exceeds firmware buffer limit    |
| `AddressOutOfRange`  | Address outside the 7-bit range          |
| `Other`              | Unspecified firmware error               |
| `ZeroLengthWrite`    | Write requested with an empty payload    |

The full status-code mapping for FFI lives in
[`appendix/status-codes.md`](../appendix/status-codes.md).
`ZeroLengthWrite` maps to `InvalidArgument` (-5).

## Zero-Length Writes Are Not Supported

A write with an empty payload — the address-only `START + ADDR + STOP`
probe that some I²C stacks use for bus scanning — is rejected by the
firmware with `ZeroLengthWrite`. This applies to `i2c/write` and to any
`Write` operation inside an `i2c/batch`; the batch is refused as a whole
during validation, so no earlier operation in it reaches the bus.

Every host surface also refuses it locally, before the request is
transmitted, so the call fails immediately instead of spending a USB
round-trip to be told no. The Zephyr module is not a host surface — it is
an FFI consumer — but it refuses the same shape locally, and is listed
here alongside them:

| Surface | Refusal |
|---|---|
| `pico-de-gallo-lib` | `PicoDeGalloError::Endpoint(I2cError::ZeroLengthWrite)` |
| `pico-de-gallo-hal` | `I2cHalError::I2c(ZeroLengthWrite)`, whose `ErrorKind` is `Other` |
| C FFI | `Status::InvalidArgument` (-5); for a batch, `*out_failed_op` names the operation |
| `pyco-de-gallo` | `RuntimeError` carrying the same message |
| `gallo-mcp` | An invalid-argument error naming the offending operation |
| `gallo` CLI | Unreachable: the byte arguments require at least one value |
| Zephyr module | `-ENOTSUP`; set `CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ` to substitute a 1-byte read instead. See [`zephyr/README.md`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/zephyr/README.md) |

Except for the Zephyr opt-in, which substitutes a read rather than
refusing, the local refusal returns the identical error the firmware
would have returned, so callers need not distinguish the two.

Note that `i2c/write-read` is **not** affected: an empty write phase
there is legal, because that transfer does not terminate with a STOP.
Probing with `i2c_write_read(addr, &[], n)` works.

The restriction is a hardware limitation, not a firmware policy choice.
The RP2040/RP2350 `DW_apb_i2c` block drives the address phase only as a
side effect of pushing data into `IC_DATA_CMD`, so there is no way to
emit an address without at least one payload byte. See
[rp-rs/rp-hal#678](https://github.com/rp-rs/rp-hal/issues/678) and
[embassy-rs/embassy#4474](https://github.com/embassy-rs/embassy/issues/4474).

To probe for a device, use a 1-byte read instead:

```bash
gallo i2c read --address 0x48 --count 1   # NoAcknowledge => absent
gallo i2c scan                            # or scan the whole bus
```

`i2c/scan` already probes this way. Note that a read probe is not
semantically identical to a write probe: a write-only device may
acknowledge its write address while refusing a read address.
