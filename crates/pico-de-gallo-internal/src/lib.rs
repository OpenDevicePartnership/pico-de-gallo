//! Shared wire-protocol types for the Pico de Gallo USB bridge.
//!
//! This crate defines the [postcard-rpc](https://docs.rs/postcard-rpc) endpoints,
//! request/response types, and shared constants used by both the firmware
//! ([`pico-de-gallo-firmware`]) and the host-side library ([`pico-de-gallo-lib`]).
//!
//! # Wire Compatibility
//!
//! All types are serialized with [postcard](https://docs.rs/postcard). Postcard
//! encodes enum variants by **index** (0, 1, 2, …), not by discriminant value.
//! Reordering variants in any `enum` in this crate is a **breaking wire change**
//! that will silently corrupt communication between mismatched firmware and host
//! versions.
//!
//! # Feature Flags
//!
//! - **`use-std`** — Enables `Vec<u8>` response types for the host side. Without
//!   this feature (the default for firmware), responses use borrowed `&[u8]` slices.
//!
//! # Crate Organization
//!
//! - **Constants**: [`MICROSOFT_VID`], [`PICO_DE_GALLO_PID`], [`MAX_TRANSFER_SIZE`],
//!   [`MAX_RESPONSE_PAYLOAD`]
//! - **Endpoints**: Defined via the [`postcard_rpc::endpoints!`] macro — see
//!   [`ENDPOINT_LIST`] for the full table.
//! - **I2C types**: [`I2cReadRequest`], [`I2cWriteRequest`], [`I2cWriteReadRequest`],
//!   [`I2cScanRequest`], and their shared error type [`I2cError`].
//! - **SPI types**: [`SpiReadRequest`], [`SpiWriteRequest`], [`SpiTransferRequest`]
//!   and their shared error type [`SpiError`].
//! - **GPIO types**: [`GpioGetRequest`], [`GpioPutRequest`], [`GpioWaitRequest`],
//!   [`GpioState`], [`GpioDirection`], [`GpioPull`], [`GpioSetConfigurationRequest`],
//!   and their shared error type [`GpioError`].
//! - **UART types**: [`UartReadRequest`], [`UartWriteRequest`],
//!   [`UartSetConfigurationRequest`], [`UartConfigurationInfo`],
//!   [`UartDataBits`], [`UartParity`], [`UartStopBits`],
//!   and their shared error type [`UartError`].
//! - **PWM types**: [`PwmSetDutyCycleRequest`], [`PwmGetDutyCycleRequest`],
//!   [`PwmEnableRequest`], [`PwmDisableRequest`], [`PwmSetConfigurationRequest`],
//!   [`PwmGetConfigurationRequest`], [`PwmDutyCycleInfo`], [`PwmConfigurationInfo`],
//!   and their shared error type [`PwmError`].
//! - **ADC types**: [`AdcReadRequest`], [`AdcChannel`], [`AdcConfigurationInfo`],
//!   and their shared error type [`AdcError`].
//! - **1-Wire types**: [`OneWireReadRequest`], [`OneWireWriteRequest`],
//!   [`OneWireWritePullupRequest`], and their shared error type [`OneWireError`].
//! - **Batch types**: [`I2cBatchRequest`], [`I2cBatchError`], [`I2cBatchOp`],
//!   [`SpiBatchRequest`], [`SpiBatchError`], [`SpiBatchOp`], encoding helpers
//!   [`encode_i2c_batch_ops`], [`encode_spi_batch_ops`], and response-length
//!   helpers [`i2c_batch_response_len`], [`spi_batch_response_len`].
//! - **Configuration**: [`I2cSetConfigurationRequest`], [`SpiSetConfigurationRequest`],
//!   [`GpioSetConfigurationRequest`], [`UartSetConfigurationRequest`],
//!   [`PwmSetConfigurationRequest`],
//!   [`I2cFrequency`], [`SpiPhase`], [`SpiPolarity`],
//!   [`GpioDirection`], [`GpioPull`], [`SpiConfigurationInfo`],
//!   [`UartConfigurationInfo`], [`UartDataBits`], [`UartParity`],
//!   [`UartStopBits`].
//! - **Version**: [`VersionInfo`].
//! - **Device Info**: [`DeviceInfo`], [`Capabilities`].

#![cfg_attr(not(feature = "use-std"), no_std)]

use postcard_rpc::{TopicDirection, endpoints, topics};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

// Auto-generated schema version constants from Cargo.toml
include!(concat!(env!("OUT_DIR"), "/schema_version.rs"));

/// USB Vendor ID (Microsoft Corporation).
pub const MICROSOFT_VID: u16 = 0x045e;

/// USB Product ID assigned to Pico de Gallo.
pub const PICO_DE_GALLO_PID: u16 = 0x067d;

/// Maximum number of bytes the firmware can handle in a single I2C or SPI
/// transaction. Requests exceeding this limit will be rejected by the
/// firmware with an error.
///
/// This bounds a *request* argument and the firmware's scratch buffer. It
/// is **not** the bound on how much data can come back: see
/// [`MAX_RESPONSE_PAYLOAD`], which is far tighter.
pub const MAX_TRANSFER_SIZE: usize = 4096;

/// The size of the buffer every inbound USB transfer is read into on the
/// host, in bytes.
///
/// `postcard-rpc-0.12.1/src/host_client/raw_nusb.rs:20`:
///
/// ```text
/// // TODO: These should all be configurable, PRs welcome
/// /// The size in bytes of the largest possible IN transfer
/// pub(crate) const MAX_TRANSFER_SIZE: usize = 1024;
/// ```
///
/// Private and non-configurable upstream, hence the copy. A response frame
/// longer than this fills the buffer, the transfer completes, and the
/// remaining bytes land in the *next* transfer as an unparseable fragment.
const HOST_IN_TRANSFER_BUDGET: usize = 1024;

/// Encoded length of the postcard-rpc header on a response frame, in bytes.
///
/// One discriminant byte (`0bNNMM_VVVV`), then a two-byte key, then a
/// four-byte sequence number:
///
/// * **Key — 2 bytes.** The server shrinks every reply key to
///   `Dispatch::min_key_len()`, which `define_dispatch!` derives from
///   `postcard_rpc::server::min_key_needed` over [`ENDPOINT_LIST`],
///   [`TOPICS_IN_LIST`] and [`TOPICS_OUT_LIST`]. For this protocol that is
///   `VarKeyKind::Key2`, pinned by `reply_key_length_is_two_bytes`.
/// * **Sequence number — 4 bytes.** `HostClient::send_resp` builds every
///   request with `VarSeq::Seq4` and the server echoes the request's
///   `seq_no` verbatim. The `VarSeqKind` handed to `try_new_raw_nusb` never
///   takes effect: `VarSeq::resize` is defined in postcard-rpc 0.12.1 and
///   called nowhere in it. Passing `Seq2` therefore buys no header bytes,
///   which is why a hand-rolled `Seq2` probe measures a 5-byte header while
///   the real client gets 7.
const RESPONSE_HEADER_LEN: usize = 1 + 2 + 4;

/// The postcard variant index of a `Result`, in bytes.
///
/// Every response-bearing endpoint answers with `Result<_, E>`, whose `Ok`
/// arm costs one leading byte before the payload.
const RESPONSE_RESULT_TAG_LEN: usize = 1;

/// The postcard varint length prefix on the returned byte sequence, in
/// bytes.
///
/// Two bytes for any length in `128..=16383`, which
/// [`MAX_RESPONSE_PAYLOAD`] is — asserted by
/// `max_response_payload_needs_a_two_byte_length_prefix`.
const RESPONSE_LEN_PREFIX_LEN: usize = 2;

/// Largest byte payload a single response frame can actually deliver to the
/// host.
///
/// This is a *deliverable-response* ceiling, not a firmware buffer bound.
/// The firmware can produce up to [`MAX_TRANSFER_SIZE`] bytes and drive the
/// bus to obtain them, but a response frame larger than the host
/// transport's inbound buffer is truncated in transit and the caller sees a
/// `Postcard(DeserializeUnexpectedEnd)` that looks like a comms fault.
/// Everything the request already did to the bus has still happened.
///
/// # Derivation
///
/// Every byte is accounted for. Nothing here is fitted to the measurement.
///
/// | Bytes | Term |
/// |------:|------|
/// | `1024` | [`HOST_IN_TRANSFER_BUDGET`] |
/// | `-7` | [`RESPONSE_HEADER_LEN`] — 1 discriminant + 2 key + 4 sequence |
/// | `-1` | [`RESPONSE_RESULT_TAG_LEN`] — the `Result` variant index |
/// | `-2` | [`RESPONSE_LEN_PREFIX_LEN`] — the sequence length varint |
/// | **`= 1014`** | |
///
/// # Measurement
///
/// Confirmed on hardware on two separate boards
/// (`5256657D8A5D7F03` in issue #158, `49742081C885AC69` in issue #179) and
/// across `spi/read`, `spi/transfer`, `i2c/read` and `onewire/read`: `1014`
/// bytes return normally, `1015` fails with
/// `Comms(Postcard(DeserializeUnexpectedEnd))`, and the board stays alive.
/// Reading raw USB transfers off `49742081C885AC69` shows the frame for a
/// 1014-byte `spi/read` is exactly 1024 bytes, and 1015 arrives as
/// `1024 + 1`.
///
/// # When this moves
///
/// The value is a property of the *host* transport, which the firmware
/// cannot observe. It is defined here, rather than host-side only, because
/// the firmware is the only place that can decline to begin irreversible
/// bus side effects for a response it will not be able to deliver — see the
/// batch handlers, and issue #179. It moves if postcard-rpc's inbound
/// transfer size changes, or if this protocol grows enough endpoints to
/// need a four-byte key, in which case it drops to 1012 and
/// `reply_key_length_is_two_bytes` fails first.
pub const MAX_RESPONSE_PAYLOAD: usize = HOST_IN_TRANSFER_BUDGET
    - RESPONSE_HEADER_LEN
    - RESPONSE_RESULT_TAG_LEN
    - RESPONSE_LEN_PREFIX_LEN;

// Compile-time invariants on the derivation above. These are assertions,
// not tests, so drift is a build failure rather than something CI has to
// catch.
const _: () = {
    // `RESPONSE_LEN_PREFIX_LEN` assumes a two-byte postcard varint, which
    // is only right while the ceiling lands in `128..=16383`.
    assert!(MAX_RESPONSE_PAYLOAD >= 128);
    assert!(MAX_RESPONSE_PAYLOAD <= 16383);
    // A response bound, and strictly tighter than the request/buffer bound.
    // Conflating the two is the defect in #179.
    assert!(MAX_RESPONSE_PAYLOAD < MAX_TRANSFER_SIZE);
};

/// Size of each postcard-rpc packet buffer on the firmware, in bytes.
///
/// The firmware's `BufStorage` is
/// `PacketBuffers<{FIRMWARE_PACKET_BUFFER}, {FIRMWARE_PACKET_BUFFER}>`, so
/// this constant *is* the buffer rather than a copy of it and the two
/// cannot drift. Both directions are sized the same; only the receive side
/// enters [`MAX_REQUEST_FRAME`]'s derivation, because the transmit side is
/// bounded far earlier by [`MAX_RESPONSE_PAYLOAD`].
///
/// The `+ 1024` is protocol overhead room around a maximal
/// [`MAX_TRANSFER_SIZE`] argument. It is *not* a second payload allowance:
/// a batch can spend all of it on operation data, which is how issue #186
/// reached the frame ceiling.
pub const FIRMWARE_PACKET_BUFFER: usize = MAX_TRANSFER_SIZE + 1024;

/// The one byte of the firmware's receive buffer that a frame may not use.
///
/// A mechanism, not a safety margin. postcard-rpc's `EUsbWireRx::receive`
/// ends a frame when a **short** USB packet arrives, and loops
/// `while !window.is_empty()`. A frame whose length equals the buffer size
/// fills the window with full 64-byte packets, so the loop runs out of
/// window before it ever sees a short packet, falls into its "ran out of
/// space" branch, reads the transfer-terminating zero-length packet there,
/// and reports `WireRxErrorKind::ReceivedMessageTooLarge`. Frames that are
/// a multiple of 64 but *smaller* than the buffer are unaffected, because
/// window remains and the zero-length packet ends the frame normally.
///
/// Derived by reading `receive()`'s control flow, not by measurement:
/// telling "exact fill refused" apart from "window one byte short" needs a
/// firmware whose receive buffer is not a multiple of 64. Issue #180.
const REQUEST_FRAME_EXACT_FILL_PENALTY: usize = 1;

/// Widest postcard-rpc header a *request* frame can carry, in bytes.
///
/// One discriminant byte, then the key, then the sequence number:
///
/// * **Key — 8 bytes.** Unlike the response header, this is not a constant
///   property of the protocol but of the connection's age.
///   `HostClient` starts at `VarKeyKind::Key8` and adopts the server's
///   narrower `Key2` only after it has **received a reply**
///   (`host_client/mod.rs:202`, `:424`, `:431` are the only sites that
///   touch `kkind`). So a process's first request carries 13 bytes of
///   header and every request after the first reply carries 7.
/// * **Sequence number — 4 bytes.** `VarSeq::Seq4` always, for the reason
///   given on [`RESPONSE_HEADER_LEN`].
///
/// The widest — "cold" — value is the one used to bound a request, because
/// `kkind` is private to postcard-rpc and a caller cannot ask which state
/// its client is in. Bounding against the narrow value would accept a frame
/// that is dropped whenever it happens to be the first one a process sends.
/// The cost is six bytes of headroom on a warm connection, which is the
/// right trade for an answer that does not depend on connection state.
/// Pinned by `request_header_encodes_to_thirteen_bytes`.
const REQUEST_HEADER_LEN_MAX: usize = 1 + 8 + 4;

/// Largest request frame the firmware will accept, in bytes, header
/// included.
///
/// # Derivation
///
/// | Bytes | Term |
/// |------:|------|
/// | `5120` | [`FIRMWARE_PACKET_BUFFER`] |
/// | `-1` | [`REQUEST_FRAME_EXACT_FILL_PENALTY`] |
/// | **`= 5119`** | |
///
/// # Why it needs enforcing
///
/// A frame that does not fit is discarded **silently**. postcard-rpc's
/// server maps `WireRxErrorKind::ReceivedMessageTooLarge` to `continue`, so
/// nothing is sent back and the caller sees only its own timeout. Nothing
/// names the argument at fault and no size is suggested that would work.
/// The firmware cannot help: the request never arrives, so the bound has to
/// be applied host-side, before transmission.
///
/// Since issue #158 every host surface caps a single write argument at
/// [`MAX_TRANSFER_SIZE`], which puts the worst-case frame a little over
/// 4110 bytes — a kilobyte clear of this. Batches were the exception:
/// nothing bounded their aggregate outgoing bytes, so `i2c/batch` and
/// `spi/batch` were the one remaining door from a supported host surface
/// into this ceiling (issue #186).
///
/// # Measurement
///
/// Confirmed on board `49742081C885AC69` (hw-rev2, firmware
/// `firmware-v0.11.0-88-gfcb38de3a1ff`) under Linux/nusb, and previously on
/// the same board under firmware `-79-gc3c6a3e07bec` across sixteen
/// boundaries spanning four key widths (issue #180). Driven through
/// `pico-de-gallo-lib`'s `i2c_batch`, a frame of `5119` bytes is executed
/// and answers; `5120` is dropped and the call times out. The edge sits at
/// the same frame length cold and warm, six payload bytes apart, which is
/// what identifies it as a *frame* ceiling rather than a payload one.
///
/// # When this moves
///
/// It tracks the firmware's receive buffer, which is why that buffer is
/// [`FIRMWARE_PACKET_BUFFER`] rather than a literal. Unlike
/// [`MAX_RESPONSE_PAYLOAD`] this is a property of the *device*, so a host
/// built against a firmware with a different buffer would be wrong — the
/// two travel together in the same release (§6.5).
pub const MAX_REQUEST_FRAME: usize = FIRMWARE_PACKET_BUFFER - REQUEST_FRAME_EXACT_FILL_PENALTY;

// Compile-time invariants on the derivation above, matching the treatment
// `MAX_RESPONSE_PAYLOAD` gets.
const _: () = {
    // The header has to leave room for a payload at all. Anything near this
    // means a term is wrong by orders of magnitude.
    assert!(MAX_REQUEST_FRAME > REQUEST_HEADER_LEN_MAX);
    // The request budget is the looser of the two directions. Every
    // send-direction bound in `pico-de-gallo-lib` relies on this ordering,
    // and #179 is what conflating them looks like.
    assert!(MAX_RESPONSE_PAYLOAD < MAX_REQUEST_FRAME);
    // A single maximal write argument must stay clear of the frame ceiling,
    // or #158's per-argument bounds would themselves be reachable here and
    // every plain endpoint would need this check too.
    assert!(MAX_TRANSFER_SIZE + REQUEST_HEADER_LEN_MAX < MAX_REQUEST_FRAME);
};

/// Ceiling the firmware applies to any caller-supplied handler timeout.
///
/// A `timeout_ms` of `0` does **not** mean "wait forever": both `0` and
/// oversized values clamp to this, after which the handler returns its normal
/// `Timeout` error. Hosts use this to bound how long a call carrying a
/// caller-supplied duration can legitimately take.
///
/// Mirrored by `MAX_HANDLER_TIMEOUT` in the firmware's `progress` module,
/// which derives its `Duration` from this constant so the two cannot drift.
pub const MAX_HANDLER_TIMEOUT_MS: u32 = 30 * 60 * 1000;

/// Supervisor budget the firmware grants a handler that declares no budget of
/// its own.
///
/// Sized to cover `i2c/scan`'s worst case (128 addresses × 50 ms = 6.4 s) with
/// margin. Hosts use this to bound calls that are slow but carry no
/// caller-supplied duration — `i2c/scan` above all, which legitimately exceeds
/// a typical per-call default.
///
/// Mirrored by `DEFAULT_DISPATCH_BUDGET` in the firmware's `progress` module,
/// which derives its `Duration` from this constant so the two cannot drift.
pub const UNDECLARED_DISPATCH_BUDGET_MS: u32 = 10_000;

// ---

/// Response type for I2C write operations.
pub type I2cWriteResponse = Result<(), I2cError>;

/// Response type for I2C read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type I2cReadResponse<'a> = Result<Vec<u8>, I2cError>;
/// Response type for I2C read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type I2cReadResponse<'a> = Result<&'a [u8], I2cError>;

/// Response type for I2C write-read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type I2cWriteReadResponse<'a> = Result<Vec<u8>, I2cError>;
/// Response type for I2C write-read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type I2cWriteReadResponse<'a> = Result<&'a [u8], I2cError>;

/// Response type for SPI write operations.
pub type SpiWriteResponse = Result<(), SpiError>;

/// Response type for SPI read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type SpiReadResponse<'a> = Result<Vec<u8>, SpiError>;
/// Response type for SPI read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type SpiReadResponse<'a> = Result<&'a [u8], SpiError>;

/// Response type for SPI flush operations.
pub type SpiFlushResponse = Result<(), SpiError>;

/// Response type for SPI transfer operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type SpiTransferResponse<'a> = Result<Vec<u8>, SpiError>;
/// Response type for SPI transfer operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type SpiTransferResponse<'a> = Result<&'a [u8], SpiError>;

/// Response type for GPIO get operations.
pub type GpioGetResponse = Result<GpioState, GpioError>;
/// Response type for GPIO put operations.
pub type GpioPutResponse = Result<(), GpioError>;
/// Response type for GPIO wait operations.
pub type GpioWaitResponse = Result<(), GpioError>;
/// Response type for GPIO set-configuration operations.
pub type GpioSetConfigurationResponse = Result<(), GpioError>;
/// Response type for GPIO subscribe operations.
pub type GpioSubscribeResponse = Result<(), GpioError>;
/// Response type for GPIO unsubscribe operations.
pub type GpioUnsubscribeResponse = Result<(), GpioError>;
/// Response type for `system/reset-subscriptions`.
///
/// Returns the number of GPIO subscriptions that were torn down (0 if
/// none were active). Always succeeds — the endpoint is idempotent and
/// is meant to be called by hosts on connect to clean up any
/// subscriptions that survived a previous host crash, disconnect, or
/// `nusb::Interface` drop.
pub type SystemResetSubscriptionsResponse = u8;
/// Response type for I2C bus configuration operations.
pub type I2cSetConfigurationResponse = Result<(), I2cConfigError>;
/// Response type for I2C bus scan operations.
/// On the host (`use-std`), returns `Vec<u8>` of responding addresses;
/// on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type I2cScanResponse<'a> = Result<Vec<u8>, I2cError>;
/// Response type for I2C bus scan operations.
/// On the host (`use-std`), returns `Vec<u8>` of responding addresses;
/// on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type I2cScanResponse<'a> = Result<&'a [u8], I2cError>;
/// Response type for SPI bus configuration operations.
pub type SpiSetConfigurationResponse = Result<(), SpiConfigError>;

/// Response type for I2C get-configuration queries.
///
/// Returns the currently active I2C bus frequency.
pub type I2cGetConfigurationResponse = I2cFrequency;

/// Response type for SPI get-configuration queries.
///
/// Returns the currently active SPI bus parameters.
pub type SpiGetConfigurationResponse = SpiConfigurationInfo;

/// Response type for UART write operations.
pub type UartWriteResponse = Result<(), UartError>;

/// Response type for UART read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type UartReadResponse<'a> = Result<Vec<u8>, UartError>;
/// Response type for UART read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type UartReadResponse<'a> = Result<&'a [u8], UartError>;

/// Response type for UART flush operations.
pub type UartFlushResponse = Result<(), UartError>;

/// Response type for UART bus configuration operations.
pub type UartSetConfigurationResponse = Result<(), UartConfigError>;

/// Response type for UART get-configuration queries.
///
/// Returns the currently active UART parameters.
pub type UartGetConfigurationResponse = Result<UartConfigurationInfo, UartError>;

