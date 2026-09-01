# Changelog

All notable changes to `pico-de-gallo-ffi` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- Appended `char build_id[GALLO_BUILD_ID_LEN]` to `GalloDeviceInfo`, with
  `GALLO_BUILD_ID_LEN` fixed at 65 bytes including the NUL terminator.
  `gallo_get_device_info` populates it only on `Status::Ok`; no status code was
  added or renumbered. Closes #159.

## [0.8.0] — 2026-08-27

### Added

- Added `gallo_num_gpios` and the coherent #104 status set:
  `SpiInvalidCsPin = -71`, `SpiCsPinUnavailable = -72`,
  `SpiCsPinMonitored = -73`, `SpiNoGpios = -74`, and
  `DeviceInfoTimeout = -75`. Existing C ABI status codes remain stable
  and are never renumbered.

### Changed

- SPI batch calls now bound-check chip-select against device-reported
  metadata on the host, keeping an invalid pin, zero GPIOs, and metadata
  failure as distinct outcomes. Closes #104.

- `gallo_i2c_write` rejects `len == 0`, and `gallo_i2c_batch` rejects a
  `Write` op with `data_len == 0`, returning `InvalidArgument` before the
  device is contacted — before the `gallo` pointer is even dereferenced. For
  a batch, `*out_failed_op` receives the offending operation's index and
  `*out_len` is set to zero. Firmware has refused these since #101, so no
  previously-working call breaks; the refusal is now local and costs no USB
  round-trip. Closes #136.

- **Documentation correction.** The `GalloI2cBatchOp` doc comment and the
  book previously stated that `data` may be `NULL` when `data_len == 0`,
  which advertised the exact call that used to wedge the device. A `Write`
  op now documents `data_len > 0` and a non-NULL `data`. Only the I²C batch
  op changed; `GalloSpiBatchOp` still permits an empty payload, which is
  legitimate for SPI.

- Corrected the `gallo_i2c_batch` documentation to describe one atomic I²C
  transaction: adjacent same-direction operations concatenate, direction
  changes use a repeated START, and only the final operation receives a STOP.
  A bus failure now returns `out_failed_op = 0` because it cannot be attributed
  to one operation; validation failures retain an exact index.
  The C ABI is unchanged. Closes #128.

- `I2cError::ZeroLengthWrite` (new upstream in `pico-de-gallo-internal`)
  maps to the existing `InvalidArgument = -5`. No new status code is
  added: `Status` values are stable C ABI, and a new value falls through
  the exhaustive `switch ((enum Status)x)` that C consumers are told to
  write. A zero-length write is an argument this hardware cannot honour,
  so `InvalidArgument` is the honest existing code. Closes #101.

## [0.7.1] — 2026-08-03

### Changed

- The C FFI API now runs on a shared multi-threaded Tokio runtime
  instead of a per-call `futures::executor::block_on`. This prevents
  `postcard-rpc`'s nusb transport from panicking when it spawns
  background tasks during device initialization, and keeps those
  background tasks between individual FFI calls. The C ABI is
  unchanged — the exported functions, `#[repr(C)]` struct layouts, and
  `Status` code values in `pico_de_gallo.h` are the same (only a
  documentation-comment example in the generated header was
  reformatted).
- Bumped the `pico-de-gallo-lib` dependency to 0.7.1 for its new
  fallible `try_new` / `try_new_with_serial_number` constructors.

### Fixed

- `gallo_init_strict()` and `gallo_init_strict_with_serial_number()`
  now return `NULL` (with a diagnostic on stderr) when no matching
  device is reachable or the USB interface cannot be claimed, instead
  of panicking. They are now built on `pico-de-gallo-lib`'s fallible
  `try_new` constructors rather than the panicking `new()`.

## [0.7.0] — 2026-06-22

### Added (2026-06-04 — Category A hotfix host-only PR)

- `gallo_init_strict()` and `gallo_init_strict_with_serial_number(c_serial_number)`.
  Both call `PicoDeGallo::validate()` internally before returning
  the opaque pointer. Return `NULL` on device-not-found, schema
  version mismatch, legacy firmware, or any validation error.
  Prefer in production C code over the lazy `gallo_init` —
  failures (device not present, schema mismatch) surface at
  construct time rather than on the first RPC. Closes Category A
  finding #4 at the FFI layer.

### Changed (2026-06-04 — Category A hotfix host-only PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0 (validate() now
  also checks `schema_major`, so the new `gallo_init_strict`
  surfaces major-version skew that the previous validation
  silently accepted).

### Added (2026-06-03 — Category A hotfix wire PR)

- `gallo_gpio_wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout_ms`
  C functions. `timeout_ms == 0` preserves wait-forever behavior;
  non-zero bounds the firmware-side wait and returns
  `Status::GpioTimeout` on expiry. Available on firmware schema
  0.6+; older firmware returns `Status::SchemaMismatch`.
