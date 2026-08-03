//! `gallo-mcp` — an MCP server exposing a Pico de Gallo device to AI agents.
//!
//! Wraps [`pico_de_gallo_lib::PicoDeGallo`] and presents one MCP tool per
//! peripheral operation over stdio. Bytes cross the tool boundary as hex
//! strings (in) and [`encoding::Bytes`] (out). Write/actuation tools are
//! annotated `destructive_hint = true`; approval is delegated to the MCP
//! client.

pub mod adc;
pub mod device;
pub mod encoding;
pub mod error;
pub mod gpio;
pub mod i2c;
pub mod onewire;
pub mod pwm;
pub mod select;
pub mod spi;
pub mod uart;

use std::sync::Arc;
use std::time::Duration;

use pico_de_gallo_lib::{DeviceInfo, PicoDeGallo};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool_handler};
use serde::Serialize;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Wrap a serializable value as a successful tool result.
///
/// Only for tools that answer without opening a device (`list_devices`,
/// `status`). Device tools must use [`ok_device_json`] so the response names
/// the board that served it.
pub(crate) fn ok_json<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
}

/// A tool response tagged with the device that served it.
///
/// Every device tool returns this shape so an agent can see, on any call,
/// which board answered — rather than only when it thinks to ask.
#[derive(Serialize)]
pub(crate) struct Envelope<'a, T> {
    /// Serial of the board that served the call. `null` only when the sole
    /// attached board reports no USB serial number.
    pub(crate) serial_number: Option<&'a str>,
    /// The tool's own payload, unchanged.
    pub(crate) result: &'a T,
}

/// Wrap a tool payload together with the serial of the device that served it.
pub(crate) fn ok_device_json<T: Serialize>(
    dev: &Device,
    value: &T,
) -> Result<CallToolResult, ErrorData> {
    ok_json(&Envelope {
        serial_number: dev.serial(),
        result: value,
    })
}

/// Serial of every attached Pico de Gallo, in enumeration order.
///
/// A `None` entry is a board that reports no USB serial number.
pub(crate) fn attached_serials() -> Vec<Option<String>> {
    pico_de_gallo_lib::list_devices()
        .into_iter()
        .map(|d| d.serial_number)
        .collect()
}

/// Substring of the error postcard-rpc's `try_new_raw_nusb` returns when USB
/// enumeration finds no matching device (postcard-rpc 0.12.1, raw_nusb.rs).
/// [`open_with_retry`] classifies on it to distinguish "no board attached"
/// from a transient claim failure.
const NOT_FOUND: &str = "Failed to find matching nusb device";

/// The error message for a board that could not be opened after enumeration
/// found it.
///
/// Reaching this means the board went away between enumeration and open:
/// selection already proved something was attached, so "no device attached"
/// would be wrong in either arm.
fn vanished_board_msg(serial: Option<&str>) -> String {
    match serial {
        Some(sn) => format!(
            "device {sn} was attached a moment ago but is gone now; \
             check the USB connection and retry"
        ),
        None => "the attached board vanished between enumeration and open; \
                 check the USB connection and retry"
            .to_string(),
    }
}

/// Open the board `serial` names, retrying a transient interface claim.
///
/// `None` opens the sole attached board, which reports no serial number.
///
/// The claim can fail transiently — e.g. the previous connection's
/// asynchronous teardown has not released the exclusive USB claim yet, the
/// Windows double-claim hazard in AGENTS.md §13.17 — so this retries a few
/// times with a short backoff before giving up. A [`NOT_FOUND`] failure is
/// not transient and returns immediately.
async fn open_with_retry(serial: Option<&str>) -> Result<PicoDeGallo, ErrorData> {
    /// Total attempts to claim the interface before giving up.
    const MAX_ATTEMPTS: u32 = 5;
    /// Backoff between claim attempts (absorbs async release window).
    const BACKOFF: Duration = Duration::from_millis(100);

    let mut attempt: u32 = 1;
    loop {
        let result = match serial {
            Some(sn) => PicoDeGallo::try_new_with_serial_number(sn),
            None => PicoDeGallo::try_new(),
        };
        match result {
            Ok(dev) => return Ok(dev),
            Err(e) if e.contains(NOT_FOUND) => {
                return Err(ErrorData::internal_error(vanished_board_msg(serial), None));
            }
            Err(e) if attempt >= MAX_ATTEMPTS => {
                return Err(ErrorData::internal_error(
                    format!("failed to open device after {attempt} attempts: {e}"),
                    None,
                ));
            }
            Err(_) => {
                attempt += 1;
                tokio::time::sleep(BACKOFF).await;
            }
        }
    }
}

