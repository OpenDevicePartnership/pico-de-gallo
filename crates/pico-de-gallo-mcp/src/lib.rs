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

use std::collections::HashMap;
use std::sync::Arc;
// The outer map is guarded by a *std* mutex, not a tokio one, so that holding
// its guard across an await fails to compile: the guard is `!Send`, so it
// would make rmcp's handler futures `!Send`. See [`GalloMcp::lock_for`].
// `BoardLock` stays a tokio mutex — that one is held across an await by
// design.
use std::sync::Mutex as MapLock;
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
/// It holds that board's connection lock (`_claim`) for its whole lifetime,
/// so at most one connection **per board** exists at a time. Connections to
/// different boards proceed concurrently: the claim this serialises belongs to
/// one device (see [`GalloMcp::lock_for`]).
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
    /// Serializes access to *this board* across concurrent tool calls; held
    /// for the lifetime of the connection. Released (after `inner`) when this
    /// drops.
    ///
    /// Must remain the **last** field: fields drop in declaration order, and
    /// the transport in `inner` has to be torn down before the lock is
    /// released, or the next `connect` for the same board can win the lock
    /// while this connection still holds the USB claim.
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

/// The lock guarding one board's exclusive USB interface claim.
///
/// An `Arc` because [`GalloMcp::lock_for`] has to clone this *out* of the map
/// and await it after the map's own guard is released; a bare `Mutex` in the
/// map could only be awaited while still borrowing the map. It stays a
/// `tokio::sync::Mutex` because it is deliberately held across awaits — for
/// the whole lifetime of a [`Device`], including a `gpio_wait_*`.
///
/// Also keeps `GalloMcp::locks`'s map under clippy's `type_complexity`
/// threshold, and reads as what it is: a lock per board.
type BoardLock = Arc<Mutex<()>>;

