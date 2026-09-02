//! UART tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes};
use crate::error::{invalid_arg, map_pdg_err};
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UartReadParams {
    /// Number of bytes to read.
    pub count: u16,
    /// Read timeout in milliseconds. Zero selects a 1 ms non-blocking poll;
    /// non-zero values above the firmware's 30-minute ceiling are clamped.
    pub timeout_ms: u32,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UartWriteParams {
    /// Bytes to write, as a hex string.
    pub data: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UartSetConfigParams {
    /// Baud rate in bits per second.
    pub baud_rate: u32,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[tool_router(router = uart_router, vis = "pub(crate)")]
impl GalloMcp {
    /// Read bytes from UART with a timeout.
    #[tool(
        description = "Read bytes from UART with a timeout",
        annotations(read_only_hint = true)
    )]
    async fn uart_read(
        &self,
        Parameters(p): Parameters<UartReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let data = dev
            .uart_read(p.count, p.timeout_ms)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&data))
    }

    /// Write bytes to UART.
    #[tool(
        description = "Write bytes to UART",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn uart_write(
        &self,
        Parameters(p): Parameters<UartWriteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.uart_write(&bytes).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Flush the UART TX buffer.
    #[tool(
        description = "Flush the UART TX buffer",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn uart_flush(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.uart_flush().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Get the current UART configuration.
    #[tool(
        description = "Get the current UART configuration",
        annotations(read_only_hint = true)
    )]
    async fn uart_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.uart_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
    }

    /// Set the UART baud rate.
    #[tool(
        description = "Set the UART baud rate",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn uart_set_config(
        &self,
        Parameters(p): Parameters<UartSetConfigParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.uart_set_config(p.baud_rate)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_params_deserialize() {
        let p: UartReadParams = serde_json::from_str(r#"{"count":4,"timeout_ms":1000}"#).unwrap();
        assert_eq!(p.count, 4);
        assert_eq!(p.timeout_ms, 1000);
    }

    #[test]
    fn uart_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in [
            "uart_read",
            "uart_write",
            "uart_flush",
            "uart_get_config",
            "uart_set_config",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: UartReadParams =
            serde_json::from_str(r#"{"count":4,"timeout_ms":1000}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: UartReadParams =
            serde_json::from_str(r#"{"count":4,"timeout_ms":1000,"serial_number":"ABC123"}"#)
                .unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
}
