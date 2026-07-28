//! SPI tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes};
use crate::error::{invalid_arg, map_pdg_err};
use crate::{GalloMcp, ok_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiReadParams {
    /// Number of bytes to read.
    pub count: u16,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiWriteParams {
    /// Bytes to write, as a hex string.
    pub data: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiTransferParams {
    /// Bytes to clock out (full-duplex); an equal number is read back.
    pub data: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiSetConfigParams {
    /// Clock frequency in Hz.
    pub frequency: u32,
    /// Sample on the first clock transition (CPHA=0) when true.
    #[serde(default)]
    pub first_transition: bool,
    /// Idle-low clock (CPOL=0) when true.
    #[serde(default)]
    pub idle_low: bool,
}

#[tool_router(router = spi_router, vis = "pub(crate)")]
impl GalloMcp {
    /// Read bytes from SPI.
    #[tool(
        description = "Read bytes from SPI",
        annotations(read_only_hint = true)
    )]
    async fn spi_read(
        &self,
        Parameters(p): Parameters<SpiReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let data = self.device.spi_read(p.count).await.map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
    }
    /// Write bytes to SPI.
    #[tool(
        description = "Write bytes to SPI",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_write(
        &self,
        Parameters(p): Parameters<SpiWriteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device.spi_write(&bytes).await.map_err(map_pdg_err)?;
        ok_json(&"ok")
    }
    /// Full-duplex SPI transfer.
    #[tool(
        description = "Full-duplex SPI transfer",
        annotations(read_only_hint = true)
    )]
    async fn spi_transfer(
        &self,
        Parameters(p): Parameters<SpiTransferParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let data = self
            .device
            .spi_transfer(&bytes)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
    }
    /// Flush the SPI TX buffer.
    #[tool(
        description = "Flush the SPI TX buffer",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_flush(&self) -> Result<CallToolResult, ErrorData> {
        self.device.spi_flush().await.map_err(map_pdg_err)?;
        ok_json(&"ok")
    }
    /// Get the current SPI configuration.
    #[tool(
        description = "Get the current SPI configuration",
        annotations(read_only_hint = true)
    )]
    async fn spi_get_config(&self) -> Result<CallToolResult, ErrorData> {
        let c = self.device.spi_get_config().await.map_err(map_pdg_err)?;
        ok_json(&format!("{c:?}"))
    }
    /// Set SPI frequency/phase/polarity.
    #[tool(
        description = "Set SPI frequency/phase/polarity",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_set_config(
        &self,
        Parameters(p): Parameters<SpiSetConfigParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::{SpiPhase, SpiPolarity};
        let phase = if p.first_transition {
            SpiPhase::CaptureOnFirstTransition
        } else {
            SpiPhase::CaptureOnSecondTransition
        };
        let pol = if p.idle_low {
            SpiPolarity::IdleLow
        } else {
            SpiPolarity::IdleHigh
        };
        self.device
            .spi_set_config(p.frequency, phase, pol)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_params_deserialize() {
        let p: SpiTransferParams = serde_json::from_str(r#"{"data":"0x01,0x02"}"#).unwrap();
        assert_eq!(p.data, "0x01,0x02");
    }
    #[tokio::test]
    async fn spi_tools_registered() {
        let svc = crate::GalloMcp::new(None);
        let names: Vec<String> = svc
            .tool_router
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in [
            "spi_read",
            "spi_write",
            "spi_transfer",
            "spi_flush",
            "spi_get_config",
            "spi_set_config",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }
}
