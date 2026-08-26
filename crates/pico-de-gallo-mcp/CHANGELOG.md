# Changelog

All notable changes to `gallo-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Documented that the `i2c_batch` tool executes one I²C transaction: adjacent
  same-direction operations concatenate, direction changes use a repeated
  START, and only the final operation receives a STOP. A bus failure reports
  `failed_op = 0` for the transaction as a whole; validation failures retain an
  exact index. The MCP tool schema is unchanged. Closes #128.

## [0.3.0] — 2026-08-24

### Changed

- `spi_batch` validates `cs` against retained, already-validated
  `DeviceInfo::num_gpios` before it transmits, and reports invalid
  parameters distinctly. Payload parsing deliberately precedes `connect()`:
  connecting calls `system_reset_subscriptions()`, which tears down every
  GPIO subscription on the board, including subscriptions owned by other
  processes, so malformed hex cannot cause that destructive cross-process
  side effect. Closes #104.

## [0.2.0] — 2026-08-05

### Added

- Optional `serial_number` argument on every tool except `list_devices`, so a
  single server instance can address any attached board per call.
- Calls naming different boards run concurrently. The connection lock is keyed
  on the board rather than on the server, so a long `gpio_wait_*` holds only
  the board it addressed; calls to the *same* board queue.
- A startup warning when `--serial-number` names a board that is not attached
  while others are. A mistyped pin previously started a healthy-looking server
  that then failed every device call for the whole session. It stays a warning,
  not a startup error: running with no board attached and plugging one in
  mid-session is supported. Emitted by default on stderr, so it reaches
  operators whose MCP client sets no `RUST_LOG`; `RUST_LOG` still overrides
  verbosity when set.

### Changed

- **Breaking (tool surface).** Omitting `serial_number` with two or more
  boards attached is now an error listing the available serials, instead of
  silently binding to whichever board enumerated first. The single-board case
  is unchanged: `serial_number` stays optional.
- **Breaking (response shape).** Device tool responses are now wrapped as
  `{ "serial_number": ..., "result": ... }`, reporting which board served the
  call. `list_devices` and `status` are the exceptions: the first opens no
  board, and the second already carries a `serial_number` of its own.
- **Breaking (response shape).** `list_devices` returns an object instead of a
  bare array. Entries gained `pinned` and `default_target` flags, and the
  object adds `pinned`, `serial_number_required`, and a `note` that is present
  only when a serial is required.
- `--serial-number` now pins the server: a tool call naming a different board
  is refused, and `serial_number` stays optional however many boards are
  attached.
- Two or more attached boards reporting the *same* serial number are now
  refused rather than resolved to whichever enumerated first — naming that
  serial no longer identifies a board. This affects one bench configuration:
  the firmware falls back to an all-zero serial when the OTP chip-ID read
  fails, so two such boards collide, and `--serial-number` does not rescue it
  because the pinned path refuses too. Detach all but one.
- Opening a board still resets **every** GPIO subscription on it, including
  ones owned by other host processes. Per-call selection widens which board
  that reaches: previously only the board the server was bound to, now any
  attached board named by any call — including from the `readOnlyHint` tools
  (`status`, `device_info`, `version`, `ping`, `i2c_scan`). `list_devices`
  opens no board and is unaffected.
- `status` no longer reports `attached: false` when selection fails. It never
  errors, and gained `serial_number`, `ambiguous`, `available`, `pinned`, and a
  `reason` for an unresolved target, alongside the existing `firmware_version`,
  `schema_major`, and `schema_minor`.

### Fixed

- Errors raised after a board was successfully opened no longer claim no
  device is attached, which sent an agent looking for a missing board instead
  of a board that stopped responding.

## [0.1.0] — 2026-07-28

### Added

- Initial release of `gallo-mcp`: a Model Context Protocol server that exposes
  a Pico de Gallo USB bridge to AI agents over stdio, built on the
  [`rmcp`](https://crates.io/crates/rmcp) SDK and wrapping `pico-de-gallo-lib`.
- One MCP tool per peripheral operation across I²C, SPI, UART, GPIO, PWM, ADC,
  and 1-Wire (38 tools), plus device tools (`list_devices`, `status`,
  `device_info`, `version`, `ping`) — 43 in total.
- Hex-string byte payloads (`"0x00,0x10"` in; `{ "hex", "bytes" }` out).
- Read tools annotated `readOnlyHint`; write/actuation tools annotated
  `destructiveHint`. Write approval is delegated to the MCP client.
- Timeout-bounded GPIO edge waits (non-zero timeout enforced). No infinite
  waits or push subscriptions in this release.
- Per-call device connection: each tool opens the board, runs, and releases it
  when the call completes, so the board stays free for the `gallo` CLI and
  other host processes between calls. The server starts with no board attached
  and each tool begins working as soon as one is present.
