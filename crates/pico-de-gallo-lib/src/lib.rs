//! Host-side library for communicating with a Pico de Gallo USB bridge.
//!
//! This crate provides [`PicoDeGallo`], an async client for interacting with
//! the Pico de Gallo firmware over USB. It supports I2C reads/writes, SPI
//! operations (including full-duplex transfers), UART reads/writes, GPIO
//! control, PWM output, ADC sampling, 1-Wire bus operations, and device
//! configuration — all via [postcard-rpc](https://docs.rs/postcard-rpc)
//! endpoints.
//!
//! # Quick Start
//!
//! ```no_run
//! use pico_de_gallo_lib::PicoDeGallo;
//!
//! #[tokio::main]
//! async fn main() {
//!     let gallo = PicoDeGallo::new();
//!     let version = gallo.version().await.unwrap();
//!     println!("Firmware v{}.{}.{}", version.major, version.minor, version.patch);
//! }
//! ```
//!
//! # Multiple Devices
//!
//! When multiple Pico de Gallo boards are connected, use [`list_devices`] to
//! enumerate them and [`PicoDeGallo::new_with_serial_number`] to connect to a
//! specific board:
//!
//! ```no_run
//! use pico_de_gallo_lib::{PicoDeGallo, list_devices};
//!
//! #[tokio::main]
//! async fn main() {
//!     for dev in list_devices() {
//!         println!("Found: {:?}", dev.serial_number);
//!     }
//!     let gallo = PicoDeGallo::new_with_serial_number("ABCD1234");
//! }
//! ```
//!
//! # Error Handling
//!
//! All methods return [`Result<T, PicoDeGalloError<E>>`](PicoDeGalloError)
//! where `E` is the endpoint-specific error type. Errors are either
//! communication failures ([`PicoDeGalloError::Comms`]) or endpoint-level
//! errors ([`PicoDeGalloError::Endpoint`]).

pub mod decode;

use nusb::DeviceInfo as NusbDeviceInfo;
use pico_de_gallo_internal::{
    AdcGetConfiguration, AdcRead, AdcReadRequest, GetDeviceInfo, GpioEventTopic, GpioGet, GpioGetRequest, GpioPut,
    GpioPutRequest, GpioSetConfiguration, GpioSetConfigurationRequest, GpioSubscribe, GpioSubscribeRequest,
    GpioUnsubscribe, GpioUnsubscribeRequest, GpioWaitForAny, GpioWaitForFalling, GpioWaitForHigh, GpioWaitForLow,
    GpioWaitForRising, GpioWaitRequest, I2cBatch, I2cBatchRequest, I2cGetConfiguration, I2cRead, I2cReadRequest,
    I2cScan, I2cScanRequest, I2cSetConfiguration, I2cSetConfigurationRequest, I2cWrite, I2cWriteRead,
    I2cWriteReadRequest, I2cWriteRequest, MAX_HANDLER_TIMEOUT_MS, MICROSOFT_VID, OneWireRead, OneWireReadRequest,
    OneWireReset, OneWireSearch, OneWireSearchNext, OneWireWrite, OneWireWritePullup, OneWireWritePullupRequest,
    OneWireWriteRequest, PICO_DE_GALLO_PID, PwmDisable, PwmDisableRequest, PwmEnable, PwmEnableRequest,
    PwmGetConfiguration, PwmGetConfigurationRequest, PwmGetDutyCycle, PwmGetDutyCycleRequest, PwmSetConfiguration,
    PwmSetConfigurationRequest, PwmSetDutyCycle, PwmSetDutyCycleRequest, SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR,
    SpiBatch, SpiBatchRequest, SpiFlush, SpiGetConfiguration, SpiRead, SpiReadRequest, SpiSetConfiguration,
    SpiSetConfigurationRequest, SpiTransfer, SpiTransferRequest, SpiWrite, SpiWriteRequest, SystemResetSubscriptions,
    UNDECLARED_DISPATCH_BUDGET_MS, UartFlush, UartGetConfiguration, UartRead, UartReadRequest, UartSetConfiguration,
    UartSetConfigurationRequest, UartWrite, UartWriteRequest, Version,
};

pub use pico_de_gallo_internal::{
    AdcChannel, AdcConfigurationInfo, Capabilities, DeviceInfo, GpioDirection, GpioEdge, GpioEvent, GpioPull,
    GpioState, I2cBatchOp, I2cFrequency, PwmConfigurationInfo, PwmDutyCycleInfo, SpiBatchOp, SpiConfigurationInfo,
    SpiPhase, SpiPolarity, UartConfigurationInfo, UartDataBits, UartParity, UartStopBits, VersionInfo,
};
pub use pico_de_gallo_internal::{
    AdcError, GpioError, I2cBatchError, I2cError, OneWireError, PwmError, SpiBatchError, SpiError, UartError,
};
pub use pico_de_gallo_internal::{
    BUILD_ID_CAPACITY, MAX_BATCH_OPS, MAX_REQUEST_FRAME, MAX_RESPONSE_PAYLOAD, MAX_TRANSFER_SIZE, NUM_GPIOS,
};
pub use pico_de_gallo_internal::{
    encode_i2c_batch_ops, encode_spi_batch_ops, i2c_batch_request_frame_len, i2c_batch_response_len,
    spi_batch_request_frame_len, spi_batch_response_len,
};

pub use postcard_rpc::host_client;
pub use postcard_rpc::host_client::HostErr;
pub use postcard_rpc::host_client::{IoClosed, MultiSubscription};
pub use postcard_rpc::standard_icd::WireError;
use postcard_rpc::{
    Endpoint,
    header::VarSeqKind,
    host_client::HostClient,
    postcard_schema::Schema,
    standard_icd::{ERROR_PATH, PingEndpoint},
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::convert::Infallible;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Upper bound on how long a validated `device/info` round-trip may take
/// before [`PicoDeGallo::validate`] gives up with [`ValidateError::Timeout`].
///
/// Firmware dispatch is serial: one legal 64-operation SPI batch made
/// entirely of `DelayNs { ns: u32::MAX }` can occupy the dispatcher for
/// roughly 275 seconds, and a `device/info` queued behind it would be
/// delayed by that much without anything being wrong. Five minutes leaves
/// about 25 seconds of headroom for dispatch and USB overhead, so a healthy
/// maximum-length batch never produces a false timeout, while still giving
/// operators a finite, comprehensible upper bound instead of an
/// indefinite wait.
///
/// This bound is separate from [`DEFAULT_CALL_TIMEOUT`] and deliberately far
/// larger, because a queued `device/info` is exactly the case it must not
/// misdiagnose.
pub const DEVICE_INFO_TIMEOUT: Duration = Duration::from_secs(300);

/// Default bound applied to every RPC that does not carry its own duration.
///
/// Without a bound, a request whose response is never produced parks the
/// caller forever: there is no lower layer that will give up. That is not
/// hypothetical — a request frame that exceeds the firmware's receive buffer
/// is dropped before the dispatcher ever sees it, so the device stays
/// perfectly healthy while the call waits indefinitely (issue #178).
///
/// Five seconds is roughly twice the slowest measured legitimate operation
/// (a 4096-byte 1-Wire read, ~2.4 s). Calls that can legitimately take longer
/// are **not** bounded by this value; they derive their own bound instead:
///
/// | Call | Bound |
/// |---|---|
/// | [`PicoDeGallo::i2c_scan`] | [`UNDECLARED_DISPATCH_BUDGET_MS`] + slack |
/// | [`PicoDeGallo::uart_read`] | caller's `timeout_ms` + slack |
/// | [`PicoDeGallo::onewire_write_pullup`] | caller's `pullup_duration_ms` + slack |
/// | `gpio_wait_*` | [`MAX_HANDLER_TIMEOUT_MS`] + slack |
/// | [`PicoDeGallo::spi_batch`] | sum of `DelayNs` operations + slack |
/// | [`PicoDeGallo::device_info`], [`PicoDeGallo::validate`] | [`DEVICE_INFO_TIMEOUT`] |
///
/// In every case the slack term is the configured call timeout itself, so
/// raising it with [`PicoDeGallo::with_call_timeout`] widens the derived
/// bounds proportionally.
///
/// # Concurrency caveat
///
/// Firmware dispatch is **serial**: one handler runs at a time. A call issued
/// while another is in flight waits for that one to finish *before* its own
/// handler starts, and the host cannot observe when that happens. So if you
/// share a [`PicoDeGallo`] across tasks and one of them issues a long call — a
/// `gpio_wait_*`, or an [`spi_batch`](PicoDeGallo::spi_batch) carrying large
/// `DelayNs` operations — concurrent calls can exceed this default through no
/// fault of the device.
///
/// Sequential use, which is the overwhelmingly common pattern and the one the
/// `embedded-hal` implementations produce, is unaffected. If you do drive a
/// board concurrently alongside long-running calls, raise the bound with
/// [`PicoDeGallo::with_call_timeout`].
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Description of a connected Pico de Gallo device.
#[derive(Debug, Clone)]
pub struct DeviceDescription {
    /// USB serial number (unique per board, derived from chip ID).
    pub serial_number: Option<String>,
    /// USB manufacturer string.
    pub manufacturer: Option<String>,
    /// USB product string.
    pub product: Option<String>,
}

/// List all connected Pico de Gallo devices.
///
/// Returns a description for each device found on the USB bus matching the
/// Pico de Gallo VID/PID. Use the serial number with
/// [`PicoDeGallo::new_with_serial_number`] to connect to a specific device.
pub fn list_devices() -> Vec<DeviceDescription> {
    let devices = match nusb::list_devices() {
        Ok(iter) => iter,
        Err(_) => return Vec::new(),
    };
    devices
        .filter(|dev| dev.vendor_id() == MICROSOFT_VID && dev.product_id() == PICO_DE_GALLO_PID)
        .map(|dev| DeviceDescription {
            serial_number: dev.serial_number().map(String::from),
            manufacturer: dev.manufacturer_string().map(String::from),
            product: dev.product_string().map(String::from),
        })
        .collect()
}

/// Error type for Pico de Gallo operations.
///
/// Every method on [`PicoDeGallo`] returns this error type, parameterized by the
/// endpoint-specific error `E`. In practice, `E` is a rich error enum like
/// [`I2cError`], [`SpiError`], or [`GpioError`].
#[derive(Debug)]
pub enum PicoDeGalloError<E> {
    /// A transport-level communication error (USB disconnect, timeout, wire format error).
    Comms(HostErr<WireError>),
    /// The request was rejected with an endpoint-specific error.
    ///
    /// Usually the firmware processed the request and returned this. A few
    /// requests are refused locally, before transmission, when the host can
    /// prove the firmware would reject them — a zero-length I2C write, for
    /// instance (issue #136). Those carry the identical error value the
    /// firmware would have returned, so callers need not distinguish the
    /// two, and must not assume this variant implies the device was
    /// contacted.
    Endpoint(E),
    /// The device did not answer within the bound for this call.
    ///
    /// The request was transmitted but no response arrived. This is
    /// **distinct** from [`Comms`](Self::Comms): the transport is healthy and
    /// the device is very likely healthy too. The known cause is a request
    /// frame large enough to be dropped before the firmware's dispatcher sees
    /// it, in which case the board keeps answering every other call normally
    /// (issue #178).
    ///
    /// The handle remains usable. Abandoning the call deregisters its reply
    /// waiter, and sequence numbers are never reused, so a late reply is
    /// discarded rather than mismatched to a later call.
    ///
    /// See [`DEFAULT_CALL_TIMEOUT`] for how each call's bound is chosen and
    /// for the concurrency caveat.
    Timeout {
        /// How long the call waited before giving up.
        waited: Duration,
    },
}

impl<E: core::fmt::Display> core::fmt::Display for PicoDeGalloError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Comms(e) => write!(f, "communication error: {e:?}"),
            Self::Endpoint(e) => write!(f, "endpoint error: {e}"),
            Self::Timeout { waited } => write!(f, "device did not respond within {:.3} s", waited.as_secs_f64()),
        }
    }
}

impl<E: core::fmt::Debug + core::fmt::Display> std::error::Error for PicoDeGalloError<E> {}

impl<E> From<HostErr<WireError>> for PicoDeGalloError<E> {
    fn from(value: HostErr<WireError>) -> Self {
        Self::Comms(value)
    }
}

/// Error returned by [`PicoDeGallo::validate()`] when the connected firmware
/// is incompatible with this host library.
#[derive(Debug)]
pub enum ValidateError {
    /// Could not communicate with the device (USB disconnect, timeout, etc.).
    Comms(HostErr<WireError>),
    /// The `device/info` round-trip did not complete within
    /// [`DEVICE_INFO_TIMEOUT`].
    ///
    /// Distinct from [`ValidateError::Comms`] on purpose: when the timeout
    /// expires postcard-rpc has produced no transport error at all — the
    /// request is simply still outstanding — so there is no [`HostErr`] to
    /// carry. Folding this into `Comms` would erase the only actionable
    /// distinction the caller has.
    Timeout,
    /// The firmware does not support the `device/info` endpoint (legacy firmware).
    LegacyFirmware,
    /// The schema (wire protocol) version does not match.
    ///
    /// The host and firmware were compiled against different versions of
    /// `pico-de-gallo-internal`. They must be upgraded together.
    SchemaMismatch {
        /// Schema major version expected by this host library.
        expected_major: u16,
        /// Schema major version reported by the firmware.
        actual_major: u16,
        /// Schema minor version expected by this host library.
        expected_minor: u16,
        /// Schema minor version reported by the firmware.
        actual_minor: u16,
    },
}

impl core::fmt::Display for ValidateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Comms(e) => write!(f, "communication error: {e:?}"),
            Self::Timeout => write!(
                f,
                "device/info did not respond within 300 seconds — either \
                 the board is unresponsive, or host and firmware were built \
                 from different trees, in which case the response is sent \
                 under a different endpoint key and silently dropped. This \
                 host cannot tell the two apart"
            ),
            Self::LegacyFirmware => write!(
                f,
                "firmware does not support the device/info endpoint — upgrade firmware"
            ),
            Self::SchemaMismatch {
                expected_major,
                actual_major,
                expected_minor,
                actual_minor,
            } => write!(
                f,
                "schema version mismatch: host expects \
                 {expected_major}.{expected_minor}.x but firmware reports \
                 {actual_major}.{actual_minor}.x — upgrade both together"
            ),
        }
    }
}

impl std::error::Error for ValidateError {}

/// Map a [`HostErr`] surfaced by [`PicoDeGallo::validate`] to a
/// [`ValidateError`].
///
/// Policy: only the two `WireError` variants that postcard-rpc emits when
/// the server has no handler for an endpoint key —
/// [`WireError::UnknownKey`] and [`WireError::KeyTooSmall`] — are treated
/// as a *legacy-firmware* signal. Every other variant (including
/// [`WireError::DeserFailed`], frame-size errors, host-side
/// [`HostErr::BadResponse`], [`HostErr::Postcard`], and
/// [`HostErr::Closed`]) is a real comms/protocol fault and routes to
/// [`ValidateError::Comms`] so users do not chase a non-existent firmware
/// upgrade.
///
/// In particular, `DeserFailed` is *not* `LegacyFirmware`: a legacy
/// firmware that lacks the endpoint will reply with `UnknownKey`, not a
/// deserialization failure. A `DeserFailed` here means the firmware
/// reached the handler but our request/response shape disagrees — that
/// is a comms-layer / wire-schema bug, not a missing endpoint.
///
/// The wildcard arm is deliberate so that future additions to either
/// `HostErr` or `WireError` (both `#[non_exhaustive]`-adjacent in
/// practice) default to the safer `Comms` classification.
fn map_validate_error(e: HostErr<WireError>) -> ValidateError {
    match &e {
        HostErr::Wire(WireError::UnknownKey) | HostErr::Wire(WireError::KeyTooSmall) => ValidateError::LegacyFirmware,
        _ => ValidateError::Comms(e),
    }
}

/// Returns `Ok(())` if the firmware-reported schema in `info` is
/// compatible with the host's compiled-in
/// [`SCHEMA_VERSION_MAJOR`] / [`SCHEMA_VERSION_MINOR`].
///
/// Both major and minor are checked. Pre-1.0, the minor is the
/// breaking-change axis (per AGENTS.md §6.2); the major is also
/// checked so that any future 1.0+ bump is caught immediately rather
/// than silently mis-decoding wire bytes against a host on 0.x.
/// Closes Category A finding #1.
///
/// Extracted from [`PicoDeGallo::validate`] so the policy is
/// independently testable.
fn check_schema_compatible(info: &DeviceInfo) -> Result<(), ValidateError> {
    if info.schema_major != SCHEMA_VERSION_MAJOR || info.schema_minor != SCHEMA_VERSION_MINOR {
        return Err(ValidateError::SchemaMismatch {
            expected_major: SCHEMA_VERSION_MAJOR,
            actual_major: info.schema_major,
            expected_minor: SCHEMA_VERSION_MINOR,
            actual_minor: info.schema_minor,
        });
    }
    Ok(())
}

/// Returns `Err(I2cError::ZeroLengthWrite)` if `contents` is empty.
///
/// The RP2040/RP2350 `DW_apb_i2c` block cannot emit an address-only
/// transaction: the address phase is driven solely by pushing at least one
/// byte into `IC_DATA_CMD`, so a zero-length write is not merely
/// unsupported but physically unreachable. Forwarding one to firmware
/// built before schema 0.7 wedges the device dispatcher outright — every
/// endpoint, not just I2C — until USB re-enumeration (issue #101).
///
/// Current firmware refuses it, so this guard is not what keeps the device
/// alive. It exists so the refusal costs no USB round-trip, and so every
/// host surface reports the same thing whatever firmware is attached.
/// Probe an address with a 1-byte read or with `i2c/scan` instead.
///
/// `i2c/write-read` is deliberately **not** guarded: an empty write phase
/// there is legal and useful, because that transfer does not terminate
/// with a STOP and so returns rather than parking.
///
/// Extracted so the policy is independently testable (issue #136).
fn check_i2c_write_payload(contents: &[u8]) -> Result<(), I2cError> {
    if contents.is_empty() {
        return Err(I2cError::ZeroLengthWrite);
    }
    Ok(())
}

/// `true` when a response carrying `len` bytes cannot be delivered to the
/// host.
///
/// See [`MAX_RESPONSE_PAYLOAD`] for the byte-by-byte derivation. This is a
/// property of the host transport, not of the firmware, so it is the one
/// bound the device cannot be relied upon to apply for us.
///
/// Applies to any length the caller asks the device to *send back*: a plain
/// read count, the read half of `i2c/write-read`, the (duplex) length of an
/// `spi/transfer`, and the aggregate of a batch's returning operations.
/// Issue #179 covered only the batch aggregate, because a batch commits
/// `Write` operations before losing its response and so corrupts state; the
/// plain endpoints commit nothing, but still report the overflow as
/// `Comms(Postcard(DeserializeUnexpectedEnd))`, which names the transport
/// rather than the argument at fault. Issue #158 extended it to the rest.
///
/// Extracted so the policy is independently testable (issue #136).
fn response_len_is_undeliverable(len: usize) -> bool {
    len > MAX_RESPONSE_PAYLOAD
}

/// `true` when a request payload of `len` bytes exceeds what the firmware
/// accepts in one transaction.
///
/// [`MAX_TRANSFER_SIZE`] bounds the *outbound* direction and is far looser
/// than [`response_len_is_undeliverable`]'s ceiling; the two budgets are
/// independent, which the #158 triage established by showing that a
/// 1021-byte request payload did not move the response ceiling by a single
/// byte. That figure is inherited; nothing here was re-measured.
/// Applies to every payload the caller *sends*: `i2c/write`, `spi/write`,
/// `uart/write`, both 1-Wire writes, and the write half of
/// `i2c/write-read`.
///
/// A duplex `spi/transfer` is both directions at once and so must satisfy
/// this *and* the response ceiling; since the latter is strictly tighter
/// (asserted by a `const _` beside that function)
/// checking the response ceiling alone suffices there.
///
/// Extracted so the policy is independently testable (issue #136).
fn request_payload_is_too_long(len: usize) -> bool {
    len > MAX_TRANSFER_SIZE
}

// `spi_transfer` checks only the response ceiling, on the grounds that it is
// the tighter of the two. That reasoning is only sound while this holds, so
// make an inversion a build failure rather than something a test has to
// catch — the same treatment the derivation itself gets in
// `pico-de-gallo-internal`.
const _: () = assert!(
    MAX_RESPONSE_PAYLOAD < MAX_TRANSFER_SIZE,
    "spi_transfer's single bound assumes the response ceiling binds first"
);

/// `true` when a request frame of `len` bytes will not reach the firmware.
///
/// The send-direction mirror of [`response_len_is_undeliverable`], and the
/// worse of the two to leave unchecked in one specific way: an
/// over-ceiling *response* still gets an error back, whereas an
/// over-ceiling *request* is discarded by postcard-rpc's `receive()`
/// before any handler runs and is answered by nothing at all. The caller
/// waits out its own bound and is told the device did not respond, which
/// names neither the argument at fault nor a size that would work.
///
/// Unlike the other two predicates this bounds the whole *frame* — header
/// included — not a payload, because the overhead around a payload is not
/// a constant: two postcard varints in the batch request widen with the
/// values they carry. [`MAX_REQUEST_FRAME`] and the
/// `*_batch_request_frame_len` helpers derive both halves term by term.
///
/// Only the batch endpoints need this. Since #158 every single-argument
/// write is capped at [`MAX_TRANSFER_SIZE`], which leaves the worst-case
/// frame a kilobyte clear of the ceiling — an ordering asserted by a
/// `const _` in `pico-de-gallo-internal` so it cannot rot into being
/// false. A batch's aggregate outgoing bytes were bounded by nothing,
/// which made it the one remaining route into the ceiling from a supported
/// host surface (issue #186).
///
/// Extracted so the policy is independently testable (issue #136).
fn request_frame_is_undeliverable(len: usize) -> bool {
    len > MAX_REQUEST_FRAME
}

