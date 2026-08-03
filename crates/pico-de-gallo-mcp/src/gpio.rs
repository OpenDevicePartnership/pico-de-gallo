//! GPIO tools (timeout-bounded edge waits only; no subscriptions).

use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::validate_timeout_ms;
use crate::error::{invalid_arg, map_pdg_err};
use crate::{GalloMcp, ok_device_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GpioGetParams {
    /// GPIO pin number.
    pub pin: u8,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GpioPutParams {
    /// GPIO pin number.
    pub pin: u8,
    /// Drive the pin high when true, low when false.
    pub high: bool,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GpioSetConfigParams {
    /// GPIO pin number.
    pub pin: u8,
    /// "input" or "output".
    pub direction: String,
    /// "none", "up", or "down".
    #[serde(default = "default_pull")]
    pub pull: String,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

fn default_pull() -> String {
    "none".to_string()
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GpioWaitParams {
    /// GPIO pin number.
    pub pin: u8,
    /// Timeout in milliseconds (must be non-zero).
    pub timeout_ms: u32,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[tool_router(router = gpio_router, vis = "pub(crate)")]
impl GalloMcp {
    /// Read a GPIO pin level.
    #[tool(
        description = "Read a GPIO pin level",
        annotations(read_only_hint = true)
    )]
    async fn gpio_get(
        &self,
        Parameters(p): Parameters<GpioGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let state = dev.gpio_get(p.pin).await.map_err(map_pdg_err)?;
        ok_device_json(
            &dev,
            &serde_json::json!({ "high": matches!(state, pico_de_gallo_lib::GpioState::High) }),
        )
    }

    /// Set a GPIO pin level.
    #[tool(
        description = "Set a GPIO pin level",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn gpio_put(
        &self,
        Parameters(p): Parameters<GpioPutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::GpioState;
        let s = if p.high {
            GpioState::High
        } else {
            GpioState::Low
        };
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.gpio_put(p.pin, s).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Configure a GPIO pin direction and pull.
    #[tool(
        description = "Configure a GPIO pin direction and pull",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn gpio_set_config(
        &self,
        Parameters(p): Parameters<GpioSetConfigParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use pico_de_gallo_lib::{GpioDirection, GpioPull};
        let dir = match p.direction.to_lowercase().as_str() {
            "input" => GpioDirection::Input,
            "output" => GpioDirection::Output,
            o => return Err(invalid_arg(format!("unknown direction '{o}'"))),
        };
        let pull = match p.pull.to_lowercase().as_str() {
            "none" => GpioPull::None,
            "up" => GpioPull::Up,
            "down" => GpioPull::Down,
            o => return Err(invalid_arg(format!("unknown pull '{o}'"))),
        };
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.gpio_set_config(p.pin, dir, pull)
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Wait for a rising edge with a timeout.
    #[tool(
        description = "Wait for a rising edge with a timeout",
        annotations(read_only_hint = true)
    )]
    async fn gpio_wait_for_rising_edge_with_timeout(
        &self,
        Parameters(p): Parameters<GpioWaitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.gpio_wait_for_rising_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"edge")
    }

    /// Wait for a falling edge with a timeout.
    #[tool(
        description = "Wait for a falling edge with a timeout",
        annotations(read_only_hint = true)
    )]
    async fn gpio_wait_for_falling_edge_with_timeout(
        &self,
        Parameters(p): Parameters<GpioWaitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.gpio_wait_for_falling_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"edge")
    }

    /// Wait for any edge with a timeout.
    #[tool(
        description = "Wait for any edge with a timeout",
        annotations(read_only_hint = true)
    )]
    async fn gpio_wait_for_any_edge_with_timeout(
        &self,
        Parameters(p): Parameters<GpioWaitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ms = validate_timeout_ms(p.timeout_ms).map_err(invalid_arg)?;
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.gpio_wait_for_any_edge_with_timeout(p.pin, Duration::from_millis(u64::from(ms)))
            .await
            .map_err(map_pdg_err)?;
        ok_device_json(&dev, &"edge")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_params_deserialize() {
        let p: GpioWaitParams = serde_json::from_str(r#"{"pin":5,"timeout_ms":1000}"#).unwrap();
        assert_eq!(p.pin, 5);
        assert_eq!(p.timeout_ms, 1000);
    }

    #[test]
    fn gpio_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in [
            "gpio_get",
            "gpio_put",
            "gpio_set_config",
            "gpio_wait_for_rising_edge_with_timeout",
            "gpio_wait_for_falling_edge_with_timeout",
            "gpio_wait_for_any_edge_with_timeout",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn wait_params_accept_an_optional_serial_number() {
        let without: GpioWaitParams =
            serde_json::from_str(r#"{"pin":5,"timeout_ms":1000}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: GpioWaitParams =
            serde_json::from_str(r#"{"pin":5,"timeout_ms":1000,"serial_number":"ABC123"}"#)
                .unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
}
