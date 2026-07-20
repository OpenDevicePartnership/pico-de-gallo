# Changelog

All notable changes to `pyco-de-gallo` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/pyco-v0.2.0...pyco-v0.10.0) (2026-07-20)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))
* **lib,hal,ffi,application,pyco:** enforce schema validation, expose HAL recovery ([c8e2f13](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/c8e2f13be1bacf83e905d9e1453f6ec4b3abc69c))
* **lib:** add gpio_wait_for_*_with_timeout, bump internal to 0.7 ([9840232](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/98402325a49a21f773d30fba7007c2da8addd698))
* **pyco:** add gpio_wait_for_*_with_timeout Python methods ([9b324da](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/9b324da75131b1a0bcacb2faebc8eee523dcb6ad))
* **pyco:** add PycoDeGallo.open_strict for validation-on-construct ([5b671f3](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/5b671f37a226f64273742a19b3f001e1f9af1fc5))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * pico-de-gallo-lib bumped from 0.6.0 to 0.10.0

## [0.4.2] — 2026-06-22

### Added (2026-06-04 — Category A hotfix host-only PR)

- `pyco_de_gallo.open_strict()` and
  `pyco_de_gallo.open_strict_with_serial_number(serial_number)`.
  Both call `PicoDeGallo::validate()` internally before returning
  the `PycoDeGallo` handle. Raise `RuntimeError` on device-not-found,
  schema version mismatch, legacy firmware, or any validation error.
  Prefer in production Python code over the lazy `open()` /
  `open_with_serial_number()` — failures surface at construct time
  rather than on the first RPC. Closes Category A finding #4 at
  the Python layer.

### Changed (2026-06-04 — Category A hotfix host-only PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0 (validate() now
  also checks `schema_major`).
- Updated `book/src/crates/python.md` to document the two new
  entry points.

### Added (2026-06-03 — Category A hotfix wire PR)

- `PycoDeGallo.gpio_wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout`
  Python methods accept a `timeout_ms: int`. 0 waits forever
  (matches the existing methods); non-zero bounds the wait and
  raises `RuntimeError` on `GpioError::Timeout`. Available on
  firmware schema 0.6+.

### Changed (2026-06-03 — Category A hotfix wire PR)

- Bumped `pico-de-gallo-lib` dependency to 0.6.0. Lockstep release
  with `pico-de-gallo-internal` 0.6.0 / `pico-de-gallo-firmware`
  0.10.0 per AGENTS.md §6.5.

### Added

- `PycoDeGallo.system_reset_subscriptions()` returns an `int`.

## [0.2.0] — 2026-05-04

### Added

- `pyco-de-gallo` is now part of the `check.yml` CI matrix (fmt,
  clippy, doc, hack, test, msrv) on equal footing with the other
  host crates.