/// Returns `Err` naming the first [`I2cBatchOp::Write`] in `ops` that
/// carries an empty payload.
///
/// Validates the whole list before anything is transmitted, mirroring the
/// firmware, which validates a batch up front rather than mid-execution so
/// that a rejected batch never drives its earlier operations onto the bus.
///
/// `failed_op` carries the offending operation's exact index. This matches
/// the firmware's contract for *validation* errors; only *bus* errors
/// collapse to `failed_op = 0`, because an atomic transaction fails as a
/// unit (issue #128).
///
/// Also refuses a batch whose reads total more than
/// [`MAX_RESPONSE_PAYLOAD`], with `failed_op = 0`: an aggregate overflow is
/// not attributable to any one operation, and the firmware reports it the
/// same way (`handlers/i2c.rs`). The aggregate is checked *after* the
/// per-operation walk so that a batch which is wrong in both ways reports
/// the same error here as it would on the device.
///
/// Finally refuses a batch whose outgoing bytes cannot fit one request
/// frame, also with `failed_op = 0` and also as `BufferTooLong`, so the
/// two aggregate overflows are indistinguishable to a caller and their
/// relative order is unobservable. Unlike the read aggregate this one has
/// no firmware counterpart to agree with — the frame never arrives, so
/// there is no device behaviour to match — which is exactly why it has to
/// be here (issue #186).
///
/// Extracted so the policy is independently testable (issue #136).
fn check_i2c_batch_ops(ops: &[I2cBatchOp<'_>]) -> Result<(), I2cBatchError> {
    for (i, op) in ops.iter().enumerate() {
        if let I2cBatchOp::Write { data } = op
            && let Err(kind) = check_i2c_write_payload(data)
        {
            return Err(I2cBatchError {
                failed_op: i as u16,
                kind,
            });
        }
    }
    if response_len_is_undeliverable(i2c_batch_response_len(ops))
        || request_frame_is_undeliverable(i2c_batch_request_frame_len(ops))
    {
        return Err(I2cBatchError {
            failed_op: 0,
            kind: I2cError::BufferTooLong,
        });
    }
    Ok(())
}

/// Returns `Err` if `ops` will not fit through the transport in either
/// direction: if the bytes they return exceed [`MAX_RESPONSE_PAYLOAD`], or
/// if the bytes they send exceed [`MAX_REQUEST_FRAME`].
///
/// Coming back, `Read` and `Transfer` count and `Write` and `DelayNs` do
/// not. Going out, every operation costs its encoding and `Write` and
/// `Transfer` additionally cost their payloads. `failed_op` is `0` for
/// both, because an aggregate overflow is not attributable to any one
/// operation — the same convention the firmware uses at the matching check
/// in `handlers/spi.rs`.
///
/// Both checks precede the `device/info` round-trip in
/// [`PicoDeGallo::spi_batch`], since neither needs device metadata and a
/// batch that cannot work should not cost one.
///
/// Extracted so the policy is independently testable (issues #179, #186).
fn check_spi_batch_ops(ops: &[SpiBatchOp<'_>]) -> Result<(), SpiBatchError> {
    if response_len_is_undeliverable(spi_batch_response_len(ops))
        || request_frame_is_undeliverable(spi_batch_request_frame_len(ops))
    {
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::BufferTooLong,
        });
    }
    Ok(())
}

/// Error returned by [`PicoDeGallo::spi_batch`].
///
/// The five variants are deliberately disjoint so that a caller — and every
/// downstream host surface — can tell a *local* refusal of the chip-select
/// argument apart from a *failure to learn what the valid range even is*.
/// Misreporting a metadata failure as an invalid chip-select would send
/// users hunting for a bug in their own arguments; see issue #104.
///
/// Not `#[non_exhaustive]`: appending a variant must break every exhaustive
/// match so each host surface makes a deliberate decision about how the new
/// case reaches its callers.
#[derive(Debug)]
pub enum SpiBatchCallError {
    /// The device-reported GPIO count could not be established.
    ///
    /// Carries the exact [`ValidateError`] — transport, timeout, legacy
    /// firmware, or schema mismatch. **Never** a chip-select complaint.
    DeviceInfo(ValidateError),
    /// The device successfully reported that it exposes zero GPIOs, so no
    /// chip-select pin exists. Distinct from [`Self::InvalidCsPin`]: the
    /// caller's index is not the problem.
    NoGpios,
    /// The requested chip-select index is at or beyond the device-reported
    /// GPIO count. Refused locally: no `spi/batch` RPC is sent.
    InvalidCsPin {
        /// The chip-select index the caller supplied, verbatim.
        cs: u8,
        /// The device-reported GPIO count that `cs` was checked against.
        num_gpios: u8,
    },
    /// A transport-level failure of the `spi/batch` request itself, after
    /// the chip-select was accepted.
    Comms(HostErr<WireError>),
    /// The device did not answer the `spi/batch` request within its bound.
    ///
    /// Disjoint from [`Self::Comms`] for the same reason the other variants
    /// are: the transport is healthy and the batch's fate is *unknown*. It
    /// may have executed fully, partially, or not at all, so unlike a local
    /// refusal it must not be retried blindly.
    ///
    /// See [`PicoDeGalloError::Timeout`] and [`DEFAULT_CALL_TIMEOUT`].
    Timeout {
        /// How long the call waited before giving up.
        waited: Duration,
    },
    /// The firmware executed the batch and refused or failed an operation.
    Endpoint(SpiBatchError),
}

impl core::fmt::Display for SpiBatchCallError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DeviceInfo(e) => write!(f, "failed to determine num_gpios: {e}"),
            Self::NoGpios => write!(f, "device reports num_gpios=0; no SPI chip-select pin is available"),
            Self::InvalidCsPin { cs, num_gpios } => write!(
                f,
                "invalid SPI chip-select pin {cs}; device reports {num_gpios} GPIOs (valid 0..{num_gpios})"
            ),
            Self::Comms(e) => write!(f, "communication error: {e:?}"),
            Self::Timeout { waited } => write!(
                f,
                "device did not respond to spi/batch within {:.3} s; the batch may or may not have executed",
                waited.as_secs_f64()
            ),
            Self::Endpoint(e) => write!(f, "endpoint error: {e}"),
        }
    }
}

impl std::error::Error for SpiBatchCallError {}

/// Total firmware-side time a SPI batch's `DelayNs` operations can consume,
/// in milliseconds, saturating at [`MAX_HANDLER_TIMEOUT_MS`].
///
/// `DelayNs` is caller-supplied, so a legal 64-operation batch of
/// `DelayNs { ns: u32::MAX }` occupies the dispatcher for roughly 275
/// seconds. Bounding such a batch by the default call timeout would report a
/// perfectly healthy device as unresponsive, so the host's bound has to be
/// derived from the batch itself.
///
/// Only `DelayNs` is counted. The bus time of the other operations is bounded
/// by the transfer size and is negligible beside a caller-chosen delay; the
/// call's slack term absorbs it.
fn spi_batch_delay_budget_ms(ops: &[SpiBatchOp<'_>]) -> u32 {
    let total_ns: u64 = ops
        .iter()
        .map(|op| match op {
            SpiBatchOp::DelayNs { ns } => u64::from(*ns),
            _ => 0,
        })
        .sum();
    // Round up: a sub-millisecond delay still deserves a non-zero budget.
    let ms = total_ns.div_ceil(1_000_000);
    u32::try_from(ms).unwrap_or(u32::MAX).min(MAX_HANDLER_TIMEOUT_MS)
}

/// Classify a chip-select index against a device-reported GPIO count.///
/// `num_gpios` must come from an `Ok(_)` metadata read — never from a
/// default, a cast, or the compile-time [`NUM_GPIOS`]. A count of zero is
/// [`SpiBatchCallError::NoGpios`] for *every* index, including zero, so a
/// board that genuinely exposes no GPIOs is diagnosable as exactly that
/// rather than as an ordinary out-of-range index.
fn classify_cs(cs: u8, num_gpios: u8) -> Result<(), SpiBatchCallError> {
    if num_gpios == 0 {
        return Err(SpiBatchCallError::NoGpios);
    }
    if cs >= num_gpios {
        return Err(SpiBatchCallError::InvalidCsPin { cs, num_gpios });
    }
    Ok(())
}

///
/// This is the primary type for interacting with the hardware. It wraps a
/// [`postcard_rpc::host_client::HostClient`] and provides typed async methods
/// for every firmware endpoint. The client is cheaply cloneable (the inner
/// transport is reference-counted) and safe to share across tasks.
///
/// The USB device is enumerated when the client is constructed: [`new`] and
/// [`new_with_serial_number`] **panic** if no matching device is present or
/// the interface cannot be claimed. Use the fallible [`try_new`] /
/// [`try_new_with_serial_number`] variants to handle those cases. Once
/// constructed, the connection handshake completes in the background, so
/// per-RPC calls fail (rather than the constructor) if the link drops later.
///
/// [`new`]: Self::new
/// [`new_with_serial_number`]: Self::new_with_serial_number
/// [`try_new`]: Self::try_new
/// [`try_new_with_serial_number`]: Self::try_new_with_serial_number
#[derive(Clone)]
pub struct PicoDeGallo {
    client: HostClient<WireError>,
    /// Device-reported GPIO count, populated only after a successful,
    /// timeout-bounded, schema-checked `device/info`.
    ///
    /// Shared by clones so a warm cache is not re-fetched per handle. A
    /// failed fetch leaves it empty, so the next call retries. `std`'s
    /// `OnceLock` rather than a Tokio cell: it is runtime-independent and
    /// does not depend on a Tokio feature we only get transitively.
    num_gpios_cache: Arc<OnceLock<u8>>,
    /// Per-handle bound on the validated metadata fetch. Production
    /// constructors set this to [`DEVICE_INFO_TIMEOUT`]; the private
    /// test constructor uses a short one so the timeout path is
    /// executable without waiting five minutes.
    metadata_timeout: Duration,
    /// Per-handle bound on every RPC that does not derive its own. Defaults
    /// to [`DEFAULT_CALL_TIMEOUT`]; override with
    /// [`with_call_timeout`](Self::with_call_timeout).
    call_timeout: Duration,
}

/// A [`HostClient`] paired with the bound to apply to one call.
///
/// Exists so every RPC in this crate goes through a single place that can
/// time out, rather than 50-odd call sites each having to remember to. See
/// [`PicoDeGallo::bounded`].
struct Bounded<'a> {
    client: &'a HostClient<WireError>,
    bound: Duration,
}

impl Bounded<'_> {
    /// Issue an endpoint request, giving up after the bound.
    ///
    /// Shadows [`HostClient::send_resp`] deliberately: call sites read
    /// identically to the unbounded version, so the bound cannot be
    /// forgotten by writing the call the "obvious" way.
    ///
    /// Returns [`CallError`] rather than [`PicoDeGalloError`] so the method
    /// has exactly one type parameter and call sites can keep writing
    /// `send_resp::<SomeEndpoint>(..)`. The `?` operator converts it.
    async fn send_resp<E>(&self, request: &E::Request) -> Result<E::Response, CallError>
    where
        E: Endpoint,
        E::Request: Serialize + Schema,
        E::Response: DeserializeOwned + Schema,
    {
        match tokio::time::timeout(self.bound, self.client.send_resp::<E>(request)).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(CallError::Comms(e)),
            Err(_elapsed) => Err(CallError::Timeout { waited: self.bound }),
        }
    }
}

/// Outcome of a bounded transport call, before it is widened to the
/// endpoint-specific [`PicoDeGalloError`].
enum CallError {
    Comms(HostErr<WireError>),
    Timeout { waited: Duration },
}

impl<E> From<CallError> for PicoDeGalloError<E> {
    fn from(value: CallError) -> Self {
        match value {
            CallError::Comms(e) => Self::Comms(e),
            CallError::Timeout { waited } => Self::Timeout { waited },
        }
    }
}

impl Default for PicoDeGallo {
    fn default() -> Self {
        Self::new()
    }
}

impl PicoDeGallo {
    /// Create a new instance for the Pico de Gallo device.
    ///
    /// NOTICE:
    ///
    /// This constructor will return the first matching device in case
    /// there are more than one connected.
    ///
    /// If you want more control, please use `new_with_serial_number`
    /// instead.
    pub fn new() -> Self {
        Self::new_inner(|dev| dev.vendor_id() == MICROSOFT_VID && dev.product_id() == PICO_DE_GALLO_PID)
    }

    /// Create a new instance for the Pico de Gallo device with the
    /// given serial number.
    pub fn new_with_serial_number(serial_number: &str) -> Self {
        Self::new_inner(|dev| {
            dev.vendor_id() == MICROSOFT_VID
                && dev.product_id() == PICO_DE_GALLO_PID
                && dev.serial_number() == Some(serial_number)
        })
    }

    /// Fallible variant of [`new`](Self::new): returns an error instead of
    /// panicking when no matching device is present or the interface cannot
    /// be claimed.
    pub fn try_new() -> Result<Self, String> {
        Self::try_new_inner(|dev| dev.vendor_id() == MICROSOFT_VID && dev.product_id() == PICO_DE_GALLO_PID)
    }

    /// Fallible variant of [`new_with_serial_number`](Self::new_with_serial_number).
    pub fn try_new_with_serial_number(serial_number: &str) -> Result<Self, String> {
        Self::try_new_inner(|dev| {
            dev.vendor_id() == MICROSOFT_VID
                && dev.product_id() == PICO_DE_GALLO_PID
                && dev.serial_number() == Some(serial_number)
        })
    }

    fn try_new_inner<F: FnMut(&NusbDeviceInfo) -> bool>(func: F) -> Result<Self, String> {
        let client = HostClient::try_new_raw_nusb(func, ERROR_PATH, 8, VarSeqKind::Seq2)?;
        Ok(Self {
            client,
            num_gpios_cache: Arc::new(OnceLock::new()),
            metadata_timeout: DEVICE_INFO_TIMEOUT,
            call_timeout: DEFAULT_CALL_TIMEOUT,
        })
    }

    /// Override the bound applied to RPCs that do not derive their own.
    ///
    /// Defaults to [`DEFAULT_CALL_TIMEOUT`]. Raising it also widens the
    /// derived bounds, which all add the configured value as transport slack.
    ///
    /// Raise this if you share one handle across tasks while issuing long
    /// calls — see the concurrency caveat on [`DEFAULT_CALL_TIMEOUT`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pico_de_gallo_lib::PicoDeGallo;
    /// use std::time::Duration;
    ///
    /// let pg = PicoDeGallo::try_new()?.with_call_timeout(Duration::from_secs(30));
    /// # Ok::<(), String>(())
    /// ```
    #[must_use]
    pub fn with_call_timeout(mut self, timeout: Duration) -> Self {
        self.call_timeout = timeout;
        self
    }

    /// The bound currently applied to undifferentiated RPCs.
    #[must_use]
    pub fn call_timeout(&self) -> Duration {
        self.call_timeout
    }

