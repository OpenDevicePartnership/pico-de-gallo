# Changelog

All notable changes to `pico-de-gallo-internal` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/internal-v0.6.0...internal-v0.11.0) (2026-07-23)


### ⚠ BREAKING CHANGES

* **internal:** wire-protocol minor bump. Pre-0.7 firmware cannot decode 0.7 client requests (they include 4 extra bytes per wait request). Pre-0.7 hosts cannot decode 0.7 firmware responses that return GpioError::Timeout (unknown enum variant). Lockstep release required per AGENTS.md §6.5.
* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))
* **internal:** add timeout_ms to GpioWaitRequest, GpioError::Timeout ([fdb3ba1](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/fdb3ba15e64d03a07167203cb3897930a7e3dfbf))
* **lib,hal,ffi,application,pyco:** enforce schema validation, expose HAL recovery ([c8e2f13](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/c8e2f13be1bacf83e905d9e1453f6ec4b3abc69c))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

## [0.6.0] — 2026-06-22

### Breaking Changes (2026-06-03 — Category A hotfix)

- `GpioWaitRequest` gained a `timeout_ms: u32` field, used by all
  five `gpio/wait-*` endpoints (`gpio/wait-high`, `wait-low`,
  `wait-rising`, `wait-falling`, `wait-any`). A value of `0`
  preserves pre-0.6 wait-forever behavior; non-zero bounds the
  firmware-side wait and returns `GpioError::Timeout` on expiry.
- `GpioError::Timeout` variant appended at end of enum (safe wire-
  protocol addition per AGENTS.md §6.1).
- Schema version bumps via the `pico-de-gallo-internal` version
  bump (`0.5.0` → `0.6.0`); under the pre-1.0 schema-versioning
  rule this is a breaking schema bump, so hosts and firmware must
  be upgraded together. This work ships in the same `0.6.0`
  release as the `system/reset-subscriptions` change below.
  Lockstep version bumps: `pico-de-gallo-internal` `0.5.0` →
  `0.6.0`, `pico-de-gallo-lib` `0.5.0` → `0.6.0`,
  `pico-de-gallo-hal` `0.5.0` → `0.6.0`, `pico-de-gallo-ffi`
  `0.6.0` → `0.7.0`, `gallo` (CLI) `0.6.0` → `0.7.0`,
  `pyco-de-gallo` `0.2.0` → `0.4.2`, `pico-de-gallo-firmware`
  `0.9.0` → `0.10.0`. Closes Category A finding #2 (reliability
  subagent B1: GPIO `wait_for_*` on a never-transitioning pin
  previously wedged the entire firmware dispatcher).

### Breaking Changes

- New `system/reset-subscriptions` endpoint appended to the wire
  protocol. Schema version bumps via the `pico-de-gallo-internal`
  version bump (`0.5.0` → `0.6.0`); under the pre-1.0
  schema-versioning rule this is a breaking schema bump, so hosts
  and firmware must be upgraded together. Lockstep version bumps:
  `pico-de-gallo-internal` `0.5.0` → `0.6.0`, `pico-de-gallo-lib`
  `0.5.0` → `0.6.0`, `pico-de-gallo-hal` `0.5.0` → `0.6.0`,
  `pico-de-gallo-ffi` `0.6.0` → `0.7.0`, `gallo` (CLI) `0.6.0` →
  `0.7.0`, `pyco-de-gallo` `0.2.0` → `0.4.2`,
  `pico-de-gallo-firmware` `0.9.0` → `0.10.0`. ([REVIEW-2026-05-29
  P1-3])

### Added

- `system/reset-subscriptions` endpoint (request `()`, response `u8`
  count). The endpoint is the recovery path for the leak described
  in P1-3: GPIO subscriptions are server-side state that survives
  the USB transport, so a host process that crashed (or was killed,
  or dropped its `nusb::Interface`) without sending
  `gpio/unsubscribe` would permanently strand the affected pins
  until a power cycle.

## [0.5.0] — 2026-05-04

### Breaking Changes

- `UartGetConfigurationResponse` and `AdcGetConfigurationResponse`
  are now `Result<…>` instead of bare struct values, so endpoints
  can report `Unsupported` on hardware revisions that don't route
  the peripheral. Wire protocol is **not** backward compatible —
  firmware and host must be upgraded together.
- New `Unsupported` variant added to `UartError`, `AdcError`, and
  `OneWireError`. Because these enums are not `#[non_exhaustive]`,
  existing exhaustive matches in downstream code must add the new
  arm.

### Added

- `GetDeviceInfo` endpoint (`"device/info"`), `DeviceInfo` struct,
  `Capabilities` bitflag newtype (`u64`) with named constants
  (`I2C`, `SPI`, `UART`, `GPIO`, `PWM`, `ADC`, `ONEWIRE`). Schema
  version constants auto-generated from `Cargo.toml` via `build.rs`.

