# Changelog

All notable changes to `pico-de-gallo-lib` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- `PicoDeGallo::uart_set_config` now takes `data_bits`, `parity`, and
  `stop_bits` after `baud_rate`; `uart_get_config` reports all four values.
  The crate re-exports `UartDataBits`, `UartParity`, and `UartStopBits` for
  selecting the RP2350-supported framing. Closes #152.

### Fixed

- `i2c_batch` and `spi_batch` now bound their aggregate *outgoing* bytes
  against `MAX_REQUEST_FRAME`, locally and before transmission, reporting
  `BufferTooLong` with `failed_op = 0`. Closes #186.

  A batch's outgoing bytes were bounded by nothing. `check_i2c_batch_ops`
  walked the operations for zero-length writes and then checked only the
  *read* aggregate; `check_spi_batch_ops` checked only the read/transfer
  aggregate. A batch could therefore build a request frame larger than the
  firmware's receive buffer, which postcard-rpc's `receive()` discards
  before any handler runs — so nothing was sent back at all and the caller
  saw `Timeout`, an error naming neither the argument at fault nor a size
  that would work. On `spi_batch` it was worse: a batch carrying no
  `DelayNs` is bounded by the firmware's 30-minute handler ceiling, so the
  caller waited over half an hour.

  This is the one ceiling with no firmware counterpart, and cannot have one:
  the device never receives the frame. It is also the last route from a
  supported host surface into the request-frame ceiling, since #158 capped
  every single-argument write at `MAX_TRANSFER_SIZE`.

  The bound assumes the widest request header, so the answer does not depend
  on whether the client has received a reply yet — postcard-rpc narrows its
  key from 8 bytes to 2 only after a first reply, and the field is private.
  A warm connection is therefore refused up to six bytes earlier than the
  wire strictly requires, which is the deliberate price of a bound that does
  not vary with what the process did beforehand.

  Individual batch operations remain *unbounded* by `MAX_TRANSFER_SIZE`, and
  deliberately so: a batch `Write` streams straight out of the received
  frame and never enters the firmware's scratch buffer, so unlike
  `i2c_write` there is no buffer contract to honour. The frame is its real
  limit.

### Added

- Re-exports `MAX_REQUEST_FRAME`, `i2c_batch_request_frame_len` and
  `spi_batch_request_frame_len` so callers can size a batch without
  modelling postcard overhead by hand. Part of #186.

### Fixed

- Every payload-carrying entry point now enforces a size ceiling locally,
  before anything is transmitted, instead of documenting one and enforcing
  nothing. Part of #158.

  Two ceilings apply, and which one binds depends on the direction of the
  bytes rather than on the endpoint:

  | Direction | Ceiling | Methods |
  |---|---|---|
  | device to host | `MAX_RESPONSE_PAYLOAD` (1014) | `i2c_read`, `spi_read`, `uart_read`, `onewire_read`, the read half of `i2c_write_read`, and `spi_transfer` |
  | host to device | `MAX_TRANSFER_SIZE` (4096) | `i2c_write`, `spi_write`, `uart_write`, `onewire_write`, `onewire_write_pullup`, and the write half of `i2c_write_read` |

  Overflow is reported as `BufferTooLong` on the relevant peripheral error
  type. No new error variant and no wire change: `BufferTooLong` already
  existed on all four.

  `spi_transfer` is the case that shows the two ceilings are genuinely
  distinct rather than cosmetic. It is full duplex, so its single argument
  is both the request payload and the response length, and the tighter bound
  wins: it now accepts 1014 bytes where `spi_write` accepts 4096. #158
  measured `spi/write` completing normally at 1015 bytes, the size at which
  `spi/transfer` fails, so collapsing the two into one ceiling would be
  wrong in one direction or the other whichever value were chosen.

  Previously a read of 1015..=4096 bytes was accepted, the bus was driven,
  and the reply was then truncated in transport — surfacing as
  `Comms(Postcard(DeserializeUnexpectedEnd))`, which names the transport
  rather than the argument at fault and suggests no size that would work.
  Unlike the batch endpoints fixed in #179, nothing is corrupted by this:
  a plain read commits no writes, so it is a diagnosability fix rather than
  a data-integrity one.

### Changed

- The doc comments on `i2c_read`, `i2c_write_read`, `spi_read`,
  `spi_transfer` and `uart_read` no longer state 4096 as the caller-facing
  limit. They were wrong twice over: nothing enforced the number, and for a
  read path the real ceiling is 1014, not 4096.