    /// Wrap the transport with this handle's default bound.
    fn bounded(&self) -> Bounded<'_> {
        Bounded {
            client: &self.client,
            bound: self.call_timeout,
        }
    }

    /// Wrap the transport with a bound derived from a firmware-side duration.
    ///
    /// `firmware_ms` is what the firmware itself will allow the handler, so
    /// the host must wait at least that long plus transport slack. The slack
    /// is the configured call timeout, and the firmware-side term is clamped
    /// exactly as the firmware clamps it, so the two cannot disagree.
    ///
    /// A `firmware_ms` of `0` means the firmware's ceiling, not "no time" —
    /// see [`MAX_HANDLER_TIMEOUT_MS`]. Getting this backwards would bound a
    /// `gpio/wait-*` at the transport slack alone and fail every edge that
    /// took longer than a few seconds to arrive.
    fn bounded_for(&self, firmware_ms: u32) -> Bounded<'_> {
        let clamped_ms = if firmware_ms == 0 {
            MAX_HANDLER_TIMEOUT_MS
        } else {
            firmware_ms.min(MAX_HANDLER_TIMEOUT_MS)
        };
        Bounded {
            client: &self.client,
            bound: Duration::from_millis(u64::from(clamped_ms)).saturating_add(self.call_timeout),
        }
    }

    /// Build a handle over a caller-supplied transport with a caller-supplied
    /// metadata timeout.
    ///
    /// Test-only seam: it is the only way to exercise the real public
    /// [`spi_batch`](Self::spi_batch) / [`num_gpios`](Self::num_gpios)
    /// paths — including the timeout — without opening USB or waiting
    /// [`DEVICE_INFO_TIMEOUT`].
    #[cfg(test)]
    pub(crate) fn new_for_test(client: HostClient<WireError>, metadata_timeout: Duration) -> Self {
        Self {
            client,
            num_gpios_cache: Arc::new(OnceLock::new()),
            metadata_timeout,
            call_timeout: DEFAULT_CALL_TIMEOUT,
        }
    }

    fn new_inner<F: FnMut(&NusbDeviceInfo) -> bool>(func: F) -> Self {
        Self::try_new_inner(func).expect("should have found nusb device")
    }

    /// Wait until the client has closed the connection.
    pub async fn wait_closed(&self) {
        self.client.wait_closed().await;
    }

    /// Ping endpoint.
    ///
    /// Only used for testing purposes. Send a `u32` and get the same
    /// `u32` as a response.
    pub async fn ping(&self, id: u32) -> Result<u32, PicoDeGalloError<Infallible>> {
        Ok(self.bounded().send_resp::<PingEndpoint>(&id).await?)
    }

    /// Read `count` bytes from the I2C device at `address`.
    ///
    /// # Response ceiling
    ///
    /// `count` may be at most [`MAX_RESPONSE_PAYLOAD`] (1014). A larger read
    /// is refused locally, before anything is transmitted, with
    /// [`I2cError::BufferTooLong`].
    ///
    /// That is far below [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096), which bounds the firmware's scratch buffer rather than what a
    /// response frame can carry. Before issue #158 this method documented
    /// 4096 and enforced nothing: a read of 1015..=4096 bytes was accepted,
    /// the bus was clocked, and the reply was then truncated in transport,
    /// surfacing as `Comms(Postcard(DeserializeUnexpectedEnd))` — an error
    /// that names the transport rather than the argument at fault.
    pub async fn i2c_read(&self, address: u8, count: u16) -> Result<Vec<u8>, PicoDeGalloError<I2cError>> {
        if response_len_is_undeliverable(usize::from(count)) {
            return Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<I2cRead>(&I2cReadRequest { address, count })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the I2C device at `address`.
    ///
    /// An empty payload is refused with [`I2cError::ZeroLengthWrite`]; see
    /// [`check_i2c_write_payload`] for why that one is a wedge rather than
    /// merely an error. A payload above
    /// [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`] (4096) is refused with
    /// [`I2cError::BufferTooLong`]. Both refusals are local and cost no USB
    /// round-trip.
    pub async fn i2c_write(&self, address: u8, contents: &[u8]) -> Result<(), PicoDeGalloError<I2cError>> {
        check_i2c_write_payload(contents).map_err(PicoDeGalloError::Endpoint)?;
        if request_payload_is_too_long(contents.len()) {
            return Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<I2cWrite>(&I2cWriteRequest { address, contents })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the I2C device at `address` and read back `count` bytes.
    ///
    /// # Ceilings
    ///
    /// The two halves are bounded independently, because the request and
    /// response budgets are independent — #158 measured a 1021-byte request
    /// payload leaving the response ceiling unmoved:
    ///
    /// * `count` may be at most [`MAX_RESPONSE_PAYLOAD`] (1014);
    /// * `contents` may be at most
    ///   [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`] (4096).
    ///
    /// Either overflow is refused locally with [`I2cError::BufferTooLong`]
    /// before anything is transmitted. An *empty* `contents` remains legal
    /// and is deliberately not refused: that transfer does not terminate
    /// with a STOP, so it returns rather than parking, and it is the
    /// idiomatic way to probe an address.
    pub async fn i2c_write_read(
        &self,
        address: u8,
        contents: &[u8],
        count: u16,
    ) -> Result<Vec<u8>, PicoDeGalloError<I2cError>> {
        if response_len_is_undeliverable(usize::from(count)) || request_payload_is_too_long(contents.len()) {
            return Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<I2cWriteRead>(&I2cWriteReadRequest {
                address,
                contents,
                count,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Scan the I2C bus and return the addresses of all responding devices.
    ///
    /// The firmware probes each 7-bit address by attempting a 1-byte read.
    /// Addresses that ACK are returned in ascending order. When
    /// `include_reserved` is `false`, only the standard range (0x08–0x77) is
    /// probed; when `true`, the full range (0x00–0x7F) is scanned.
    pub async fn i2c_scan(&self, include_reserved: bool) -> Result<Vec<u8>, PicoDeGalloError<I2cError>> {
        // i2c/scan probes up to 128 addresses; the firmware's own budget
        // for it is UNDECLARED_DISPATCH_BUDGET_MS, well above the default.
        self.bounded_for(UNDECLARED_DISPATCH_BUDGET_MS)
            .send_resp::<I2cScan>(&I2cScanRequest { include_reserved })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Execute a batch of I2C operations as a single bus transaction.
    ///
    /// Pass a slice of [`I2cBatchOp`] values directly — they are encoded
    /// internally. On success, returns the concatenated read data from
    /// all Read operations in order.
    ///
    /// # Bus semantics
    ///
    /// The whole batch is one I2C transaction, matching the
    /// `embedded-hal` [`I2c::transaction`] contract:
    ///
    /// - a START and address precede the first operation;
    /// - adjacent operations of the same type are sent back to back with
    ///   no STOP and no repeated START between them, so two adjacent
    ///   `Write` ops form a **single gather write** rather than two
    ///   separate writes;
    /// - a direction change emits a repeated START and a re-addressing;
    /// - a STOP follows the last operation, and only the last one.
    ///
    /// The gather-write rule is the one to keep in mind when porting
    /// code that used to issue the operations separately: a register
    /// address followed by its payload is the intended idiom, but two
    /// logically independent writes will be concatenated into one, not
    /// framed as two.
    ///
    /// # Firmware requirement
    ///
    /// The atomic framing above requires firmware built from schema 0.7
    /// or newer. Older firmware executes each operation as its own
    /// transaction, with a STOP after every one.
    ///
    /// The batch travels as a single request, so it is also one USB
    /// round-trip instead of one per operation.
    ///
    /// # Response ceiling
    ///
    /// The `Read` operations may total at most [`MAX_RESPONSE_PAYLOAD`]
    /// bytes. A larger batch is refused locally, before anything is
    /// transmitted, with `I2cBatchError { failed_op: 0, kind: BufferTooLong }`.
    /// `Write` payloads are not counted — nothing comes back from them.
    ///
    /// The bound is on what the transport can deliver, not on what the
    /// firmware can buffer, so it is far below
    /// [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]. Before issue #179 such
    /// a batch was accepted and *executed*, and only then lost its response
    /// to transport truncation, which surfaced as
    /// `Comms(Postcard(DeserializeUnexpectedEnd))`.
    ///
    /// # Request-frame ceiling
    ///
    /// The whole request must also fit one frame: at most
    /// [`MAX_REQUEST_FRAME`] bytes, header and encoding overhead included.
    /// Use [`i2c_batch_request_frame_len`] to size a batch in advance. An
    /// over-ceiling batch is refused locally with the same
    /// `I2cBatchError { failed_op: 0, kind: BufferTooLong }` as the response
    /// overflow, since neither is attributable to one operation and both
    /// are aggregate length faults.
    ///
    /// This bounds the aggregate, not any single operation, so a batch of
    /// individually modest writes can still overrun it — the hardware
    /// measurement in issue #186 tripped it with eight 635-byte writes.
    /// Nothing else bounds a batch's outgoing bytes: unlike
    /// [`Self::i2c_write`], an individual batch `Write` is not capped at
    /// `MAX_TRANSFER_SIZE`, because its payload streams straight out of the
    /// received frame and never enters the firmware's scratch buffer.
    ///
    /// Before issue #186 an over-ceiling batch was discarded by the
    /// firmware's receiver before any handler ran, so nothing was sent back
    /// at all and the call failed with [`PicoDeGalloError::Timeout`] — a
    /// message about the device not answering, for what is really an
    /// argument that is too big.
    ///
    /// [`I2c::transaction`]: https://docs.rs/embedded-hal/1.0.0/embedded_hal/i2c/trait.I2c.html#tymethod.transaction
    pub async fn i2c_batch(
        &self,
        address: u8,
        ops: &[I2cBatchOp<'_>],
    ) -> Result<Vec<u8>, PicoDeGalloError<I2cBatchError>> {
        check_i2c_batch_ops(ops).map_err(PicoDeGalloError::Endpoint)?;
        let encoded = encode_i2c_batch_ops(ops);
        self.bounded()
            .send_resp::<I2cBatch>(&I2cBatchRequest {
                address,
                count: ops.len() as u16,
                ops: &encoded,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Read `count` bytes from the SPI bus.
    ///
    /// # Response ceiling
    ///
    /// `count` may be at most [`MAX_RESPONSE_PAYLOAD`] (1014). A larger read
    /// is refused locally, before anything is transmitted, with
    /// [`SpiError::BufferTooLong`], rather than clocking the bus and then
    /// losing the reply to transport truncation. See
    /// [`response_len_is_undeliverable`].
    pub async fn spi_read(&self, count: u16) -> Result<Vec<u8>, PicoDeGalloError<SpiError>> {
        if response_len_is_undeliverable(usize::from(count)) {
            return Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<SpiRead>(&SpiReadRequest { count })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the SPI bus.
    ///
    /// `contents` may be at most
    /// [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`] (4096) bytes; more is
    /// refused locally with [`SpiError::BufferTooLong`].
    ///
    /// Note this is the *looser* of the two ceilings, deliberately. A write
    /// returns no payload, so the response budget does not apply — #158
    /// triage recorded `spi/write` completing normally at 1015 bytes, the
    /// size at which [`Self::spi_transfer`] fails, so the two endpoints
    /// cannot share one bound. That figure is inherited from that run.
    pub async fn spi_write(&self, contents: &[u8]) -> Result<(), PicoDeGalloError<SpiError>> {
        if request_payload_is_too_long(contents.len()) {
            return Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<SpiWrite>(&SpiWriteRequest { contents })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Flush the SPI interface.
    pub async fn spi_flush(&self) -> Result<(), PicoDeGalloError<SpiError>> {
        self.bounded()
            .send_resp::<SpiFlush>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Perform a full-duplex SPI transfer.
    ///
    /// Simultaneously sends `write_data` and receives the same number of
    /// bytes.
    ///
    /// # Response ceiling
    ///
    /// `write_data` may be at most [`MAX_RESPONSE_PAYLOAD`] (1014) bytes —
    /// **not** [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`], which this
    /// method previously documented. Because the transfer is full duplex,
    /// its single argument is simultaneously the request payload and the
    /// response length, so the tighter of the two ceilings binds. A larger
    /// transfer is refused locally with [`SpiError::BufferTooLong`].
    ///
    /// This is the asymmetry worth remembering: [`Self::spi_write`] accepts
    /// four times as much, because it asks for nothing back.
    pub async fn spi_transfer(&self, write_data: &[u8]) -> Result<Vec<u8>, PicoDeGalloError<SpiError>> {
        // The response ceiling is strictly tighter than the request ceiling
        // (asserted by the `const _` beside `request_payload_is_too_long`),
        // so checking it alone also satisfies `request_payload_is_too_long`.
        if response_len_is_undeliverable(write_data.len()) {
            return Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<SpiTransfer>(&SpiTransferRequest { contents: write_data })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Execute a batch of SPI operations atomically under chip-select.
    ///
    /// Pass a slice of [`SpiBatchOp`] values directly — they are encoded
    /// internally. The firmware asserts CS on `cs_pin` before the first
    /// operation and deasserts it after the last (or on error). On success,
    /// returns concatenated data from all Read and Transfer operations
    /// in order.
    ///
    /// # Response ceiling
    ///
    /// The `Read` and `Transfer` operations may return at most
    /// [`MAX_RESPONSE_PAYLOAD`] bytes in total. A larger batch is refused
    /// locally with `SpiBatchError { failed_op: 0, kind: BufferTooLong }`.
    /// `Write` and `DelayNs` do not count — nothing comes back from them.
    ///
    /// This is checked **first**, before the chip-select preflight, because
    /// it needs no device metadata: a batch that can never deliver its
    /// response should not cost a `device/info` round-trip to refuse. It
    /// also matches the firmware, which walks and bounds the operations
    /// before it looks at `cs_pin`, so a batch that is wrong in both ways
    /// reports the same error whichever side refuses it.
    ///
    /// The bound is on what the transport can deliver, not on what the
    /// firmware can buffer, so it is far below
    /// [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]. Before issue #179 such
    /// a batch was accepted and *executed* — chip-select asserted and
    /// deasserted, clocks driven — and only then lost its response to
    /// transport truncation.
    ///
    /// # Request-frame ceiling
    ///
    /// The whole request must also fit one frame: at most
    /// [`MAX_REQUEST_FRAME`] bytes, header and encoding overhead included.
    /// `Write` and `Transfer` payloads both travel out and both count;
    /// `Read` and `DelayNs` cost only their few bytes of encoding. Use
    /// [`spi_batch_request_frame_len`] to size a batch in advance. An
    /// over-ceiling batch is refused with the same
    /// `SpiBatchError { failed_op: 0, kind: BufferTooLong }` as the response
    /// overflow.
    ///
    /// Before issue #186 an over-ceiling batch was discarded by the
    /// firmware's receiver before any handler ran, and because a batch
    /// carrying no `DelayNs` is bounded by the firmware's 30-minute handler
    /// ceiling rather than by the ordinary call timeout, the caller waited
    /// over half an hour for a [`SpiBatchCallError::Timeout`] describing a
    /// device that had never received the request. Nothing was driven: the
    /// bus stayed idle and chip-select never moved.
    ///
    /// # Chip-select preflight
    ///
    /// `cs_pin` is checked against the device-reported GPIO count *before*
    /// anything is encoded or transmitted. On a cold cache this performs
    /// one implicit [`validate`](Self::validate); afterwards the count is
    /// read locally from the clone-shared cache and no extra round-trip
    /// occurs.
    ///
    /// The order is: refuse a batch that cannot fit through the transport
    /// in either direction; obtain the bound; zero count is
    /// [`SpiBatchCallError::NoGpios`]; `cs_pin >= bound` is
    /// [`SpiBatchCallError::InvalidCsPin`]; only then encode and send
    /// exactly one `spi/batch` RPC. A local refusal transmits nothing and
    /// never fabricates a failed-operation index. This is defence in depth
    /// in front of the firmware's own refusal (issue #104): it stops a
    /// stray index from reaching a board whose firmware predates that
    /// refusal and would silently reconfigure the pin as an output.
    ///
    /// If the bound cannot be established the call fails with
    /// [`SpiBatchCallError::DeviceInfo`], carrying the exact
    /// [`ValidateError`]. A metadata failure is *never* reported as an
    /// invalid chip-select.
    ///
    /// A cached count belongs to the handle that learned it. If the board
    /// is unplugged, the cached byte survives in that (now dead) handle and
    /// a plausible-looking request will proceed to the batch RPC and fail
    /// with [`SpiBatchCallError::Comms`]; the client never rebinds itself
    /// to a different board. A freshly constructed handle starts cold.
    pub async fn spi_batch(&self, cs_pin: u8, ops: &[SpiBatchOp<'_>]) -> Result<Vec<u8>, SpiBatchCallError> {
        check_spi_batch_ops(ops).map_err(SpiBatchCallError::Endpoint)?;
        let num_gpios = self.num_gpios().await.map_err(SpiBatchCallError::DeviceInfo)?;
        classify_cs(cs_pin, num_gpios)?;

        let encoded = encode_spi_batch_ops(ops);
        // Bound from the batch's own accumulated delays: `DelayNs` operations
        // are caller-supplied and a legal 64-operation batch can occupy the
        // firmware dispatcher for minutes. Bounding this by the default would
        // fail a perfectly healthy long batch.
        self.bounded_for(spi_batch_delay_budget_ms(ops))
            .send_resp::<SpiBatch>(&SpiBatchRequest {
                cs_pin,
                count: ops.len() as u16,
                ops: &encoded,
            })
            .await
            .map_err(|e| match e {
                CallError::Comms(e) => SpiBatchCallError::Comms(e),
                CallError::Timeout { waited } => SpiBatchCallError::Timeout { waited },
            })?
            .map_err(SpiBatchCallError::Endpoint)
    }

    /// Read up to `count` bytes from the UART bus.
    ///
    /// The firmware reads up to `count` bytes from the UART receive buffer.
    /// If no data is immediately available, it waits up to `timeout_ms`
    /// milliseconds for at least one byte. Returns whatever bytes are
    /// available (1 to `count`), or an empty `Vec` on timeout.
    /// `timeout_ms == 0` selects a single non-blocking poll; non-zero values
    /// above the firmware's 30-minute ceiling are clamped to it.
    ///
    /// The firmware buffer is limited to [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes, but `count` is bounded far tighter by
    /// [`MAX_RESPONSE_PAYLOAD`] (1014): more than that cannot be carried
    /// back in one response frame, so it is refused locally with
    /// [`UartError::BufferTooLong`] before anything is transmitted.
    pub async fn uart_read(&self, count: u16, timeout_ms: u32) -> Result<Vec<u8>, PicoDeGalloError<UartError>> {
        if response_len_is_undeliverable(usize::from(count)) {
            return Err(PicoDeGalloError::Endpoint(UartError::BufferTooLong));
        }
        // The caller's own read timeout is the firmware-side duration here.
        self.bounded_for(timeout_ms)
            .send_resp::<UartRead>(&UartReadRequest { count, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the UART bus.
    ///
    /// Bytes are queued to the firmware's UART transmit buffer. The call
    /// returns once all bytes have been accepted by the TX buffer (not
    /// necessarily transmitted on the wire). Use [`uart_flush`](Self::uart_flush)
    /// to wait for transmission to complete.
    ///
    /// `contents` may be at most
    /// [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`] (4096) bytes; more is
    /// refused locally with [`UartError::BufferTooLong`].
    pub async fn uart_write(&self, contents: &[u8]) -> Result<(), PicoDeGalloError<UartError>> {
        if request_payload_is_too_long(contents.len()) {
            return Err(PicoDeGalloError::Endpoint(UartError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<UartWrite>(&UartWriteRequest { contents })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Flush the UART transmit buffer.
    ///
    /// Blocks until all pending bytes have been transmitted on the wire.
    pub async fn uart_flush(&self) -> Result<(), PicoDeGalloError<UartError>> {
        self.bounded()
            .send_resp::<UartFlush>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Get the current state of GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_get(&self, pin: u8) -> Result<GpioState, PicoDeGalloError<GpioError>> {
        self.bounded()
            .send_resp::<GpioGet>(&GpioGetRequest { pin })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Set the GPIO numbered by `pin` to state `state`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_put(&self, pin: u8, state: GpioState) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded()
            .send_resp::<GpioPut>(&GpioPutRequest { pin, state })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `High` state.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call selects the firmware's 30-minute ceiling and returns
    /// [`GpioError::Timeout`] on expiry. For a shorter wait, use
    /// [`gpio_wait_for_high_with_timeout`](Self::gpio_wait_for_high_with_timeout).
    pub async fn gpio_wait_for_high(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded_for(0)
            .send_resp::<GpioWaitForHigh>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `Low` state.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call selects the firmware's 30-minute ceiling and returns
    /// [`GpioError::Timeout`] on expiry. For a shorter wait, use
    /// [`gpio_wait_for_low_with_timeout`](Self::gpio_wait_for_low_with_timeout).
    pub async fn gpio_wait_for_low(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded_for(0)
            .send_resp::<GpioWaitForLow>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a rising edge on the GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call selects the firmware's 30-minute ceiling and returns
    /// [`GpioError::Timeout`] on expiry. For a shorter wait, use
    /// [`gpio_wait_for_rising_edge_with_timeout`](Self::gpio_wait_for_rising_edge_with_timeout).
    pub async fn gpio_wait_for_rising_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded_for(0)
            .send_resp::<GpioWaitForRising>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a falling edge on the GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call selects the firmware's 30-minute ceiling and returns
    /// [`GpioError::Timeout`] on expiry. For a shorter wait, use
    /// [`gpio_wait_for_falling_edge_with_timeout`](Self::gpio_wait_for_falling_edge_with_timeout).
    pub async fn gpio_wait_for_falling_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded_for(0)
            .send_resp::<GpioWaitForFalling>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for either a rising edge or a falling edge on the GPIO
    /// numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call selects the firmware's 30-minute ceiling and returns
    /// [`GpioError::Timeout`] on expiry. For a shorter wait, use
    /// [`gpio_wait_for_any_edge_with_timeout`](Self::gpio_wait_for_any_edge_with_timeout).
    pub async fn gpio_wait_for_any_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded_for(0)
            .send_resp::<GpioWaitForAny>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `High` state, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if the level is not reached within `timeout`. `Duration::ZERO`
    /// selects the firmware's 30-minute ceiling; larger durations are
    /// clamped to the same ceiling.
    ///
    /// Available on firmware schema 0.7+.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_wait_for_high_with_timeout(
        &self,
        pin: u8,
        timeout: std::time::Duration,
    ) -> Result<(), PicoDeGalloError<GpioError>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self.bounded_for(timeout_ms)
            .send_resp::<GpioWaitForHigh>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `Low` state, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if the level is not reached within `timeout`. `Duration::ZERO`
    /// selects the firmware's 30-minute ceiling; larger durations are
    /// clamped to the same ceiling.
    ///
    /// Available on firmware schema 0.7+.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_wait_for_low_with_timeout(
        &self,
        pin: u8,
        timeout: std::time::Duration,
    ) -> Result<(), PicoDeGalloError<GpioError>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self.bounded_for(timeout_ms)
            .send_resp::<GpioWaitForLow>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a rising edge on the GPIO numbered by `pin`, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if no rising edge is detected within `timeout`. `Duration::ZERO`
    /// selects the firmware's 30-minute ceiling; larger durations are
    /// clamped to the same ceiling.
    ///
    /// Available on firmware schema 0.7+.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_wait_for_rising_edge_with_timeout(
        &self,
        pin: u8,
        timeout: std::time::Duration,
    ) -> Result<(), PicoDeGalloError<GpioError>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self.bounded_for(timeout_ms)
            .send_resp::<GpioWaitForRising>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a falling edge on the GPIO numbered by `pin`, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if no falling edge is detected within `timeout`. `Duration::ZERO`
    /// selects the firmware's 30-minute ceiling; larger durations are
    /// clamped to the same ceiling.
    ///
    /// Available on firmware schema 0.7+.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_wait_for_falling_edge_with_timeout(
        &self,
        pin: u8,
        timeout: std::time::Duration,
    ) -> Result<(), PicoDeGalloError<GpioError>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self.bounded_for(timeout_ms)
            .send_resp::<GpioWaitForFalling>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for either a rising edge or a falling edge on the GPIO
    /// numbered by `pin`, with a host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if no edge is detected within `timeout`. `Duration::ZERO` selects
    /// the firmware's 30-minute ceiling; larger durations are clamped to
    /// the same ceiling.
    ///
    /// Available on firmware schema 0.7+.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_wait_for_any_edge_with_timeout(
        &self,
        pin: u8,
        timeout: std::time::Duration,
    ) -> Result<(), PicoDeGalloError<GpioError>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self.bounded_for(timeout_ms)
            .send_resp::<GpioWaitForAny>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Configure a GPIO pin's direction and internal pull resistor.
    ///
    /// After configuration, the pin enters explicit mode: `gpio_get` and
    /// `gpio_put` will no longer auto-switch direction. Calling `gpio_put`
    /// on an input pin (or `gpio_get`/wait on an output pin) will return
    /// [`GpioError::WrongDirection`].
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_set_config(
        &self,
        pin: u8,
        direction: GpioDirection,
        pull: GpioPull,
    ) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded()
            .send_resp::<GpioSetConfiguration>(&GpioSetConfigurationRequest { pin, direction, pull })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Subscribe to GPIO edge events on a pin.
    ///
    /// Starts push-based monitoring for the specified edge type. While subscribed,
    /// the pin cannot be used by other GPIO operations (they will return
    /// [`GpioError::PinMonitored`]). Use [`gpio_unsubscribe`](Self::gpio_unsubscribe)
    /// to release the pin.
    ///
    /// Call [`subscribe_gpio_events`](Self::subscribe_gpio_events) to receive the
    /// event stream.
    pub async fn gpio_subscribe(&self, pin: u8, edge: GpioEdge) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded()
            .send_resp::<GpioSubscribe>(&GpioSubscribeRequest { pin, edge })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Unsubscribe from GPIO edge events on a pin.
    ///
    /// Stops monitoring and returns the pin to normal operation. Returns
    /// [`GpioError::PinNotMonitored`] if the pin is not currently subscribed.
    pub async fn gpio_unsubscribe(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.bounded()
            .send_resp::<GpioUnsubscribe>(&GpioUnsubscribeRequest { pin })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Subscribe to the GPIO event topic stream.
    ///
    /// Returns a [`MultiSubscription`] that yields [`GpioEvent`] messages as edges
    /// are detected on any subscribed pin. Call this *before* or *after*
    /// [`gpio_subscribe`](Self::gpio_subscribe) — events are buffered up to
    /// `depth` messages.
    ///
    /// Edge detection is best-effort: if the pin changes faster than the
    /// firmware monitor loop cadence, intermediate transitions may be missed.
    pub async fn subscribe_gpio_events(
        &self,
        depth: usize,
    ) -> Result<MultiSubscription<GpioEvent>, PicoDeGalloError<Infallible>> {
        self.client
            .subscribe_multi::<GpioEventTopic>(depth)
            .await
            .map_err(|_| PicoDeGalloError::Comms(HostErr::Closed))
    }

    /// Set I2C bus configuration parameters.
    ///
    /// Changes the I2C bus clock frequency. Takes effect immediately before
    /// the next I2C operation.
    pub async fn i2c_set_config(&self, frequency: I2cFrequency) -> Result<(), PicoDeGalloError<I2cError>> {
        self.bounded()
            .send_resp::<I2cSetConfiguration>(&I2cSetConfigurationRequest { frequency })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Set SPI bus configuration parameters.
    ///
    /// Changes the SPI bus clock frequency, phase, and polarity. Takes effect
    /// immediately before the next SPI operation.
    pub async fn spi_set_config(
        &self,
        spi_frequency: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) -> Result<(), PicoDeGalloError<SpiError>> {
        self.bounded()
            .send_resp::<SpiSetConfiguration>(&SpiSetConfigurationRequest {
                spi_frequency,
                spi_phase,
                spi_polarity,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Get the firmware version from the Pico de Gallo device.
    pub async fn version(&self) -> Result<VersionInfo, PicoDeGalloError<Infallible>> {
        Ok(self.bounded().send_resp::<Version>(&()).await?)
    }

    /// Get extended device information including firmware version, schema
    /// (wire protocol) version, hardware revision, peripheral capabilities,
    /// and the informational firmware build identity.
    pub async fn device_info(&self) -> Result<DeviceInfo, PicoDeGalloError<Infallible>> {
        Ok(self.bounded().send_resp::<GetDeviceInfo>(&()).await?)
    }

    /// Fetch `device/info` under [`Self::metadata_timeout`] and check the
    /// reported schema.
    ///
    /// Only the `send_resp` future is wrapped: expiry maps to
    /// [`ValidateError::Timeout`], a completed transport failure keeps its
    /// existing [`map_validate_error`] classification, and a decoded
    /// response is schema-checked. No caching happens here, so every error
    /// path leaves the cache untouched and therefore retryable.
    async fn fetch_validated_info(&self) -> Result<DeviceInfo, ValidateError> {
        // Deliberately the raw client, not `bounded()`: this path carries its
        // own, far longer `metadata_timeout`, and nesting the default call
        // bound inside it would silently shorten `validate()` to five seconds.
        let fut = self.client.send_resp::<GetDeviceInfo>(&());
        let info = match tokio::time::timeout(self.metadata_timeout, fut).await {
            Err(_elapsed) => return Err(ValidateError::Timeout),
            Ok(Err(e)) => return Err(map_validate_error(e)),
            Ok(Ok(info)) => info,
        };

        check_schema_compatible(&info)?;

        Ok(info)
    }

    /// Validate that the connected firmware is wire-compatible with this
    /// host library.
    ///
    /// Queries the `device/info` endpoint under a
    /// [`DEVICE_INFO_TIMEOUT`] bound and checks that the schema major and
    /// minor versions match (pre-1.0 semver: minor bumps are breaking).
    /// Returns the [`DeviceInfo`] on success so callers can inspect
    /// capabilities without an extra round-trip.
    ///
    /// On success the reported GPIO count is stored in the clone-shared
    /// cache that [`num_gpios`](Self::num_gpios) and
    /// [`spi_batch`](Self::spi_batch) read. The `num_gpios` field of the
    /// returned `DeviceInfo` is the *stored* byte, re-read after the
    /// store attempt — not this call's freshly fetched byte. When several
    /// callers race on a cold cache, they therefore all observe the one
    /// authoritative winner rather than each observing its own response.
    ///
    /// This checks the reported numbers; it cannot make them trustworthy,
    /// and there are two distinct ways they can mislead.
    ///
    /// Wire *shape*: a matching schema version does not prove shape
    /// compatibility, and for one specific type this call cannot report
    /// the difference at all. postcard-rpc derives each endpoint's key
    /// from the response type's schema, so changing a type's shape —
    /// appends included — silently re-keys its endpoint. A peer built
    /// against the other shape replies under the other key, the
    /// dispatcher drops the unmatched frame, and the call never
    /// returns.
    ///
    /// Where that bites depends on *which* type changed. If the change
    /// is to any type other than [`DeviceInfo`] — appending an
    /// `I2cError` variant, say — then `device/info` still answers, its
    /// reply still decodes, the schema minors still differ, and this
    /// call correctly returns [`ValidateError::SchemaMismatch`]. Schema
    /// versioning works as designed.
    ///
    /// If the change is to [`DeviceInfo`] itself, `device/info` is the
    /// endpoint that re-keys, so the probe that would have reported the
    /// mismatch is the one that breaks. The schema numbers describing
    /// the incompatibility are sealed inside the message that is
    /// dropped, and no version bump can surface them: the version is
    /// payload, not key. This call can then only return
    /// [`ValidateError::Timeout`] after [`DEVICE_INFO_TIMEOUT`],
    /// indistinguishable from a board that genuinely stopped answering.
    /// `DeviceInfo` is, in that sense, a blind spot for its own
    /// versioning mechanism.
    ///
    /// This is not confined to development trees. Any two *released*
    /// versions whose `DeviceInfo` shapes differ pair this way — a
    /// schema 0.8 host against schema 0.7 firmware reports a timeout,
    /// not a mismatch. When a board is unexpectedly unresponsive to
    /// this call, a version skew is as likely an explanation as a
    /// hardware fault. `gallo version` still works across such a pair,
    /// because [`VersionInfo`]'s schema and key are deliberately held
    /// stable.
    ///
    /// Wire *behaviour*: the schema version is derived from the wire
    /// crate's package version, so it is intended to track wire-type
    /// changes, not handler changes. Two firmware builds can report
    /// identical versions and still frame the bus differently. To
    /// identify the image, read
    /// [`DeviceInfo::build_id()`](method@pico_de_gallo_internal::DeviceInfo::build_id).
    /// It is informational only and never affects the outcome of this
    /// call.
    ///
    /// # Errors
    ///
    /// - [`ValidateError::Comms`] — could not reach the device.
    /// - [`ValidateError::Timeout`] — no response within
    ///   [`DEVICE_INFO_TIMEOUT`]. The cache stays empty; retry is allowed.
    /// - [`ValidateError::LegacyFirmware`] — firmware does not support
    ///   `device/info` (upgrade firmware).
    /// - [`ValidateError::SchemaMismatch`] — firmware and host disagree on
    ///   the wire protocol version.
    pub async fn validate(&self) -> Result<DeviceInfo, ValidateError> {
        let mut info = self.fetch_validated_info().await?;

        // Attempt the store, then *re-read*. The re-read is mandatory even
        // when the store succeeded: on a concurrent cold-cache race only one
        // value wins, and every racer must report the winner.
        let _ = self.num_gpios_cache.set(info.num_gpios);
        info.num_gpios = *self
            .num_gpios_cache
            .get()
            .expect("cache is populated by the set() immediately above");

        Ok(info)
    }

    /// The number of GPIO pins the connected device reports.
    ///
    /// This is the runtime-authoritative bound for a chip-select index, and
    /// it is what [`spi_batch`](Self::spi_batch) checks against. Prefer it
    /// over the compile-time [`NUM_GPIOS`].
    ///
    /// On a warm cache this is a local read with no USB traffic. On a cold
    /// cache it performs an implicit [`validate`](Self::validate) — one
    /// bounded `device/info` round-trip — and returns the stored value. A
    /// reported count of zero is a legitimate, cacheable answer, not a miss.
    ///
    /// # Errors
    ///
    /// Any [`ValidateError`]. A failure leaves the cache empty, so the next
    /// call re-attempts the fetch.
    pub async fn num_gpios(&self) -> Result<u8, ValidateError> {
        if let Some(n) = self.num_gpios_cache.get() {
            return Ok(*n);
        }
        self.validate().await?;
        Ok(*self
            .num_gpios_cache
            .get()
            .expect("a successful validate() always populates the cache"))
    }

    /// Tear down any GPIO subscriptions left over from a previous host
    /// session.
    ///
    /// Subscriptions are server-side state that survives the USB transport
    /// — if a previous host crashed, was killed, or dropped its
    /// [`nusb::Interface`] without sending `gpio/unsubscribe`, the firmware
    /// will still consider those pins owned by a monitor task. Calling this
    /// method on connect cleans up that state so the new host can use the
    /// pins.
    ///
    /// Returns the number of subscriptions that were torn down (0 if none
    /// were active). The endpoint is idempotent and cheap to call when no
    /// subscriptions exist, so the recommended sequence after construction
    /// is:
    ///
    /// ```no_run
    /// # use pico_de_gallo_lib::PicoDeGallo;
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let gallo = PicoDeGallo::new();
    /// let _info = gallo.validate().await?;
    /// let _reset = gallo.system_reset_subscriptions().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn system_reset_subscriptions(&self) -> Result<u8, PicoDeGalloError<Infallible>> {
        Ok(self.bounded().send_resp::<SystemResetSubscriptions>(&()).await?)
    }

    /// Query the current I2C bus configuration.
    ///
    /// Returns the [`I2cFrequency`] value that is currently active on the
    /// firmware. The default is [`I2cFrequency::Standard`] (100 kHz).
    pub async fn i2c_get_config(&self) -> Result<I2cFrequency, PicoDeGalloError<Infallible>> {
        Ok(self.bounded().send_resp::<I2cGetConfiguration>(&()).await?)
    }

    /// Query the current SPI bus configuration.
    ///
    /// Returns a [`SpiConfigurationInfo`] struct with the active SPI
    /// frequency, phase, and polarity. The defaults are 1 MHz,
    /// `CaptureOnFirstTransition`, and `IdleLow`.
    pub async fn spi_get_config(&self) -> Result<SpiConfigurationInfo, PicoDeGalloError<Infallible>> {
        Ok(self.bounded().send_resp::<SpiGetConfiguration>(&()).await?)
    }

    /// Set the UART baud rate and framing.
    ///
    /// This replaces the **complete** UART configuration. There is no partial
    /// update: to change only the baud rate, read [`Self::uart_get_config`]
    /// first and pass its framing fields back. The power-on configuration is
    /// 115200 8N1.
    ///
    /// # Runtime reconfiguration
    ///
    /// Baud rate and framing are applied before this call returns, but the
    /// hardware transition is not atomic. The firmware first applies the baud
    /// divisor and then applies word length, parity, and stop bits. During the
    /// transition the UART is disabled twice, and it is briefly enabled with
    /// the new baud rate and the previous framing.
    ///
    /// Reconfiguration does not drain either UART direction. Bytes already
    /// queued by [`Self::uart_write`], or bytes sent by the peer during the
    /// transition, may cross the configuration boundary. A malformed received
    /// character may be dropped and reported by a later read as a generic UART
    /// error; some mismatches can instead produce an apparently valid but
    /// incorrect byte.
    ///
    /// Before calling this method, serialize UART access, stop issuing writes,
    /// arrange for the peer to stop transmitting under the old configuration,
    /// call [`Self::uart_flush`] to drain this device's TX queue, and consume
    /// or discard any pending RX data. Resume traffic only after this call
    /// succeeds and the peer has switched to the same configuration. An empty
    /// read alone cannot prove that no character is currently in flight.
    ///
    /// If this call times out while the same connection has concurrent
    /// long-running RPCs, its eventual outcome is uncertain: firmware dispatch
    /// is serial and a request already sent cannot be cancelled. Do not retry
    /// blindly. Quiesce the link and query [`Self::uart_get_config`] once the
    /// dispatcher is responsive.
    ///
    /// # Errors
    ///
    /// Returns [`UartError::InvalidBaudRate`] if `baud_rate` is zero, and
    /// [`UartError::Unsupported`] if the firmware's hardware revision has no
    /// UART.
    pub async fn uart_set_config(
        &self,
        baud_rate: u32,
        data_bits: UartDataBits,
        parity: UartParity,
        stop_bits: UartStopBits,
    ) -> Result<(), PicoDeGalloError<UartError>> {
        self.bounded()
            .send_resp::<UartSetConfiguration>(&UartSetConfigurationRequest {
                baud_rate,
                data_bits,
                parity,
                stop_bits,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Query the current UART configuration.
    ///
    /// Returns the last successfully **requested** baud rate, data bits,
    /// parity and stop bits. The power-on configuration is 115200 8N1.
    ///
    /// This is firmware software-shadow state, not a register read-back: the
    /// reported `baud_rate` is the value that was asked for, not the rate the
    /// divisor actually achieves after rounding.
    ///
    /// A persistent timeout here against a device that otherwise responds can
    /// indicate firmware predating UART framing support: this endpoint's
    /// response type changed, so such firmware replies under a key this host
    /// no longer waits on. Check the device's `build_id` rather than retrying.
    ///
    /// # Errors
    ///
    /// Returns [`UartError::Unsupported`] if the firmware's hardware revision
    /// has no UART.
    pub async fn uart_get_config(&self) -> Result<UartConfigurationInfo, PicoDeGalloError<UartError>> {
        self.bounded()
            .send_resp::<UartGetConfiguration>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    // -----------------------------------------------------------------------
    // PWM
    // -----------------------------------------------------------------------

    /// Set the raw duty cycle of a PWM channel (0–3).
    ///
    /// `duty` is a raw compare value in the range `0..=top`. Use
    /// [`pwm_get_duty_cycle`](Self::pwm_get_duty_cycle) to discover `max_duty`
    /// (which equals the current `top` value).
    ///
    /// Channels 0–1 share PWM slice 6, channels 2–3 share PWM slice 7.
    pub async fn pwm_set_duty_cycle(&self, channel: u8, duty: u16) -> Result<(), PicoDeGalloError<PwmError>> {
        self.bounded()
            .send_resp::<PwmSetDutyCycle>(&PwmSetDutyCycleRequest { channel, duty })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Query the current duty cycle of a PWM channel (0–3).
    ///
    /// Returns a [`PwmDutyCycleInfo`] with `current_duty` (the raw compare
    /// value) and `max_duty` (the `top` register + 1, i.e., the full-scale
    /// value).
    pub async fn pwm_get_duty_cycle(&self, channel: u8) -> Result<PwmDutyCycleInfo, PicoDeGalloError<PwmError>> {
        self.bounded()
            .send_resp::<PwmGetDutyCycle>(&PwmGetDutyCycleRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Enable the PWM slice that owns `channel` (0–3).
    ///
    /// Because PWM slices drive two channels, enabling channel 0 also
    /// enables channel 1 (and vice versa). Same for channels 2/3.
    pub async fn pwm_enable(&self, channel: u8) -> Result<(), PicoDeGalloError<PwmError>> {
        self.bounded()
            .send_resp::<PwmEnable>(&PwmEnableRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Disable the PWM slice that owns `channel` (0–3).
    ///
    /// Because PWM slices drive two channels, disabling channel 0 also
    /// disables channel 1 (and vice versa). Same for channels 2/3.
    pub async fn pwm_disable(&self, channel: u8) -> Result<(), PicoDeGalloError<PwmError>> {
        self.bounded()
            .send_resp::<PwmDisable>(&PwmDisableRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Configure the PWM slice behind `channel` (0–3).
    ///
    /// Sets the output frequency and phase-correct mode. The firmware
    /// computes `top` and `divider` automatically. Existing duty-cycle
    /// compare values are scaled proportionally to the new `top`.
    ///
    /// Channels 0–1 share a slice, so configuring channel 0 also affects
    /// channel 1 (and vice versa). Same for channels 2/3.
    pub async fn pwm_set_config(
        &self,
        channel: u8,
        frequency_hz: u32,
        phase_correct: bool,
    ) -> Result<(), PicoDeGalloError<PwmError>> {
        self.bounded()
            .send_resp::<PwmSetConfiguration>(&PwmSetConfigurationRequest {
                channel,
                frequency_hz,
                phase_correct,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Query the current configuration of the PWM slice behind `channel` (0–3).
    ///
    /// Returns a [`PwmConfigurationInfo`] with the effective frequency,
    /// phase-correct flag, and enabled state.
    pub async fn pwm_get_config(&self, channel: u8) -> Result<PwmConfigurationInfo, PicoDeGalloError<PwmError>> {
        self.bounded()
            .send_resp::<PwmGetConfiguration>(&PwmGetConfigurationRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    // ---- ADC methods ----

    /// Perform a single-shot ADC read on the specified channel.
    ///
    /// Returns a raw 12-bit value (0–4095). Convert to voltage with:
    /// `V ≈ raw × 3.3 / 4096` (approximate — depends on ADC_AVDD).
    pub async fn adc_read(&self, channel: AdcChannel) -> Result<u16, PicoDeGalloError<AdcError>> {
        self.bounded()
            .send_resp::<AdcRead>(&AdcReadRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Query the ADC configuration (resolution, reference, channel count).
    ///
    /// Returns an [`AdcConfigurationInfo`] with fixed values for the RP2350
    /// ADC. Useful for host-side discovery.
    ///
    /// Returns [`AdcError::Unsupported`] if the firmware's hardware revision
    /// does not support ADC.
    pub async fn adc_get_config(&self) -> Result<AdcConfigurationInfo, PicoDeGalloError<AdcError>> {
        self.bounded()
            .send_resp::<AdcGetConfiguration>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    // ---- 1-Wire ----

    /// Perform a 1-Wire bus reset and detect device presence.
    ///
    /// Returns `true` if one or more devices responded with a presence pulse.
    pub async fn onewire_reset(&self) -> Result<bool, PicoDeGalloError<OneWireError>> {
        self.bounded()
            .send_resp::<OneWireReset>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Read `len` bytes from the 1-Wire bus.
    ///
    /// The firmware sends `0xFF` read slots and captures the device's response bits.
    ///
    /// `len` may be at most [`MAX_RESPONSE_PAYLOAD`] (1014); more cannot be
    /// carried back in one response frame and is refused locally with
    /// [`OneWireError::BufferTooLong`] before any read slot is driven.
    pub async fn onewire_read(&self, len: u16) -> Result<Vec<u8>, PicoDeGalloError<OneWireError>> {
        if response_len_is_undeliverable(usize::from(len)) {
            return Err(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<OneWireRead>(&OneWireReadRequest { len })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write raw bytes to the 1-Wire bus.
    ///
    /// `data` may be at most [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes; more is refused locally with
    /// [`OneWireError::BufferTooLong`].
    pub async fn onewire_write(&self, data: &[u8]) -> Result<(), PicoDeGalloError<OneWireError>> {
        if request_payload_is_too_long(data.len()) {
            return Err(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong));
        }
        self.bounded()
            .send_resp::<OneWireWrite>(&OneWireWriteRequest { data })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write bytes to the 1-Wire bus, then apply a strong pullup for the given duration.
    ///
    /// This is needed for parasitic-power devices like the DS18B20 during temperature
    /// conversion. The bus is held high for `pullup_duration_ms` milliseconds after
    /// the last bit is sent.
    ///
    /// `data` may be at most [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes; more is refused locally with
    /// [`OneWireError::BufferTooLong`], before the bus is driven and before
    /// the pullup hold begins.
    pub async fn onewire_write_pullup(
        &self,
        data: &[u8],
        pullup_duration_ms: u16,
    ) -> Result<(), PicoDeGalloError<OneWireError>> {
        if request_payload_is_too_long(data.len()) {
            return Err(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong));
        }
        // The caller's strong-pullup hold is firmware-side time on top of the
        // write itself, so it must widen the bound or a long parasitic-power
        // conversion would look like an unresponsive device.
        self.bounded_for(u32::from(pullup_duration_ms))
            .send_resp::<OneWireWritePullup>(&OneWireWritePullupRequest {
                data,
                pullup_duration_ms,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Start a new 1-Wire ROM search and return the first device address.
    ///
    /// Returns `Some(rom_id)` for the first device found, or `None` if no devices
    /// are on the bus. Call [`onewire_search_next`](Self::onewire_search_next) to
    /// continue enumerating.
    pub async fn onewire_search(&self) -> Result<Option<u64>, PicoDeGalloError<OneWireError>> {
        self.bounded()
            .send_resp::<OneWireSearch>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Continue the current 1-Wire ROM search.
    ///
    /// Returns the next device's 64-bit ROM ID, or `None` when all devices have
    /// been enumerated.
    pub async fn onewire_search_next(&self) -> Result<Option<u64>, PicoDeGalloError<OneWireError>> {
        self.bounded()
            .send_resp::<OneWireSearchNext>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Scripted transport harness
    // -------------------------------------------------------------------
    //
    // A `HostClient` built from a local `WireTx`/`WireRx`/`WireSpawn` triple,
    // so the *real* public `spi_batch()` / `num_gpios()` / `validate()`
    // methods can be driven with no board attached. Pure classifier tests
    // cannot prove which RPCs were emitted, in what order, or how many
    // times, and those are precisely the properties issue #104 is about.
    //
    // Deliberately uses only `std` synchronisation plus tokio `time`/`rt`,
    // all of which are direct dependencies. It does not use
    // `tokio::sync`, which this crate only gets transitively through
    // postcard-rpc, nor postcard-rpc's own `test-utils` feature, which is
    // not enabled and could not be enabled without a manifest change.

    use postcard_rpc::Endpoint;
    use postcard_rpc::header::{VarHeader, VarKey};
    use postcard_rpc::host_client::{RpcFrame, WireRx, WireSpawn, WireTx};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// One scripted reply for one inbound request, in arrival order.
    #[derive(Debug)]
    enum Reply {
        /// Reply on `device/info`'s response key with a valid `DeviceInfo`.
        DeviceInfo(DeviceInfo),
        /// Reply on `device/info`'s response key with an undecodable body.
        /// Surfaces as `HostErr::Postcard(..)` — a *decode* failure, which
        /// (unlike a wire error) does not stop the client, so a retry can
        /// still succeed.
        TruncatedInfo,
        /// Reply on the error path, producing `HostErr::Wire(..)`.
        WireErr(WireError),
        /// Reply on `spi/batch`'s response key with `Ok(bytes)`.
        SpiBatchOk(Vec<u8>),
        /// Reply on `spi/batch`'s response key with `Err(e)`.
        SpiBatchErr(SpiBatchError),
        /// Send nothing at all: the caller waits until its timeout.
        Silent,
        /// Send nothing and make the receive side fail permanently, which
        /// stops the `HostClient` (postcard-rpc treats a `WireRx` error as
        /// fatal) and closes every pending and future call.
        CloseWire,
    }

    #[derive(Default)]
    struct ScriptState {
        /// Endpoint paths, in the order the host transmitted them.
        order: Vec<&'static str>,
        /// Encoded frames waiting to be handed to `WireRx::receive`.
        inbox: VecDeque<Vec<u8>>,
        /// Replies to apply, one per inbound request.
        plan: VecDeque<Reply>,
        /// Once set, `receive` fails and the client stops for good.
        dead: bool,
    }

    #[derive(Clone, Default)]
    struct Script(Arc<Mutex<ScriptState>>);

    impl Script {
        fn with(plan: Vec<Reply>) -> Self {
            let me = Self::default();
            me.0.lock().unwrap().plan = plan.into();
            me
        }

        /// Fail every `receive` from the outset (an unplugged board).
        fn dead_on_arrival(&self) {
            self.0.lock().unwrap().dead = true;
        }

        fn order(&self) -> Vec<&'static str> {
            self.0.lock().unwrap().order.clone()
        }

        fn count(&self, path: &str) -> usize {
            self.0.lock().unwrap().order.iter().filter(|p| **p == path).count()
        }
    }

    #[derive(Debug)]
    struct WireDead;

    impl core::fmt::Display for WireDead {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "scripted wire is closed")
        }
    }

    impl std::error::Error for WireDead {}

    fn reply_frame(header: VarHeader, key: postcard_rpc::Key, body: Vec<u8>) -> Vec<u8> {
        RpcFrame {
            header: VarHeader {
                // Echo the request's sequence verbatim: postcard-rpc matches
                // a response on the exact (seq_no, key) pair.
                seq_no: header.seq_no,
                key: VarKey::Key8(key),
            },
            body,
        }
        .to_bytes()
    }

    struct ScriptedTx(Script);

    impl WireTx for ScriptedTx {
        type Error = WireDead;

        async fn send(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
            let (hdr, _body) = VarHeader::take_from_slice(&data).expect("scripted wire got an undecodable header");

            let path = if hdr.key == VarKey::Key8(GetDeviceInfo::REQ_KEY) {
                "device/info"
            } else if hdr.key == VarKey::Key8(SpiBatch::REQ_KEY) {
                "spi/batch"
            } else if hdr.key == VarKey::Key8(I2cWrite::REQ_KEY) {
                "i2c/write"
            } else if hdr.key == VarKey::Key8(I2cBatch::REQ_KEY) {
                "i2c/batch"
            } else if hdr.key == VarKey::Key8(I2cWriteRead::REQ_KEY) {
                "i2c/write-read"
            } else if hdr.key == VarKey::Key8(I2cRead::REQ_KEY) {
                "i2c/read"
            } else if hdr.key == VarKey::Key8(SpiRead::REQ_KEY) {
                "spi/read"
            } else if hdr.key == VarKey::Key8(SpiWrite::REQ_KEY) {
                "spi/write"
            } else if hdr.key == VarKey::Key8(SpiTransfer::REQ_KEY) {
                "spi/transfer"
            } else if hdr.key == VarKey::Key8(UartRead::REQ_KEY) {
                "uart/read"
            } else if hdr.key == VarKey::Key8(UartWrite::REQ_KEY) {
                "uart/write"
            } else if hdr.key == VarKey::Key8(OneWireRead::REQ_KEY) {
                "onewire/read"
            } else if hdr.key == VarKey::Key8(OneWireWrite::REQ_KEY) {
                "onewire/write"
            } else if hdr.key == VarKey::Key8(OneWireWritePullup::REQ_KEY) {
                "onewire/write-pullup"
            } else {
                "other"
            };

            let mut st = self.0.0.lock().unwrap();
            st.order.push(path);
            let reply = st.plan.pop_front().unwrap_or(Reply::Silent);

            let frame = match reply {
                Reply::DeviceInfo(info) => Some(reply_frame(
                    hdr,
                    GetDeviceInfo::RESP_KEY,
                    postcard::to_stdvec(&info).unwrap(),
                )),
                Reply::TruncatedInfo => Some(reply_frame(hdr, GetDeviceInfo::RESP_KEY, vec![0x00])),
                Reply::WireErr(e) => Some(reply_frame(
                    hdr,
                    postcard_rpc::Key::for_path::<WireError>(ERROR_PATH),
                    postcard::to_stdvec(&e).unwrap(),
                )),
                Reply::SpiBatchOk(bytes) => Some(reply_frame(
                    hdr,
                    SpiBatch::RESP_KEY,
                    postcard::to_stdvec(&Ok::<Vec<u8>, SpiBatchError>(bytes)).unwrap(),
                )),
                Reply::SpiBatchErr(e) => Some(reply_frame(
                    hdr,
                    SpiBatch::RESP_KEY,
                    postcard::to_stdvec(&Err::<Vec<u8>, SpiBatchError>(e)).unwrap(),
                )),
                Reply::Silent => None,
                Reply::CloseWire => {
                    st.dead = true;
                    None
                }
            };

            if let Some(f) = frame {
                st.inbox.push_back(f);
            }
            Ok(())
        }
    }

    struct ScriptedRx(Script);

    impl WireRx for ScriptedRx {
        type Error = WireDead;

        async fn receive(&mut self) -> Result<Vec<u8>, Self::Error> {
            loop {
                {
                    let mut st = self.0.0.lock().unwrap();
                    if st.dead {
                        return Err(WireDead);
                    }
                    if let Some(frame) = st.inbox.pop_front() {
                        return Ok(frame);
                    }
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    }

    struct ScriptedSpawn;

    impl WireSpawn for ScriptedSpawn {
        fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) {
            tokio::task::spawn(fut);
        }
    }

    /// Build a handle on the scripted transport, using the *production*
    /// sequence kind, error path and queue depth so the harness exercises
    /// the real configuration rather than a convenient one.
    fn scripted(plan: Vec<Reply>, metadata_timeout: Duration) -> (PicoDeGallo, Script) {
        let script = Script::with(plan);
        let client = HostClient::<WireError>::new_with_wire(
            ScriptedTx(script.clone()),
            ScriptedRx(script.clone()),
            ScriptedSpawn,
            VarSeqKind::Seq2,
            ERROR_PATH,
            8,
        );
        (PicoDeGallo::new_for_test(client, metadata_timeout), script)
    }

    /// A `DeviceInfo` that passes `check_schema_compatible` and reports `n`
    /// GPIOs.
    fn good_info(n: u8) -> DeviceInfo {
        let mut info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
        info.num_gpios = n;
        info
    }

    fn one_read_op() -> Vec<SpiBatchOp<'static>> {
        vec![SpiBatchOp::Read { len: 1 }]
    }

    /// Long enough that no healthy scripted exchange can trip it, short
    /// enough that the timeout tests finish in milliseconds.
    const TEST_TIMEOUT: Duration = Duration::from_millis(200);

    // --- PicoDeGalloError tests ---

    #[test]
    fn endpoint_error_wraps_inner() {
        let err: PicoDeGalloError<&str> = PicoDeGalloError::Endpoint("endpoint failed");
        match err {
            PicoDeGalloError::Endpoint(e) => assert_eq!(e, "endpoint failed"),
            PicoDeGalloError::Comms(_) => panic!("expected Endpoint, got Comms"),
            PicoDeGalloError::Timeout { .. } => panic!("expected Endpoint, got Timeout"),
        }
    }

    #[test]
    fn map_err_converts_ok() {
        let result: Result<u32, &str> = Ok(42);
        let mapped: Result<u32, PicoDeGalloError<&str>> = result.map_err(PicoDeGalloError::Endpoint);
        assert_eq!(mapped.unwrap(), 42);
    }

    #[test]
    fn map_err_converts_err() {
        let result: Result<(), I2cError> = Err(I2cError::NoAcknowledge);
        let mapped = result.map_err(PicoDeGalloError::Endpoint);
        match mapped {
            Err(PicoDeGalloError::Endpoint(I2cError::NoAcknowledge)) => {}
            _ => panic!("expected Endpoint(I2cError::NoAcknowledge)"),
        }
    }

    // --- PicoDeGalloError From impl ---

    #[test]
    fn host_err_converts_to_comms_error() {
        let host_err: HostErr<WireError> = HostErr::Closed;
        let err: PicoDeGalloError<Infallible> = PicoDeGalloError::from(host_err);
        match err {
            PicoDeGalloError::Comms(HostErr::Closed) => {}
            _ => panic!("expected Comms(Closed)"),
        }
    }

    // --- PicoDeGalloError Debug ---

    #[test]
    fn error_debug_format_is_readable() {
        let err: PicoDeGalloError<I2cError> = PicoDeGalloError::Endpoint(I2cError::Bus);
        let debug = format!("{:?}", err);
        assert!(debug.contains("Endpoint"));
        assert!(debug.contains("Bus"));

        let comms_err: PicoDeGalloError<Infallible> = PicoDeGalloError::Comms(HostErr::Closed);
        let debug = format!("{:?}", comms_err);
        assert!(debug.contains("Comms"));
    }

    // --- PicoDeGalloError Display ---

    #[test]
    fn error_display_endpoint() {
        // Use a simple Display-implementing type
        let err: PicoDeGalloError<&str> = PicoDeGalloError::Endpoint("sensor timeout");
        let msg = format!("{err}");
        assert!(msg.contains("endpoint error"));
        assert!(msg.contains("sensor timeout"));
    }

    #[test]
    fn error_display_comms() {
        let err: PicoDeGalloError<&str> = PicoDeGalloError::Comms(HostErr::Closed);
        let msg = format!("{err}");
        assert!(msg.contains("communication error"));
    }

    #[test]
    fn error_is_std_error() {
        fn assert_error<E: std::error::Error>() {}
        assert_error::<PicoDeGalloError<&str>>();
    }

    // --- Device enumeration ---

    #[test]
    fn list_devices_returns_vec() {
        // Without hardware this returns an empty vec, but should not panic
        let devices = list_devices();
        // Each returned device must have the correct VID/PID (already filtered)
        for dev in &devices {
            assert!(dev.serial_number.is_some() || dev.serial_number.is_none());
        }
        // Mainly verifying the function doesn't panic
        let _ = devices;
    }

    #[test]
    fn device_description_is_clone_and_debug() {
        let desc = DeviceDescription {
            serial_number: Some("ABC123".to_string()),
            manufacturer: Some("Microsoft".to_string()),
            product: Some("Pico de Gallo".to_string()),
        };
        let cloned = desc.clone();
        assert_eq!(format!("{:?}", desc), format!("{:?}", cloned));
    }

    // --- map_validate_error policy (P1-1) ---
    //
    // The matches below are deliberately exhaustive: if upstream
    // postcard-rpc adds a new `WireError` or `HostErr` variant, the
    // compiler will force us to revisit the mapping policy rather than
    // silently routing the new variant through the wildcard arm.

    use postcard_rpc::standard_icd::{FrameTooLong, FrameTooShort};

    fn assert_legacy(e: HostErr<WireError>) {
        match map_validate_error(e) {
            ValidateError::LegacyFirmware => {}
            other => panic!("expected LegacyFirmware, got {other:?}"),
        }
    }

    fn assert_comms(e: HostErr<WireError>) {
        match map_validate_error(e) {
            ValidateError::Comms(_) => {}
            other => panic!("expected Comms, got {other:?}"),
        }
    }

    #[test]
    fn map_validate_error_covers_every_wire_error_variant() {
        // Exhaustive match on WireError so any new variant fails the
        // build until the policy here is updated.
        let variants = [
            WireError::FrameTooLong(FrameTooLong { len: 1, max: 0 }),
            WireError::FrameTooShort(FrameTooShort { len: 0 }),
            WireError::DeserFailed,
            WireError::SerFailed,
            WireError::UnknownKey,
            WireError::FailedToSpawn,
            WireError::KeyTooSmall,
        ];
        for v in variants {
            // Force exhaustiveness check.
            match &v {
                WireError::FrameTooLong(_)
                | WireError::FrameTooShort(_)
                | WireError::DeserFailed
                | WireError::SerFailed
                | WireError::UnknownKey
                | WireError::FailedToSpawn
                | WireError::KeyTooSmall => {}
            }
            match v {
                WireError::UnknownKey | WireError::KeyTooSmall => assert_legacy(HostErr::Wire(v)),
                _ => assert_comms(HostErr::Wire(v)),
            }
        }
    }

    #[test]
    fn map_validate_error_covers_every_host_err_variant() {
        // Exhaustive coverage on HostErr<WireError> (excluding the
        // Wire variant, which is exercised above).
        let cases: [HostErr<WireError>; 3] = [
            HostErr::BadResponse,
            HostErr::Postcard(postcard::Error::DeserializeUnexpectedEnd),
            HostErr::Closed,
        ];
        for e in cases {
            // Force exhaustiveness.
            match &e {
                HostErr::Wire(_) | HostErr::BadResponse | HostErr::Postcard(_) | HostErr::Closed => {}
            }
            assert_comms(e);
        }
    }

    #[test]
    fn map_validate_error_deser_failed_routes_to_comms_not_legacy() {
        // Regression guard for the policy comment on map_validate_error:
        // DeserFailed must NOT be interpreted as "missing endpoint".
        assert_comms(HostErr::Wire(WireError::DeserFailed));
    }

    #[test]
    fn map_validate_error_unknown_key_is_legacy() {
        assert_legacy(HostErr::Wire(WireError::UnknownKey));
    }

    #[test]
    fn map_validate_error_key_too_small_is_legacy() {
        assert_legacy(HostErr::Wire(WireError::KeyTooSmall));
    }

    #[test]
    fn map_validate_error_closed_is_comms() {
        assert_comms(HostErr::Closed);
    }

    // --- check_schema_compatible policy (Category A finding #1) ---

    fn make_device_info(schema_major: u16, schema_minor: u16) -> DeviceInfo {
        DeviceInfo {
            fw_major: 0,
            fw_minor: 0,
            fw_patch: 0,
            schema_major,
            schema_minor,
            schema_patch: 0,
            hw_version: 1,
            capabilities: Capabilities::NONE,
            num_gpios: NUM_GPIOS as u8,
            build_id: "test-build".try_into().unwrap(),
        }
    }

    #[test]
    fn check_schema_compatible_accepts_matching_versions() {
        let info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
        check_schema_compatible(&info).expect("matching versions must validate");
    }

    #[test]
    fn check_schema_compatible_ignores_build_id() {
        // `build_id` is informational: it names the image, it does not gate
        // compatibility. If this ever starts failing, someone has wired the
        // field into the compatibility policy, which would force a dishonest
        // schema bump for every behavioural change (issue #159).
        for id in ["", "unknown", "firmware-v0.11.0-27-gdeadbee-dirty"] {
            let mut info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
            info.build_id = id.try_into().unwrap();
            check_schema_compatible(&info).unwrap_or_else(|e| panic!("build_id {id:?} must not gate: {e}"));
        }
    }

    #[test]
    fn device_info_exposes_build_id_accessor() {
        let mut info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
        info.build_id = "firmware-v0.11.0".try_into().unwrap();
        assert_eq!(info.build_id(), "firmware-v0.11.0");
    }

    #[test]
    fn check_schema_compatible_rejects_bumped_major() {
        // Pre-fix: validate() only compared schema_minor, so a bumped
        // major with matching minor would silently pass and the host
        // would mis-decode subsequent RPCs against an incompatible
        // firmware. This test guards the post-fix behavior.
        let info = make_device_info(SCHEMA_VERSION_MAJOR.wrapping_add(1), SCHEMA_VERSION_MINOR);
        match check_schema_compatible(&info) {
            Err(ValidateError::SchemaMismatch {
                expected_major,
                actual_major,
                expected_minor,
                actual_minor,
            }) => {
                assert_eq!(expected_major, SCHEMA_VERSION_MAJOR);
                assert_eq!(actual_major, SCHEMA_VERSION_MAJOR.wrapping_add(1));
                assert_eq!(expected_minor, SCHEMA_VERSION_MINOR);
                assert_eq!(actual_minor, SCHEMA_VERSION_MINOR);
            }
            other => panic!("expected SchemaMismatch on bumped major, got {other:?}"),
        }
    }

    #[test]
    fn check_schema_compatible_rejects_bumped_minor() {
        let info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR.wrapping_add(1));
        match check_schema_compatible(&info) {
            Err(ValidateError::SchemaMismatch { actual_minor, .. }) => {
                assert_eq!(actual_minor, SCHEMA_VERSION_MINOR.wrapping_add(1))
            }
            other => panic!("expected SchemaMismatch on bumped minor, got {other:?}"),
        }
    }

    #[test]
    fn check_schema_compatible_rejects_both_bumped() {
        let info = make_device_info(
            SCHEMA_VERSION_MAJOR.wrapping_add(1),
            SCHEMA_VERSION_MINOR.wrapping_add(2),
        );
        match check_schema_compatible(&info) {
            Err(ValidateError::SchemaMismatch {
                actual_major,
                actual_minor,
                ..
            }) => {
                assert_eq!(actual_major, SCHEMA_VERSION_MAJOR.wrapping_add(1));
                assert_eq!(actual_minor, SCHEMA_VERSION_MINOR.wrapping_add(2));
            }
            other => panic!("expected SchemaMismatch on both bumped, got {other:?}"),
        }
    }

    #[test]
    fn schema_mismatch_display_includes_both_versions() {
        let err = ValidateError::SchemaMismatch {
            expected_major: 0,
            actual_major: 1,
            expected_minor: 7,
            actual_minor: 0,
        };
        let s = format!("{err}");
        assert!(s.contains("0.7"), "expected '0.7' in display, got: {s}");
        assert!(s.contains("1.0"), "expected '1.0' in display, got: {s}");
    }

    // ===================================================================
    // M3 — SPI chip-select bounds (issue #104)
    // ===================================================================

    // --- L1-L5: error-type shape and diagnostics ---

    #[test]
    fn device_info_timeout_constant_is_three_hundred_seconds() {
        // L25/L26 drive the timeout path through a millisecond seam, so the
        // production value is only pinned here. Changing it silently would
        // otherwise pass the whole suite.
        assert_eq!(DEVICE_INFO_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn validate_error_timeout_display_names_endpoint_and_bound() {
        let s = format!("{}", ValidateError::Timeout);
        assert!(s.contains("device/info"), "got: {s}");
        assert!(s.contains("300"), "got: {s}");
    }

    /// A `device/info` timeout has two indistinguishable causes: a board
    /// that stopped answering, and a host/firmware pair built from
    /// different trees, whose differing endpoint keys make the reply be
    /// dropped as unmatched rather than decoded. The message must name
    /// both and must not advise a retry, which cannot help with the
    /// second.
    #[test]
    fn validate_error_timeout_display_names_build_mismatch() {
        let s = format!("{}", ValidateError::Timeout);
        assert!(s.contains("endpoint key"), "got: {s}");
        assert!(s.contains("different trees"), "got: {s}");
        let lower = s.to_lowercase();
        assert!(!lower.contains("retry"), "got: {s}");
        assert!(!lower.contains("replug"), "got: {s}");
    }

    fn all_validate_errors() -> Vec<ValidateError> {
        vec![
            ValidateError::Comms(HostErr::Closed),
            ValidateError::Timeout,
            ValidateError::LegacyFirmware,
            ValidateError::SchemaMismatch {
                expected_major: 0,
                actual_major: 0,
                expected_minor: 7,
                actual_minor: 6,
            },
        ]
    }

    #[test]
    fn validate_error_display_variants_are_pairwise_distinct() {
        let msgs: Vec<String> = all_validate_errors().iter().map(|e| format!("{e}")).collect();
        let unique: std::collections::HashSet<&String> = msgs.iter().collect();
        assert_eq!(unique.len(), msgs.len(), "collision in {msgs:?}");
        for m in &msgs {
            let lower = m.to_lowercase();
            assert!(!lower.contains("chip-select"), "metadata error leaked CS wording: {m}");
            assert!(!lower.contains("cs_pin"), "metadata error leaked CS wording: {m}");
        }
    }

    #[test]
    fn validate_error_witness_is_exhaustive() {
        // Compile-time witness (the M1 technique): appending a variant to
        // ValidateError must fail to *compile* here, forcing a deliberate
        // decision at every host surface, rather than silently falling
        // through a wildcard.
        fn witness(e: &ValidateError) -> u8 {
            match e {
                ValidateError::Comms(_) => 0,
                ValidateError::Timeout => 1,
                ValidateError::LegacyFirmware => 2,
                ValidateError::SchemaMismatch { .. } => 3,
            }
        }
        let tags: std::collections::HashSet<u8> = all_validate_errors().iter().map(witness).collect();
        assert_eq!(tags.len(), 4);
    }

    fn all_spi_batch_call_errors() -> Vec<SpiBatchCallError> {
        vec![
            SpiBatchCallError::DeviceInfo(ValidateError::Timeout),
            SpiBatchCallError::NoGpios,
            SpiBatchCallError::InvalidCsPin { cs: 255, num_gpios: 4 },
            SpiBatchCallError::Comms(HostErr::Closed),
            SpiBatchCallError::Timeout {
                waited: Duration::from_secs(5),
            },
            SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op: 0,
                kind: SpiError::Other,
            }),
        ]
    }

    #[test]
    fn spi_batch_call_error_witness_is_exhaustive() {
        fn witness(e: &SpiBatchCallError) -> u8 {
            match e {
                SpiBatchCallError::DeviceInfo(_) => 0,
                SpiBatchCallError::NoGpios => 1,
                SpiBatchCallError::InvalidCsPin { .. } => 2,
                SpiBatchCallError::Comms(_) => 3,
                SpiBatchCallError::Endpoint(_) => 4,
                SpiBatchCallError::Timeout { .. } => 5,
            }
        }
        let tags: std::collections::HashSet<u8> = all_spi_batch_call_errors().iter().map(witness).collect();
        assert_eq!(tags.len(), 6);
    }

    #[test]
    fn spi_batch_call_error_display_variants_are_pairwise_distinct() {
        let msgs: Vec<String> = all_spi_batch_call_errors().iter().map(|e| format!("{e}")).collect();
        let unique: std::collections::HashSet<&String> = msgs.iter().collect();
        assert_eq!(unique.len(), msgs.len(), "collision in {msgs:?}");

        let device_info = format!(
            "{}",
            SpiBatchCallError::DeviceInfo(ValidateError::Comms(HostErr::Closed))
        );
        let lower = device_info.to_lowercase();
        assert!(!lower.contains("chip-select"), "got: {device_info}");
        assert!(!lower.contains("pin"), "got: {device_info}");
    }

    // --- L6-L13: pure classifier boundaries ---

    #[test]
    fn classify_cs_accepts_zero_at_n_four() {
        classify_cs(0, 4).expect("pin 0 is valid when the device reports 4 GPIOs");
    }

    #[test]
    fn classify_cs_accepts_upper_boundary_at_n_four() {
        classify_cs(3, 4).expect("pin 3 is the last valid pin when n == 4");
    }

    #[test]
    fn classify_cs_rejects_exact_bound_at_n_four() {
        match classify_cs(4, 4) {
            Err(SpiBatchCallError::InvalidCsPin { cs, num_gpios }) => {
                assert_eq!((cs, num_gpios), (4, 4));
            }
            other => panic!("expected InvalidCsPin, got {other:?}"),
        }
    }

    #[test]
    fn classify_cs_rejects_max_u8_without_truncation() {
        // The payload must carry 255, not 3. A `cs & 3` truncation would
        // classify 255 as pin 3 and *accept* it — driving the wrong GPIO.
        // Asserting the payload is the only thing that kills that mutant.
        match classify_cs(255, 4) {
            Err(SpiBatchCallError::InvalidCsPin { cs, num_gpios }) => {
                assert_eq!(cs, 255);
                assert_eq!(num_gpios, 4);
            }
            other => panic!("expected InvalidCsPin{{cs:255}}, got {other:?}"),
        }
    }

    #[test]
    fn classify_cs_accepts_upper_boundary_at_n_seven() {
        classify_cs(6, 7).expect("nothing may hardcode NUM_GPIOS == 4");
    }

    #[test]
    fn classify_cs_rejects_exact_bound_at_n_seven() {
        match classify_cs(7, 7) {
            Err(SpiBatchCallError::InvalidCsPin { cs, num_gpios }) => {
                assert_eq!((cs, num_gpios), (7, 7));
            }
            other => panic!("expected InvalidCsPin, got {other:?}"),
        }
    }

    #[test]
    fn classify_cs_accepts_pin_four_only_when_bound_exceeds_four() {
        assert!(classify_cs(4, 4).is_err());
        classify_cs(4, 7).expect("the bound is read from the device, not assumed");
    }

    #[test]
    fn classify_cs_zero_bound_rejects_every_pin_as_no_gpios() {
        for cs in 0..=u8::MAX {
            match classify_cs(cs, 0) {
                Err(SpiBatchCallError::NoGpios) => {}
                other => panic!("cs {cs} at n=0 must be NoGpios, got {other:?}"),
            }
        }
    }

    // --- L14-L30: transport-backed `spi_batch` ---

    #[tokio::test]
    async fn spi_batch_unvalidated_fetches_info_then_sends_batch() {
        let (pg, script) = scripted(
            vec![Reply::DeviceInfo(good_info(4)), Reply::SpiBatchOk(vec![0xAA])],
            TEST_TIMEOUT,
        );
        let out = pg.spi_batch(0, &one_read_op()).await.expect("valid CS must succeed");
        assert_eq!(out, vec![0xAA]);
        assert_eq!(script.order(), vec!["device/info", "spi/batch"]);
    }

    #[tokio::test]
    async fn spi_batch_out_of_range_sends_no_batch_rpc() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4))], TEST_TIMEOUT);
        match pg.spi_batch(4, &one_read_op()).await {
            Err(SpiBatchCallError::InvalidCsPin { cs, num_gpios }) => assert_eq!((cs, num_gpios), (4, 4)),
            other => panic!("expected InvalidCsPin, got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0, "a local refusal must transmit nothing");
        assert_eq!(script.count("device/info"), 1);
    }

    #[tokio::test]
    async fn spi_batch_max_u8_cs_sends_no_batch_rpc() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4))], TEST_TIMEOUT);
        match pg.spi_batch(255, &one_read_op()).await {
            Err(SpiBatchCallError::InvalidCsPin { cs, .. }) => assert_eq!(cs, 255),
            other => panic!("expected InvalidCsPin{{cs:255}}, got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);
    }

    #[tokio::test]
    async fn spi_batch_zero_bound_sends_no_batch_rpc() {
        for cs in [0u8, 255u8] {
            let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(0))], TEST_TIMEOUT);
            match pg.spi_batch(cs, &one_read_op()).await {
                Err(SpiBatchCallError::NoGpios) => {}
                other => panic!("cs {cs} at n=0 must be NoGpios, got {other:?}"),
            }
            assert_eq!(script.count("spi/batch"), 0);
        }
    }

    #[tokio::test]
    async fn spi_batch_upper_boundary_sends_exactly_one_batch_rpc() {
        let (pg, script) = scripted(
            vec![Reply::DeviceInfo(good_info(4)), Reply::SpiBatchOk(vec![])],
            TEST_TIMEOUT,
        );
        pg.spi_batch(3, &one_read_op()).await.expect("pin 3 is valid at n = 4");
        assert_eq!(script.count("spi/batch"), 1);
    }

    #[tokio::test]
    async fn spi_batch_second_call_reuses_cached_bound() {
        let (pg, script) = scripted(
            vec![
                Reply::DeviceInfo(good_info(4)),
                Reply::SpiBatchOk(vec![]),
                Reply::SpiBatchOk(vec![]),
            ],
            TEST_TIMEOUT,
        );
        pg.spi_batch(0, &one_read_op()).await.expect("first batch");
        pg.spi_batch(0, &one_read_op()).await.expect("second batch");
        assert_eq!(script.count("device/info"), 1, "cache hit must not re-query");
        assert_eq!(script.count("spi/batch"), 2);
    }

    #[tokio::test]
    async fn spi_batch_clone_shares_metadata_cache() {
        let (pg, script) = scripted(
            vec![
                Reply::DeviceInfo(good_info(4)),
                Reply::SpiBatchOk(vec![]),
                Reply::SpiBatchOk(vec![]),
            ],
            TEST_TIMEOUT,
        );
        pg.spi_batch(0, &one_read_op()).await.expect("first batch");
        let clone = pg.clone();
        clone.spi_batch(0, &one_read_op()).await.expect("clone batch");
        assert_eq!(script.count("device/info"), 1, "clones share the Arc<OnceLock<u8>>");
    }

    #[tokio::test]
    async fn spi_batch_metadata_decode_failure_stays_device_info_comms() {
        // THE BINDING CONSTRAINT. cs = 0 is in range for any plausible bound,
        // so an implementation that fell back to NUM_GPIOS would happily
        // transmit the batch, and one that fell back to 0 would report
        // NoGpios. Both are wrong: the bound was never learned.
        let (pg, script) = scripted(vec![Reply::TruncatedInfo], TEST_TIMEOUT);
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::DeviceInfo(ValidateError::Comms(HostErr::Postcard(_)))) => {}
            other => panic!("expected DeviceInfo(Comms(Postcard(_))), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);
    }

    #[tokio::test]
    async fn spi_batch_metadata_failure_leaves_cache_empty_and_retries() {
        // A *decode* failure is recoverable; a WireRx error is not (it stops
        // the HostClient permanently), which is why this scripts a truncated
        // body rather than a transport error.
        let (pg, script) = scripted(
            vec![
                Reply::TruncatedInfo,
                Reply::DeviceInfo(good_info(4)),
                Reply::SpiBatchOk(vec![]),
            ],
            TEST_TIMEOUT,
        );
        assert!(matches!(
            pg.spi_batch(0, &one_read_op()).await,
            Err(SpiBatchCallError::DeviceInfo(ValidateError::Comms(_)))
        ));
        pg.spi_batch(0, &one_read_op()).await.expect("retry must succeed");
        assert_eq!(script.count("device/info"), 2, "a failure must not be cached");
        assert_eq!(script.count("spi/batch"), 1);
    }

    #[tokio::test]
    async fn spi_batch_metadata_wire_error_unknown_key_is_legacy_firmware() {
        let (pg, script) = scripted(vec![Reply::WireErr(WireError::UnknownKey)], TEST_TIMEOUT);
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::DeviceInfo(ValidateError::LegacyFirmware)) => {}
            other => panic!("expected DeviceInfo(LegacyFirmware), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);
    }

    #[tokio::test]
    async fn spi_batch_metadata_schema_mismatch_is_not_invalid_cs() {
        let mut info = good_info(4);
        info.schema_minor = SCHEMA_VERSION_MINOR.wrapping_add(1);
        let (pg, script) = scripted(vec![Reply::DeviceInfo(info)], TEST_TIMEOUT);
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::DeviceInfo(ValidateError::SchemaMismatch { .. })) => {}
            other => panic!("expected DeviceInfo(SchemaMismatch), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);
    }

    // --- issue #178: no RPC may block indefinitely ---

    /// The headline guarantee: a device that never answers produces an error
    /// rather than parking the caller forever.
    ///
    /// `Reply::Silent` is exactly the observed failure — an oversized request
    /// frame is dropped before the dispatcher sees it, so the board stays
    /// healthy and simply never replies.
    #[tokio::test]
    async fn silent_device_times_out_instead_of_hanging() {
        let (pg, _script) = scripted(vec![Reply::Silent], DEVICE_INFO_TIMEOUT);
        let pg = pg.with_call_timeout(Duration::from_millis(50));

        match pg.ping(0xABCD).await {
            Err(PicoDeGalloError::Timeout { waited }) => {
                assert_eq!(waited, Duration::from_millis(50));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    /// A timed-out call must not poison the handle.
    ///
    /// Abandoning the future drops its reply waiter, and sequence numbers are
    /// never reused, so a late reply is discarded rather than mismatched onto
    /// the next call. If that were wrong, the second call here would receive
    /// the first call's answer or fail outright.
    #[tokio::test]
    async fn handle_stays_usable_after_a_timeout() {
        let (pg, script) = scripted(
            vec![Reply::Silent, Reply::DeviceInfo(good_info(4))],
            DEVICE_INFO_TIMEOUT,
        );
        let pg = pg.with_call_timeout(Duration::from_millis(50));

        assert!(matches!(pg.ping(1).await, Err(PicoDeGalloError::Timeout { .. })));

        let info = pg.device_info().await.expect("second call must still work");
        assert_eq!(info.num_gpios, 4);
        assert_eq!(script.count("device/info"), 1);
    }

    /// The default must not be so tight that a legitimately slow call fails.
    /// The slowest measured operation is a 4096-byte 1-Wire read at ~2.4 s.
    #[test]
    fn default_call_timeout_clears_the_slowest_measured_operation() {
        assert_eq!(DEFAULT_CALL_TIMEOUT, Duration::from_secs(5));
        assert!(DEFAULT_CALL_TIMEOUT >= Duration::from_millis(2400) * 2);
    }

    /// `0` means the firmware's ceiling, not "no time". Reversing this would
    /// bound every `gpio_wait_*` at the transport slack alone.
    #[tokio::test]
    async fn bounded_for_zero_selects_the_handler_ceiling() {
        let (pg, _script) = scripted(vec![], DEVICE_INFO_TIMEOUT);
        let pg = pg.with_call_timeout(Duration::from_secs(5));

        let ceiling = Duration::from_millis(u64::from(MAX_HANDLER_TIMEOUT_MS));
        assert_eq!(pg.bounded_for(0).bound, ceiling + Duration::from_secs(5));
    }

    /// A caller-supplied duration widens the bound, and is clamped exactly
    /// where the firmware clamps it.
    #[tokio::test]
    async fn bounded_for_adds_slack_and_clamps_like_the_firmware() {
        let (pg, _script) = scripted(vec![], DEVICE_INFO_TIMEOUT);
        let pg = pg.with_call_timeout(Duration::from_secs(5));

        assert_eq!(
            pg.bounded_for(1_000).bound,
            Duration::from_secs(1) + Duration::from_secs(5)
        );

        let ceiling = Duration::from_millis(u64::from(MAX_HANDLER_TIMEOUT_MS));
        assert_eq!(
            pg.bounded_for(u32::MAX).bound,
            ceiling + Duration::from_secs(5),
            "an oversized request must clamp, not overflow"
        );
    }

    #[tokio::test]
    async fn with_call_timeout_overrides_the_default() {
        let (pg, _script) = scripted(vec![], DEVICE_INFO_TIMEOUT);
        assert_eq!(pg.call_timeout(), DEFAULT_CALL_TIMEOUT);

        let pg = pg.with_call_timeout(Duration::from_secs(30));
        assert_eq!(pg.call_timeout(), Duration::from_secs(30));
        assert_eq!(pg.bounded().bound, Duration::from_secs(30));
    }

    #[test]
    fn spi_batch_delay_budget_counts_only_delays() {
        let data = [0u8; 8];
        let ops = [
            SpiBatchOp::Write { data: &data },
            SpiBatchOp::DelayNs { ns: 3_000_000 },
            SpiBatchOp::Read { len: 16 },
            SpiBatchOp::DelayNs { ns: 2_000_000 },
        ];
        assert_eq!(spi_batch_delay_budget_ms(&ops), 5);
    }

    /// A sub-millisecond delay must still produce a non-zero budget, because
    /// `bounded_for(0)` means "the ceiling" — rounding down to zero here would
    /// silently turn a 100 µs batch into a 30-minute bound.
    #[test]
    fn spi_batch_delay_budget_rounds_sub_millisecond_up() {
        let ops = [SpiBatchOp::DelayNs { ns: 1 }];
        assert_eq!(spi_batch_delay_budget_ms(&ops), 1);
    }

    /// The documented worst case: 64 operations of `DelayNs { ns: u32::MAX }`
    /// is roughly 275 s, which must be represented rather than overflowing.
    #[test]
    fn spi_batch_delay_budget_handles_the_worst_legal_batch() {
        let ops = vec![SpiBatchOp::DelayNs { ns: u32::MAX }; MAX_BATCH_OPS];
        let ms = spi_batch_delay_budget_ms(&ops);
        assert_eq!(ms, 274_878);
        assert!(ms < MAX_HANDLER_TIMEOUT_MS);
    }

    #[test]
    fn spi_batch_delay_budget_saturates_at_the_handler_ceiling() {
        // Not reachable through the public API (MAX_BATCH_OPS bounds it), but
        // the saturation must be correct rather than wrapping.
        let ops = vec![SpiBatchOp::DelayNs { ns: u32::MAX }; 100_000];
        assert_eq!(spi_batch_delay_budget_ms(&ops), MAX_HANDLER_TIMEOUT_MS);
    }

    #[test]
    fn timeout_display_names_the_bound() {
        let e: PicoDeGalloError<Infallible> = PicoDeGalloError::Timeout {
            waited: Duration::from_millis(1500),
        };
        assert_eq!(e.to_string(), "device did not respond within 1.500 s");
    }

    #[test]
    fn spi_batch_timeout_display_admits_the_batch_may_have_run() {
        let e = SpiBatchCallError::Timeout {
            waited: Duration::from_secs(5),
        };
        let text = e.to_string();
        assert!(text.contains("may or may not have executed"), "got: {text}");
    }

    #[test]
    fn call_error_widens_to_pico_de_gallo_error() {
        let e: PicoDeGalloError<Infallible> = CallError::Timeout {
            waited: Duration::from_secs(2),
        }
        .into();
        assert!(matches!(
            e,
            PicoDeGalloError::Timeout { waited } if waited == Duration::from_secs(2)
        ));

        let e: PicoDeGalloError<Infallible> = CallError::Comms(HostErr::Closed).into();
        assert!(matches!(e, PicoDeGalloError::Comms(_)));
    }

    #[tokio::test]
    async fn spi_batch_metadata_timeout_is_device_info_timeout() {
        let (pg, script) = scripted(vec![Reply::Silent], Duration::from_millis(50));
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::DeviceInfo(ValidateError::Timeout)) => {}
            other => panic!("expected DeviceInfo(Timeout), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);
    }

    #[tokio::test]
    async fn spi_batch_metadata_timeout_leaves_cache_empty_and_retries() {
        let (pg, script) = scripted(
            vec![
                Reply::Silent,
                Reply::DeviceInfo(good_info(4)),
                Reply::SpiBatchOk(vec![]),
            ],
            Duration::from_millis(50),
        );
        assert!(matches!(
            pg.spi_batch(0, &one_read_op()).await,
            Err(SpiBatchCallError::DeviceInfo(ValidateError::Timeout))
        ));
        pg.spi_batch(0, &one_read_op()).await.expect("retry after timeout");
        assert_eq!(script.count("device/info"), 2);
    }

    #[tokio::test]
    async fn spi_batch_closed_transport_is_device_info_comms_not_invalid_cs() {
        let (pg, script) = scripted(vec![], TEST_TIMEOUT);
        script.dead_on_arrival();
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::DeviceInfo(ValidateError::Comms(HostErr::Closed))) => {}
            other => panic!("expected DeviceInfo(Comms(Closed)), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn spi_batch_endpoint_error_preserves_failed_op() {
        let (pg, _script) = scripted(
            vec![
                Reply::DeviceInfo(good_info(4)),
                Reply::SpiBatchErr(SpiBatchError {
                    failed_op: 2,
                    kind: SpiError::Other,
                }),
            ],
            TEST_TIMEOUT,
        );
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::Endpoint(e)) => assert_eq!(e.failed_op, 2),
            other => panic!("expected Endpoint, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn spi_batch_local_refusals_carry_no_failed_op() {
        let (pg, _s) = scripted(vec![Reply::DeviceInfo(good_info(4))], TEST_TIMEOUT);
        let range = pg.spi_batch(4, &one_read_op()).await;
        assert!(!matches!(range, Err(SpiBatchCallError::Endpoint(_))));

        let (pg0, _s0) = scripted(vec![Reply::DeviceInfo(good_info(0))], TEST_TIMEOUT);
        let zero = pg0.spi_batch(0, &one_read_op()).await;
        assert!(!matches!(zero, Err(SpiBatchCallError::Endpoint(_))));
    }

    #[tokio::test]
    async fn spi_batch_transport_error_after_metadata_is_comms_not_device_info() {
        let (pg, _script) = scripted(vec![Reply::DeviceInfo(good_info(4)), Reply::CloseWire], TEST_TIMEOUT);
        match pg.spi_batch(0, &one_read_op()).await {
            Err(SpiBatchCallError::Comms(_)) => {}
            other => panic!("batch-phase transport failure must be Comms, got {other:?}"),
        }
    }

    // --- L31-L36: `num_gpios` accessor and cache authority ---

    #[tokio::test]
    async fn num_gpios_returns_cached_value_without_second_fetch() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4))], TEST_TIMEOUT);
        pg.validate().await.expect("validate");
        assert_eq!(pg.num_gpios().await.expect("cached"), 4);
        assert_eq!(script.count("device/info"), 1);
    }

    #[tokio::test]
    async fn num_gpios_on_unvalidated_handle_fetches_and_caches() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(7))], TEST_TIMEOUT);
        assert_eq!(pg.num_gpios().await.expect("lazy fetch"), 7);
        assert_eq!(script.count("device/info"), 1);
        assert_eq!(pg.num_gpios().await.expect("cache hit"), 7);
        assert_eq!(script.count("device/info"), 1);
    }

    #[tokio::test]
    async fn num_gpios_zero_is_cached_and_returned_as_zero() {
        // Zero is a legitimate answer, not a cache miss. An implementation
        // using 0 as an "empty" sentinel would re-query here.
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(0))], TEST_TIMEOUT);
        assert_eq!(pg.num_gpios().await.expect("zero is a success"), 0);
        assert_eq!(pg.num_gpios().await.expect("zero is cached"), 0);
        assert_eq!(script.count("device/info"), 1);
    }

    #[tokio::test]
    async fn num_gpios_failure_is_not_cached() {
        let (pg, script) = scripted(
            vec![Reply::TruncatedInfo, Reply::DeviceInfo(good_info(4))],
            TEST_TIMEOUT,
        );
        assert!(pg.num_gpios().await.is_err());
        assert_eq!(pg.num_gpios().await.expect("retry"), 4);
        assert_eq!(script.count("device/info"), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn num_gpios_concurrent_misses_return_stored_winner() {
        // Amendment A3. The script answers the two racing requests with
        // *different* counts. An implementation that returns its own fetched
        // byte yields 4 and 7; the post-`set` re-read yields one winner to
        // both.
        let (pg, _script) = scripted(
            vec![Reply::DeviceInfo(good_info(4)), Reply::DeviceInfo(good_info(7))],
            TEST_TIMEOUT,
        );
        let a = pg.clone();
        let b = pg.clone();
        let ta = tokio::spawn(async move { a.num_gpios().await });
        let tb = tokio::spawn(async move { b.num_gpios().await });
        let ra = ta.await.unwrap().expect("racer a");
        let rb = tb.await.unwrap().expect("racer b");
        assert_eq!(ra, rb, "both racers must observe the stored winner");
        assert!(ra == 4 || ra == 7, "unexpected value {ra}");
    }

    #[tokio::test]
    async fn validate_returns_stored_bound_not_fetched_bound() {
        let (pg, _script) = scripted(
            vec![Reply::DeviceInfo(good_info(4)), Reply::DeviceInfo(good_info(7))],
            TEST_TIMEOUT,
        );
        let first = pg.validate().await.expect("first validate");
        assert_eq!(first.num_gpios, 4);
        let second = pg.validate().await.expect("second validate");
        assert_eq!(
            second.num_gpios, 4,
            "validate() must report the stored byte, not the freshly fetched one"
        );
    }

    // --- Zero-length I2C write refusal (issue #136) ---
    //
    // The firmware has refused an empty payload since #133, so these tests
    // are about refusing it *here*, without spending a USB round-trip to be
    // told no. The `count(...) == 0` assertions are the load-bearing ones:
    // they prove the guard fires before transmission rather than merely
    // reshaping the error that comes back.
    //
    // Each refusal test scripts a `CloseWire` it never expects to consume.
    // Without the guard the request is transmitted and the scripted reply
    // fails the call promptly; with an unscripted (`Silent`) plan the same
    // test would instead hang forever, exactly as the wedged device does —
    // an accurate reproduction, but a useless test failure.

    #[tokio::test]
    async fn i2c_write_empty_payload_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.i2c_write(0x48, &[]).await {
            Err(PicoDeGalloError::Endpoint(I2cError::ZeroLengthWrite)) => {}
            other => panic!("expected Endpoint(ZeroLengthWrite), got {other:?}"),
        }
        assert_eq!(script.count("i2c/write"), 0, "a local refusal must transmit nothing");
    }

    #[tokio::test]
    async fn i2c_write_non_empty_payload_reaches_the_wire() {
        // Positive control for the test above: proves the harness can see an
        // `i2c/write` at all, so its `count == 0` is a real observation and
        // not an artifact of the classifier missing the endpoint.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.i2c_write(0x48, &[0x01]).await;
        assert_eq!(script.count("i2c/write"), 1, "a non-empty write must still be sent");
    }

    #[tokio::test]
    async fn i2c_batch_empty_write_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![I2cBatchOp::Write { data: &[] }];
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::ZeroLengthWrite,
            })) => assert_eq!(failed_op, 0),
            other => panic!("expected Endpoint(I2cBatchError{{ZeroLengthWrite}}), got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0, "a local refusal must transmit nothing");
    }

    #[tokio::test]
    async fn i2c_batch_reports_index_of_offending_write() {
        // Catches a guard that always reports 0, and an off-by-one. The
        // firmware retains the exact index for validation errors (only bus
        // errors collapse to 0, because the transaction fails as a unit), so
        // the local refusal must agree. Confirmed on hardware: a batch of
        // [Read(1), Write([])] is refused by firmware with failed_op == 1.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![
            I2cBatchOp::Write { data: &[0x01] },
            I2cBatchOp::Read { len: 2 },
            I2cBatchOp::Write { data: &[] },
        ];
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::ZeroLengthWrite,
            })) => assert_eq!(failed_op, 2, "must name the offending operation, not the first"),
            other => panic!("expected Endpoint(I2cBatchError{{ZeroLengthWrite}}), got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0);
    }

    #[tokio::test]
    async fn i2c_batch_all_non_empty_writes_reach_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![I2cBatchOp::Write { data: &[0x01] }, I2cBatchOp::Read { len: 2 }];
        let _ = pg.i2c_batch(0x48, &ops).await;
        assert_eq!(script.count("i2c/batch"), 1, "a valid batch must still be sent");
    }

    // --- Aggregate batch-read ceiling (issue #179) ---
    //
    // A batch whose reads total more than `MAX_RESPONSE_PAYLOAD` used to be
    // accepted, executed on the bus, and only then lose its response to
    // transport truncation. The caller saw `DeserializeUnexpectedEnd`, which
    // reads like a comms fault and invites a retry that repeats every write.
    //
    // As with the zero-length guards above, the firmware refuses this too,
    // so the `count(...) == 0` assertions are the load-bearing ones: they
    // are what proves the refusal is local and costs no round-trip.

    #[tokio::test]
    async fn i2c_batch_read_total_at_the_ceiling_reaches_the_wire() {
        // Positive control. Without it, a guard that refused *every* batch
        // would pass the refusal tests below.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![I2cBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16,
        }];
        let _ = pg.i2c_batch(0x48, &ops).await;
        assert_eq!(
            script.count("i2c/batch"),
            1,
            "a batch reading exactly the ceiling is deliverable and must be sent"
        );
    }

    #[tokio::test]
    async fn i2c_batch_read_total_above_the_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![I2cBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16 + 1,
        }];
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::BufferTooLong,
            })) => assert_eq!(
                failed_op, 0,
                "an aggregate overflow is not attributable to one operation"
            ),
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0, "a local refusal must transmit nothing");
    }

    #[tokio::test]
    async fn i2c_batch_read_ceiling_is_aggregate_not_per_operation() {
        // Two individually legal reads that together exceed the ceiling.
        // A per-operation check would wave this through.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let half = MAX_RESPONSE_PAYLOAD as u16 / 2;
        let ops = vec![
            I2cBatchOp::Read { len: half },
            I2cBatchOp::Read {
                len: MAX_RESPONSE_PAYLOAD as u16 + 1 - half,
            },
        ];
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op: 0,
                kind: I2cError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0);
    }

    #[tokio::test]
    async fn i2c_batch_writes_do_not_count_towards_the_read_ceiling() {
        // Only Read data comes back, so a large write must not trip the
        // response guard. Getting this wrong would break gather writes.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; MAX_RESPONSE_PAYLOAD];
        let ops = vec![
            I2cBatchOp::Write { data: &payload },
            I2cBatchOp::Read {
                len: MAX_RESPONSE_PAYLOAD as u16,
            },
        ];
        let _ = pg.i2c_batch(0x48, &ops).await;
        assert_eq!(script.count("i2c/batch"), 1);
    }

    #[tokio::test]
    async fn i2c_batch_zero_length_write_outranks_an_oversized_read() {
        // Both guards fire; the firmware walks the operations in order and
        // reports the zero-length write with its exact index, checking the
        // aggregate only after the walk. The host must agree, or the same
        // batch reports two different errors depending on which side refused
        // it.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![
            I2cBatchOp::Read {
                len: MAX_RESPONSE_PAYLOAD as u16 + 1,
            },
            I2cBatchOp::Write { data: &[] },
        ];
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op: 1,
                kind: I2cError::ZeroLengthWrite,
            })) => {}
            other => panic!("expected Endpoint(I2cBatchError{{ZeroLengthWrite}}) at op 1, got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0);
    }

    #[tokio::test]
    async fn spi_batch_read_total_at_the_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4)), Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![SpiBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16,
        }];
        let _ = pg.spi_batch(0, &ops).await;
        assert_eq!(
            script.count("spi/batch"),
            1,
            "a batch reading exactly the ceiling is deliverable and must be sent"
        );
    }

    #[tokio::test]
    async fn spi_batch_read_total_above_the_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![SpiBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16 + 1,
        }];
        match pg.spi_batch(0, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op,
                kind: SpiError::BufferTooLong,
            })) => assert_eq!(
                failed_op, 0,
                "an aggregate overflow is not attributable to one operation"
            ),
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0, "a local refusal must transmit nothing");
        assert_eq!(
            script.count("device/info"),
            0,
            "the read ceiling needs no device metadata, so it must be checked \
             before the chip-select preflight spends a round-trip"
        );
    }

    #[tokio::test]
    async fn spi_batch_counts_transfer_towards_the_read_ceiling() {
        // `Transfer` returns as many bytes as it sends, so it consumes the
        // response budget exactly like `Read`. `Write` and `DelayNs` do not.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; MAX_RESPONSE_PAYLOAD + 1];
        let ops = vec![SpiBatchOp::Transfer { data: &payload }];
        match pg.spi_batch(0, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op: 0,
                kind: SpiError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);
    }

    #[tokio::test]
    async fn spi_batch_write_and_delay_do_not_count_towards_the_read_ceiling() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4)), Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; MAX_RESPONSE_PAYLOAD];
        let ops = vec![
            SpiBatchOp::Write { data: &payload },
            SpiBatchOp::DelayNs { ns: 1_000 },
            SpiBatchOp::Read {
                len: MAX_RESPONSE_PAYLOAD as u16,
            },
        ];
        let _ = pg.spi_batch(0, &ops).await;
        assert_eq!(script.count("spi/batch"), 1);
    }

    #[tokio::test]
    async fn spi_batch_read_ceiling_outranks_an_invalid_chip_select() {
        // The firmware walks and bounds the operations before it looks at
        // `cs_pin`, so a batch that is wrong in both ways must report
        // `BufferTooLong`, not `InvalidCsPin`. Checking locally also means
        // no `device/info` round-trip is spent on a batch that can never
        // work.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![SpiBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16 + 1,
        }];
        match pg.spi_batch(200, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op: 0,
                kind: SpiError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("device/info"), 0);
        assert_eq!(script.count("spi/batch"), 0);
    }

    #[tokio::test]
    async fn i2c_write_read_still_accepts_empty_contents() {
        // `i2c/write-read` with an empty payload is legal and must stay that
        // way: `write_read_async` passes `send_stop = false`, so it returns
        // early instead of parking on a `STOP_DET` that never arrives. This
        // was verified on hardware in #135. A guard that leaks from
        // `i2c_write` into this path would break address probing.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.i2c_write_read(0x48, &[], 2).await;
        assert_eq!(
            script.count("i2c/write-read"),
            1,
            "empty write-read must not be refused locally"
        );
    }

    // -------------------------------------------------------------------
    // Per-call payload ceilings (issue #158)
    // -------------------------------------------------------------------
    //
    // #179 bounded only the two *batch* endpoints, because those commit
    // `Write` operations to the bus before losing their response. The plain
    // endpoints commit nothing, so the same overflow is merely confusing
    // rather than destructive — but it is still reported as
    // `Comms(Postcard(DeserializeUnexpectedEnd))`, which names the transport
    // rather than the caller's argument and gives no hint what size would
    // have worked.
    //
    // Two independent ceilings apply, and which one binds depends on the
    // *direction* of the bytes, not on the endpoint:
    //
    //   * a length the device must send back  -> MAX_RESPONSE_PAYLOAD (1014)
    //   * a payload the caller sends          -> MAX_TRANSFER_SIZE    (4096)
    //
    // Measured in #158 and #179, the two budgets are independent: a
    // 1021-byte request payload did not move the response ceiling by one
    // byte. `spi/transfer` is the case that proves the distinction is real
    // rather than cosmetic — it is full duplex, so its single argument is
    // both, and the tighter of the two must win.
    //
    // As in the #179 block above, the `count(...) == 0` assertions are the
    // load-bearing ones, and each refusal test scripts a `CloseWire` it
    // never expects to consume so that a regression fails promptly instead
    // of hanging.

    /// Every response-bearing entry point, as
    /// `(name, at-ceiling call, over-ceiling call)`.
    ///
    /// Written out per test rather than as a loop so a failure names the
    /// endpoint that broke.
    #[tokio::test]
    async fn i2c_read_at_the_response_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.i2c_read(0x48, MAX_RESPONSE_PAYLOAD as u16).await;
        assert_eq!(script.count("i2c/read"), 1, "a deliverable read must still be sent");
    }

    #[tokio::test]
    async fn i2c_read_above_the_response_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.i2c_read(0x48, MAX_RESPONSE_PAYLOAD as u16 + 1).await {
            Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("i2c/read"), 0, "a local refusal must transmit nothing");
    }

    #[tokio::test]
    async fn i2c_write_read_at_the_response_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.i2c_write_read(0x48, &[0x00], MAX_RESPONSE_PAYLOAD as u16).await;
        assert_eq!(script.count("i2c/write-read"), 1);
    }

    #[tokio::test]
    async fn i2c_write_read_above_the_response_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.i2c_write_read(0x48, &[0x00], MAX_RESPONSE_PAYLOAD as u16 + 1).await {
            Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("i2c/write-read"), 0);
    }

    #[tokio::test]
    async fn i2c_write_read_bounds_its_write_by_the_transfer_limit() {
        // The write phase is a request payload, so it gets the *looser*
        // ceiling. A 2000-byte write with a small read must go out: applying
        // the response ceiling to both halves would refuse a legal call.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.i2c_write_read(0x48, &vec![0xA5; 2000], 2).await;
        assert_eq!(
            script.count("i2c/write-read"),
            1,
            "the write phase must not inherit the response ceiling"
        );
    }

    #[tokio::test]
    async fn i2c_write_read_above_the_transfer_limit_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.i2c_write_read(0x48, &vec![0xA5; MAX_TRANSFER_SIZE + 1], 2).await {
            Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("i2c/write-read"), 0);
    }

    #[tokio::test]
    async fn spi_read_at_the_response_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.spi_read(MAX_RESPONSE_PAYLOAD as u16).await;
        assert_eq!(script.count("spi/read"), 1);
    }

    #[tokio::test]
    async fn spi_read_above_the_response_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.spi_read(MAX_RESPONSE_PAYLOAD as u16 + 1).await {
            Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("spi/read"), 0);
    }

    #[tokio::test]
    async fn spi_transfer_at_the_response_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.spi_transfer(&vec![0xA5; MAX_RESPONSE_PAYLOAD]).await;
        assert_eq!(script.count("spi/transfer"), 1);
    }

    #[tokio::test]
    async fn spi_transfer_is_bounded_by_the_response_ceiling_not_the_transfer_limit() {
        // The case that proves the two ceilings are genuinely distinct.
        // `spi/transfer` is full duplex: its one argument is simultaneously
        // the request payload and the response length, so the tighter bound
        // wins. 1015 is comfortably legal as a request (MAX_TRANSFER_SIZE is
        // 4096) and would have been accepted by a guard that only knew about
        // the request budget — and would then have lost its response.
        // `MAX_RESPONSE_PAYLOAD + 1` being a legal request size is what
        // makes this test meaningful; a `const` assertion beside
        // `request_payload_is_too_long` keeps it true.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.spi_transfer(&vec![0xA5; MAX_RESPONSE_PAYLOAD + 1]).await {
            Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("spi/transfer"), 0);
    }

    #[tokio::test]
    async fn uart_read_at_the_response_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.uart_read(MAX_RESPONSE_PAYLOAD as u16, 10).await;
        assert_eq!(script.count("uart/read"), 1);
    }

    #[tokio::test]
    async fn uart_read_above_the_response_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.uart_read(MAX_RESPONSE_PAYLOAD as u16 + 1, 10).await {
            Err(PicoDeGalloError::Endpoint(UartError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("uart/read"), 0);
    }

    #[tokio::test]
    async fn onewire_read_at_the_response_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.onewire_read(MAX_RESPONSE_PAYLOAD as u16).await;
        assert_eq!(script.count("onewire/read"), 1);
    }

    #[tokio::test]
    async fn onewire_read_above_the_response_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.onewire_read(MAX_RESPONSE_PAYLOAD as u16 + 1).await {
            Err(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("onewire/read"), 0);
    }

    // Request-bearing paths. These carry the looser MAX_TRANSFER_SIZE
    // ceiling, and each at-limit control doubles as proof that the guard is
    // not simply refusing everything.

    #[tokio::test]
    async fn i2c_write_at_the_transfer_limit_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.i2c_write(0x48, &vec![0xA5; MAX_TRANSFER_SIZE]).await;
        assert_eq!(script.count("i2c/write"), 1);
    }

    #[tokio::test]
    async fn i2c_write_above_the_transfer_limit_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.i2c_write(0x48, &vec![0xA5; MAX_TRANSFER_SIZE + 1]).await {
            Err(PicoDeGalloError::Endpoint(I2cError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("i2c/write"), 0);
    }

    #[tokio::test]
    async fn spi_write_at_the_transfer_limit_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.spi_write(&vec![0xA5; MAX_TRANSFER_SIZE]).await;
        assert_eq!(script.count("spi/write"), 1);
    }

    #[tokio::test]
    async fn spi_write_above_the_transfer_limit_sends_no_rpc() {
        // Note the asymmetry with `spi_transfer` above: `spi/write` returns
        // no payload, so it keeps the looser ceiling and 1015 bytes is fine
        // here. #158 measured `spi/write` succeeding at 1015 on hardware,
        // which is why the two endpoints cannot share one bound.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.spi_write(&vec![0xA5; MAX_TRANSFER_SIZE + 1]).await {
            Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("spi/write"), 0);
    }

    #[tokio::test]
    async fn spi_write_between_the_two_ceilings_still_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.spi_write(&vec![0xA5; MAX_RESPONSE_PAYLOAD + 1]).await;
        assert_eq!(
            script.count("spi/write"),
            1,
            "a write returns nothing, so the response ceiling must not apply"
        );
    }

    #[tokio::test]
    async fn uart_write_at_the_transfer_limit_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.uart_write(&vec![0xA5; MAX_TRANSFER_SIZE]).await;
        assert_eq!(script.count("uart/write"), 1);
    }

    #[tokio::test]
    async fn uart_write_above_the_transfer_limit_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.uart_write(&vec![0xA5; MAX_TRANSFER_SIZE + 1]).await {
            Err(PicoDeGalloError::Endpoint(UartError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("uart/write"), 0);
    }

    #[tokio::test]
    async fn onewire_write_at_the_transfer_limit_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.onewire_write(&vec![0xA5; MAX_TRANSFER_SIZE]).await;
        assert_eq!(script.count("onewire/write"), 1);
    }

    #[tokio::test]
    async fn onewire_write_above_the_transfer_limit_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.onewire_write(&vec![0xA5; MAX_TRANSFER_SIZE + 1]).await {
            Err(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("onewire/write"), 0);
    }

    #[tokio::test]
    async fn onewire_write_pullup_at_the_transfer_limit_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let _ = pg.onewire_write_pullup(&vec![0xA5; MAX_TRANSFER_SIZE], 1).await;
        assert_eq!(script.count("onewire/write-pullup"), 1);
    }

    #[tokio::test]
    async fn onewire_write_pullup_above_the_transfer_limit_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        match pg.onewire_write_pullup(&vec![0xA5; MAX_TRANSFER_SIZE + 1], 1).await {
            Err(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }
        assert_eq!(script.count("onewire/write-pullup"), 0);
    }

    // The two policy predicates, tested directly at their boundaries so a
    // drift in either constant is attributed to the predicate rather than to
    // whichever endpoint test happens to run first.

    #[test]
    fn response_len_boundary_is_max_response_payload() {
        assert!(!response_len_is_undeliverable(0));
        assert!(!response_len_is_undeliverable(MAX_RESPONSE_PAYLOAD - 1));
        assert!(!response_len_is_undeliverable(MAX_RESPONSE_PAYLOAD));
        assert!(response_len_is_undeliverable(MAX_RESPONSE_PAYLOAD + 1));
    }

    #[test]
    fn request_payload_boundary_is_max_transfer_size() {
        assert!(!request_payload_is_too_long(0));
        assert!(!request_payload_is_too_long(MAX_TRANSFER_SIZE - 1));
        assert!(!request_payload_is_too_long(MAX_TRANSFER_SIZE));
        assert!(request_payload_is_too_long(MAX_TRANSFER_SIZE + 1));
    }

    #[test]
    fn request_frame_boundary_is_max_request_frame() {
        assert!(!request_frame_is_undeliverable(0));
        assert!(!request_frame_is_undeliverable(MAX_REQUEST_FRAME - 1));
        assert!(!request_frame_is_undeliverable(MAX_REQUEST_FRAME));
        assert!(request_frame_is_undeliverable(MAX_REQUEST_FRAME + 1));
    }

    // --- Batch request-frame ceiling (issue #186) ---
    //
    // The send-direction mirror of the read-ceiling tests above, and the
    // one place where the `count(...) == 0` assertions are not merely
    // load-bearing but the *whole* proof: the firmware cannot refuse an
    // over-ceiling frame, because it never receives one. If these guards
    // were removed there would be no second line of defence, only a
    // timeout.
    //
    // Sizes are expressed as "the payload that makes the frame exactly
    // MAX_REQUEST_FRAME" rather than as literals, so the tests follow the
    // constant instead of pinning a number twice.

    /// Payload length whose single-`Write` I2C batch frame is exactly
    /// [`MAX_REQUEST_FRAME`].
    fn i2c_payload_filling_the_frame() -> usize {
        let probe = vec![0u8; MAX_REQUEST_FRAME];
        let overhead = i2c_batch_request_frame_len(&[I2cBatchOp::Write { data: &probe }]) - MAX_REQUEST_FRAME;
        MAX_REQUEST_FRAME - overhead
    }

    #[tokio::test]
    async fn i2c_batch_request_at_the_frame_ceiling_reaches_the_wire() {
        // Positive control, and the one that would catch an off-by-one in
        // the conservative direction: 5119 bytes is measured-good on
        // hardware, so refusing it would break working calls.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; i2c_payload_filling_the_frame()];
        let ops = vec![I2cBatchOp::Write { data: &payload }];
        assert_eq!(i2c_batch_request_frame_len(&ops), MAX_REQUEST_FRAME);
        let _ = pg.i2c_batch(0x48, &ops).await;
        assert_eq!(
            script.count("i2c/batch"),
            1,
            "a batch whose frame exactly fills the ceiling is deliverable and must be sent"
        );
    }

    #[tokio::test]
    async fn i2c_batch_request_above_the_frame_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; i2c_payload_filling_the_frame() + 1];
        let ops = vec![I2cBatchOp::Write { data: &payload }];
        assert_eq!(i2c_batch_request_frame_len(&ops), MAX_REQUEST_FRAME + 1);
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::BufferTooLong,
            })) => assert_eq!(
                failed_op, 0,
                "an aggregate overflow is not attributable to one operation"
            ),
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(
            script.count("i2c/batch"),
            0,
            "an over-ceiling frame must be refused locally; the firmware never sees it"
        );
    }

    #[tokio::test]
    async fn i2c_batch_request_ceiling_is_aggregate_not_per_operation() {
        // Eight writes, each an eighth of the budget, none of them
        // individually remarkable. This is the shape measured on hardware
        // (8 x 635 bytes dropped, 8 x 634 accepted) and the one a
        // per-operation bound would wave straight through.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let per = i2c_payload_filling_the_frame() / 8;
        let payload = vec![0xA5u8; per];
        let ops: Vec<I2cBatchOp<'_>> = (0..8).map(|_| I2cBatchOp::Write { data: &payload }).collect();
        assert!(
            i2c_batch_request_frame_len(&ops) > MAX_REQUEST_FRAME,
            "fixture must exceed the ceiling in aggregate"
        );
        assert!(
            per <= MAX_TRANSFER_SIZE,
            "and must do so with every operation individually legal"
        );
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op: 0,
                kind: I2cError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0);
    }

    #[tokio::test]
    async fn i2c_batch_reads_do_not_count_towards_the_request_ceiling() {
        // A `Read` costs three bytes going out however many it brings back.
        // Confusing the two directions would refuse every large read batch,
        // which is #179's mistake in mirror image.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let ops = vec![I2cBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16,
        }];
        assert!(i2c_batch_request_frame_len(&ops) < 32);
        let _ = pg.i2c_batch(0x48, &ops).await;
        assert_eq!(script.count("i2c/batch"), 1);
    }

    #[tokio::test]
    async fn i2c_batch_zero_length_write_outranks_an_oversized_frame() {
        // Same ordering rule as the read ceiling: the attributable
        // per-operation fault is reported with its exact index before
        // either aggregate.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; i2c_payload_filling_the_frame() + 1];
        let ops = vec![I2cBatchOp::Write { data: &payload }, I2cBatchOp::Write { data: &[] }];
        match pg.i2c_batch(0x48, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op: 1,
                kind: I2cError::ZeroLengthWrite,
            })) => {}
            other => panic!("expected Endpoint(I2cBatchError{{ZeroLengthWrite}}) at op 1, got {other:?}"),
        }
        assert_eq!(script.count("i2c/batch"), 0);
    }

    /// Payload length whose single-`Write` SPI batch frame is exactly
    /// [`MAX_REQUEST_FRAME`].
    fn spi_payload_filling_the_frame() -> usize {
        let probe = vec![0u8; MAX_REQUEST_FRAME];
        let overhead = spi_batch_request_frame_len(&[SpiBatchOp::Write { data: &probe }]) - MAX_REQUEST_FRAME;
        MAX_REQUEST_FRAME - overhead
    }

    #[tokio::test]
    async fn spi_batch_request_at_the_frame_ceiling_reaches_the_wire() {
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4)), Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; spi_payload_filling_the_frame()];
        let ops = vec![SpiBatchOp::Write { data: &payload }];
        assert_eq!(spi_batch_request_frame_len(&ops), MAX_REQUEST_FRAME);
        let _ = pg.spi_batch(0, &ops).await;
        assert_eq!(script.count("spi/batch"), 1);
    }

    #[tokio::test]
    async fn spi_batch_request_above_the_frame_ceiling_sends_no_rpc() {
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        let payload = vec![0xA5u8; spi_payload_filling_the_frame() + 1];
        let ops = vec![SpiBatchOp::Write { data: &payload }];
        match pg.spi_batch(0, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op,
                kind: SpiError::BufferTooLong,
            })) => assert_eq!(failed_op, 0),
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0, "a local refusal must transmit nothing");
        assert_eq!(
            script.count("device/info"),
            0,
            "the frame ceiling needs no device metadata, so it must be checked \
             before the chip-select preflight spends a round-trip"
        );
    }

    #[tokio::test]
    async fn spi_batch_counts_transfer_towards_the_request_ceiling() {
        // `Transfer` is the one operation bounded by both ceilings, which
        // makes it awkward to test: a batch of transfers big enough to
        // reach the frame ceiling trips the far tighter response ceiling
        // first, and would pass this test for the wrong reason. So the bulk
        // is carried by `Write`s — which return nothing — and a single
        // small `Transfer` tips the frame over. The response total is then
        // just that transfer, comfortably legal, so only the request check
        // can be what fires.
        let (pg, script) = scripted(vec![Reply::CloseWire], TEST_TIMEOUT);
        // The write is eight bytes short of filling the frame on its own;
        // the transfer costs ten (variant byte, length varint, payload), so
        // together they overrun by two.
        let bulk = vec![0xA5u8; spi_payload_filling_the_frame() - 8];
        let tip = vec![0xA5u8; 8];
        let ops = vec![SpiBatchOp::Write { data: &bulk }, SpiBatchOp::Transfer { data: &tip }];
        assert!(
            !response_len_is_undeliverable(spi_batch_response_len(&ops)),
            "fixture must be legal in the response direction, or this test \
             proves nothing about the request direction"
        );
        assert_eq!(spi_batch_request_frame_len(&ops), MAX_REQUEST_FRAME + 2);
        match pg.spi_batch(0, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op: 0,
                kind: SpiError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert_eq!(script.count("spi/batch"), 0);

        // Control: the same batch with the transfer removed fits, so it is
        // the transfer's outgoing bytes that made the difference and not
        // the bulk write on its own.
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4)), Reply::CloseWire], TEST_TIMEOUT);
        assert!(spi_batch_request_frame_len(&ops[..1]) <= MAX_REQUEST_FRAME);
        let _ = pg.spi_batch(0, &ops[..1]).await;
        assert_eq!(script.count("spi/batch"), 1);
    }

    #[tokio::test]
    async fn spi_batch_delay_and_read_barely_touch_the_request_ceiling() {
        // The counterpart of the I2C read test: neither `Read` nor
        // `DelayNs` carries a payload out, so a batch full of them must
        // stay far below the frame ceiling.
        let (pg, script) = scripted(vec![Reply::DeviceInfo(good_info(4)), Reply::CloseWire], TEST_TIMEOUT);
        let mut ops: Vec<SpiBatchOp<'_>> = Vec::new();
        for _ in 0..(MAX_BATCH_OPS / 2) {
            ops.push(SpiBatchOp::Read { len: 8 });
            ops.push(SpiBatchOp::DelayNs { ns: u32::MAX });
        }
        assert!(spi_batch_request_frame_len(&ops) < 512);
        let _ = pg.spi_batch(0, &ops).await;
        assert_eq!(script.count("spi/batch"), 1);
    }

    // -------------------------------------------------------------------
    // UART framing re-exports
    // -------------------------------------------------------------------

    #[test]
    fn uart_framing_enums_are_reachable_through_this_crate() {
        // Load-bearing: the FFI, CLI and Python crates name these through
        // `pico_de_gallo_lib::`, not through `pico_de_gallo_internal::`.
        // A dropped re-export would break them without touching this crate.
        let data_bits: crate::UartDataBits = crate::UartDataBits::Eight;
        let parity: crate::UartParity = crate::UartParity::None;
        let stop_bits: crate::UartStopBits = crate::UartStopBits::One;

        assert_eq!(data_bits, UartDataBits::Eight);
        assert_eq!(parity, UartParity::None);
        assert_eq!(stop_bits, UartStopBits::One);
    }

    #[test]
    fn uart_configuration_info_carries_the_framing_fields() {
        // Pins the shape `uart_get_config` returns, so a wire-side field
        // rename shows up here rather than only in a downstream crate.
        let info = UartConfigurationInfo {
            baud_rate: 115_200,
            data_bits: UartDataBits::Eight,
            parity: UartParity::None,
            stop_bits: UartStopBits::One,
        };

        assert_eq!(info.baud_rate, 115_200);
        assert_eq!(info.data_bits, UartDataBits::Eight);
        assert_eq!(info.parity, UartParity::None);
        assert_eq!(info.stop_bits, UartStopBits::One);
    }

    #[test]
    fn uart_set_configuration_request_carries_all_four_fields() {
        // This pins the shape of the request struct only: that it carries all
        // four fields and that they are distinguishable (e.g. parity and stop
        // bits are not the same type and cannot be silently transposed here).
        //
        // It does NOT exercise `uart_set_config`'s argument forwarding: the
        // method is never invoked. Catching an omitted, duplicated or
        // transposed argument inside `uart_set_config` would require the
        // scripted transport to decode and inspect the transmitted request.
        let req = UartSetConfigurationRequest {
            baud_rate: 9600,
            data_bits: UartDataBits::Seven,
            parity: UartParity::Even,
            stop_bits: UartStopBits::Two,
        };

        assert_eq!(req.baud_rate, 9600);
        assert_eq!(req.data_bits, UartDataBits::Seven);
        assert_eq!(req.parity, UartParity::Even);
        assert_eq!(req.stop_bits, UartStopBits::Two);
    }
}

