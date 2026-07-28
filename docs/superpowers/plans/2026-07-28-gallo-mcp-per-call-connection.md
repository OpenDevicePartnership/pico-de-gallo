# gallo-mcp per-call device connection — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change `gallo-mcp` from holding one long-lived USB connection for the whole server lifetime to opening the Pico de Gallo per tool call and releasing it when the call completes, so agentic (MCP) and traditional (`gallo` CLI) workflows compose instead of the server holding the board busy forever.

**Architecture:** `GalloMcp` stops owning a device; it stores only the target `serial_number`. A new `pub(crate)` RAII guard `Device` owns a freshly-constructed `PicoDeGallo` plus its validated `DeviceInfo`, and `Deref`s to `PicoDeGallo` so handlers call device methods unchanged. A new `GalloMcp::connect()` constructs the device, validates schema compatibility, runs the connect-time subscription reset, and returns the guard. Every tool handler calls `let dev = self.connect().await?;` after argument validation and operates through `dev`; the guard drops at handler end, releasing the USB claim. No wire-protocol, CLI-surface, or tool-surface change.

**Tech Stack:** Rust 2024, `rmcp` 2.2 (`#[tool_router]`/`#[tool_handler]` macros), `pico-de-gallo-lib`, `tokio`.

**Design doc:** `docs/superpowers/specs/2026-07-28-gallo-mcp-per-call-connection-design.md`

---

## Notes for the implementer

- **This is a mechanical refactor, not a feature.** There is no hardware-free unit test that meaningfully exercises per-call connect (it requires a board). Verification is: it **compiles**, `clippy` is clean, the **existing** router-registration + param-deserialization tests still pass, and `fmt` is clean. Behavior on hardware is verified manually (see design doc). Do not invent weak tests to fake red/green TDD.
- **The whole code change is one commit.** Removing the `device`/`info` fields from `GalloMcp` breaks every handler that references `self.device`, so the struct change and all handler edits must land together to keep the build green (AGENTS.md §4 rule #9: each commit builds cleanly on its own). Splitting them would create a non-compiling or, worse, a Windows-ACCESS_DENIED-at-runtime intermediate (two concurrent claims — AGENTS.md §13.17).
- **Docs are a second commit in the SAME PR.** AGENTS.md §15.1 requires the book change to ship with the code change in one logical change/PR. Do not merge the code commit without the docs commit.
- **LF endings.** All these files are already LF. The `edit` tool preserves LF. Still run `dos2unix` on every touched file before committing (AGENTS.md §3).
- **No dependency change** → no `Cargo.toml`/`Cargo.lock` edits, no lockfile-drift risk.
- **No version bump.** `gallo-mcp` is unpublished (`0.1.0`); AGENTS.md §4 rule #12 — do not touch `[package].version`.
- **`connect()` placement rule (uniform across handlers):** put `let dev = self.connect().await?;` *after* all fallible argument parsing/validation (`.map_err(invalid_arg)?` lines and any `match` that early-returns) and *immediately before* the first device call. This fails fast on bad arguments without opening USB, and holds the claim for the shortest time.
- **Commit trailers (AGENTS.md §10):** every commit gets, and must NOT get `Signed-off-by`:
  ```
  Assisted-by: GitHub Copilot:claude-opus-4.8
  Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
  ```
  Set the `Assisted-by` model to the model you are actually running as. If unsure, `GitHub Copilot:claude-opus-4.8` is the current session's model.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/pico-de-gallo-mcp/src/lib.rs` | Service struct, router wiring, connection helper | Replace `device`/`info` fields with `serial_number`; add `Device` guard + `connect()`; remove `ensure_validated`, `Arc`, `OnceCell` |
| `crates/pico-de-gallo-mcp/src/device.rs` | device tools | `status` uses `connect()` and catches error; `device_info`/`version`/`ping` go through `connect()` |
| `crates/pico-de-gallo-mcp/src/i2c.rs` | I2C tools (7) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/src/spi.rs` | SPI tools (7) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/src/uart.rs` | UART tools (5) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/src/gpio.rs` | GPIO tools (6) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/src/pwm.rs` | PWM tools (6) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/src/adc.rs` | ADC tools (2) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/src/onewire.rs` | 1-Wire tools (5) | each handler: `connect()` + `dev` |
| `crates/pico-de-gallo-mcp/README.md` | crate readme | rewrite the connection paragraph |
| `book/src/crates/mcp.md` | book chapter | rewrite the "Lazy connect" bullet |
| `crates/pico-de-gallo-mcp/CHANGELOG.md` | changelog | amend the unreleased 0.1.0 connection bullet |

`main.rs` is **unchanged**: `GalloMcp::new(cli.serial_number.as_deref())` still compiles (signature unchanged).

---

## Task 1: Refactor to per-call connection (one commit)

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs`
- Modify: `crates/pico-de-gallo-mcp/src/device.rs`
- Modify: `crates/pico-de-gallo-mcp/src/i2c.rs`
- Modify: `crates/pico-de-gallo-mcp/src/spi.rs`
- Modify: `crates/pico-de-gallo-mcp/src/uart.rs`
- Modify: `crates/pico-de-gallo-mcp/src/gpio.rs`
- Modify: `crates/pico-de-gallo-mcp/src/pwm.rs`
- Modify: `crates/pico-de-gallo-mcp/src/adc.rs`
- Modify: `crates/pico-de-gallo-mcp/src/onewire.rs`

- [ ] **Step 1: Rewrite `lib.rs` — struct, `new`, `connect`, `Device` guard; drop `ensure_validated`/`Arc`/`OnceCell`**

Replace the block from `use std::sync::Arc;` (line 20) through the closing `}` of `impl GalloMcp` (line 98) with:

```rust
use pico_de_gallo_lib::{DeviceInfo, PicoDeGallo};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool_handler};
use serde::Serialize;

