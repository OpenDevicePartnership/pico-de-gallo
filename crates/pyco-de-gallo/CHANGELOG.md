# Changelog

All notable changes to `pyco-de-gallo` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] — 2026-09-01

### Added

- `DeviceInfo.build_id` exposes the firmware's diagnostic build identity as a
  Python `str`. It is informational and never controls whether a call
  succeeds. Closes #159.

## [0.5.0] — 2026-08-27

### Added

- Added `DeviceInfo.num_gpios` and `PycoDeGallo.num_gpios()`.
  Closes #104.

### Changed

- SPI batch calls now check the runtime GPIO bound and raise distinct
  `RuntimeError` messages for metadata failure, zero GPIOs, and an
  invalid index. Closes #104.

- `PycoDeGallo.i2c_write(addr, b"")`, and any `i2c_batch` containing an empty
  write payload, now raise `RuntimeError` without a USB round-trip,
  inheriting the guard added to `pico-de-gallo-lib`. No binding code changed
  and the Python API is unchanged; the message is the same one firmware
  returned before. `i2c_write_read` still accepts an empty write phase.
  Closes #136.

- Documented that `PycoDeGallo.i2c_batch` executes one I²C transaction:
  adjacent same-direction operations concatenate, direction changes use a
  repeated START, and only the final operation receives a STOP. A bus failure
  reports `failed_op = 0` for the transaction as a whole; validation failures
  retain an exact index. The Python API is unchanged. Closes #128.

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