/// Hardware-in-the-loop checks for the batch guards: the zero-length I2C
/// write (issue #136), the aggregate batch-read ceiling (issue #179), the
/// per-call ceilings (issue #158) and the batch request-frame ceiling
/// (issue #186).
///
/// **Ignored by default**: these need a board attached, so CI — which has
/// none — must not run them. Run them explicitly:
///
/// ```bash
/// cargo test -p pico-de-gallo-lib -- --ignored --test-threads=1
/// ```
///
/// # Bench setup
///
/// - One Pico de Gallo attached. Set `GALLO_TEST_SERIAL` to pick a
///   specific board; with exactly one attached it may be omitted.
/// - An I2C target that tolerates a pointer-register write at
///   `GALLO_TEST_I2C_ADDR` (default `0x48`, a TMP102). Needed by
///   [`empty_batch_write_never_reaches_the_bus`],
///   [`i2c_batch_at_the_response_ceiling_still_returns_data`] and
///   [`oversized_i2c_batch_never_reaches_the_bus`]; the rest need any
///   responding address.
/// - **Nothing at `0x50`.** The #186 frame-ceiling tests address it
///   deliberately, so that a batch which escaped the guard would be NACKed
///   rather than driving five kilobytes into a real chip.
/// - GPIO 0 free to be driven. [`oversized_spi_batch_never_moves_chip_select`]
///   and [`oversized_spi_batch_frame_never_moves_chip_select`] use it as a
///   chip-select witness and leave it an output, low.
///
/// # Why these exist on top of the unit tests
///
/// The unit tests prove the guards refuse locally and transmit nothing,
/// against a scripted transport. They cannot prove the guards left real
/// bus behaviour alone. These do: positive controls that a normal write
/// and a maximal deliverable batch still work, the `write-read` path that
/// is legal-but-empty, a pointer-register witness showing a refused batch
/// never drove its leading operation onto the bus, and a chip-select
/// witness showing a refused SPI batch never asserted CS.
///
/// # What these cannot prove
///
/// They cannot show the refusal was *local*. Firmware refuses both an
/// empty payload (since #133) and an undeliverable read total (since
/// #179), and these guards deliberately return the identical error value,
/// so removing the host guard would leave every assertion here still
/// passing — only slower. Locality is proven by the `count(...) == 0`
/// assertions in the unit tests, and nowhere else. Proving the *firmware*
/// guard specifically requires a host binary built without the guard; see
/// the A/B run recorded in AGENTS.md §13.17.
///
/// A useful bench control for the I2C half, since most of those drive the
/// bus: point `GALLO_TEST_I2C_ADDR` at an unpopulated address and they
/// must fail with `NoAcknowledge`. Only
/// [`empty_write_is_refused_and_the_device_stays_responsive`] should still
/// pass, because its guard fires before any addressing happens. The two
/// SPI tests touch no I2C target and are unaffected.
#[cfg(test)]
mod hardware {
    use super::*;