/// Response type for PWM set-duty-cycle operations.
pub type PwmSetDutyCycleResponse = Result<(), PwmError>;
/// Response type for PWM get-duty-cycle queries.
pub type PwmGetDutyCycleResponse = Result<PwmDutyCycleInfo, PwmError>;
/// Response type for PWM enable operations.
pub type PwmEnableResponse = Result<(), PwmError>;
/// Response type for PWM disable operations.
pub type PwmDisableResponse = Result<(), PwmError>;
/// Response type for PWM set-configuration operations.
pub type PwmSetConfigurationResponse = Result<(), PwmConfigError>;
/// Response type for PWM get-configuration queries.
pub type PwmGetConfigurationResponse = Result<PwmConfigurationInfo, PwmError>;

/// Response type for ADC read operations.
pub type AdcReadResponse = Result<u16, AdcError>;
/// Response type for ADC get-configuration queries.
pub type AdcGetConfigurationResponse = Result<AdcConfigurationInfo, AdcError>;

/// Response type for 1-Wire reset operations.
/// Returns `true` if at least one device is present on the bus.
pub type OneWireResetResponse = Result<bool, OneWireError>;

/// Response type for 1-Wire read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type OneWireReadResponse<'a> = Result<Vec<u8>, OneWireError>;
/// Response type for 1-Wire read operations.
/// On the host (`use-std`), returns `Vec<u8>`; on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type OneWireReadResponse<'a> = Result<&'a [u8], OneWireError>;

/// Response type for 1-Wire write operations.
pub type OneWireWriteResponse = Result<(), OneWireError>;

/// Response type for 1-Wire write-with-pullup operations.
pub type OneWireWritePullupResponse = Result<(), OneWireError>;

/// Response type for 1-Wire ROM search operations.
/// Returns `Some(rom_id)` for the next device found, or `None` if the
/// search is complete.
pub type OneWireSearchResponse = Result<Option<u64>, OneWireError>;

/// Response type for I2C batch transaction operations.
/// On the host (`use-std`), returns `Vec<u8>` of concatenated read data;
/// on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type I2cBatchResponse<'a> = Result<Vec<u8>, I2cBatchError>;
/// Response type for I2C batch transaction operations.
/// On the host (`use-std`), returns `Vec<u8>` of concatenated read data;
/// on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type I2cBatchResponse<'a> = Result<&'a [u8], I2cBatchError>;

/// Response type for SPI batch transaction operations.
/// On the host (`use-std`), returns `Vec<u8>` of concatenated read/transfer data;
/// on firmware, returns `&[u8]`.
#[cfg(feature = "use-std")]
pub type SpiBatchResponse<'a> = Result<Vec<u8>, SpiBatchError>;
/// Response type for SPI batch transaction operations.
/// On the host (`use-std`), returns `Vec<u8>` of concatenated read/transfer data;
/// on firmware, returns `&[u8]`.
#[cfg(not(feature = "use-std"))]
pub type SpiBatchResponse<'a> = Result<&'a [u8], SpiBatchError>;

endpoints! {
    list = ENDPOINT_LIST;
    | EndpointTy          | RequestTy                  | ResponseTy                  | Path                |
    | ----------          | ---------                  | ----------                  | ----                |
    | PingEndpoint        | u32                        | u32                         | "ping"              |
    | I2cRead             | I2cReadRequest             | I2cReadResponse<'a>         | "i2c/read"          |
    | I2cWrite            | I2cWriteRequest<'a>        | I2cWriteResponse            | "i2c/write"         |
    | I2cWriteRead        | I2cWriteReadRequest<'a>    | I2cWriteReadResponse<'b>    | "i2c/write-read"    |
    | SpiRead             | SpiReadRequest             | SpiReadResponse<'a>         | "spi/read"          |
    | SpiWrite            | SpiWriteRequest<'a>        | SpiWriteResponse            | "spi/write"         |
    | SpiFlush            | ()                         | SpiFlushResponse            | "spi/flush"         |
    | SpiTransfer         | SpiTransferRequest<'a>     | SpiTransferResponse<'b>     | "spi/transfer"      |
    | GpioGet             | GpioGetRequest             | GpioGetResponse             | "gpio/get"          |
    | GpioPut             | GpioPutRequest             | GpioPutResponse             | "gpio/put"          |
    | GpioWaitForHigh     | GpioWaitRequest            | GpioWaitResponse            | "gpio/wait-high"    |
    | GpioWaitForLow      | GpioWaitRequest            | GpioWaitResponse            | "gpio/wait-low"     |
    | GpioWaitForRising   | GpioWaitRequest            | GpioWaitResponse            | "gpio/wait-rising"  |
    | GpioWaitForFalling  | GpioWaitRequest            | GpioWaitResponse            | "gpio/wait-falling" |
    | GpioWaitForAny      | GpioWaitRequest            | GpioWaitResponse            | "gpio/wait-any"     |
    | I2cSetConfiguration | I2cSetConfigurationRequest | I2cSetConfigurationResponse | "i2c/set-config"    |
    | I2cScan             | I2cScanRequest             | I2cScanResponse<'a>         | "i2c/scan"          |
    | SpiSetConfiguration  | SpiSetConfigurationRequest  | SpiSetConfigurationResponse  | "spi/set-config"    |
    | GpioSetConfiguration | GpioSetConfigurationRequest | GpioSetConfigurationResponse | "gpio/set-config"   |
    | I2cGetConfiguration  | ()                          | I2cGetConfigurationResponse  | "i2c/get-config"    |
    | SpiGetConfiguration  | ()                          | SpiGetConfigurationResponse  | "spi/get-config"    |
    | UartRead             | UartReadRequest             | UartReadResponse<'a>         | "uart/read"         |
    | UartWrite            | UartWriteRequest<'a>        | UartWriteResponse            | "uart/write"        |
    | UartFlush            | ()                          | UartFlushResponse            | "uart/flush"        |
    | UartSetConfiguration | UartSetConfigurationRequest | UartSetConfigurationResponse | "uart/set-config"   |
    | UartGetConfiguration  | ()                            | UartGetConfigurationResponse  | "uart/get-config"    |
    | PwmSetDutyCycle       | PwmSetDutyCycleRequest        | PwmSetDutyCycleResponse       | "pwm/set-duty-cycle" |
    | PwmGetDutyCycle       | PwmGetDutyCycleRequest        | PwmGetDutyCycleResponse       | "pwm/get-duty-cycle" |
    | PwmEnable             | PwmEnableRequest              | PwmEnableResponse             | "pwm/enable"         |
    | PwmDisable            | PwmDisableRequest             | PwmDisableResponse            | "pwm/disable"        |
    | PwmSetConfiguration   | PwmSetConfigurationRequest    | PwmSetConfigurationResponse   | "pwm/set-config"     |
    | PwmGetConfiguration   | PwmGetConfigurationRequest    | PwmGetConfigurationResponse   | "pwm/get-config"     |
    | AdcRead               | AdcReadRequest                | AdcReadResponse               | "adc/read"           |
    | AdcGetConfiguration   | ()                            | AdcGetConfigurationResponse   | "adc/get-config"     |
    | GpioSubscribe         | GpioSubscribeRequest          | GpioSubscribeResponse         | "gpio/subscribe"     |
    | GpioUnsubscribe       | GpioUnsubscribeRequest        | GpioUnsubscribeResponse       | "gpio/unsubscribe"   |
    | I2cBatch              | I2cBatchRequest<'a>           | I2cBatchResponse<'b>          | "i2c/batch"          |
    | SpiBatch              | SpiBatchRequest<'a>           | SpiBatchResponse<'b>          | "spi/batch"          |
    | OneWireReset          | ()                            | OneWireResetResponse          | "onewire/reset"      |
    | OneWireRead           | OneWireReadRequest            | OneWireReadResponse<'a>       | "onewire/read"       |
    | OneWireWrite          | OneWireWriteRequest<'a>       | OneWireWriteResponse          | "onewire/write"      |
    | OneWireWritePullup    | OneWireWritePullupRequest<'a> | OneWireWritePullupResponse    | "onewire/write-pullup" |
    | OneWireSearch         | ()                            | OneWireSearchResponse         | "onewire/search"     |
    | OneWireSearchNext     | ()                            | OneWireSearchResponse         | "onewire/search-next" |
    | Version               | ()                            | VersionInfo                   | "version"            |
    | GetDeviceInfo         | ()                            | DeviceInfo                    | "device/info"        |
    | SystemResetSubscriptions | ()                         | SystemResetSubscriptionsResponse | "system/reset-subscriptions" |
}

topics! {
    list = TOPICS_IN_LIST;
    direction = TopicDirection::ToServer;
    | TopicTy | MessageTy | Path |
    | ------- | --------- | ---- |
}

topics! {
    list = TOPICS_OUT_LIST;
    direction = TopicDirection::ToClient;
    | TopicTy         | MessageTy  | Path              | Cfg |
    | -------         | --------- | ----               | --- |
    | GpioEventTopic    | GpioEvent   | "gpio/event"    |     |
}

// --- I2C

/// Request to write bytes to an I2C device, then read back.
///
/// The firmware performs a write followed by a repeated-start read in a
/// single I2C transaction, which is the standard pattern for reading
/// registers from most I2C devices.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cWriteReadRequest<'a> {
    /// 7-bit I2C slave address.
    pub address: u8,
    /// Bytes to write (typically a register address).
    pub contents: &'a [u8],
    /// Number of bytes to read back (max [`MAX_TRANSFER_SIZE`]).
    pub count: u16,
}

/// Error from I2C operations, propagated from firmware.
///
/// Maps directly to [`embedded_hal::i2c::ErrorKind`] variants on the host side.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index** (0, 1, 2, …). Do **not** reorder,
/// rename, or insert variants in the middle — only append new variants at
/// the end. Removing or reordering is a breaking wire change.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cError {
    /// Bus error (unexpected condition on the I2C bus).
    Bus,
    /// No acknowledge received from the target device.
    NoAcknowledge,
    /// Arbitration lost to another controller on the bus.
    ArbitrationLoss,
    /// Data overrun — firmware could not keep up with the bus clock.
    Overrun,
    /// A length bound was exceeded.
    ///
    /// One of three bounds was exceeded: a request argument past the
    /// firmware buffer limit ([`MAX_TRANSFER_SIZE`]); a batch whose `Read`
    /// operations would return more than [`MAX_RESPONSE_PAYLOAD`] bytes in
    /// total, more than one response frame can carry (issue #179); or a
    /// batch whose whole request frame would exceed [`MAX_REQUEST_FRAME`]
    /// (issue #186). Nothing on the wire tells them apart, so consult all
    /// three. The last is refused host-side only — the device never
    /// receives such a frame and so can never report it.
    BufferTooLong,
    /// I2C address is outside the valid 7-bit range (0x00–0x7F).
    AddressOutOfRange,
    /// An unspecified error occurred in the firmware.
    Other,
    /// A write was requested with an empty payload.
    ///
    /// The RP2040/RP2350 `DW_apb_i2c` block cannot emit an address-only
    /// transaction: the address phase is driven solely by pushing at least
    /// one byte into `IC_DATA_CMD`, so `START + ADDR + STOP` is physically
    /// unreachable (rp-rs/rp-hal#678, embassy-rs/embassy#4474). Forwarding
    /// such a write to `embassy_rp::i2c::write_async` starts no transaction
    /// yet still waits for a `STOP_DET`/`TX_ABRT` interrupt that can never
    /// arrive, which wedges the firmware dispatcher device-wide. The
    /// firmware therefore refuses the request up front. Probe an address
    /// with a 1-byte read (`i2c/read`) or with `i2c/scan` instead.
    ZeroLengthWrite,
}

impl core::fmt::Display for I2cError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bus => write!(f, "I2C bus error"),
            Self::NoAcknowledge => write!(f, "no acknowledge from target"),
            Self::ArbitrationLoss => write!(f, "I2C arbitration loss"),
            Self::Overrun => write!(f, "I2C data overrun"),
            Self::BufferTooLong => write!(f, "buffer exceeds firmware limit"),
            Self::AddressOutOfRange => write!(f, "I2C address out of range"),
            Self::Other => write!(f, "I2C error"),
            Self::ZeroLengthWrite => {
                write!(f, "zero-length I2C write is not supported by this hardware")
            }
        }
    }
}

/// Request to read bytes from an I2C device.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cReadRequest {
    /// 7-bit I2C slave address.
    pub address: u8,
    /// Number of bytes to read (max [`MAX_TRANSFER_SIZE`]).
    pub count: u16,
}

/// Request to write bytes to an I2C device.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cWriteRequest<'a> {
    /// 7-bit I2C slave address.
    pub address: u8,
    /// Bytes to write.
    pub contents: &'a [u8],
}

/// Request to scan the I2C bus for responding devices.
///
/// The firmware probes addresses by attempting a 1-byte read at each
/// 7-bit address. Addresses that ACK are included in the response.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cScanRequest {
    /// When `true`, also probe reserved addresses (0x00–0x07 and 0x78–0x7F).
    /// When `false`, only probe the standard range 0x08–0x77.
    pub include_reserved: bool,
}

// --- SPI

/// Error from SPI operations, propagated from firmware.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index**. Do **not** reorder or insert
/// variants in the middle — only append at the end.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    /// A length bound was exceeded.
    ///
    /// One of three bounds was exceeded: a request argument past the
    /// firmware buffer limit ([`MAX_TRANSFER_SIZE`]); a batch whose `Read`
    /// and `Transfer` operations would return more than
    /// [`MAX_RESPONSE_PAYLOAD`] bytes in total, more than one response
    /// frame can carry (issue #179); or a batch whose whole request frame
    /// would exceed [`MAX_REQUEST_FRAME`] (issue #186). Nothing on the wire
    /// tells them apart, so consult all three. The last is refused
    /// host-side only — the device never receives such a frame and so can
    /// never report it.
    BufferTooLong,
    /// An unspecified error occurred in the firmware.
    Other,
    /// The requested chip-select pin is outside the bound reported by
    /// [`DeviceInfo::num_gpios`].
    InvalidCsPin,
    /// The chip-select pin is explicitly configured as an input; it is left
    /// unchanged.
    CsPinUnavailable,
    /// The chip-select pin is being monitored for GPIO events; it is left
    /// unchanged.
    CsPinMonitored,
}

impl core::fmt::Display for SpiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BufferTooLong => write!(f, "buffer exceeds firmware limit"),
            Self::Other => write!(f, "SPI error"),
            Self::InvalidCsPin => write!(f, "invalid SPI chip-select pin"),
            Self::CsPinUnavailable => {
                write!(f, "SPI chip-select pin is configured as input")
            }
            Self::CsPinMonitored => {
                write!(f, "SPI chip-select pin is being monitored for events")
            }
        }
    }
}

/// Request to read bytes from the SPI bus.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiReadRequest {
    /// Number of bytes to read (max [`MAX_TRANSFER_SIZE`]).
    pub count: u16,
}

/// Request to write bytes to the SPI bus.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiWriteRequest<'a> {
    /// Bytes to write.
    pub contents: &'a [u8],
}

/// Request for a full-duplex SPI transfer.
///
/// The firmware simultaneously transmits `contents` and receives the same
/// number of bytes. This is a true full-duplex operation using DMA.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiTransferRequest<'a> {
    /// Bytes to transmit. The response will contain the same number of received bytes.
    pub contents: &'a [u8],
}

/// Error returned when an SPI transfer operation fails.
///
/// This is a convenience alias — SPI transfers share the same error type
/// as other SPI operations.
pub type SpiTransferError = SpiError;

// --- GPIO

/// Compile-time default number of user-controllable GPIO pins.
///
/// Pin indices 0–3 map to physical GPIO8–GPIO11 on the Pico 2 header.
/// Requests naming an index at or above this bound are rejected with
/// [`GpioError::InvalidPin`].
///
/// The same indices double as SPI chip-select selectors: see
/// [`SpiBatchRequest::cs_pin`]. The header pin silkscreened `SPI_CS`
/// (GPIO5) is a PCB net label rather than a usable chip select: it is
/// *not* one of these indices, and no firmware claims or drives it.
/// See [issue #99](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/99).
///
/// Host code must use [`DeviceInfo::num_gpios`] as the runtime-authoritative
/// count. This constant is only the compile-time default.
pub const NUM_GPIOS: usize = 4;

const _: () = assert!(NUM_GPIOS <= u8::MAX as usize);

/// Request to read the current level of a GPIO pin.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioGetRequest {
    /// GPIO pin index (0–3).
    pub pin: u8,
}

/// Error from GPIO operations, propagated from firmware.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index**. Do **not** reorder or insert
/// variants in the middle — only append at the end.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioError {
    /// The requested pin number is invalid (outside 0–3 range).
    InvalidPin,
    /// An unspecified error occurred in the firmware.
    Other,
    /// The pin is configured in a direction that does not support this operation.
    WrongDirection,
    /// The pin is currently being monitored for events and cannot be used
    /// for regular GPIO operations. Unsubscribe first.
    PinMonitored,
    /// The pin is not currently monitored — cannot unsubscribe.
    PinNotMonitored,
    /// Returned by `gpio_wait_*` endpoints when the host-supplied
    /// `timeout_ms` elapses before the requested edge or level is detected.
    /// Introduced in schema 0.7. A `timeout_ms` of `0` selects the firmware's
    /// 30-minute ceiling; oversized values are clamped to the same ceiling.
    Timeout,
}

impl core::fmt::Display for GpioError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidPin => write!(f, "invalid GPIO pin number"),
            Self::Other => write!(f, "GPIO error"),
            Self::WrongDirection => write!(f, "GPIO pin configured in wrong direction"),
            Self::PinMonitored => write!(f, "GPIO pin is being monitored for events"),
            Self::PinNotMonitored => write!(f, "GPIO pin is not monitored"),
            Self::Timeout => write!(f, "GPIO wait timed out"),
        }
    }
}

/// Request to set a GPIO pin to a specific level.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioPutRequest {
    /// GPIO pin index (0–3).
    pub pin: u8,
    /// Desired output level.
    pub state: GpioState,
}

/// Logic level of a GPIO pin.
//
// WARNING: Do not reorder enum variants — postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpioState {
    /// Logic low (0V).
    Low,
    /// Logic high (3.3V on RP2350).
    High,
}

impl From<bool> for GpioState {
    fn from(value: bool) -> Self {
        if value {
            GpioState::High
        } else {
            GpioState::Low
        }
    }
}

impl From<GpioState> for bool {
    fn from(state: GpioState) -> Self {
        matches!(state, GpioState::High)
    }
}

/// Request to wait for a GPIO pin to reach a specific state or edge.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioWaitRequest {
    /// GPIO pin index (0–3).
    pub pin: u8,
    /// Per-request timeout in milliseconds. A value of `0` selects the
    /// firmware's 30-minute ceiling; oversized values are clamped to the same
    /// ceiling. Expiry returns [`GpioError::Timeout`].
    pub timeout_ms: u32,
}

/// GPIO pin direction.
//
// WARNING: Do not reorder enum variants — postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GpioDirection {
    /// Configure the pin as a digital input.
    Input = 0,
    /// Configure the pin as a digital output.
    Output = 1,
}

/// GPIO internal pull resistor configuration.
//
// WARNING: Do not reorder enum variants — postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GpioPull {
    /// No internal pull resistor.
    None = 0,
    /// Internal pull-up resistor enabled.
    Up = 1,
    /// Internal pull-down resistor enabled.
    Down = 2,
}

/// Request to configure a GPIO pin's direction and pull resistor.
///
/// After configuration, the pin retains its explicit mode until the
/// firmware is reset. See [`GpioDirection`] and [`GpioPull`] for
/// available options.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioSetConfigurationRequest {
    /// GPIO pin index (0–3).
    pub pin: u8,
    /// Desired pin direction.
    pub direction: GpioDirection,
    /// Internal pull resistor setting.
    pub pull: GpioPull,
}

// --- GPIO event monitoring

/// Edge type for GPIO event monitoring.
///
/// Selects which transitions trigger a [`GpioEvent`] notification.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index**. Do **not** reorder or insert
/// variants in the middle — only append at the end.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GpioEdge {
    /// Trigger on low-to-high transitions.
    Rising = 0,
    /// Trigger on high-to-low transitions.
    Falling = 1,
    /// Trigger on any transition (rising or falling).
    Any = 2,
}

impl core::fmt::Display for GpioEdge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Rising => write!(f, "rising"),
            Self::Falling => write!(f, "falling"),
            Self::Any => write!(f, "any"),
        }
    }
}

/// A GPIO event notification published by the firmware.
///
/// Sent as a [`GpioEventTopic`] message whenever a monitored pin detects
/// an edge matching its subscription. The host receives these via
/// [`postcard_rpc::host_client::HostClient::subscribe`].
///
/// **Note:** Event delivery is best-effort. Edges faster than the
/// firmware's monitor loop may be coalesced or missed.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct GpioEvent {
    /// GPIO pin index (0–3) that triggered the event.
    pub pin: u8,
    /// The edge type that was detected.
    pub edge: GpioEdge,
    /// Pin level sampled immediately after the event (may differ from
    /// the triggering edge if the signal bounced).
    pub state: GpioState,
    /// Monotonic timestamp in microseconds since firmware boot.
    pub timestamp_us: u64,
}

/// Request to subscribe a GPIO pin to edge-event monitoring.
///
/// Once subscribed, the pin is exclusively owned by the monitor task.
/// Regular GPIO operations ([`GpioGet`], [`GpioPut`], wait, set-config)
/// on this pin will return [`GpioError::PinMonitored`] until
/// [`GpioUnsubscribe`] is called.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioSubscribeRequest {
    /// GPIO pin index (0–3).
    pub pin: u8,
    /// Which edges to monitor.
    pub edge: GpioEdge,
}

/// Request to unsubscribe a GPIO pin from edge-event monitoring.
///
/// The pin is returned to normal GPIO mode and can be used for regular
/// operations again.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioUnsubscribeRequest {
    /// GPIO pin index (0–3).
    pub pin: u8,
}

