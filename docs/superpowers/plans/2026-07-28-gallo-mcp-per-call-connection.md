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
