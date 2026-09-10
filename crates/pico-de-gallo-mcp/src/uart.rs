//! UART tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes, validate_read_count, validate_write_payload};
use crate::error::{invalid_arg, map_pdg_err};
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
use pico_de_gallo_lib::{UartConfigurationInfo, UartDataBits, UartParity, UartStopBits};

/// Parses the tool's `data_bits` argument.
///
/// Extracted from the tool body so the accepted set is testable without a
/// board — the tool function itself calls `connect()` first. Same
/// extracted-policy shape as the #136 / #158 host guards.
///
/// `None` selects the 8N1 power-on value.
pub(crate) fn parse_data_bits(v: Option<&str>) -> Result<UartDataBits, ErrorData> {
    match v.unwrap_or("8") {
        "5" => Ok(UartDataBits::Five),
        "6" => Ok(UartDataBits::Six),
        "7" => Ok(UartDataBits::Seven),
        "8" => Ok(UartDataBits::Eight),
        other => Err(invalid_arg(format!(
            "invalid data_bits {other:?}; expected \"5\", \"6\", \"7\" or \"8\" \
             (the RP2350 has no 9-bit mode)"
        ))),
    }
}

/// Parses the tool's `parity` argument.
///
/// Extracted for the same reason as [`parse_data_bits`]. `None` selects the
/// 8N1 power-on value.
pub(crate) fn parse_parity(v: Option<&str>) -> Result<UartParity, ErrorData> {
    match v.unwrap_or("none") {
        "none" => Ok(UartParity::None),
        "odd" => Ok(UartParity::Odd),
        "even" => Ok(UartParity::Even),
        "mark" => Ok(UartParity::Mark),
        "space" => Ok(UartParity::Space),
        other => Err(invalid_arg(format!(
            "invalid parity {other:?}; expected \"none\", \"odd\", \"even\", \
             \"mark\" or \"space\""
        ))),
    }
}

/// Parses the tool's `stop_bits` argument.
///
/// Extracted for the same reason as [`parse_data_bits`]. `None` selects the
/// 8N1 power-on value.
pub(crate) fn parse_stop_bits(v: Option<&str>) -> Result<UartStopBits, ErrorData> {
    match v.unwrap_or("1") {
        "1" => Ok(UartStopBits::One),
        "2" => Ok(UartStopBits::Two),
        other => Err(invalid_arg(format!(
            "invalid stop_bits {other:?}; expected \"1\" or \"2\" \
             (the hardware has no half stop bits)"
        ))),
    }
}