- `batch_read_total_is_undeliverable` (private) is now
  `response_len_is_undeliverable`, since the same predicate governs plain
  reads. A sibling `request_payload_is_too_long` covers the other
  direction, and a `const` assertion pins the response ceiling as the
  strictly tighter of the two — the invariant `spi_transfer` relies on when
  it checks only one.

### Added

- Re-export of `MAX_RESPONSE_PAYLOAD` from `pico-de-gallo-internal`.

- `DEFAULT_CALL_TIMEOUT` (5 s), `PicoDeGallo::with_call_timeout` and
  `PicoDeGallo::call_timeout`. Every RPC is now bounded; previously only the
  validated `device/info` fetch was. Closes #178.

### Changed

- **Breaking:** `PicoDeGalloError` gained a `Timeout { waited }` variant and
  `SpiBatchCallError` gained `Timeout { waited }`. Neither enum is
  `#[non_exhaustive]`, deliberately, so every downstream match had to make a
  decision about the new case rather than absorbing it into a wildcard.

- Calls that can legitimately outlast the default derive their own bound
  instead: `i2c_scan` from `UNDECLARED_DISPATCH_BUDGET_MS`, `uart_read`
  and the `gpio_wait_*_with_timeout` family from the caller's `timeout_ms`,
  `onewire_write_pullup` from `pullup_duration_ms`, the plain
  `gpio_wait_*` family from `MAX_HANDLER_TIMEOUT_MS`, and `spi_batch`
  from the sum of its `DelayNs` operations. In each case the configured call
  timeout is added as transport slack, so raising it widens the derived bounds
  too. `device_info`/`validate` keep `DEVICE_INFO_TIMEOUT` unchanged.

### Fixed

- `i2c_batch` and `spi_batch` now refuse, locally and before transmitting
  anything, a batch whose operations would return more than
  `MAX_RESPONSE_PAYLOAD` (1014) bytes in total. Closes #179.

  Such a batch used to be accepted by both the host and the firmware,
  *executed on the bus*, and only then lose its response to transport
  truncation — surfacing as `Comms(Postcard(DeserializeUnexpectedEnd))`,
  which reads like a transport fault and invites a retry that repeats every
  write. The refusal is `BufferTooLong` with `failed_op: 0`, matching the
  firmware, because an aggregate overflow names no single operation.

  For `spi_batch` the check runs *before* the chip-select preflight, so an
  undeliverable batch no longer costs a `device/info` round-trip and reports
  `BufferTooLong` rather than `InvalidCsPin` when both are wrong — the same
  precedence the firmware uses.

  Reads totalling exactly 1014 bytes are unaffected. Only `Read` counts for
  I²C; `Read` and `Transfer` count for SPI.

- A request whose response is never produced no longer parks the caller
  forever. The known trigger is a request frame larger than the firmware's
  receive buffer: it is dropped before the dispatcher sees it, so the board
  stays completely healthy — measured answering 40/40 concurrent pings — while
  the call waits indefinitely.

  A timed-out call leaves the handle usable. Abandoning it drops the reply
  waiter, and sequence numbers are never reused, so a late reply is discarded
  rather than mismatched onto a later call.

  Note that firmware dispatch is serial, so a call issued while another is in
  flight also waits for that one. Sequential use is unaffected; sharing a
  handle across tasks alongside long calls may need `with_call_timeout`.

### Dependencies

- Added a direct `serde` dependency (already present transitively) for the
  trait bounds on the bounded-transport wrapper.
## [0.9.0] — 2026-09-01

### Breaking Changes

- Requires firmware reporting schema 0.8. Because the schema 0.8 change was
  to `DeviceInfo` itself, a 0.9.0 host against schema 0.7 firmware does not
  return `SchemaMismatch` — `device/info` is re-keyed, the reply is dropped
  unmatched, and `validate()` can only return `Timeout`. See the rewritten
  "Wire shape" note in `PicoDeGallo::validate()`'s documentation.

### Added

- Re-exported `BUILD_ID_CAPACITY`; `DeviceInfo::build_id()` exposes the
  firmware build identity without requiring callers to depend on `heapless`.
  `validate()` returns it for diagnostics but deliberately never treats it as
  a compatibility gate. Closes #159.

### Changed

- `ValidateError::Timeout`'s `Display` no longer advises a retry or a
  replug. A `device/info` timeout is at least as likely to be a
  host/firmware build mismatch: postcard-rpc derives each endpoint key from
  the response type's schema, so peers built from different trees answer
  under different keys and the reply is dropped as unmatched rather than
  decoded. Nothing fails, so nothing is reported — the call simply never
  completes. The message now names both causes and says plainly that the
  host cannot tell them apart. Refs #159.

## [0.8.0] — 2026-08-27

### Added

