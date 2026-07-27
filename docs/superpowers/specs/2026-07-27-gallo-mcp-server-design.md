# Design: `gallo-mcp` — MCP server for pico-de-gallo

- **Date:** 2026-07-27
- **Status:** Approved (brainstorming complete; ready for implementation planning)
- **Branch:** `feat/gallo-mcp`
- **Author:** Felipe Balbi (with AI assistance)

## 1. Summary

Add a seventh host-workspace crate, `pico-de-gallo-mcp` (package and binary
name `gallo-mcp`), that exposes a Pico de Gallo device to AI coding agents
through a Model Context Protocol (MCP) server built on the Rust MCP SDK
(`rmcp`). The server wraps the existing async host library
`pico-de-gallo-lib::PicoDeGallo` — the same surface the `gallo` CLI and the
Python bindings wrap — and presents each peripheral operation as an MCP tool.

The goal is to close pico-de-gallo's core develop-test loop for agents: an
agent writing an embedded driver can scan a bus, poke registers, read an ADC,
and toggle GPIO on real hardware, then verify its code against ground truth
instead of hallucinating device behavior.

## 2. Motivation

pico-de-gallo exists to let developers write and test embedded drivers on
their laptop without cross-compiling and flashing. An MCP server extends that
value to AI agents: it gives an agent a way to actually touch hardware while
writing driver code. No MCP, JSON-RPC, or agent-facing tooling exists in the
repository today — this is greenfield.

The cleanest integration point is `pico-de-gallo-lib::PicoDeGallo` (async,
tokio-native, ~60 public methods across I²C, SPI, UART, GPIO, PWM, ADC, and
1-Wire), each returning `Result<T, PicoDeGalloError<E>>`. This is a third,
structurally-identical consumer alongside the CLI and PyO3 bindings.

### Distribution / "start on demand"

The server is scoped by **project**, not by device presence. MCP clients
(opencode, Claude Code, Cursor) spawn a local stdio server from a per-project
config file that is safe to commit to git (opencode `opencode.json`, Claude
Code `.mcp.json`, Cursor `.cursor/mcp.json`). opencode discovers
`opencode.json` from the working directory up to the git root and merges it
over global config. So the server is present only in repositories that opt in,
and, thanks to lazy connection (§5), tolerates the Pico being unplugged.

## 3. Scope

### In scope (v1)

- `gallo-mcp` binary: stdio MCP server wrapping `PicoDeGallo`.
- ~35 tools, one per peripheral operation (§6).
- Single persistent, lazily-connected USB session (§5).
- Hex-string byte encoding in and out (§7).
- Timeout-only GPIO edge waits (§6).
- Tool annotations (`readOnlyHint` / `destructiveHint`) so clients can prompt
  for write approval (§8).
- Unit tests (no hardware) + a documented hardware-in-the-loop smoke test
  against a TMP108 I²C temperature sensor (§9).
- Book chapter, client-config snippets, CHANGELOG, README, CI, release lane
  (§10).
- Published to crates.io like `gallo` (`mcp-v*` tag lane).

### Explicitly out of scope (v1)

- Infinite / no-timeout GPIO blocking waits (`gpio_wait_for_high/low`,
  no-timeout edge waits).
- GPIO push subscriptions (`gpio_subscribe`, `subscribe_gpio_events`) — a
  push stream does not fit MCP's request/response model.
- HTTP / SSE (remote) transport — stdio only in v1.
- Server-side write gating via env var (dropped in favor of client-side
  permission prompts; see §8).
- A pin allowlist, rate limiting, or policy config file.

## 4. Crate identity & placement

- **Directory:** `crates/pico-de-gallo-mcp/`.
- **Package name:** `gallo-mcp`. Binary name defaults to the package name
  (`gallo-mcp`); no `[[bin]]` section needed.
- **Edition:** 2024. **MSRV:** `rust-version = "1.90"`.
- **Metadata:** `license = "MIT"`, `repository`, `documentation`,
  `categories`, `keywords`, `readme = "README.md"` — mirroring
  `pico-de-gallo-app`.
- **Workspace:** added to the root `Cargo.toml` `members` list (host
  workspace). NOT added to the firmware workspace.