    /// Serialises the suite over the single board. WinUSB grants exclusive
    /// interface access, so two concurrent tests would fight over the claim
    /// and fail as if the driver were broken.
    static BENCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Open the board under test, failing loudly if the bench is not set up.
    ///
    /// Validates up front so a schema mismatch is reported as a bench
    /// problem here rather than as a mysterious product failure later.
    async fn board() -> PicoDeGallo {
        let pg = match std::env::var("GALLO_TEST_SERIAL") {
            Ok(s) => PicoDeGallo::try_new_with_serial_number(&s)
                .unwrap_or_else(|e| panic!("GALLO_TEST_SERIAL={s} is set but that board did not open: {e}")),
            Err(_) => {
                let attached = list_devices();
                assert_eq!(
                    attached.len(),
                    1,
                    "expected exactly one attached board (found {}); set GALLO_TEST_SERIAL to choose one",
                    attached.len()
                );
                PicoDeGallo::try_new().expect("the sole attached board did not open")
            }
        };
        pg.validate()
            .await
            .expect("attached firmware failed validation; flash a build matching this tree");
        pg
    }

    /// The witness target address.
    fn addr() -> u8 {
        match std::env::var("GALLO_TEST_I2C_ADDR") {
            Ok(s) => {
                let t = s.trim();
                let parsed = t
                    .strip_prefix("0x")
                    .or_else(|| t.strip_prefix("0X"))
                    .map(|h| u8::from_str_radix(h, 16))
                    .unwrap_or_else(|| t.parse::<u8>());
                parsed.unwrap_or_else(|e| panic!("GALLO_TEST_I2C_ADDR={s:?} is not a byte: {e}"))
            }
            Err(_) => 0x48,
        }
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn empty_write_is_refused_and_the_device_stays_responsive() {
        let _bench = BENCH.lock().await;
        let pg = board().await;
        let a = addr();

        match pg.i2c_write(a, &[]).await {
            Err(PicoDeGalloError::Endpoint(I2cError::ZeroLengthWrite)) => {}
            other => panic!("expected Endpoint(ZeroLengthWrite), got {other:?}"),
        }

        // The whole point of issue #101 was that this request wedged every
        // endpoint device-wide. Prove the device still answers.
        pg.i2c_scan(false)
            .await
            .expect("device unresponsive after a refused empty write");
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn non_empty_write_still_reaches_the_bus() {
        // Positive control. Without this, the test above would also pass if
        // the guard refused *every* write.
        let _bench = BENCH.lock().await;
        let pg = board().await;
        pg.i2c_write(addr(), &[0x00])
            .await
            .expect("a one-byte pointer write must still succeed");
    }

    #[tokio::test]
    #[ignore = "requires an attached board and a TMP102-like target; see module docs"]
    async fn empty_batch_write_never_reaches_the_bus() {
        // Replicates the witness method from #135: if the refused batch had
        // executed its leading operation, the target's pointer register
        // would move and the follow-up read would return a different
        // register. It must not.
        let _bench = BENCH.lock().await;
        let pg = board().await;
        let a = addr();

        pg.i2c_write(a, &[0x00]).await.expect("seed pointer to register 0x00");
        let before = pg.i2c_read(a, 2).await.expect("baseline read");

        let ops = vec![I2cBatchOp::Write { data: &[0x01] }, I2cBatchOp::Write { data: &[] }];
        match pg.i2c_batch(a, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::ZeroLengthWrite,
            })) => assert_eq!(failed_op, 1, "must name the offending operation"),
            other => panic!("expected Endpoint(I2cBatchError{{ZeroLengthWrite}}), got {other:?}"),
        }

