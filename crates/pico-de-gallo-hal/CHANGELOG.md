# Changelog

All notable changes to `pico-de-gallo-hal` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.0] — 2026-08-27

### Breaking Changes

- `Hal::spi_device(cs_pin)` is now fallible because it validates the
  index against the device-reported GPIO count. Metadata failure, zero
  GPIOs, and an invalid index remain distinct errors. Closes #104.

### Added

- Added public, runtime-authoritative `Hal::num_gpios()`. Closes #104.

- `embedded-io` 0.7 support. `Uart` now implements the blocking and
  async `Read`/`Write` traits from `embedded-io` 0.7 /
  `embedded-io-async` 0.7 under the additive `embedded-io-07`
  feature. The existing 0.6 impls remain enabled by default behind
  `embedded-io-06`; enable `embedded-io-07` alongside it to support
  both majors. Either feature can be selected alone, or disable both
  to drop the `embedded-io` dependency entirely.

### Changed

- `I2c::write(addr, &[])` and any transaction containing an empty `Write`
  operation now fail without a USB round-trip, inheriting the guard added to
  `pico-de-gallo-lib`. No HAL code changed. `embedded-hal` permits an empty
  write and some drivers use it as a probe, but this hardware genuinely
  cannot perform one, so the honest answer is an error: `I2cHalError::I2c(
  ZeroLengthWrite)`, whose `ErrorKind` is `Other`. It is deliberately not
  `NoAcknowledge`, which would wrongly imply the bus was driven and the
  target stayed silent. Tests pin both the conversion and the kind. Closes
  #136.

- Documented the corrected I²C transaction semantics inherited from firmware.
  The HAL implements only `I2c::transaction`; the embedded-hal defaults for
  `read`, `write`, and `write_read` all route through it and therefore through
  `i2c/batch`. Adjacent same-direction operations now concatenate, direction
  changes use a repeated START, and only the final operation receives a STOP.
  The Rust API is unchanged. Closes #128.

## [0.6.0] — 2026-06-22

### Added (2026-06-04 — Category A hotfix host-only PR)

- `Hal::new_validated()` and
  `Hal::new_validated_with_serial_number(serial)` constructors call
  `validate()` before returning, failing loudly on
  device-not-connected or schema-version mismatch. The existing
  lazy `Hal::new()` / `Hal::new_with_serial_number()` continue to
  defer failures until the first RPC.
- `Hal::validate()` accessor for callers that constructed via the
  lazy constructors and want to validate after the fact.
- `Hal::system_reset_subscriptions() -> Result<u8, SystemHalError>`
  exposes the firmware-side subscription teardown previously only
  reachable via `pico-de-gallo-lib`. Recommended after
  `new_validated()` in any application that uses GPIO subscriptions,
  so a prior host's crashed-mid-subscription state is cleared.
- `HalInitError` (wraps `pico_de_gallo_lib::ValidateError`) and
  `SystemHalError` (just `Comms` today).
- Re-exported `AdcChannel`, `AdcConfigurationInfo`, `GpioDirection`,
  `GpioEdge`, `GpioPull` from `pico-de-gallo-lib`. Driver authors
  no longer need to add `pico-de-gallo-lib` to their `Cargo.toml`
  for these types.

### Fixed (2026-06-04 — Category A hotfix host-only PR)

- Removed the stale doc-comment reference to a non-existent
  `Hal::uart_set_config` method on the `Uart` struct. Documentation
  now correctly notes that UART baud is fixed at the firmware
  default and changes require dropping to `pico-de-gallo-lib`.

### Changed (2026-06-04 — Category A hotfix host-only PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0 (validate() now
  also checks `schema_major`).
- Updated `docs/ai-agents/pico-de-gallo-hal-examples.md`:
  - §4 Cargo setup: removed the "not re-exported by the HAL"
    bullet for `AdcChannel` et al. (they are re-exported now).
  - §6.4–§6.7 GPIO snippets: switched `use pico_de_gallo_lib::{...}`
    to `use pico_de_gallo_hal::{...}` for `GpioDirection`,
    `GpioPull`, `GpioEdge`.
  - §6.7 GPIO subscribe gotcha: documents
    `hal.system_reset_subscriptions()` as the recovery path.
  - §6.9 ADC snippet/HIL/gotchas: switched `AdcChannel` import
    to the HAL re-export; historical note preserved.

### Added (2026-06-03 — Category A hotfix wire PR)

- `Gpio::wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout`
  async methods accept a `std::time::Duration` and return
  `Err(GpioHalError::Gpio(GpioError::Timeout))` on expiry.
  `embedded-hal-async`'s `Wait` trait does not support timeouts, so
  these are exposed as inherent methods on `Gpio` instead. Recommended
  for production code; the trait methods retain their wait-forever
  semantics for compatibility with existing drivers.

### Changed (2026-06-03 — Category A hotfix wire PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0.
- Updated `docs/ai-agents/pico-de-gallo-hal-examples.md` §6.6 Gotchas
  to recommend the bounded `_with_timeout` variants for production
  use.

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
