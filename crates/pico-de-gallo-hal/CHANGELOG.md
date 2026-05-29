# Changelog

All notable changes to `pico-de-gallo-hal` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/hal-v0.5.0...hal-v1.0.0) (2026-05-29)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

## [0.4.0] — 2026-04-22

### Breaking Changes

- Single `Error` type replaced with `I2cHalError`, `SpiHalError`,
  and `GpioHalError` — each wraps the endpoint-specific error plus
  a `Comms` variant. I2C `ErrorKind` mapping now returns accurate
  variants (NoAcknowledge, ArbitrationLoss, Bus, Overrun) instead
  of `Other` for all errors.
- `I2c::transaction()` and `SpiDevice::transaction()` now use batch
  endpoints under the hood — one USB round-trip per transaction
  instead of one per operation. This is a behavioral change:
  previously each operation in a transaction was an independent USB
  transfer.

### Added

- `gpio_subscribe(pin, edge)` and `gpio_unsubscribe(pin)` blocking
  methods. Re-exported `GpioEdge`, `GpioEvent`.
- `I2c::transaction()` and `SpiDevice::transaction()` (blocking and
  async) rewritten to use batch endpoints — 10–50× fewer USB
  round-trips for multi-operation transactions.
- `PwmChannel` wrapper implementing
  `embedded_hal::pwm::SetDutyCycle`. `PwmHalError` type.
  `Hal::pwm_channel(n)` accessor. `pwm_set_config` and
  `pwm_get_config` convenience methods on `Hal`.
- `AdcHalError` type. `Hal::adc_read(channel)`, `adc_get_config()`
  convenience methods.
- `OneWire` handle struct with blocking wrappers. `OneWireHalError`
  type. `Hal::onewire()` accessor.
- `Uart` wrapper struct implementing `embedded_io::Read`,
  `embedded_io::Write`, `embedded_io_async::Read`, and
  `embedded_io_async::Write`. `UartHalError` type with
  `embedded_io::Error` impl. `Hal::uart()` accessor with 1000ms
  default timeout.
- `Hal::i2c_scan(include_reserved)` method returning `Vec<u8>`.
- `SpiDev` type implementing both `embedded_hal::spi::SpiDevice`
  and `embedded_hal_async::spi::SpiDevice`. Manages chip-select
  (CS) via a GPIO pin, asserting CS low before operations and
  deasserting high afterward with automatic flush. Created via
  `Hal::spi_device(cs_pin)`.
- `Gpio::set_config(pin, direction, pull)` method.
- `Hal::i2c_get_config()` and `spi_get_config()` methods.

## [0.3.0] — 2025-04-20

### Breaking Changes

- Split `set_config()` into `i2c_set_config()` and
  `spi_set_config()`.

### Added

- Per-call async context detection (reuses existing tokio runtime
  if available).

## [0.2.0] — 2025-03-15

### Changed

- Updated dependencies and API to match library.
