//! SPI tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes};
use crate::error::{invalid_arg, map_pdg_err};
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiReadParams {
    /// Number of bytes to read.
    pub count: u16,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiWriteParams {
    /// Bytes to write, as a hex string.
    pub data: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiTransferParams {
    /// Bytes to clock out (full-duplex); an equal number is read back.
    pub data: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
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
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum SpiBatchOpParam {
    /// Read `count` bytes.
    Read { count: u16 },
    /// Write `data` (hex string).
    Write { data: String },
    /// Full-duplex transfer of `data` (hex string).
    Transfer { data: String },
    /// Delay for `ns` nanoseconds.
    Delay { ns: u32 },
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiBatchParams {
    /// Chip-select pin.
    pub cs: u8,
    /// Ordered list of operations.
    pub ops: Vec<SpiBatchOpParam>,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
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
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let data = dev.spi_read(p.count).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&data))
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
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.spi_write(&bytes).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
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
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let data = dev.spi_transfer(&bytes).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&data))
    }
    /// Flush the SPI TX buffer.
    #[tool(
        description = "Flush the SPI TX buffer",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_flush(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.spi_flush().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }
    /// Get the current SPI configuration.
    #[tool(
        description = "Get the current SPI configuration",
        annotations(read_only_hint = true)
    )]
    async fn spi_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.spi_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
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
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.spi_set_config(p.frequency, phase, pol)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Execute a batch of SPI operations under chip-select.
    #[tool(
        description = "Execute a batch of SPI operations under chip-select",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_batch(
        &self,
        Parameters(p): Parameters<SpiBatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::SpiBatchOp;
        // Parse all write/transfer payloads into owned buffers first (the ops
        // borrow &[u8]).
        let mut bufs: Vec<Vec<u8>> = Vec::new();
        for op in &p.ops {
            match op {
                SpiBatchOpParam::Write { data } | SpiBatchOpParam::Transfer { data } => {
                    bufs.push(parse_bytes(data).map_err(invalid_arg)?);
                }
                _ => {}
            }
        }
        let mut ops: Vec<SpiBatchOp<'_>> = Vec::with_capacity(p.ops.len());
        let mut b = 0usize;
        for op in &p.ops {
            match op {
                SpiBatchOpParam::Read { count } => ops.push(SpiBatchOp::Read { len: *count }),
                SpiBatchOpParam::Write { .. } => {
                    ops.push(SpiBatchOp::Write { data: &bufs[b] });
                    b += 1;
                }
                SpiBatchOpParam::Transfer { .. } => {
                    ops.push(SpiBatchOp::Transfer { data: &bufs[b] });
                    b += 1;
                }
                SpiBatchOpParam::Delay { ns } => ops.push(SpiBatchOp::DelayNs { ns: *ns }),
            }
        }
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let out = dev.spi_batch(p.cs, &ops).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&out))
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
    #[test]
    fn batch_params_deserialize() {
        let p: SpiBatchParams = serde_json::from_str(
            r#"{"cs":0,"ops":[{"op":"write","data":"0x9F"},{"op":"read","count":3},{"op":"delay","ns":1000}]}"#,
        )
        .unwrap();
        assert_eq!(p.cs, 0);
        assert_eq!(p.ops.len(), 3);
    }
    #[test]
    fn spi_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
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
            "spi_batch",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: SpiReadParams = serde_json::from_str(r#"{"count":4}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: SpiReadParams =
            serde_json::from_str(r#"{"count":4,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
}