## [0.4.0] — 2026-04-22

### Breaking Changes

- Reduced GPIO count from 8 (GPIO 8–15) to 4 (GPIO 8–11). GPIO
  12–15 are now reserved for PWM output. All GPIO indices are now
  0–3 instead of 0–7. (Joint firmware/internal change.)
- Replaced 12 unit-struct error types (`I2cReadFail`,
  `SpiWriteFail`, etc.) with 3 rich error enums: `I2cError` (7
  variants), `SpiError` (2 variants), `GpioError` (2 variants).
  Wire protocol is **not** backward compatible — firmware and host
  must be upgraded together.
- `GpioError` now has 4 variants — added `PinMonitored` and
  `PinNotMonitored` for the GPIO event subscription system.

### Added

- `GpioEventTopic` (device-to-host topic), `GpioEdge` enum
  (Rising/Falling/Any), `GpioEvent` struct (pin, edge,
  timestamp_us), `GpioSubscribe`/`GpioUnsubscribe` endpoints with
  request/response types. `TOPICS_OUT_LIST` now contains the GPIO
  event topic.
- `I2cBatch` and `SpiBatch` endpoints, `I2cBatchOp`/`SpiBatchOp`
  enums, `I2cBatchRequest`/`SpiBatchRequest`/`I2cBatchError`/
  `SpiBatchError` types, `encode_i2c_batch_ops`/
  `encode_spi_batch_ops` helpers, `i2c_batch_response_len`/
  `spi_batch_response_len`/`count_i2c_batch_ops`/
  `count_spi_batch_ops` parsing helpers. Constants:
  `MAX_BATCH_OPS`, `BATCH_OP_READ`, `BATCH_OP_WRITE`,
  `BATCH_OP_TRANSFER`, `BATCH_OP_DELAY_NS`.
- 6 PWM endpoints (`pwm/set-duty-cycle`, `pwm/get-duty-cycle`,
  `pwm/enable`, `pwm/disable`, `pwm/set-config`, `pwm/get-config`),
  `PwmError` enum (4 variants), request/response types,
  `PwmDutyCycleInfo` and `PwmConfigurationInfo` structs,
  `NUM_PWM_CHANNELS` constant.
- 2 ADC endpoints (`adc/read`, `adc/get-config`), `AdcChannel` enum
  (4 variants: Adc0–Adc3), `AdcError` enum (2 variants),
  `AdcReadRequest` and `AdcConfigurationInfo` types. Constants:
  `NUM_ADC_GPIO_CHANNELS`, `ADC_RESOLUTION_BITS`,
  `ADC_NOMINAL_REFERENCE_MV`.
- 6 1-Wire endpoints (`onewire/reset`, `onewire/read`,
  `onewire/write`, `onewire/write-pullup`, `onewire/search`,
  `onewire/search-next`), `OneWireError` enum (4 variants),
  `OneWireReadRequest`, `OneWireWriteRequest`,
  `OneWireWritePullupRequest` types. Response type aliases with
  `use-std` feature gating for `onewire/read`.
- 5 UART endpoints (`uart/read`, `uart/write`, `uart/flush`,
  `uart/set-config`, `uart/get-config`), `UartError` enum (7
  variants), `UartReadRequest`, `UartWriteRequest`,
  `UartSetConfigurationRequest`, and `UartConfigurationInfo` types.
  Response type aliases with `use-std` feature gating for owned vs
  borrowed data.
- `I2cScan` endpoint and `I2cScanRequest` type for firmware-side
  bus scanning. Returns a `Vec<u8>` of responding addresses — a
  single USB round-trip replaces 112 individual reads.
- `GpioDirection` and `GpioPull` enums,
  `GpioSetConfigurationRequest`, `GpioSetConfigurationResponse`,
  and `GpioSetConfiguration` endpoint for runtime GPIO pin
  direction and pull-resistor configuration.
- `GpioError::WrongDirection` variant — returned when a get/wait is
  attempted on a pin configured as output, or a put on a pin
  configured as input.
- `I2cGetConfiguration` and `SpiGetConfiguration` endpoints with
  `SpiConfigurationInfo` struct for querying the active bus
  configuration without relying on local state.

## [0.3.0] — 2025-04-20

### Breaking Changes

- Split `SetConfigurationRequest` into `I2cSetConfigurationRequest`
  and `SpiSetConfigurationRequest`.
- Replaced raw `u32` I2C frequency with `I2cFrequency` enum
  (`Standard`, `Fast`, `FastPlus`).

### Added

- SPI full-duplex transfer endpoint (`spi/transfer`) using DMA.
- `From<bool>` / `Into<bool>` conversions for `GpioState`.
- `MAX_TRANSFER_SIZE` constant (4096 bytes) shared across crates.
