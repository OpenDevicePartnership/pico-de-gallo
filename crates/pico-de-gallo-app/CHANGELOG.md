# Changelog

All notable changes to `gallo` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.0] — 2026-09-01

### Changed

- `gallo version` now renders summary and capability tables, including the
  runtime GPIO count and the firmware build identity. Closes #159.

### Fixed

- `gallo version` no longer hangs indefinitely against a firmware built
  from a different tree. `PicoDeGallo::device_info` carries no timeout of
  its own, so the existing fallback to the legacy `version` endpoint was
  unreachable: no `Err` was ever produced. The call is now bounded at five
  seconds — `device/info` has no user-controllable work, unlike the
  `spi_batch` worst case that sizes the library's 300-second bound — and an
  elapsed timeout takes the fallback path. `VersionInfo`'s schema is
  unchanged, so the legacy endpoint still matches across the mismatch.
  Refs #159.

- The firmware-validation failure message no longer advises retrying or
  replugging on a `device/info` timeout. postcard-rpc keys each endpoint by
  its response type's schema, so a host and firmware built from different
  trees exchange `device/info` under different keys and the reply is dropped
  as unmatched rather than decoded — a compile-time mismatch that no retry
  can fix. The message now names both that cause and a genuinely
  unresponsive board, and states that the host cannot distinguish them.
  Refs #159.

## [0.9.0] — 2026-08-27

### Breaking Changes

- Replaced `gallo gpio put --high` with the required
  `--level <high|low>`. There is no `-h` alias: the old derived short
  option collided with clap's `--help`, causing startup to panic and
  making it impossible to request a low level. Closes #104.

### Changed

- `spi batch --cs` is validated against device-reported metadata before
  operations are parsed or anything is transmitted. Closes #104.

- Documented that `gallo i2c batch` sends one I²C transaction rather than one
  independent transaction per operation. Adjacent same-direction operations
  concatenate, direction changes use a repeated START, and only the final
  operation receives a STOP. The command-line surface is unchanged. Closes
  #128.

## [0.8.0] — 2026-08-19

### Breaking Changes

- `gallo spi set-config` now takes `--mode <0|1|2|3>` in place of the
  `--first-transition` and `--idle-low` boolean flags. See the entry
  under **Fixed** for why the old flags could not simply be given
  correct defaults. Values outside 0–3 are rejected by the argument
  parser rather than masked. The old flags were never documented in the
  book — it described a `--phase` / `--polarity` pair that never
  existed — so no working documented invocation changes meaning.

### Added