/// A live, validated connection to a Pico de Gallo device.
///
/// Constructed per tool call by [`GalloMcp::connect`]. Dereferences to the
/// underlying [`PicoDeGallo`] so tool handlers call device methods directly.
/// It holds the shared connection lock (`_claim`) for its whole lifetime, so
/// at most one connection exists at a time (see [`GalloMcp::connect`]).
///
/// Dropping the guard drops the transport and then releases the lock. The
/// transport tears down asynchronously (postcard-rpc owns the USB interface on
/// detached tasks and exposes no "released" signal), so the next [`connect`]
/// may briefly observe the interface still claimed; `connect` absorbs that with
/// a bounded retry. Between tool calls the board is free for other host
/// processes (e.g. the `gallo` CLI).
///
/// [`connect`]: GalloMcp::connect
pub(crate) struct Device {
    inner: PicoDeGallo,
    info: DeviceInfo,
    /// Serial this connection was opened with, as chosen by
    /// [`select::resolve_target`]. `None` only for a sole serial-less board.
    serial: Option<String>,
    /// Serializes device access across concurrent tool calls; held for the
    /// lifetime of the connection. Released (after `inner`) when this drops.
    ///
    /// Must remain the **last** field: fields drop in declaration order, and
    /// the transport in `inner` has to be torn down before the lock is
    /// released, or the next `connect` can win the lock while this
    /// connection still holds the USB claim.
    _claim: OwnedMutexGuard<()>,
}

impl std::ops::Deref for Device {
    type Target = PicoDeGallo;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Device {
    /// The device info captured during validation on connect.
    pub(crate) fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// USB serial of the board this connection is bound to.
    ///
    /// `None` only when the sole attached board reports no serial number.
    pub(crate) fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }
}

