# Changelog

All notable changes to `pico-de-gallo-lib` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/library-v0.5.0...library-v0.10.0) (2026-06-22)


### ⚠ BREAKING CHANGES

* **internal:** wire-protocol minor bump. Pre-0.7 firmware cannot decode 0.7 client requests (they include 4 extra bytes per wait request). Pre-0.7 hosts cannot decode 0.7 firmware responses that return GpioError::Timeout (unknown enum variant). Lockstep release required per AGENTS.md §6.5.
* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))
* **internal:** add timeout_ms to GpioWaitRequest, GpioError::Timeout ([fdb3ba1](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/fdb3ba15e64d03a07167203cb3897930a7e3dfbf))
* **lib,hal,ffi,application,pyco:** enforce schema validation, expose HAL recovery ([c8e2f13](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/c8e2f13be1bacf83e905d9e1453f6ec4b3abc69c))
* **lib:** add gpio_wait_for_*_with_timeout, bump internal to 0.7 ([9840232](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/98402325a49a21f773d30fba7007c2da8addd698))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))
* **lib:** check schema_major in validate(), extend SchemaMismatch payload ([8257e2b](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/8257e2b0bbb8a3ece5f95685bd7de040dff2fdc1))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * pico-de-gallo-internal bumped from 0.6.0 to 0.10.0

## [Unreleased]

### Fixed (2026-06-04 — Category A hotfix host-only PR)

- `PicoDeGallo::validate()` now checks `schema_major` in addition
  to `schema_minor`. Previously, a firmware reporting a bumped
  major version with a matching minor would silently pass
  validation and the host would subsequently mis-decode wire
  bytes (silent garbage out). The schema-check policy is now
  extracted into a private `check_schema_compatible(&DeviceInfo)`
  helper with four regression tests covering matching versions
  and the three rejection cases (bumped major, bumped minor,
  both bumped).
- `ValidateError::SchemaMismatch` payload extended with
  `expected_major` and `actual_major` fields; `Display` impl
  shows the full `MAJOR.MINOR.x` skew rather than just the minor
  versions.

  This is a structural change to a public enum variant payload.
  Direct constructors and exhaustive matches against
  `SchemaMismatch` will need to add the two new fields. The
  variant is not on the wire (`ValidateError` is a host-side
  type), so there is no schema impact.

### Added (2026-06-03 — Category A hotfix wire PR, already on main as 0.7.0)

- `PicoDeGallo::gpio_wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout`
  methods take a `std::time::Duration` and return
  `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))` on expiry.
  The existing two-arg methods (`gpio_wait_for_high(pin)` etc.)
  preserve the wait-forever behavior by passing `timeout_ms: 0`
  on the wire. Closes Category A finding #2 at the host-library
  layer.

### Changed (2026-06-03 — Category A hotfix wire PR, already on main as 0.7.0)

- Bumped `pico-de-gallo-internal` dependency to 0.7.0 (wire schema
  change: append-only `timeout_ms: u32` on `GpioWaitRequest`,
  append-only `GpioError::Timeout` variant). Lockstep with firmware
  0.11.0 per AGENTS.md §6.5.

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