// --- Set config

/// Request to reconfigure I2C bus parameters.
///
/// Takes effect immediately. The firmware applies the new frequency before
/// processing the next I2C operation.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cSetConfigurationRequest {
    /// I2C bus clock frequency.
    pub frequency: I2cFrequency,
}

// WARNING: do not reorder variants — postcard encodes by index, not discriminant.
/// I2C bus clock frequency.
///
/// The RP2350 supports Standard (100 kHz), Fast (400 kHz), and Fast+ (1 MHz)
/// modes. Ultra-Fast mode is defined by the specification but not supported by
/// the RP2350 hardware.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum I2cFrequency {
    /// Standard mode — 100 kHz.
    Standard = 0,
    /// Fast mode — 400 kHz.
    Fast = 1,
    /// Fast+ mode — 1 MHz.
    FastPlus = 2,
}

/// Error returned when I2C configuration fails.
///
/// This is a convenience alias — I2C configuration shares the same error
/// type as other I2C operations.
pub type I2cConfigError = I2cError;

/// Request to reconfigure SPI bus parameters.
///
/// Takes effect immediately. The firmware applies the new settings before
/// processing the next SPI operation.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiSetConfigurationRequest {
    /// SPI bus clock frequency in Hz.
    pub spi_frequency: u32,
    /// SPI clock phase.
    pub spi_phase: SpiPhase,
    /// SPI clock polarity.
    pub spi_polarity: SpiPolarity,
}

/// SPI clock phase setting.
//
// WARNING: Do not reorder enum variants — postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpiPhase {
    /// Data captured on the leading (first) clock edge.
    CaptureOnFirstTransition = 0,
    /// Data captured on the trailing (second) clock edge.
    CaptureOnSecondTransition = 1,
}

/// SPI clock polarity setting.
//
// WARNING: Do not reorder enum variants — postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpiPolarity {
    /// Clock idles at logic low (CPOL=0).
    IdleLow = 0,
    /// Clock idles at logic high (CPOL=1).
    IdleHigh = 1,
}

/// Error returned when SPI configuration fails.
///
/// This is a convenience alias — SPI configuration shares the same error
/// type as other SPI operations.
pub type SpiConfigError = SpiError;

// --- UART

// WARNING: Do not reorder enum variants - postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
/// UART word length, in data bits per character.
///
/// The RP2350's PL011 encodes this in `UARTLCR_H.WLEN`, which is two bits
/// wide, so 5..=8 is the complete hardware range and there is no 9-bit mode.
/// The discriminants below **are** the `WLEN` encoding, which the firmware
/// relies on; `uart_data_bits_discriminants_are_the_wlen_encoding` pins it.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UartDataBits {
    /// Five data bits.
    Five = 0,
    /// Six data bits.
    Six = 1,
    /// Seven data bits.
    Seven = 2,
    /// Eight data bits. The power-on default.
    Eight = 3,
}

// WARNING: Do not reorder enum variants - postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
/// UART parity mode.
///
/// `None`, `Odd` and `Even` use the PL011's `PEN`/`EPS` bits. `Mark` and
/// `Space` additionally set `SPS` (stick parity): with `SPS` set, `EPS = 0`
/// transmits and checks the parity bit as 1, and `EPS = 1` as 0.
///
/// Mark and space are unreachable through embassy's own `uart::Parity`, which
/// has only three variants. They are offered here because the firmware writes
/// the register directly.
///
/// These indices happen to coincide with Zephyr's `UART_CFG_PARITY_*`
/// ordinals. That is a **coincidence, not a contract**: nothing here is
/// derived from Zephyr's numbering and nothing constrains this enum to keep
/// tracking it. Consumers must map explicitly rather than casting, and no
/// test pins the correspondence, because pinning it would turn the
/// coincidence into a contract by accident.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UartParity {
    /// No parity bit. The power-on default.
    None = 0,
    /// Odd parity.
    Odd = 1,
    /// Even parity.
    Even = 2,
    /// Parity bit always 1.
    Mark = 3,
    /// Parity bit always 0.
    Space = 4,
}

// WARNING: Do not reorder enum variants - postcard serializes by
// variant index, not by discriminant. Reordering breaks wire compat.
/// UART stop-bit count.
///
/// The PL011 has a single `STP2` bit, so one and two stop bits are the
/// complete hardware range. Half stop bits are not representable.
///
/// These indices deliberately do **not** match Zephyr's
/// `uart_config_stop_bits` (`0_5 = 0, 1 = 1, 1_5 = 2, 2 = 3`). Matching it
/// would bake a foreign subsystem's ABI into this wire format and leave two
/// permanently unrepresentable holes; consumers map explicitly instead.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UartStopBits {
    /// One stop bit. The power-on default.
    One = 0,
    /// Two stop bits.
    Two = 1,
}

/// Error from UART operations, propagated from firmware.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index**. Do **not** reorder or insert
/// variants in the middle — only append at the end.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartError {
    /// Request exceeds the firmware buffer limit ([`MAX_TRANSFER_SIZE`]).
    BufferTooLong,
    /// UART receiver FIFO overrun — data arrived faster than the firmware
    /// could process it.
    Overrun,
    /// A break condition was detected on the UART RX line.
    Break,
    /// Parity mismatch between received data and configured parity setting.
    Parity,
    /// The received character did not have a valid stop bit.
    Framing,
    /// The requested baud rate is invalid (zero or unsupported by hardware).
    InvalidBaudRate,
    /// An unspecified error occurred in the firmware.
    Other,
    /// The peripheral is not available on this hardware revision.
    // WARNING: Do not reorder — postcard encodes by variant index.
    Unsupported,
}

impl core::fmt::Display for UartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BufferTooLong => write!(f, "buffer exceeds firmware limit"),
            Self::Overrun => write!(f, "UART receiver overrun"),
            Self::Break => write!(f, "UART break condition"),
            Self::Parity => write!(f, "UART parity error"),
            Self::Framing => write!(f, "UART framing error"),
            Self::InvalidBaudRate => write!(f, "invalid baud rate"),
            Self::Other => write!(f, "UART error"),
            Self::Unsupported => write!(f, "UART not supported on this hardware"),
        }
    }
}

/// Request to read bytes from the UART bus.
///
/// The firmware reads up to `count` bytes from the UART receive buffer.
/// If no data is immediately available, the firmware waits up to
/// `timeout_ms` milliseconds for at least one byte. Returns whatever
/// bytes are available (1 to `count`), or an empty result on timeout.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct UartReadRequest {
    /// Maximum number of bytes to read (max [`MAX_TRANSFER_SIZE`]).
    pub count: u16,
    /// Maximum time to wait for data, in milliseconds. Use `0` for a 1 ms
    /// non-blocking poll that returns only already-buffered data. Non-zero
    /// values above the firmware's 30-minute ceiling are clamped to it.
    pub timeout_ms: u32,
}

/// Request to write bytes to the UART bus.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct UartWriteRequest<'a> {
    /// Bytes to write.
    pub contents: &'a [u8],
}

/// Request to reconfigure UART bus parameters.
///
/// Takes effect immediately. The firmware applies the new configuration
/// before processing the next UART operation.
///
/// All four parameters are applied together; there is no partial update. A
/// caller that wants to change only the baud rate must read the current
/// configuration with `uart/get-config` first and echo the framing fields
/// back.
///
/// The framing values are limited to what the RP2350 PL011 can produce. See
/// [`UartDataBits`], [`UartParity`] and [`UartStopBits`].
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct UartSetConfigurationRequest {
    /// UART baud rate in bits per second. Must be non-zero.
    pub baud_rate: u32,
    /// Word length in data bits.
    pub data_bits: UartDataBits,
    /// Parity mode.
    pub parity: UartParity,
    /// Stop-bit count.
    pub stop_bits: UartStopBits,
}

/// Error returned when UART configuration fails.
///
/// This is a convenience alias — UART configuration shares the same error
/// type as other UART operations.
pub type UartConfigError = UartError;

/// Current UART bus configuration as reported by the firmware.
///
/// Returned by `uart/get-config`. Reflects the last successfully applied
/// configuration.
///
/// This is a software shadow of what was requested, not a register
/// read-back, so `baud_rate` is the value that was asked for rather than the
/// value the divisor actually achieves after rounding.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct UartConfigurationInfo {
    /// UART baud rate in bits per second.
    pub baud_rate: u32,
    /// Word length in data bits.
    pub data_bits: UartDataBits,
    /// Parity mode.
    pub parity: UartParity,
    /// Stop-bit count.
    pub stop_bits: UartStopBits,
}

/// Current SPI bus configuration as reported by the firmware.
///
/// Returned by `spi/get-config`. The field names mirror
/// [`SpiSetConfigurationRequest`] for consistency.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct SpiConfigurationInfo {
    /// SPI bus clock frequency in Hz.
    pub spi_frequency: u32,
    /// SPI clock phase.
    pub spi_phase: SpiPhase,
    /// SPI clock polarity.
    pub spi_polarity: SpiPolarity,
}

// --- PWM

/// Number of PWM output channels exposed by the firmware.
///
/// Channels 0–3 map to physical pins GPIO12–GPIO15 on the Pico 2 header.
/// Channels 0–1 share PWM slice 6; channels 2–3 share PWM slice 7.
pub const NUM_PWM_CHANNELS: usize = 4;

/// Error from PWM operations, propagated from firmware.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index**. Do **not** reorder or insert
/// variants in the middle — only append at the end.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PwmError {
    /// The requested channel index exceeds the available PWM channels
    /// ([`NUM_PWM_CHANNELS`]).
    InvalidChannel,
    /// The requested duty cycle exceeds the maximum for the current
    /// configuration (i.e., `duty > top`).
    InvalidDutyCycle,
    /// The requested configuration is invalid (e.g., zero frequency or
    /// unsupported divider value).
    InvalidConfiguration,
    /// An unspecified error occurred in the firmware.
    Other,
}

impl core::fmt::Display for PwmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidChannel => write!(f, "invalid PWM channel"),
            Self::InvalidDutyCycle => write!(f, "duty cycle exceeds maximum"),
            Self::InvalidConfiguration => write!(f, "invalid PWM configuration"),
            Self::Other => write!(f, "PWM error"),
        }
    }
}

/// Request to set the duty cycle of a PWM channel.
///
/// The `duty` value is a raw compare value (0 to `top`). Use
/// [`PwmGetDutyCycle`] to query the current `max_duty` (top) before
/// computing a duty cycle from a percentage.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct PwmSetDutyCycleRequest {
    /// PWM channel index (0–3).
    pub channel: u8,
    /// Raw duty cycle value (0 to top).
    pub duty: u16,
}

/// Request to query the current duty cycle of a PWM channel.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct PwmGetDutyCycleRequest {
    /// PWM channel index (0–3).
    pub channel: u8,
}

/// Information about a PWM channel's current duty cycle.
///
/// The `max_duty` field corresponds to the slice's `top` register value.
/// The `current_duty` field is the raw compare value for the channel.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct PwmDutyCycleInfo {
    /// Maximum duty cycle value (the `top` value of the PWM slice).
    pub max_duty: u16,
    /// Current duty cycle value (raw compare register).
    pub current_duty: u16,
}

/// Request to enable a PWM channel's slice.
///
/// **Note:** Channels 0–1 share a slice and channels 2–3 share a slice.
/// Enabling one channel enables the entire slice (both channels).
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct PwmEnableRequest {
    /// PWM channel index (0–3). The parent slice is enabled.
    pub channel: u8,
}

/// Request to disable a PWM channel's slice.
///
/// **Note:** Channels 0–1 share a slice and channels 2–3 share a slice.
/// Disabling one channel disables the entire slice (both channels).
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct PwmDisableRequest {
    /// PWM channel index (0–3). The parent slice is disabled.
    pub channel: u8,
}

/// Request to reconfigure the PWM slice behind a channel.
///
/// Sets the output frequency and phase-correct mode. The firmware
/// computes `top` and `divider` from the requested `frequency_hz`.
///
/// **Note:** Channels 0–1 share a slice and channels 2–3 share a slice.
/// Configuring one channel reconfigures the entire slice.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct PwmSetConfigurationRequest {
    /// PWM channel index (0–3). Identifies the target slice.
    pub channel: u8,
    /// Desired PWM output frequency in Hz.
    pub frequency_hz: u32,
    /// Enable phase-correct mode. When `true`, the output frequency is
    /// halved and the pulse is centered.
    pub phase_correct: bool,
}

/// Request to query the current configuration of a PWM channel's slice.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct PwmGetConfigurationRequest {
    /// PWM channel index (0–3).
    pub channel: u8,
}

/// Error returned when PWM configuration fails.
///
/// This is a convenience alias — PWM configuration shares the same error
/// type as other PWM operations.
pub type PwmConfigError = PwmError;

/// Current PWM slice configuration as reported by the firmware.
///
/// Returned by `pwm/get-config`. Reflects the last successfully applied
/// configuration.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct PwmConfigurationInfo {
    /// Actual PWM output frequency in Hz (may differ slightly from
    /// the requested value due to divider/top quantization).
    pub frequency_hz: u32,
    /// Whether phase-correct mode is active.
    pub phase_correct: bool,
    /// Whether the slice is currently enabled.
    pub enabled: bool,
}

// --- ADC (Analog-to-Digital Converter)

/// Number of external (GPIO-based) ADC channels exposed by the firmware.
///
/// Channels [`AdcChannel::Adc0`] through [`AdcChannel::Adc3`] map to physical
/// pins GPIO26–GPIO29 on the Pico 2 header.
pub const NUM_ADC_GPIO_CHANNELS: usize = 4;

/// ADC resolution in bits.
///
/// The RP2350 has a 12-bit SAR ADC; raw readings are in the range 0–4095.
pub const ADC_RESOLUTION_BITS: u8 = 12;

/// Nominal ADC reference voltage in millivolts.
///
/// On RP2350, ADC readings are referenced to ADC_AVDD (nominally 3.3 V).
/// This is **not** a precision reference — actual voltage may vary with
/// supply quality.
pub const ADC_NOMINAL_REFERENCE_MV: u16 = 3300;

/// ADC channel selector.
///
/// Identifies which ADC input to sample. GPIO-based channels (`Adc0`–`Adc3`)
/// read external analog voltages on GPIO26–GPIO29.
///
/// # Wire Compatibility
///
/// Variants are serialized as discriminant integers (0–3). **Do not**
/// reorder or insert variants before existing ones — that changes the
/// wire encoding and breaks backward compatibility.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcChannel {
    /// ADC channel 0 — GPIO26.
    Adc0 = 0,
    /// ADC channel 1 — GPIO27.
    Adc1 = 1,
    /// ADC channel 2 — GPIO28.
    Adc2 = 2,
    /// ADC channel 3 — GPIO29.
    Adc3 = 3,
}

impl core::fmt::Display for AdcChannel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Adc0 => write!(f, "ADC0 (GPIO26)"),
            Self::Adc1 => write!(f, "ADC1 (GPIO27)"),
            Self::Adc2 => write!(f, "ADC2 (GPIO28)"),
            Self::Adc3 => write!(f, "ADC3 (GPIO29)"),
        }
    }
}

/// Error from ADC operations, propagated from firmware.
///
/// # Wire Compatibility
///
/// Variants are serialized as discriminant integers. **Do not** reorder or
/// insert variants before existing ones — that changes the wire encoding
/// and breaks backward compatibility.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcError {
    /// The ADC hardware signaled a conversion error.
    ConversionFailed,
    /// Catch-all for unexpected ADC errors.
    Other,
    /// The peripheral is not available on this hardware revision.
    // WARNING: Do not reorder — postcard encodes by variant index.
    Unsupported,
}

impl core::fmt::Display for AdcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ConversionFailed => write!(f, "ADC conversion failed"),
            Self::Other => write!(f, "ADC error"),
            Self::Unsupported => write!(f, "ADC not supported on this hardware"),
        }
    }
}

/// Error returned when ADC configuration fails.
///
/// This is a convenience alias — ADC configuration shares the same error
/// type as other ADC operations.
pub type AdcConfigError = AdcError;

/// Request to perform a single-shot ADC read on a specific channel.
///
/// Returns a raw 12-bit value (0–4095). The host can convert to voltage
/// using `V = raw * nominal_reference_mv / 4096`.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct AdcReadRequest {
    /// Which ADC channel to sample.
    pub channel: AdcChannel,
}

/// Current ADC configuration as reported by the firmware.
///
/// Returned by `adc/get-config`. Values are fixed for the RP2350 ADC
/// but exposed for host discovery and consistency with other peripherals.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct AdcConfigurationInfo {
    /// ADC resolution in bits (always 12 for RP2350).
    pub resolution_bits: u8,
    /// Nominal ADC reference voltage in millivolts (typically 3300).
    ///
    /// **Note:** This is the nominal ADC_AVDD voltage, not a precision
    /// reference. Actual voltage may vary.
    pub nominal_reference_mv: u16,
    /// Number of external (GPIO-based) ADC channels.
    pub num_gpio_channels: u8,
}

// --- 1-Wire

/// Error from 1-Wire operations, propagated from firmware.
///
/// # Wire Compatibility
///
/// Variants are serialized by **index** (0, 1, 2, …). Do **not** reorder,
/// rename, or remove existing variants — only append new ones at the end.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneWireError {
    /// No device responded to reset (no presence pulse detected).
    NoPresence,
    /// Bus communication error (short circuit, stuck bus, etc.).
    BusError,
    /// Requested transfer exceeds [`MAX_TRANSFER_SIZE`].
    BufferTooLong,
    /// Catch-all for unexpected 1-Wire errors.
    Other,
    /// The peripheral is not available on this hardware revision.
    // WARNING: Do not reorder — postcard encodes by variant index.
    Unsupported,
}

impl core::fmt::Display for OneWireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoPresence => write!(f, "no device present on 1-Wire bus"),
            Self::BusError => write!(f, "1-Wire bus error"),
            Self::BufferTooLong => write!(f, "buffer exceeds firmware limit"),
            Self::Other => write!(f, "1-Wire error"),
            Self::Unsupported => write!(f, "1-Wire not supported on this hardware"),
        }
    }
}

/// Request to read bytes from the 1-Wire bus.
///
/// The firmware reads `len` bytes from the bus after the host has already
/// issued the appropriate ROM and function commands via [`OneWireWrite`].
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct OneWireReadRequest {
    /// Number of bytes to read (max [`MAX_TRANSFER_SIZE`]).
    pub len: u16,
}

/// Request to write bytes to the 1-Wire bus.
///
/// Writes raw bytes (ROM commands, function commands, data) to the bus.
/// The host is responsible for issuing the correct 1-Wire command
/// sequences (e.g., Skip ROM `0xCC` + Convert T `0x44`).
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct OneWireWriteRequest<'a> {
    /// Bytes to write to the bus.
    pub data: &'a [u8],
}

/// Request to write bytes to the 1-Wire bus with strong pullup.
///
/// After writing, the firmware drives the data line high for
/// `pullup_duration_ms` milliseconds to supply parasitic power.
/// This is required for devices like DS18B20 that draw power from
/// the data line during temperature conversion.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct OneWireWritePullupRequest<'a> {
    /// Bytes to write to the bus.
    pub data: &'a [u8],
    /// Duration in milliseconds to hold the strong pullup after writing.
    pub pullup_duration_ms: u16,
}

// --- Transaction Batching
///
/// This limits stack usage on the firmware side. Each operation requires
/// a small amount of bookkeeping during execution.
pub const MAX_BATCH_OPS: usize = 64;

/// A single I2C operation for building a batch request.
///
/// The typed ops are serialized by postcard into the `ops` byte stream
/// of [`I2cBatchRequest`]. Use [`encode_i2c_batch_ops`] on the host
/// side to produce it, and [`postcard::take_from_bytes`] on the firmware
/// side to iterate.
///
/// WARNING: do not reorder variants — postcard encodes by index, not discriminant.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq)]
pub enum I2cBatchOp<'a> {
    /// Read `len` bytes from the device.
    Read { len: u16 },
    /// Write `data` to the device.
    Write {
        #[serde(borrow)]
        data: &'a [u8],
    },
}

/// A single SPI operation for building a batch request.
///
/// The typed ops are serialized by postcard into the `ops` byte stream
/// of [`SpiBatchRequest`]. Use [`encode_spi_batch_ops`] on the host
/// side to produce it, and [`postcard::take_from_bytes`] on the firmware
/// side to iterate.
///
/// WARNING: do not reorder variants — postcard encodes by index, not discriminant.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq)]
pub enum SpiBatchOp<'a> {
    /// Read `len` bytes from the bus (MISO only).
    Read { len: u16 },
    /// Write `data` to the bus (MOSI only).
    Write {
        #[serde(borrow)]
        data: &'a [u8],
    },
    /// Full-duplex transfer: send `data` on MOSI, receive same number of bytes on MISO.
    Transfer {
        #[serde(borrow)]
        data: &'a [u8],
    },
    /// Delay for `ns` nanoseconds (best-effort, firmware resolution).
    DelayNs { ns: u32 },
}

