# Changelog

All notable changes to `pico-de-gallo-firmware` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.0] — 2026-09-01

### Added

- The build script now derives `BUILD_ID` from
  `git describe --always --dirty --tags --match firmware-v*`, falling back to
  `"unknown"` and truncating to 64 bytes on a character boundary. The firmware
  logs it at boot and returns it as the final `DeviceInfo` field. The script
  reruns on every build so incremental builds cannot retain stale identity.
  Closes #159.

### Added

- Dispatch-progress supervision now feeds the RP2350 watchdog only while the
  postcard-rpc dispatch and aggregate transmit slots remain within their
  budgets. An expired slot logs its frame breadcrumb, records it in watchdog
  scratch registers, and forces a reset; the next boot reports the cause over
  RTT. A reset drops USB and all GPIO subscriptions. The supervisor does not
  cover a wedge inside `receive()`, and the TX slot has no hardware acceptance
  trigger. Because that slot measures aggregate rather than per-sender
  progress, one starved sender can remain masked while another completes at
  least once per 60-second TX budget. Board-attached acceptance is pending.

### Changed

- **BREAKING (behaviour):** the five `gpio/wait-*` handlers now cap every
  caller-supplied timeout at 30 minutes. `timeout_ms == 0` selects that ceiling
  instead of waiting forever, oversized values are clamped to it, and expiry
  returns `GpioError::Timeout`. `uart/read` remains asymmetric: `0` is still a
  1 ms non-blocking poll, while only its non-zero path is clamped.

  > The `timeout_ms == 0` clamp changes documented wire semantics without
  > changing wire shape. `pico-de-gallo-internal` remains at `0.7.0`, and
  > `internal-v0.7.0` is already published, so `validate()` cannot distinguish a
  > pre-clamp 0.7 firmware from a post-clamp one. Host and firmware must be
  > built from the same tree until the bump lands. Before any release,
  > AGENTS.md §16 step 2 requires bumping `internal` to `0.8.0` in lockstep
  > across all eight released crates, with dep-spec rewrites, per-crate
  > CHANGELOGs and both regenerated `Cargo.lock`s.

- **BREAKING (build):** `hw-rev2` is now the default Cargo feature;
  `hw-rev1` is no longer default. A build with no feature flags produces
  a **rev2** image. Building for a v1.0 landing board now requires
  `--no-default-features --features hw-rev1` explicitly. Previously the
  default produced a rev1 image, so anyone building from source without
  reading the feature table got a device whose UART, ADC, and 1-Wire
  endpoints returned `Unsupported` despite the README advertising seven
  peripherals. The `[rev1, rev2]` matrices in `nostd.yml` and
  `release-firmware.yml` were swapped to match, so `firmware-rev1.uf2`
  and `firmware-rev2.uf2` keep targeting the same board revisions they
  always did. Closes #156.

### Deprecated

- `hw-rev1` is deprecated as of this change. It remains fully supported,
  built and linted in CI, and published as `firmware-rev1.uf2` on every
  firmware release. **Removal will not happen before 2031-09-01** — a
  floor, not a commitment to remove on that date. Cargo features cannot
  carry `#[deprecated]`, so two signals are emitted instead: `build.rs`
  prints a `cargo:warning` naming the removal floor, and `main()` logs a
  `defmt::warn!` over RTT at every boot so a rev1 image is identifiable
  from the device and not only from the build log. The boot warning is
  compiled in, so a rev1 image built from this commit differs in content
  from `firmware-v0.11.0`'s.

## [0.11.0] — 2026-08-27

### Fixed

- `spi/batch` now applies three ordered refusals before touching the
  chip-select pin: invalid index, monitored pin, then `ExplicitInput`.
  Only `ExplicitInput` would corrupt tracked state, so `LegacyAuto` and
  `ExplicitOutput` remain accepted. An accepted pin is parked high; its
  prior level is not restored, and `pin_modes` is not written. Removed
  both pre-existing `.unwrap()` calls from the chip-select path. Closes
  #104.

- `i2c/batch` now issues one
  `embedded_hal_async::i2c::I2c::transaction()` instead of executing each
  operation as a separate transaction with its own START and STOP. Adjacent
  same-direction operations concatenate, direction changes use a repeated
  START, and only the final operation is followed by a STOP. A TMP102 hardware
  reproduction showed two adjacent writes previously returning success while
  modifying a register the caller never named. Bus failures now report
  `failed_op = 0` because the atomic transaction fails as a unit; validation
  failures retain their exact operation index. Direct dependencies on
  `embedded-hal-async` 1.0 and `heapless` 0.9 were added from versions already
  present in the resolved graph. Closes #128.

