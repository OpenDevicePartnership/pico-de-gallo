//! Device-level tools: enumeration, status, info, version, ping.

use pico_de_gallo_lib::{DeviceDescription, DeviceInfo};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use serde::Serialize;

use crate::error::map_pdg_err;
use crate::select::{SelectError, TargetParams, resolve_target};
use crate::{GalloMcp, attached_serials, ok_device_json, ok_json};

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DeviceEntry {
    serial_number: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
    /// True when this board is the server's `--serial-number` pin.
    pinned: bool,
    /// True when a call that omits `serial_number` will use this board.
    default_target: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListDevicesResult {
    /// Every attached Pico de Gallo.
    devices: Vec<DeviceEntry>,
    /// The server's `--serial-number` pin, if any. Reported separately from
    /// the per-entry flag because a pinned board that is not attached
    /// produces no entry at all — precisely when you want to be told.
    pinned: Option<String>,
    /// True when `serial_number` must be supplied on every device tool call.
    serial_number_required: bool,
    /// Present only when `serial_number_required`.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct StatusResult {
    /// True when at least one board is attached.
    attached: bool,
    /// Serial of the board actually reached; null when none was.
    serial_number: Option<String>,
    /// True when two or more boards are attached and the server is unpinned.
    ambiguous: bool,
    /// Serial of every attached board; a null entry reports no serial.
    available: Vec<Option<String>>,
    /// The server's `--serial-number` pin, if any.
    pinned: Option<String>,
    /// Why `serial_number` is null, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    firmware_version: Option<String>,
    schema_major: Option<u16>,
    schema_minor: Option<u16>,
    /// Firmware build identity (`git describe`), null until a board is
    /// reached. Informational: it names the running image, and never affects
    /// whether a call succeeds.
    build_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PingParams {
    /// Value to echo back.
    id: u32,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached and the server is not pinned to one; optional
    /// otherwise.
    #[serde(default)]
    serial_number: Option<String>,
}

/// Build the `list_devices` response.
///
/// Asks [`resolve_target`] the same question `connect` asks, so the advertised
/// default can never drift from the board a bare call actually reaches.
fn build_list_result(
    descs: Vec<DeviceDescription>,
    pinned_serial: Option<&str>,
) -> ListDevicesResult {
    let attached: Vec<Option<String>> = descs.iter().map(|d| d.serial_number.clone()).collect();
    let resolved = resolve_target(&attached, pinned_serial, None);
    let serial_number_required = matches!(resolved, Err(SelectError::Ambiguous { .. }));
    // `Some(t)` is the board a bare call would use. `t` is itself optional
    // because a sole serial-less board is still a valid target.
    let default_target = resolved.ok();

    let devices: Vec<DeviceEntry> = descs
        .into_iter()
        .map(|d| DeviceEntry {
            pinned: pinned_serial.is_some() && d.serial_number.as_deref() == pinned_serial,
            default_target: matches!(&default_target, Some(t) if *t == d.serial_number),
            serial_number: d.serial_number,
            manufacturer: d.manufacturer,
            product: d.product,
        })
        .collect();

    let note = serial_number_required.then(|| {
        format!(
            "{} devices attached and this server is not pinned; pass serial_number \
             on every device tool call.",
            devices.len()
        )
    });

    ListDevicesResult {
        devices,
        pinned: pinned_serial.map(str::to_string),
        serial_number_required,
        note,
    }
}

/// Build the part of the `status` response that needs no connection.
///
/// `status` must stay answerable when nothing is resolvable — that is the
/// whole point of the tool — so this never fails. The caller fills in the
/// device fields if it manages to connect.
fn build_status(
    available: Vec<Option<String>>,
    pinned_serial: Option<&str>,
    requested: Option<&str>,
) -> StatusResult {
    let resolved = resolve_target(&available, pinned_serial, requested);
    // Deliberately resolved without `requested`: this field answers "would a
    // call that omits serial_number be ambiguous?", which is a property of
    // the bench, not of this call. Computing it from `resolved` would report
    // false whenever the caller happened to name a board.
    let bare = resolve_target(&available, pinned_serial, None);
    StatusResult {
        attached: !available.is_empty(),
        serial_number: None,
        ambiguous: matches!(bare, Err(SelectError::Ambiguous { .. })),
        pinned: pinned_serial.map(str::to_string),
        reason: resolved.err().map(|e| e.to_string()),
        available,
        firmware_version: None,
        schema_major: None,
        schema_minor: None,
        build_id: None,
    }
}

/// Fill in the fields of a `status` response that require a reached board.
///
/// Split out from `status` so it is unit-testable: the caller needs a live
/// `connect()` and therefore a physical board, which means anything left
/// inline there has no automated coverage at all.
fn fill_status_from_device(out: &mut StatusResult, serial: Option<&str>, info: &DeviceInfo) {
    out.serial_number = serial.map(str::to_string);
    out.firmware_version = Some(format!(
        "{}.{}.{}",
        info.fw_major, info.fw_minor, info.fw_patch
    ));
    out.schema_major = Some(info.schema_major);
    out.schema_minor = Some(info.schema_minor);
    out.build_id = Some(info.build_id().to_string());
}

#[tool_router(router = device_router, vis = "pub(crate)")]
impl GalloMcp {
    /// List all Pico de Gallo devices currently attached (no connection needed).
    #[tool(
        description = "List attached Pico de Gallo devices",
        annotations(read_only_hint = true)
    )]
    async fn list_devices(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&build_list_result(
            pico_de_gallo_lib::list_devices(),
            self.pinned_serial(),
        ))
    }

    /// Report which board is reachable, and why not when none is.
    #[tool(
        description = "Get device attachment status and version",
        annotations(read_only_hint = true)
    )]
    async fn status(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let requested = p.serial_number.as_deref();
        let mut out = build_status(attached_serials(), self.pinned_serial(), requested);
        if out.reason.is_none() {
            match self.connect(requested).await {
                Ok(dev) => fill_status_from_device(&mut out, dev.serial(), dev.info()),
                // Selection succeeded but the board could not be opened or
                // validated. Say so rather than leaving a bare null.
                Err(e) => out.reason = Some(e.message.to_string()),
            }
        }
        ok_json(&out)
    }

    /// Get full device info (firmware version, schema version, capabilities).
    #[tool(
        description = "Get firmware and schema device info",
        annotations(read_only_hint = true)
    )]
    async fn device_info(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let info = dev.device_info().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &info)
    }

    /// Get the firmware version string.
    #[tool(
        description = "Get firmware version",
        annotations(read_only_hint = true)
    )]
    async fn version(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let v = dev.version().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &v)
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
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let echoed = dev.ping(p.id).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &echoed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GalloMcp;

    const A: &str = "9A54ED7E3A1D9D98";
    const B: &str = "5256657D8A5D7F03";

    fn desc(serial: Option<&str>) -> DeviceDescription {
        DeviceDescription {
            serial_number: serial.map(str::to_string),
            manufacturer: Some("Microsoft".to_string()),
            product: Some("Pico de Gallo".to_string()),
        }
    }

    #[test]
    fn fill_status_from_device_populates_every_device_field() {
        // `status` can only reach this logic through `connect()`, i.e. with a
        // physical board attached, so without this test the population is
        // completely uncovered -- deleting the build_id assignment used to
        // leave the whole suite green.
        let mut out = build_status(vec![Some("ABC123".to_string())], None, None);
        let info = DeviceInfo {
            fw_major: 0,
            fw_minor: 11,
            fw_patch: 0,
            schema_major: 0,
            schema_minor: 7,
            schema_patch: 0,
            hw_version: 2,
            capabilities: pico_de_gallo_lib::Capabilities::NONE,
            num_gpios: pico_de_gallo_lib::NUM_GPIOS as u8,
            build_id: "firmware-v0.11.0-27-gdeadbee-dirty".try_into().unwrap(),
        };
        fill_status_from_device(&mut out, Some("ABC123"), &info);
        assert_eq!(out.serial_number.as_deref(), Some("ABC123"));
        assert_eq!(out.firmware_version.as_deref(), Some("0.11.0"));
        assert_eq!(out.schema_major, Some(0));
        assert_eq!(out.schema_minor, Some(7));
        assert_eq!(
            out.build_id.as_deref(),
            Some("firmware-v0.11.0-27-gdeadbee-dirty")
        );
    }

    #[test]
    fn status_result_serializes_build_id() {
        let mut out = build_status(vec![Some("ABC123".to_string())], None, None);
        out.build_id = Some("firmware-v0.11.0-27-gdeadbee-dirty".to_string());
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(
            json["build_id"],
            serde_json::json!("firmware-v0.11.0-27-gdeadbee-dirty")
        );
    }

    #[test]
    fn status_result_build_id_is_null_before_connecting() {
        // `build_status` runs before any connection, so it cannot know the
        // build identity. It must say null rather than inventing one.
        let out = build_status(vec![Some("ABC123".to_string())], None, None);
        assert!(out.build_id.is_none());
        let json = serde_json::to_value(&out).unwrap();
        assert!(
            json.get("build_id").is_some(),
            "field must still be present"
        );
        assert!(
            json["build_id"].is_null(),
            "build_id must serialize as null, not be omitted"
        );
    }

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

    #[test]
    fn ping_params_accept_an_optional_serial_number() {
        let without: PingParams = serde_json::from_str(r#"{"id":7}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: PingParams =
            serde_json::from_str(r#"{"id":7,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }

    #[test]
    fn sole_board_is_the_default_target_and_needs_no_serial() {
        let out = build_list_result(vec![desc(Some(A))], None);
        assert!(!out.serial_number_required);
        assert!(out.note.is_none());
        assert!(out.devices[0].default_target);
        assert!(!out.devices[0].pinned);
    }

    #[test]
    fn sole_serialless_board_is_still_the_default_target() {
        let out = build_list_result(vec![desc(None)], None);
        assert!(!out.serial_number_required);
        assert!(out.devices[0].default_target);
    }

    #[test]
    fn two_boards_require_a_serial_and_have_no_default() {
        let out = build_list_result(vec![desc(Some(A)), desc(Some(B))], None);
        assert!(out.serial_number_required);
        assert!(out.note.as_deref().unwrap().contains("2 devices attached"));
        assert!(out.devices.iter().all(|d| !d.default_target));
        assert!(out.devices.iter().all(|d| !d.pinned));
    }

    #[test]
    fn a_pin_removes_the_ambiguity_and_marks_its_board() {
        let out = build_list_result(vec![desc(Some(A)), desc(Some(B))], Some(A));
        assert!(!out.serial_number_required);
        assert!(out.note.is_none());
        assert_eq!(out.pinned.as_deref(), Some(A));
        assert!(out.devices[0].pinned && out.devices[0].default_target);
        assert!(!out.devices[1].pinned && !out.devices[1].default_target);
    }

    #[test]
    fn a_pin_to_an_absent_board_is_still_reported() {
        // No entry can carry the flag, which is exactly when the top-level
        // field earns its place.
        let out = build_list_result(vec![desc(Some(B))], Some(A));
        assert_eq!(out.pinned.as_deref(), Some(A));
        assert!(out.devices.iter().all(|d| !d.pinned));
        assert!(out.devices.iter().all(|d| !d.default_target));
    }

    #[test]
    fn status_reports_ambiguity_rather_than_claiming_no_board() {
        let available = vec![Some(A.to_string()), Some(B.to_string())];
        let out = build_status(available, None, None);
        assert!(out.attached, "two boards are attached");
        assert!(out.ambiguous);
        assert!(out.serial_number.is_none());
        assert_eq!(out.available.len(), 2);
        let reason = out.reason.unwrap();
        assert!(reason.contains(A) && reason.contains(B), "{reason}");
    }

    #[test]
    fn status_reports_an_empty_bus_as_unattached() {
        let out = build_status(vec![], None, None);
        assert!(!out.attached);
        assert!(!out.ambiguous);
        assert!(out.reason.is_some());
    }

    #[test]
    fn status_explains_a_pin_conflict_instead_of_answering_for_another_board() {
        let available = vec![Some(A.to_string()), Some(B.to_string())];
        let out = build_status(available, Some(A), Some(B));
        assert!(out.serial_number.is_none());
        let reason = out.reason.unwrap();
        assert!(reason.contains("--serial-number"), "{reason}");
    }

    #[test]
    fn status_has_no_reason_when_selection_succeeds() {
        let out = build_status(vec![Some(A.to_string())], None, None);
        assert!(out.reason.is_none());
        assert!(!out.ambiguous);
    }

    #[test]
    fn status_still_reports_ambiguity_when_the_caller_named_a_board() {
        let available = vec![Some(A.to_string()), Some(B.to_string())];
        let out = build_status(available, None, Some(A));
        // The call resolved, so there is nothing to explain about it...
        assert_eq!(out.serial_number, None); // filled in by the handler, not the builder
        assert!(out.reason.is_none());
        // ...but a bare call would still be ambiguous, and an agent asking
        // "is A up?" must not conclude it can drop the argument next time.
        assert!(out.ambiguous);
    }
}
