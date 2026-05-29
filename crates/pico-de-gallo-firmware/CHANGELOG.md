# Changelog

All notable changes to `pico-de-gallo-firmware` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/firmware-v0.9.0...firmware-v1.0.0) (2026-05-29)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

## [Unreleased]

### Added

- `system/reset-subscriptions` endpoint handler. Firmware iterates
  its GPIO monitor slots, signals stop on each live one, awaits the
  `Flex` pin back from the monitor task, and returns it to
  `Context`. Idempotent and cheap when no subscriptions are active.
  The endpoint is the recovery path for the leak described in P1-3:
  a host process that crashed without sending `gpio/unsubscribe`
  would previously strand the affected pins until a power cycle.

## [0.9.0] — 2026-05-04

### Added

- `hw-rev1` and `hw-rev2` Cargo feature flags (mutually exclusive).
  `hw-rev1` is the default and matches the current v1 landing
  board. Unsupported peripherals (UART, ADC, 1-Wire on v1) return
  `Unsupported` errors instead of silently failing.
- `device_info_handler` returning firmware version, schema version,
  hardware revision, and capabilities bitfield. Capabilities are
  gated by hardware revision feature flag.
- Hardware v1.1 landing board — single keyed 2×12 (0.1″ pitch)
  shrouded header replacing the seven individual connectors of
  v1.0. Routes all 20 firmware signals (UART, SPI CS, 1-Wire, ADC
  now connected). On-board passives: 4.7 kΩ I²C pull-ups (R1/R2),
  100 Ω ADC series resistors (R3–R5), 100 nF decoupling capacitor
  (C1). VREF pin hardwired to 3.3 V. Uses `hw-rev2` firmware.

### Changed

- `release-firmware.yml` now generates the `.uf2` with
  [`elf2uf2-rs`](https://github.com/JoNil/elf2uf2-rs) instead of
  downloading `picotool` from the `pico-sdk-tools` release tarball.
  The tool is installed from git (`cargo install --git ...
  --locked`) because the published crates.io 2.2.0 release does not
  yet expose the `--family` CLI option. The conversion uses
  `--family rp2350-arm-ns` (non-secure Cortex-M33; TrustZone is not
  enabled). Output artifact names (`firmware-{rev1,rev2}.uf2` and
  `pico-de-gallo-firmware-{rev1,rev2}` ELF) are unchanged.
- Renamed firmware crate package from `pico-de-gallo-fw` to
  `pico-de-gallo-firmware` (matches the directory name). The
  release ELF asset uploaded by `release-firmware.yml` is now
  `pico-de-gallo-firmware-{rev1,rev2}` (was
  `pico-de-gallo-fw-{rev1,rev2}`). The `firmware-{rev1,rev2}.uf2`
  artifact name is unchanged.
- CI: `nostd.yml` now builds and lints firmware for both `hw-rev1`
  and `hw-rev2`. `release-firmware.yml` produces per-revision
  release assets (`firmware-rev1.uf2`, `firmware-rev2.uf2`).

### Fixed

- Pin `embassy-usb-driver = "=0.2.0"` to work around an upstream
  incompatibility — `embassy-usb-driver 0.2.1` bumped
  `embedded-io-async` from 0.6 to 0.7, but `embassy-usb 0.5.1`'s
  CDC-ACM `embedded_io_async::ErrorType` impl still expects the 0.6
  trait. The mismatch produces an `EndpointError:
  embedded_io_async::Error` trait-bound error inside `embassy-usb`.
  We can't move to embassy-usb 0.6 because `postcard-rpc 0.12` only
  ships an `embassy-usb-0_5-server` feature.

## [0.8.0] — 2026-04-22

### Breaking Changes

- Reduced GPIO count from 8 (GPIO 8–15) to 4 (GPIO 8–11). GPIO
  12–15 are now reserved for PWM output. All GPIO indices are now
  0–3 instead of 0–7. (Joint firmware/internal change.)
- `gpios` field in `Context` changed from `[Flex<'static>;
  NUM_GPIOS]` to `[Option<Flex<'static>>; NUM_GPIOS]`. GPIO
  operations on a monitored pin return `GpioError::PinMonitored`.
- I2C handlers now map embassy-rp `AbortReason` variants to rich
  error types. SPI `set-config` validates frequency before applying
  (prevents panic on zero frequency).

### Added

- GPIO event monitoring via 4 pooled `gpio_monitor_task` instances.
  Subscribe takes ownership of the pin, monitors for edges, and
  publishes `GpioEvent` via `Sender::publish`. Unsubscribe returns
  the pin to the context. Static channels for
  start/stop/return/armed synchronization.
- `i2c_batch_handler` and `spi_batch_handler` with pre-validation,
  CS assertion/deassertion for SPI batches. SPI batch executes
  atomically under chip-select.
- PWM output on GPIO 12–15 (PWM slices 6–7, 4 channels).
  Frequency/phase-correct configuration with automatic
  top/divider computation. Duty-cycle compare values scaled
  proportionally when frequency changes.
- ADC support on GPIO 26–29 (4 GPIO channels). Uses
  `Adc::new_blocking` for single-shot reads.
- 1-Wire support via PIO0/SM0 on GPIO 16 using embassy-rp's
  `PioOneWire` driver. 6 async handlers. ROM search state held in
  Context.
- UART0 support via `BufferedUart` (interrupt-driven, 1024-byte
  TX/RX buffers). 5 UART handlers with timeout support on reads.
  Baud rate validation (must be > 0). Uses GPIO0 (TX) and GPIO1
  (RX).
- `i2c_scan_handler` — probes addresses by 1-byte read, collects
  responding addresses. Supports `include_reserved` flag.
- `gpio_set_config_handler` and per-pin `PinMode` tracking. Once a
  pin is configured via `gpio/set-config`, it enters explicit mode
  and get/put/wait respect the configured direction (returns
  `WrongDirection` on mismatch). Legacy auto-switching preserved
  for unconfigured pins.
- `i2c_get_config_handler` and `spi_get_config_handler` — return
  the currently active configuration. Firmware now tracks config
  values set by `set-config` endpoints.

## [0.7.0] — 2025-04-20

### Breaking Changes

- Wire protocol updated — firmware and host must be upgraded
  together.

### Changed

- Handler functions modernized with improved ergonomics.
- Buffer increased to `MAX_TRANSFER_SIZE` (4096 bytes).
- `PacketBuffers` sized to `MAX_TRANSFER_SIZE + 1024` per
  direction.

## [0.6.0] — 2025-03-15

### Added

- Updated all Embassy and postcard-rpc dependencies.
- Addressed critical safety issues and improved API ergonomics.
- Added more tests and extracted `connect()` helper.
