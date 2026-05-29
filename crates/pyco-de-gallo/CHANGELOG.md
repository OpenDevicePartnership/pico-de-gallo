# Changelog

All notable changes to `pyco-de-gallo` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0](https://github.com/OpenDevicePartnership/pico-de-gallo/compare/pyco-v0.2.0...pyco-v1.0.0) (2026-05-29)


### ⚠ BREAKING CHANGES

* **internal,firmware,lib,hal,ffi,application,pyco:** pico-de-gallo-internal gains the `system/reset-subscriptions` endpoint; postcard-rpc requires firmware and every host crate to be rebuilt against the matching SCHEMA_VERSION_MINOR. Mixing a 0.5.x firmware with a 0.6.x host (or vice versa) will fail `validate()` with a schema-version mismatch. Additionally, the FFI handle-borrowing entry points now take `*const PicoDeGallo`; this is source-compatible for C consumers but technically a signature change.

### Features

* **internal,firmware,lib,hal,ffi,application,pyco:** address P1 review findings ([00ea9df](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/00ea9dfde78dd8ec531cfdd986b7205671d2ae25))


### Bug Fixes

* address P1 findings from REVIEW-2026-05-29 (validate mapping, FFI surface, GPIO subscription leak, const handles) ([ce5cc15](https://github.com/OpenDevicePartnership/pico-de-gallo/commit/ce5cc15267bb3ab982e007e6bb56742db238cdd1))

## [Unreleased]

### Added

- `PycoDeGallo.system_reset_subscriptions()` returns an `int`.

## [0.2.0] — 2026-05-04

### Added

- `pyco-de-gallo` is now part of the `check.yml` CI matrix (fmt,
  clippy, doc, hack, test, msrv) on equal footing with the other
  host crates.