        let after = pg.i2c_read(a, 2).await.expect("witness read");
        assert_eq!(
            before, after,
            "the refused batch moved the pointer register, so its leading write reached the bus"
        );
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn empty_write_read_still_returns_data() {
        // `i2c/write-read` with an empty write phase is legal, because that
        // transfer does not terminate with a STOP. Verified on hardware in
        // #135; pinned here so the write guard cannot leak into this path.
        let _bench = BENCH.lock().await;
        let pg = board().await;
        let got = pg
            .i2c_write_read(addr(), &[], 2)
            .await
            .expect("empty write-read must still work");
        assert_eq!(got.len(), 2);
    }

    // --- Aggregate batch-read ceiling (issue #179) ---
    //
    // These pin the boundary and boundary+1 on real hardware, and prove the
    // refusal happens before the bus moves. The witnesses are the ones the
    // repo has used before: a TMP102 pointer register for I2C (as in #135
    // and #160), and the chip-select pin's own level for SPI.
    //
    // # What these cannot prove
    //
    // They cannot show which side refused. `pico-de-gallo-lib` and the
    // firmware return the identical error value on purpose, so deleting the
    // host guard leaves every assertion here passing. Locality is proven by
    // the `count(...) == 0` assertions in the unit tests above. Proving the
    // *firmware* guard specifically needs a host built without the guard —
    // the A/B run recorded in AGENTS.md §13.17 used a pre-fix `gallo`
    // binary against post-fix firmware for exactly that.