- `i2c/write` and `i2c/batch` now refuse an empty write payload with
  `I2cError::ZeroLengthWrite` instead of forwarding it to
  `embassy_rp::i2c::write_async`. The RP2040/RP2350 `DW_apb_i2c` block
  drives the address phase only by pushing bytes into `IC_DATA_CMD`, so
  an address-only `START + ADDR + STOP` is physically unreachable
  (rp-rs/rp-hal#678, embassy-rs/embassy#4474). embassy-rp 0.10.0 guards
  this in `write_blocking_internal` but not in `write_async_internal`:
  with an empty iterator it queues no command, starts no transaction, and
  then still awaits a `STOP_DET`/`TX_ABRT` interrupt that can never fire.
  Because postcard-rpc dispatches handlers serially, that await wedged
  every endpoint on the device until USB re-enumeration, and the
  independently fed watchdog did not fire. Reachable from the C FFI,
  `pico-de-gallo-lib`, the embedded-hal `I2c::write` impl, Python, MCP
  and Zephyr. In a batch the refusal happens during validation, so no
  earlier operation in that batch reaches the bus. Closes #101.

## [0.10.1] — 2026-08-04

### Added

- WebUSB platform capability in the BOS descriptor, plus a
  landing-page URL descriptor pointing at
  <https://balbi.sh/pico-de-gallo/>. Browsers now surface a
  notification when the device is plugged in and can discover it as a
  WebUSB-aware device. Closes #87.

  The USB device is now built through postcard-rpc's
  `WireStorage::init_without_build()` so that
  `embassy_usb::class::web_usb::WebUsb::configure()` can append the
  capability before `Builder::build()`. This adds a second,
  endpoint-less vendor-class interface; postcard-rpc's bulk interface
  remains interface 0, which all host transports depend on. No new
  dependencies and no wire-protocol change.

  Windows users on the manual Zadig fallback path must bind WinUSB to
  **interface 0** — the device now presents two interfaces with an
  identical `(0xFF, 0x00, 0x00)` triple, and interface 1 has no
  endpoints. See the Windows section of the book's USB & OS Notes.

## [0.10.0] — 2026-06-22

### Added (2026-06-03 — Category A hotfix)

- `gpio_wait_for_*` handlers now honor the per-request `timeout_ms`
  field added in `pico-de-gallo-internal` 0.6. A value of `0`
  preserves the pre-0.6 wait-forever behavior. Non-zero wraps the
  embassy `wait_for_*_edge()` future in
  `embassy_time::with_timeout(Duration::from_millis(timeout_ms))`
  and returns `GpioError::Timeout` on expiry.
- Embassy-rp watchdog enabled at 2-second timeout, fed every
  800 ms by a dedicated `watchdog_feeder_task`. `pause_on_debug(true)`
  is set so debugger sessions don't reset the chip. Recovers the
  device from any future handler hang regression (1-Wire PIO
  stalls, embassy-rp peripheral bugs, etc.). The feeder is a
  separate embassy task — RPC handlers cannot be trusted to feed
  because a wedged handler would also wedge any handler-based
  feed scheme (see closed dispatcher-wedge regression below).

### Fixed (2026-06-03 — Category A hotfix)

- `i2c_scan_handler` now wraps each per-address probe in
  `with_timeout(Duration::from_millis(50))`. A single stuck
  address (slave NAKs slowly, electrical issue, etc.) no longer
  burns the entire scan budget. The watchdog feeder task runs
  independently so the device stays alive even during long scans.

### Why

- Closes the dispatcher-wedge regression where a `gpio_wait_for_*`
  on a never-transitioning pin blocked **every other endpoint**
  until power-cycle (reliability finding B1, captured in
  `docs/superpowers/specs/2026-06-03-pico-de-gallo-category-a-review-synthesis.md`).
- Closes the no-recovery-from-handler-hang gap (reliability
  finding R5).
- Reduces worst-case impact of a flaky I²C bus on `i2c_scan`
  (reliability finding B2 / #33).

### Lockstep

- Coupled to `pico-de-gallo-internal` 0.6.0 (schema minor bump
  adding `timeout_ms` to `GpioWaitRequest` and `GpioError::Timeout`
  variant). See AGENTS.md §6.5.

### Added (system/reset-subscriptions)

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
