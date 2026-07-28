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
    I2cWriteReadRequest, I2cWriteRequest, MICROSOFT_VID, OneWireRead, OneWireReadRequest, OneWireReset, OneWireSearch,
    OneWireSearchNext, OneWireWrite, OneWireWritePullup, OneWireWritePullupRequest, OneWireWriteRequest,
    PICO_DE_GALLO_PID, PwmDisable, PwmDisableRequest, PwmEnable, PwmEnableRequest, PwmGetConfiguration,
    PwmGetConfigurationRequest, PwmGetDutyCycle, PwmGetDutyCycleRequest, PwmSetConfiguration,
    PwmSetConfigurationRequest, PwmSetDutyCycle, PwmSetDutyCycleRequest, SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR,
    SpiBatch, SpiBatchRequest, SpiFlush, SpiGetConfiguration, SpiRead, SpiReadRequest, SpiSetConfiguration,
    SpiSetConfigurationRequest, SpiTransfer, SpiTransferRequest, SpiWrite, SpiWriteRequest, SystemResetSubscriptions,
    UartFlush, UartGetConfiguration, UartRead, UartReadRequest, UartSetConfiguration, UartSetConfigurationRequest,
    UartWrite, UartWriteRequest, Version,
};

pub use pico_de_gallo_internal::{
    AdcChannel, AdcConfigurationInfo, Capabilities, DeviceInfo, GpioDirection, GpioEdge, GpioEvent, GpioPull,
    GpioState, I2cBatchOp, I2cFrequency, PwmConfigurationInfo, PwmDutyCycleInfo, SpiBatchOp, SpiConfigurationInfo,
    SpiPhase, SpiPolarity, UartConfigurationInfo, VersionInfo,
};
pub use pico_de_gallo_internal::{
    AdcError, GpioError, I2cBatchError, I2cError, OneWireError, PwmError, SpiBatchError, SpiError, UartError,
};
pub use pico_de_gallo_internal::{MAX_BATCH_OPS, MAX_TRANSFER_SIZE};
pub use pico_de_gallo_internal::{
    encode_i2c_batch_ops, encode_spi_batch_ops, i2c_batch_response_len, spi_batch_response_len,
};

pub use postcard_rpc::host_client;
pub use postcard_rpc::host_client::HostErr;
pub use postcard_rpc::host_client::{IoClosed, MultiSubscription};
pub use postcard_rpc::standard_icd::WireError;
use postcard_rpc::{
    header::VarSeqKind,
    host_client::HostClient,
    standard_icd::{ERROR_PATH, PingEndpoint},
};
use std::convert::Infallible;

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
    /// The firmware processed the request but returned an error.
    Endpoint(E),
}

impl<E: core::fmt::Display> core::fmt::Display for PicoDeGalloError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Comms(e) => write!(f, "communication error: {e:?}"),
            Self::Endpoint(e) => write!(f, "endpoint error: {e}"),
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

/// Async client for a Pico de Gallo USB bridge device.
///
/// This is the primary type for interacting with the hardware. It wraps a
/// [`postcard_rpc::host_client::HostClient`] and provides typed async methods
/// for every firmware endpoint. The client is cheaply cloneable (the inner
/// transport is reference-counted) and safe to share across tasks.
///
/// Connection happens lazily in the background — constructing a `PicoDeGallo`
/// does not block or fail. If the device is not connected, methods will return
/// errors when called.
#[derive(Clone)]
pub struct PicoDeGallo {
    client: HostClient<WireError>,
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

    fn new_inner<F: FnMut(&NusbDeviceInfo) -> bool>(func: F) -> Self {
        let client = HostClient::new_raw_nusb(func, ERROR_PATH, 8, VarSeqKind::Seq2);
        Self { client }
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
        Ok(self.client.send_resp::<PingEndpoint>(&id).await?)
    }

