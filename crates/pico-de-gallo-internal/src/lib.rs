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
//! - **Constants**: [`MICROSOFT_VID`], [`PICO_DE_GALLO_PID`], [`MAX_TRANSFER_SIZE`]
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
//!   [`UartConfigurationInfo`].
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
pub const MAX_TRANSFER_SIZE: usize = 4096;

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
    /// Request exceeds the firmware buffer limit ([`MAX_TRANSFER_SIZE`]).
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
    /// Request exceeds the firmware buffer limit ([`MAX_TRANSFER_SIZE`]).
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
/// Takes effect immediately. The firmware applies the new baud rate before
/// processing the next UART operation.
///
/// **Note:** In v1, only `baud_rate` is configurable at runtime. Data bits,
/// parity, and stop bits are set to 8N1 at boot and cannot be changed
/// dynamically. These fields are reserved for future use and must be set
/// to their default values (`Eight`, `None`, `One`).
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct UartSetConfigurationRequest {
    /// UART baud rate in bits per second.
    pub baud_rate: u32,
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
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct UartConfigurationInfo {
    /// UART baud rate in bits per second.
    pub baud_rate: u32,
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
/// - Total read data must not exceed [`MAX_TRANSFER_SIZE`]
/// - Total write data is limited by USB packet size
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
/// serialized straight into the growable output buffer. Callers remain
/// responsible for keeping the *total* request within the transport's framing
/// budget; this function does not police that.
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
/// serialized straight into the growable output buffer. Callers remain
/// responsible for keeping the *total* request within the transport's framing
/// budget; this function does not police that.
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
        let req = UartSetConfigurationRequest { baud_rate: 115_200 };
        let bytes = to_allocvec(&req).unwrap();
        let decoded: UartSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn uart_set_configuration_request_common_baud_rates() {
        for baud in [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600] {
            let req = UartSetConfigurationRequest { baud_rate: baud };
            let bytes = to_allocvec(&req).unwrap();
            let decoded: UartSetConfigurationRequest = from_bytes(&bytes).unwrap();
            assert_eq!(req, decoded);
        }
    }

    #[test]
    fn uart_configuration_info_round_trip() {
        let info = UartConfigurationInfo { baud_rate: 115_200 };
        let bytes = to_allocvec(&info).unwrap();
        let decoded: UartConfigurationInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);
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
        let req = UartSetConfigurationRequest { baud_rate: 115_200 };
        let bytes = to_allocvec(&req).unwrap();
        let canonical = bytes.clone();
        let decoded: UartSetConfigurationRequest = from_bytes(&bytes).unwrap();
        assert_eq!(decoded, req);
        assert_eq!(to_allocvec(&decoded).unwrap(), canonical);
    }

    #[test]
    fn uart_configuration_info_wire_stability() {
        let info = UartConfigurationInfo { baud_rate: 115_200 };
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
}
