# Changelog

All notable changes to `pico-de-gallo-ffi` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/ffi-v0.6.0...ffi-v0.10.0) (2026-06-22)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **ffi:** add gallo_gpio_wait_for_*_with_timeout_ms, Status::GpioTimeout ([c90f5db](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/c90f5db2eed88c01aa6ae0faf174546796971cc4))
* **ffi:** add gallo_init_strict for validation-on-construct ([24dd759](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/24dd759cc2f3dba899c8335fa4352b78a0eb0808))
* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))
* **lib,hal,ffi,application,pyco:** enforce schema validation, expose HAL recovery ([c8e2f13](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/c8e2f13be1bacf83e905d9e1453f6ec4b3abc69c))
* **lib:** add gpio_wait_for_*_with_timeout, bump internal to 0.7 ([9840232](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/98402325a49a21f773d30fba7007c2da8addd698))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * pico-de-gallo-lib bumped from 0.6.0 to 0.10.0

## [Unreleased]

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

- Bumped `pico-de-gallo-lib` dependency to 0.7.1 (validate() now
  also checks `schema_major`, so the new `gallo_init_strict`
  surfaces major-version skew that the previous validation
  silently accepted).

### Added (2026-06-03 — Category A hotfix wire PR, already on main as 0.8.0)

- `gallo_gpio_wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout_ms`
  C functions. `timeout_ms == 0` preserves wait-forever behavior;
  non-zero bounds the firmware-side wait and returns
  `Status::GpioTimeout` on expiry. Available on firmware schema
  0.7+; older firmware returns `Status::SchemaMismatch`.
- `Status::GpioTimeout = -70` enum variant (appended at end of
  `Status` enum; preserves stable C ABI per AGENTS.md §8).

### Changed (2026-06-03 — Category A hotfix wire PR, already on main as 0.8.0)

- Bumped `pico-de-gallo-lib` dependency to 0.7.0 for the
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