- `gallo ping` is now implemented. It round-trips a random `u32`
  through the firmware's `ping` endpoint and prints `Ping OK` when the
  echo matches. The subcommand was already documented in the book
  (`getting-started/verify.md`, `appendix/troubleshooting.md`) but had
  never been implemented, so `gallo 0.7.1` answered with
  `error: unrecognized subcommand 'ping'`. The payload is randomised
  per invocation so a stale, duplicated, or default-initialised
  response cannot pass as a healthy round trip; a completed round trip
  carrying the wrong value is reported as
  `ping echo mismatch: sent 0x…, received 0x…` rather than as a generic
  transport error, because the two failures have different causes.
  Fixes [#113](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/113).
- `-s, --serial-number` now has help text. The flag previously rendered
  with an empty description column in `gallo -h`.

### Changed

- `ping` and `version` are now the only device subcommands exempt from
  the up-front schema-version check. `ping` is the transport-level
  liveness probe, so validating first would report a schema error on a
  board whose USB path is precisely what the operator is trying to test.

### Fixed

- `gallo spi set-config` no longer changes the SPI mode as a side
  effect of setting the clock. `--first-transition` and `--idle-low`
  were opt-in flags defaulting to `false`, which the handler mapped to
  `CaptureOnSecondTransition` and `IdleHigh` — so a bare
  `gallo spi set-config --frequency 1000000` silently selected **mode
  3**, while the firmware boots in **mode 0**. Selecting mode 0 required
  passing both flags, the exact inverse of what their names suggest.
  Giving the flags a `true` default does not fix this: `clap` derives
  `ArgAction::SetTrue` for a bare `bool`, so `false` becomes
  unreachable (`--idle-low=false` errors with *"unexpected value for an
  argument found"*) and modes 1–3 would be impossible to select. The
  flags are therefore replaced by `--mode`, which defaults to 0 and
  matches the firmware's power-on configuration. The previous
  regression test asserted the parsed flag values rather than the
  resulting bus configuration, which is why it never caught this; the
  replacement asserts the `(SpiPhase, SpiPolarity)` pair.
- `gallo --help` no longer prints the crate's internal API
  documentation. `clap` derives `long_about` from the `Cli` struct's
  rustdoc when one is present, so long help rendered *"Top-level CLI
  argument parser. Parse with [`clap::Parser::parse`] and execute with
  [`Cli::run`]."* in place of the configured `about` string. `-h` was
  unaffected. Now pinned with `long_about = None`.

## [0.7.1] — 2026-07-20

### Fixed

- `gallo` now opens a **single** USB connection per invocation and
  shares it (by reference) across schema validation and the command
  handler. Previously every subcommand except `list`/`version`
  opened one connection to run `validate()`, dropped it, then opened
  a second connection for the operation (`spi write-read` opened a
  third). On Windows, WinUSB grants exclusive access to one session
  per interface, and the first connection's background `nusb` worker
  had not released the handle before the second `claim_interface`,
  so the operation panicked with
  `Failed claiming interface: … Access is denied`. Commands such as
  `gallo i2c scan`, `i2c get-config`, and `adc info` failed
  deterministically on Windows, while `version`/`list` (single/zero
  connections) worked — making it look like a driver or permissions
  problem. Linux and macOS release the interface synchronously on
  drop, so CI never caught it. Regression from the 2026-06-04
  up-front `validate()` change (Category A finding #4). No CLI
  surface changed.

## [0.7.0] — 2026-06-22

### Fixed (2026-06-04 — Category A hotfix host-only PR)

- `gallo` now calls `validate()` at the top of every subcommand
  except `list` and `version`. Previously the CLI connected
  lazily and surfaced schema-version mismatches as confusing
  `CommsFailed` errors on the first RPC; now the mismatch is
  reported up-front with an actionable error message that
  points at `gallo version` for the device-reported schema and
  recommends either re-flashing the firmware or installing a
  matching `gallo` build. Closes Category A finding #4 (reviewer
  R4) at the CLI layer.

  `list` is exempt because it doesn't touch a connected device.
  `version` is exempt because it IS the diagnostic subcommand
  that reports schema skew (it already handles legacy firmware
  via `device_info()` with fallback).

### Changed (2026-06-04 — Category A hotfix host-only PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0 (validate() now
  also checks `schema_major`, so any future major-version skew
  surfaces immediately rather than silently mis-decoding wire
  bytes).

### Changed (2026-06-03 — Category A hotfix wire PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0. Required for
  lockstep release with the wire-protocol schema bump in
  `pico-de-gallo-internal` 0.6.0 / `pico-de-gallo-firmware` 0.10.0
  (`timeout_ms` field on `GpioWaitRequest`, `GpioError::Timeout`
  variant).
- Existing `gallo` CLI behavior is unchanged in this release: the
  pre-existing `gpio` subcommands (`get`, `put`, `set-config`,
  `monitor`) all keep working. The CLI does not currently expose
  `gpio wait-for-*` subcommands, so no new flags are added here.
  Bounded waits remain accessible to Rust / C / Python consumers
  via `pico-de-gallo-lib`, `pico-de-gallo-hal`, and `pico-de-gallo-ffi`.

## [0.6.0] — 2026-05-04

### Added

- `gallo version` now shows schema version, HW revision, and
  capabilities with graceful fallback for legacy firmware.

## [0.5.0] — 2026-04-22

### Added

- `gallo gpio monitor --pin N --edge rising|falling|any` command.
  Subscribes, prints edge events with timestamps, unsubscribes on
  Ctrl+C.
- `gallo i2c batch` and `gallo spi batch` CLI commands for
  executing batched operations (e.g.,
  `--op write:0x00,0x10 --op read:16`).
- `gallo pwm` subcommand group with `set-duty`, `get-duty`,
  `enable`, `disable`, `set-config`, and `get-config` commands.
- `gallo adc` subcommand group with `read` and `info` commands.
- `gallo onewire` subcommand group with `reset`, `read`, `write`,
  `write-pullup`, and `search` commands.
- `gallo uart` subcommand group with `read`, `write`, `flush`,
  `set-config`, and `get-config` commands.
- `gallo i2c scan` now uses the dedicated scan endpoint (single
  round-trip) instead of 112 individual reads.
- `gallo gpio set-config`, `gallo gpio get`, and `gallo gpio put`
  subcommands for direct GPIO access from the command line.
- `gallo i2c get-config` and `gallo spi get-config` subcommands.

## [0.4.0] — 2025-04-20

### Breaking Changes

- CLI `set-config` command replaced by `i2c set-config` and
  `spi set-config` subcommands.

### Added

- `list` command to show connected devices with serial numbers.

### Changed

- `I2cFrequency` exposed as `--frequency standard|fast|fast-plus`
  CLI arg.

## [0.2.1] — 2025-03-15

### Fixed

- Bumped library dependency for latest fixes.
