# Changelog

All notable changes to `pico-de-gallo-lib` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] — 2026-09-01

### Breaking Changes

- Requires firmware reporting schema 0.8. Because the schema 0.8 change was
  to `DeviceInfo` itself, a 0.9.0 host against schema 0.7 firmware does not
  return `SchemaMismatch` — `device/info` is re-keyed, the reply is dropped
  unmatched, and `validate()` can only return `Timeout`. See the rewritten
  "Wire shape" note in `PicoDeGallo::validate()`'s documentation.

### Added

- Re-exported `BUILD_ID_CAPACITY`; `DeviceInfo::build_id()` exposes the
  firmware build identity without requiring callers to depend on `heapless`.
  `validate()` returns it for diagnostics but deliberately never treats it as
  a compatibility gate. Closes #159.

### Changed

- `ValidateError::Timeout`'s `Display` no longer advises a retry or a
  replug. A `device/info` timeout is at least as likely to be a
  host/firmware build mismatch: postcard-rpc derives each endpoint key from
  the response type's schema, so peers built from different trees answer
  under different keys and the reply is dropped as unmatched rather than
  decoded. Nothing fails, so nothing is reported — the call simply never
  completes. The message now names both causes and says plainly that the
  host cannot tell them apart. Refs #159.

## [0.8.0] — 2026-08-27

### Added

- Added cached, runtime-authoritative `PicoDeGallo::num_gpios()`.
  Closes #104.

- Hardware-in-the-loop tests for the zero-length write guards, `#[ignore]`d by
  default so CI does not run them. Run with
  `cargo test -p pico-de-gallo-lib -- --ignored --test-threads=1`; see the
  `hardware` module documentation for bench setup.

### Changed

- `spi_batch` now refuses `cs_pin >= num_gpios` locally without
  transmitting. A `device/info` failure remains a metadata error rather
  than becoming an invalid chip-select, and `num_gpios == 0` is distinct.
  The implicit metadata request uses the existing 300-second
  `DEVICE_INFO_TIMEOUT`. Closes #104.

- `PicoDeGallo::i2c_write` and `PicoDeGallo::i2c_batch` refuse a zero-length
  write locally, before transmitting. Firmware has refused it since #101, so
  this does not change which requests succeed — it removes a USB round-trip
  spent being told no, and makes the refusal independent of the attached
  firmware. Both return the value firmware would have returned:
  `Endpoint(I2cError::ZeroLengthWrite)`, and for a batch an `I2cBatchError`
  whose `failed_op` is the offending operation's exact index. A batch is
  validated in full before anything is sent, so a rejected batch never drives
  an earlier operation onto the bus. `i2c_write_read` is deliberately
  unaffected: an empty write phase there is legal, because that transfer does
  not terminate with a STOP. Closes #136.

- Reworded `PicoDeGalloError::Endpoint` documentation. It previously said the
  firmware had processed the request, which a locally refused request makes
  untrue. Callers still need not distinguish the two cases, but must not infer
  from this variant that the device was contacted.

- Documented that `PicoDeGallo::i2c_batch` executes its operations as one I²C
  transaction: adjacent same-direction operations concatenate, direction
  changes use a repeated START, and only the final operation receives a STOP.
  Bus failures report `failed_op = 0` for the transaction as a whole, while
  validation failures retain an exact index. The Rust API and wire shape are
  unchanged. Closes #128.

## [0.7.1] — 2026-07-28

### Added

- Add fallible constructors `PicoDeGallo::try_new()` and
  `PicoDeGallo::try_new_with_serial_number()` returning
  `Result<PicoDeGallo, String>`. Unlike `new()` / `new_with_serial_number()`
  (which panic when no matching device is present or the interface cannot be
  claimed), these surface the error, letting callers report "no device
  attached" or retry a transient claim failure. Additive and non-breaking.

## [0.7.0] — 2026-07-28

### Added

- Re-export `HostErr` (from `postcard_rpc::host_client`) and
  `WireError` (from `postcard_rpc::standard_icd`) from the crate
  root, and make the `host_client` module path public. This lets
  downstream crates name the transport error types when mapping
  [`PicoDeGalloError`] and [`ValidateError`] into their own error
  representations without taking a direct dependency on
  `postcard-rpc`. Additive and non-breaking; the names were
  previously imported privately for the crate's own use.

  Motivated by the new `gallo-mcp` crate, whose error mapping
  matches on `PicoDeGalloError::Comms(HostErr::Closed)` to surface
  a distinct "no device attached" message.

## [0.6.0] — 2026-06-22

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

### Added (2026-06-03 — Category A hotfix wire PR)

- `PicoDeGallo::gpio_wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout`
  methods take a `std::time::Duration` and return
  `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))` on expiry.
  The existing two-arg methods (`gpio_wait_for_high(pin)` etc.)
  preserve the wait-forever behavior by passing `timeout_ms: 0`
  on the wire. Closes Category A finding #2 at the host-library
  layer.

### Changed (2026-06-03 — Category A hotfix wire PR)

- Bumped `pico-de-gallo-internal` dependency to 0.6.0 (wire schema
  change: append-only `timeout_ms: u32` on `GpioWaitRequest`,
  append-only `GpioError::Timeout` variant). Lockstep with firmware
  0.10.0 per AGENTS.md §6.5.

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
