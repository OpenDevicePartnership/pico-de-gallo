# Changelog

All notable changes to `pico-de-gallo-ffi` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/ffi-v0.6.0...ffi-v1.0.0) (2026-05-29)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

## [Unreleased]

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
