# Changelog

All notable changes to `gallo` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/application-v0.6.0...application-v1.0.0) (2026-05-29)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

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
