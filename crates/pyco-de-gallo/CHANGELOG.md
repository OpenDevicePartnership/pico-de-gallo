# Changelog

All notable changes to `pyco-de-gallo` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/pyco-v0.4.2...pyco-v0.11.0) (2026-07-23)


### Miscellaneous Chores

* **pyco:** Synchronize pico-de-gallo versions


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * pico-de-gallo-lib bumped from 0.6.0 to 0.11.0

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