/// The UART configuration payload, before the `{serial_number, result}`
/// envelope is applied.
///
/// The framing spellings are exactly those [`parse_data_bits`],
/// [`parse_parity`] and [`parse_stop_bits`] accept, so a `uart_get_config`
/// response can be fed straight back into `uart_set_config`.
pub(crate) fn uart_config_json(c: &UartConfigurationInfo) -> serde_json::Value {
    serde_json::json!({
        "baud_rate": c.baud_rate,
        "data_bits": match c.data_bits {
            UartDataBits::Five => "5",
            UartDataBits::Six => "6",
            UartDataBits::Seven => "7",
            UartDataBits::Eight => "8",
        },
        "parity": match c.parity {
            UartParity::None => "none",
            UartParity::Odd => "odd",
            UartParity::Even => "even",
            UartParity::Mark => "mark",
            UartParity::Space => "space",
        },
        "stop_bits": match c.stop_bits {
            UartStopBits::One => "1",
            UartStopBits::Two => "2",
        },
    })
}

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
    /// Data bits per character: "5", "6", "7" or "8". Omitting this selects
    /// the 8N1 default of "8", overwriting any previously configured word
    /// length. The RP2350 has no 9-bit mode.
    #[serde(default)]
    pub data_bits: Option<String>,
    /// Parity: "none", "odd", "even", "mark" or "space". Omitting this
    /// selects the 8N1 default of "none", overwriting any previously
    /// configured parity.
    #[serde(default)]
    pub parity: Option<String>,
    /// Stop bits: "1" or "2". Omitting this selects the 8N1 default of "1",
    /// overwriting any previously configured stop-bit count. Half stop bits
    /// are not supported by the hardware.
    #[serde(default)]
    pub stop_bits: Option<String>,
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
        validate_read_count(p.count).map_err(invalid_arg)?;
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
        validate_write_payload(&bytes).map_err(invalid_arg)?;
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
        description = "Return the last successfully requested UART baud rate, data bits, parity and \
                       stop bits, in the same spellings uart_set_config accepts.\n\n\
                       These are firmware software-shadow values, not a register read-back: the \
                       reported baud rate is what was asked for, not the rate the divisor achieves \
                       after rounding.",
        annotations(read_only_hint = true)
    )]
    async fn uart_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.uart_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &uart_config_json(&c))
    }

    /// Set the UART baud rate and framing.
    #[tool(
        description = "Replace the complete UART configuration: baud rate and framing.\n\n\
                       There is no partial update. Omitting a framing field selects its 8N1 \
                       power-on value and overwrites whatever framing was previously configured, \
                       so a baud-only change must repeat the framing fields. The applied \
                       configuration is returned.\n\n\
                       Reconfiguration is not atomic at the UART pins: the device applies the baud \
                       divisor before the framing and drains neither direction. Ensure transmit \
                       and receive traffic are quiescent while reconfiguring.",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn uart_set_config(
        &self,
        Parameters(p): Parameters<UartSetConfigParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Parse before connecting: an invalid argument must never open a
        // board, and the refusal must name the valid set so an agent can
        // self-correct rather than retry.
        let data_bits = parse_data_bits(p.data_bits.as_deref())?;
        let parity = parse_parity(p.parity.as_deref())?;
        let stop_bits = parse_stop_bits(p.stop_bits.as_deref())?;

        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.uart_set_config(p.baud_rate, data_bits, parity, stop_bits)
            .await
            .map_err(map_pdg_err)?;

        // Report the applied configuration rather than a bare "ok", so the
        // 8N1 default an omitted field selects is observable instead of
        // silently overwriting the previous framing.
        ok_device_json(
            &dev,
            &uart_config_json(&UartConfigurationInfo {
                baud_rate: p.baud_rate,
                data_bits,
                parity,
                stop_bits,
            }),
        )
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

    // --- UART framing arguments (issue #152) ---

    #[test]
    fn set_config_params_deserialize_framing() {
        let p: UartSetConfigParams = serde_json::from_str(
            r#"{"baud_rate":9600,"data_bits":"7","parity":"even","stop_bits":"2"}"#,
        )
        .unwrap();
        assert_eq!(p.baud_rate, 9600);
        assert_eq!(p.data_bits.as_deref(), Some("7"));
        assert_eq!(p.parity.as_deref(), Some("even"));
        assert_eq!(p.stop_bits.as_deref(), Some("2"));
    }

    #[test]
    fn set_config_params_framing_is_optional() {
        let p: UartSetConfigParams = serde_json::from_str(r#"{"baud_rate":115200}"#).unwrap();
        assert!(p.data_bits.is_none());
        assert!(p.parity.is_none());
        assert!(p.stop_bits.is_none());
    }

    #[test]
    fn parse_framing_accepts_every_representable_value() {
        assert_eq!(parse_data_bits(Some("5")).unwrap(), UartDataBits::Five);
        assert_eq!(parse_data_bits(Some("6")).unwrap(), UartDataBits::Six);
        assert_eq!(parse_data_bits(Some("7")).unwrap(), UartDataBits::Seven);
        assert_eq!(parse_data_bits(Some("8")).unwrap(), UartDataBits::Eight);

        assert_eq!(parse_parity(Some("none")).unwrap(), UartParity::None);
        assert_eq!(parse_parity(Some("odd")).unwrap(), UartParity::Odd);
        assert_eq!(parse_parity(Some("even")).unwrap(), UartParity::Even);
        assert_eq!(parse_parity(Some("mark")).unwrap(), UartParity::Mark);
        assert_eq!(parse_parity(Some("space")).unwrap(), UartParity::Space);

        assert_eq!(parse_stop_bits(Some("1")).unwrap(), UartStopBits::One);
        assert_eq!(parse_stop_bits(Some("2")).unwrap(), UartStopBits::Two);
    }

    /// Omitted fields select their 8N1 power-on value, which **overwrites**
    /// whatever framing was previously configured. `uart_set_config` replaces
    /// the complete configuration; all four parameters are applied together on
    /// the wire and there is no partial update. The overwrite is not silent:
    /// the tool returns the applied configuration, so an agent that changed
    /// only the baud rate can see the framing it also just set.
    #[test]
    fn parse_framing_defaults_to_8n1() {
        assert_eq!(parse_data_bits(None).unwrap(), UartDataBits::Eight);
        assert_eq!(parse_parity(None).unwrap(), UartParity::None);
        assert_eq!(parse_stop_bits(None).unwrap(), UartStopBits::One);
    }

    /// An unknown value must be refused locally, before `connect()`, with a
    /// message naming the valid set — so an agent can self-correct instead
    /// of retrying against a device error that does not say what it should
    /// have sent.
    #[test]
    fn parse_framing_refuses_unknown_values_naming_the_valid_set() {
        let e = parse_data_bits(Some("x")).unwrap_err();
        let m = e.message.to_string();
        for needle in ["\"5\"", "\"6\"", "\"7\"", "\"8\""] {
            assert!(m.contains(needle), "data_bits error omits {needle}: {m}");
        }

        let e = parse_parity(Some("x")).unwrap_err();
        let m = e.message.to_string();
        for needle in ["none", "odd", "even", "mark", "space"] {
            assert!(m.contains(needle), "parity error omits {needle}: {m}");
        }

        let e = parse_stop_bits(Some("x")).unwrap_err();
        let m = e.message.to_string();
        for needle in ["\"1\"", "\"2\""] {
            assert!(m.contains(needle), "stop_bits error omits {needle}: {m}");
        }
    }

    /// "9" is the value an agent is most likely to try, because 9-bit UART
    /// exists on other MCUs. The refusal must say *why*, or the agent will
    /// assume a transport problem and retry.
    #[test]
    fn parse_data_bits_refuses_nine_and_explains_why() {
        let e = parse_data_bits(Some("9")).unwrap_err();
        let m = e.message.to_string();
        assert!(
            m.contains("9-bit") || m.contains("9 bit"),
            "the refusal must name the missing 9-bit mode: {m}"
        );
        assert!(
            m.contains("RP2350") || m.contains("hardware"),
            "the refusal must attribute the limit to the hardware: {m}"
        );
    }

    #[test]
    fn parse_stop_bits_refuses_halves_and_explains_why() {
        for v in ["0.5", "1.5", "3", "0"] {
            let e = parse_stop_bits(Some(v)).unwrap_err();
            assert!(e.message.contains("stop_bits"), "{v} refusal is unlabelled");
        }
    }

    /// String matching is case-sensitive. Pinned so it cannot drift: an
    /// `eq_ignore_ascii_case` added later widens the accepted surface
    /// without failing any test above, and then a `get`→`set` round trip
    /// could start succeeding for spellings `get` never emits.
    ///
    /// If this fails, decide deliberately and apply the same rule to all
    /// three parsers. Do not just invert the assertion.
    #[test]
    fn parse_framing_matching_is_case_sensitive() {
        for v in ["None", "NONE", "Even", "Mark"] {
            assert!(
                parse_parity(Some(v)).is_err(),
                "parity {v:?} parsed; matching is no longer case-sensitive"
            );
        }
    }

    /// The response body must use field names and spellings that
    /// `uart_set_config` accepts, so `get` → `set` is a working round
    /// trip. This is a real behaviour change: the tool previously returned
    /// `format!("{c:?}")` debug text, which no `set` call can consume.
    #[test]
    fn uart_config_json_round_trips_through_the_set_config_parsers() {
        for data_bits in [
            UartDataBits::Five,
            UartDataBits::Six,
            UartDataBits::Seven,
            UartDataBits::Eight,
        ] {
            for parity in [
                UartParity::None,
                UartParity::Odd,
                UartParity::Even,
                UartParity::Mark,
                UartParity::Space,
            ] {
                for stop_bits in [UartStopBits::One, UartStopBits::Two] {
                    let info = UartConfigurationInfo {
                        baud_rate: 115_200,
                        data_bits,
                        parity,
                        stop_bits,
                    };
                    let body = uart_config_json(&info);

                    assert_eq!(body["baud_rate"], 115_200);

                    let d = body["data_bits"]
                        .as_str()
                        .expect("data_bits must be a string");
                    let p = body["parity"].as_str().expect("parity must be a string");
                    let s = body["stop_bits"]
                        .as_str()
                        .expect("stop_bits must be a string");

                    assert_eq!(parse_data_bits(Some(d)).unwrap(), data_bits);
                    assert_eq!(parse_parity(Some(p)).unwrap(), parity);
                    assert_eq!(parse_stop_bits(Some(s)).unwrap(), stop_bits);
                }
            }
        }
    }

    /// The exact field names are the tool contract an agent reads. Pin
    /// them: a rename to `dataBits` would keep the round-trip test above
    /// green while breaking every prompt that names the field.
    #[test]
    fn uart_config_json_field_names_are_pinned() {
        let body = uart_config_json(&UartConfigurationInfo {
            baud_rate: 9600,
            data_bits: UartDataBits::Seven,
            parity: UartParity::Even,
            stop_bits: UartStopBits::Two,
        });
        assert_eq!(
            body,
            serde_json::json!({
                "baud_rate": 9600,
                "data_bits": "7",
                "parity": "even",
                "stop_bits": "2",
            })
        );
    }

    /// Every device response carries `{serial_number, result}` so the
    /// board that answered is observable on every call — see the
    /// 2026-07-29 row in AGENTS.md §13.17, where two servers silently
    /// bound to the same board and returned byte-identical payloads.
    /// `ok_device_json` needs a `Device`, so the envelope shape is pinned
    /// directly here.
    #[test]
    fn uart_get_config_body_sits_inside_the_device_envelope() {
        let body = uart_config_json(&UartConfigurationInfo {
            baud_rate: 9600,
            data_bits: UartDataBits::Eight,
            parity: UartParity::None,
            stop_bits: UartStopBits::One,
        });
        let enveloped = serde_json::to_value(crate::Envelope {
            serial_number: Some("49742081C885AC69"),
            result: &body,
        })
        .unwrap();

        assert_eq!(enveloped["serial_number"], "49742081C885AC69");
        assert_eq!(enveloped["result"]["data_bits"], "8");
        assert_eq!(enveloped["result"]["baud_rate"], 9600);
    }
}