/// Request to execute a batch of I2C operations as a single transaction.
///
/// The `ops` field contains a sequence of postcard-serialized
/// [`I2cBatchOp`] values. Use [`encode_i2c_batch_ops`] on the host
/// side to build it from a typed slice.
///
/// ## Bus semantics
///
/// The whole batch executes as one I2C transaction, matching the
/// `embedded-hal` [`I2c::transaction`] contract:
///
/// - a START and address precede the first operation;
/// - adjacent operations of the same type are sent back to back with no
///   STOP and no repeated START between them, so two adjacent `Write`
///   ops form a single gather write;
/// - a direction change emits a repeated START and a re-addressing;
/// - a STOP follows the last operation, and only the last one.
///
/// Requires firmware built from schema 0.7 or newer. Older firmware
/// executes each operation as a separate transaction.
///
/// [`I2c::transaction`]: https://docs.rs/embedded-hal/1.0.0/embedded_hal/i2c/trait.I2c.html#tymethod.transaction
///
/// ## Response
///
/// On success, the response contains the concatenated read data from all
/// Read operations in order. The host already knows the expected lengths
/// from the request, so it can split the response accordingly.
///
/// ## Limitations
///
/// - Total read data must not exceed [`MAX_RESPONSE_PAYLOAD`], which is far
///   tighter than [`MAX_TRANSFER_SIZE`]. A batch reading more than a single
///   response frame can carry would otherwise execute in full — including
///   every `Write` — and only then lose its reply (issue #179).
/// - The whole request frame, header and encoding overhead included, must
///   not exceed [`MAX_REQUEST_FRAME`]. This is the only bound on a batch's
///   outgoing bytes, and it bounds the aggregate rather than any one
///   operation. A longer frame is discarded by the firmware's receiver
///   before any handler runs, so it would otherwise produce no reply at all
///   (issue #186). Size a batch with [`i2c_batch_request_frame_len`].
/// - Maximum [`MAX_BATCH_OPS`] operations per batch
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cBatchRequest<'a> {
    /// 7-bit I2C slave address.
    pub address: u8,
    /// Number of operations encoded in `ops`.
    pub count: u16,
    /// Postcard-serialized [`I2cBatchOp`] sequence.
    pub ops: &'a [u8],
}

/// Error returned when an I2C batch transaction fails.
///
/// ## Interpreting `failed_op`
///
/// The batch executes as one atomic transaction, so the two error
/// classes attribute differently:
///
/// - **Validation** errors are raised before any bus access and carry the
///   exact zero-based index of the offending operation.
/// - **Bus** errors are reported by the transaction as a whole and cannot
///   be attributed to one operation. They always carry `failed_op = 0`.
///
/// A non-zero `failed_op` therefore always means validation. A zero
/// `failed_op` means either the first operation failed validation or the
/// transaction failed on the bus; `kind` distinguishes them.
///
/// No read data is returned on failure.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct I2cBatchError {
    /// Zero-based index of the operation that failed.
    pub failed_op: u16,
    /// The I2C error that occurred.
    pub kind: I2cError,
}

impl core::fmt::Display for I2cBatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "I2C batch operation {} failed: {}",
            self.failed_op, self.kind
        )
    }
}

/// Request to execute a batch of SPI operations as a single transaction.
///
/// The firmware asserts CS on the specified pin before executing the
/// operations, and deasserts CS after completion — including when an
/// operation fails after CS was asserted. Failures raised *before* CS is
/// driven (pin validation and refusals) leave the pin untouched; see
/// [`SpiBatchRequest::cs_pin`] for the full contract. This provides atomic
/// [`SpiDevice::transaction`] semantics.
///
/// The `ops` field contains a sequence of postcard-serialized
/// [`SpiBatchOp`] values. Use [`encode_spi_batch_ops`] on the host
/// side to build it from a typed slice.
///
/// ## Response
///
/// On success, the response contains concatenated data from Read and
/// Transfer operations in order. The host knows expected lengths from
/// the request.
///
/// [`SpiDevice::transaction`]: embedded_hal::spi::SpiDevice::transaction
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiBatchRequest<'a> {
    /// GPIO pin index to use as chip select.
    ///
    /// Firmware implementing this protocol revision drives the pin as an output
    /// during a successful batch and leaves it configured as an output,
    /// deasserted high; the prior direction is not restored. An execution failure
    /// after assertion also leaves CS deasserted high. Pre-validation and refusal
    /// failures leave the pin untouched. Pins explicitly configured as inputs are
    /// refused; firmware predating this contract may instead reconfigure them.
    pub cs_pin: u8,
    /// Number of operations encoded in `ops`.
    pub count: u16,
    /// Postcard-serialized [`SpiBatchOp`] sequence.
    pub ops: &'a [u8],
}

/// Error returned when an SPI batch transaction fails.
///
/// See [`I2cBatchError`] for the general pattern. For SPI batches, the
/// firmware deasserts CS before returning whenever it had already asserted
/// it; errors raised before CS is driven leave the pin untouched. See
/// [`SpiBatchRequest::cs_pin`] for the full contract.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpiBatchError {
    /// Zero-based index of the operation that failed.
    pub failed_op: u16,
    /// The SPI error that occurred.
    pub kind: SpiError,
}

impl core::fmt::Display for SpiBatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "SPI batch operation {} failed: {}",
            self.failed_op, self.kind
        )
    }
}