/// Wrap a serializable value as a successful tool result.
pub(crate) fn ok_json<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
}

/// A live, validated connection to a Pico de Gallo device.
///
/// Constructed per tool call by [`GalloMcp::connect`]. Dereferences to the
/// underlying [`PicoDeGallo`] so tool handlers call device methods directly.
/// Dropping the guard drops the transport, releasing the USB interface claim
/// so other host processes (e.g. the `gallo` CLI) can use the board between
/// tool calls.
pub(crate) struct Device {
    inner: PicoDeGallo,
    info: DeviceInfo,
}

impl std::ops::Deref for Device {
    type Target = PicoDeGallo;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Device {
    /// The device info captured during validation on connect.
    pub(crate) fn info(&self) -> &DeviceInfo {
        &self.info
    }
}

/// The MCP service. Holds only the device selector; the USB connection is
/// opened per tool call by [`GalloMcp::connect`] and released when the call
/// completes, so the board is free for other host processes between calls.
#[derive(Clone)]
pub struct GalloMcp {
    serial_number: Option<String>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl GalloMcp {
    /// Construct the service, optionally pinned to a specific USB serial
    /// number. No USB connection is made here — each tool call opens and
    /// releases its own connection.
    pub fn new(serial_number: Option<&str>) -> Self {
        Self {
            serial_number: serial_number.map(str::to_string),
            tool_router: Self::device_router()
                + Self::i2c_router()
                + Self::spi_router()
                + Self::uart_router()
                + Self::gpio_router()
                + Self::pwm_router()
                + Self::adc_router()
                + Self::onewire_router(),
        }
    }

    /// Build the merged tool router without constructing a device.
    ///
    /// Test-only: lets registration tests assert the tool surface without
    /// touching USB hardware.
    #[cfg(test)]
    pub(crate) fn router_for_test() -> ToolRouter<Self> {
        Self::device_router()
            + Self::i2c_router()
            + Self::spi_router()
            + Self::uart_router()
            + Self::gpio_router()
            + Self::pwm_router()
            + Self::adc_router()
            + Self::onewire_router()
    }

    /// Open and validate a fresh connection to the target device.
    ///
    /// Constructs a new [`PicoDeGallo`], validates schema compatibility, and
    /// performs the connect-time subscription reset the host is expected to
    /// do. The returned [`Device`] owns the connection; dropping it releases
    /// the USB claim.
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

Leave the `pub mod ...;` lines (9-18) and the `#[tool_handler(router = self.tool_router)] impl ServerHandler for GalloMcp` block (100-109) unchanged.

- [ ] **Step 2: Rewrite the four `device.rs` handlers**

Edit `status` — replace:

```rust
        match self.ensure_validated().await {
            Ok(info) => ok_json(&StatusResult {
                attached: true,
                firmware_version: Some(format!(
                    "{}.{}.{}",
                    info.fw_major, info.fw_minor, info.fw_patch
                )),
                schema_major: Some(info.schema_major),
                schema_minor: Some(info.schema_minor),
            }),
            Err(_) => ok_json(&StatusResult {
                attached: false,
                firmware_version: None,
                schema_major: None,
                schema_minor: None,
            }),
        }
```

with:

```rust
        match self.connect().await {
            Ok(dev) => {
                let info = dev.info();
                ok_json(&StatusResult {
                    attached: true,
                    firmware_version: Some(format!(
                        "{}.{}.{}",
                        info.fw_major, info.fw_minor, info.fw_patch
                    )),
                    schema_major: Some(info.schema_major),
                    schema_minor: Some(info.schema_minor),
                })
            }
            Err(_) => ok_json(&StatusResult {
                attached: false,
                firmware_version: None,
                schema_major: None,
                schema_minor: None,
            }),
        }
```

Edit `device_info` — replace:

```rust
        let info = self.device.device_info().await.map_err(map_pdg_err)?;
        ok_json(&info)
```

with:

```rust
        let dev = self.connect().await?;
        let info = dev.device_info().await.map_err(map_pdg_err)?;
        ok_json(&info)
```

Edit `version` — replace:

```rust
        let v = self.device.version().await.map_err(map_pdg_err)?;
        ok_json(&v)
```

with:

```rust
        let dev = self.connect().await?;
        let v = dev.version().await.map_err(map_pdg_err)?;
        ok_json(&v)
```

Edit `ping` — replace:

```rust
        let echoed = self.device.ping(p.id).await.map_err(map_pdg_err)?;
        ok_json(&echoed)
```

with:

```rust
        let dev = self.connect().await?;
        let echoed = dev.ping(p.id).await.map_err(map_pdg_err)?;
        ok_json(&echoed)
```

- [ ] **Step 3: Rewrite the seven `i2c.rs` handlers**

`i2c_read` — replace:

```rust
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let data = self
            .device
            .i2c_read(addr, p.count)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
```

with:

```rust
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        let data = dev.i2c_read(addr, p.count).await.map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
```

`i2c_write` — replace:

```rust
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device
            .i2c_write(addr, &bytes)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.i2c_write(addr, &bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`i2c_write_read` — replace:

```rust
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let data = self
            .device
            .i2c_write_read(addr, &bytes, p.count)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
```

with:

```rust
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        let data = dev
            .i2c_write_read(addr, &bytes, p.count)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
```

`i2c_scan` — replace:

```rust
        let addrs = self
            .device
            .i2c_scan(p.include_reserved)
            .await
            .map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let addrs = dev
            .i2c_scan(p.include_reserved)
            .await
            .map_err(map_pdg_err)?;
```

`i2c_get_config` — replace:

```rust
        let f = self.device.i2c_get_config().await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let f = dev.i2c_get_config().await.map_err(map_pdg_err)?;
```

`i2c_set_config` — replace:

```rust
        self.device
            .i2c_set_config(freq)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.i2c_set_config(freq).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`i2c_batch` — replace:

```rust
        let out = self
            .device
            .i2c_batch(addr, &ops)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&out))
```

with:

```rust
        let dev = self.connect().await?;
        let out = dev.i2c_batch(addr, &ops).await.map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&out))
```

- [ ] **Step 4: Rewrite the seven `spi.rs` handlers**

`spi_read` — replace:

```rust
        let data = self.device.spi_read(p.count).await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let data = dev.spi_read(p.count).await.map_err(map_pdg_err)?;
```

`spi_write` — replace:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device.spi_write(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.spi_write(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`spi_transfer` — replace:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let data = self
            .device
            .spi_transfer(&bytes)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
```

with:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        let data = dev.spi_transfer(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
```

`spi_flush` — replace:

```rust
        self.device.spi_flush().await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.spi_flush().await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`spi_get_config` — replace:

```rust
        let c = self.device.spi_get_config().await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let c = dev.spi_get_config().await.map_err(map_pdg_err)?;
```

`spi_set_config` — replace:

```rust
        self.device
            .spi_set_config(p.frequency, phase, pol)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.spi_set_config(p.frequency, phase, pol)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`spi_batch` — replace:

```rust
        let out = self
            .device
            .spi_batch(p.cs, &ops)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&out))
```

with:

```rust
        let dev = self.connect().await?;
        let out = dev.spi_batch(p.cs, &ops).await.map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&out))
```

- [ ] **Step 5: Rewrite the five `uart.rs` handlers**

`uart_read` — replace:

```rust
        let data = self
            .device
            .uart_read(p.count, p.timeout_ms)
            .await
            .map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let data = dev
            .uart_read(p.count, p.timeout_ms)
            .await
            .map_err(map_pdg_err)?;
```

`uart_write` — replace:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device.uart_write(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.uart_write(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`uart_flush` — replace:

```rust
        self.device.uart_flush().await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.uart_flush().await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`uart_get_config` — replace:

```rust
        let c = self.device.uart_get_config().await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let c = dev.uart_get_config().await.map_err(map_pdg_err)?;
```

`uart_set_config` — replace:

```rust
        self.device
            .uart_set_config(p.baud_rate)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.uart_set_config(p.baud_rate).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

- [ ] **Step 6: Rewrite the six `gpio.rs` handlers**

`gpio_get` — replace:

```rust
        let state = self.device.gpio_get(p.pin).await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let state = dev.gpio_get(p.pin).await.map_err(map_pdg_err)?;
```

`gpio_put` — replace:

```rust
        self.device.gpio_put(p.pin, s).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.gpio_put(p.pin, s).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`gpio_set_config` — replace:

```rust
        self.device
            .gpio_set_config(p.pin, dir, pull)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.gpio_set_config(p.pin, dir, pull)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`gpio_wait_for_rising_edge_with_timeout` — replace:

```rust
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        self.device
            .gpio_wait_for_rising_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"edge")
```

with:

```rust
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.gpio_wait_for_rising_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"edge")
```

`gpio_wait_for_falling_edge_with_timeout` — replace:

```rust
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        self.device
            .gpio_wait_for_falling_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"edge")
```

with:

```rust
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.gpio_wait_for_falling_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"edge")
```

`gpio_wait_for_any_edge_with_timeout` — replace:

```rust
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        self.device
            .gpio_wait_for_any_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"edge")
```

with:

```rust
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.gpio_wait_for_any_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"edge")
```

- [ ] **Step 7: Rewrite the six `pwm.rs` handlers**

`pwm_get_duty_cycle` — replace:

```rust
        let d = self
            .device
            .pwm_get_duty_cycle(p.channel)
            .await
            .map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let d = dev
            .pwm_get_duty_cycle(p.channel)
            .await
            .map_err(map_pdg_err)?;
```

`pwm_get_config` — replace:

```rust
        let c = self
            .device
            .pwm_get_config(p.channel)
            .await
            .map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let c = dev
            .pwm_get_config(p.channel)
            .await
            .map_err(map_pdg_err)?;
```

`pwm_set_duty_cycle` — replace:

```rust
        self.device
            .pwm_set_duty_cycle(p.channel, p.duty)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.pwm_set_duty_cycle(p.channel, p.duty)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`pwm_enable` — replace:

```rust
        self.device
            .pwm_enable(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.pwm_enable(p.channel).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`pwm_disable` — replace:

```rust
        self.device
            .pwm_disable(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.pwm_disable(p.channel).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`pwm_set_config` — replace:

```rust
        self.device
            .pwm_set_config(p.channel, p.frequency, p.phase_correct)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let dev = self.connect().await?;
        dev.pwm_set_config(p.channel, p.frequency, p.phase_correct)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

- [ ] **Step 8: Rewrite the two `adc.rs` handlers**

`adc_read` — replace:

```rust
        let raw = self.device.adc_read(channel).await.map_err(map_pdg_err)?;
        ok_json(&serde_json::json!({ "raw": raw }))
```

with:

```rust
        let dev = self.connect().await?;
        let raw = dev.adc_read(channel).await.map_err(map_pdg_err)?;
        ok_json(&serde_json::json!({ "raw": raw }))
```

`adc_get_config` — replace:

```rust
        let c = self.device.adc_get_config().await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let c = dev.adc_get_config().await.map_err(map_pdg_err)?;
```

- [ ] **Step 9: Rewrite the five `onewire.rs` handlers**

`onewire_reset` — replace:

```rust
        let present = self.device.onewire_reset().await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let present = dev.onewire_reset().await.map_err(map_pdg_err)?;
```

`onewire_read` — replace:

```rust
        let data = self.device.onewire_read(p.len).await.map_err(map_pdg_err)?;
```

with:

```rust
        let dev = self.connect().await?;
        let data = dev.onewire_read(p.len).await.map_err(map_pdg_err)?;
```

`onewire_write` — replace:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device
            .onewire_write(&bytes)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.onewire_write(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`onewire_write_pullup` — replace:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device
            .onewire_write_pullup(&bytes, p.duration_ms)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

with:

```rust
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect().await?;
        dev.onewire_write_pullup(&bytes, p.duration_ms)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
```

`onewire_search` — replace:

```rust
        let rom = if p.continue_search {
            self.device
                .onewire_search_next()
                .await
                .map_err(map_pdg_err)?
        } else {
            self.device.onewire_search().await.map_err(map_pdg_err)?
        };
```

with:

```rust
        let dev = self.connect().await?;
        let rom = if p.continue_search {
            dev.onewire_search_next().await.map_err(map_pdg_err)?
        } else {
            dev.onewire_search().await.map_err(map_pdg_err)?
        };
```

- [ ] **Step 10: Normalize line endings**

Run (AGENTS.md §3):

```bash
dos2unix crates/pico-de-gallo-mcp/src/lib.rs crates/pico-de-gallo-mcp/src/device.rs crates/pico-de-gallo-mcp/src/i2c.rs crates/pico-de-gallo-mcp/src/spi.rs crates/pico-de-gallo-mcp/src/uart.rs crates/pico-de-gallo-mcp/src/gpio.rs crates/pico-de-gallo-mcp/src/pwm.rs crates/pico-de-gallo-mcp/src/adc.rs crates/pico-de-gallo-mcp/src/onewire.rs
```

Expected: `converting file ... to Unix format` or nothing if already LF.

- [ ] **Step 11: Format**

Run (working directory `crates/pico-de-gallo-mcp`):

```bash
cargo fmt
```

Then verify clean:

```bash
cargo fmt --check
```

Expected: no output (exit 0).

- [ ] **Step 12: Build (primary verification that the refactor is internally consistent)**

Run (working directory `crates/pico-de-gallo-mcp`):

```bash
cargo build --locked
```

Expected: `Finished` with no errors. If the compiler reports a leftover `self.device`, `self.info`, `ensure_validated`, `Arc`, or `OnceCell` reference, fix that handler/site and rebuild. There must be zero remaining references (grep to confirm):

```bash
grep -rn "self\.device\|self\.info\|ensure_validated\|OnceCell\|Arc<PicoDeGallo>" crates/pico-de-gallo-mcp/src/
```

Expected: no matches.

- [ ] **Step 13: Clippy (CI gate)**

Run (working directory `crates/pico-de-gallo-mcp`):

```bash
cargo clippy --all-targets --locked -- -D warnings
```

Expected: no warnings, exit 0. (Watch for an unused-import warning on `DeviceInfo`/`PicoDeGallo` — both are still used, by `Device`. If clippy flags `Arc`/`OnceCell` still imported, you missed removing an import in Step 1.)

- [ ] **Step 14: Tests (regression guard — existing hardware-free tests)**

Run (working directory `crates/pico-de-gallo-mcp`):

```bash
cargo test --locked
```

Expected: all tests pass. These are the per-module `*_tools_registered` router tests and param-deserialization tests. They exercise `router_for_test()` and `serde` and do **not** touch USB, so they must pass unchanged. If a `*_tools_registered` test fails, a `#[tool_router]` block was damaged during editing — compare the affected handler against the originals.

- [ ] **Step 15: Commit**

```bash
git add crates/pico-de-gallo-mcp/src/
git commit -m "fix(mcp): Open device per tool call, release between calls" -m "The MCP server constructed one PicoDeGallo in GalloMcp::new() and shared
it for the whole server lifetime. Its background nusb worker claimed the
USB interface until the process exited, so the board was busy forever and
agentic (MCP) and traditional (gallo CLI) workflows could not compose.

Replace the shared Arc<PicoDeGallo> + OnceCell<DeviceInfo> with a stored
serial_number and open a fresh connection per tool call via a new
GalloMcp::connect(), which validates schema compatibility and runs the
connect-time subscription reset, returning a Device RAII guard that
Derefs to PicoDeGallo. Each handler connects after argument validation
and operates through the guard; dropping it at handler end releases the
USB claim, freeing the board between calls.

Sequential stdio tool calls never overlap, so the Windows double-claim
ACCESS_DENIED race (AGENTS.md 13.17) does not apply. No wire-protocol,
CLI-surface, or tool-surface change." -m "Assisted-by: GitHub Copilot:claude-opus-4.8" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

Expected: one commit created, 9 files changed.

---

## Task 2: Documentation (second commit, same PR)

**Files:**
- Modify: `crates/pico-de-gallo-mcp/README.md`
- Modify: `book/src/crates/mcp.md`
- Modify: `crates/pico-de-gallo-mcp/CHANGELOG.md`

- [ ] **Step 1: Update `README.md`**

Replace:

```
The server connects to the device lazily: it starts even with no board
attached, and tools begin working as soon as a Pico de Gallo is plugged in.
Logs go to stderr; stdout carries the MCP protocol.
```

with:

```
The server holds no persistent connection to the board. Each tool call opens
the device, runs the operation, and releases it when the call completes, so
the board stays free for the `gallo` CLI or other host processes between
calls. The server starts even with no board attached; tools begin working as
soon as a Pico de Gallo is plugged in. Logs go to stderr; stdout carries the
MCP protocol.
```

- [ ] **Step 2: Update `book/src/crates/mcp.md`**

Replace the bullet:

```
- **Lazy connect:** the server starts even with no board attached and
  begins serving as soon as a Pico de Gallo is present. You can plug the
  board in mid-session; the tools work once it appears.
```

with:

```
- **Per-call connection:** the server holds no persistent USB claim. Each
  tool call opens the board, runs, and releases it when the call completes,
  so the device is free for the `gallo` CLI or other host processes between
  calls. The server starts even with no board attached and tools begin
  working as soon as a Pico de Gallo is present; you can plug the board in
  mid-session.
```

Leave the rest of the chapter unchanged: the GPIO-waits section (still bounded, still accurate), the `status`/`i2c_scan` validation examples (still accurate), and the tool catalog (unchanged).

- [ ] **Step 3: Update `CHANGELOG.md`**

The `0.1.0` section is unreleased (dated today, never published), so amend it in place — do not add a new version section and do not bump the version. Replace:

```
- Lazy device connection: the server starts with no board attached and begins
  serving as soon as one is present.
```

with:

```
- Per-call device connection: each tool opens the board, runs, and releases it
  when the call completes, so the board stays free for the `gallo` CLI and
  other host processes between calls. The server starts with no board attached
  and each tool begins working as soon as one is present.
```

- [ ] **Step 4: Normalize line endings**

```bash
dos2unix crates/pico-de-gallo-mcp/README.md book/src/crates/mcp.md crates/pico-de-gallo-mcp/CHANGELOG.md
```

- [ ] **Step 5: Build the book (parity gate, AGENTS.md §15.1 item 6)**

If `mdbook` is installed:

```bash
mdbook build book
```

Expected: builds with no broken-link/missing-file errors. (If `mdbook` is not installed locally, note that CI's `gh-pages.yml` build step will verify it on the PR.)

- [ ] **Step 6: Commit**

```bash
git add crates/pico-de-gallo-mcp/README.md book/src/crates/mcp.md crates/pico-de-gallo-mcp/CHANGELOG.md
git commit -m "docs(mcp): Document per-call device connection model" -m "Update the README, book chapter, and CHANGELOG to describe the per-call
open/close connection model: the server no longer holds a persistent USB
claim, so the board is free for the gallo CLI and other host processes
between tool calls. Documentation-only; pairs with the code change in the
same PR per the book-code parity rule (AGENTS.md 15.1)." -m "Assisted-by: GitHub Copilot:claude-opus-4.8" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

Expected: one commit created, 3 files changed.

---

## Final verification (before opening the PR)

- [ ] From `crates/pico-de-gallo-mcp`, re-run the full local gate:

```bash
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

Expected: fmt clean, clippy clean, all tests pass.

- [ ] Confirm no stray references remain:

```bash
grep -rn "self\.device\|self\.info\|ensure_validated\|OnceCell\|Arc<PicoDeGallo>" crates/pico-de-gallo-mcp/src/
```

Expected: no matches.

- [ ] Confirm the two commits are present and scoped correctly:

```bash
git --no-pager log --oneline -2
git --no-pager show --stat HEAD~1 HEAD | grep -E "files? changed|\.rs|\.md"
```

Expected: `fix(mcp): ...` (9 `.rs` files) then `docs(mcp): ...` (3 doc files).

- [ ] **Manual hardware check (not CI-able; from the design doc):** with a board attached, start the server, run a read tool (e.g. `i2c_scan`) and confirm it succeeds; while the server is idle between calls, run `gallo i2c scan` from a second shell and confirm the CLI now succeeds (previously blocked). On Windows specifically, confirm no `ACCESS_DENIED` panic across two successive tool calls.

---

## Self-review (completed by plan author)

- **Spec coverage:** per-call scope (Task 1 `connect`), validate-on-every-call (Task 1 `connect` body), `validate()` as connect gate (relies on lib's lazy connect; no extra readiness code — matches design), RAII Deref guard carrying `DeviceInfo` (Task 1 `Device`), board-tolerant tools all through `connect()` with `status` catching the error (Task 1 Step 2), docs parity (Task 2), no version bump / unpublished (Task 2 Step 3 + notes). All spec sections map to a task.
- **Placeholder scan:** none — every code step shows complete old/new text.
- **Type consistency:** `connect() -> Result<Device, ErrorData>`; `Device::info() -> &DeviceInfo`; handlers bind `let dev = self.connect().await?;` and call `dev.<method>()` via `Deref`. `GalloMcp::new(Option<&str>)` signature preserved, so `main.rs` is untouched. `Device` is `pub(crate)`; handlers never name it. Names used identically across all tasks.

---

# Revision 2 — robustness fixes (Tasks 3–4)

Review found (and source verified) two problems with the shipped Task 1/2 design; see the design doc "Revision 2". Task 1 already landed a shared-mutex serialization fix (in commit `3ac2560`). Tasks 3–4 add the fallible constructor + graceful/retry handling so no-board and reconnect-race paths return clean errors instead of panicking.

## Task 3: Fallible constructor in `pico-de-gallo-lib`

**Files:**
- Modify: `crates/pico-de-gallo-lib/src/lib.rs`
- Modify: `book/src/crates/lib.md`
- Modify: `crates/pico-de-gallo-lib/CHANGELOG.md`

- [ ] **Step 1: Add `try_new*` and route `new*` through them (`src/lib.rs`)**

Replace the `PicoDeGallo` doc comment block (lines 262-264):

```rust
/// Connection happens lazily in the background — constructing a `PicoDeGallo`
/// does not block or fail. If the device is not connected, methods will return
/// errors when called.
```

with:

```rust
/// The USB device is enumerated when the client is constructed: [`new`] and
/// [`new_with_serial_number`] **panic** if no matching device is present or
/// the interface cannot be claimed. Use the fallible [`try_new`] /
/// [`try_new_with_serial_number`] variants to handle those cases. Once
/// constructed, the connection handshake completes in the background, so
/// per-RPC calls fail (rather than the constructor) if the link drops later.
///
/// [`new`]: Self::new
/// [`new_with_serial_number`]: Self::new_with_serial_number
/// [`try_new`]: Self::try_new
/// [`try_new_with_serial_number`]: Self::try_new_with_serial_number
```

Then replace the `new_inner` helper (lines 300-303):

```rust
    fn new_inner<F: FnMut(&NusbDeviceInfo) -> bool>(func: F) -> Self {
        let client = HostClient::new_raw_nusb(func, ERROR_PATH, 8, VarSeqKind::Seq2);
        Self { client }
    }
```

with the fallible variants plus a panicking `new_inner` that delegates to them:

```rust
    /// Fallible variant of [`new`](Self::new): returns an error instead of
    /// panicking when no matching device is present or the interface cannot
    /// be claimed.
    pub fn try_new() -> Result<Self, String> {
        Self::try_new_inner(|dev| dev.vendor_id() == MICROSOFT_VID && dev.product_id() == PICO_DE_GALLO_PID)
    }

    /// Fallible variant of [`new_with_serial_number`](Self::new_with_serial_number).
    pub fn try_new_with_serial_number(serial_number: &str) -> Result<Self, String> {
        Self::try_new_inner(|dev| {
            dev.vendor_id() == MICROSOFT_VID
                && dev.product_id() == PICO_DE_GALLO_PID
                && dev.serial_number() == Some(serial_number)
        })
    }

    fn try_new_inner<F: FnMut(&NusbDeviceInfo) -> bool>(func: F) -> Result<Self, String> {
        let client = HostClient::try_new_raw_nusb(func, ERROR_PATH, 8, VarSeqKind::Seq2)?;
        Ok(Self { client })
    }

    fn new_inner<F: FnMut(&NusbDeviceInfo) -> bool>(func: F) -> Self {
        Self::try_new_inner(func).expect("should have found nusb device")
    }
```

(Keep `new()` and `new_with_serial_number()` exactly as they are — they call `new_inner` and preserve the existing panicking behavior for backward compatibility.)

- [ ] **Step 2: Build + clippy + test (`crates/pico-de-gallo-lib`)**

Run:

```bash
cargo build --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Expected: all green. No new unit test is added for `try_new()` itself — it is a thin wrapper over postcard-rpc's already-tested `try_new_raw_nusb`, and asserting `Err`/`Ok` would depend on whether a board happens to be attached to the test machine (flaky). The existing round-trip tests must still pass unchanged.

- [ ] **Step 3: Update `book/src/crates/lib.md`**

Replace (lines 13-16):

```
`PicoDeGallo::new()` and `PicoDeGallo::new_with_serial_number()` are **synchronous
constructors**. They do not perform an async handshake up front; the client
connects lazily in the background and operations fail only when you actually try
to use the device.
```

with:

```
`PicoDeGallo::new()` and `PicoDeGallo::new_with_serial_number()` are **synchronous
constructors**. They enumerate USB when called and **panic** if no matching board
is present or the interface cannot be claimed; the fallible `PicoDeGallo::try_new()`
and `PicoDeGallo::try_new_with_serial_number()` return a `Result<PicoDeGallo, String>`
instead. They do not perform an async handshake up front — once constructed, the
client completes the connection in the background and per-RPC calls (not the
constructor) fail if the link drops later.
```

Add two rows to the "Constructors and discovery" table (after the `new_with_serial_number` row, line 29):

```
| `PicoDeGallo::try_new()` | Fallible `new()` — `Err` instead of panic when absent/unclaimable |
| `PicoDeGallo::try_new_with_serial_number(serial)` | Fallible `new_with_serial_number()` |
```

- [ ] **Step 4: Update `crates/pico-de-gallo-lib/CHANGELOG.md`**

The `[0.7.0]` section is dated today. Add a second bullet under its existing `### Added` list (do **not** add a new version section, do **not** bump the version):

```
- Add fallible constructors `PicoDeGallo::try_new()` and
  `PicoDeGallo::try_new_with_serial_number()` returning
  `Result<PicoDeGallo, String>`. Unlike `new()` / `new_with_serial_number()`
  (which panic when no matching device is present or the interface cannot be
  claimed), these surface the error, letting callers report "no device
  attached" or retry a transient claim failure. Additive and non-breaking.
```

- [ ] **Step 5: Normalize + verify + commit**

```bash
dos2unix crates/pico-de-gallo-lib/src/lib.rs book/src/crates/lib.md crates/pico-de-gallo-lib/CHANGELOG.md
```

Verify from `crates/pico-de-gallo-lib`: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` all green; and `mdbook build book` from repo root is clean. Then:

```bash
git add crates/pico-de-gallo-lib/src/lib.rs book/src/crates/lib.md crates/pico-de-gallo-lib/CHANGELOG.md
git commit -m "feat(lib): Add fallible PicoDeGallo::try_new constructors" -m "new()/new_with_serial_number() call postcard-rpc's new_raw_nusb, which is
try_new_raw_nusb(...).expect(...): it enumerates USB at construction and
panics when no matching device is present or the interface claim fails. The
prior doc comment (\"constructing … does not block or fail\") was wrong.

Add try_new()/try_new_with_serial_number() returning Result<Self, String>
(thin wrappers over try_new_raw_nusb) so callers — notably gallo-mcp's
per-call connect() — can report \"no device attached\" or retry a transient
claim failure instead of panicking. new_inner now delegates to try_new_inner
().expect(...), preserving the existing panicking behavior. Correct the
PicoDeGallo doc and book/lib.md accordingly. Additive, non-breaking." -m "Assisted-by: GitHub Copilot:claude-opus-4.8" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

## Task 4: Graceful no-device + retry in `gallo-mcp::connect()`

**Files:**
- Modify: `crates/pico-de-gallo-mcp/Cargo.toml` (add `time` to tokio features)
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs` (retry/classify in `connect()`; fix doc comments)

- [ ] **Step 1: Add the `time` feature to tokio (`Cargo.toml`)**

Replace:

```toml
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "io-std", "signal"] }
```

with:

```toml
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "io-std", "signal", "time"] }
```

Then from repo root confirm no lock drift:

```bash
cargo check --locked -p gallo-mcp
```

Expected: builds. (Adding a feature to an existing dependency version does not change `Cargo.lock`; if `cargo` reports the lock is stale, run `cargo update -p tokio --precise 1.52.3` is unnecessary — instead `cargo check` without `--locked` once to refresh, then re-run with `--locked`. Commit `Cargo.lock` too only if it actually changed.)

- [ ] **Step 2: Rewrite `connect()` and the `Device`/`connect` doc comments (`src/lib.rs`)**

Add the `Duration` import near the top imports (after `use std::sync::Arc;`):

```rust
use std::time::Duration;
```

Replace the `Device` struct doc comment (the block starting `/// A live, validated connection` through the paragraph ending `tool calls.`) with an accurate version that does not claim a synchronous release ordering:

```rust
/// A live, validated connection to a Pico de Gallo device.
///
/// Constructed per tool call by [`GalloMcp::connect`]. Dereferences to the
/// underlying [`PicoDeGallo`] so tool handlers call device methods directly.
/// It holds the shared connection lock (`_claim`) for its whole lifetime, so
/// at most one connection exists at a time (see [`GalloMcp::connect`]).
///
/// Dropping the guard drops the transport and then releases the lock. The
/// transport tears down asynchronously (postcard-rpc owns the USB interface on
/// detached tasks and exposes no "released" signal), so the next [`connect`]
/// may briefly observe the interface still claimed; `connect` absorbs that with
/// a bounded retry. Between tool calls the board is free for other host
/// processes (e.g. the `gallo` CLI).
///
/// [`connect`]: GalloMcp::connect
```

Keep the `_claim` field and its inline comment as-is (the `inner`-before-`_claim` ordering still matters to start teardown before freeing the lock).

Replace the entire `connect()` method (its doc comment and body) with:

```rust
    /// Open and validate a fresh connection to the target device.
    ///
    /// Serializes device access with the shared lock (rmcp dispatches each tool
    /// call on its own `tokio::spawn` task, so handlers can run concurrently),
    /// constructs the [`PicoDeGallo`] with the fallible `try_new*`, validates
    /// schema compatibility, and runs the connect-time subscription reset. The
    /// returned [`Device`] owns the connection and the lock.
    ///
    /// If no matching board is present, returns a clean "no device attached"
    /// error (so `status` can report `attached: false`). If the interface claim
    /// fails transiently — e.g. the previous connection's asynchronous teardown
    /// has not released the exclusive USB claim yet, the Windows double-claim
    /// hazard in AGENTS.md §13.17 — retries a few times with a short backoff
    /// before giving up.
    pub(crate) async fn connect(&self) -> Result<Device, ErrorData> {
        /// Substring postcard-rpc uses when USB enumeration finds no match.
        const NOT_FOUND: &str = "Failed to find matching nusb device";
        /// Total attempts to claim the interface before giving up.
        const MAX_ATTEMPTS: u32 = 5;
        /// Backoff between claim attempts (absorbs async release window).
        const BACKOFF: Duration = Duration::from_millis(100);

        let claim = self.connection.clone().lock_owned().await;

        let mut attempt: u32 = 1;
        let inner = loop {
            let result = match self.serial_number.as_deref() {
                Some(sn) => PicoDeGallo::try_new_with_serial_number(sn),
                None => PicoDeGallo::try_new(),
            };
            match result {
                Ok(dev) => break dev,
                Err(e) if e.contains(NOT_FOUND) => {
                    return Err(ErrorData::internal_error(
                        "no device attached: connect a Pico de Gallo and retry".to_string(),
                        None,
                    ));
                }
                Err(e) if attempt >= MAX_ATTEMPTS => {
                    return Err(ErrorData::internal_error(
                        format!("failed to open device after {attempt} attempts: {e}"),
                        None,
                    ));
                }
                Err(_) => {
                    attempt += 1;
                    tokio::time::sleep(BACKOFF).await;
                }
            }
        };
        let info = inner.validate().await.map_err(error::map_validate_err)?;
        let _ = inner.system_reset_subscriptions().await;
        Ok(Device {
            inner,
            info,
            _claim: claim,
        })
    }
```

- [ ] **Step 3: Add a unit test for the not-found classification (`src/lib.rs`)**

The retry loop's only hardware-free, deterministic piece is the `NOT_FOUND` substring check against the exact string postcard-rpc returns. Add a test module at the end of `lib.rs` (there is none today) that pins this contract:

```rust
#[cfg(test)]
mod tests {
    /// The exact error text postcard-rpc's `try_new_raw_nusb` returns when
    /// enumeration finds no match (postcard-rpc 0.12.1, raw_nusb.rs). If this
    /// literal changes upstream, `connect()`'s not-found classification must
    /// be updated in lockstep.
    #[test]
    fn not_found_substring_matches_postcard_error() {
        let postcard_err = "Failed to find matching nusb device!";
        assert!(postcard_err.contains("Failed to find matching nusb device"));
    }
}
```

- [ ] **Step 4: Normalize + full verify (`crates/pico-de-gallo-mcp`)**

```bash
dos2unix crates/pico-de-gallo-mcp/src/lib.rs crates/pico-de-gallo-mcp/Cargo.toml
```

From `crates/pico-de-gallo-mcp`: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` (now 33 tests: the new not-found test) all green.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-mcp/Cargo.toml crates/pico-de-gallo-mcp/src/lib.rs
git commit -m "fix(mcp): Handle missing board and claim races in connect" -m "PicoDeGallo::new() panics when no board is present or the USB claim fails,
so the per-call connect() could panic instead of returning a clean error —
defeating status's attached:false path and the \"starts even with no board\"
promise — and the previous connection's asynchronous USB release could make
a fresh claim transiently fail on Windows (AGENTS.md 13.17) even with the
serialization mutex.

connect() now uses the fallible PicoDeGallo::try_new*(): a \"Failed to find
matching nusb device\" error maps to a clean \"no device attached\" result,
and other (claim) failures are retried up to 5 times with a 100ms backoff to
absorb the async release window. Adds the tokio time feature for sleep and a
test pinning the not-found substring contract. Corrects the Device/connect
doc comments that over-claimed a synchronous release ordering." -m "Assisted-by: GitHub Copilot:claude-opus-4.8" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

## Revision 2 self-review

- **Coverage:** fallible constructor (Task 3 Step 1), lib doc + book parity (Task 3 Steps 1,3), lib CHANGELOG (Step 4), graceful no-device (Task 4 Step 2 not-found arm), retry/backoff for the async-release race (Task 4 Step 2 loop), tokio `time` (Step 1), corrected over-claiming doc comments (Task 4 Step 2), test for the string contract (Step 3). Both review blockers addressed.
- **Type consistency:** `try_new*() -> Result<Self, String>` in lib; `connect()` matches on `String` via `.contains(NOT_FOUND)`; return type `Result<Device, ErrorData>` unchanged, so all handlers are untouched. `Device` gains no new field beyond the `_claim` already added in Task 1.
- **No version bumps:** lib change is additive (methods added; `new*` behavior preserved); gallo-mcp stays `0.1.0`. tokio feature add does not change `Cargo.lock` versions.
