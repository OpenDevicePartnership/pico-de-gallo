# Changelog

All notable changes to `pico-de-gallo-lib` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/library-v0.5.0...library-v1.0.0) (2026-05-29)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

## [Unreleased]

### Added

- `PicoDeGallo::system_reset_subscriptions()` host method returns
  the number of subscriptions reset. The recommended connect
  sequence is now `new()` → `validate().await?` →
  `system_reset_subscriptions().await?`.
- `MAX_BATCH_OPS` and `MAX_TRANSFER_SIZE` are now re-exported from
  `pico-de-gallo-internal` so downstream consumers don't have to
  pull in the wire crate just to validate batch sizes.

### Fixed

- `PicoDeGallo::validate()` no longer mis-classifies transport,
  postcard-decode, and frame-size errors as
  `ValidateError::LegacyFirmware`. Only `WireError::UnknownKey` and
  `WireError::KeyTooSmall` (the postcard-rpc signals for "this
  firmware has no handler for that endpoint key") map to
  `LegacyFirmware`; every other host error routes to
  `ValidateError::Comms`, so users see "comms failure" instead of
  being told to upgrade firmware that is already current. Surfaces
  in `gallo_get_device_info` as the correct `Status::CommsFailed`
  (−1) when the wire is the actual problem. ([REVIEW-2026-05-29
  P1-1])

## [0.5.0] — 2026-05-04

### Breaking Changes

- `uart_get_config()` now returns `PicoDeGalloError<UartError>` and
  `adc_get_config()` now returns `PicoDeGalloError<AdcError>` (was
  `PicoDeGalloError<Infallible>`).

### Added

- `device_info()` and `validate()` methods, `ValidateError` enum.
  Re-exported `Capabilities` and `DeviceInfo`.

## [0.4.0] — 2026-04-22

### Breaking Changes

- All method return types updated from `PicoDeGalloError<*Fail>` to
  `PicoDeGalloError<I2cError>`, `PicoDeGalloError<SpiError>`, or
  `PicoDeGalloError<GpioError>`.

### Added

- `gpio_subscribe(pin, edge)`, `gpio_unsubscribe(pin)`, and
  `subscribe_gpio_events(depth)` methods. Re-exported `GpioEdge`,
  `GpioEvent`, `IoClosed`, `MultiSubscription`.
- `i2c_batch(address, ops)` and `spi_batch(cs, ops)` async methods.
  Re-exported `I2cBatchOp`, `SpiBatchOp`, `encode_i2c_batch_ops`,
  `encode_spi_batch_ops`, `I2cBatchError`, `SpiBatchError`.
- `pwm_set_duty_cycle`, `pwm_get_duty_cycle`, `pwm_enable`,
  `pwm_disable`, `pwm_set_config`, `pwm_get_config` async methods.
  Re-exported `PwmError`, `PwmDutyCycleInfo`,
  `PwmConfigurationInfo`.
- `adc_read(channel)`, `adc_get_config()` methods. Re-exported
  `AdcChannel`, `AdcError`, `AdcConfigurationInfo`.
- `onewire_reset()`, `onewire_read(len)`, `onewire_write(data)`,
  `onewire_write_pullup(data, duration_ms)`, `onewire_search()`,
  `onewire_search_next()` methods. Re-exported `OneWireError`.
- `uart_read(count, timeout_ms)`, `uart_write(contents)`,
  `uart_flush()`, `uart_set_config(baud_rate)`,
  `uart_get_config()` methods. Re-exported `UartError` and
  `UartConfigurationInfo`.
- `PicoDeGallo::i2c_scan(include_reserved)` method returning
  `Vec<u8>`.
- `PicoDeGallo::gpio_set_config(pin, direction, pull)` method;
  re-exported `GpioDirection` and `GpioPull`.
- `PicoDeGallo::i2c_get_config()` and `spi_get_config()` methods;
  re-exported `SpiConfigurationInfo`.

### Fixed

- Corrected `MAX_TRANSFER_SIZE` references in rustdoc for
  `i2c_read`, `i2c_write_read`, and `spi_read` (was 512, actual
  value is 4096).

## [0.3.0] — 2025-04-20

### Breaking Changes

- Split `set_config()` into `i2c_set_config()` and
  `spi_set_config()`.
- `PicoDeGalloError` is now generic over the endpoint error type.

### Added

- `list_devices()` function for enumerating connected boards.
- `Display` and `std::error::Error` implementations for
  `PicoDeGalloError`.

### Changed

- `client` field made private (was accidentally public).