- `Status::GpioTimeout = -70` enum variant (appended at end of
  `Status` enum; preserves stable C ABI per AGENTS.md §8).

### Changed (2026-06-03 — Category A hotfix wire PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0 for the
  `gpio_wait_for_*_with_timeout` host methods.

### Added

- `gallo_system_reset_subscriptions(const PicoDeGallo *, uint8_t
  *out_reset)`. `out_reset` may be `NULL`. New appended `Status`
  code: `SystemResetSubscriptionsFailed = -69`.
- `gallo_spi_transfer`, `gallo_spi_batch`, and `gallo_i2c_batch`
  expose the high-throughput SPI full-duplex and atomic CS-held
  batch primitives (and the equivalent I<sup>2</sup>C multi-op
  primitive) to C consumers that previously could only call them
  from Rust. Batch ops are passed via C-friendly tagged structs
  (`GalloSpiBatchOp`, `GalloI2cBatchOp`); on per-operation failure,
  an optional `out_failed_op` pointer receives the zero-based index
  of the failing op. Three new appended `Status` codes:
  `I2cBatchFailed = -66`, `SpiBatchFailed = -67`,
  `SpiTransferFailed = -68`. The wire protocol is unchanged — these
  are pure FFI surface additions over existing endpoints.
  ([REVIEW-2026-05-29 P1-2])

### Changed

- All `gallo_*` functions now take `const PicoDeGallo *` for the
  device handle (previously `PicoDeGallo *` on every function
  except `gallo_init*` / `gallo_free`). The C ABI (pointer width,
  calling convention, status codes) is unchanged, but C consumers
  that typed their handle as `PicoDeGallo *` and previously cast
  away `const` on every call can now drop those casts. Header
  consumers with `-Wcast-qual` enabled will stop warning. The
  opaque handle remains thread-safe (`Send + Sync`) and
  interior-mutable. ([REVIEW-2026-05-29 P1-4])

## [0.6.0] — 2026-05-04

### Added

- `gallo_get_device_info()` function, `GalloDeviceInfo` C struct
  with `capabilities` u64 bitfield, `GALLO_CAP_*` constants. 4 new
  status codes: `DeviceInfoFailed` (−62), `SchemaMismatch` (−63),
  `LegacyFirmware` (−64), `Unsupported` (−65).

## [0.5.0] — 2026-04-22

### Breaking Changes

- Added 8 new status codes (`I2cNack`, `I2cBusError`,
  `I2cArbitrationLoss`, `I2cOverrun`, `BufferTooLong`,
  `I2cAddressOutOfRange`, `GpioInvalidPin`, `CommsFailed`).

### Added

- `gallo_gpio_subscribe(pin, edge)` and `gallo_gpio_unsubscribe(pin)`
  FFI functions. 4 new status codes: `GpioPinMonitored` (-54),
  `GpioPinNotMonitored` (-55), `GpioSubscribeFailed` (-56),
  `GpioUnsubscribeFailed` (-57).
- 6 PWM FFI functions (`gallo_pwm_set_duty_cycle`,
  `gallo_pwm_get_duty_cycle`, `gallo_pwm_enable`,
  `gallo_pwm_disable`, `gallo_pwm_set_config`,
  `gallo_pwm_get_config`) and 9 status codes (-41 to -49).
- 2 ADC FFI functions (`gallo_adc_read`, `gallo_adc_get_config`)
  and 4 status codes (-50 to -53).
- 5 1-Wire FFI functions (`gallo_onewire_reset`,
  `gallo_onewire_read`, `gallo_onewire_write`,
  `gallo_onewire_write_pullup`, `gallo_onewire_search`) and 5
  status codes (-57 to -61).
- 5 UART FFI functions (`gallo_uart_read`, `gallo_uart_write`,
  `gallo_uart_flush`, `gallo_uart_set_config`,
  `gallo_uart_get_config`) and 10 status codes (-31 to -40).
- `gallo_i2c_scan()` function (writes responding addresses to
  caller buffer) and `I2cScanFailed` status code.
- `gallo_gpio_set_config()` function and `GpioSetConfigFailed` /
  `GpioWrongDirection` status codes.
- `gallo_i2c_get_config()` and `gallo_spi_get_config()` functions,
  `I2cGetConfigFailed` and `SpiGetConfigFailed` status codes.

## [0.4.0] — 2025-04-20

### Breaking Changes

- Split `gallo_set_config()` into `gallo_i2c_set_config()` and
  `gallo_spi_set_config()`.

### Added

- Compile-time `Send + Sync` assertion for thread safety.

## [0.3.0] — 2025-03-15

### Changed

- Updated dependencies to match library changes.