    /// The registers used as an I2C witness.
    ///
    /// TLOW and THIGH hold stable, distinct power-on values (`0x4B00` and
    /// `0x5000` on a TMP102), unlike the temperature register, which
    /// changes between reads and would make an equality assertion flaky.
    const TMP102_TLOW: u8 = 0x02;
    const TMP102_THIGH: u8 = 0x03;

    #[tokio::test]
    #[ignore = "requires an attached board and a TMP102-like target; see module docs"]
    async fn i2c_batch_at_the_response_ceiling_still_returns_data() {
        // Positive control for the refusal test below: the largest
        // deliverable batch must still run, return every byte, and have its
        // leading write take effect.
        let _bench = BENCH.lock().await;
        let pg = board().await;
        let a = addr();

        let ops = vec![
            I2cBatchOp::Write { data: &[TMP102_TLOW] },
            I2cBatchOp::Read {
                len: MAX_RESPONSE_PAYLOAD as u16,
            },
        ];
        let got = pg
            .i2c_batch(a, &ops)
            .await
            .expect("a batch reading exactly the ceiling must still be delivered");
        assert_eq!(got.len(), MAX_RESPONSE_PAYLOAD, "the whole response must arrive");

        let after = pg.i2c_read(a, 1).await.expect("witness read");
        assert_eq!(
            after[0], 0x4B,
            "the accepted batch's leading write must have selected TLOW"
        );
    }

