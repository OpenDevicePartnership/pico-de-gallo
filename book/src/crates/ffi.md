# `pico-de-gallo-ffi`

`pico-de-gallo-ffi` is the C-facing surface for Pico de Gallo. It wraps
`pico-de-gallo-lib` behind an **opaque pointer** and a stable `Status` enum so
C, C++, Zig, and other FFI-friendly languages can use the device without
knowing anything about Rust internals.

At a glance:

- the device handle is opaque: C code only sees `const PicoDeGallo *`,
- the handle is safe to share across threads (`Send + Sync` on the Rust side),
- all FFI calls drive the async Rust client on a shared multi-threaded Tokio
  runtime (required by the `postcard-rpc` nusb transport),
- the crate builds as a `cdylib`:
  - Linux: `libpico_de_gallo_ffi.so`
  - macOS: `libpico_de_gallo_ffi.dylib`
  - Windows: `pico_de_gallo_ffi.dll`

## Lifecycle

Every FFI program follows the same three-step shape:

1. create a handle,
2. call `gallo_*` functions,
3. free the handle.

```c
#include "pico_de_gallo.h"

const PicoDeGallo *gallo = gallo_init();
uint32_t id = 42;
Status s = gallo_ping(gallo, &id);
gallo_free(gallo);
```


### Initialization and teardown

| Function | Purpose |
|---|---|
| `const PicoDeGallo *gallo_init(void)` | Connect to the first matching board (lazy — failures surface on first RPC) |
| `const PicoDeGallo *gallo_init_with_serial_number(const char *serial)` | Connect to a board with a specific USB serial number (lazy) |
| `const PicoDeGallo *gallo_init_strict(void)` | Like `gallo_init` but calls `validate()` before returning; returns `NULL` on schema mismatch or device-not-present |
| `const PicoDeGallo *gallo_init_strict_with_serial_number(const char *serial)` | Like the above with serial-number selection; recommended in production |
| `void gallo_free(const PicoDeGallo *gallo)` | Release the opaque handle; `NULL` is a safe no-op |

## Status Codes

All operational functions return `Status`.

- `Status::Ok` is success.
- All failures are negative values.
- The values are part of the **stable C ABI**.

> [!WARNING]
> `Status` values are append-only. Do not renumber existing codes, and do not
> overload an old value with a new meaning. Existing C callers may already have
> those integers compiled into `switch` statements.

The full status-code list lives in the
[Status Code Reference](../appendix/status-codes.md).

## Limits and configuration enums

The header exports the firmware's transfer limits and pin count, so C
callers can size buffers and validate indices from the same numbers the
firmware enforces instead of hard-coding copies:

```c
#define GALLO_MAX_TRANSFER_SIZE 4096
#define GALLO_MAX_BATCH_OPS 64
#define GALLO_NUM_GPIOS 4
```