/// The MCP service. Holds only the device selector and a connection lock; the
/// USB connection is opened per tool call by [`GalloMcp::connect`] and released
/// when the call completes, so the board is free for other host processes
/// between calls.
#[derive(Clone)]
pub struct GalloMcp {
    serial_number: Option<String>,
    /// Serializes device access. rmcp dispatches each tool call on its own
    /// task, so handlers run concurrently; this lock ensures at most one live
    /// [`Device`] (USB claim) at a time. Shared across handler clones.
    connection: Arc<Mutex<()>>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl GalloMcp {
    /// Construct the service, optionally pinned to a specific USB serial
    /// number. No USB connection is made here — each tool call opens and
    /// releases its own connection.
    pub fn new(serial_number: Option<&str>) -> Self {
        Self {
            serial_number: serial_number.map(str::to_string),
            connection: Arc::new(Mutex::new(())),
            tool_router: Self::device_router()
                + Self::i2c_router()
                + Self::spi_router()
                + Self::uart_router()
                + Self::gpio_router()
                + Self::pwm_router()
                + Self::adc_router()
                + Self::onewire_router(),
        }
    }

    /// Build the merged tool router without constructing a device.
    ///
    /// Test-only: lets registration tests assert the tool surface without
    /// touching USB hardware.
    #[cfg(test)]
    pub(crate) fn router_for_test() -> ToolRouter<Self> {
        Self::device_router()
            + Self::i2c_router()
            + Self::spi_router()
            + Self::uart_router()
            + Self::gpio_router()
            + Self::pwm_router()
            + Self::adc_router()
            + Self::onewire_router()
    }

    /// The server's `--serial-number` pin, if any.
    ///
    /// A pinned server cannot address any other board; that is the only
    /// guarantee enforced by construction rather than by agent diligence.
    ///
    /// Named `pinned_serial` rather than `pin` because in this repository
    /// `pin` means a GPIO pin.
    pub(crate) fn pinned_serial(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }

    /// Open and validate a fresh connection to the target device.
    ///
    /// `requested` is the call's optional `serial_number`. The target is
    /// chosen by [`select::resolve_target`] from the attached boards, the
    /// server's `--serial-number` pin, and `requested` — so an ambiguous
    /// choice is refused rather than guessed.
    ///
    /// Serializes device access with the shared lock (rmcp dispatches each
    /// tool call on its own `tokio::spawn` task, so handlers can run
    /// concurrently), opens the resolved board with [`open_with_retry`],
    /// validates schema compatibility, and runs the connect-time subscription
    /// reset. The returned [`Device`] owns the connection, the resolved
    /// serial, and the lock.
    pub(crate) async fn connect(&self, requested: Option<&str>) -> Result<Device, ErrorData> {
        let claim = self.connection.clone().lock_owned().await;

        // Resolve before opening: with no board attached this reports
        // `NoDevice` without opening a device.
        let serial = select::resolve_target(&attached_serials(), self.pinned_serial(), requested)
            .map_err(select::map_select_err)?;

        let inner = open_with_retry(serial.as_deref()).await?;
        let info = inner.validate().await.map_err(error::map_validate_err)?;
        let _ = inner.system_reset_subscriptions().await;
        Ok(Device {
            inner,
            info,
            serial,
            _claim: claim,
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for GalloMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Bridge to a Pico de Gallo USB device (I2C/SPI/UART/GPIO/PWM/ADC/1-Wire). \
                 Bytes are hex strings like \"0x48,0x00\". Read tools are safe; tools that \
                 write or actuate pins are marked destructive and may require approval.",
        )
    }
}

#[cfg(test)]
mod tests {
    /// A sampled copy of the exact error text postcard-rpc's `try_new_raw_nusb`
    /// returns when enumeration finds no match (postcard-rpc 0.12.1,
    /// raw_nusb.rs). Ties [`crate::NOT_FOUND`] — the substring `connect()`
    /// classifies on — to that literal, so editing the const in isolation
    /// (breaking the match) fails this test. It cannot detect an upstream
    /// change to the postcard-rpc message itself.
    #[test]
    fn not_found_substring_matches_postcard_error() {
        let postcard_err = "Failed to find matching nusb device!";
        assert!(postcard_err.contains(crate::NOT_FOUND));
    }

    #[test]
    fn envelope_puts_the_payload_under_result() {
        let payload = serde_json::json!({ "hex": "0x48" });
        let env = crate::Envelope {
            serial_number: Some("9A54ED7E3A1D9D98"),
            result: &payload,
        };
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(v["serial_number"], "9A54ED7E3A1D9D98");
        assert_eq!(v["result"]["hex"], "0x48");
    }

    #[test]
    fn envelope_reports_a_serialless_board_as_null() {
        let env = crate::Envelope {
            serial_number: None,
            result: &"ok",
        };
        // Exact serialization: `serde_json`'s `Index` returns `Null` for a
        // missing key too, so only this proves the field is present. An
        // agent has to be able to tell a serial-less board from a response
        // that was never enveloped.
        assert_eq!(
            serde_json::to_string(&env).unwrap(),
            r#"{"serial_number":null,"result":"ok"}"#
        );
    }

    #[test]
    fn vanished_board_msg_names_the_serial_it_looked_for() {
        let msg = crate::vanished_board_msg(Some("9A54ED7E3A1D9D98"));
        assert!(msg.contains("9A54ED7E3A1D9D98"), "{msg}");
        assert!(msg.contains("check the USB connection"), "{msg}");
    }

    #[test]
    fn vanished_board_msg_for_an_unnamed_board_does_not_deny_the_board() {
        // Only reachable when selection resolved a sole attached board, so
        // claiming nothing is attached would contradict what we just saw.
        let msg = crate::vanished_board_msg(None);
        assert!(msg.contains("vanished"), "{msg}");
        assert!(!msg.contains("no device attached"), "{msg}");
    }
}
