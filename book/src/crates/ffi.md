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

## Function Reference

The generated header is the canonical API surface, but these are the functions
you will use most often.

### Ping and device metadata

```c
Status gallo_ping(const PicoDeGallo *gallo, uint32_t *id);

Status gallo_version(const PicoDeGallo *gallo,
                     uint16_t *major, uint16_t *minor, uint32_t *patch);

Status gallo_get_device_info(const PicoDeGallo *gallo, GalloDeviceInfo *info);

Status gallo_system_reset_subscriptions(const PicoDeGallo *gallo,
                                        uint8_t *out_reset);
```

`gallo_get_device_info` returns firmware version, schema version, hardware
revision, and a capability bitfield.

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
`2 = FastPlus`.

#### I<sup>2</sup>C batch

```c
typedef struct GalloI2cBatchOp {
    uint8_t       tag;       // 0 = Read, 1 = Write
    uint16_t      read_len;  // Read variant
    const uint8_t *data;     // Write variant (may be NULL when data_len == 0)
    size_t        data_len;  // Write variant
} GalloI2cBatchOp;

Status gallo_i2c_batch(const PicoDeGallo *gallo,
                       uint8_t address,
                       const GalloI2cBatchOp *ops, size_t ops_count,
                       uint8_t *out_buf, size_t out_capacity,
                       size_t *out_len,
                       uint16_t *out_failed_op);  // may be NULL
```

Operations run sequentially with a STOP between each (this is *not*
repeated-start; for write-then-read to the same device use
`gallo_i2c_write_read`). Concatenated read data is written to `out_buf`
and the total length to `*out_len`. On failure, `*out_failed_op` (if
non-NULL) receives the zero-based index of the operation that failed,
and the status reflects the underlying I<sup>2</sup>C error
(`I2cNack`, `I2cBusError`, etc.). `BufferTooLong` means `out_buf` was
too small; `*out_len` still receives the required capacity.

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
- line endings: LF.

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