- Added cached, runtime-authoritative `PicoDeGallo::num_gpios()`.
  Closes #104.

- Hardware-in-the-loop tests for the zero-length write guards, `#[ignore]`d by
  default so CI does not run them. Run with
  `cargo test -p pico-de-gallo-lib -- --ignored --test-threads=1`; see the
  `hardware` module documentation for bench setup.

### Changed

- `spi_batch` now refuses `cs_pin >= num_gpios` locally without
  transmitting. A `device/info` failure remains a metadata error rather
  than becoming an invalid chip-select, and `num_gpios == 0` is distinct.
  The implicit metadata request uses the existing 300-second
  `DEVICE_INFO_TIMEOUT`. Closes #104.

- `PicoDeGallo::i2c_write` and `PicoDeGallo::i2c_batch` refuse a zero-length
  write locally, before transmitting. Firmware has refused it since #101, so
  this does not change which requests succeed — it removes a USB round-trip
  spent being told no, and makes the refusal independent of the attached
  firmware. Both return the value firmware would have returned:
  `Endpoint(I2cError::ZeroLengthWrite)`, and for a batch an `I2cBatchError`
  whose `failed_op` is the offending operation's exact index. A batch is
  validated in full before anything is sent, so a rejected batch never drives
  an earlier operation onto the bus. `i2c_write_read` is deliberately
  unaffected: an empty write phase there is legal, because that transfer does
  not terminate with a STOP. Closes #136.

- Reworded `PicoDeGalloError::Endpoint` documentation. It previously said the
  firmware had processed the request, which a locally refused request makes
  untrue. Callers still need not distinguish the two cases, but must not infer
  from this variant that the device was contacted.

- Documented that `PicoDeGallo::i2c_batch` executes its operations as one I²C
  transaction: adjacent same-direction operations concatenate, direction
  changes use a repeated START, and only the final operation receives a STOP.
  Bus failures report `failed_op = 0` for the transaction as a whole, while
  validation failures retain an exact index. The Rust API and wire shape are
  unchanged. Closes #128.

## [0.7.1] — 2026-07-28

### Added

- Add fallible constructors `PicoDeGallo::try_new()` and
  `PicoDeGallo::try_new_with_serial_number()` returning
  `Result<PicoDeGallo, String>`. Unlike `new()` / `new_with_serial_number()`
  (which panic when no matching device is present or the interface cannot be
  claimed), these surface the error, letting callers report "no device
  attached" or retry a transient claim failure. Additive and non-breaking.

## [0.7.0] — 2026-07-28

### Added

- Re-export `HostErr` (from `postcard_rpc::host_client`) and
  `WireError` (from `postcard_rpc::standard_icd`) from the crate
  root, and make the `host_client` module path public. This lets
  downstream crates name the transport error types when mapping
  [`PicoDeGalloError`] and [`ValidateError`] into their own error
  representations without taking a direct dependency on
  `postcard-rpc`. Additive and non-breaking; the names were
  previously imported privately for the crate's own use.

  Motivated by the new `gallo-mcp` crate, whose error mapping
  matches on `PicoDeGalloError::Comms(HostErr::Closed)` to surface
  a distinct "no device attached" message.

## [0.6.0] — 2026-06-22

### Fixed (2026-06-04 — Category A hotfix host-only PR)

- `PicoDeGallo::validate()` now checks `schema_major` in addition
  to `schema_minor`. Previously, a firmware reporting a bumped
  major version with a matching minor would silently pass
  validation and the host would subsequently mis-decode wire
  bytes (silent garbage out). The schema-check policy is now
  extracted into a private `check_schema_compatible(&DeviceInfo)`
  helper with four regression tests covering matching versions
  and the three rejection cases (bumped major, bumped minor,
  both bumped).
- `ValidateError::SchemaMismatch` payload extended with
  `expected_major` and `actual_major` fields; `Display` impl
  shows the full `MAJOR.MINOR.x` skew rather than just the minor
  versions.

  This is a structural change to a public enum variant payload.
  Direct constructors and exhaustive matches against
  `SchemaMismatch` will need to add the two new fields. The
  variant is not on the wire (`ValidateError` is a host-side
  type), so there is no schema impact.

### Added (2026-06-03 — Category A hotfix wire PR)

- `PicoDeGallo::gpio_wait_for_{high,low,rising_edge,falling_edge,any_edge}_with_timeout`
  methods take a `std::time::Duration` and return
  `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))` on expiry.
  The existing two-arg methods (`gpio_wait_for_high(pin)` etc.)
  preserve the wait-forever behavior by passing `timeout_ms: 0`
  on the wire. Closes Category A finding #2 at the host-library
  layer.

