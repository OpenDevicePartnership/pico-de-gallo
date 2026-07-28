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
use tokio::sync::OnceCell;

/// Wrap a serializable value as a successful tool result.
pub(crate) fn ok_json<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
}

/// The MCP service. Holds one lazily-connected, cheaply-cloneable device
/// handle shared across all tool calls.
#[derive(Clone)]
pub struct GalloMcp {
    pub(crate) device: Arc<PicoDeGallo>,
    /// Cached device info, populated once by the first `validate()`.
    pub(crate) info: Arc<OnceCell<DeviceInfo>>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl GalloMcp {
    /// Construct the service for the first matching device, or a specific
    /// serial number if provided. Connection is lazy — no USB claim happens
    /// here.
    pub fn new(serial_number: Option<&str>) -> Self {
        let device = match serial_number {
            Some(sn) => PicoDeGallo::new_with_serial_number(sn),
            None => PicoDeGallo::new(),
        };
        Self {
            device: Arc::new(device),
            info: Arc::new(OnceCell::new()),
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
    /// touching USB hardware (avoids the exclusive WinUSB claim race).
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

    /// Validate schema compatibility once and cache the result. Also performs
    /// the connect-time subscription reset the host is expected to do.
    pub(crate) async fn ensure_validated(&self) -> Result<&DeviceInfo, ErrorData> {
        self.info
            .get_or_try_init(|| async {
                let info = self
                    .device
                    .validate()
                    .await
                    .map_err(error::map_validate_err)?;
                let _ = self.device.system_reset_subscriptions().await;
                Ok(info)
            })
            .await
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
