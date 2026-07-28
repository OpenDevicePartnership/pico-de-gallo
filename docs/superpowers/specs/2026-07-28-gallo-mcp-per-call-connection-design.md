# gallo-mcp: per-tool-call device connection

**Date:** 2026-07-28
**Crate:** `gallo-mcp` (`crates/pico-de-gallo-mcp`)
**Type:** behavior change (host-only; no wire-protocol, no CLI-surface, no tool-surface change)

## Problem

`gallo-mcp` currently constructs one `PicoDeGallo` in `GalloMcp::new()` and
shares it (`Arc<PicoDeGallo>`) for the entire server lifetime. The
`PicoDeGallo` transport spawns a background `nusb` worker that claims the USB
interface (an exclusive WinUSB claim on Windows) as soon as it connects, and
holds it until dropped.

Consequence: the MCP server keeps the Pico de Gallo device **busy forever**.
An agentic workflow (through the MCP server) and a traditional workflow
(through the `gallo` CLI or another host process) cannot compose — the second
one to open the board is blocked while the MCP server is running.

## Goal

Hold no USB connection between tool calls. Open the device on tool entry,
run the operation, release it when the tool completes. Between calls the board
is free for the `gallo` CLI and other host processes.

## Design

### Decisions

| Question | Decision |
|----------|----------|
| Connection scope | **Per tool call** — construct `PicoDeGallo`, use, drop, around each handler. |
| Validation | **Validate on every call** — `validate()` (schema gate) + `system_reset_subscriptions()` on each connect. |
| No-board / not-connected | **Rely on `validate()` as the connect gate** — the lib connects lazily; `validate()` is the first RPC and blocks until connected or errors. |
| Connection shape | **RAII guard** derefing to `PicoDeGallo` and carrying the validated `DeviceInfo`. |
| Board-tolerant tools | **All go through `connect()`**; `status` catches the error and reports `attached:false`; `list_devices` stays connectionless. |

### Architecture

`GalloMcp` no longer owns a device or a `OnceCell<DeviceInfo>`; it owns only
the target selector:

```rust
#[derive(Clone)]
pub struct GalloMcp {
    serial_number: Option<String>,
    tool_router: ToolRouter<Self>,
}
```

### RAII guard

```rust
pub(crate) struct Device {
    inner: PicoDeGallo,
    info: DeviceInfo,
}

impl std::ops::Deref for Device {
    type Target = PicoDeGallo;
    fn deref(&self) -> &PicoDeGallo { &self.inner }
}

impl Device {
    pub(crate) fn info(&self) -> &DeviceInfo { &self.info }
}
```

`connect()` constructs, validates, resets subscriptions, and returns the guard:

```rust
impl GalloMcp {
    pub(crate) async fn connect(&self) -> Result<Device, ErrorData> {
        let inner = match self.serial_number.as_deref() {
            Some(sn) => PicoDeGallo::new_with_serial_number(sn),
            None => PicoDeGallo::new(),
        };
        let info = inner.validate().await.map_err(error::map_validate_err)?;
        let _ = inner.system_reset_subscriptions().await;
        Ok(Device { inner, info })
    }
}
```

Dropping the guard at handler end drops `PicoDeGallo`, whose nusb worker
releases the USB claim.

### Handler rewrite pattern

Each handler connects first, then operates through the guard:

```rust
async fn i2c_read(&self, Parameters(p): Parameters<I2cReadParams>)
    -> Result<CallToolResult, ErrorData>
{
    let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
    let dev = self.connect().await?;
    let data = dev.i2c_read(addr, p.count).await.map_err(map_pdg_err)?;
    ok_json(&Bytes::from_slice(&data))
}
```

- The I2C-batch handler keeps its owned-`write_bufs` prelude; `dev` outlives
  the borrowed `&ops` slice within the handler body, so lifetimes are
  unaffected.
- **`list_devices`** — unchanged; uses `pico_de_gallo_lib::list_devices()`,
  no `connect()`.
- **`status`** — calls `connect()` but catches the error:
  `Ok(dev) => attached:true (dev.info())`, `Err(_) => attached:false`.
- **`version` / `device_info` / `ping`** — now go through `connect()` and
  therefore validate first (stricter than today, but consistent and safe).

The old `ensure_validated()` helper, `OnceCell`, and `Arc` usage are removed.

### Why the Windows ACCESS_DENIED race does not apply

AGENTS.md §13.17 (2026-07-20) documents an `ACCESS_DENIED` failure caused by
**two concurrent** `PicoDeGallo` connections within one CLI invocation. Here,
MCP stdio tool calls are sequential and each guard is fully dropped before the
next handler runs, so connections never overlap. No double-claim occurs.

### Firmware-owned state survives disconnect (intended)

`pwm_enable`, `i2c_set_config`, GPIO output levels, etc. mutate state that
lives in the **firmware**, which persists across host disconnect. Per-call
connect/drop does not reset that peripheral state — exactly the desired
behavior. The MCP server exposes only self-contained `gpio_wait_for_*_with_timeout`
tools (single blocking RPCs), not the push-based `gpio/subscribe` topic, so no
long-lived subscription needs to survive across calls.

## Error handling

Unchanged mappings: `map_pdg_err`, `map_validate_err`, `invalid_arg`. Connect
failures (no board / schema mismatch) surface as tool errors, except `status`
which catches them and reports `attached:false`.

## Testing

- `router_for_test()` registration tests untouched (never touch USB).
- Param-deserialization tests untouched.
- No new hardware-dependent tests added; the connect path requires a board.

## Documentation (AGENTS.md §15.1 parity)

- `README.md` — rewrite the "connects lazily / starts with no board" paragraph
  to describe the per-call open/close model and the "board free between calls"
  property.
- `book/src/crates/mcp.md` — update any statement describing a long-lived
  session to the per-call model.
- `CHANGELOG.md` — add an entry (Keep a Changelog) under the crate.

## Scope / release

Host-only. No wire-protocol change, no CLI-surface change, no tool-surface
change. `gallo-mcp` has never been published (still at its initial `0.1.0`),
so **do not** bump `[package].version` — there is no released version to
supersede. Commit as a `fix`/`refactor(mcp)`.