/// Encode a sequence of I2C batch operations into the postcard wire format.
///
/// Returns the serialized byte stream suitable for [`I2cBatchRequest::ops`].
///
/// There is no limit on the size of an individual operation: each one is
/// serialized straight into the growable output buffer, and unlike a plain
/// write it is not bounded by [`MAX_TRANSFER_SIZE`], because the firmware
/// streams it out of the received frame rather than through its scratch
/// buffer.
///
/// The *total* request is bounded, by [`MAX_REQUEST_FRAME`]. This function
/// still does not police that — it has no view of the header — but
/// [`i2c_batch_request_frame_len`] and [`spi_batch_request_frame_len`] do,
/// and `pico-de-gallo-lib` applies them before transmission (issue #186).
///
/// # Panics
///
/// Panics if `ops.len()` exceeds [`MAX_BATCH_OPS`]. Serialization itself
/// cannot fail, because [`postcard::to_extend`] only reports an error when the
/// sink rejects a write and [`Vec`] never does.
#[cfg(feature = "use-std")]
pub fn encode_i2c_batch_ops(ops: &[I2cBatchOp<'_>]) -> Vec<u8> {
    assert!(ops.len() <= MAX_BATCH_OPS, "too many batch operations");
    let mut buf = Vec::new();
    for op in ops {
        buf = postcard::to_extend(op, buf).expect("serializing into a Vec cannot fail");
    }
    buf
}

/// Encode a sequence of SPI batch operations into the postcard wire format.
///
/// Returns the serialized byte stream suitable for [`SpiBatchRequest::ops`].
///
/// There is no limit on the size of an individual operation: each one is
/// serialized straight into the growable output buffer, and unlike a plain
/// write it is not bounded by [`MAX_TRANSFER_SIZE`], because the firmware
/// streams it out of the received frame rather than through its scratch
/// buffer.
///
/// The *total* request is bounded, by [`MAX_REQUEST_FRAME`]. This function
/// still does not police that — it has no view of the header — but
/// [`i2c_batch_request_frame_len`] and [`spi_batch_request_frame_len`] do,
/// and `pico-de-gallo-lib` applies them before transmission (issue #186).
///
/// # Panics
///
/// Panics if `ops.len()` exceeds [`MAX_BATCH_OPS`]. Serialization itself
/// cannot fail, because [`postcard::to_extend`] only reports an error when the
/// sink rejects a write and [`Vec`] never does.
#[cfg(feature = "use-std")]
pub fn encode_spi_batch_ops(ops: &[SpiBatchOp<'_>]) -> Vec<u8> {
    assert!(ops.len() <= MAX_BATCH_OPS, "too many batch operations");
    let mut buf = Vec::new();
    for op in ops {
        buf = postcard::to_extend(op, buf).expect("serializing into a Vec cannot fail");
    }
    buf
}

/// Compute the total number of bytes that will appear in the response for
/// an I2C batch — the sum of all Read lengths.
pub fn i2c_batch_response_len(ops: &[I2cBatchOp<'_>]) -> usize {
    ops.iter()
        .map(|op| match op {
            I2cBatchOp::Read { len } => *len as usize,
            I2cBatchOp::Write { .. } => 0,
        })
        .sum()
}

/// Compute the total number of bytes that will appear in the response for
/// an SPI batch — the sum of Read and Transfer lengths.
pub fn spi_batch_response_len(ops: &[SpiBatchOp<'_>]) -> usize {
    ops.iter()
        .map(|op| match op {
            SpiBatchOp::Read { len } => *len as usize,
            SpiBatchOp::Transfer { data } => data.len(),
            _ => 0,
        })
        .sum()
}

/// Encoded length of `n` as a postcard varint, in bytes.
///
/// postcard writes an unsigned integer as LEB128: seven payload bits per
/// byte, high bit set on every byte but the last. Used to size the
/// `count` field and the two length prefixes that a batch request carries,
/// none of which are a fixed width.
const fn varint_len(mut n: usize) -> usize {
    let mut len = 1;
    while n >= 0x80 {
        n >>= 7;
        len += 1;
    }
    len
}

/// Encoded length of one [`I2cBatchOp`] in the `ops` byte stream.
///
/// One byte of variant index — both variants are below `0x80`, so the index
/// varint is always one byte — then the operation's own fields.
fn i2c_batch_op_len(op: &I2cBatchOp<'_>) -> usize {
    1 + match op {
        I2cBatchOp::Read { len } => varint_len(*len as usize),
        I2cBatchOp::Write { data } => varint_len(data.len()) + data.len(),
    }
}

/// Encoded length of one [`SpiBatchOp`] in the `ops` byte stream.
fn spi_batch_op_len(op: &SpiBatchOp<'_>) -> usize {
    1 + match op {
        SpiBatchOp::Read { len } => varint_len(*len as usize),
        SpiBatchOp::Write { data } | SpiBatchOp::Transfer { data } => {
            varint_len(data.len()) + data.len()
        }
        SpiBatchOp::DelayNs { ns } => varint_len(*ns as usize),
    }
}

/// Bytes a batch request occupies around its encoded operation stream.
///
/// The shared shape of [`I2cBatchRequest`] and [`SpiBatchRequest`]: a `u8`
/// selector (`address` or `cs_pin`, one plain byte — postcard does not
/// varint a `u8`), the `u16` operation `count`, and the `ops` slice's own
/// varint length prefix.
fn batch_request_frame_len(count: usize, encoded_ops_len: usize) -> usize {
    REQUEST_HEADER_LEN_MAX + 1 + varint_len(count) + varint_len(encoded_ops_len) + encoded_ops_len
}

/// Compute the number of bytes an I2C batch occupies as a request frame on
/// the wire, header included.
///
/// The mirror of [`i2c_batch_response_len`], and the quantity to compare
/// against [`MAX_REQUEST_FRAME`]. A batch is the only supported way to
/// build an over-ceiling request, because its aggregate outgoing bytes are
/// bounded by nothing else (issue #186).
///
/// Assumes the widest header, so the answer does not depend on whether the
/// caller's client has received a reply yet — see [`REQUEST_HEADER_LEN_MAX`].
/// On a warm connection the real frame is six bytes shorter than this.
///
/// Computed rather than measured so it costs no second encoding pass; the
/// arithmetic is pinned against [`encode_i2c_batch_ops`] and against
/// [`postcard::to_allocvec`] of a real [`I2cBatchRequest`] by
/// `i2c_batch_request_frame_len_matches_a_real_encoding`.
pub fn i2c_batch_request_frame_len(ops: &[I2cBatchOp<'_>]) -> usize {
    let encoded: usize = ops.iter().map(i2c_batch_op_len).sum();
    batch_request_frame_len(ops.len(), encoded)
}

/// Compute the number of bytes an SPI batch occupies as a request frame on
/// the wire, header included.
///
/// The send-direction counterpart of [`spi_batch_response_len`]. `Write`
/// and `Transfer` payloads both travel out; `Read` and `DelayNs` cost only
/// their operation encoding. See [`i2c_batch_request_frame_len`] for why
/// the widest header is assumed.
pub fn spi_batch_request_frame_len(ops: &[SpiBatchOp<'_>]) -> usize {
    let encoded: usize = ops.iter().map(spi_batch_op_len).sum();
    batch_request_frame_len(ops.len(), encoded)
}

// --- Version
/// Firmware version information.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct VersionInfo {
    /// Major version number.
    pub major: u16,
    /// Minor version number.
    pub minor: u16,
    /// Patch version number.
    pub patch: u32,
}

// --- Device Info

/// Hardware capabilities — a bitflag set describing which peripherals are
/// available on the connected device.
///
/// Each capability is a single bit. New capabilities can be added by
/// defining new constants without changing the wire format — existing
/// bits remain stable.
///
/// Use [`BitOr`](core::ops::BitOr) to combine flags and
/// [`contains`](Capabilities::contains) to test them:
///
/// ```
/// use pico_de_gallo_internal::Capabilities;
///
/// let caps = Capabilities::I2C | Capabilities::SPI;
/// assert!(caps.contains(Capabilities::I2C));
/// assert!(!caps.contains(Capabilities::UART));
/// ```
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Capabilities(pub u64);

impl Capabilities {
    /// No capabilities.
    pub const NONE: Self = Self(0);
    /// I2C bus support (bit 0).
    pub const I2C: Self = Self(1 << 0);
    /// SPI bus support (bit 1).
    pub const SPI: Self = Self(1 << 1);
    /// UART support (bit 2).
    pub const UART: Self = Self(1 << 2);
    /// GPIO support (bit 3).
    pub const GPIO: Self = Self(1 << 3);
    /// PWM output support (bit 4).
    pub const PWM: Self = Self(1 << 4);
    /// ADC input support (bit 5).
    pub const ADC: Self = Self(1 << 5);
    /// 1-Wire bus support (bit 6).
    pub const ONEWIRE: Self = Self(1 << 6);

    /// Returns `true` if all bits in `other` are set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns the raw `u64` bitfield value.
    pub const fn bits(self) -> u64 {
        self.0
    }
}

impl core::ops::BitOr for Capabilities {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for Capabilities {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// Maximum length of [`DeviceInfo::build_id()`], in bytes.
///
/// The field must always be spelled `heapless::String<BUILD_ID_CAPACITY>`,
/// leaving `LenT` at its default. `postcard-schema` implements `Schema` only
/// for `String<N>` at the default `LenT`, so naming a non-default `LenT`
/// silently breaks the `Schema` derive.
///
/// This is a *receive-side* bound. `heapless` deserialization rejects a longer
/// string rather than truncating it, so the firmware build script is
/// responsible for truncating; see `crates/pico-de-gallo-firmware/build.rs`.
/// Raising this value is not a wire-format change (the encoding is a plain
/// postcard string either way), but it is a `Schema` change and still needs a
/// lockstep bump per AGENTS.md §6.2.
pub const BUILD_ID_CAPACITY: usize = 64;

/// Extended device information including firmware version, schema version,
/// hardware version, and peripheral capabilities.
///
/// This is returned by a separate endpoint from [`VersionInfo`] so that
/// changes here cannot disturb the `version` endpoint. That separation is
/// what keeps `version` working against a peer whose `device/info` has
/// re-keyed: [`VersionInfo`]'s schema is unchanged, so the `version`
/// endpoint key is unchanged, and old and new peers still match each
/// other's frames. It is not that older hosts parse `DeviceInfo`
/// leniently — they never see it.
// CHANGING THIS TYPE IS QUALITATIVELY WORSE THAN CHANGING ANY OTHER WIRE
// TYPE. Think hard before adding, removing, or retyping a field here.
//
// postcard-rpc keys every endpoint by `Key::for_path::<T>(path)`, i.e. a
// hash of the type's `Schema` together with the path string, and the
// `endpoints!` macro derives `RESP_KEY` from the RESPONSE type's schema.
// Any change to a request or response type's shape therefore silently
// re-keys that endpoint, appends included. A peer built against the other
// shape replies under the other key; the receiving dispatcher finds no
// match, drops the frame, and never wakes the pending call. The call does
// not return. This is NOT a decode error -- postcard is never reached --
// so `map_validate_error` never sees a `DeserFailed`.
//
// For every other wire type that is survivable, because `device/info`
// itself still answers and `PicoDeGallo::validate()` reports the version
// difference as a `SchemaMismatch`. For THIS type it is not: re-keying
// `device/info` breaks the very probe that would have reported the
// mismatch, so the schema numbers that describe the incompatibility are
// sealed inside the one message that is dropped. `validate()` can only
// time out, indistinguishable from an unresponsive board, and bumping the
// schema version does not help -- the version is payload, not key.
//
// `DeviceInfo` is thus a blind spot for its own versioning mechanism. See
// `device_info_response_key_is_pinned` below, which fails the suite rather
// than letting a re-key reach the field silently.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct DeviceInfo {
    /// Firmware version — major.
    pub fw_major: u16,
    /// Firmware version — minor.
    pub fw_minor: u16,
    /// Firmware version — patch.
    pub fw_patch: u32,
    /// Schema (wire protocol) version — major.
    pub schema_major: u16,
    /// Schema (wire protocol) version — minor.
    pub schema_minor: u16,
    /// Schema (wire protocol) version — patch.
    pub schema_patch: u32,
    /// Hardware revision number (1 = original Pico 2 board).
    pub hw_version: u8,
    /// Peripheral capabilities of the connected device.
    pub capabilities: Capabilities,
    /// Runtime-authoritative number of user-controllable GPIO pins.
    ///
    /// Valid GPIO and SPI chip-select indices are `0..num_gpios`; when this value
    /// is zero, no index is valid. This supersedes the compile-time [`NUM_GPIOS`]
    /// default and must never be synthesized or defaulted when `device/info`
    /// decoding fails.
    pub num_gpios: u8,
    /// Firmware build identity.
    ///
    /// The output of `git describe --always --dirty --tags --match
    /// firmware-v*` captured when the firmware was built, or `"unknown"` when
    /// git was unavailable at build time.
    ///
    /// Informational only. This is **never** a compatibility gate: schema
    /// version answers "can we talk?", and this field answers "are you the
    /// build I think you are?". `PicoDeGallo::validate()` deliberately ignores
    /// it. Use [`DeviceInfo::build_id()`] to read it as a `&str`.
    pub build_id: heapless::String<BUILD_ID_CAPACITY>,
}

impl DeviceInfo {
    /// The firmware build identity as a string slice.
    ///
    /// Provided so host crates never need to name `heapless` themselves.
    ///
    /// Note for doc authors: this method and the `build_id` field share a
    /// name, so a bare intra-doc link `[DeviceInfo::build_id]` is ambiguous
    /// and fails the `RUSTDOCFLAGS` doc job. Always write
    /// `[DeviceInfo::build_id()]` for this method.
    pub fn build_id(&self) -> &str {
        &self.build_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postcard::{from_bytes, take_from_bytes, to_allocvec};
    use postcard_rpc::Endpoint;

    // --- Endpoint key pinning ---
    //
    // postcard-rpc keys each endpoint by `Key::for_path::<T>(path)`, a hash
    // of the type's `Schema` and the path string; the `endpoints!` macro
    // derives `RESP_KEY` from the *response* type. These keys are therefore
    // derived from the struct's shape: changing a field -- adding, removing,
    // renaming, or retyping one -- changes the key.
    //
    // A re-keyed endpoint does not produce a decode error. A peer built from
    // a different tree replies under the old key, the receiving dispatcher
    // finds no match, drops the frame, and the pending call is never woken:
    // the call hangs until its caller-side timeout, if it has one.
    //
    // If one of these tests fails, you have made a wire-incompatible change.
    // That is allowed, but it requires a lockstep schema-version bump per
    // AGENTS.md §6.2/§6.5 and a coordinated firmware release -- then update
    // the pinned bytes here in the same commit.

    /// Pins the `device/info` response key, derived from [`DeviceInfo`].
    #[test]
    fn device_info_response_key_is_pinned() {
        assert_eq!(
            GetDeviceInfo::RESP_KEY.to_bytes(),
            [0x63, 0x8a, 0x52, 0xf9, 0xb6, 0xda, 0xea, 0x52],
            "device/info's endpoint key changed, which means DeviceInfo's \
             schema changed. A peer built from a different tree will reply \
             under the old key, the frame is dropped unmatched, and the call \
             hangs instead of erroring. If this change is intended, bump the \
             schema version per AGENTS.md §6.2/§6.5, release firmware and \
             host in lockstep, and update this pin in the same commit."
        );
    }

    /// Pins the `version` response key, derived from [`VersionInfo`].
    ///
    /// `version` keeping working against a released firmware while
    /// `device/info` hangs is precisely because this key is unchanged; that
    /// stability is a documented guarantee, so pin it.
    #[test]
    fn version_response_key_is_pinned() {
        assert_eq!(
            Version::RESP_KEY.to_bytes(),
            [0xe7, 0xf8, 0x84, 0x38, 0xd2, 0xc6, 0x29, 0xc6],
            "version's endpoint key changed, which means VersionInfo's schema \
             changed. VersionInfo's stability is a documented guarantee: it is \
             why `gallo version`'s legacy fallback still reports something \
             useful against a firmware whose device/info has re-keyed. Break \
             it and that last diagnostic path hangs too, leaving no way to \
             identify a mismatched board. Do not re-key VersionInfo to carry \
             new data -- add a field to DeviceInfo instead. If this really is \
             intended, bump the schema version per AGENTS.md §6.2/§6.5 and \
             update this pin in the same commit."
        );
    }

    // --- Schema version vs. Cargo.toml ---

    /// AGENTS.md §13.8 warns about a stale-incremental-cache trap
    /// where `SCHEMA_VERSION_*` constants don't match the crate
    /// `[package].version`. The `build.rs` derives them from the
    /// package version at build time, but a corrupted incremental
    /// cache could theoretically ship a mismatched build. This test
    /// guards against that by parsing `CARGO_PKG_VERSION` at compile
    /// time and asserting equality.
    ///
    /// Closes Category A finding #35.
    #[test]
    fn schema_version_matches_cargo_pkg_version() {
        let cargo_version = env!("CARGO_PKG_VERSION");
        let mut parts = cargo_version.split('.');
        let major_str = parts.next().expect("CARGO_PKG_VERSION must have major");
        let minor_str = parts.next().expect("CARGO_PKG_VERSION must have minor");
        let patch_str = parts.next().expect("CARGO_PKG_VERSION must have patch");
        assert!(
            parts.next().is_none(),
            "CARGO_PKG_VERSION must be MAJOR.MINOR.PATCH; got {cargo_version}",
        );
        let cargo_major: u16 = major_str
            .parse()
            .expect("CARGO_PKG_VERSION major must parse as u16");
        let cargo_minor: u16 = minor_str
            .parse()
            .expect("CARGO_PKG_VERSION minor must parse as u16");
        let cargo_patch: u32 = patch_str
            .parse()
            .expect("CARGO_PKG_VERSION patch must parse as u32");
        assert_eq!(
            SCHEMA_VERSION_MAJOR, cargo_major,
            "SCHEMA_VERSION_MAJOR ({SCHEMA_VERSION_MAJOR}) does not match \
             CARGO_PKG_VERSION ({cargo_version}). Did you bump the [package].version \
             but skip a clean rebuild? See AGENTS.md §13.8.",
        );
        assert_eq!(
            SCHEMA_VERSION_MINOR, cargo_minor,
            "SCHEMA_VERSION_MINOR ({SCHEMA_VERSION_MINOR}) does not match \
             CARGO_PKG_VERSION ({cargo_version}).",
        );
        assert_eq!(
            SCHEMA_VERSION_PATCH, cargo_patch,
            "SCHEMA_VERSION_PATCH ({SCHEMA_VERSION_PATCH}) does not match \
             CARGO_PKG_VERSION ({cargo_version}).",
        );
    }

    // --- I2C round-trip tests ---

    #[test]
    fn i2c_read_request_round_trip() {
        let req = I2cReadRequest {
            address: 0x48,
            count: 4,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn i2c_write_request_round_trip() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        let req = I2cWriteRequest {
            address: 0x50,
            contents: &data,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn i2c_write_read_request_round_trip() {
        let data = [0x01, 0x02];
        let req = I2cWriteReadRequest {
            address: 0x68,
            contents: &data,
            count: 6,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cWriteReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn i2c_read_request_max_count() {
        let req = I2cReadRequest {
            address: 0x7F,
            count: u16::MAX,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    // --- SPI round-trip tests ---

    #[test]
    fn spi_read_request_round_trip() {
        let req = SpiReadRequest { count: 128 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_write_request_round_trip() {
        let data = [0xCA, 0xFE];
        let req = SpiWriteRequest { contents: &data };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_transfer_request_round_trip() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let req = SpiTransferRequest { contents: &data };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiTransferRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn spi_transfer_request_max_size() {
        let data = vec![0xAA; MAX_TRANSFER_SIZE];
        let req = SpiTransferRequest { contents: &data };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiTransferRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    // --- GPIO round-trip tests ---

    #[test]
    fn gpio_get_request_round_trip() {
        for pin in 0..8u8 {
            let req = GpioGetRequest { pin };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: GpioGetRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn gpio_put_request_round_trip() {
        for state in [GpioState::Low, GpioState::High] {
            let req = GpioPutRequest { pin: 3, state };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: GpioPutRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn gpio_wait_request_round_trip() {
        let req = GpioWaitRequest {
            pin: 7,
            timeout_ms: 0,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: GpioWaitRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn gpio_wait_request_round_trip_with_timeout() {
        let original = GpioWaitRequest {
            pin: 2,
            timeout_ms: 500,
        };
        let bytes = to_allocvec(&original).unwrap();
        let decoded: GpioWaitRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded.pin, 2);
        assert_eq!(decoded.timeout_ms, 500);
    }

    #[test]
    fn gpio_state_round_trip() {
        for state in [GpioState::Low, GpioState::High] {
            let bytes = to_allocvec(&state).unwrap();
            let decoded: GpioState = from_bytes(&bytes).unwrap();
            assert_eq!(state, decoded);
        }
    }

    #[test]
    fn gpio_state_from_bool() {
        assert_eq!(GpioState::from(true), GpioState::High);
        assert_eq!(GpioState::from(false), GpioState::Low);
    }

    #[test]
    fn bool_from_gpio_state() {
        assert!(bool::from(GpioState::High));
        assert!(!bool::from(GpioState::Low));
    }

    // --- Config round-trip tests ---

    #[test]
    fn i2c_set_configuration_request_round_trip() {
        let req = I2cSetConfigurationRequest {
            frequency: I2cFrequency::Fast,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_set_configuration_request_round_trip() {
        let req = SpiSetConfigurationRequest {
            spi_frequency: 1_000_000,
            spi_phase: SpiPhase::CaptureOnSecondTransition,
            spi_polarity: SpiPolarity::IdleHigh,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_phase_round_trip() {
        for phase in [
            SpiPhase::CaptureOnFirstTransition,
            SpiPhase::CaptureOnSecondTransition,
        ] {
            let bytes = to_allocvec(&phase).unwrap();
            let decoded: SpiPhase = from_bytes(&bytes).unwrap();
            assert_eq!(phase, decoded);
        }
    }

    #[test]
    fn spi_polarity_round_trip() {
        for pol in [SpiPolarity::IdleLow, SpiPolarity::IdleHigh] {
            let bytes = to_allocvec(&pol).unwrap();
            let decoded: SpiPolarity = from_bytes(&bytes).unwrap();
            assert_eq!(pol, decoded);
        }
    }

    // --- Version round-trip test ---

    #[test]
    fn version_info_round_trip() {
        let ver = VersionInfo {
            major: 1,
            minor: 2,
            patch: 42,
        };
        let bytes = to_allocvec(&ver).unwrap();
        let decoded: VersionInfo = from_bytes(&bytes).unwrap();
        assert_eq!(ver, decoded);
    }

    // --- Error enum round-trip tests ---

    /// Compile-time exhaustiveness tripwire. Appending a variant to
    /// [`I2cError`] is legal on the wire, but must not happen silently:
    /// this stops compiling until the author has seen — and extended — the
    /// hand-maintained tables below and the exhaustive match sites in
    /// `pico-de-gallo-ffi`.
    fn i2c_error_variant_index_witness(e: I2cError) -> u8 {
        match e {
            I2cError::Bus => 0,
            I2cError::NoAcknowledge => 1,
            I2cError::ArbitrationLoss => 2,
            I2cError::Overrun => 3,
            I2cError::BufferTooLong => 4,
            I2cError::AddressOutOfRange => 5,
            I2cError::Other => 6,
            I2cError::ZeroLengthWrite => 7,
        }
    }

    const I2C_ERROR_VARIANTS: [I2cError; 8] = [
        I2cError::Bus,
        I2cError::NoAcknowledge,
        I2cError::ArbitrationLoss,
        I2cError::Overrun,
        I2cError::BufferTooLong,
        I2cError::AddressOutOfRange,
        I2cError::Other,
        I2cError::ZeroLengthWrite,
    ];

    #[test]
    fn i2c_error_variants_round_trip() {
        for err in I2C_ERROR_VARIANTS {
            let bytes = to_allocvec(&err).unwrap();
            let decoded: I2cError = from_bytes(&bytes).unwrap();
            assert_eq!(err, decoded);
        }
    }

    #[test]
    fn i2c_error_variant_indices_are_stable() {
        // These values are deployed wire ABI. This test must NOT be updated
        // to accommodate an insertion, a swap, or a deletion — changing an
        // index is a deployed wire break. Appending a new variant at index 8
        // is the only legal change. See AGENTS.md §6.1 and §13.17.
        for (index, variant) in I2C_ERROR_VARIANTS.iter().copied().enumerate() {
            let n = u8::try_from(index).unwrap();
            assert_eq!(to_allocvec(&variant).unwrap().as_slice(), &[n][..]);
            let decoded: I2cError = from_bytes(&[n]).unwrap();
            assert_eq!(decoded, variant);
            assert_eq!(i2c_error_variant_index_witness(variant), n);
        }
    }

    #[test]
    fn i2c_error_encodings_are_distinct() {
        for (i, a_variant) in I2C_ERROR_VARIANTS.iter().enumerate() {
            for (j, b_variant) in I2C_ERROR_VARIANTS.iter().enumerate().skip(i + 1) {
                let a = to_allocvec(a_variant).unwrap();
                let b = to_allocvec(b_variant).unwrap();
                assert_ne!(a, b, "I2cError variants {i} and {j} share an encoding");
            }
        }
    }

    #[test]
    fn i2c_error_rejects_unknown_variant_index() {
        // Index 8 is one past the last defined variant. If a ninth variant
        // is ever appended, this probe must move to the new first-unused
        // index rather than being deleted.
        assert!(from_bytes::<I2cError>(&[8u8]).is_err());
    }

    #[test]
    fn i2c_error_zero_length_write_is_appended_last() {
        // Regression guard for issue #101. `ZeroLengthWrite` was appended
        // after `Other`; asserting its index here means a future insertion
        // above it fails loudly instead of silently remapping a deployed
        // firmware's error codes.
        assert_eq!(
            to_allocvec(&I2cError::ZeroLengthWrite).unwrap().as_slice(),
            &[7u8][..]
        );
    }

    #[test]
    fn gpio_error_variants_round_trip() {
        for err in [
            GpioError::InvalidPin,
            GpioError::Other,
            GpioError::WrongDirection,
            GpioError::PinMonitored,
            GpioError::PinNotMonitored,
            GpioError::Timeout,
        ] {
            let bytes = to_allocvec(&err).unwrap();
            let decoded: GpioError = from_bytes(&bytes).unwrap();
            assert_eq!(err, decoded);
        }
    }

    #[test]
    fn gpio_error_timeout_variant_round_trip() {
        let bytes = postcard::to_allocvec(&GpioError::Timeout).unwrap();
        let decoded: GpioError = postcard::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded, GpioError::Timeout));
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn i2c_error_display() {
        assert_eq!(
            format!("{}", I2cError::NoAcknowledge),
            "no acknowledge from target"
        );
        assert_eq!(format!("{}", I2cError::Bus), "I2C bus error");
        assert_eq!(
            format!("{}", I2cError::ArbitrationLoss),
            "I2C arbitration loss"
        );
        assert_eq!(format!("{}", I2cError::Overrun), "I2C data overrun");
        assert_eq!(
            format!("{}", I2cError::BufferTooLong),
            "buffer exceeds firmware limit"
        );
        assert_eq!(
            format!("{}", I2cError::AddressOutOfRange),
            "I2C address out of range"
        );
        assert_eq!(format!("{}", I2cError::Other), "I2C error");
        assert_eq!(
            format!("{}", I2cError::ZeroLengthWrite),
            "zero-length I2C write is not supported by this hardware"
        );
    }

    /// Compile-time exhaustiveness tripwire. Appending a variant to
    /// [`SpiError`] is legal on the wire, but must not happen silently:
    /// this stops compiling until the author has seen — and extended — the
    /// hand-maintained tables below and the exhaustive match sites in
    /// `pico-de-gallo-ffi`.
    fn spi_error_variant_index_witness(e: SpiError) -> u8 {
        match e {
            SpiError::BufferTooLong => 0,
            SpiError::Other => 1,
            SpiError::InvalidCsPin => 2,
            SpiError::CsPinUnavailable => 3,
            SpiError::CsPinMonitored => 4,
        }
    }

    const SPI_ERROR_VARIANTS: [SpiError; 5] = [
        SpiError::BufferTooLong,
        SpiError::Other,
        SpiError::InvalidCsPin,
        SpiError::CsPinUnavailable,
        SpiError::CsPinMonitored,
    ];

    #[test]
    fn spi_error_variants_round_trip() {
        for variant in SPI_ERROR_VARIANTS {
            let bytes = to_allocvec(&variant).unwrap();
            let decoded: SpiError = from_bytes(&bytes).unwrap();
            assert_eq!(variant, decoded);
        }
    }

    #[test]
    fn spi_error_variant_indices_are_stable() {
        // These values are deployed wire ABI. This test must NOT be updated
        // to accommodate an insertion, a swap, or a deletion — changing an
        // index is a deployed wire break. Appending a new variant at index 5
        // is the only legal change. See AGENTS.md §6.1 and §13.17.
        for (index, variant) in SPI_ERROR_VARIANTS.iter().copied().enumerate() {
            let n = u8::try_from(index).unwrap();
            assert_eq!(to_allocvec(&variant).unwrap().as_slice(), &[n][..]);
            let decoded: SpiError = from_bytes(&[n]).unwrap();
            assert_eq!(decoded, variant);
            assert_eq!(spi_error_variant_index_witness(variant), n);
        }
    }

    #[test]
    fn spi_error_encodings_are_distinct() {
        for (i, a_variant) in SPI_ERROR_VARIANTS.iter().enumerate() {
            for (j, b_variant) in SPI_ERROR_VARIANTS.iter().enumerate().skip(i + 1) {
                let a = to_allocvec(a_variant).unwrap();
                let b = to_allocvec(b_variant).unwrap();
                assert_ne!(a, b, "SpiError variants {i} and {j} share an encoding");
            }
        }
    }

    #[test]
    fn spi_error_rejects_unknown_variant_index() {
        // Index 5 is one past the last defined variant. If a sixth variant
        // is ever appended, this probe must move to the new first-unused
        // index rather than being deleted.
        assert!(from_bytes::<SpiError>(&[5u8]).is_err());
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn spi_error_display() {
        assert_eq!(
            format!("{}", SpiError::BufferTooLong),
            "buffer exceeds firmware limit"
        );
        assert_eq!(format!("{}", SpiError::Other), "SPI error");
        assert_eq!(
            format!("{}", SpiError::InvalidCsPin),
            "invalid SPI chip-select pin"
        );
        assert_eq!(
            format!("{}", SpiError::CsPinUnavailable),
            "SPI chip-select pin is configured as input"
        );
        assert_eq!(
            format!("{}", SpiError::CsPinMonitored),
            "SPI chip-select pin is being monitored for events"
        );
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn spi_error_display_strings_are_distinct() {
        for (i, a) in SPI_ERROR_VARIANTS.iter().enumerate() {
            for (j, b) in SPI_ERROR_VARIANTS.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    format!("{a}"),
                    format!("{b}"),
                    "SpiError variants {i} and {j} render identically"
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn gpio_error_display() {
        assert_eq!(
            format!("{}", GpioError::InvalidPin),
            "invalid GPIO pin number"
        );
        assert_eq!(format!("{}", GpioError::Other), "GPIO error");
        assert_eq!(
            format!("{}", GpioError::WrongDirection),
            "GPIO pin configured in wrong direction"
        );
        assert_eq!(
            format!("{}", GpioError::PinMonitored),
            "GPIO pin is being monitored for events"
        );
        assert_eq!(
            format!("{}", GpioError::PinNotMonitored),
            "GPIO pin is not monitored"
        );
        assert_eq!(format!("{}", GpioError::Timeout), "GPIO wait timed out");
    }

    // --- P1: Schema stability tests ---
    //
    // These lock down the wire encoding for each type. If a field is
    // added, removed, or reordered the serialized bytes will change
    // and these tests will catch it.

    #[test]
    fn i2c_read_request_wire_stability() {
        let req = I2cReadRequest {
            address: 0x48,
            count: 4,
        };
        let bytes = to_allocvec(&req).unwrap();
        assert_eq!(
            bytes,
            to_allocvec(&req).unwrap(),
            "encoding is deterministic"
        );
        // Re-decode and compare to ensure exact round-trip
        let decoded: I2cReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        // Lock the exact byte representation
        let snapshot = bytes.clone();
        let freshly_encoded = to_allocvec(&decoded).unwrap();
        assert_eq!(freshly_encoded, snapshot, "wire format must not change");
    }

    #[test]
    fn i2c_set_configuration_request_wire_stability() {
        let req = I2cSetConfigurationRequest {
            frequency: I2cFrequency::Fast,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: I2cSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn spi_set_configuration_request_wire_stability() {
        let req = SpiSetConfigurationRequest {
            spi_frequency: 1_000_000,
            spi_phase: SpiPhase::CaptureOnFirstTransition,
            spi_polarity: SpiPolarity::IdleLow,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: SpiSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn version_info_wire_stability() {
        let ver = VersionInfo {
            major: 1,
            minor: 0,
            patch: 0,
        };
        let bytes = to_allocvec(&ver).unwrap();
        let canonical = bytes.clone();
        let decoded: VersionInfo = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, ver);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn gpio_put_request_wire_stability() {
        let req = GpioPutRequest {
            pin: 0,
            state: GpioState::High,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: GpioPutRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    // --- P1: Boundary value tests ---

    #[test]
    fn i2c_read_request_zero_count() {
        let req = I2cReadRequest {
            address: 0x00,
            count: 0,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn i2c_read_request_max_address() {
        let req = I2cReadRequest {
            address: u8::MAX,
            count: 1,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_read_request_max_count() {
        let req = SpiReadRequest { count: u16::MAX };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_read_request_zero_count() {
        let req = SpiReadRequest { count: 0 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn i2c_write_request_empty_contents() {
        let req = I2cWriteRequest {
            address: 0x50,
            contents: &[],
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn spi_write_request_empty_contents() {
        let req = SpiWriteRequest { contents: &[] };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn i2c_write_read_request_empty_contents_max_count() {
        let req = I2cWriteReadRequest {
            address: 0x7F,
            contents: &[],
            count: u16::MAX,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cWriteReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn gpio_get_request_all_pins() {
        for pin in 0..=u8::MAX {
            let req = GpioGetRequest { pin };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: GpioGetRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn gpio_wait_request_all_pins() {
        for pin in 0..=u8::MAX {
            let req = GpioWaitRequest { pin, timeout_ms: 0 };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: GpioWaitRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn version_info_boundary_values() {
        for ver in [
            VersionInfo {
                major: 0,
                minor: 0,
                patch: 0,
            },
            VersionInfo {
                major: u16::MAX,
                minor: u16::MAX,
                patch: u32::MAX,
            },
        ] {
            let bytes = to_allocvec(&ver).unwrap();
            let decoded: VersionInfo = from_bytes(&bytes).unwrap();
            assert_eq!(ver, decoded);
        }
    }

    #[test]
    fn spi_set_configuration_request_all_enum_combinations() {
        for (phase, polarity) in [
            (SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleLow),
            (SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleHigh),
            (SpiPhase::CaptureOnSecondTransition, SpiPolarity::IdleLow),
            (SpiPhase::CaptureOnSecondTransition, SpiPolarity::IdleHigh),
        ] {
            let req = SpiSetConfigurationRequest {
                spi_frequency: 500_000,
                spi_phase: phase,
                spi_polarity: polarity,
            };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: SpiSetConfigurationRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn i2c_set_configuration_request_all_frequencies() {
        for freq in [
            I2cFrequency::Standard,
            I2cFrequency::Fast,
            I2cFrequency::FastPlus,
        ] {
            let req = I2cSetConfigurationRequest { frequency: freq };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: I2cSetConfigurationRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn i2c_frequency_discriminants_are_stable() {
        assert_eq!(I2cFrequency::Standard as u8, 0);
        assert_eq!(I2cFrequency::Fast as u8, 1);
        assert_eq!(I2cFrequency::FastPlus as u8, 2);
    }

    #[test]
    fn spi_set_configuration_request_max_frequency() {
        let req = SpiSetConfigurationRequest {
            spi_frequency: u32::MAX,
            spi_phase: SpiPhase::CaptureOnFirstTransition,
            spi_polarity: SpiPolarity::IdleLow,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    // SpiPhase and SpiPolarity discriminants must be stable for wire compat
    #[test]
    fn spi_phase_discriminants_are_stable() {
        assert_eq!(
            to_allocvec(&SpiPhase::CaptureOnFirstTransition).unwrap(),
            to_allocvec(&SpiPhase::CaptureOnFirstTransition).unwrap()
        );
        // Different variants must produce different bytes
        assert_ne!(
            to_allocvec(&SpiPhase::CaptureOnFirstTransition).unwrap(),
            to_allocvec(&SpiPhase::CaptureOnSecondTransition).unwrap()
        );
    }

    #[test]
    fn spi_polarity_discriminants_are_stable() {
        assert_ne!(
            to_allocvec(&SpiPolarity::IdleLow).unwrap(),
            to_allocvec(&SpiPolarity::IdleHigh).unwrap()
        );
    }

    #[test]
    fn gpio_state_discriminants_are_stable() {
        assert_ne!(
            to_allocvec(&GpioState::Low).unwrap(),
            to_allocvec(&GpioState::High).unwrap()
        );
    }

    // --- GPIO direction/pull tests ---

    #[test]
    fn gpio_direction_round_trip() {
        for dir in [GpioDirection::Input, GpioDirection::Output] {
            let bytes = to_allocvec(&dir).unwrap();
            let decoded: GpioDirection = from_bytes(&bytes).unwrap();
            assert_eq!(dir, decoded);
        }
    }

    #[test]
    fn gpio_pull_round_trip() {
        for pull in [GpioPull::None, GpioPull::Up, GpioPull::Down] {
            let bytes = to_allocvec(&pull).unwrap();
            let decoded: GpioPull = from_bytes(&bytes).unwrap();
            assert_eq!(pull, decoded);
        }
    }

    #[test]
    fn gpio_set_configuration_request_round_trip() {
        let req = GpioSetConfigurationRequest {
            pin: 3,
            direction: GpioDirection::Input,
            pull: GpioPull::Up,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: GpioSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn gpio_set_configuration_request_all_combinations() {
        for dir in [GpioDirection::Input, GpioDirection::Output] {
            for pull in [GpioPull::None, GpioPull::Up, GpioPull::Down] {
                let req = GpioSetConfigurationRequest {
                    pin: 0,
                    direction: dir,
                    pull,
                };
                let bytes = to_allocvec(&req).unwrap();
                let decoded: GpioSetConfigurationRequest = from_bytes(&bytes).unwrap();
                assert_eq!(req, decoded);
            }
        }
    }

    #[test]
    fn gpio_direction_discriminants_are_stable() {
        assert_eq!(GpioDirection::Input as u8, 0);
        assert_eq!(GpioDirection::Output as u8, 1);
    }

    #[test]
    fn gpio_pull_discriminants_are_stable() {
        assert_eq!(GpioPull::None as u8, 0);
        assert_eq!(GpioPull::Up as u8, 1);
        assert_eq!(GpioPull::Down as u8, 2);
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn gpio_direction_golden_bytes() {
        // Lock exact wire encoding: Input=0x00, Output=0x01
        assert_eq!(to_allocvec(&GpioDirection::Input).unwrap(), vec![0x00]);
        assert_eq!(to_allocvec(&GpioDirection::Output).unwrap(), vec![0x01]);
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn gpio_pull_golden_bytes() {
        // Lock exact wire encoding: None=0x00, Up=0x01, Down=0x02
        assert_eq!(to_allocvec(&GpioPull::None).unwrap(), vec![0x00]);
        assert_eq!(to_allocvec(&GpioPull::Up).unwrap(), vec![0x01]);
        assert_eq!(to_allocvec(&GpioPull::Down).unwrap(), vec![0x02]);
    }

    #[test]
    fn gpio_set_configuration_request_wire_stability() {
        let req = GpioSetConfigurationRequest {
            pin: 5,
            direction: GpioDirection::Output,
            pull: GpioPull::None,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: GpioSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    // --- Config query round-trip tests ---

    #[test]
    fn spi_configuration_info_round_trip() {
        let info = SpiConfigurationInfo {
            spi_frequency: 1_000_000,
            spi_phase: SpiPhase::CaptureOnFirstTransition,
            spi_polarity: SpiPolarity::IdleLow,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: SpiConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn spi_configuration_info_all_variants() {
        let info = SpiConfigurationInfo {
            spi_frequency: 25_000_000,
            spi_phase: SpiPhase::CaptureOnSecondTransition,
            spi_polarity: SpiPolarity::IdleHigh,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: SpiConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn spi_configuration_info_golden_bytes() {
        // freq=1_000_000 (varint), phase=0, polarity=0
        let info = SpiConfigurationInfo {
            spi_frequency: 1_000_000,
            spi_phase: SpiPhase::CaptureOnFirstTransition,
            spi_polarity: SpiPolarity::IdleLow,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: SpiConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, info);
        // phase and polarity are varint-0
        assert_eq!(bytes[bytes.len() - 2], 0x00); // phase
        assert_eq!(bytes[bytes.len() - 1], 0x00); // polarity
    }

    #[test]
    fn i2c_frequency_round_trip_all_variants() {
        for freq in [
            I2cFrequency::Standard,
            I2cFrequency::Fast,
            I2cFrequency::FastPlus,
        ] {
            let bytes = to_allocvec(&freq).unwrap();
            let decoded: I2cFrequency = from_bytes(&bytes).unwrap();
            assert_eq!(freq, decoded);
        }
    }

    // --- UART round-trip tests ---

    #[test]
    fn uart_read_request_round_trip() {
        let req = UartReadRequest {
            count: 64,
            timeout_ms: 1000,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_read_request_zero_timeout() {
        let req = UartReadRequest {
            count: 10,
            timeout_ms: 0,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_read_request_max_count() {
        let req = UartReadRequest {
            count: u16::MAX,
            timeout_ms: 5000,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_write_request_round_trip() {
        let data = [0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let req = UartWriteRequest { contents: &data };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_write_request_empty_contents() {
        let req = UartWriteRequest { contents: &[] };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_set_configuration_request_round_trip() {
        let req = UartSetConfigurationRequest {
            baud_rate: 115_200,
            data_bits: UartDataBits::Seven,
            parity: UartParity::Mark,
            stop_bits: UartStopBits::Two,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_set_configuration_request_common_baud_rates() {
        for baud in [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600] {
            let req = UartSetConfigurationRequest {
                baud_rate: baud,
                data_bits: UartDataBits::Eight,
                parity: UartParity::None,
                stop_bits: UartStopBits::One,
            };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: UartSetConfigurationRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn uart_configuration_info_round_trip() {
        let info = UartConfigurationInfo {
            baud_rate: 115_200,
            data_bits: UartDataBits::Eight,
            parity: UartParity::None,
            stop_bits: UartStopBits::One,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: UartConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    // --- UART framing enum ABI tests ---
    //
    // These follow the `i2c_error_*` house pattern: a `const` table of every
    // variant, an exhaustive-match witness so appending a variant stops
    // compiling until the table is extended, and index/distinctness checks
    // over the whole table rather than just its ends. Pinning only the first
    // and last variant would let `Six`/`Seven`, `Odd`/`Even` or `Mark`/`Space`
    // be swapped with the tests still green — a deployed wire break with no
    // build-time warning. See AGENTS.md §6.1.

    /// Compile-time exhaustiveness tripwire for [`UartDataBits`].
    fn uart_data_bits_variant_index_witness(v: UartDataBits) -> u8 {
        match v {
            UartDataBits::Five => 0,
            UartDataBits::Six => 1,
            UartDataBits::Seven => 2,
            UartDataBits::Eight => 3,
        }
    }

    /// Compile-time exhaustiveness tripwire for [`UartParity`].
    fn uart_parity_variant_index_witness(v: UartParity) -> u8 {
        match v {
            UartParity::None => 0,
            UartParity::Odd => 1,
            UartParity::Even => 2,
            UartParity::Mark => 3,
            UartParity::Space => 4,
        }
    }

    /// Compile-time exhaustiveness tripwire for [`UartStopBits`].
    fn uart_stop_bits_variant_index_witness(v: UartStopBits) -> u8 {
        match v {
            UartStopBits::One => 0,
            UartStopBits::Two => 1,
        }
    }

    const UART_DATA_BITS_VARIANTS: [UartDataBits; 4] = [
        UartDataBits::Five,
        UartDataBits::Six,
        UartDataBits::Seven,
        UartDataBits::Eight,
    ];

    const UART_PARITY_VARIANTS: [UartParity; 5] = [
        UartParity::None,
        UartParity::Odd,
        UartParity::Even,
        UartParity::Mark,
        UartParity::Space,
    ];

    const UART_STOP_BITS_VARIANTS: [UartStopBits; 2] = [UartStopBits::One, UartStopBits::Two];

    #[test]
    fn uart_data_bits_round_trip() {
        for v in UART_DATA_BITS_VARIANTS {
            let bytes = to_allocvec(&v).unwrap();
            assert_eq!(from_bytes::<UartDataBits>(&bytes).unwrap(), v);
        }
    }

    #[test]
    fn uart_parity_round_trip() {
        for v in UART_PARITY_VARIANTS {
            let bytes = to_allocvec(&v).unwrap();
            assert_eq!(from_bytes::<UartParity>(&bytes).unwrap(), v);
        }
    }

    #[test]
    fn uart_stop_bits_round_trip() {
        for v in UART_STOP_BITS_VARIANTS {
            let bytes = to_allocvec(&v).unwrap();
            assert_eq!(from_bytes::<UartStopBits>(&bytes).unwrap(), v);
        }
    }

    #[test]
    fn uart_data_bits_variant_indices_are_stable() {
        // These values are wire ABI. This test must NOT be updated to
        // accommodate an insertion, a swap, or a deletion. `variant as u8`
        // is additionally the `UARTLCR_H.WLEN` encoding the firmware casts
        // into, so a reorder is a hardware misconfiguration too.
        for (index, variant) in UART_DATA_BITS_VARIANTS.iter().copied().enumerate() {
            let n = u8::try_from(index).unwrap();
            assert_eq!(to_allocvec(&variant).unwrap().as_slice(), &[n][..]);
            assert_eq!(from_bytes::<UartDataBits>(&[n]).unwrap(), variant);
            assert_eq!(uart_data_bits_variant_index_witness(variant), n);
            assert_eq!(variant as u8, n, "WLEN cast assumption broken");
        }
    }

    #[test]
    fn uart_parity_variant_indices_are_stable() {
        for (index, variant) in UART_PARITY_VARIANTS.iter().copied().enumerate() {
            let n = u8::try_from(index).unwrap();
            assert_eq!(to_allocvec(&variant).unwrap().as_slice(), &[n][..]);
            assert_eq!(from_bytes::<UartParity>(&[n]).unwrap(), variant);
            assert_eq!(uart_parity_variant_index_witness(variant), n);
        }
    }

    #[test]
    fn uart_stop_bits_variant_indices_are_stable() {
        for (index, variant) in UART_STOP_BITS_VARIANTS.iter().copied().enumerate() {
            let n = u8::try_from(index).unwrap();
            assert_eq!(to_allocvec(&variant).unwrap().as_slice(), &[n][..]);
            assert_eq!(from_bytes::<UartStopBits>(&[n]).unwrap(), variant);
            assert_eq!(uart_stop_bits_variant_index_witness(variant), n);
        }
    }

    #[test]
    fn uart_framing_encodings_are_distinct() {
        for (i, a) in UART_DATA_BITS_VARIANTS.iter().enumerate() {
            for (j, b) in UART_DATA_BITS_VARIANTS.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    to_allocvec(a).unwrap(),
                    to_allocvec(b).unwrap(),
                    "UartDataBits variants {i} and {j} share an encoding"
                );
            }
        }
        for (i, a) in UART_PARITY_VARIANTS.iter().enumerate() {
            for (j, b) in UART_PARITY_VARIANTS.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    to_allocvec(a).unwrap(),
                    to_allocvec(b).unwrap(),
                    "UartParity variants {i} and {j} share an encoding"
                );
            }
        }
        for (i, a) in UART_STOP_BITS_VARIANTS.iter().enumerate() {
            for (j, b) in UART_STOP_BITS_VARIANTS.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    to_allocvec(a).unwrap(),
                    to_allocvec(b).unwrap(),
                    "UartStopBits variants {i} and {j} share an encoding"
                );
            }
        }
    }

    /// The three enums are sized to the PL011's register fields, not to any
    /// consumer's numbering. Pinning the cardinality makes a future
    /// "helpful" `Nine` or `OneAndHalf` — neither representable in `WLEN` or
    /// `STP2` — a test failure rather than a value the firmware cannot apply.
    #[test]
    fn uart_framing_enums_match_the_hardware_envelope() {
        assert_eq!(UART_DATA_BITS_VARIANTS.len(), 4, "WLEN is two bits wide");
        assert_eq!(UART_PARITY_VARIANTS.len(), 5, "PEN/EPS/SPS give five modes");
        assert_eq!(UART_STOP_BITS_VARIANTS.len(), 2, "STP2 is a single bit");
    }

    /// `UartDataBits`' discriminants are deliberately the RP2350 `UARTLCR_H.WLEN`
    /// encoding, so the firmware maps with a cast rather than a table. If this
    /// ever stops holding, `apply_framing` in the firmware must gain a match.
    #[test]
    fn uart_data_bits_discriminants_are_the_wlen_encoding() {
        assert_eq!(UartDataBits::Five as u8, 0b00);
        assert_eq!(UartDataBits::Six as u8, 0b01);
        assert_eq!(UartDataBits::Seven as u8, 0b10);
        assert_eq!(UartDataBits::Eight as u8, 0b11);
    }

    /// An index one past the last defined variant must be refused rather than
    /// coerced. This is the local form of "a newer firmware appended a variant
    /// and an older host received it". If a variant is ever appended, the
    /// first probe must move to the new first-unused index, not be deleted.
    #[test]
    fn uart_framing_rejects_unknown_variant_indices() {
        assert!(from_bytes::<UartDataBits>(&[4]).is_err());
        assert!(from_bytes::<UartDataBits>(&[0xFF]).is_err());
        assert!(from_bytes::<UartParity>(&[5]).is_err());
        assert!(from_bytes::<UartParity>(&[0xFF]).is_err());
        assert!(from_bytes::<UartStopBits>(&[2]).is_err());
        assert!(from_bytes::<UartStopBits>(&[0xFF]).is_err());
    }

    /// A valid baud rate must not rescue a request whose framing byte is out
    /// of range: the whole request has to fail rather than decode the baud
    /// and leave the framing at some default.
    #[test]
    fn uart_set_configuration_request_rejects_out_of_range_framing() {
        // 115200 varint, then data_bits = 4 (one past Eight).
        assert!(
            from_bytes::<UartSetConfigurationRequest>(&[0x80, 0x84, 0x07, 0x04, 0x00, 0x00])
                .is_err()
        );
        // parity = 5 (one past Space).
        assert!(
            from_bytes::<UartSetConfigurationRequest>(&[0x80, 0x84, 0x07, 0x03, 0x05, 0x00])
                .is_err()
        );
        // stop_bits = 2 (one past Two).
        assert!(
            from_bytes::<UartSetConfigurationRequest>(&[0x80, 0x84, 0x07, 0x03, 0x00, 0x02])
                .is_err()
        );
    }

    /// The old one-field encoding is a prefix of the new one. Decoding it as
    /// the new shape must fail rather than synthesise framing from nothing —
    /// this is the firmware/host skew scenario in miniature, with no hardware.
    #[test]
    fn uart_config_types_refuse_the_old_truncated_encoding() {
        // What the pre-framing request encoded for 115200: baud varint only.
        let old = [0x80u8, 0x84, 0x07];
        assert!(from_bytes::<UartSetConfigurationRequest>(&old).is_err());
        assert!(from_bytes::<UartConfigurationInfo>(&old).is_err());
    }

    /// The old one-field request encoded to 3 bytes for 115200. The new one
    /// appends three single-byte enum indices. This pins the encoding so a
    /// field reorder or rename is caught here rather than on a board.
    ///
    /// Two pins, deliberately. The 8N1 case documents the power-on default,
    /// but its parity and stop bytes are both `0x00`, so swapping those two
    /// field declarations would be invisible to it. The second case uses
    /// three *pairwise distinct* framing indices, so any swap of any two of
    /// the three framing fields changes the encoding. Note that a triple like
    /// 7E2 would not do: `Seven` and `Even` are both index 2, so a
    /// data_bits/parity swap would still encode identically.
    #[test]
    fn uart_set_configuration_request_encoding_is_pinned() {
        // The baud varint is postcard's LEB128: 7-bit groups, low group
        // first, high bit set on every group but the last.
        //
        //   115200 = 7*128^2 + 4*128 + 0
        //          = 0b111_0000100_0000000
        //
        // so the groups low-to-high are 0, 4, 7, emitted as
        // 0x00|0x80 = 0x80, then 0x04|0x80 = 0x84, then 0x07 (last, no
        // continuation bit).
        //
        // Then WLEN = Eight = 3, parity = None = 0, stop = One = 0.
        let default_8n1 = UartSetConfigurationRequest {
            baud_rate: 115_200,
            data_bits: UartDataBits::Eight,
            parity: UartParity::None,
            stop_bits: UartStopBits::One,
        };
        assert_eq!(
            to_allocvec(&default_8n1).unwrap(),
            [0x80, 0x84, 0x07, 0x03, 0x00, 0x00]
        );

        // 7M2: Seven = 2, Mark = 3, Two = 1. Pairwise distinct, so no
        // permutation of the three framing fields yields these bytes.
        let seven_mark_two = UartSetConfigurationRequest {
            baud_rate: 115_200,
            data_bits: UartDataBits::Seven,
            parity: UartParity::Mark,
            stop_bits: UartStopBits::Two,
        };
        assert_eq!(
            to_allocvec(&seven_mark_two).unwrap(),
            [0x80, 0x84, 0x07, 0x02, 0x03, 0x01]
        );
    }

    /// Same reasoning as the request pin, for the response payload: the
    /// framing indices are pairwise distinct so a field reorder cannot hide.
    #[test]
    fn uart_configuration_info_encoding_is_pinned() {
        // 7M2: Seven = 2, Mark = 3, Two = 1.
        let info = UartConfigurationInfo {
            baud_rate: 115_200,
            data_bits: UartDataBits::Seven,
            parity: UartParity::Mark,
            stop_bits: UartStopBits::Two,
        };
        assert_eq!(
            to_allocvec(&info).unwrap(),
            [0x80, 0x84, 0x07, 0x02, 0x03, 0x01]
        );
    }

    #[test]
    fn uart_error_variants_round_trip() {
        for err in [
            UartError::BufferTooLong,
            UartError::Overrun,
            UartError::Break,
            UartError::Parity,
            UartError::Framing,
            UartError::InvalidBaudRate,
            UartError::Other,
            UartError::Unsupported,
        ] {
            let bytes = to_allocvec(&err).unwrap();
            let decoded: UartError = from_bytes(&bytes).unwrap();
            assert_eq!(err, decoded);
        }
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn uart_error_display() {
        assert_eq!(
            format!("{}", UartError::BufferTooLong),
            "buffer exceeds firmware limit"
        );
        assert_eq!(format!("{}", UartError::Overrun), "UART receiver overrun");
        assert_eq!(format!("{}", UartError::Break), "UART break condition");
        assert_eq!(format!("{}", UartError::Parity), "UART parity error");
        assert_eq!(format!("{}", UartError::Framing), "UART framing error");
        assert_eq!(
            format!("{}", UartError::InvalidBaudRate),
            "invalid baud rate"
        );
        assert_eq!(format!("{}", UartError::Other), "UART error");
        assert_eq!(
            format!("{}", UartError::Unsupported),
            "UART not supported on this hardware"
        );
    }

    #[test]
    fn uart_read_request_wire_stability() {
        let req = UartReadRequest {
            count: 64,
            timeout_ms: 1000,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: UartReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn uart_set_configuration_request_wire_stability() {
        // Non-default framing in every field, so re-encoding a decoded value
        // has something to get wrong.
        let req = UartSetConfigurationRequest {
            baud_rate: 115_200,
            data_bits: UartDataBits::Five,
            parity: UartParity::Mark,
            stop_bits: UartStopBits::Two,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: UartSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn uart_configuration_info_wire_stability() {
        let info = UartConfigurationInfo {
            baud_rate: 115_200,
            data_bits: UartDataBits::Five,
            parity: UartParity::Mark,
            stop_bits: UartStopBits::Two,
        };
        let bytes = to_allocvec(&info).unwrap();
        let canonical = bytes.clone();
        let decoded: UartConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, info);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    // --- PWM round-trip tests ---

    #[test]
    fn pwm_set_duty_cycle_request_round_trip() {
        let req = PwmSetDutyCycleRequest {
            channel: 2,
            duty: 32768,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmSetDutyCycleRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_set_duty_cycle_request_zero_duty() {
        let req = PwmSetDutyCycleRequest {
            channel: 0,
            duty: 0,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmSetDutyCycleRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_set_duty_cycle_request_max_duty() {
        let req = PwmSetDutyCycleRequest {
            channel: 3,
            duty: u16::MAX,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmSetDutyCycleRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_get_duty_cycle_request_round_trip() {
        let req = PwmGetDutyCycleRequest { channel: 1 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmGetDutyCycleRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_duty_cycle_info_round_trip() {
        let info = PwmDutyCycleInfo {
            max_duty: 65535,
            current_duty: 32768,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: PwmDutyCycleInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn pwm_enable_request_round_trip() {
        let req = PwmEnableRequest { channel: 0 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmEnableRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_disable_request_round_trip() {
        let req = PwmDisableRequest { channel: 3 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmDisableRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_set_configuration_request_round_trip() {
        let req = PwmSetConfigurationRequest {
            channel: 1,
            frequency_hz: 1_000,
            phase_correct: false,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_set_configuration_request_phase_correct() {
        let req = PwmSetConfigurationRequest {
            channel: 2,
            frequency_hz: 50,
            phase_correct: true,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_set_configuration_request_common_frequencies() {
        for freq in [50, 100, 1_000, 10_000, 50_000, 100_000, 1_000_000] {
            let req = PwmSetConfigurationRequest {
                channel: 0,
                frequency_hz: freq,
                phase_correct: false,
            };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: PwmSetConfigurationRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn pwm_get_configuration_request_round_trip() {
        let req = PwmGetConfigurationRequest { channel: 3 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: PwmGetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn pwm_configuration_info_round_trip() {
        let info = PwmConfigurationInfo {
            frequency_hz: 1_000,
            phase_correct: false,
            enabled: true,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: PwmConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn pwm_error_variants_round_trip() {
        for err in [
            PwmError::InvalidChannel,
            PwmError::InvalidDutyCycle,
            PwmError::InvalidConfiguration,
            PwmError::Other,
        ] {
            let bytes = to_allocvec(&err).unwrap();
            let decoded: PwmError = from_bytes(&bytes).unwrap();
            assert_eq!(err, decoded);
        }
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn pwm_error_display() {
        assert_eq!(
            format!("{}", PwmError::InvalidChannel),
            "invalid PWM channel"
        );
        assert_eq!(
            format!("{}", PwmError::InvalidDutyCycle),
            "duty cycle exceeds maximum"
        );
        assert_eq!(
            format!("{}", PwmError::InvalidConfiguration),
            "invalid PWM configuration"
        );
        assert_eq!(format!("{}", PwmError::Other), "PWM error");
    }

    #[test]
    fn pwm_set_duty_cycle_request_wire_stability() {
        let req = PwmSetDutyCycleRequest {
            channel: 1,
            duty: 1000,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: PwmSetDutyCycleRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn pwm_set_configuration_request_wire_stability() {
        let req = PwmSetConfigurationRequest {
            channel: 0,
            frequency_hz: 1_000,
            phase_correct: false,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: PwmSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn pwm_configuration_info_wire_stability() {
        let info = PwmConfigurationInfo {
            frequency_hz: 50_000,
            phase_correct: true,
            enabled: true,
        };
        let bytes = to_allocvec(&info).unwrap();
        let canonical = bytes.clone();
        let decoded: PwmConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, info);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    // --- ADC tests ---

    #[test]
    fn adc_read_request_round_trip() {
        let req = AdcReadRequest {
            channel: AdcChannel::Adc2,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: AdcReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn adc_channel_display() {
        assert_eq!(format!("{}", AdcChannel::Adc0), "ADC0 (GPIO26)");
        assert_eq!(format!("{}", AdcChannel::Adc1), "ADC1 (GPIO27)");
        assert_eq!(format!("{}", AdcChannel::Adc2), "ADC2 (GPIO28)");
        assert_eq!(format!("{}", AdcChannel::Adc3), "ADC3 (GPIO29)");
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn adc_error_display() {
        assert_eq!(
            format!("{}", AdcError::ConversionFailed),
            "ADC conversion failed"
        );
        assert_eq!(format!("{}", AdcError::Other), "ADC error");
        assert_eq!(
            format!("{}", AdcError::Unsupported),
            "ADC not supported on this hardware"
        );
    }

    #[test]
    fn adc_configuration_info_round_trip() {
        let info = AdcConfigurationInfo {
            resolution_bits: 12,
            nominal_reference_mv: 3300,
            num_gpio_channels: 4,
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: AdcConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn adc_read_request_wire_stability() {
        let req = AdcReadRequest {
            channel: AdcChannel::Adc1,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: AdcReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn adc_configuration_info_wire_stability() {
        let info = AdcConfigurationInfo {
            resolution_bits: 12,
            nominal_reference_mv: 3300,
            num_gpio_channels: 4,
        };
        let bytes = to_allocvec(&info).unwrap();
        let canonical = bytes.clone();
        let decoded: AdcConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, info);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn adc_channel_discriminants_are_stable() {
        // Verify enum discriminants match expected wire values
        assert_eq!(to_allocvec(&AdcChannel::Adc0).unwrap(), [0]);
        assert_eq!(to_allocvec(&AdcChannel::Adc1).unwrap(), [1]);
        assert_eq!(to_allocvec(&AdcChannel::Adc2).unwrap(), [2]);
        assert_eq!(to_allocvec(&AdcChannel::Adc3).unwrap(), [3]);
    }

    // --- GPIO event monitoring tests ---

    #[test]
    fn gpio_edge_round_trip() {
        for edge in [GpioEdge::Rising, GpioEdge::Falling, GpioEdge::Any] {
            let bytes = to_allocvec(&edge).unwrap();
            let decoded: GpioEdge = from_bytes(&bytes).unwrap();
            assert_eq!(edge, decoded);
        }
    }

    #[test]
    fn gpio_edge_discriminants_are_stable() {
        assert_eq!(GpioEdge::Rising as u8, 0);
        assert_eq!(GpioEdge::Falling as u8, 1);
        assert_eq!(GpioEdge::Any as u8, 2);
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn gpio_edge_display() {
        assert_eq!(format!("{}", GpioEdge::Rising), "rising");
        assert_eq!(format!("{}", GpioEdge::Falling), "falling");
        assert_eq!(format!("{}", GpioEdge::Any), "any");
    }

    #[test]
    fn gpio_event_round_trip() {
        let event = GpioEvent {
            pin: 2,
            edge: GpioEdge::Rising,
            state: GpioState::High,
            timestamp_us: 123_456_789,
        };
        let bytes = to_allocvec(&event).unwrap();
        let decoded: GpioEvent = from_bytes(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn gpio_event_all_edge_variants() {
        for edge in [GpioEdge::Rising, GpioEdge::Falling, GpioEdge::Any] {
            for state in [GpioState::Low, GpioState::High] {
                let event = GpioEvent {
                    pin: 0,
                    edge,
                    state,
                    timestamp_us: 0,
                };
                let bytes = to_allocvec(&event).unwrap();
                let decoded: GpioEvent = from_bytes(&bytes).unwrap();
                assert_eq!(event, decoded);
            }
        }
    }

    #[test]
    fn gpio_event_wire_stability() {
        let event = GpioEvent {
            pin: 1,
            edge: GpioEdge::Falling,
            state: GpioState::Low,
            timestamp_us: 999_999,
        };
        let bytes = to_allocvec(&event).unwrap();
        let canonical = bytes.clone();
        let decoded: GpioEvent = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn gpio_subscribe_request_round_trip() {
        for edge in [GpioEdge::Rising, GpioEdge::Falling, GpioEdge::Any] {
            let req = GpioSubscribeRequest { pin: 3, edge };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: GpioSubscribeRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn gpio_unsubscribe_request_round_trip() {
        for pin in 0..4u8 {
            let req = GpioUnsubscribeRequest { pin };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: GpioUnsubscribeRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn gpio_subscribe_request_wire_stability() {
        let req = GpioSubscribeRequest {
            pin: 2,
            edge: GpioEdge::Any,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: GpioSubscribeRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    // --- Transaction Batching Tests ---

    #[test]
    fn i2c_batch_op_round_trip() {
        let ops = [
            I2cBatchOp::Read { len: 16 },
            I2cBatchOp::Write {
                data: &[0xAA, 0xBB],
            },
        ];
        for op in &ops {
            let bytes = to_allocvec(op).unwrap();
            let decoded: I2cBatchOp = from_bytes(&bytes).unwrap();
            assert_eq!(&decoded, op);
        }
    }

    #[test]
    fn spi_batch_op_round_trip() {
        let ops = [
            SpiBatchOp::Read { len: 8 },
            SpiBatchOp::Write {
                data: &[0x01, 0x02],
            },
            SpiBatchOp::Transfer { data: &[0xFF; 4] },
            SpiBatchOp::DelayNs { ns: 1_000_000 },
        ];
        for op in &ops {
            let bytes = to_allocvec(op).unwrap();
            let decoded: SpiBatchOp = from_bytes(&bytes).unwrap();
            assert_eq!(&decoded, op);
        }
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn i2c_batch_ops_take_from_bytes() {
        let ops = [
            I2cBatchOp::Write {
                data: &[0xAA, 0xBB],
            },
            I2cBatchOp::Read { len: 4 },
        ];
        let encoded = encode_i2c_batch_ops(&ops);
        let mut remaining: &[u8] = &encoded;
        let mut decoded = Vec::new();
        while !remaining.is_empty() {
            let (op, rest) = postcard::take_from_bytes::<I2cBatchOp>(remaining).unwrap();
            decoded.push(op);
            remaining = rest;
        }
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0], ops[0]);
        assert_eq!(decoded[1], ops[1]);
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn spi_batch_ops_take_from_bytes() {
        let ops = [
            SpiBatchOp::Write {
                data: &[0x01, 0x02],
            },
            SpiBatchOp::Read { len: 8 },
            SpiBatchOp::Transfer { data: &[0xFF; 4] },
            SpiBatchOp::DelayNs { ns: 1000 },
        ];
        let encoded = encode_spi_batch_ops(&ops);
        let mut remaining: &[u8] = &encoded;
        let mut decoded = Vec::new();
        while !remaining.is_empty() {
            let (op, rest) = postcard::take_from_bytes::<SpiBatchOp>(remaining).unwrap();
            decoded.push(op);
            remaining = rest;
        }
        assert_eq!(decoded.len(), 4);
        for (d, o) in decoded.iter().zip(ops.iter()) {
            assert_eq!(d, o);
        }
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn i2c_batch_request_round_trip() {
        let ops = encode_i2c_batch_ops(&[
            I2cBatchOp::Write {
                data: &[0xAA, 0xBB],
            },
            I2cBatchOp::Read { len: 4 },
        ]);
        let req = I2cBatchRequest {
            address: 0x50,
            count: 2,
            ops: &ops,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: I2cBatchRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn spi_batch_request_round_trip() {
        let ops = encode_spi_batch_ops(&[
            SpiBatchOp::Write {
                data: &[0x01, 0x02],
            },
            SpiBatchOp::Read { len: 8 },
            SpiBatchOp::Transfer { data: &[0xFF; 4] },
            SpiBatchOp::DelayNs { ns: 1000 },
        ]);
        let req = SpiBatchRequest {
            cs_pin: 2,
            count: 4,
            ops: &ops,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: SpiBatchRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn i2c_batch_error_round_trip() {
        let err = I2cBatchError {
            failed_op: 3,
            kind: I2cError::NoAcknowledge,
        };
        let bytes = to_allocvec(&err).unwrap();
        let decoded: I2cBatchError = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, err);
    }

    #[test]
    fn spi_batch_error_round_trip() {
        let err = SpiBatchError {
            failed_op: 1,
            kind: SpiError::Other,
        };
        let bytes = to_allocvec(&err).unwrap();
        let decoded: SpiBatchError = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, err);
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn encode_i2c_empty_batch() {
        let ops = encode_i2c_batch_ops(&[]);
        assert!(ops.is_empty());
        assert_eq!(i2c_batch_response_len(&[]), 0);
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn encode_spi_empty_batch() {
        let ops = encode_spi_batch_ops(&[]);
        assert!(ops.is_empty());
        assert_eq!(spi_batch_response_len(&[]), 0);
    }

    #[test]
    fn i2c_batch_response_len_mixed() {
        let ops = [
            I2cBatchOp::Write {
                data: &[0x00, 0x10],
            },
            I2cBatchOp::Read { len: 16 },
            I2cBatchOp::Write {
                data: &[0x00, 0x20],
            },
            I2cBatchOp::Read { len: 32 },
        ];
        assert_eq!(i2c_batch_response_len(&ops), 48);
    }

    #[test]
    fn spi_batch_response_len_mixed() {
        let ops = [
            SpiBatchOp::Write {
                data: &[0x01, 0x02],
            },
            SpiBatchOp::Read { len: 8 },
            SpiBatchOp::Transfer { data: &[0xFF; 4] },
            SpiBatchOp::DelayNs { ns: 1000 },
        ];
        // Read(8) + Transfer(4) = 12
        assert_eq!(spi_batch_response_len(&ops), 12);
    }

    #[cfg(feature = "use-std")]
    #[test]
    #[should_panic(expected = "too many batch operations")]
    fn encode_i2c_batch_ops_panics_over_limit() {
        let ops: Vec<I2cBatchOp> = (0..MAX_BATCH_OPS + 1)
            .map(|_| I2cBatchOp::Read { len: 1 })
            .collect();
        encode_i2c_batch_ops(&ops);
    }

    #[cfg(feature = "use-std")]
    #[test]
    #[should_panic(expected = "too many batch operations")]
    fn encode_spi_batch_ops_panics_over_limit() {
        let ops: Vec<SpiBatchOp> = (0..MAX_BATCH_OPS + 1)
            .map(|_| SpiBatchOp::Read { len: 1 })
            .collect();
        encode_spi_batch_ops(&ops);
    }

    /// Regression for #176: a single op must encode regardless of size.
    ///
    /// The encoder used to serialize each op through a fixed scratch buffer,
    /// so an op larger than that buffer aborted the host process with
    /// `SerializeBufferFull`. The buffer was 128 bytes, then 1024 — giving
    /// cliffs at 125 and 1021 bytes of payload respectively.
    ///
    /// The sizes below are deliberate. 1022 is the first payload that the
    /// 1024-byte buffer rejected, and [`MAX_TRANSFER_SIZE`] is past any
    /// scratch buffer this function has ever had. The previous version of this
    /// test pinned 256 bytes, which passed under *both* historical buffers and
    /// so could never have caught either cliff.
    #[cfg(feature = "use-std")]
    #[test]
    fn encode_i2c_batch_ops_encodes_ops_of_any_size() {
        for len in [1021, 1022, MAX_TRANSFER_SIZE] {
            let data = vec![0xA5u8; len];
            let op = I2cBatchOp::Write { data: &data };
            let encoded = encode_i2c_batch_ops(core::slice::from_ref(&op));
            let (decoded, rest) = postcard::take_from_bytes::<I2cBatchOp>(&encoded).unwrap();
            assert!(rest.is_empty(), "trailing bytes for len {len}");
            assert_eq!(decoded, op, "round-trip mismatch for len {len}");
        }
    }

    /// Regression for #176. See [`encode_i2c_batch_ops_encodes_ops_of_any_size`].
    #[cfg(feature = "use-std")]
    #[test]
    fn encode_spi_batch_ops_encodes_ops_of_any_size() {
        for len in [1021, 1022, MAX_TRANSFER_SIZE] {
            let data = vec![0xA5u8; len];
            let op = SpiBatchOp::Write { data: &data };
            let encoded = encode_spi_batch_ops(core::slice::from_ref(&op));
            let (decoded, rest) = postcard::take_from_bytes::<SpiBatchOp>(&encoded).unwrap();
            assert!(rest.is_empty(), "trailing bytes for len {len}");
            assert_eq!(decoded, op, "round-trip mismatch for len {len}");
        }
    }

    /// Regression for #176: several oversized ops in one batch must all
    /// concatenate correctly.
    ///
    /// Encoding now writes straight into the accumulating buffer rather than
    /// copying each op out of a scratch array, so this pins that successive
    /// ops still land back-to-back and decode in order.
    #[cfg(feature = "use-std")]
    #[test]
    fn encode_i2c_batch_ops_concatenates_multiple_large_ops() {
        let first = vec![0x11u8; 2000];
        let second = vec![0x22u8; 3000];
        let ops = [
            I2cBatchOp::Write { data: &first },
            I2cBatchOp::Read { len: 7 },
            I2cBatchOp::Write { data: &second },
        ];

        let encoded = encode_i2c_batch_ops(&ops);

        let mut rest: &[u8] = &encoded;
        for (i, expected) in ops.iter().enumerate() {
            let (decoded, tail) = postcard::take_from_bytes::<I2cBatchOp>(rest).unwrap();
            assert_eq!(decoded, *expected, "mismatch at op {i}");
            rest = tail;
        }
        assert!(rest.is_empty());
    }

    /// Regression for #176. See
    /// [`encode_i2c_batch_ops_concatenates_multiple_large_ops`].
    #[cfg(feature = "use-std")]
    #[test]
    fn encode_spi_batch_ops_concatenates_multiple_large_ops() {
        let first = vec![0x11u8; 2000];
        let second = vec![0x22u8; 3000];
        let ops = [
            SpiBatchOp::Write { data: &first },
            SpiBatchOp::Transfer { data: &second },
            SpiBatchOp::DelayNs { ns: 1234 },
        ];

        let encoded = encode_spi_batch_ops(&ops);

        let mut rest: &[u8] = &encoded;
        for (i, expected) in ops.iter().enumerate() {
            let (decoded, tail) = postcard::take_from_bytes::<SpiBatchOp>(rest).unwrap();
            assert_eq!(decoded, *expected, "mismatch at op {i}");
            rest = tail;
        }
        assert!(rest.is_empty());
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn i2c_batch_ops_at_max_limit() {
        let ops: Vec<I2cBatchOp> = (0..MAX_BATCH_OPS)
            .map(|_| I2cBatchOp::Read { len: 1 })
            .collect();
        let encoded = encode_i2c_batch_ops(&ops);
        // Verify we can decode all ops back
        let mut remaining: &[u8] = &encoded;
        let mut count = 0;
        while !remaining.is_empty() {
            let (_, rest) = postcard::take_from_bytes::<I2cBatchOp>(remaining).unwrap();
            remaining = rest;
            count += 1;
        }
        assert_eq!(count, MAX_BATCH_OPS);
        assert_eq!(i2c_batch_response_len(&ops), MAX_BATCH_OPS);
    }

    #[cfg(feature = "use-std")]
    #[test]
    fn spi_batch_ops_at_max_limit() {
        let ops: Vec<SpiBatchOp> = (0..MAX_BATCH_OPS)
            .map(|_| SpiBatchOp::Read { len: 1 })
            .collect();
        let encoded = encode_spi_batch_ops(&ops);
        let mut remaining: &[u8] = &encoded;
        let mut count = 0;
        while !remaining.is_empty() {
            let (_, rest) = postcard::take_from_bytes::<SpiBatchOp>(remaining).unwrap();
            remaining = rest;
            count += 1;
        }
        assert_eq!(count, MAX_BATCH_OPS);
        assert_eq!(spi_batch_response_len(&ops), MAX_BATCH_OPS);
    }

    #[test]
    fn i2c_batch_write_only_response_len() {
        let ops = [
            I2cBatchOp::Write {
                data: &[0x00, 0x10],
            },
            I2cBatchOp::Write { data: &[0xFF; 32] },
        ];
        // No reads → zero response bytes
        assert_eq!(i2c_batch_response_len(&ops), 0);
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn i2c_batch_error_display() {
        let err = I2cBatchError {
            failed_op: 2,
            kind: I2cError::Bus,
        };
        assert_eq!(
            format!("{err}"),
            "I2C batch operation 2 failed: I2C bus error"
        );
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn spi_batch_error_display() {
        let err = SpiBatchError {
            failed_op: 0,
            kind: SpiError::Other,
        };
        assert_eq!(format!("{err}"), "SPI batch operation 0 failed: SPI error");
    }

    // --- 1-Wire round-trip tests ---

    #[test]
    fn onewire_read_request_round_trip() {
        let req = OneWireReadRequest { len: 9 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: OneWireReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn onewire_write_request_round_trip() {
        let data = [0xCC, 0x44];
        let req = OneWireWriteRequest { data: &data };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: OneWireWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn onewire_write_pullup_request_round_trip() {
        let data = [0xCC, 0x44];
        let req = OneWireWritePullupRequest {
            data: &data,
            pullup_duration_ms: 750,
        };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: OneWireWritePullupRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn onewire_error_variants_round_trip() {
        for err in [
            OneWireError::NoPresence,
            OneWireError::BusError,
            OneWireError::BufferTooLong,
            OneWireError::Other,
            OneWireError::Unsupported,
        ] {
            let bytes = to_allocvec(&err).unwrap();
            let decoded: OneWireError = from_bytes(&bytes).unwrap();
            assert_eq!(err, decoded);
        }
    }

    #[test]
    #[cfg(feature = "use-std")]
    fn onewire_error_display() {
        assert_eq!(
            format!("{}", OneWireError::NoPresence),
            "no device present on 1-Wire bus"
        );
        assert_eq!(format!("{}", OneWireError::BusError), "1-Wire bus error");
        assert_eq!(
            format!("{}", OneWireError::BufferTooLong),
            "buffer exceeds firmware limit"
        );
        assert_eq!(format!("{}", OneWireError::Other), "1-Wire error");
        assert_eq!(
            format!("{}", OneWireError::Unsupported),
            "1-Wire not supported on this hardware"
        );
    }

    #[test]
    fn onewire_error_discriminants_are_stable() {
        assert_eq!(to_allocvec(&OneWireError::NoPresence).unwrap(), [0]);
        assert_eq!(to_allocvec(&OneWireError::BusError).unwrap(), [1]);
        assert_eq!(to_allocvec(&OneWireError::BufferTooLong).unwrap(), [2]);
        assert_eq!(to_allocvec(&OneWireError::Other).unwrap(), [3]);
        assert_eq!(to_allocvec(&OneWireError::Unsupported).unwrap(), [4]);
    }

    #[test]
    fn onewire_read_request_wire_stability() {
        let req = OneWireReadRequest { len: 9 };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: OneWireReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn onewire_write_pullup_request_wire_stability() {
        let data = [0xCC, 0x44];
        let req = OneWireWritePullupRequest {
            data: &data,
            pullup_duration_ms: 750,
        };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: OneWireWritePullupRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn onewire_read_request_zero_len() {
        let req = OneWireReadRequest { len: 0 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: OneWireReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn onewire_read_request_max_len() {
        let req = OneWireReadRequest { len: u16::MAX };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: OneWireReadRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn onewire_write_request_empty() {
        let req = OneWireWriteRequest { data: &[] };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: OneWireWriteRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn onewire_search_response_some_round_trip() {
        let resp: OneWireSearchResponse = Ok(Some(0x0028_FF12_3456_7800));
        let bytes = to_allocvec(&resp).unwrap();
        let decoded: OneWireSearchResponse = from_bytes(&bytes).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn onewire_search_response_none_round_trip() {
        let resp: OneWireSearchResponse = Ok(None);
        let bytes = to_allocvec(&resp).unwrap();
        let decoded: OneWireSearchResponse = from_bytes(&bytes).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn onewire_reset_response_round_trip() {
        for present in [true, false] {
            let resp: OneWireResetResponse = Ok(present);
            let bytes = to_allocvec(&resp).unwrap();
            let decoded: OneWireResetResponse = from_bytes(&bytes).unwrap();
            assert_eq!(resp, decoded);
        }
    }

    // --- Logic Capture Tests ---

    // --- DeviceInfo / Capabilities round-trip tests ---

    #[test]
    fn capabilities_bitflag_basics() {
        let caps = Capabilities::I2C | Capabilities::SPI;
        assert!(caps.contains(Capabilities::I2C));
        assert!(caps.contains(Capabilities::SPI));
        assert!(!caps.contains(Capabilities::UART));
        assert!(!caps.contains(Capabilities::GPIO));
        assert_eq!(caps.bits(), 0b11);
    }

    #[test]
    fn capabilities_none_is_zero() {
        assert_eq!(Capabilities::NONE.bits(), 0);
        assert!(!Capabilities::NONE.contains(Capabilities::I2C));
    }

    #[test]
    fn capabilities_round_trip() {
        let caps = Capabilities::I2C
            | Capabilities::SPI
            | Capabilities::GPIO
            | Capabilities::PWM
            | Capabilities::ADC;
        let bytes = to_allocvec(&caps).unwrap();
        let decoded: Capabilities = from_bytes(&bytes).unwrap();
        assert_eq!(caps, decoded);
    }

    #[test]
    fn device_info_round_trip() {
        let info = DeviceInfo {
            fw_major: 0,
            fw_minor: 8,
            fw_patch: 0,
            schema_major: 0,
            schema_minor: 4,
            schema_patch: 0,
            hw_version: 1,
            capabilities: Capabilities::I2C
                | Capabilities::SPI
                | Capabilities::UART
                | Capabilities::GPIO
                | Capabilities::PWM
                | Capabilities::ADC
                | Capabilities::ONEWIRE,
            num_gpios: NUM_GPIOS as u8,
            build_id: heapless::String::new(),
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn device_info_no_capabilities_round_trip() {
        let info = DeviceInfo {
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            schema_major: 1,
            schema_minor: 0,
            schema_patch: 0,
            hw_version: 2,
            capabilities: Capabilities::NONE,
            num_gpios: NUM_GPIOS as u8,
            build_id: heapless::String::new(),
        };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
    }

    #[test]
    fn device_info_round_trip_carries_build_id() {
        // Sweep the three shapes the firmware can actually produce: a full
        // describe string, the git-unavailable fallback, and empty (which a
        // buggy build.rs could emit and which must still decode).
        for id in [
            "firmware-v0.11.0-27-g8ddb1da5a681-dirty",
            "unknown",
            "",
            // Exactly BUILD_ID_CAPACITY bytes.
            "0123456789012345678901234567890123456789012345678901234567890123",
        ] {
            let info = DeviceInfo {
                fw_major: 0,
                fw_minor: 11,
                fw_patch: 0,
                schema_major: 0,
                schema_minor: 7,
                schema_patch: 0,
                hw_version: 2,
                capabilities: Capabilities::I2C,
                num_gpios: NUM_GPIOS as u8,
                build_id: heapless::String::try_from(id).unwrap(),
            };
            let bytes = to_allocvec(&info).unwrap();
            let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
            assert_eq!(info, decoded, "build_id {id:?} must round-trip");
            assert_eq!(decoded.build_id(), id);
        }
    }

    #[test]
    fn build_id_capacity_is_sixty_four() {
        // Pinned because the firmware build script hardcodes the same number
        // for truncation and cannot import this constant.
        assert_eq!(BUILD_ID_CAPACITY, 64);
    }

    #[test]
    fn device_info_rejects_overlong_build_id() {
        // heapless deserialization errors rather than truncating, so an
        // over-long id is a decode failure, not silent data loss. This is why
        // the firmware build script must do the truncating.
        #[derive(Serialize)]
        struct OverlongDeviceInfo {
            fw_major: u16,
            fw_minor: u16,
            fw_patch: u32,
            schema_major: u16,
            schema_minor: u16,
            schema_patch: u32,
            hw_version: u8,
            capabilities: Capabilities,
            num_gpios: u8,
            build_id: &'static str,
        }

        let overlong = OverlongDeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 4,
            schema_minor: 5,
            schema_patch: 6,
            hw_version: 7,
            capabilities: Capabilities(8),
            num_gpios: 9,
            // 65 bytes: one past BUILD_ID_CAPACITY.
            build_id: "01234567890123456789012345678901234567890123456789012345678901234",
        };
        let bytes = to_allocvec(&overlong).unwrap();
        assert!(from_bytes::<DeviceInfo>(&bytes).is_err());
    }

    /// Mirrors the pre-`num_gpios` eight-field [`DeviceInfo`] shape so the
    /// tests below can encode and decode what an older peer would put on
    /// the wire. It deliberately reuses the real [`Capabilities`] type so
    /// the mirror cannot drift from the production definition.
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct LegacyDeviceInfo {
        fw_major: u16,
        fw_minor: u16,
        fw_patch: u32,
        schema_major: u16,
        schema_minor: u16,
        schema_patch: u32,
        hw_version: u8,
        capabilities: Capabilities,
    }

    /// Mirrors the pre-chip-select two-variant [`SpiError`] shape, i.e. what
    /// an older decoder is able to accept.
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum LegacySpiError {
        BufferTooLong,
        Other,
    }

    #[test]
    fn device_info_legacy_bytes_fail_new_shape_decode() {
        let legacy = LegacyDeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 4,
            schema_minor: 5,
            schema_patch: 6,
            hw_version: 7,
            capabilities: Capabilities(8),
        };
        let bytes = to_allocvec(&legacy).unwrap();
        let err = from_bytes::<DeviceInfo>(&bytes).unwrap_err();
        assert_eq!(err, postcard::Error::DeserializeUnexpectedEnd);
    }

    #[test]
    fn device_info_legacy_shape_decode_leaves_num_gpios_trailing() {
        let info = DeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 4,
            schema_minor: 5,
            schema_patch: 6,
            hw_version: 7,
            capabilities: Capabilities(8),
            num_gpios: 9,
            build_id: heapless::String::new(),
        };
        let bytes = to_allocvec(&info).unwrap();
        let (legacy, remaining) = take_from_bytes::<LegacyDeviceInfo>(&bytes).unwrap();
        assert_eq!(legacy.fw_major, 1);
        assert_eq!(legacy.fw_minor, 2);
        assert_eq!(legacy.fw_patch, 3);
        assert_eq!(legacy.schema_major, 4);
        assert_eq!(legacy.schema_minor, 5);
        assert_eq!(legacy.schema_patch, 6);
        assert_eq!(legacy.hw_version, 7);
        assert_eq!(legacy.capabilities, Capabilities(8));
        // `num_gpios`, then the one-byte length of the empty `build_id`.
        assert_eq!(remaining, &[9u8, 0]);
    }

    #[test]
    fn spi_error_chip_select_indices_rejected_by_legacy_decode() {
        // `is_err()` rather than a specific postcard error variant: postcard
        // routes out-of-range enum discriminants through
        // `serde::de::Error::custom`, which is an implementation detail.
        for byte in [2u8, 3u8, 4u8] {
            assert!(from_bytes::<LegacySpiError>(&[byte]).is_err());
        }
    }

    #[test]
    fn num_gpios_fits_in_u8() {
        assert!(u8::try_from(NUM_GPIOS).is_ok());
    }

    #[test]
    fn device_info_encodes_fields_in_declared_order() {
        // A plain round-trip is self-consistent by construction: it would
        // still pass if a field were never serialized at all. This test pins
        // the actual byte image, which is what proves each field is genuinely
        // on the wire, in the declared position. Every numeric value is below
        // 128 so each postcard varint occupies exactly one byte.
        //
        // `build_id` is last, and postcard encodes a string as a varint length
        // followed by UTF-8 bytes -- `heapless`'s Serialize impl is
        // `serialize_str`, so neither N nor LenT appears on the wire.
        let info = DeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 4,
            schema_minor: 5,
            schema_patch: 6,
            hw_version: 7,
            capabilities: Capabilities(8),
            num_gpios: 9,
            build_id: heapless::String::try_from("ab").unwrap(),
        };
        let bytes = to_allocvec(&info).unwrap();
        assert_eq!(
            bytes.as_slice(),
            &[1u8, 2, 3, 4, 5, 6, 7, 8, 9, 2, b'a', b'b'][..]
        );
        assert_eq!(bytes.len(), 12);
        let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);

        // An empty build_id costs exactly one byte (the zero length).
        let empty = DeviceInfo {
            build_id: heapless::String::new(),
            ..info
        };
        let bytes = to_allocvec(&empty).unwrap();
        assert_eq!(bytes.as_slice(), &[1u8, 2, 3, 4, 5, 6, 7, 8, 9, 0][..]);
    }

    #[test]
    fn device_info_num_gpios_round_trips_full_u8_range() {
        // A fixture that only ever uses `NUM_GPIOS` cannot distinguish
        // "serialized the field" from "hardcoded the constant", so sweep
        // values on both sides of the varint boundary.
        for n in [0u8, 1, NUM_GPIOS as u8, 127, 128, 254, 255] {
            let info = DeviceInfo {
                fw_major: 1,
                fw_minor: 2,
                fw_patch: 3,
                schema_major: 4,
                schema_minor: 5,
                schema_patch: 6,
                hw_version: 7,
                capabilities: Capabilities(8),
                num_gpios: n,
                // Empty, so it contributes exactly one trailing zero byte and
                // `num_gpios` remains the second-to-last byte on the wire.
                build_id: heapless::String::new(),
            };
            let bytes = to_allocvec(&info).unwrap();
            assert_eq!(bytes.len(), 10, "unexpected length for num_gpios {n}");
            assert_eq!(*bytes.last().unwrap(), 0);
            assert_eq!(bytes[bytes.len() - 2], n);
            let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
            assert_eq!(decoded.num_gpios, n);
        }
    }

    // --- Deliverable response ceiling (issue #179) ---
    //
    // These pin the two inputs to `MAX_RESPONSE_PAYLOAD`'s derivation that
    // live in *this* crate's own protocol shape, so a future endpoint
    // addition or response-type change cannot move the ceiling silently.

    /// Pins the reply key length the firmware dispatcher will choose.
    ///
    /// `define_dispatch!` sets `Dispatch::min_key_len()` from
    /// `min_key_needed` over this crate's endpoint and topic lists, and
    /// `Sender::reply` shrinks every reply key to it. Two bytes is one of
    /// the terms in [`MAX_RESPONSE_PAYLOAD`]; adding endpoints until the
    /// hash space needs four bytes would cost two payload bytes.
    #[cfg(feature = "use-std")]
    #[test]
    fn reply_key_length_is_two_bytes() {
        use postcard_rpc::Key;
        use postcard_rpc::server::min_key_needed;

        let ep_in: Vec<Key> = ENDPOINT_LIST.endpoints.iter().map(|e| e.1).collect();
        let ep_out: Vec<Key> = ENDPOINT_LIST.endpoints.iter().map(|e| e.2).collect();
        let tp_in: Vec<Key> = TOPICS_IN_LIST.topics.iter().map(|t| t.1).collect();
        let tp_out: Vec<Key> = TOPICS_OUT_LIST.topics.iter().map(|t| t.1).collect();

        // `define_dispatch!` takes the larger of the two directions.
        let needed = min_key_needed(&[&ep_in, &tp_in]).max(min_key_needed(&[&ep_out, &tp_out]));
        assert_eq!(
            needed, 2,
            "the dispatcher's minimum key length changed, so the response \
             header is no longer 7 bytes and MAX_RESPONSE_PAYLOAD's \
             derivation is stale. Re-derive it (see the constant's docs) \
             rather than adjusting this number."
        );
    }

    /// Pins the encoded size of the response header postcard-rpc actually
    /// emits: one discriminant byte, a two-byte key, and a four-byte
    /// sequence number.
    #[test]
    fn response_header_encodes_to_seven_bytes() {
        use postcard_rpc::header::{VarHeader, VarKey, VarKeyKind, VarSeq};

        // Exactly what `Sender::reply` does: start from the 8-byte key and
        // shrink to the dispatcher's minimum.
        let mut key = VarKey::Key8(Version::RESP_KEY);
        key.shrink_to(VarKeyKind::Key2);
        let hdr = VarHeader {
            key,
            // What `HostClient::send_resp` emits and the server echoes back.
            seq_no: VarSeq::Seq4(0x1234_5678),
        };
        let mut buf = [0u8; 32];
        let (used, _) = hdr.write_to_slice(&mut buf).expect("header must encode");
        assert_eq!(
            used.len(),
            RESPONSE_HEADER_LEN,
            "the postcard-rpc header encoding changed; MAX_RESPONSE_PAYLOAD's \
             derivation is stale"
        );
    }

    /// The whole point of the constant: a maximal `Ok` payload must encode
    /// to *exactly* the transport's IN-transfer budget, and one more byte
    /// must overflow it.
    #[cfg(feature = "use-std")]
    #[test]
    fn max_response_payload_exactly_fills_the_transport_budget() {
        let ok: Result<Vec<u8>, I2cError> = Ok(vec![0u8; MAX_RESPONSE_PAYLOAD]);
        assert_eq!(
            RESPONSE_HEADER_LEN + to_allocvec(&ok).unwrap().len(),
            HOST_IN_TRANSFER_BUDGET,
            "a maximal response no longer fills the transport budget exactly"
        );

        let over: Result<Vec<u8>, I2cError> = Ok(vec![0u8; MAX_RESPONSE_PAYLOAD + 1]);
        assert_eq!(
            RESPONSE_HEADER_LEN + to_allocvec(&over).unwrap().len(),
            HOST_IN_TRANSFER_BUDGET + 1,
            "one byte over the ceiling must overflow the budget by exactly one"
        );
    }

    /// The batch response types share the `Result<_, E>` shape the ceiling
    /// is derived from, so the same number bounds them.
    #[cfg(feature = "use-std")]
    #[test]
    fn batch_responses_share_the_read_response_framing() {
        let payload = vec![0u8; MAX_RESPONSE_PAYLOAD];
        let spi: SpiBatchResponse = Ok(payload.clone());
        let i2c: I2cBatchResponse = Ok(payload.clone());
        let read: SpiReadResponse = Ok(payload);
        let n = to_allocvec(&read).unwrap().len();
        assert_eq!(to_allocvec(&spi).unwrap().len(), n);
        assert_eq!(to_allocvec(&i2c).unwrap().len(), n);
    }

    /// Ties the derivation to the hardware measurement.
    ///
    /// The empirical edge is solid — 1014 good / 1015 bad, reproduced on two
    /// boards across four endpoints (issues #158 and #179) — so if the
    /// arithmetic above ever stops landing on it, the model is wrong, not
    /// the measurement. Fail loudly rather than quietly shipping a ceiling
    /// nobody checked against a bus.
    #[test]
    fn max_response_payload_matches_the_measured_edge() {
        assert_eq!(
            MAX_RESPONSE_PAYLOAD, 1014,
            "the derived ceiling no longer matches the edge measured on \
             hardware. Re-measure before changing this number: the last good \
             read was 1014 bytes and 1015 failed with \
             Comms(Postcard(DeserializeUnexpectedEnd))."
        );
    }

    // --- Request frame ceiling (issue #186) ---
    //
    // The send-direction mirror of the block above. Where the response
    // terms are all properties of the *host* transport, these are split:
    // the buffer is the firmware's and the header width is the host
    // client's, so both sides get pinned here.

    /// Pins the widest request header postcard-rpc actually emits.
    ///
    /// The counterpart of `response_header_encodes_to_seven_bytes`, but
    /// with the key left at its initial `Key8` rather than shrunk: that is
    /// the state `HostClient` is in until it has received a reply, and the
    /// state a bound has to assume because it cannot observe `kkind`.
    #[test]
    fn request_header_encodes_to_thirteen_bytes() {
        use postcard_rpc::header::{VarHeader, VarKey, VarSeq};

        let hdr = VarHeader {
            // No `shrink_to`: this is `HostClient`'s starting width.
            key: VarKey::Key8(Version::REQ_KEY),
            seq_no: VarSeq::Seq4(0x1234_5678),
        };
        let mut buf = [0u8; 32];
        let (used, _) = hdr.write_to_slice(&mut buf).expect("header must encode");
        assert_eq!(
            used.len(),
            REQUEST_HEADER_LEN_MAX,
            "the postcard-rpc request header encoding changed; \
             MAX_REQUEST_FRAME's derivation is stale"
        );
    }

    /// A warm request header is six bytes narrower, which is the whole
    /// reason the bound assumes the cold one.
    ///
    /// If this difference ever went to zero the conservatism would be free
    /// and the doc comments explaining the trade would be wrong.
    #[test]
    fn a_warm_request_header_is_six_bytes_narrower() {
        use postcard_rpc::header::{VarHeader, VarKey, VarKeyKind, VarSeq};

        let mut key = VarKey::Key8(Version::REQ_KEY);
        key.shrink_to(VarKeyKind::Key2);
        let hdr = VarHeader {
            key,
            seq_no: VarSeq::Seq4(0x1234_5678),
        };
        let mut buf = [0u8; 32];
        let (used, _) = hdr.write_to_slice(&mut buf).expect("header must encode");
        assert_eq!(REQUEST_HEADER_LEN_MAX - used.len(), 6);
    }

    /// Pins `varint_len` against postcard itself, at every width boundary
    /// a batch can reach.
    ///
    /// The arithmetic in `i2c_batch_request_frame_len` is only sound while
    /// this helper agrees with the encoder it is modelling.
    #[cfg(feature = "use-std")]
    #[test]
    fn varint_len_matches_postcard() {
        for n in [
            0usize,
            1,
            127,
            128,
            129,
            16383,
            16384,
            65535,
            MAX_TRANSFER_SIZE,
        ] {
            assert_eq!(
                varint_len(n),
                to_allocvec(&(n as u32)).unwrap().len(),
                "varint_len disagrees with postcard at {n}"
            );
        }
    }

    /// The load-bearing test: the computed frame length must equal what a
    /// real `I2cBatchRequest` actually encodes to, plus the header.
    ///
    /// Computing rather than encoding avoids a second pass over a
    /// five-kilobyte payload, but only if the arithmetic is right. Sweeping
    /// shapes across the varint boundaries is what makes this more than a
    /// restatement of the formula: a wrong length-prefix width shows up at
    /// 127/128 and a wrong `count` width at 64 operations.
    #[cfg(feature = "use-std")]
    #[test]
    fn i2c_batch_request_frame_len_matches_a_real_encoding() {
        let big = vec![0xA5u8; 5000];
        let small = vec![0xA5u8; 127];
        let boundary = vec![0xA5u8; 128];
        let cases: Vec<Vec<I2cBatchOp<'_>>> = vec![
            vec![],
            vec![I2cBatchOp::Read { len: 1 }],
            vec![I2cBatchOp::Read { len: 127 }],
            vec![I2cBatchOp::Read { len: 128 }],
            vec![I2cBatchOp::Read { len: u16::MAX }],
            vec![I2cBatchOp::Write { data: &small }],
            vec![I2cBatchOp::Write { data: &boundary }],
            vec![I2cBatchOp::Write { data: &big }],
            vec![I2cBatchOp::Write { data: &[] }],
            vec![
                I2cBatchOp::Write { data: &boundary },
                I2cBatchOp::Read { len: 300 },
                I2cBatchOp::Write { data: &small },
            ],
            // A maximal operation count, to exercise the `count` varint.
            (0..MAX_BATCH_OPS)
                .map(|_| I2cBatchOp::Read { len: 8 })
                .collect(),
        ];

        for ops in &cases {
            let encoded = encode_i2c_batch_ops(ops);
            let req = I2cBatchRequest {
                address: 0x48,
                count: ops.len() as u16,
                ops: &encoded,
            };
            let actual = REQUEST_HEADER_LEN_MAX + to_allocvec(&req).unwrap().len();
            assert_eq!(
                i2c_batch_request_frame_len(ops),
                actual,
                "frame length mismatch for {} ops, {} encoded bytes",
                ops.len(),
                encoded.len()
            );
        }
    }

    /// The SPI half of the test above, including the two variants I2C does
    /// not have.
    #[cfg(feature = "use-std")]
    #[test]
    fn spi_batch_request_frame_len_matches_a_real_encoding() {
        let big = vec![0xA5u8; 5000];
        let boundary = vec![0xA5u8; 128];
        let cases: Vec<Vec<SpiBatchOp<'_>>> = vec![
            vec![],
            vec![SpiBatchOp::Read { len: 128 }],
            vec![SpiBatchOp::Write { data: &big }],
            vec![SpiBatchOp::Transfer { data: &boundary }],
            // `DelayNs` is a u32: sweep it across three varint widths, since
            // it is the only field wide enough to need more than two bytes.
            vec![SpiBatchOp::DelayNs { ns: 0 }],
            vec![SpiBatchOp::DelayNs { ns: 127 }],
            vec![SpiBatchOp::DelayNs { ns: 16_384 }],
            vec![SpiBatchOp::DelayNs { ns: u32::MAX }],
            vec![
                SpiBatchOp::Write { data: &boundary },
                SpiBatchOp::DelayNs { ns: 1_000_000 },
                SpiBatchOp::Transfer { data: &boundary },
                SpiBatchOp::Read { len: 16 },
            ],
            (0..MAX_BATCH_OPS)
                .map(|_| SpiBatchOp::Read { len: 8 })
                .collect(),
        ];

        for ops in &cases {
            let encoded = encode_spi_batch_ops(ops);
            let req = SpiBatchRequest {
                cs_pin: 0,
                count: ops.len() as u16,
                ops: &encoded,
            };
            let actual = REQUEST_HEADER_LEN_MAX + to_allocvec(&req).unwrap().len();
            assert_eq!(
                spi_batch_request_frame_len(ops),
                actual,
                "frame length mismatch for {} ops, {} encoded bytes",
                ops.len(),
                encoded.len()
            );
        }
    }

    /// Ties the derivation to the hardware measurement, exactly as
    /// `max_response_payload_matches_the_measured_edge` does.
    #[test]
    fn max_request_frame_matches_the_measured_edge() {
        assert_eq!(
            MAX_REQUEST_FRAME, 5119,
            "the derived ceiling no longer matches the edge measured on \
             hardware. Re-measure before changing this number: a 5119-byte \
             i2c/batch frame was executed and answered, and 5120 was dropped \
             with no reply at all."
        );
    }

    /// Reproduces the exact hardware boundary from issue #186 through the
    /// public helper, in the same units the measurement was taken in.
    ///
    /// A 5099-byte single `Write` was accepted on board `49742081C885AC69`
    /// and 5100 was dropped. That is a payload figure; this asserts the
    /// helper turns those two payloads into 5119 and 5120, which is what
    /// makes the constant and the measurement the same claim.
    #[test]
    fn the_measured_i2c_payload_edge_lands_on_the_frame_ceiling() {
        let accepted = [0xA5u8; 5099];
        let dropped = [0xA5u8; 5100];
        assert_eq!(
            i2c_batch_request_frame_len(&[I2cBatchOp::Write { data: &accepted }]),
            MAX_REQUEST_FRAME
        );
        assert_eq!(
            i2c_batch_request_frame_len(&[I2cBatchOp::Write { data: &dropped }]),
            MAX_REQUEST_FRAME + 1
        );
    }

    /// The multi-operation arm of the same measurement: eight writes of 634
    /// bytes were accepted and 635 dropped, which is a different point on
    /// the same ceiling rather than a per-operation limit.
    #[test]
    fn the_measured_multi_op_edge_lands_on_the_same_ceiling() {
        // Fixed-size arrays rather than `Vec`, so this runs without the
        // `use-std` feature too — CI checks each crate on its own, where
        // `alloc` is not in scope (AGENTS.md §13.14).
        let accepted = [0xA5u8; 634];
        let dropped = [0xA5u8; 635];
        let eight_accepted =
            core::array::from_fn::<_, 8, _>(|_| I2cBatchOp::Write { data: &accepted });
        let eight_dropped =
            core::array::from_fn::<_, 8, _>(|_| I2cBatchOp::Write { data: &dropped });
        assert!(i2c_batch_request_frame_len(&eight_accepted) <= MAX_REQUEST_FRAME);
        assert!(i2c_batch_request_frame_len(&eight_dropped) > MAX_REQUEST_FRAME);
    }
}