- **Key dependencies:**
  - `pico-de-gallo-lib = { version = "0.6.0", path = "../pico-de-gallo-lib" }`
    (matching the CLI's current dep spec).
  - `rmcp` 2.2.0 with the stdio server feature.
  - `tokio` with `rt-multi-thread`, `macros`, `signal` (and `time` if needed).
  - `serde`, `serde_json` for tool params/results.
  - `clap` (derive) for `--serial-number`.
  - A logging/error facility consistent with the repo (e.g. `tracing` to
    stderr; MUST NOT write logs to stdout, which carries the MCP framing).
- **Release coupling:** depends on `pico-de-gallo-lib`, so it participates in
  the lockstep dep-spec rule (AGENTS.md §6.5): when `lib` is version-bumped in
  a release commit, this crate's `pico-de-gallo-lib` dep spec bumps too. It is
  NOT wire-coupled to `pico-de-gallo-internal` directly, so it does not force
  a schema-version bump. It gets its own tag lane, `mcp-v*`, wired into
  `release-crates.yml`.

## 5. Server architecture & connection model

### Process shape

`main.rs`:
1. Parse CLI (`--serial-number <STRING>` optional).
2. Initialize a multi-thread tokio runtime.
3. Construct the `GalloMcp` service.
4. Serve over stdio via rmcp.
5. Handle `signal`-based shutdown; drop the service cleanly.

### Module layout (Approach A — one handler module per peripheral)

```
crates/pico-de-gallo-mcp/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs      # stdio bootstrap, tokio runtime, CLI parsing
    ├── lib.rs       # GalloMcp service struct + shared state; router composition
    ├── error.rs     # lib error -> rmcp ErrorData mapping; "no device" classification
    ├── encoding.rs  # hex parse/format helpers; shared param/result structs
    ├── device.rs    # list_devices, status, device_info, version, ping
    ├── i2c.rs       # i2c_* tools
    ├── spi.rs       # spi_* tools
    ├── uart.rs      # uart_* tools
    ├── gpio.rs      # gpio_* tools (timeout waits only)
    ├── pwm.rs       # pwm_* tools
    ├── adc.rs       # adc_* tools
    └── onewire.rs   # onewire_* tools
```

Each peripheral module contributes its tools via rmcp's `#[tool]` /
`#[tool_router]` macros; the per-module routers are merged onto the single
`GalloMcp` service in `lib.rs`.

### Shared state

`GalloMcp` holds:
- `device: Arc<PicoDeGallo>` — constructed once at startup with lazy connect
  (`new()` or `new_with_serial_number()`). No USB claim happens until first
  I/O.
- Cached `DeviceInfo` (populated after the first successful `validate()`).
- An async `Mutex` (or equivalent) to serialize device access if the lib does
  not already guarantee transaction serialization (verified during
  implementation; the mutex is added only if needed).

There is **no** `write_allowed` flag and **no** `GALLO_MCP_ALLOW_WRITE` env
var (see §8).

### Connection lifecycle

- One persistent `PicoDeGallo` for the whole process; every tool reuses the
  same `Arc`.
- **Lazy + resilient:** `PicoDeGallo` connects lazily and retries. If no Pico
  is attached, the first I/O tool returns a clean "no device attached" error
  (§'Error handling'). Plugging a device in mid-session makes the next call
  succeed with no restart.
- **Validation once:** on the first I/O tool call (or via the `status` tool),
  the server calls `validate()` to check schema compatibility, then calls
  `system_reset_subscriptions()` — mirroring the host's connect behavior
  (AGENTS.md 2026-05-29 row). The result is cached so it is paid once.
- **WinUSB:** a single long-lived connection sidesteps the claim/release race
  (AGENTS.md §13.17) entirely.

### Concurrency

rmcp may dispatch tool calls concurrently. Because a single USB transaction
(e.g. write-then-read) must not interleave with another, device access is
serialized behind the shared mutex.

## 6. Tool surface (~35 tools)

Tools are namespaced by the server's client-side name (e.g.
`pico-de-gallo_i2c_read`). Byte payloads are hex strings in and out (§7).
Read/write classification drives tool annotations only (§8), not gating.

### device.rs (read-only)
- `list_devices` — enumerate attached Picos (no connection needed).
- `status` — attached? serial, firmware/schema version, capabilities;
  triggers the one-time `validate()`.