### Changed (2026-06-03 — Category A hotfix wire PR)

- Bumped `pico-de-gallo-internal` dependency to 0.6.0 (wire schema
  change: append-only `timeout_ms: u32` on `GpioWaitRequest`,
  append-only `GpioError::Timeout` variant). Lockstep with firmware
  0.10.0 per AGENTS.md §6.5.

### Added

- `PicoDeGallo::system_reset_subscriptions()` host method returns
  the number of subscriptions reset. The recommended connect
  sequence is now `new()` → `validate().await?` →
  `system_reset_subscriptions().await?`.
- `MAX_BATCH_OPS` and `MAX_TRANSFER_SIZE` are now re-exported from
  `pico-de-gallo-internal` so downstream consumers don't have to
  pull in the wire crate just to validate batch sizes.

### Fixed

- `PicoDeGallo::validate()` no longer mis-classifies transport,
  postcard-decode, and frame-size errors as
  `ValidateError::LegacyFirmware`. Only `WireError::UnknownKey` and
  `WireError::KeyTooSmall` (the postcard-rpc signals for "this
  firmware has no handler for that endpoint key") map to
  `LegacyFirmware`; every other host error routes to
  `ValidateError::Comms`, so users see "comms failure" instead of
  being told to upgrade firmware that is already current. Surfaces
  in `gallo_get_device_info` as the correct `Status::CommsFailed`
  (−1) when the wire is the actual problem. ([REVIEW-2026-05-29
  P1-1])

## [0.5.0] — 2026-05-04

### Breaking Changes

- `uart_get_config()` now returns `PicoDeGalloError<UartError>` and
  `adc_get_config()` now returns `PicoDeGalloError<AdcError>` (was
  `PicoDeGalloError<Infallible>`).

### Added

- `device_info()` and `validate()` methods, `ValidateError` enum.
  Re-exported `Capabilities` and `DeviceInfo`.

## [0.4.0] — 2026-04-22

### Breaking Changes

- All method return types updated from `PicoDeGalloError<*Fail>` to
  `PicoDeGalloError<I2cError>`, `PicoDeGalloError<SpiError>`, or
  `PicoDeGalloError<GpioError>`.

### Added

- `gpio_subscribe(pin, edge)`, `gpio_unsubscribe(pin)`, and
  `subscribe_gpio_events(depth)` methods. Re-exported `GpioEdge`,
  `GpioEvent`, `IoClosed`, `MultiSubscription`.
- `i2c_batch(address, ops)` and `spi_batch(cs, ops)` async methods.
  Re-exported `I2cBatchOp`, `SpiBatchOp`, `encode_i2c_batch_ops`,
  `encode_spi_batch_ops`, `I2cBatchError`, `SpiBatchError`.
- `pwm_set_duty_cycle`, `pwm_get_duty_cycle`, `pwm_enable`,
  `pwm_disable`, `pwm_set_config`, `pwm_get_config` async methods.
  Re-exported `PwmError`, `PwmDutyCycleInfo`,
  `PwmConfigurationInfo`.
- `adc_read(channel)`, `adc_get_config()` methods. Re-exported
  `AdcChannel`, `AdcError`, `AdcConfigurationInfo`.
- `onewire_reset()`, `onewire_read(len)`, `onewire_write(data)`,
  `onewire_write_pullup(data, duration_ms)`, `onewire_search()`,
  `onewire_search_next()` methods. Re-exported `OneWireError`.
- `uart_read(count, timeout_ms)`, `uart_write(contents)`,
  `uart_flush()`, `uart_set_config(baud_rate)`,
  `uart_get_config()` methods. Re-exported `UartError` and
  `UartConfigurationInfo`.
- `PicoDeGallo::i2c_scan(include_reserved)` method returning
  `Vec<u8>`.
- `PicoDeGallo::gpio_set_config(pin, direction, pull)` method;
  re-exported `GpioDirection` and `GpioPull`.
- `PicoDeGallo::i2c_get_config()` and `spi_get_config()` methods;
  re-exported `SpiConfigurationInfo`.

### Fixed

- Corrected `MAX_TRANSFER_SIZE` references in rustdoc for
  `i2c_read`, `i2c_write_read`, and `spi_read` (was 512, actual
  value is 4096).

## [0.3.0] — 2025-04-20

### Breaking Changes

- Split `set_config()` into `i2c_set_config()` and
  `spi_set_config()`.
- `PicoDeGalloError` is now generic over the endpoint error type.

### Added

- `list_devices()` function for enumerating connected boards.
- `Display` and `std::error::Error` implementations for
  `PicoDeGalloError`.

### Changed

- `client` field made private (was accidentally public).
