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

/// A live, validated connection to a Pico de Gallo device.
///
/// Constructed per tool call by [`GalloMcp::connect`]. Dereferences to the
/// underlying [`PicoDeGallo`] so tool handlers call device methods directly.
/// It also holds the shared connection lock (`_claim`) for its whole lifetime,
/// so at most one connection exists at a time (see [`GalloMcp::connect`]).
///
/// Field order matters: `inner` is declared before `_claim` so that on drop
/// the transport is torn down — releasing the USB interface claim — *before*
/// the lock is released to the next waiting tool call. Dropping the guard thus
/// frees the board for other host processes (e.g. the `gallo` CLI) between
/// tool calls.
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
    /// Acquires the shared connection lock, constructs a new [`PicoDeGallo`],
    /// validates schema compatibility, and performs the connect-time
    /// subscription reset the host is expected to do. The returned [`Device`]
    /// owns the connection and the lock; dropping it releases the USB claim,
    /// then the lock.
    ///
    /// The lock is required for correctness, not just fairness. rmcp dispatches
    /// each tool call on its own spawned task (`tokio::spawn`; this crate does
    /// not enable rmcp's `local` feature), so tool handlers can run
    /// concurrently — e.g. when an agent issues parallel tool calls. The
    /// Pico de Gallo USB interface is an exclusive claim, so two concurrent
    /// `connect`s would race and the second would fail with `ACCESS_DENIED` on
    /// Windows (the WinUSB double-claim hazard in AGENTS.md §13.17). Holding
    /// the lock for the whole connection serializes device access: concurrent
    /// calls queue instead of racing, while the board is still released to
    /// other host processes between calls.
    pub(crate) async fn connect(&self) -> Result<Device, ErrorData> {
        let claim = self.connection.clone().lock_owned().await;
        let inner = match self.serial_number.as_deref() {
            Some(sn) => PicoDeGallo::new_with_serial_number(sn),
            None => PicoDeGallo::new(),
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