- `device_info`, `version`, `ping`.

### i2c.rs
- Read: `i2c_read`, `i2c_write_read`, `i2c_scan`, `i2c_get_config`.
- Write: `i2c_write`, `i2c_batch`, `i2c_set_config`.

### spi.rs
- Read: `spi_read`, `spi_transfer`, `spi_get_config`.
- Write: `spi_write`, `spi_flush`, `spi_batch`, `spi_set_config`.

### uart.rs
- Read: `uart_read`, `uart_get_config`.
- Write: `uart_write`, `uart_flush`, `uart_set_config`.

### gpio.rs
- Read: `gpio_get`, `gpio_wait_for_rising_edge_with_timeout`,
  `gpio_wait_for_falling_edge_with_timeout`,
  `gpio_wait_for_any_edge_with_timeout` (all require non-zero `timeout_ms`).
- Write: `gpio_put`, `gpio_set_config`.

### pwm.rs
- Read: `pwm_get_duty_cycle`, `pwm_get_config`.
- Write: `pwm_set_duty_cycle`, `pwm_enable`, `pwm_disable`, `pwm_set_config`.

### adc.rs
- Read: `adc_read` (channel 0–3, validated), `adc_get_config`.

### onewire.rs
- Read: `onewire_read`, `onewire_search` (folds `onewire_search` /
  `onewire_search_next` into one tool with a cursor / continue argument;
  returns the ROM code and a cursor for continuation).
- Write: `onewire_reset` (classified as a write — conservative, since it
  drives a presence pulse on the bus), `onewire_write`, `onewire_write_pullup`.

### Excluded from v1
Infinite/no-timeout GPIO waits, plain `gpio_wait_for_high/low`,
`gpio_subscribe` / `subscribe_gpio_events`.

## 7. Byte payload encoding

- **Input:** write tools accept a hex string using the `gallo` CLI's existing
  parser conventions — `0x48,0x00`, `4800`, or decimal.
- **Output:** read tools return a structured result containing both a hex
  string and a decoded integer array, e.g.
  `{ "hex": "0x48,0x00", "bytes": [72, 0] }`.
- Rationale: hex is how datasheets express register addresses and values, so
  it is the most natural representation for a datasheet-driven agent, and it
  matches the CLI documentation.
- `encoding.rs` owns the parse/format helpers and is unit-tested with
  round-trip tests.

## 8. Write approval model

**Client prompt only, no server gate.**

- All tools are always registered. There is no server-side write gate and no
  `GALLO_MCP_ALLOW_WRITE` env var.
- Read tools carry `readOnlyHint: true`. Write / actuation tools carry
  `destructiveHint: true` (and `readOnlyHint: false`).
- Approval is delegated entirely to the MCP client's permission system. In
  opencode, the operator configures `permission` (e.g. `"ask"`) for the
  `pico-de-gallo_*` write tools (or globally), and the client prompts before
  each write call. Reads pass through silently.

**Accepted risk:** under a client with no permission system, or one configured
to blanket-allow, an autonomous agent can drive GPIO/PWM and actuate pins with
no gate. This is idiomatic MCP (the client owns human consent) but must be
called out prominently in the book's security section so downstream users know
to configure client permissions.

Rationale for not doing server-side prompting: `gallo-mcp` is a stdio process
with no TTY, and rmcp server-initiated elicitation is not universally
supported by clients — a server-side confirmation prompt would be fragile and
client-dependent.

## 9. Error handling & data flow

### Error mapping (`error.rs`)

A single helper converts lib errors into rmcp `ErrorData` with actionable
messages:
- `PicoDeGalloError::Comms(HostErr::Closed)` / device-not-found → a distinct
  **"no device attached"** error, so the agent knows to prompt for a plug-in
  and retry rather than treating it as a generic failure.
- `PicoDeGalloError::Endpoint(e)` → the peripheral error surfaced via its
  `Display` (e.g. I²C `NoAcknowledge`), so the agent can reason about it.
- `ValidateError::{LegacyFirmware, SchemaMismatch, Comms}` → clear message
  including expected vs actual schema version for `SchemaMismatch`.
