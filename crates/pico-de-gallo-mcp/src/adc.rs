//! ADC tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::validate_adc_channel;
use crate::error::{invalid_arg, map_pdg_err};
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AdcReadParams {
    /// ADC channel index (0..=3).
    pub channel: u8,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[tool_router(router = adc_router, vis = "pub(crate)")]
impl GalloMcp {
    /// Read a single ADC sample.
    #[tool(
        description = "Read a single ADC sample",
        annotations(read_only_hint = true)
    )]
    async fn adc_read(
        &self,
        Parameters(p): Parameters<AdcReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::AdcChannel;
        let ch = validate_adc_channel(p.channel).map_err(invalid_arg)?;
        let channel = match ch {
            0 => AdcChannel::Adc0,
            1 => AdcChannel::Adc1,
            2 => AdcChannel::Adc2,
            _ => AdcChannel::Adc3,
        };
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let raw = dev.adc_read(channel).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &serde_json::json!({ "raw": raw }))
    }

    /// Get ADC capabilities.
    #[tool(
        description = "Get ADC capabilities",
        annotations(read_only_hint = true)
    )]
    async fn adc_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.adc_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_params_deserialize() {
        let p: AdcReadParams = serde_json::from_str(r#"{"channel":2}"#).unwrap();
        assert_eq!(p.channel, 2);
    }

    #[test]
    fn adc_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in ["adc_read", "adc_get_config"] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: AdcReadParams = serde_json::from_str(r#"{"channel":2}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: AdcReadParams =
            serde_json::from_str(r#"{"channel":2,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
}