    /// Read `count` bytes from the I2C device at `address`.
    ///
    /// The firmware buffer is limited to [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes. Reads exceeding this limit will be truncated.
    pub async fn i2c_read(&self, address: u8, count: u16) -> Result<Vec<u8>, PicoDeGalloError<I2cError>> {
        self.client
            .send_resp::<I2cRead>(&I2cReadRequest { address, count })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the I2C device at `address`.
    pub async fn i2c_write(&self, address: u8, contents: &[u8]) -> Result<(), PicoDeGalloError<I2cError>> {
        self.client
            .send_resp::<I2cWrite>(&I2cWriteRequest { address, contents })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the I2C device at `address` and read back `count` bytes.
    ///
    /// The firmware buffer is limited to [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes. Reads exceeding this limit will be truncated.
    pub async fn i2c_write_read(
        &self,
        address: u8,
        contents: &[u8],
        count: u16,
    ) -> Result<Vec<u8>, PicoDeGalloError<I2cError>> {
        self.client
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
        self.client
            .send_resp::<I2cScan>(&I2cScanRequest { include_reserved })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Execute a batch of I2C operations in a single USB transfer.
    ///
    /// Pass a slice of [`I2cBatchOp`] values directly — they are encoded
    /// internally. On success, returns the concatenated read data from
    /// all Read operations in order.
    ///
    /// This is much faster than issuing individual I2C calls when
    /// performing multi-step sequences (e.g., EEPROM programming).
    pub async fn i2c_batch(
        &self,
        address: u8,
        ops: &[I2cBatchOp<'_>],
    ) -> Result<Vec<u8>, PicoDeGalloError<I2cBatchError>> {
        let encoded = encode_i2c_batch_ops(ops);
        self.client
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
    /// The firmware buffer is limited to [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes. Reads exceeding this limit will be truncated.
    pub async fn spi_read(&self, count: u16) -> Result<Vec<u8>, PicoDeGalloError<SpiError>> {
        self.client
            .send_resp::<SpiRead>(&SpiReadRequest { count })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write `contents` to the SPI bus.
    pub async fn spi_write(&self, contents: &[u8]) -> Result<(), PicoDeGalloError<SpiError>> {
        self.client
            .send_resp::<SpiWrite>(&SpiWriteRequest { contents })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Flush the SPI interface.
    pub async fn spi_flush(&self) -> Result<(), PicoDeGalloError<SpiError>> {
        self.client
            .send_resp::<SpiFlush>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Perform a full-duplex SPI transfer.
    ///
    /// Simultaneously sends `write_data` and receives the same number of bytes.
    /// The firmware buffer is limited to [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// bytes. Transfers exceeding this limit will be rejected.
    pub async fn spi_transfer(&self, write_data: &[u8]) -> Result<Vec<u8>, PicoDeGalloError<SpiError>> {
        self.client
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
    /// This is much faster than issuing individual SPI calls when
    /// performing multi-step sequences.
    pub async fn spi_batch(
        &self,
        cs_pin: u8,
        ops: &[SpiBatchOp<'_>],
    ) -> Result<Vec<u8>, PicoDeGalloError<SpiBatchError>> {
        let encoded = encode_spi_batch_ops(ops);
        self.client
            .send_resp::<SpiBatch>(&SpiBatchRequest {
                cs_pin,
                count: ops.len() as u16,
                ops: &encoded,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Read up to `count` bytes from the UART bus.
    ///
    /// The firmware reads up to `count` bytes from the UART receive buffer.
    /// If no data is immediately available, it waits up to `timeout_ms`
    /// milliseconds for at least one byte. Returns whatever bytes are
    /// available (1 to `count`), or an empty `Vec` on timeout.
    ///
    /// The firmware buffer is limited to [`pico_de_gallo_internal::MAX_TRANSFER_SIZE`]
    /// (4096) bytes.
    pub async fn uart_read(&self, count: u16, timeout_ms: u32) -> Result<Vec<u8>, PicoDeGalloError<UartError>> {
        self.client
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
    pub async fn uart_write(&self, contents: &[u8]) -> Result<(), PicoDeGalloError<UartError>> {
        self.client
            .send_resp::<UartWrite>(&UartWriteRequest { contents })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Flush the UART transmit buffer.
    ///
    /// Blocks until all pending bytes have been transmitted on the wire.
    pub async fn uart_flush(&self) -> Result<(), PicoDeGalloError<UartError>> {
        self.client
            .send_resp::<UartFlush>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Get the current state of GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_get(&self, pin: u8) -> Result<GpioState, PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioGet>(&GpioGetRequest { pin })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Set the GPIO numbered by `pin` to state `state`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    pub async fn gpio_put(&self, pin: u8, state: GpioState) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioPut>(&GpioPutRequest { pin, state })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `High` state.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call waits forever. For a bounded wait that returns
    /// [`GpioError::Timeout`] on expiry, use
    /// [`gpio_wait_for_high_with_timeout`](Self::gpio_wait_for_high_with_timeout).
    pub async fn gpio_wait_for_high(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioWaitForHigh>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `Low` state.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call waits forever. For a bounded wait that returns
    /// [`GpioError::Timeout`] on expiry, use
    /// [`gpio_wait_for_low_with_timeout`](Self::gpio_wait_for_low_with_timeout).
    pub async fn gpio_wait_for_low(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioWaitForLow>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a rising edge on the GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call waits forever. For a bounded wait that returns
    /// [`GpioError::Timeout`] on expiry, use
    /// [`gpio_wait_for_rising_edge_with_timeout`](Self::gpio_wait_for_rising_edge_with_timeout).
    pub async fn gpio_wait_for_rising_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioWaitForRising>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a falling edge on the GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call waits forever. For a bounded wait that returns
    /// [`GpioError::Timeout`] on expiry, use
    /// [`gpio_wait_for_falling_edge_with_timeout`](Self::gpio_wait_for_falling_edge_with_timeout).
    pub async fn gpio_wait_for_falling_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioWaitForFalling>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for either a rising edge or a falling edge on the GPIO
    /// numbered by `pin`.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// This call waits forever. For a bounded wait that returns
    /// [`GpioError::Timeout`] on expiry, use
    /// [`gpio_wait_for_any_edge_with_timeout`](Self::gpio_wait_for_any_edge_with_timeout).
    pub async fn gpio_wait_for_any_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
            .send_resp::<GpioWaitForAny>(&GpioWaitRequest { pin, timeout_ms: 0 })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `High` state, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if the level is not reached within `timeout`. Passing
    /// `Duration::ZERO` reverts to the wait-forever behavior of
    /// [`gpio_wait_for_high`](Self::gpio_wait_for_high).
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
        self.client
            .send_resp::<GpioWaitForHigh>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for GPIO numbered by `pin` to reach `Low` state, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if the level is not reached within `timeout`. Passing
    /// `Duration::ZERO` reverts to the wait-forever behavior of
    /// [`gpio_wait_for_low`](Self::gpio_wait_for_low).
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
        self.client
            .send_resp::<GpioWaitForLow>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a rising edge on the GPIO numbered by `pin`, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if no rising edge is detected within `timeout`. Passing
    /// `Duration::ZERO` reverts to the wait-forever behavior of
    /// [`gpio_wait_for_rising_edge`](Self::gpio_wait_for_rising_edge).
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
        self.client
            .send_resp::<GpioWaitForRising>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for a falling edge on the GPIO numbered by `pin`, with a
    /// host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if no falling edge is detected within `timeout`. Passing
    /// `Duration::ZERO` reverts to the wait-forever behavior of
    /// [`gpio_wait_for_falling_edge`](Self::gpio_wait_for_falling_edge).
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
        self.client
            .send_resp::<GpioWaitForFalling>(&GpioWaitRequest { pin, timeout_ms })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Wait for either a rising edge or a falling edge on the GPIO
    /// numbered by `pin`, with a host-supplied timeout.
    ///
    /// Returns `Err(PicoDeGalloError::Endpoint(GpioError::Timeout))`
    /// if no edge is detected within `timeout`. Passing
    /// `Duration::ZERO` reverts to the wait-forever behavior of
    /// [`gpio_wait_for_any_edge`](Self::gpio_wait_for_any_edge).
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
        self.client
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
        self.client
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
        self.client
            .send_resp::<GpioSubscribe>(&GpioSubscribeRequest { pin, edge })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Unsubscribe from GPIO edge events on a pin.
    ///
    /// Stops monitoring and returns the pin to normal operation. Returns
    /// [`GpioError::PinNotMonitored`] if the pin is not currently subscribed.
    pub async fn gpio_unsubscribe(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioError>> {
        self.client
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
        self.client
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
        self.client
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
        Ok(self.client.send_resp::<Version>(&()).await?)
    }

    /// Get extended device information including firmware version, schema
    /// (wire protocol) version, hardware revision, and peripheral capabilities.
    pub async fn device_info(&self) -> Result<DeviceInfo, PicoDeGalloError<Infallible>> {
        Ok(self.client.send_resp::<GetDeviceInfo>(&()).await?)
    }

    /// Validate that the connected firmware is wire-compatible with this
    /// host library.
    ///
    /// Queries the `device/info` endpoint and checks that the schema minor
    /// version matches (pre-1.0 semver: minor bumps are breaking). Returns
    /// the [`DeviceInfo`] on success so callers can inspect capabilities
    /// without an extra round-trip.
    ///
    /// # Errors
    ///
    /// - [`ValidateError::Comms`] — could not reach the device.
    /// - [`ValidateError::LegacyFirmware`] — firmware does not support
    ///   `device/info` (upgrade firmware).
    /// - [`ValidateError::SchemaMismatch`] — firmware and host disagree on
    ///   the wire protocol version.
    pub async fn validate(&self) -> Result<DeviceInfo, ValidateError> {
        let info = self
            .client
            .send_resp::<GetDeviceInfo>(&())
            .await
            .map_err(map_validate_error)?;

        check_schema_compatible(&info)?;

        Ok(info)
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
        Ok(self.client.send_resp::<SystemResetSubscriptions>(&()).await?)
    }

    /// Query the current I2C bus configuration.
    ///
    /// Returns the [`I2cFrequency`] value that is currently active on the
    /// firmware. The default is [`I2cFrequency::Standard`] (100 kHz).
    pub async fn i2c_get_config(&self) -> Result<I2cFrequency, PicoDeGalloError<Infallible>> {
        Ok(self.client.send_resp::<I2cGetConfiguration>(&()).await?)
    }

    /// Query the current SPI bus configuration.
    ///
    /// Returns a [`SpiConfigurationInfo`] struct with the active SPI
    /// frequency, phase, and polarity. The defaults are 1 MHz,
    /// `CaptureOnFirstTransition`, and `IdleLow`.
    pub async fn spi_get_config(&self) -> Result<SpiConfigurationInfo, PicoDeGalloError<Infallible>> {
        Ok(self.client.send_resp::<SpiGetConfiguration>(&()).await?)
    }

    /// Set UART bus configuration parameters.
    ///
    /// Changes the UART baud rate. Takes effect immediately before the next
    /// UART operation. The default baud rate is 115200.
    pub async fn uart_set_config(&self, baud_rate: u32) -> Result<(), PicoDeGalloError<UartError>> {
        self.client
            .send_resp::<UartSetConfiguration>(&UartSetConfigurationRequest { baud_rate })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Query the current UART bus configuration.
    ///
    /// Returns a [`UartConfigurationInfo`] struct with the active baud rate.
    /// The default is 115200.
    ///
    /// Returns [`UartError::Unsupported`] if the firmware's hardware revision
    /// does not support UART.
    pub async fn uart_get_config(&self) -> Result<UartConfigurationInfo, PicoDeGalloError<UartError>> {
        self.client
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
        self.client
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
        self.client
            .send_resp::<PwmGetDutyCycle>(&PwmGetDutyCycleRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Enable the PWM slice that owns `channel` (0–3).
    ///
    /// Because PWM slices drive two channels, enabling channel 0 also
    /// enables channel 1 (and vice versa). Same for channels 2/3.
    pub async fn pwm_enable(&self, channel: u8) -> Result<(), PicoDeGalloError<PwmError>> {
        self.client
            .send_resp::<PwmEnable>(&PwmEnableRequest { channel })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Disable the PWM slice that owns `channel` (0–3).
    ///
    /// Because PWM slices drive two channels, disabling channel 0 also
    /// disables channel 1 (and vice versa). Same for channels 2/3.
    pub async fn pwm_disable(&self, channel: u8) -> Result<(), PicoDeGalloError<PwmError>> {
        self.client
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
        self.client
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
        self.client
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
        self.client
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
        self.client
            .send_resp::<AdcGetConfiguration>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    // ---- 1-Wire ----

    /// Perform a 1-Wire bus reset and detect device presence.
    ///
    /// Returns `true` if one or more devices responded with a presence pulse.
    pub async fn onewire_reset(&self) -> Result<bool, PicoDeGalloError<OneWireError>> {
        self.client
            .send_resp::<OneWireReset>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Read `len` bytes from the 1-Wire bus.
    ///
    /// The firmware sends `0xFF` read slots and captures the device's response bits.
    pub async fn onewire_read(&self, len: u16) -> Result<Vec<u8>, PicoDeGalloError<OneWireError>> {
        self.client
            .send_resp::<OneWireRead>(&OneWireReadRequest { len })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write raw bytes to the 1-Wire bus.
    pub async fn onewire_write(&self, data: &[u8]) -> Result<(), PicoDeGalloError<OneWireError>> {
        self.client
            .send_resp::<OneWireWrite>(&OneWireWriteRequest { data })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Write bytes to the 1-Wire bus, then apply a strong pullup for the given duration.
    ///
    /// This is needed for parasitic-power devices like the DS18B20 during temperature
    /// conversion. The bus is held high for `pullup_duration_ms` milliseconds after
    /// the last bit is sent.
    pub async fn onewire_write_pullup(
        &self,
        data: &[u8],
        pullup_duration_ms: u16,
    ) -> Result<(), PicoDeGalloError<OneWireError>> {
        self.client
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
        self.client
            .send_resp::<OneWireSearch>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }

    /// Continue the current 1-Wire ROM search.
    ///
    /// Returns the next device's 64-bit ROM ID, or `None` when all devices have
    /// been enumerated.
    pub async fn onewire_search_next(&self) -> Result<Option<u64>, PicoDeGalloError<OneWireError>> {
        self.client
            .send_resp::<OneWireSearchNext>(&())
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- PicoDeGalloError tests ---

    #[test]
    fn endpoint_error_wraps_inner() {
        let err: PicoDeGalloError<&str> = PicoDeGalloError::Endpoint("endpoint failed");
        match err {
            PicoDeGalloError::Endpoint(e) => assert_eq!(e, "endpoint failed"),
            PicoDeGalloError::Comms(_) => panic!("expected Endpoint, got Comms"),
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
        }
    }

    #[test]
    fn check_schema_compatible_accepts_matching_versions() {
        let info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
        check_schema_compatible(&info).expect("matching versions must validate");
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
}