- Bad hex / out-of-range ADC channel / zero timeout → validation error naming
  the offending argument.

### Data flow (per tool call)

deserialize params → (validate-once if first use) → parse hex args → acquire
device mutex → call lib method → format result (hex string + int array for
byte payloads; typed struct for configs) → release mutex → serialize response.

### Result shapes

- `i2c_read` → `{ "hex": "0x48,0x00", "bytes": [72, 0] }`.
- `adc_read` → `{ "raw": 1234 }`.
- `i2c_scan` → `{ "addresses": ["0x48"] }`.
- Config tools → the lib's `*ConfigurationInfo` fields as JSON.

## 10. Testing & documentation

### Testing

- **Unit tests (no hardware, run in CI):**
  - Hex parse/format round-trips (`encoding.rs`).
  - Argument validation (bad hex, out-of-range ADC channel, zero timeout
    rejected).
  - Error mapping (lib error → correct rmcp `ErrorData` shape; "no device"
    classification).
  - JSON (de)serialization of every tool's params/result structs.
  - Tool-router wiring: assert the expected tool set is registered with the
    correct hints (read vs destructive).
- **Hardware-in-the-loop (manual, not CI):** a documented smoke test against a
  TMP108 I²C temperature sensor — `status`, `i2c_scan` (expect `0x48`),
  `i2c_write_read` to read the temperature register, decode to °C. Captured in
  the book as the worked validation example. CI remains hardware-free.

### Documentation (book-parity, AGENTS.md §15.1 — hard rule)

- New chapter `book/src/crates/mcp.md`: what it is, install, the tool catalog,
  byte/hex conventions, the "no device attached" behavior, and the
  **client-permission security note** (the only write guard).
- Client-config snippets (own chapter or a section within `mcp.md`):
  ready-to-commit `opencode.json`, `.mcp.json` (Claude Code),
  `.cursor/mcp.json` — the "drop this in your driver repo" story.
- Add new pages to `book/src/SUMMARY.md`.
- Update `book/src/crates/overview.md` to list the 7th crate.
- Ship a root-level `opencode.json` in the pico-de-gallo repo so contributors
  developing pico-de-gallo itself get the tools instantly.
- `CHANGELOG.md` entry (Keep a Changelog).
- Crate `README.md`.
- rustdoc on every public item + crate-level `//!` docs.

### CI & release integration

- Add `pico-de-gallo-mcp` to the per-crate matrix in `check.yml` (fmt, clippy
  `-D warnings`, test, `cargo hack` feature-powerset, MSRV 1.90, doc).
- Add it to `cargo-deny` coverage.
- Add an `mcp-v*` tag lane to `release-crates.yml` publishing to crates.io.
- Commit `Cargo.lock` alongside the `Cargo.toml` change (both host locks in
  sync; CI `lockfile` job gates this).
- LF endings on every new file (`dos2unix` on Windows).

## 11. Open items deferred to implementation

- Confirm whether `pico-de-gallo-lib` already serializes USB transactions; add
  the async mutex only if it does not.
- Exact `rmcp` 2.2.0 API for multi-module `#[tool_router]` composition and for
  setting `readOnlyHint` / `destructiveHint` annotations.
- Precise shape of the `onewire_search` cursor argument.
- Whether `--serial-number` should be complemented by an env var for config
  ergonomics.

## 12. Hard rules to respect (from AGENTS.md)

- LF line endings on every text file (§3).
- Commit `Cargo.lock` alongside any `Cargo.toml` change; validate with
  `--locked` (§4, §7).
- Conventional Commits with a crate scope; a new `mcp` scope for this crate
  (§10). AI-assisted commits carry `Assisted-by:` and
  `Co-authored-by: Copilot` trailers; never `Signed-off-by:` (§4).
- Book and code land together (§4 rule 11, §15.1).
- No version *bumps* in ordinary feature PRs — versioning is a deliberate
  manual release step (§4 rule 12). A brand-new crate must still carry an
  initial `[package].version` to build; it is introduced at `0.1.0` in this
  feature PR and left untouched thereafter. The first publish to crates.io
  (the `mcp-v0.1.0` tag) is a separate, deliberate release step performed by a
  maintainer, not part of this feature PR.
- Do not push or force-push without explicit permission (§4 rule 8).
