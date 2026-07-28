//! `gallo-mcp` — an MCP server exposing a Pico de Gallo device to AI agents.
//!
//! Wraps [`pico_de_gallo_lib::PicoDeGallo`] and presents one MCP tool per
//! peripheral operation over stdio. Bytes cross the tool boundary as hex
//! strings (in) and [`encoding::Bytes`] (out). Write/actuation tools are
//! annotated `destructive_hint = true`; approval is delegated to the MCP
//! client.

pub mod adc;
pub mod device;
pub mod encoding;
pub mod error;
pub mod gpio;
pub mod i2c;
pub mod onewire;
pub mod pwm;
pub mod spi;
pub mod uart;

use std::sync::Arc;
use std::time::Duration;

use pico_de_gallo_lib::{DeviceInfo, PicoDeGallo};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool_handler};
use serde::Serialize;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Wrap a serializable value as a successful tool result.
pub(crate) fn ok_json<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
}

/// Substring of the error postcard-rpc's `try_new_raw_nusb` returns when USB
/// enumeration finds no matching device (postcard-rpc 0.12.1, raw_nusb.rs).
/// [`GalloMcp::connect`] classifies on it to distinguish "no board attached"
/// from a transient claim failure.
const NOT_FOUND: &str = "Failed to find matching nusb device";

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
pub(crate) struct Device {
    inner: PicoDeGallo,
    info: DeviceInfo,
    /// Serializes device access across concurrent tool calls; held for the
    /// lifetime of the connection. Released (after `inner`) when this drops.
    _claim: OwnedMutexGuard<()>,
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

/// The MCP service. Holds only the device selector and a connection lock; the
/// USB connection is opened per tool call by [`GalloMcp::connect`] and released
/// when the call completes, so the board is free for other host processes
/// between calls.
#[derive(Clone)]
pub struct GalloMcp {
    serial_number: Option<String>,
    /// Serializes device access. rmcp dispatches each tool call on its own
    /// task, so handlers run concurrently; this lock ensures at most one live
    /// [`Device`] (USB claim) at a time. Shared across handler clones.
    connection: Arc<Mutex<()>>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl GalloMcp {
    /// Construct the service, optionally pinned to a specific USB serial
    /// number. No USB connection is made here — each tool call opens and
    /// releases its own connection.
    pub fn new(serial_number: Option<&str>) -> Self {
        Self {
            serial_number: serial_number.map(str::to_string),
            connection: Arc::new(Mutex::new(())),
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
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for GalloMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Bridge to a Pico de Gallo USB device (I2C/SPI/UART/GPIO/PWM/ADC/1-Wire). \
                 Bytes are hex strings like \"0x48,0x00\". Read tools are safe; tools that \
                 write or actuate pins are marked destructive and may require approval.",
        )
    }
}

#[cfg(test)]
mod tests {
    /// A sampled copy of the exact error text postcard-rpc's `try_new_raw_nusb`
    /// returns when enumeration finds no match (postcard-rpc 0.12.1,
    /// raw_nusb.rs). Ties [`crate::NOT_FOUND`] — the substring `connect()`
    /// classifies on — to that literal, so editing the const in isolation
    /// (breaking the match) fails this test. It cannot detect an upstream
    /// change to the postcard-rpc message itself.
    #[test]
    fn not_found_substring_matches_postcard_error() {
        let postcard_err = "Failed to find matching nusb device!";
        assert!(postcard_err.contains(crate::NOT_FOUND));
    }
}
