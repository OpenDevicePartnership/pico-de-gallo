//! SPI tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes};
use crate::error::{invalid_arg, map_pdg_err, map_validate_err};
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiReadParams {
    /// Number of bytes to read.
    pub count: u16,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiWriteParams {
    /// Bytes to write, as a hex string.
    pub data: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SpiTransferParams {
    /// Bytes to clock out (full-duplex); an equal number is read back.
    pub data: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
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
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
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
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

/// Parse every write/transfer payload into an owned buffer.
///
/// Deliberately device-free and called **before** [`GalloMcp::connect`]: a
/// malformed hex string is a purely local argument error, and `connect`
/// runs `system_reset_subscriptions`, which tears down every GPIO
/// subscription on the board — including ones owned by other host
/// processes. A typo in a request must not have that cross-process side
/// effect.
fn parse_batch_payloads(ops: &[SpiBatchOpParam]) -> Result<Vec<Vec<u8>>, ErrorData> {
    let mut bufs: Vec<Vec<u8>> = Vec::new();
    for op in ops {
        match op {
            SpiBatchOpParam::Write { data } | SpiBatchOpParam::Transfer { data } => {
                bufs.push(parse_bytes(data).map_err(invalid_arg)?);
            }
            SpiBatchOpParam::Read { .. } | SpiBatchOpParam::Delay { .. } => {}
        }
    }
    Ok(bufs)
}

/// Classify a chip-select index against the device-reported GPIO count.
///
/// `num_gpios` comes from the [`DeviceInfo`](pico_de_gallo_lib::DeviceInfo)
/// captured when the connection was validated, so it is always a value the
/// device actually reported. A failure to establish it aborts in `connect`
/// and is mapped by [`map_validate_err`] to an internal error (-32603) —
/// never to invalid params (-32602). See issue #104.
fn classify_cs(cs: u8, num_gpios: u8) -> Result<(), ErrorData> {
    if num_gpios == 0 {
        return Err(invalid_arg(
            "device reports num_gpios=0; no SPI chip-select pin is available",
        ));
    }
    if cs >= num_gpios {
        return Err(invalid_arg(format!(
            "invalid SPI chip-select pin {cs}; device reports {num_gpios} \
             GPIOs (valid 0..{num_gpios})"
        )));
    }
    Ok(())
}

/// Map a [`SpiBatchCallError`](pico_de_gallo_lib::SpiBatchCallError) to an
/// [`ErrorData`].
///
/// Argument faults (-32602) and host/transport faults (-32603) stay
/// disjoint: a metadata failure keeps the `ValidateError` classification
/// supplied by [`map_validate_err`] and can never become an invalid-params
/// complaint about the caller's chip-select.
///
/// Exhaustive on purpose: appending a variant must break this build.
fn map_spi_batch_call_err(e: pico_de_gallo_lib::SpiBatchCallError) -> ErrorData {
    use pico_de_gallo_lib::SpiBatchCallError as E;
    match e {
        E::DeviceInfo(v) => map_validate_err(v),
        E::NoGpios => {
            invalid_arg("device reports num_gpios=0; no SPI chip-select pin is available")
        }
        E::InvalidCsPin { cs, num_gpios } => invalid_arg(format!(
            "invalid SPI chip-select pin {cs}; device reports {num_gpios} \
             GPIOs (valid 0..{num_gpios})"
        )),
        E::Comms(c) => ErrorData::internal_error(format!("communication error: {c:?}"), None),
        // Internal, not invalid-params, and the message must say the batch may
        // already have run: a caller that retries blindly repeats its writes.
        E::Timeout { waited } => ErrorData::internal_error(
            format!(
                "device did not respond to spi/batch within {:.3} s; the batch \
                 may or may not have executed, so do not retry blindly",
                waited.as_secs_f64()
            ),
            None,
        ),
        E::Endpoint(be) => ErrorData::invalid_params(format!("device error: {be}"), None),
    }
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
    ///
    /// Order is fixed and load-bearing: parse every payload, connect
    /// exactly once, read the GPIO count from the `DeviceInfo` that
    /// connection already validated, classify the chip-select, only then
    /// build the borrowed operations and call the library once.
    ///
    /// Parsing precedes `connect` because `connect` tears down every GPIO
    /// subscription on the board; a malformed request must not do that.
    /// Classification precedes the library call because a refused
    /// chip-select must drive no pin (issue #104). No second metadata query
    /// is issued.
    #[tool(
        description = "Execute a batch of SPI operations under chip-select",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_batch(
        &self,
        Parameters(p): Parameters<SpiBatchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::SpiBatchOp;

        // 1. Parse all write/transfer payloads into owned buffers first (the
        //    ops borrow &[u8]), with no device access.
        let bufs = parse_batch_payloads(&p.ops)?;

        // 2. Connect exactly once. Do NOT hoist this above the parse: see
        //    `parse_batch_payloads`.
        let dev = self.connect(p.serial_number.as_deref()).await?;

        // 3. Read the retained, already-validated count; 4. classify.
        classify_cs(p.cs, dev.info().num_gpios)?;

        // 5. Build the borrowed operations.
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

        // 6. One library call.
        let out = dev
            .spi_batch(p.cs, &ops)
            .await
            .map_err(map_spi_batch_call_err)?;
        ok_device_json(&dev, &Bytes::from_slice(&out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_params_deserialize() {
        let p: SpiTransferParams =
            serde_json::from_str(r#"{"data":"0x01,0x02","serial_number":"ABC123"}"#).unwrap();
        assert_eq!(p.data, "0x01,0x02");
        assert_eq!(p.serial_number.as_deref(), Some("ABC123"));
    }

    #[test]
    fn batch_params_deserialize() {
        let p: SpiBatchParams = serde_json::from_str(
            r#"{"cs":0,"ops":[{"op":"write","data":"0x9F"},{"op":"read","count":3},{"op":"delay","ns":1000}],"serial_number":"ABC123"}"#,
        )
        .unwrap();
        assert_eq!(p.cs, 0);
        assert_eq!(p.ops.len(), 3);
        assert_eq!(p.serial_number.as_deref(), Some("ABC123"));
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

    // ===================================================================
    // M3 — SPI chip-select bounds (issue #104)
    // ===================================================================

    use pico_de_gallo_lib::{SpiBatchCallError, ValidateError};

    const INVALID_PARAMS: i32 = -32602;
    const INTERNAL_ERROR: i32 = -32603;

    fn code(e: &ErrorData) -> i32 {
        e.code.0
    }

    #[test]
    fn batch_params_accept_max_u8_cs() {
        // 255 must reach the runtime bound check, not be rejected by the
        // schema, so the surfaced message names the real device count.
        let p: SpiBatchParams =
            serde_json::from_str(r#"{"cs":255,"ops":[{"op":"read","count":1}]}"#).unwrap();
        assert_eq!(p.cs, 255);
    }

    #[test]
    fn batch_params_reject_cs_above_u8() {
        let r: Result<SpiBatchParams, _> =
            serde_json::from_str(r#"{"cs":256,"ops":[{"op":"read","count":1}]}"#);
        assert!(r.is_err());
    }

    #[test]
    fn parse_batch_payloads_rejects_malformed_hex_as_invalid_params() {
        let ops = vec![SpiBatchOpParam::Write {
            data: "0xZZ".to_string(),
        }];
        let e = parse_batch_payloads(&ops).expect_err("0xZZ is not hex");
        assert_eq!(code(&e), INVALID_PARAMS);
    }

    #[tokio::test]
    async fn spi_batch_malformed_payload_does_not_report_a_device_error() {
        // Amendment A1, as a regression guard. With no board attached,
        // `connect()` fails with a device-selection error. If the handler
        // connected before parsing, that is the error we would see. Getting
        // the parse diagnostic instead proves parsing ran first — and
        // therefore that a malformed request cannot trigger
        // `system_reset_subscriptions`, which would tear down GPIO
        // subscriptions owned by other host processes.
        //
        // NOTE: this is a behavioural discriminator, not a call counter. It
        // proves `connect()` did not *complete* first. On a machine with a
        // board attached the discriminating signal disappears and this
        // becomes a false pass; a true counter would need a seam in
        // `src/lib.rs`, which is outside M3's locked file inventory.
        let mcp = crate::GalloMcp::new(None);
        let p = SpiBatchParams {
            cs: 0,
            ops: vec![SpiBatchOpParam::Write {
                data: "0xZZ".to_string(),
            }],
            serial_number: None,
        };
        let e = mcp
            .spi_batch(Parameters(p))
            .await
            .expect_err("malformed hex must fail");
        assert_eq!(code(&e), INVALID_PARAMS, "message was: {}", e.message);
        let msg = e.message.to_lowercase();
        assert!(
            !msg.contains("stopped responding"),
            "connected first: {}",
            e.message
        );
        assert!(!msg.contains("serial"), "connected first: {}", e.message);
        assert!(
            !msg.contains("no pico de gallo"),
            "connected first: {}",
            e.message
        );
    }

    #[test]
    fn classify_cs_out_of_range_is_invalid_params() {
        for (cs, n) in [(4u8, 4u8), (7, 7), (255, 4)] {
            let e = classify_cs(cs, n).expect_err("out of range");
            assert_eq!(code(&e), INVALID_PARAMS);
            assert!(
                e.message.contains(&cs.to_string()),
                "message must echo the caller's index verbatim: {}",
                e.message
            );
        }
    }

    #[test]
    fn classify_cs_zero_bound_is_invalid_params_with_distinct_message() {
        let zero = classify_cs(0, 0).expect_err("no pin is valid at n = 0");
        assert_eq!(code(&zero), INVALID_PARAMS);
        assert!(
            zero.message.contains("num_gpios=0"),
            "got: {}",
            zero.message
        );

        let range = classify_cs(4, 4).expect_err("out of range");
        assert_ne!(zero.message, range.message);
    }

    #[test]
    fn classify_cs_accepts_boundaries() {
        for (cs, n) in [(0u8, 4u8), (3, 4), (6, 7)] {
            classify_cs(cs, n).unwrap_or_else(|e| panic!("cs {cs} at n {n}: {}", e.message));
        }
    }

    #[test]
    fn map_validate_err_timeout_is_internal_error() {
        let e = map_validate_err(ValidateError::Timeout);
        assert_eq!(code(&e), INTERNAL_ERROR);
        assert!(e.message.contains("device/info"), "got: {}", e.message);
    }

    #[test]
    fn map_validate_err_never_produces_invalid_params() {
        // The binding constraint at the MCP boundary: a failure to learn the
        // valid chip-select range is a host/transport fault, never a
        // complaint about the caller's arguments.
        let variants = [
            ValidateError::Comms(pico_de_gallo_lib::host_client::HostErr::Closed),
            ValidateError::Timeout,
            ValidateError::LegacyFirmware,
            ValidateError::SchemaMismatch {
                expected_major: 0,
                actual_major: 0,
                expected_minor: 7,
                actual_minor: 6,
            },
        ];
        for v in variants {
            let e = map_validate_err(v);
            assert_eq!(code(&e), INTERNAL_ERROR, "message was: {}", e.message);
        }
    }

    #[test]
    fn spi_batch_call_error_mapping_is_exhaustive_and_disjoint() {
        // Compile-time witness plus the code assignment per variant.
        fn witness(e: &SpiBatchCallError) -> u8 {
            match e {
                SpiBatchCallError::DeviceInfo(_) => 0,
                SpiBatchCallError::NoGpios => 1,
                SpiBatchCallError::InvalidCsPin { .. } => 2,
                SpiBatchCallError::Comms(_) => 3,
                SpiBatchCallError::Endpoint(_) => 4,
                SpiBatchCallError::Timeout { .. } => 5,
            }
        }
        let cases = vec![
            (
                SpiBatchCallError::DeviceInfo(ValidateError::Timeout),
                INTERNAL_ERROR,
            ),
            (SpiBatchCallError::NoGpios, INVALID_PARAMS),
            (
                SpiBatchCallError::InvalidCsPin {
                    cs: 255,
                    num_gpios: 4,
                },
                INVALID_PARAMS,
            ),
            (
                SpiBatchCallError::Comms(pico_de_gallo_lib::host_client::HostErr::Closed),
                INTERNAL_ERROR,
            ),
            (
                SpiBatchCallError::Endpoint(pico_de_gallo_lib::SpiBatchError {
                    failed_op: 0,
                    kind: pico_de_gallo_lib::SpiError::Other,
                }),
                INVALID_PARAMS,
            ),
        ];
        let tags: std::collections::HashSet<u8> = cases.iter().map(|(e, _)| witness(e)).collect();
        assert_eq!(tags.len(), 5, "one case per variant is required");
        for (e, want) in cases {
            let mapped = map_spi_batch_call_err(e);
            assert_eq!(code(&mapped), want, "message was: {}", mapped.message);
        }
    }
}