`GALLO_MAX_TRANSFER_SIZE` mirrors the protocol's 4096-byte packet-buffer and
local argument bound. It is **not** a guarantee that 4096 bytes of application
payload can traverse the framed transport; deliverable size depends on the
operation's request and response shape. Exceeding the local bound yields
`Status::BufferTooLong`, while a smaller framed request can still fail in
transport. See the measured [SPI limits](../interfaces/spi.md#holding-chip-select-and-the-fault-latch).
Exceeding `GALLO_MAX_BATCH_OPS` in `gallo_i2c_batch` or `gallo_spi_batch`
yields `Status::InvalidArgument`.

> [!WARNING]
> The Zephyr driver's 1013-byte containment does not apply to C callers. A
> 1015-byte TX-only SPI request reproduced a device-wide firmware-dispatcher
> wedge. Until an operation-specific host limit exists, keep individual SPI
> payloads at or below 512 bytes; see [troubleshooting](../appendix/troubleshooting.md#buffertoolong-22).

`GALLO_NUM_GPIOS` bounds the valid pin indices, `0..GALLO_NUM_GPIOS`, which
map to physical GPIO8-GPIO11 on the Pico 2 header. Anything at or above it
yields `Status::GpioInvalidPin`. Note the dedicated `SPI_CS` header pin
(GPIO5) is not one of these and is not currently claimed by the firmware.

For the SPI chip select in `gallo_spi_batch`, prefer the *runtime* count
from `gallo_num_gpios` over this compile-time constant — see below.
`GALLO_NUM_GPIOS` is only this build's default, and the library checks
`cs_pin` against what the connected board actually reports.


All three mirror constants in `pico-de-gallo-internal`, and a compile-time
assertion in `pico-de-gallo-ffi` fails the build if they ever disagree.

The header also names the integer values that the `gallo_*` entry points
already accept and return as `uint8_t`:

| Enum                  | Values                                                                    | Used by                                            |
|-----------------------|---------------------------------------------------------------------------|----------------------------------------------------|
| `GalloI2cFrequency`   | `_Standard` 0, `_Fast` 1, `_FastPlus` 2                                   | `gallo_i2c_set_config`, `gallo_i2c_get_config`     |
| `GalloGpioDirection`  | `_Input` 0, `_Output` 1                                                   | `gallo_gpio_set_config`                            |
| `GalloGpioPull`       | `_None` 0, `_Up` 1, `_Down` 2                                             | `gallo_gpio_set_config`                            |
| `GalloGpioEdge`       | `_Rising` 0, `_Falling` 1, `_Any` 2                                       | `gallo_gpio_subscribe`                             |
| `GalloI2cBatchOpTag`  | `_Read` 0, `_Write` 1                                                     | `GalloI2cBatchOp::tag`                             |
| `GalloSpiBatchOpTag`  | `_Read` 0, `_Write` 1, `_Transfer` 2, `_DelayNs` 3                        | `GalloSpiBatchOp::tag`                             |

Variant names are prefixed with the enum name (`GalloGpioPull_Up`), because C
enum variants share a single global namespace and several of these would
otherwise collide — both batch-op tags define `Read` and `Write`.

> [!NOTE]
> These enums are additive. The function signatures still take plain
> `uint8_t`, so existing code that passes literals keeps working unchanged.

> [!WARNING]
> Like `Status`, these values are **stable C ABI** and must match the
> `pico-de-gallo-internal` wire enums they mirror. Wire-enum variant order is
> itself ABI, because postcard serializes by variant index. A unit test
> (`config_enums_match_wire_enums`) asserts the two numberings agree.

## Function Reference

The generated header is the canonical API surface, but these are the functions
you will use most often.

### Ping and device metadata

```c
Status gallo_ping(const PicoDeGallo *gallo, uint32_t *id);

Status gallo_version(const PicoDeGallo *gallo,
                     uint16_t *major, uint16_t *minor, uint32_t *patch);

Status gallo_get_device_info(const PicoDeGallo *gallo, GalloDeviceInfo *info);

Status gallo_num_gpios(const PicoDeGallo *gallo, uint8_t *out_num_gpios);

Status gallo_system_reset_subscriptions(const PicoDeGallo *gallo,
                                        uint8_t *out_reset);
```

`gallo_get_device_info` returns firmware version, schema version, hardware
revision, and a capability bitfield.

`gallo_num_gpios` returns the GPIO count the connected device reports —
the runtime-authoritative bound for a pin index and for the SPI chip
select. It performs one `device/info` round-trip on first use, bounded at
300 seconds, and caches the value per handle; the round-trip also
validates the firmware's reported schema version. `*out_num_gpios` is
written only on `Ok`, including a successful zero.

`gallo_system_reset_subscriptions` tears down any GPIO subscriptions
left over from a previous host session and writes the reset count to
`*out_reset` (which may be `NULL` if the caller does not need the
count). Subscriptions are server-side state that outlives the USB
transport, so a host that crashed without calling
`gallo_gpio_unsubscribe` leaves the affected pins owned by firmware
monitor tasks. Call this once on connect, immediately after
`gallo_init` (or after `validate()` in the Rust library), to reclaim
those pins. The call is idempotent and cheap on a fresh device.

### I<sup>2</sup>C

```c
Status gallo_i2c_read(const PicoDeGallo *gallo,
                      uint8_t address, uint8_t *buf, size_t len);
Status gallo_i2c_write(const PicoDeGallo *gallo,
                       uint8_t address, const uint8_t *buf, size_t len);
Status gallo_i2c_write_read(const PicoDeGallo *gallo,
                            uint8_t address,
                            const uint8_t *txbuf, size_t txlen,
                            uint8_t *rxbuf, size_t rxlen);
Status gallo_i2c_scan(const PicoDeGallo *gallo,
                      bool include_reserved,
                      uint8_t *buf, size_t buf_len, size_t *found);
Status gallo_i2c_set_config(const PicoDeGallo *gallo, uint8_t frequency);
Status gallo_i2c_get_config(const PicoDeGallo *gallo, uint8_t *out_frequency);
```

`frequency` uses the wire enum encoding: `0 = Standard`, `1 = Fast`,
`2 = FastPlus`. The header also defines `GalloI2cFrequency`
(`GalloI2cFrequency_Standard`, `_Fast`, `_FastPlus`) if you would rather pass a
name than a number — see [Limits and configuration enums](#limits-and-configuration-enums).

#### I<sup>2</sup>C batch

```c
typedef struct GalloI2cBatchOp {
    uint8_t       tag;       // 0 = Read, 1 = Write
    uint16_t      read_len;  // Read variant
    const uint8_t *data;     // Write variant (must be non-NULL; data_len > 0)
    size_t        data_len;  // Write variant
} GalloI2cBatchOp;

Status gallo_i2c_batch(const PicoDeGallo *gallo,
                       uint8_t address,
                       const GalloI2cBatchOp *ops, size_t ops_count,
                       uint8_t *out_buf, size_t out_capacity,
                       size_t *out_len,
                       uint16_t *out_failed_op);  // may be NULL
```

The batch executes as a single I<sup>2</sup>C transaction. A START and address
precede the first operation; adjacent operations of the same type are sent back
to back with no STOP and no repeated START between them, so two adjacent Write
ops form one gather write; a direction change emits a repeated START and
re-addresses the target; and only the last operation is followed by a STOP.

A `Write` op must carry at least one byte. `gallo_i2c_batch` rejects
`data_len == 0` with `InvalidArgument` before contacting the device,
writing the offending operation's index to `*out_failed_op`. Likewise
`gallo_i2c_write` rejects `len == 0`. This hardware cannot emit an
address-only transaction — see
[Zero-length writes](../interfaces/i2c.md#zero-length-writes-are-not-supported).

This requires firmware built from schema 0.7 or newer. Older firmware executes
each operation as a separate transaction. Zero-length writes are rejected.

Concatenated read data is written to `out_buf` and the total length to
`*out_len`. On failure, `*out_failed_op` (if non-NULL) receives the zero-based
index of the operation that failed, and the status reflects the underlying
I<sup>2</sup>C error (`I2cNack`, `I2cBusError`, etc.). `BufferTooLong` means
`out_buf` was too small; `*out_len` still receives the required capacity.

### SPI

```c
Status gallo_spi_read(const PicoDeGallo *gallo, uint8_t *buf, size_t len);
Status gallo_spi_write(const PicoDeGallo *gallo, const uint8_t *buf, size_t len);
Status gallo_spi_flush(const PicoDeGallo *gallo);
Status gallo_spi_set_config(const PicoDeGallo *gallo,
                            uint32_t frequency,
                            bool spi_phase, bool spi_polarity);
Status gallo_spi_get_config(const PicoDeGallo *gallo,
                            uint32_t *out_frequency,
                            bool *out_phase, bool *out_polarity);
```

#### SPI full-duplex transfer

```c
Status gallo_spi_transfer(const PicoDeGallo *gallo,
                          const uint8_t *write_buf,
                          uint8_t       *read_buf,
                          size_t         len);
```

Simultaneously sends `len` bytes from `write_buf` on MOSI and receives
`len` bytes on MISO into `read_buf`. The two buffers may alias.
Returns `BufferTooLong` if `len` exceeds the firmware transfer limit,
or `SpiTransferFailed` on a generic SPI error.

#### SPI batch

```c
typedef struct GalloSpiBatchOp {
    uint8_t       tag;       // 0 = Read, 1 = Write, 2 = Transfer, 3 = DelayNs
    uint16_t      read_len;  // Read variant
    const uint8_t *data;     // Write/Transfer variant (may be NULL when data_len == 0)
    size_t        data_len;  // Write/Transfer variant
    uint32_t      delay_ns;  // DelayNs variant
} GalloSpiBatchOp;

Status gallo_spi_batch(const PicoDeGallo *gallo,
                       uint8_t cs_pin,
                       const GalloSpiBatchOp *ops, size_t ops_count,
                       uint8_t *out_buf, size_t out_capacity,
                       size_t *out_len,
                       uint16_t *out_failed_op);  // may be NULL
```

The firmware asserts `cs_pin` low before the first operation and
deasserts it after the last (or on error), providing atomic
`SpiDevice::transaction` semantics. Read data from `Read` and
`Transfer` operations is concatenated into `out_buf` in order. On
per-op failure, `*out_failed_op` (if non-NULL) receives the zero-based
index. `BufferTooLong` means `out_buf` was too small; `*out_len` still
receives the required capacity.

#### Chip-select preflight

`cs_pin` is validated against the device-reported GPIO count before any
operation payload is translated and before anything is transmitted:

```c
uint8_t n = 0;
Status s = gallo_num_gpios(gallo, &n);   // writes n only on Ok
```

`gallo_num_gpios` performs one `device/info` round-trip on first use —
which also validates the firmware's reported schema version — and caches
the answer per handle. A device reporting zero is a success and writes
zero; every error leaves your buffer untouched, so a sentinel written
before the call is a reliable "was this populated?" check.

`gallo_spi_batch` returns:

| Status | Meaning |
|---|---|
| `SpiInvalidCsPin` (−71) | `cs_pin` is at or beyond the reported count |
| `SpiNoGpios` (−74) | the device reports zero GPIOs |
| `DeviceInfoFailed` (−62) | the count could not be read (transport/decode) |
| `DeviceInfoTimeout` (−75) | `device/info` did not answer within 300 seconds |
| `LegacyFirmware` (−64) | the firmware has no `device/info` endpoint |
| `SchemaMismatch` (−63) | host and firmware disagree on the wire version |

The last four mean the host could not establish the valid range at all;
they are never reported as an invalid chip-select. A refused chip-select
drives no pin, sends nothing, leaves `*out_len` at zero, and never writes
`*out_failed_op` — only a firmware-side per-operation failure does that.

Ordering inside `gallo_spi_batch` is fixed: validate `gallo`, validate
`out_len`, validate the top-level `ops`/`ops_count`/`out_capacity` shape,
write `*out_len = 0`, resolve the GPIO count, classify `cs_pin`, translate
the operation payloads, call the library once, then write outputs. A NULL
`out_len` is therefore never dereferenced, and an invalid op `tag` or NULL
`data` pointer is reported only after the device has been reached.

### GPIO

```c
Status gallo_gpio_get(const PicoDeGallo *gallo, uint8_t pin, bool *state);
Status gallo_gpio_put(const PicoDeGallo *gallo, uint8_t pin, bool state);
Status gallo_gpio_wait_for_high(const PicoDeGallo *gallo, uint8_t pin);
Status gallo_gpio_wait_for_low(const PicoDeGallo *gallo, uint8_t pin);
Status gallo_gpio_wait_for_rising_edge(const PicoDeGallo *gallo, uint8_t pin);
Status gallo_gpio_wait_for_falling_edge(const PicoDeGallo *gallo, uint8_t pin);
Status gallo_gpio_wait_for_any_edge(const PicoDeGallo *gallo, uint8_t pin);
Status gallo_gpio_set_config(const PicoDeGallo *gallo,
                             uint8_t pin, uint8_t direction, uint8_t pull);
Status gallo_gpio_subscribe(const PicoDeGallo *gallo, uint8_t pin, uint8_t edge);
Status gallo_gpio_unsubscribe(const PicoDeGallo *gallo, uint8_t pin);
```

### UART

```c
Status gallo_uart_read(const PicoDeGallo *gallo,
                       uint8_t *buf, uint16_t count,
                       uint32_t timeout_ms, uint16_t *out_len);
Status gallo_uart_write(const PicoDeGallo *gallo,
                        const uint8_t *buf, uint16_t len);
Status gallo_uart_flush(const PicoDeGallo *gallo);
Status gallo_uart_set_config(const PicoDeGallo *gallo, uint32_t baud_rate);
Status gallo_uart_get_config(const PicoDeGallo *gallo, uint32_t *out_baud_rate);
```

### PWM

```c
Status gallo_pwm_set_duty_cycle(const PicoDeGallo *gallo,
                                uint8_t channel, uint16_t duty);
Status gallo_pwm_get_duty_cycle(const PicoDeGallo *gallo,
                                uint8_t channel,
                                uint16_t *out_duty, uint16_t *out_max_duty);
Status gallo_pwm_enable(const PicoDeGallo *gallo, uint8_t channel);
Status gallo_pwm_disable(const PicoDeGallo *gallo, uint8_t channel);
Status gallo_pwm_set_config(const PicoDeGallo *gallo,
                            uint8_t channel,
                            uint32_t frequency_hz, bool phase_correct);
Status gallo_pwm_get_config(const PicoDeGallo *gallo,
                            uint8_t channel,
                            uint32_t *out_frequency_hz,
                            bool *out_phase_correct, bool *out_enabled);
```

### ADC

```c
Status gallo_adc_read(const PicoDeGallo *gallo,
                      uint8_t channel, uint16_t *out_value);
Status gallo_adc_get_config(const PicoDeGallo *gallo,
                            uint8_t *out_resolution_bits,
                            uint16_t *out_nominal_reference_mv,
                            uint8_t *out_num_gpio_channels);
```

### 1-Wire

```c
Status gallo_onewire_reset(const PicoDeGallo *gallo, bool *out_present);
Status gallo_onewire_read(const PicoDeGallo *gallo,
                          uint8_t *buf, uint16_t len, uint16_t *out_len);
Status gallo_onewire_write(const PicoDeGallo *gallo,
                           const uint8_t *buf, uint16_t len);
Status gallo_onewire_write_pullup(const PicoDeGallo *gallo,
                                  const uint8_t *buf, uint16_t len,
                                  uint16_t pullup_duration_ms);
Status gallo_onewire_search(const PicoDeGallo *gallo,
                            uint64_t *out_rom_ids, uint16_t max_count,
                            uint16_t *out_count);
```

## Building and Linking

### Build the shared library

```bash
cd crates/pico-de-gallo-ffi
cargo build --release
```

Outputs:

| Platform | Artifact |
|---|---|
| Linux | `target/release/libpico_de_gallo_ffi.so` |
| macOS | `target/release/libpico_de_gallo_ffi.dylib` |
| Windows | `target/release/pico_de_gallo_ffi.dll` and `pico_de_gallo_ffi.dll.lib` |

### Static library

The crate also builds a `staticlib`, alongside the shared library rather than
instead of it:

| Platform | Artifact |
|---|---|
| Linux, macOS | `target/release/libpico_de_gallo_ffi.a` |
| Windows | `target/release/pico_de_gallo_ffi.lib` |

Link it when you want a self-contained executable and no runtime search-path
setup — the Zephyr module does exactly this so the `native_sim` runner carries
the FFI inside `zephyr.exe` and needs no `-Wl,-rpath`. Expect a substantially
larger binary, since the whole async runtime and USB stack come along.

A Rust `staticlib` does not carry its transitive system-library requirements,
so a manual link needs them spelled out. Ask the toolchain rather than
guessing:

```bash
cargo rustc --release --crate-type staticlib -- --print native-static-libs
```

On x86-64 Linux that currently reports:

```text
-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc
```

The list is platform-specific; re-run it on macOS or Windows instead of
reusing the Linux one. Only the shared library is published as a release
asset — see the release workflow for the exact asset names.

### Generated header

The header is generated by `cbindgen` during the build. Look under Cargo's
`OUT_DIR` for `pico_de_gallo.h`:

```text
target/release/build/pico-de-gallo-ffi-<hash>/out/include/pico_de_gallo.h
```

> [!NOTE]
> Do not hand-edit the header. It is generated from the Rust definitions and is
> supposed to stay in lockstep with them.

### cbindgen notes

`cbindgen.toml` in the crate root controls generation. The important bits are:

- language: C,
- include guard: `PICO_DE_GALLO_H`,
- style: both tagged and typedef forms,
- line endings: LF,
- an `[export] include` list.

Two cbindgen behaviours are worth knowing before you add anything to the
header, because both fail **silently**:

- **Unreferenced types are pruned.** cbindgen only emits types reachable from
  an exported function signature. The configuration enums above appear in no
  signature, so they must be listed in `[export] include` or they vanish from
  the header with no warning.
- **Const initializers must be literals.** cbindgen folds them syntactically,
  so `pub const A: usize = some::path::B;` compiles cleanly and emits nothing.
  Write the literal and guard it with a `const` assertion instead — that is
  what `GALLO_MAX_TRANSFER_SIZE`, `GALLO_MAX_BATCH_OPS` and
  `GALLO_NUM_GPIOS` do.

## Complete Example

```c
#include <stdint.h>
#include <stdio.h>
#include "pico_de_gallo.h"

int main(void) {
    const PicoDeGallo *gallo = gallo_init();
    if (!gallo) {
        fprintf(stderr, "Failed to connect to device\n");
        return 1;
    }

    uint32_t id = 0xDEADBEEF;
    Status s = gallo_ping(gallo, &id);
    if (s != Ok) {
        fprintf(stderr, "Ping failed: %d\n", s);
        gallo_free(gallo);
        return 1;
    }
    printf("Ping OK, got back: 0x%08X\n", id);

    uint16_t major, minor;
    uint32_t patch;
    s = gallo_version(gallo, &major, &minor, &patch);
    if (s == Ok) {
        printf("Firmware v%u.%u.%u\n", major, minor, patch);
    }

    GalloDeviceInfo info;
    s = gallo_get_device_info(gallo, &info);
    if (s == Ok) {
        printf("Schema v%u.%u.%u, HW rev %u\n",
               info.schema_major, info.schema_minor,
               info.schema_patch, info.hw_version);
    } else if (s == SchemaMismatch) {
        fprintf(stderr, "Schema mismatch — update firmware or host library\n");
    }

    uint8_t buf[2] = {0};
    s = gallo_i2c_read(gallo, 0x50, buf, sizeof(buf));
    if (s != Ok) {
        fprintf(stderr, "I2C read failed: %d\n", s);
        gallo_free(gallo);
        return 1;
    }

    printf("Read: 0x%02X 0x%02X\n", buf[0], buf[1]);
    gallo_free(gallo);
    return 0;
}
```
