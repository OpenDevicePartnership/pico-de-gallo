//! 1-Wire tools.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::encoding::{Bytes, parse_bytes};
use crate::error::{invalid_arg, map_pdg_err};
use crate::{GalloMcp, ok_json};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OneWireReadParams {
    /// Number of bytes to read.
    pub len: u16,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OneWireWriteParams {
    /// Bytes to write, as a hex string.
    pub data: String,
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OneWireWritePullupParams {
    /// Bytes to write, as a hex string.
    pub data: String,
    /// Strong-pullup duration in milliseconds.
    #[serde(default = "default_pullup_ms")]
    pub duration_ms: u16,
}
fn default_pullup_ms() -> u16 {
    750
}
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OneWireSearchParams {
    /// Continue a previous search instead of starting a new one.
    #[serde(default)]
    pub continue_search: bool,
}

#[tool_router(router = onewire_router, vis = "pub(crate)")]
impl GalloMcp {
    /// 1-Wire reset + presence detect.
    #[tool(
        description = "1-Wire reset + presence detect",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn onewire_reset(&self) -> Result<CallToolResult, ErrorData> {
        let present = self.device.onewire_reset().await.map_err(map_pdg_err)?;
        ok_json(&serde_json::json!({ "presence": present }))
    }
    /// Read bytes over 1-Wire.
    #[tool(
        description = "Read bytes over 1-Wire",
        annotations(read_only_hint = true)
    )]
    async fn onewire_read(
        &self,
        Parameters(p): Parameters<OneWireReadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let data = self.device.onewire_read(p.len).await.map_err(map_pdg_err)?;
        ok_json(&Bytes::from_slice(&data))
    }
    /// Write bytes over 1-Wire.
    #[tool(
        description = "Write bytes over 1-Wire",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn onewire_write(
        &self,
        Parameters(p): Parameters<OneWireWriteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device
            .onewire_write(&bytes)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
    }
    /// Write bytes over 1-Wire then apply strong pullup.
    #[tool(
        description = "Write bytes over 1-Wire then apply strong pullup",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn onewire_write_pullup(
        &self,
        Parameters(p): Parameters<OneWireWritePullupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bytes = parse_bytes(&p.data).map_err(invalid_arg)?;
        self.device
            .onewire_write_pullup(&bytes, p.duration_ms)
            .await
            .map_err(map_pdg_err)?;
        ok_json(&"ok")
    }
    /// Search for a 1-Wire ROM code (set continue_search to advance).
    #[tool(
        description = "Search for a 1-Wire ROM code (set continue_search to advance)",
        annotations(read_only_hint = true)
    )]
    async fn onewire_search(
        &self,
        Parameters(p): Parameters<OneWireSearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let rom = if p.continue_search {
            self.device
                .onewire_search_next()
                .await
                .map_err(map_pdg_err)?
        } else {
            self.device.onewire_search().await.map_err(map_pdg_err)?
        };
        match rom {
            Some(id) => ok_json(&serde_json::json!({ "rom": format!("0x{id:016X}"), "raw": id })),
            None => ok_json(&serde_json::json!({ "rom": null })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_params_default_is_new_search() {
        let p: OneWireSearchParams = serde_json::from_str("{}").unwrap();
        assert!(!p.continue_search);
    }
    #[test]
    fn write_pullup_params_default_duration() {
        let p: OneWireWritePullupParams = serde_json::from_str(r#"{"data":"0x44"}"#).unwrap();
        assert_eq!(p.duration_ms, 750);
    }
    #[test]
    fn onewire_tools_registered() {
        let names: Vec<String> = crate::GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in [
            "onewire_reset",
            "onewire_read",
            "onewire_write",
            "onewire_write_pullup",
            "onewire_search",
        ] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }
}
