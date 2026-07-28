//! PWM tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::error::map_pdg_err;
use crate::{GalloMcp, ok_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PwmChannelParams {
    /// PWM channel.
    pub channel: u8,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PwmSetDutyParams {
    /// PWM channel.
    pub channel: u8,
    /// Raw compare value.
    pub duty: u16,
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
        let d = self
            .device
            .pwm_get_duty_cycle(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&format!("{d:?}"))
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
        let c = self
            .device
            .pwm_get_config(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&format!("{c:?}"))
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
        self.device
            .pwm_set_duty_cycle(p.channel, p.duty)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
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
        self.device
            .pwm_enable(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
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
        self.device
            .pwm_disable(p.channel)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
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
        self.device
            .pwm_set_config(p.channel, p.frequency, p.phase_correct)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_config_params_deserialize() {
        let p: PwmSetConfigParams =
            serde_json::from_str(r#"{"channel":0,"frequency":1000,"phase_correct":false}"#)
                .unwrap();
        assert_eq!(p.channel, 0);
        assert_eq!(p.frequency, 1000);
        assert!(!p.phase_correct);
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
}
