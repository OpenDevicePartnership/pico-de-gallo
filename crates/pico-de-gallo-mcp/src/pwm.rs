//! PWM tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::error::map_pdg_err;
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PwmChannelParams {
    /// PWM channel.
    pub channel: u8,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PwmSetDutyParams {
    /// PWM channel.
    pub channel: u8,
    /// Raw compare value.
    pub duty: u16,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PwmSetConfigParams {
    /// PWM channel.
    pub channel: u8,
    /// Frequency in Hz.
    pub frequency: u32,
    /// Enable phase-correct mode.
    #[serde(default)]
    pub phase_correct: bool,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[tool_router(router = pwm_router, vis = "pub(crate)")]
impl GalloMcp {
    /// Get PWM duty cycle and max.
    #[tool(
        description = "Get PWM duty cycle and max",
        annotations(read_only_hint = true)
    )]
    async fn pwm_get_duty_cycle(
        &self,
        Parameters(p): Parameters<PwmChannelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let d = dev
            .pwm_get_duty_cycle(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{d:?}"))
    }

    /// Get PWM configuration.
    #[tool(
        description = "Get PWM configuration",
        annotations(read_only_hint = true)
    )]
    async fn pwm_get_config(
        &self,
        Parameters(p): Parameters<PwmChannelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.pwm_get_config(p.channel).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
    }

    /// Set PWM raw duty cycle.
    #[tool(
        description = "Set PWM raw duty cycle",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn pwm_set_duty_cycle(
        &self,
        Parameters(p): Parameters<PwmSetDutyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.pwm_set_duty_cycle(p.channel, p.duty)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Enable a PWM channel.
    #[tool(
        description = "Enable a PWM channel",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn pwm_enable(
        &self,
        Parameters(p): Parameters<PwmChannelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.pwm_enable(p.channel).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Disable a PWM channel.
    #[tool(
        description = "Disable a PWM channel",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn pwm_disable(
        &self,
        Parameters(p): Parameters<PwmChannelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.pwm_disable(p.channel).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Configure PWM frequency and phase-correct.
    #[tool(
        description = "Configure PWM frequency and phase-correct",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn pwm_set_config(
        &self,
        Parameters(p): Parameters<PwmSetConfigParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.pwm_set_config(p.channel, p.frequency, p.phase_correct)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_config_params_deserialize() {
        let legacy: PwmSetConfigParams =
            serde_json::from_str(r#"{"channel":0,"frequency":1000,"phase_correct":false}"#)
                .unwrap();
        assert_eq!(legacy.serial_number, None);

        let p: PwmSetConfigParams = serde_json::from_str(
            r#"{"channel":0,"frequency":1000,"phase_correct":false,"serial_number":"ABC123"}"#,
        )
        .unwrap();
        assert_eq!(p.channel, 0);
        assert_eq!(p.frequency, 1000);
        assert!(!p.phase_correct);
        assert_eq!(p.serial_number.as_deref(), Some("ABC123"));
    }

    #[test]
    fn pwm_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in [
            "pwm_get_duty_cycle",
            "pwm_get_config",
            "pwm_set_duty_cycle",
            "pwm_enable",
            "pwm_disable",
            "pwm_set_config",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn channel_params_accept_an_optional_serial_number() {
        let without: PwmChannelParams = serde_json::from_str(r#"{"channel":0}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: PwmChannelParams =
            serde_json::from_str(r#"{"channel":0,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
}
