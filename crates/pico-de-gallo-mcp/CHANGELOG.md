# Changelog

All notable changes to `gallo-mcp` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-07-28

### Added

- Initial release of `gallo-mcp`: a Model Context Protocol server that exposes
  a Pico de Gallo USB bridge to AI agents over stdio, built on the
  [`rmcp`](https://crates.io/crates/rmcp) SDK and wrapping `pico-de-gallo-lib`.
- One MCP tool per peripheral operation across I²C, SPI, UART, GPIO, PWM, ADC,
  and 1-Wire (35 tools total), plus device tools (`list_devices`, `status`,
  `device_info`, `version`, `ping`).
- Hex-string byte payloads (`"0x00,0x10"` in; `{ "hex", "bytes" }` out).
- Read tools annotated `readOnlyHint`; write/actuation tools annotated
  `destructiveHint`. Write approval is delegated to the MCP client.
- Timeout-bounded GPIO edge waits (non-zero timeout enforced). No infinite
  waits or push subscriptions in this release.
- Per-call device connection: each tool opens the board, runs, and releases it
  when the call completes, so the board stays free for the `gallo` CLI and
  other host processes between calls. The server starts with no board attached
  and each tool begins working as soon as one is present.
