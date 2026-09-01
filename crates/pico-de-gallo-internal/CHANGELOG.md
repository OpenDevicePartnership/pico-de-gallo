# Changelog

All notable changes to `pico-de-gallo-internal` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] — 2026-09-01

### Breaking Changes

- Appended `DeviceInfo::build_id`, a `heapless::String<64>` carrying the
  firmware's `git describe --always --dirty --tags --match firmware-v*`
  output, or `"unknown"` when git was unavailable. The field is
  informational and is never a compatibility gate: `validate()` ignores
  it. Closes #159.

  This is append-only on the wire, but the encoding is not the
  compatibility axis that matters. postcard-rpc derives each endpoint's
  key from the response type's schema, so changing `DeviceInfo` also
  changed the `device/info` endpoint key. A peer built against the other
  shape replies under the other key, the dispatcher drops the unmatched
  frame, and the call never returns. It is not a decode error — postcard
  is never reached — so `validate()` reports it as a `Timeout` after
  `DEVICE_INFO_TIMEOUT`, misattributed to an unresponsive board.

  **Schema 0.7 and 0.8 components must not be mixed, and a mixed pair
  cannot diagnose itself.** For any other wire type a version skew is
  self-describing: `device/info` still answers and `validate()` returns
  `SchemaMismatch`. Not here — the endpoint that re-keyed *is* the
  compatibility probe, so the schema numbers naming the incompatibility
  are sealed inside the dropped message. Bumping the schema version does
  not make this detectable; the version is payload, not key. `DeviceInfo`
  is a blind spot for its own versioning mechanism. `gallo version` still
  works across such a pair, because `VersionInfo`'s schema and key are
  deliberately held stable.

### Added

- `BUILD_ID_CAPACITY` (64) and the `DeviceInfo::build_id()` accessor, so
  host crates never need to name `heapless` themselves.
- `device_info_response_key_is_pinned` and
  `version_response_key_is_pinned`, which hard-code the postcard-rpc
  `RESP_KEY` bytes for `device/info` and `version`. Any future change to
  either response type's shape now fails the test suite instead of
  silently re-keying the endpoint and producing a hang in the field.

## [0.7.0] — 2026-08-27

### Breaking Changes

- Appended `SpiError::InvalidCsPin`, `SpiError::CsPinUnavailable`, and
  `SpiError::CsPinMonitored`, in that order after `Other`, and added
  `DeviceInfo::num_gpios: u8` as the final field. Both are append-only
  wire changes, but AGENTS.md §6.2 still requires a lockstep schema
  bump. This release is that bump: `SCHEMA_VERSION_MINOR` is derived
  from this crate's version by `build.rs` and now reads 7. Closes #104.
- Appended `I2cError::ZeroLengthWrite` at index 7, after `Other`. The
  firmware now returns it instead of forwarding an empty payload to
  `embassy_rp::i2c::write_async`, which never returns on RP2040/RP2350
  and wedges the dispatcher device-wide. Ships inside this same schema
  0.7 bump. Closes #101.

### Changed

- Clarified the `i2c/batch` contract without changing its wire shape:
  operations form one I²C transaction, adjacent operations in the same
  direction continue without a STOP, direction changes use a repeated START,
  and only the final operation is followed by a STOP. Validation failures keep
  their exact `failed_op`; a bus failure applies to the transaction as a whole
  and reports `failed_op = 0`. Closes #128.

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