/// The MCP service. Holds only the device selector and the per-board
/// connection locks; the USB connection is opened per tool call by
/// [`GalloMcp::connect`] and released when the call completes, so the board is
/// free for other host processes between calls.
#[derive(Clone)]
pub struct GalloMcp {
    serial_number: Option<String>,
    /// Per-board connection locks, keyed on the resolved serial.
    ///
    /// What this serialises is the exclusive USB interface claim, which is a
    /// property of one device: two different boards can be open at once. A
    /// single server-wide lock would mean a long `gpio_wait_*` on one board
    /// stalled every call to another, which defeats the point of addressing
    /// boards per call.
    ///
    /// [`open_with_retry`]'s 5 × 100 ms retry is not a substitute for this
    /// lock and deleting the lock in favour of it would be a regression: the
    /// retry absorbs the *asynchronous release window* of a connection that
    /// has already been dropped, which is milliseconds. Waiting for a call
    /// that still holds the board is unbounded — every `gpio_wait_*` exceeds
    /// 500 ms by construction — so without the lock those calls would fail
    /// with "failed to open device after 5 attempts" instead of queueing.
    ///
    /// Only serials that [`select::resolve_target`] accepted are ever
    /// inserted, so an agent cannot grow this map by naming arbitrary
    /// serials: an unattached serial is refused before [`GalloMcp::lock_for`]
    /// is reached. The bound is therefore the boards this server has *seen* —
    /// every distinct serial it resolved successfully over the process
    /// lifetime — not the boards attached right now.
    ///
    /// Entries are deliberately never removed. Evicting one correctly would
    /// mean inspecting `Arc::strong_count` under the map lock to prove no
    /// caller still holds or is queued on that board's lock, to reclaim
    /// roughly one `String` and one `Arc` per board ever seen.
    ///
    /// Shared across handler clones: rmcp dispatches each tool call on its own
    /// task, so handlers run concurrently.
    locks: Arc<MapLock<HashMap<Option<String>, BoardLock>>>,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl GalloMcp {
    /// Construct the service, optionally pinned to a specific USB serial
    /// number. No USB connection is made here — each tool call opens and
    /// releases its own connection.
    pub fn new(serial_number: Option<&str>) -> Self {
        Self {
            serial_number: serial_number.map(str::to_string),
            locks: Arc::new(MapLock::new(HashMap::new())),
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

    /// Acquire the connection lock for one board, creating it on first use.
    ///
    /// `serial` must be a [`select::resolve_target`] result. That is the sole
    /// reason [`GalloMcp::locks`] is bounded — a caller that passes an
    /// unvalidated, agent-supplied serial would let the map grow without
    /// limit.
    ///
    /// The map guard is a `std::sync::MutexGuard` (see [`MapLock`]) precisely
    /// so that holding it across the per-board await — which would restore
    /// the server-wide serialisation this exists to remove — cannot compile.
    /// That guard is `!Send`, so holding it would make every handler future
    /// `!Send`, and rmcp spawns those; the build fails pointing at the guard
    /// binding. This is enforcement, not lint advice: it holds under plain
    /// `cargo build`, with no `#[allow]` available. (`clippy::await_holding_lock`
    /// covers the same ground, but the `Send` error aborts the build before
    /// clippy's lint pass ever runs.)
    ///
    /// Contrast [`Device`]'s "`_claim` must be the last field" invariant,
    /// which has no cheap compiler enforcement and is therefore only a
    /// comment. That is exactly why this one, which does, uses it.
    pub(crate) async fn lock_for(&self, serial: Option<&str>) -> OwnedMutexGuard<()> {
        let board = {
            let mut map = self
                .locks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            map.entry(serial.map(str::to_string)).or_default().clone()
        };
        tracing::debug!(
            serial_number = serial.unwrap_or("<none>"),
            "awaiting board connection lock"
        );
        board.lock_owned().await
    }

    /// Open and validate a fresh connection to the target device.
    ///
    /// `requested` is the call's optional `serial_number`. The target is
    /// chosen by [`select::resolve_target`] from the attached boards, the
    /// server's `--serial-number` pin, and `requested` — so an ambiguous
    /// choice is refused rather than guessed.
    ///
    /// Serializes device access with that board's lock (rmcp dispatches each
    /// tool call on its own `tokio::spawn` task, so handlers can run
    /// concurrently), opens the resolved board with [`open_with_retry`],
    /// validates schema compatibility, and runs the connect-time subscription
    /// reset. The returned [`Device`] owns the connection, the resolved
    /// serial, and the lock. Calls naming different boards do not block each
    /// other; see [`GalloMcp::lock_for`].
    pub(crate) async fn connect(&self, requested: Option<&str>) -> Result<Device, ErrorData> {
        // Resolve first: the lock is per board, so we need the target before
        // we can take the right one. Enumeration takes no USB claim, so it
        // does not need the lock. With no board attached this reports
        // `NoDevice` without opening a device.
        //
        // The consequence is that this enumeration result is no longer
        // bounded by how long the open takes, but by how long another call
        // holds *this board's* lock — a concurrent `gpio_wait_*` can hold it
        // for its whole timeout. A board that goes away in that window is
        // still reported correctly, by `open_with_retry` via
        // `vanished_board_msg`; only its "a moment ago" wording reads oddly
        // after a long wait.
        let serial = select::resolve_target(&attached_serials(), self.pinned_serial(), requested)
            .map_err(select::map_select_err)?;

        let claim = self.lock_for(serial.as_deref()).await;

        let inner = open_with_retry(serial.as_deref()).await?;
        let info = inner.validate().await.map_err(error::map_validate_err)?;
        // Tears down *every* GPIO subscription on this board, including ones
        // owned by other host processes — a `gallo` CLI session or a user
        // program watching a pin loses it the moment an agent touches the
        // board. That is the documented host protocol (AGENTS.md §13.17,
        // 2026-05-29), not a defect; per-call selection only widened the
        // blast radius from "the one board this server could reach" to "any
        // attached board, on any call".
        //
        // The discarded value is `Result<u8, _>`: the count of subscriptions
        // torn down, which nothing needs, and a transport error, which is
        // deliberately swallowed. `validate()` has just succeeded on this
        // transport, so a fault here is both unlikely and not worth failing a
        // call over — a genuinely dead transport resurfaces immediately, as
        // an error on the operation the agent actually asked for.
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
             write or actuate pins are marked destructive and may require approval. \
             Every device tool takes an optional serial_number choosing the board, and \
             every response echoes the serial of the board that served the call. \
             serial_number is REQUIRED when two or more boards are attached and the \
             server is not pinned to one: without it the call fails and lists the \
             serials you can use. Call list_devices first to see what is attached. \
             Device state is per board — bus configuration, GPIO direction, PWM enable \
             and 1-Wire search progress all live on the board you addressed, so a \
             follow-up call must repeat the serial_number of the call that set it up. \
             Boards can also differ in capability by hardware revision; call \
             device_info per board rather than assuming they are interchangeable.",
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

    #[tokio::test]
    async fn different_boards_do_not_block_each_other() {
        use std::time::Duration;
        let mcp = crate::GalloMcp::new(None);
        let _a = mcp.lock_for(Some("BOARD_A")).await;
        let b =
            tokio::time::timeout(Duration::from_millis(250), mcp.lock_for(Some("BOARD_B"))).await;
        assert!(
            b.is_ok(),
            "holding board A's connection lock blocked board B; a long \
             gpio_wait on one board would stall every call to the other"
        );
    }

    #[tokio::test]
    async fn the_same_board_stays_serialised() {
        use std::time::Duration;
        let mcp = crate::GalloMcp::new(None);
        let first = mcp.lock_for(Some("BOARD_A")).await;
        let second =
            tokio::time::timeout(Duration::from_millis(250), mcp.lock_for(Some("BOARD_A"))).await;
        assert!(
            second.is_err(),
            "two connections to the same board were allowed at once; the \
             exclusive USB claim would fail on Windows (AGENTS.md §13.17)"
        );

        // The other half of the property, and the one production depends on:
        // releasing the guard has to admit the next caller. A finished
        // `gpio_wait_*` that left the board locked would block every later
        // call to it for the rest of the process.
        drop(first);
        let third =
            tokio::time::timeout(Duration::from_millis(250), mcp.lock_for(Some("BOARD_A"))).await;
        assert!(
            third.is_ok(),
            "the board was still locked after its guard dropped; every later \
             call naming it would block until the server restarted"
        );
    }

    #[tokio::test]
    async fn a_parked_waiter_does_not_block_another_board() {
        use std::future::Future;
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        let mcp = crate::GalloMcp::new(None);
        let _held = mcp.lock_for(Some("BOARD_A")).await;

        let mut cx = Context::from_waker(Waker::noop());

        // A second A-waiter, driven to its park point *inside* lock_for and
        // kept alive. No spawn and no clock: the poll returning Pending is
        // the happens-before a sleep could only approximate, so this holds on
        // any runtime flavour — including the multi_thread one a later test
        // in this file may well want.
        let mut parked = pin!(mcp.lock_for(Some("BOARD_A")));
        assert!(matches!(parked.as_mut().poll(&mut cx), Poll::Pending));

        // If the map guard were held across the per-board await, this could
        // not be Ready — that is the server-wide serialisation this removes.
        let mut b = pin!(mcp.lock_for(Some("BOARD_B")));
        assert!(
            matches!(b.as_mut().poll(&mut cx), Poll::Ready(_)),
            "a caller queued on board A blocked board B: the map lock is held \
             across the per-board await"
        );
    }

    /// The exact text the `serial_number` rustdoc compiles to.
    ///
    /// Hand-copied into 27 params structs plus `TargetParams`. It becomes the
    /// JSON Schema `description` an agent reads, so a typo, a reflow, or a
    /// well-meaning rewording in one copy leaves one tool's schema saying
    /// something different from every other, with nothing failing.
    const SERIAL_DESC: &str = "USB serial number of the board to use. \
        Required when two or more\nboards are attached and the server is not \
        pinned to one; optional\notherwise.";

    /// Every device tool must accept an optional `serial_number`, described
    /// identically.
    ///
    /// The field was added by hand to 27 params structs plus one shared
    /// selector. This is what catches the struct that got missed.
    #[test]
    fn every_device_tool_accepts_an_optional_serial_number() {
        for tool in crate::GalloMcp::router_for_test().list_all() {
            // The only tool that touches no device.
            if tool.name == "list_devices" {
                continue;
            }
            let schema = tool.input_schema.as_ref();
            let props = schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .unwrap_or_else(|| panic!("{}: input schema has no properties", tool.name));
            assert!(
                props.contains_key("serial_number"),
                "{}: input schema is missing serial_number",
                tool.name
            );
            assert_eq!(
                props["serial_number"]
                    .get("description")
                    .and_then(serde_json::Value::as_str),
                Some(SERIAL_DESC),
                "{}: serial_number description has drifted from the canonical text",
                tool.name
            );
            if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
                assert!(
                    !required.iter().any(|v| v.as_str() == Some("serial_number")),
                    "{}: serial_number must stay optional",
                    tool.name
                );
            }
        }
    }

    /// Every handler must *use* the selector it declares, and answer through
    /// the envelope.
    ///
    /// Declaring `serial_number` and then calling `connect(None)` is invisible
    /// to the schema test above: the property is there, the argument is
    /// dropped. Only handlers taking `TargetParams` alone are compile-guarded,
    /// because there `p` would be unused and `-D warnings` fires.
    ///
    /// The `ok_json` half guards the *output* contract. Today it is protected
    /// only by `ok_json` not being imported in the peripheral modules, which
    /// stops a forgotten conversion but not a future handler that imports it
    /// back. `device.rs` is exempt: `list_devices` and `status` legitimately
    /// answer without opening a board.
    ///
    /// Crude, but it catches the whole class. After Task 7 no legitimate
    /// `connect(None)` remains, so the invariant is trivially maintainable.
    #[test]
    fn every_handler_threads_the_selector_into_connect() {
        for (name, src) in [
            ("adc.rs", include_str!("adc.rs")),
            ("device.rs", include_str!("device.rs")),
            ("gpio.rs", include_str!("gpio.rs")),
            ("i2c.rs", include_str!("i2c.rs")),
            ("onewire.rs", include_str!("onewire.rs")),
            ("pwm.rs", include_str!("pwm.rs")),
            ("spi.rs", include_str!("spi.rs")),
            ("uart.rs", include_str!("uart.rs")),
        ] {
            assert!(
                !src.contains("self.connect(None)"),
                "{name}: a handler still hard-codes connect(None); \
                 pass p.serial_number.as_deref() instead"
            );
            if name != "device.rs" {
                assert!(
                    !src.contains("ok_json("),
                    "{name}: a handler returns an unenveloped result; \
                     use ok_device_json(&dev, ..) so the response names the board"
                );
            }
        }
    }

    #[test]
    fn server_instructions_state_the_disambiguation_rule() {
        use rmcp::ServerHandler;
        let info = crate::GalloMcp::new(None).get_info();
        let instructions = info.instructions.expect("server sets instructions");
        assert!(instructions.contains("serial_number"), "{instructions}");
        assert!(
            instructions.contains("two or more boards"),
            "{instructions}"
        );
        // The per-board-state warning is the only mitigation for
        // configure-on-A-operate-on-B; losing it must fail loudly.
        assert!(
            instructions.contains("Device state is per board"),
            "{instructions}"
        );
    }
}