    #[tokio::test]
    #[ignore = "requires an attached board and a TMP102-like target; see module docs"]
    async fn oversized_i2c_batch_never_reaches_the_bus() {
        // The point of #179. Before the fix this batch executed in full --
        // the pointer moved to THIGH -- and only then lost its response to
        // transport truncation, so the caller saw
        // `Comms(Postcard(DeserializeUnexpectedEnd))` with no hint that the
        // device had already changed. Measured that way on board
        // 49742081C885AC69; see AGENTS.md §13.17.
        let _bench = BENCH.lock().await;
        let pg = board().await;
        let a = addr();

        pg.i2c_write(a, &[TMP102_TLOW]).await.expect("seed pointer to TLOW");
        let before = pg.i2c_read(a, 2).await.expect("baseline read");

        let ops = vec![
            I2cBatchOp::Write { data: &[TMP102_THIGH] },
            I2cBatchOp::Read {
                len: MAX_RESPONSE_PAYLOAD as u16 + 1,
            },
        ];
        match pg.i2c_batch(a, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::BufferTooLong,
            })) => assert_eq!(failed_op, 0, "an aggregate overflow names no single operation"),
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }

        let after = pg.i2c_read(a, 2).await.expect("witness read");
        assert_eq!(
            before, after,
            "the refused batch moved the pointer register, so its leading write reached the bus"
        );

        pg.i2c_scan(false)
            .await
            .expect("device unresponsive after a refused oversized batch");
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn spi_batch_at_the_response_ceiling_still_returns_data() {
        // Positive control, in two parts. Needs no SPI target: with MISO
        // idle the bytes are meaningless, but their count is one of the
        // properties under test.
        //
        // The second part is what makes
        // [`oversized_spi_batch_never_moves_chip_select`] mean anything. An
        // *accepted* batch must leave the chip-select high, so a witness
        // that reads low after a refusal is a real observation rather than
        // a pin nothing ever touches.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        pg.gpio_put(0, GpioState::Low).await.expect("drive the CS pin low");

        let ops = vec![SpiBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16,
        }];
        let got = pg
            .spi_batch(0, &ops)
            .await
            .expect("a batch reading exactly the ceiling must still be delivered");
        assert_eq!(got.len(), MAX_RESPONSE_PAYLOAD, "the whole response must arrive");

        assert_eq!(
            pg.gpio_get(0).await.expect("witness CS read"),
            GpioState::High,
            "an accepted batch must have deasserted chip-select, or the \
             witness in oversized_spi_batch_never_moves_chip_select is blind"
        );
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn oversized_spi_batch_never_moves_chip_select() {
        // The SPI half of #179. `spi_batch_handler` configures the
        // chip-select as an output, drives it low, runs the operations, then
        // drives it high again -- so the pin's level after the call is a
        // witness for whether the transaction ran at all. Before the fix,
        // driving GPIO 0 low and then issuing this batch left it high.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        pg.gpio_put(0, GpioState::Low).await.expect("drive the CS pin low");
        assert_eq!(
            pg.gpio_get(0).await.expect("baseline CS read"),
            GpioState::Low,
            "the witness must start low, or the assertion below proves nothing"
        );

        let ops = vec![SpiBatchOp::Read {
            len: MAX_RESPONSE_PAYLOAD as u16 + 1,
        }];
        match pg.spi_batch(0, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op,
                kind: SpiError::BufferTooLong,
            })) => assert_eq!(failed_op, 0, "an aggregate overflow names no single operation"),
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }

        assert_eq!(
            pg.gpio_get(0).await.expect("witness CS read"),
            GpioState::Low,
            "the refused batch deasserted chip-select, so the transaction ran"
        );
    }

    // -------------------------------------------------------------------
    // Per-call ceilings (issue #158)
    // -------------------------------------------------------------------
    //
    // NOT YET EXECUTED ON HARDWARE. Every other test in this module was run
    // against a board and its results are recorded in AGENTS.md §13.17;
    // these three were written blind, because the bench was unavailable when
    // #158 was implemented. They are the harness for the outstanding A/B,
    // not evidence that it was done. Treat a failure here as equally likely
    // to be a bug in the test.
    //
    // The A/B they are meant to support: build a host binary with the guards
    // stubbed out, confirm 1015 returns `Comms(Postcard(...))` rather than
    // `BufferTooLong`, then rebuild with the guards and confirm it does not.

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn spi_read_at_the_response_ceiling_still_returns_data() {
        // Positive control. Needs no SPI target: with MISO idle the bytes
        // are meaningless, but their count is the property under test.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        let got = pg
            .spi_read(MAX_RESPONSE_PAYLOAD as u16)
            .await
            .expect("a read of exactly the ceiling must still be delivered");
        assert_eq!(got.len(), MAX_RESPONSE_PAYLOAD, "the whole response must arrive");
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn oversized_spi_read_is_refused_as_a_size_error() {
        // The regression #158 is about. Before the guard this returned
        // `Comms(Postcard(DeserializeUnexpectedEnd))` -- re-measured on
        // hardware 2026-09-09 during #179 and recorded in its §13.17 row --
        // which names the transport rather than the argument at fault and
        // gives the caller no clue what size would have worked.
        //
        // Unlike the batch endpoints, nothing here is corrupted by the old
        // behaviour: a read commits no writes. This is a diagnosability fix,
        // not a data-integrity one.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        match pg.spi_read(MAX_RESPONSE_PAYLOAD as u16 + 1).await {
            Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong)) => {}
            other => panic!("expected Endpoint(BufferTooLong), got {other:?}"),
        }

        // The board must still be alive: this was never a wedge, and the
        // #158 triage could not reproduce one at any size.
        pg.ping(0x5158).await.expect("the board must survive a refused read");
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn spi_write_above_the_response_ceiling_still_succeeds() {
        // The asymmetry, on real hardware. A single shared ceiling would
        // wrongly refuse this: `spi/write` returns no payload, so the
        // response budget does not apply to it, and #158 measured it
        // completing normally at 1015 bytes in 0.36 s.
        //
        // This is the test that fails if someone "simplifies" the two
        // predicates into one.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        pg.spi_write(&vec![0xA5; MAX_RESPONSE_PAYLOAD + 1])
            .await
            .expect("a write returns nothing, so the response ceiling must not apply");
    }

    // -------------------------------------------------------------------
    // Batch request-frame ceiling (issue #186)
    // -------------------------------------------------------------------
    //
    // These pin the boundary and boundary+1 for the *send* direction on
    // real hardware, and are the counterpart of the response-ceiling block
    // above.
    //
    // # What makes these different from every other test in this module
    //
    // The response-ceiling tests share their bound with the firmware, so
    // they cannot tell which side refused. These can, in principle, because
    // **the firmware has no counterpart guard and cannot have one**: an
    // over-ceiling frame is discarded by postcard-rpc's `receive()` before
    // any handler runs. So without the host guard there is no refusal at
    // all, only a timeout — which is precisely what the pre-fix arm of the
    // A/B recorded.
    //
    // # Bench setup
    //
    // As for the module: `GALLO_TEST_I2C_ADDR` should name a *responding*
    // target for the positive control. The refusal tests address 0x50,
    // deliberately unpopulated, so that if the guard were removed the batch
    // would be NACKed rather than writing five kilobytes into a real chip.

    /// An address with nothing on it, so a batch that escapes the guard
    /// NACKs instead of driving a payload into a real target.
    const UNPOPULATED_ADDR: u8 = 0x50;
    /// The payload that makes a single-`Write` I2C batch frame exactly
    /// [`MAX_REQUEST_FRAME`] — 5099 bytes as measured, but derived rather
    /// than hardcoded so the fixture follows the constant.
    fn i2c_hw_payload_filling_the_frame() -> usize {
        let probe = vec![0u8; MAX_REQUEST_FRAME];
        MAX_REQUEST_FRAME - (i2c_batch_request_frame_len(&[I2cBatchOp::Write { data: &probe }]) - MAX_REQUEST_FRAME)
    }

    /// How much a batch must overshoot before it is undeliverable
    /// regardless of connection state.
    ///
    /// The guard assumes the widest (cold) header, but [`board`] validates,
    /// so every test here runs on a *warm* client whose real header is six
    /// bytes narrower. A fixture that overshoots the cold bound by one is
    /// therefore still deliverable on the wire — confirmed in the #186
    /// control arm, where it returned `NoAcknowledge` rather than timing
    /// out. Overshooting by more than that difference is what makes a
    /// fixture undeliverable in both states.
    const WARM_HEADER_SLACK: usize = 6;

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn i2c_batch_at_the_request_frame_ceiling_still_reaches_the_bus() {
        // Positive control, and the one that would catch an over-strict
        // bound. Measured on board 49742081C885AC69 before the fix: a
        // 5099-byte single-write batch returned `NoAcknowledge` in 5 ms,
        // which is the frame arriving and the transaction being attempted.
        // It must still do exactly that.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        let payload = vec![0xA5u8; i2c_hw_payload_filling_the_frame()];
        let ops = vec![I2cBatchOp::Write { data: &payload }];
        assert_eq!(i2c_batch_request_frame_len(&ops), MAX_REQUEST_FRAME);

        match pg.i2c_batch(UNPOPULATED_ADDR, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                kind: I2cError::NoAcknowledge,
                ..
            })) => {}
            other => panic!(
                "a batch whose frame exactly fills the ceiling must still be \
                 delivered and addressed; got {other:?}"
            ),
        }
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn oversized_i2c_batch_frame_is_refused_as_a_size_error() {
        // The regression #186 is about. Before the guard this timed out
        // after the full call bound, because the firmware discarded the
        // frame and answered nothing at all -- measured in the #186 control
        // arm on board 49742081C885AC69, cold at 5100 bytes and warm here,
        // both 5001 ms.
        //
        // Overshoots by more than `WARM_HEADER_SLACK` on purpose, so the
        // frame is genuinely undeliverable rather than merely past the
        // guard's conservative cold bound. The band between the two is
        // covered by `i2c_batch_frame_guard_is_conservative_when_warm`.
        //
        // Nothing is corrupted by the old behaviour: the batch never
        // executes, so no write reaches the bus. Like #158 and unlike #179,
        // this is a diagnosability fix.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        let payload = vec![0xA5u8; i2c_hw_payload_filling_the_frame() + WARM_HEADER_SLACK + 1];
        let ops = vec![I2cBatchOp::Write { data: &payload }];
        assert!(i2c_batch_request_frame_len(&ops) > MAX_REQUEST_FRAME + WARM_HEADER_SLACK);

        let t = std::time::Instant::now();
        match pg.i2c_batch(UNPOPULATED_ADDR, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op,
                kind: I2cError::BufferTooLong,
            })) => assert_eq!(failed_op, 0, "an aggregate overflow names no single operation"),
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
        // A local refusal is immediate. The pre-fix failure took the whole
        // call bound, so this also distinguishes the two outcomes by
        // timing, not just by error value.
        assert!(
            t.elapsed() < pg.call_timeout(),
            "the refusal must be local, not a transport timeout"
        );

        pg.i2c_scan(false)
            .await
            .expect("device unresponsive after a refused oversized batch frame");
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn i2c_batch_frame_guard_is_conservative_when_warm() {
        // Pins the deliberate conservatism rather than hiding it. This
        // batch overshoots the guard's cold bound by one byte, so it is
        // refused -- but the client is warm, its real header is six bytes
        // narrower, and the #186 control arm confirmed that with the guard
        // stubbed out this exact call reaches the bus and returns
        // `NoAcknowledge`.
        //
        // That is the price of a bound that does not depend on connection
        // state: postcard-rpc's key width is private, so the library cannot
        // ask whether it has received a reply yet. Six bytes of headroom
        // is the trade. If this test starts failing because the call
        // succeeds, someone has made the bound depend on `kkind` -- which
        // would also mean the same batch is accepted or refused depending
        // on what the process did beforehand.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        let payload = vec![0xA5u8; i2c_hw_payload_filling_the_frame() + 1];
        let ops = vec![I2cBatchOp::Write { data: &payload }];
        assert_eq!(i2c_batch_request_frame_len(&ops), MAX_REQUEST_FRAME + 1);

        match pg.i2c_batch(UNPOPULATED_ADDR, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op: 0,
                kind: I2cError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn oversized_i2c_batch_frame_is_refused_in_aggregate() {
        // The multi-operation arm, which a per-operation bound would miss
        // entirely: eight writes, none of them individually past
        // MAX_TRANSFER_SIZE. Measured pre-fix at 8 x 635 bytes -> Timeout,
        // against 8 x 634 -> NoAcknowledge; and again in the #186 control
        // arm, where this exact fixture timed out at 5 s with the guard
        // stubbed out. It overshoots by well over `WARM_HEADER_SLACK`,
        // because each of the eight operations rounds up.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        let per = i2c_hw_payload_filling_the_frame() / 8;
        let payload = vec![0xA5u8; per];
        let ops: Vec<I2cBatchOp<'_>> = (0..8).map(|_| I2cBatchOp::Write { data: &payload }).collect();
        assert!(per <= MAX_TRANSFER_SIZE, "each operation must be individually legal");
        assert!(i2c_batch_request_frame_len(&ops) > MAX_REQUEST_FRAME + WARM_HEADER_SLACK);

        match pg.i2c_batch(UNPOPULATED_ADDR, &ops).await {
            Err(PicoDeGalloError::Endpoint(I2cBatchError {
                failed_op: 0,
                kind: I2cError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(I2cBatchError{{BufferTooLong}}), got {other:?}"),
        }
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn oversized_spi_batch_frame_never_moves_chip_select() {
        // The SPI half, with the same chip-select witness the #179 tests
        // use. Pre-fix this was the worst-presenting arm of the defect: a
        // batch carrying no `DelayNs` is bounded by the firmware's
        // 30-minute handler ceiling rather than the ordinary call timeout,
        // so the caller hung for over half an hour instead of the 5 s an
        // I2C batch takes. Confirmed in the #186 control arm on board
        // 49742081C885AC69: with the guard stubbed out this call had still
        // not returned after 90 s and had to be killed.
        //
        // `spi_batch` always warms the client -- it fetches `device/info`
        // for the chip-select bound -- so the fixture overshoots by more
        // than `WARM_HEADER_SLACK` to be undeliverable in fact and not just
        // past the guard's cold bound.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        pg.gpio_put(0, GpioState::Low).await.expect("drive the CS pin low");
        assert_eq!(
            pg.gpio_get(0).await.expect("baseline CS read"),
            GpioState::Low,
            "the witness must start low, or the assertion below proves nothing"
        );

        let probe = vec![0u8; MAX_REQUEST_FRAME];
        let fill = MAX_REQUEST_FRAME
            - (spi_batch_request_frame_len(&[SpiBatchOp::Write { data: &probe }]) - MAX_REQUEST_FRAME);
        let payload = vec![0xA5u8; fill + WARM_HEADER_SLACK + 1];
        let ops = vec![SpiBatchOp::Write { data: &payload }];
        assert!(spi_batch_request_frame_len(&ops) > MAX_REQUEST_FRAME + WARM_HEADER_SLACK);

        let t = std::time::Instant::now();
        match pg.spi_batch(0, &ops).await {
            Err(SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op: 0,
                kind: SpiError::BufferTooLong,
            })) => {}
            other => panic!("expected Endpoint(SpiBatchError{{BufferTooLong}}), got {other:?}"),
        }
        assert!(
            t.elapsed() < pg.call_timeout(),
            "the refusal must be local, not the 30-minute handler bound"
        );

        assert_eq!(
            pg.gpio_get(0).await.expect("witness CS read"),
            GpioState::Low,
            "the refused batch deasserted chip-select, so the transaction ran"
        );
    }

    #[tokio::test]
    #[ignore = "requires an attached board; see module docs"]
    async fn spi_batch_at_the_request_frame_ceiling_still_reaches_the_bus() {
        // Positive control for the SPI arm. Measured pre-fix at 5105 bytes
        // (warm, because `spi_batch` always fetches `device/info` first):
        // Ok in 54 ms. The guard uses the *cold* header, so it now refuses
        // six bytes earlier than the wire strictly requires; this test uses
        // the same conservative figure, and passing it means the bound is
        // reachable rather than unreachably strict.
        let _bench = BENCH.lock().await;
        let pg = board().await;

        pg.gpio_put(0, GpioState::Low).await.expect("drive the CS pin low");

        let probe = vec![0u8; MAX_REQUEST_FRAME];
        let fill = MAX_REQUEST_FRAME
            - (spi_batch_request_frame_len(&[SpiBatchOp::Write { data: &probe }]) - MAX_REQUEST_FRAME);
        let payload = vec![0xA5u8; fill];
        let ops = vec![SpiBatchOp::Write { data: &payload }];
        assert_eq!(spi_batch_request_frame_len(&ops), MAX_REQUEST_FRAME);

        pg.spi_batch(0, &ops)
            .await
            .expect("a batch whose frame exactly fills the ceiling must still be delivered");

        assert_eq!(
            pg.gpio_get(0).await.expect("witness CS read"),
            GpioState::High,
            "an accepted batch must have deasserted chip-select"
        );
    }
}
