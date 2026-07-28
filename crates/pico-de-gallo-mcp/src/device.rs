//! Device-level tools: enumeration, status, info, version, ping.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use serde::Serialize;

use crate::error::map_pdg_err;
use crate::{GalloMcp, ok_json};

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DeviceEntry {
    serial_number: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct StatusResult {
    attached: bool,
    firmware_version: Option<String>,
    schema_major: Option<u16>,
    schema_minor: Option<u16>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PingParams {
    /// Value to echo back.
    id: u32,
}

#[tool_router(router = device_router, vis = "pub(crate)")]
impl GalloMcp {
    /// List all Pico de Gallo devices currently attached (no connection needed).
    #[tool(
        description = "List attached Pico de Gallo devices",
        annotations(read_only_hint = true)
    )]
    async fn list_devices(&self) -> Result<CallToolResult, ErrorData> {
        let entries: Vec<DeviceEntry> = pico_de_gallo_lib::list_devices()
            .into_iter()
            .map(|d| DeviceEntry {
                serial_number: d.serial_number,
                manufacturer: d.manufacturer,
                product: d.product,
            })
            .collect();
        ok_json(&entries)
    }

    /// Report whether a device is reachable, plus its firmware/schema version.
    #[tool(
        description = "Get device attachment status and version",
        annotations(read_only_hint = true)
    )]
    async fn status(&self) -> Result<CallToolResult, ErrorData> {
        match self.ensure_validated().await {
            Ok(info) => ok_json(&StatusResult {
                attached: true,
                firmware_version: Some(format!(
                    "{}.{}.{}",
                    info.fw_major, info.fw_minor, info.fw_patch
                )),
                schema_major: Some(info.schema_major),
                schema_minor: Some(info.schema_minor),
            }),
            Err(_) => ok_json(&StatusResult {
                attached: false,
                firmware_version: None,
                schema_major: None,
                schema_minor: None,
            }),
        }
    }

    /// Get full device info (firmware version, schema version, capabilities).
    #[tool(
        description = "Get firmware and schema device info",
        annotations(read_only_hint = true)
    )]
    async fn device_info(&self) -> Result<CallToolResult, ErrorData> {
        let info = self.device.device_info().await.map_err(map_pdg_err)?;
        ok_json(&info)
    }

    /// Get the firmware version string.
    #[tool(
        description = "Get firmware version",
        annotations(read_only_hint = true)
    )]
    async fn version(&self) -> Result<CallToolResult, ErrorData> {
        let v = self.device.version().await.map_err(map_pdg_err)?;
        ok_json(&v)
    }

    /// Echo a u32 (connectivity test).
    #[tool(
        description = "Ping the device (echo a u32)",
        annotations(read_only_hint = true)
    )]
    async fn ping(
        &self,
        Parameters(p): Parameters<PingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let echoed = self.device.ping(p.id).await.map_err(map_pdg_err)?;
        ok_json(&echoed)
    }
}

#[cfg(test)]
mod tests {
    use crate::GalloMcp;

    #[test]
    fn device_tools_are_registered() {
        let names: Vec<String> = GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in ["list_devices", "status", "device_info", "version", "ping"] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }
}
