//! I2C tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes, validate_i2c_address};
use crate::error::{invalid_arg, map_pdg_err};
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct I2cReadParams {
    /// 7-bit I2C address (0..=0x7F).
    pub address: u8,
    /// Number of bytes to read.
    pub count: u16,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct I2cWriteParams {
    /// 7-bit I2C address (0..=0x7F).
    pub address: u8,
    /// Bytes as a hex string, e.g. "0x00,0x10" or "0010".
    pub data: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct I2cWriteReadParams {
    /// 7-bit I2C address (0..=0x7F).
    pub address: u8,
    /// Bytes to write first, as a hex string.
    pub data: String,
    /// Number of bytes to read back.
    pub count: u16,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct I2cScanParams {
    /// Include reserved addresses in the scan.
    #[serde(default)]
    pub include_reserved: bool,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct I2cSetConfigParams {
    /// Bus frequency: "standard" (100k), "fast" (400k), or "fast-plus" (1M).
    pub frequency: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum I2cBatchOpParam {
    /// Read `count` bytes.
    Read { count: u16 },
    /// Write `data` (hex string).
    Write { data: String },
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct I2cBatchParams {
    /// 7-bit I2C address.
    pub address: u8,
    /// Ordered list of operations.
    pub ops: Vec<I2cBatchOpParam>,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[tool_router(router = i2c_router, vis = "pub(crate)")]
impl GalloMcp {
    /// Read bytes from an I2C device.
    #[tool(
        description = "Read bytes from an I2C device",
        annotations(read_only_hint = true)
    )]
    async fn i2c_read(
        &self,
        Parameters(p): Parameters<I2cReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let data = dev.i2c_read(addr, p.count).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&data))
    }

    /// Write bytes to an I2C device.
    #[tool(
        description = "Write bytes to an I2C device",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn i2c_write(
        &self,
        Parameters(p): Parameters<I2cWriteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.i2c_write(addr, &bytes).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Write then read on an I2C device.
    #[tool(
        description = "Write then read on an I2C device",
        annotations(read_only_hint = true)
    )]
    async fn i2c_write_read(
        &self,
        Parameters(p): Parameters<I2cWriteReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let data = dev
            .i2c_write_read(addr, &bytes, p.count)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&data))
    }

    /// Scan the I2C bus for responding addresses.
    #[tool(
        description = "Scan the I2C bus for responding addresses",
        annotations(read_only_hint = true)
    )]
    async fn i2c_scan(
        &self,
        Parameters(p): Parameters<I2cScanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let addrs = dev
            .i2c_scan(p.include_reserved)
            .await
            .map_err(map_pdg_err)?;
        let hex: Vec<String> = addrs.iter().map(|a| format!("0x{a:02X}")).collect();
        ok_device_json(&dev, &serde_json::json!({ "addresses": hex, "raw": addrs }))
    }

    /// Get the current I2C frequency.
    #[tool(
        description = "Get the current I2C frequency",
        annotations(read_only_hint = true)
    )]
    async fn i2c_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let f = dev.i2c_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{f:?}"))
    }

    /// Set the I2C frequency.
    #[tool(
        description = "Set the I2C frequency (standard|fast|fast-plus)",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn i2c_set_config(
        &self,
        Parameters(p): Parameters<I2cSetConfigParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::I2cFrequency;
        let freq = match p.frequency.to_lowercase().as_str() {
            "standard" => I2cFrequency::Standard,
            "fast" => I2cFrequency::Fast,
            "fast-plus" | "fastplus" => I2cFrequency::FastPlus,
            other => return Err(invalid_arg(format!("unknown frequency '{other}'"))),
        };
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.i2c_set_config(freq).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Execute a batch of I2C operations under one address.
    #[tool(
        description = "Execute a batch of I2C operations under one address",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn i2c_batch(
        &self,
        Parameters(p): Parameters<I2cBatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::I2cBatchOp;
        let addr = validate_i2c_address(p.address).map_err(invalid_arg)?;
        // Parse all write payloads into owned buffers FIRST so the borrowed
        // ops below can reference them (I2cBatchOp::Write borrows &[u8]).
        let mut write_bufs: Vec<Vec<u8>> = Vec::new();
        for op in &p.ops {
            if let I2cBatchOpParam::Write { data } = op {
                write_bufs.push(parse_bytes(data).map_err(invalid_arg)?);
            }
        }
        let mut ops: Vec<I2cBatchOp<'_>> = Vec::with_capacity(p.ops.len());
        let mut w = 0usize;
        for op in &p.ops {
            match op {
                I2cBatchOpParam::Read { count } => ops.push(I2cBatchOp::Read { len: *count }),
                I2cBatchOpParam::Write { .. } => {
                    ops.push(I2cBatchOp::Write {
                        data: &write_bufs[w],
                    });
                    w += 1;
                }
            }
        }
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let out = dev.i2c_batch(addr, &ops).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_params_deserialize() {
        let p: I2cReadParams = serde_json::from_str(r#"{"address":72,"count":2}"#).unwrap();
        assert_eq!(p.address, 72);
        assert_eq!(p.count, 2);
    }

    #[test]
    fn write_params_accept_hex_bytes() {
        let p: I2cWriteParams =
            serde_json::from_str(r#"{"address":80,"data":"0x00,0x10"}"#).unwrap();
        assert_eq!(p.address, 80);
        assert_eq!(p.data, "0x00,0x10");
    }

    #[test]
    fn batch_params_deserialize() {
        let p: I2cBatchParams = serde_json::from_str(
            r#"{"address":80,"ops":[{"op":"write","data":"0x00,0x10"},{"op":"read","count":16}]}"#,
        )
        .unwrap();
        assert_eq!(p.address, 80);
        assert_eq!(p.ops.len(), 2);
    }

    #[test]
    fn i2c_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in [
            "i2c_read",
            "i2c_write",
            "i2c_write_read",
            "i2c_scan",
            "i2c_set_config",
            "i2c_get_config",
            "i2c_batch",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: I2cReadParams = serde_json::from_str(r#"{"address":72,"count":2}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: I2cReadParams =
            serde_json::from_str(r#"{"address":72,"count":2,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
}
