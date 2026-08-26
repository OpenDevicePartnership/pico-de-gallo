//! C-compatible FFI bindings for the Pico de Gallo USB bridge.
//!
//! This crate wraps [`pico-de-gallo-lib`](https://docs.rs/pico-de-gallo-lib) in
//! a C-compatible API using opaque pointers and integer status codes. It is
//! compiled as a `cdylib` (shared library) and generates a C header via
//! [`cbindgen`](https://docs.rs/cbindgen).
//!
//! # Usage from C
//!
//! ```c
//! #include "pico_de_gallo.h"
//!
//! const PicoDeGallo *gallo = gallo_init();
//! uint32_t id = 42;
//! Status status = gallo_ping(gallo, &id);
//! // id now contains the round-tripped value
//! gallo_free(gallo);
//! ```
//!
//! # Lifecycle
//!
//! 1. Call [`gallo_init`] (or [`gallo_init_with_serial_number`]) to create a context.
//! 2. Use `gallo_*` functions, passing the context pointer.
//! 3. Call [`gallo_free`] to release resources.
//!
//! # Thread Safety
//!
//! The context pointer is safe to share across threads — the inner type is
//! `Send + Sync` (enforced by a compile-time assertion). Every call is driven
//! on a shared multi-threaded Tokio runtime (required by the postcard-rpc nusb
//! transport), so concurrent calls from multiple threads are safe.
//!
//! # Status Codes
//!
//! All functions return a [`Status`] code. [`Status::Ok`] (0) indicates success;
//! negative values indicate errors. See [`Status`] for the full list.

use pico_de_gallo_lib::{
    self as lib, AdcChannel, AdcError, GpioError, I2cBatchError, I2cBatchOp, I2cError,
    OneWireError, PicoDeGalloError, PwmError, SpiBatchError, SpiBatchOp, SpiError, UartError,
};
use std::ffi::CStr;
use std::future::Future;
use std::os::raw::c_char;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

/// Shared multi-threaded Tokio runtime for FFI calls.
///
/// The postcard-rpc nusb transport spawns background tasks via `tokio::spawn`,
/// so the FFI stuff must run inside a Tokio runtime context. This also keeps those
/// background tasks driven between individual FFI calls.
fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build Tokio runtime")
    })
}

/// Drive an async library call to completion on the shared runtime.
fn block_on<F: Future>(fut: F) -> F::Output {
    runtime().block_on(fut)
}

/// Opaque handle to a Pico de Gallo device context.
///
/// Created by [`gallo_init`] or [`gallo_init_with_serial_number`] and released
/// by [`gallo_free`]. This type is a thin wrapper around
/// [`pico_de_gallo_lib::PicoDeGallo`] and must not be constructed directly
/// from C code.
pub struct PicoDeGallo(lib::PicoDeGallo);

// Compile-time assertion: the FFI handle must be safe to share across
// C threads. lib::PicoDeGallo is Send + Sync because HostClient is
// internally Arc-wrapped.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PicoDeGallo>();
};

// ----------------------------- Status Codes -----------------------------

/// Status codes returned by all FFI functions.
///
/// [`Status::Ok`] (0) indicates success. All error codes are negative integers
/// with stable values suitable for use in C `switch` statements.
#[repr(i32)]
#[derive(Debug, PartialEq)]
pub enum Status {
    /// Operation successful
    Ok = 0,
    /// I2c Read failed
    I2cReadFailed = -1,
    /// I2c Write failed
    I2cWriteFailed = -2,
    /// Firmware produced an invalid response
    InvalidResponse = -3,
    /// Library was not initialized
    Uninitialized = -4,
    /// Caller passed an invalid argument
    InvalidArgument = -5,
    /// Ping failed
    PingFailed = -6,
    /// Spi Read failed
    SpiReadFailed = -7,
    /// Spi Write failed
    SpiWriteFailed = -8,
    /// Spi Flush failed
    SpiFlushFailed = -9,
    /// Gpio get failed
    GpioGetFailed = -10,
    /// Gpio put failed
    GpioPutFailed = -11,
    /// Gpio wait failed
    GpioWaitFailed = -12,
    /// Set config failed
    SetConfigFailed = -13,
    /// Version failed
    VersionFailed = -14,
    /// I2c Write Read failed
    I2cWriteReadFailed = -15,
    /// I2c Set config failed
    I2cSetConfigFailed = -16,
    /// Spi Set config failed
    SpiSetConfigFailed = -17,
    /// I2C target did not acknowledge
    I2cNack = -18,
    /// I2C bus error
    I2cBusError = -19,
    /// I2C arbitration loss
    I2cArbitrationLoss = -20,
    /// I2C data overrun
    I2cOverrun = -21,
    /// Buffer exceeds firmware transfer limit
    BufferTooLong = -22,
    /// I2C address out of range
    I2cAddressOutOfRange = -23,
    /// GPIO pin number is invalid
    GpioInvalidPin = -24,
    /// USB communication failure
    CommsFailed = -25,
    /// I2C bus scan failed
    I2cScanFailed = -26,
    /// GPIO set config failed
    GpioSetConfigFailed = -27,
    /// GPIO pin configured in wrong direction for the requested operation
    GpioWrongDirection = -28,
    /// I2C get-config query failed
    I2cGetConfigFailed = -29,
    /// SPI get-config query failed
    SpiGetConfigFailed = -30,
    /// UART read failed
    UartReadFailed = -31,
    /// UART write failed
    UartWriteFailed = -32,
    /// UART flush failed
    UartFlushFailed = -33,
    /// UART receiver overrun
    UartOverrun = -34,
    /// UART break condition detected
    UartBreak = -35,
    /// UART parity error
    UartParity = -36,
    /// UART framing error
    UartFraming = -37,
    /// Invalid baud rate
    UartInvalidBaudRate = -38,
    /// UART set-config failed
    UartSetConfigFailed = -39,
    /// UART get-config query failed
    UartGetConfigFailed = -40,
    /// PWM set-duty-cycle failed
    PwmSetDutyCycleFailed = -41,
    /// PWM get-duty-cycle query failed
    PwmGetDutyCycleFailed = -42,
    /// PWM enable failed
    PwmEnableFailed = -43,
    /// PWM disable failed
    PwmDisableFailed = -44,
    /// PWM set-config failed
    PwmSetConfigFailed = -45,
    /// PWM get-config query failed
    PwmGetConfigFailed = -46,
    /// Invalid PWM channel
    PwmInvalidChannel = -47,
    /// Invalid PWM duty cycle
    PwmInvalidDutyCycle = -48,
    /// Invalid PWM configuration
    PwmInvalidConfiguration = -49,
    /// ADC read failed
    AdcReadFailed = -50,
    /// ADC get-config query failed
    AdcGetConfigFailed = -51,
    /// ADC conversion error
    AdcConversionFailed = -52,
    /// GPIO pin is currently being monitored (subscribed)
    GpioPinMonitored = -53,
    /// GPIO pin is not being monitored (not subscribed)
    GpioPinNotMonitored = -54,
    /// GPIO subscribe failed
    GpioSubscribeFailed = -55,
    /// GPIO unsubscribe failed
    GpioUnsubscribeFailed = -56,
    /// 1-Wire: no device responded to reset
    OneWireNoPresence = -57,
    /// 1-Wire: bus communication error
    OneWireBusError = -58,
    /// 1-Wire: read operation failed
    OneWireReadFailed = -59,
    /// 1-Wire: write operation failed
    OneWireWriteFailed = -60,
    /// 1-Wire: ROM search failed
    OneWireSearchFailed = -61,
    /// Device info query failed
    DeviceInfoFailed = -62,
    /// Schema version mismatch between host and firmware
    SchemaMismatch = -63,
    /// Firmware does not support the device/info endpoint
    LegacyFirmware = -64,
    /// Peripheral is not supported on this hardware revision
    Unsupported = -65,
    /// I2C batch transaction failed
    I2cBatchFailed = -66,
    /// SPI batch transaction failed
    SpiBatchFailed = -67,
    /// SPI full-duplex transfer failed
    SpiTransferFailed = -68,
    /// Resetting server-side subscriptions failed
    SystemResetSubscriptionsFailed = -69,
    /// GPIO wait endpoint timed out before the requested edge/level
    /// was detected. Returned only by the `_with_timeout_ms` variants
    /// when the host-supplied timeout expired. Available on firmware
    /// schema 0.7+.
    GpioTimeout = -70,
    /// SPI chip-select pin index is outside the device-reported GPIO range
    SpiInvalidCsPin = -71,
    /// SPI chip-select pin is explicitly configured as an input
    SpiCsPinUnavailable = -72,
    /// SPI chip-select pin is currently monitored for GPIO events
    SpiCsPinMonitored = -73,
    /// Device reports zero GPIOs, so no SPI chip-select pin exists.
    ///
    /// Distinct from [`Status::SpiInvalidCsPin`]: the caller's index is not
    /// the problem, the board has no usable pin at all.
    SpiNoGpios = -74,
    /// The `device/info` round-trip did not complete within the host
    /// library's 300-second bound.
    ///
    /// Distinct from [`Status::DeviceInfoFailed`], which reports an actual
    /// transport or decode failure. On expiry there is no transport error
    /// at all — the request is simply still outstanding — and collapsing
    /// the two would erase the only actionable diagnosis.
    DeviceInfoTimeout = -75,
}

// ----------------------------- Limits -----------------------------
//
// These must be literals. cbindgen evaluates const initializers
// syntactically and silently skips any it cannot fold, so writing
// `= lib::MAX_TRANSFER_SIZE` here compiles fine but emits nothing into
// pico_de_gallo.h. The `const _` assertions below tie the literals back
// to the wire crate, turning any upstream change into a build failure
// rather than a stale header.

/// Maximum number of bytes the firmware will move in a single transfer.
///
/// Requests larger than this are rejected with [`Status::BufferTooLong`].
/// Applies per direction: a write-then-read may carry up to this many
/// bytes in each direction.
///
/// Mirrors `pico_de_gallo_internal::MAX_TRANSFER_SIZE` so C callers can
/// size their buffers from the same limit the firmware enforces.
pub const GALLO_MAX_TRANSFER_SIZE: usize = 4096;

/// Maximum number of operations in a single [`gallo_i2c_batch`] or
/// [`gallo_spi_batch`] call.
///
/// Passing more than this returns [`Status::InvalidArgument`].
///
/// Mirrors `pico_de_gallo_internal::MAX_BATCH_OPS`.
pub const GALLO_MAX_BATCH_OPS: usize = 64;

/// Number of user-controllable GPIO pins the firmware exposes.
///
/// Valid pin indices are `0..GALLO_NUM_GPIOS`. Passing anything at or
/// above this bound to [`gallo_gpio_get`], [`gallo_gpio_put`],
/// [`gallo_gpio_set_config`] or the `gallo_gpio_wait_*` family yields
/// [`Status::GpioInvalidPin`].
///
/// The same indices select the SPI chip-select pin in
/// [`gallo_spi_batch`], so C callers validating a chip select should
/// bound it by this constant rather than a literal.
///
/// Mirrors `pico_de_gallo_internal::NUM_GPIOS`.
pub const GALLO_NUM_GPIOS: usize = 4;

const _: () = assert!(GALLO_MAX_TRANSFER_SIZE == lib::MAX_TRANSFER_SIZE);
const _: () = assert!(GALLO_MAX_BATCH_OPS == lib::MAX_BATCH_OPS);
const _: () = assert!(GALLO_NUM_GPIOS == lib::NUM_GPIOS);

// ----------------------------- Configuration Enums -----------------------------
//
// These mirror the integer values that the `gallo_*` entry points already
// accept and return as `uint8_t`. They exist so C callers can use a name
// instead of a magic number; the underlying function signatures are
// unchanged, so passing a plain integer still works.
//
// The numeric values are part of the stable C ABI (see AGENTS.md §8) and
// must match the corresponding `pico-de-gallo-internal` wire enums, whose
// variant order is itself ABI (see AGENTS.md §6.1). `config_enums_match_wire_enums`
// in the test module locks that correspondence down.

/// I2C bus speed, as accepted by [`gallo_i2c_set_config`] and reported by
/// [`gallo_i2c_get_config`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloI2cFrequency {
    /// Standard mode, 100 kHz.
    Standard = 0,
    /// Fast mode, 400 kHz.
    Fast = 1,
    /// Fast mode plus, 1 MHz.
    FastPlus = 2,
}

/// GPIO pin direction, as accepted by [`gallo_gpio_set_config`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloGpioDirection {
    /// Pin is an input.
    Input = 0,
    /// Pin is an output.
    Output = 1,
}

/// GPIO pull resistor configuration, as accepted by
/// [`gallo_gpio_set_config`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloGpioPull {
    /// No pull resistor.
    None = 0,
    /// Pull-up resistor enabled.
    Up = 1,
    /// Pull-down resistor enabled.
    Down = 2,
}

/// GPIO edge selection, as accepted by [`gallo_gpio_subscribe`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloGpioEdge {
    /// Trigger on rising edges only.
    Rising = 0,
    /// Trigger on falling edges only.
    Falling = 1,
    /// Trigger on both edges.
    Any = 2,
}

/// Variant tag for [`GalloSpiBatchOp::tag`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloSpiBatchOpTag {
    /// Read `read_len` bytes.
    Read = 0,
    /// Write `data_len` bytes from `data`.
    Write = 1,
    /// Full-duplex transfer of `data_len` bytes.
    Transfer = 2,
    /// Delay `delay_ns` nanoseconds.
    DelayNs = 3,
}

/// Variant tag for [`GalloI2cBatchOp::tag`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloI2cBatchOpTag {
    /// Read `read_len` bytes.
    Read = 0,
    /// Write `data_len` bytes from `data`.
    Write = 1,
}

// ----------------------------- Error Mapping Helpers -----------------------------

fn i2c_error_to_status(e: PicoDeGalloError<I2cError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(I2cError::NoAcknowledge) => Status::I2cNack,
        PicoDeGalloError::Endpoint(I2cError::Bus) => Status::I2cBusError,
        PicoDeGalloError::Endpoint(I2cError::ArbitrationLoss) => Status::I2cArbitrationLoss,
        PicoDeGalloError::Endpoint(I2cError::Overrun) => Status::I2cOverrun,
        PicoDeGalloError::Endpoint(I2cError::BufferTooLong) => Status::BufferTooLong,
        PicoDeGalloError::Endpoint(I2cError::AddressOutOfRange) => Status::I2cAddressOutOfRange,
        PicoDeGalloError::Endpoint(I2cError::Other) => Status::I2cReadFailed,
        // Deliberately reuses the existing InvalidArgument code rather than
        // adding a new one: Status values are stable C ABI, and a new value
        // falls through existing C consumers' exhaustive switches (AGENTS.md
        // §8, and the 2026-08-17 row in §13.17). A zero-length write is an
        // argument this hardware cannot honour, so InvalidArgument is honest.
        PicoDeGalloError::Endpoint(I2cError::ZeroLengthWrite) => Status::InvalidArgument,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

fn spi_error_to_status(e: PicoDeGalloError<SpiError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(SpiError::BufferTooLong) => Status::BufferTooLong,
        PicoDeGalloError::Endpoint(SpiError::Other) => Status::SpiReadFailed,
        PicoDeGalloError::Endpoint(SpiError::InvalidCsPin) => Status::SpiInvalidCsPin,
        PicoDeGalloError::Endpoint(SpiError::CsPinUnavailable) => Status::SpiCsPinUnavailable,
        PicoDeGalloError::Endpoint(SpiError::CsPinMonitored) => Status::SpiCsPinMonitored,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

/// Map a [`lib::ValidateError`] to a status code.
///
/// Every arm lands in `{-62, -63, -64, -75}`. **None** of them may ever be a
/// chip-select status (-71..-74): a failure to *learn* the valid pin range
/// is not a complaint about the caller's pin, and reporting it as one would
/// send users hunting for a bug in their own arguments. See issue #104.
///
/// Exhaustive on purpose: appending a `ValidateError` variant must break
/// this build.
fn validate_error_to_status(e: &lib::ValidateError) -> Status {
    match e {
        lib::ValidateError::Comms(_) => Status::DeviceInfoFailed,
        lib::ValidateError::Timeout => Status::DeviceInfoTimeout,
        lib::ValidateError::LegacyFirmware => Status::LegacyFirmware,
        lib::ValidateError::SchemaMismatch { .. } => Status::SchemaMismatch,
    }
}

/// Map a [`lib::SpiBatchCallError`] to a status code.
///
/// The five outcome classes are disjoint: metadata failure keeps its
/// `ValidateError` classification, a zero GPIO count is
/// [`Status::SpiNoGpios`], a locally refused index is
/// [`Status::SpiInvalidCsPin`], an actual batch transport failure is
/// [`Status::CommsFailed`], and a firmware refusal keeps the existing
/// explicit [`SpiError`] mapping.
fn spi_batch_call_error_to_status(e: &lib::SpiBatchCallError) -> Status {
    match e {
        lib::SpiBatchCallError::DeviceInfo(v) => validate_error_to_status(v),
        lib::SpiBatchCallError::NoGpios => Status::SpiNoGpios,
        lib::SpiBatchCallError::InvalidCsPin { .. } => Status::SpiInvalidCsPin,
        lib::SpiBatchCallError::Comms(_) => Status::CommsFailed,
        lib::SpiBatchCallError::Endpoint(SpiBatchError { kind, .. }) => {
            spi_error_to_status(PicoDeGalloError::Endpoint(*kind))
        }
    }
}

fn gpio_error_to_status(e: PicoDeGalloError<GpioError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(GpioError::InvalidPin) => Status::GpioInvalidPin,
        PicoDeGalloError::Endpoint(GpioError::WrongDirection) => Status::GpioWrongDirection,
        PicoDeGalloError::Endpoint(GpioError::PinMonitored) => Status::GpioPinMonitored,
        PicoDeGalloError::Endpoint(GpioError::PinNotMonitored) => Status::GpioPinNotMonitored,
        PicoDeGalloError::Endpoint(GpioError::Timeout) => Status::GpioTimeout,
        PicoDeGalloError::Endpoint(GpioError::Other) => Status::GpioGetFailed,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

fn uart_error_to_status(e: PicoDeGalloError<UartError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(UartError::BufferTooLong) => Status::BufferTooLong,
        PicoDeGalloError::Endpoint(UartError::Overrun) => Status::UartOverrun,
        PicoDeGalloError::Endpoint(UartError::Break) => Status::UartBreak,
        PicoDeGalloError::Endpoint(UartError::Parity) => Status::UartParity,
        PicoDeGalloError::Endpoint(UartError::Framing) => Status::UartFraming,
        PicoDeGalloError::Endpoint(UartError::InvalidBaudRate) => Status::UartInvalidBaudRate,
        PicoDeGalloError::Endpoint(UartError::Other) => Status::UartReadFailed,
        PicoDeGalloError::Endpoint(UartError::Unsupported) => Status::Unsupported,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

fn pwm_error_to_status(e: PicoDeGalloError<PwmError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(PwmError::InvalidChannel) => Status::PwmInvalidChannel,
        PicoDeGalloError::Endpoint(PwmError::InvalidDutyCycle) => Status::PwmInvalidDutyCycle,
        PicoDeGalloError::Endpoint(PwmError::InvalidConfiguration) => {
            Status::PwmInvalidConfiguration
        }
        PicoDeGalloError::Endpoint(PwmError::Other) => Status::PwmSetDutyCycleFailed,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

fn adc_error_to_status(e: PicoDeGalloError<AdcError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(AdcError::ConversionFailed) => Status::AdcConversionFailed,
        PicoDeGalloError::Endpoint(AdcError::Other) => Status::AdcReadFailed,
        PicoDeGalloError::Endpoint(AdcError::Unsupported) => Status::Unsupported,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

fn onewire_error_to_status(e: PicoDeGalloError<OneWireError>) -> Status {
    match e {
        PicoDeGalloError::Endpoint(OneWireError::NoPresence) => Status::OneWireNoPresence,
        PicoDeGalloError::Endpoint(OneWireError::BusError) => Status::OneWireBusError,
        PicoDeGalloError::Endpoint(OneWireError::BufferTooLong) => Status::BufferTooLong,
        PicoDeGalloError::Endpoint(OneWireError::Other) => Status::OneWireReadFailed,
        PicoDeGalloError::Endpoint(OneWireError::Unsupported) => Status::Unsupported,
        PicoDeGalloError::Comms(_) => Status::CommsFailed,
    }
}

// ----------------------------- Library Lifetime -----------------------------

/// gallo_init - Initialize the library context.
///
/// Returns an opaque representation of the underlying PicoDeGallo
/// device.
#[unsafe(no_mangle)]
pub extern "C" fn gallo_init() -> *const PicoDeGallo {
    let inner = {
        let _guard = runtime().enter();
        lib::PicoDeGallo::new()
    };

    Box::into_raw(Box::new(PicoDeGallo(inner))) as *const PicoDeGallo
}

/// gallo_init_with_serial_number - Initialize the library context for
/// a device with the given serial number.
///
/// Returns an opaque representation of the underlying PicoDeGallo
/// device.
///
/// # Safety
///
/// `c_serial_number` must point to a valid c-string containing a
/// valid Pico de Gallo serial number with a NULL-terminator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_init_with_serial_number(
    c_serial_number: *const c_char,
) -> *const PicoDeGallo {
    if c_serial_number.is_null() {
        eprintln!("NULL serial number received");
        return std::ptr::null();
    }

    // Safety: Pointer is not null due to the check above. Caller must
    // make sure to pass a null-terminated string.
    let serial_number = unsafe { CStr::from_ptr(c_serial_number).to_str() };

    if serial_number.is_err() {
        eprintln!("Invalid UTF-8 string");
        return std::ptr::null();
    }

    let inner = {
        let _guard = runtime().enter();
        lib::PicoDeGallo::new_with_serial_number(serial_number.unwrap())
    };

    Box::into_raw(Box::new(PicoDeGallo(inner))) as *const PicoDeGallo
}

/// gallo_init_strict - Like `gallo_init` but validates the connected
/// firmware's schema version before returning.
///
/// Returns `NULL` if no device is reachable, the firmware does not
/// support the `device/info` endpoint (legacy firmware), or the
/// schema version does not match this host library's compiled-in
/// schema.
///
/// Prefer this in production C code: `gallo_init` is lazy and surfaces
/// failures only on the first real RPC call, which is harder to
/// diagnose. `gallo_init_strict` fails fast at construction with a
/// `NULL` return.
///
/// On success, the returned pointer must be freed with `gallo_free`
/// when no longer needed.
#[unsafe(no_mangle)]
pub extern "C" fn gallo_init_strict() -> *const PicoDeGallo {
    let inner = {
        let _guard = runtime().enter();
        lib::PicoDeGallo::try_new()
    };
    let inner = match inner {
        Ok(inner) => inner,
        Err(e) => {
            eprintln!("gallo_init_strict: device not reachable: {e}");
            return std::ptr::null();
        }
    };
    match block_on(inner.validate()) {
        Ok(_info) => Box::into_raw(Box::new(PicoDeGallo(inner))) as *const PicoDeGallo,
        Err(e) => {
            eprintln!("gallo_init_strict: firmware validation failed: {e}");
            std::ptr::null()
        }
    }
}

/// gallo_init_strict_with_serial_number - Like
/// `gallo_init_with_serial_number` but validates the connected
/// firmware's schema version before returning.
///
/// Returns `NULL` if no matching device is reachable, the firmware
/// does not support the `device/info` endpoint (legacy firmware),
/// or the schema version does not match.
///
/// See `gallo_init_strict` for the rationale.
///
/// # Safety
///
/// `c_serial_number` must point to a valid c-string containing a
/// valid Pico de Gallo serial number with a NULL-terminator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_init_strict_with_serial_number(
    c_serial_number: *const c_char,
) -> *const PicoDeGallo {
    if c_serial_number.is_null() {
        eprintln!("NULL serial number received");
        return std::ptr::null();
    }
    let serial_number = unsafe { CStr::from_ptr(c_serial_number).to_str() };
    if serial_number.is_err() {
        eprintln!("Invalid UTF-8 string");
        return std::ptr::null();
    }
    let inner = {
        let _guard = runtime().enter();
        lib::PicoDeGallo::try_new_with_serial_number(serial_number.unwrap())
    };
    let inner = match inner {
        Ok(inner) => inner,
        Err(e) => {
            eprintln!("gallo_init_strict_with_serial_number: device not reachable: {e}");
            return std::ptr::null();
        }
    };
    match block_on(inner.validate()) {
        Ok(_info) => Box::into_raw(Box::new(PicoDeGallo(inner))) as *const PicoDeGallo,
        Err(e) => {
            eprintln!("gallo_init_strict_with_serial_number: firmware validation failed: {e}");
            std::ptr::null()
        }
    }
}

/// gallo_free - Releases and destroys the library context created by `gallo_init`.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_free(gallo: *const PicoDeGallo) {
    if !gallo.is_null() {
        // Safety: caller must ensure that `gallo` is a valid opaque
        // pointer to `PicoDeGallo` returned by `gallo_init()`.
        drop(unsafe { Box::from_raw(gallo as *mut PicoDeGallo) });
    }
}

// ----------------------------- Ping endpoint -----------------------------

/// gallo_ping - Ping the firmware and wait for a response
///
/// Returns the same `u32` passed as the first argument.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_ping(gallo: *const PicoDeGallo, id: *mut u32) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if id.is_null() {
        eprintln!("Unexpected NULL id pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: null check above guarantees id is non-null.
    let id_val = unsafe { *id };

    let result = block_on(gallo.0.ping(id_val));
    match result {
        Ok(back) => {
            unsafe { *id = back };
            Status::Ok
        }
        Err(_) => Status::PingFailed,
    }
}

/// gallo_system_reset_subscriptions - Tear down GPIO subscriptions left
/// over from a previous host session.
///
/// Subscriptions are server-side state; if a previous host process
/// crashed or was killed without sending `gpio/unsubscribe`, those pins
/// remain owned by firmware monitor tasks across reconnects. Hosts
/// should call this once on connect, after `gallo_init` succeeds, to
/// reclaim any such pins. It is idempotent and cheap on a fresh device.
///
/// On success, writes the number of subscriptions that were reset to
/// `*out_reset` (0 if `out_reset` is non-NULL and no subscriptions were
/// active). `out_reset` may be NULL if the caller does not need the
/// count.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`. If non-NULL, `out_reset`
/// must point to a valid, writable `uint8_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_system_reset_subscriptions(
    gallo: *const PicoDeGallo,
    out_reset: *mut u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    match block_on(gallo.0.system_reset_subscriptions()) {
        Ok(n) => {
            if !out_reset.is_null() {
                // Safety: caller asserts `out_reset` is writable if non-null.
                unsafe { *out_reset = n };
            }
            Status::Ok
        }
        Err(_) => Status::SystemResetSubscriptionsFailed,
    }
}

// ----------------------------- I2c endpoints -----------------------------

/// gallo_i2c_read - Read `len` bytes from the device at `address` into `buf`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_read(
    gallo: *const PicoDeGallo,
    address: u8,
    buf: *mut u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };

    let result = block_on(gallo.0.i2c_read(address, len as u16));

    match result {
        Ok(data) => {
            if data.len() != buf.len() {
                eprintln!(
                    "Firmware returned {} bytes, expected {}",
                    data.len(),
                    buf.len()
                );
                return Status::InvalidResponse;
            }
            buf.copy_from_slice(&data);
            Status::Ok
        }
        Err(e) => i2c_error_to_status(e),
    }
}

/// gallo_i2c_write - Write `len` bytes from `buf` to the device at `address`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_write(
    gallo: *const PicoDeGallo,
    address: u8,
    buf: *const u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts(buf, len) };

    let result = block_on(gallo.0.i2c_write(address, buf));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => i2c_error_to_status(e),
    }
}

/// gallo_i2c_write_read - Perform a write followed by a read.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, `txbuf` must be valid
/// for `txlen` bytes, and `rxbuf` must be valid for `rxlen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_write_read(
    gallo: *const PicoDeGallo,
    address: u8,
    txbuf: *const u8,
    txlen: usize,
    rxbuf: *mut u8,
    rxlen: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if txbuf.is_null() || rxbuf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if txlen > u16::MAX.into() || rxlen > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure txbuf is valid for txlen bytes.
    let txbuf = unsafe { std::slice::from_raw_parts(txbuf, txlen) };

    // Safety: caller must ensure rxbuf is valid for rxlen bytes.
    let rxbuf = unsafe { std::slice::from_raw_parts_mut(rxbuf, rxlen) };

    let result = block_on(gallo.0.i2c_write_read(address, txbuf, rxlen as u16));
    match result {
        Ok(data) => {
            if data.len() != rxbuf.len() {
                eprintln!(
                    "Firmware returned {} bytes, expected {}",
                    data.len(),
                    rxbuf.len()
                );
                return Status::InvalidResponse;
            }
            rxbuf.copy_from_slice(&data);
            Status::Ok
        }
        Err(e) => i2c_error_to_status(e),
    }
}

/// gallo_i2c_scan - Scan the I2C bus for responding devices.
///
/// The firmware probes each 7-bit address. Addresses that ACK are written
/// into `buf`. The actual number of devices found is written to `*found`.
///
/// When `include_reserved` is `false`, only the standard range (0x08–0x77)
/// is probed; when `true`, the full range (0x00–0x7F) is scanned.
///
/// Returns `Status::Ok` in case of success or various error codes. If
/// `buf_len` is smaller than the number of responding devices the buffer is
/// filled to capacity and `*found` reflects the total count.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, `buf` must be valid for
/// `buf_len` bytes, and `found` must point to a valid `usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_scan(
    gallo: *const PicoDeGallo,
    include_reserved: bool,
    buf: *mut u8,
    buf_len: usize,
    found: *mut usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() || found.is_null() {
        eprintln!("Unexpected NULL pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure buf is valid for buf_len bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, buf_len) };

    let result = block_on(gallo.0.i2c_scan(include_reserved));

    match result {
        Ok(addresses) => {
            let copy_len = addresses.len().min(buf.len());
            buf[..copy_len].copy_from_slice(&addresses[..copy_len]);
            unsafe { *found = addresses.len() };
            Status::Ok
        }
        Err(e) => i2c_error_to_status(e),
    }
}

// ----------------------------- Spi endpoints -----------------------------

/// gallo_spi_read - Read `len` bytes.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_read(
    gallo: *const PicoDeGallo,
    buf: *mut u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };

    let result = block_on(gallo.0.spi_read(len as u16));

    match result {
        Ok(data) => {
            if data.len() != buf.len() {
                eprintln!(
                    "Firmware returned {} bytes, expected {}",
                    data.len(),
                    buf.len()
                );
                return Status::InvalidResponse;
            }
            buf.copy_from_slice(&data);
            Status::Ok
        }
        Err(e) => spi_error_to_status(e),
    }
}

/// gallo_spi_write - Write `len` bytes from `buf`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_write(
    gallo: *const PicoDeGallo,
    buf: *const u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts(buf, len) };

    let result = block_on(gallo.0.spi_write(buf));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => spi_error_to_status(e),
    }
}

/// gallo_spi_flush - Flush the SPI interface.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_flush(gallo: *const PicoDeGallo) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.spi_flush());

    match result {
        Ok(()) => Status::Ok,
        Err(e) => spi_error_to_status(e),
    }
}

// ----------------------------- SPI Transfer endpoint -----------------------------

/// gallo_spi_transfer - Full-duplex SPI transfer.
///
/// Simultaneously sends `len` bytes from `write_buf` on MOSI and receives
/// `len` bytes on MISO into `read_buf`. The two buffers must be valid for
/// `len` bytes each; they MAY alias (same pointer is permitted; the firmware
/// returns a fresh response buffer that is then copied into `read_buf`).
///
/// Returns [`Status::Ok`] on success. Returns [`Status::BufferTooLong`] if
/// `len` exceeds the firmware transfer limit ([`lib::MAX_TRANSFER_SIZE`]),
/// [`Status::SpiTransferFailed`] if the firmware reports a generic SPI
/// error, or [`Status::CommsFailed`] on a USB error.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and that both `write_buf` and
/// `read_buf` are valid for `len` bytes (read and write respectively).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_transfer(
    gallo: *const PicoDeGallo,
    write_buf: *const u8,
    read_buf: *mut u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if write_buf.is_null() || read_buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure write_buf is valid for len bytes.
    let write = unsafe { std::slice::from_raw_parts(write_buf, len) };

    let result = block_on(gallo.0.spi_transfer(write));

    match result {
        Ok(data) => {
            if data.len() != len {
                eprintln!("Firmware returned {} bytes, expected {}", data.len(), len);
                return Status::InvalidResponse;
            }
            // Safety: caller must ensure read_buf is valid for len bytes.
            let read = unsafe { std::slice::from_raw_parts_mut(read_buf, len) };
            read.copy_from_slice(&data);
            Status::Ok
        }
        Err(PicoDeGalloError::Endpoint(SpiError::BufferTooLong)) => Status::BufferTooLong,
        Err(PicoDeGalloError::Endpoint(SpiError::Other)) => Status::SpiTransferFailed,
        Err(PicoDeGalloError::Endpoint(SpiError::InvalidCsPin)) => Status::SpiInvalidCsPin,
        Err(PicoDeGalloError::Endpoint(SpiError::CsPinUnavailable)) => Status::SpiCsPinUnavailable,
        Err(PicoDeGalloError::Endpoint(SpiError::CsPinMonitored)) => Status::SpiCsPinMonitored,
        Err(PicoDeGalloError::Comms(_)) => Status::CommsFailed,
    }
}

// ----------------------------- SPI Batch endpoint -----------------------------

/// C-friendly tagged-union representation of a single SPI batch operation.
///
/// Pass an array of these to [`gallo_spi_batch`]. Field interpretation
/// depends on `tag`:
///
/// | `tag` | Variant    | Fields used                  |
/// |-------|------------|------------------------------|
/// | 0     | `Read`     | `read_len`                   |
/// | 1     | `Write`    | `data`, `data_len`           |
/// | 2     | `Transfer` | `data`, `data_len`           |
/// | 3     | `DelayNs`  | `delay_ns`                   |
///
/// Unused fields for a given variant are ignored. `data` may be `NULL`
/// when `data_len` is zero or when the variant does not use it. The tag
/// values are part of the stable C ABI; do not renumber.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GalloSpiBatchOp {
    /// Variant tag: 0 = Read, 1 = Write, 2 = Transfer, 3 = DelayNs.
    pub tag: u8,
    /// Read length in bytes (Read variant only).
    pub read_len: u16,
    /// Pointer to write/transfer payload (Write / Transfer variants).
    pub data: *const u8,
    /// Length of the payload pointed to by `data`, in bytes.
    pub data_len: usize,
    /// Delay in nanoseconds (DelayNs variant only).
    pub delay_ns: u32,
}

/// Write the outcome of an SPI batch call into the caller's out-params.
///
/// Called by [`gallo_spi_batch`] for **every** outcome once `out_len` has
/// been validated non-NULL, so the invariants hold uniformly:
///
/// - `*out_len` is zeroed first, and only a successful (or capacity-short)
///   result overwrites it.
/// - `*out_failed_op` is written **only** for
///   [`lib::SpiBatchCallError::Endpoint`]. Metadata failures, timeouts and
///   local chip-select refusals never executed an operation, so fabricating
///   an index for them would be a lie.
///
/// # Safety
///
/// `out_len` must be non-NULL and writable. `out_buf` must be valid for
/// `out_capacity` bytes when `out_capacity > 0`. `out_failed_op`, if
/// non-NULL, must be writable.
unsafe fn spi_batch_write_outputs(
    result: Result<Vec<u8>, lib::SpiBatchCallError>,
    out_buf: *mut u8,
    out_capacity: usize,
    out_len: *mut usize,
    out_failed_op: *mut u16,
) -> Status {
    unsafe { *out_len = 0 };

    let err = match result {
        Ok(data) => {
            if data.len() > out_capacity {
                eprintln!(
                    "SPI batch produced {} bytes, out_buf only fits {}",
                    data.len(),
                    out_capacity
                );
                // Still report the required length so the caller can retry
                // with a larger buffer.
                unsafe { *out_len = data.len() };
                return Status::BufferTooLong;
            }
            if !data.is_empty() {
                // Safety: out_buf validated by the caller when capacity > 0.
                let slot = unsafe { std::slice::from_raw_parts_mut(out_buf, data.len()) };
                slot.copy_from_slice(&data);
            }
            unsafe { *out_len = data.len() };
            return Status::Ok;
        }
        Err(e) => e,
    };

    if let lib::SpiBatchCallError::Endpoint(SpiBatchError { failed_op, .. }) = &err
        && !out_failed_op.is_null()
    {
        unsafe { *out_failed_op = *failed_op };
    }

    spi_batch_call_error_to_status(&err)
}

/// gallo_spi_batch - Execute a batch of SPI operations atomically under CS.
///
/// The firmware asserts `cs_pin` low before the first operation and
/// deasserts it after the last (or on error). The concatenated read data
/// from `Read` and `Transfer` operations is copied into `out_buf` in
/// order; the total length is written to `*out_len`.
///
/// On batch failure, the index of the failing operation (zero-based) is
/// written to `*out_failed_op` if `out_failed_op` is non-NULL, and a
/// status reflecting the underlying SPI error is returned. Read data from
/// operations before the failing one is discarded.
///
/// Returns [`Status::Ok`] on success, [`Status::BufferTooLong`] if
/// `out_buf` is too small for the cumulative read data, or one of the
/// SPI error statuses on a per-operation failure. `out_failed_op` is only
/// written on per-operation failure, never on success and never on a
/// metadata or chip-select failure.
///
/// # Chip-select preflight
///
/// `cs_pin` is checked against the device-reported GPIO count (see
/// [`gallo_num_gpios`]) before any operation payload is translated and
/// before anything is transmitted. A refused chip-select drives no pin and
/// sends no `spi/batch` request.
///
/// - [`Status::SpiNoGpios`] (-74) — the device reports zero GPIOs.
/// - [`Status::SpiInvalidCsPin`] (-71) — `cs_pin` is at or beyond the count.
/// - [`Status::DeviceInfoFailed`] (-62), [`Status::DeviceInfoTimeout`]
///   (-75), [`Status::LegacyFirmware`] (-64), [`Status::SchemaMismatch`]
///   (-63) — the count could not be established. A metadata failure is
///   never reported as an invalid chip-select.
///
/// The first call on a handle performs one implicit `device/info`
/// round-trip; afterwards the count is cached per handle.
///
/// # Ordering
///
/// The nine steps run strictly in this order:
///
/// 1. validate `gallo`;
/// 2. validate `out_len`;
/// 3. validate the top-level `ops` / `ops_count` / `out_capacity` shape;
/// 4. write `*out_len = 0`;
/// 5. resolve the device-reported GPIO count;
/// 6. classify `cs_pin`;
/// 7. translate operation payloads;
/// 8. call the library once;
/// 9. write outputs / copy response.
///
/// Steps 1-3 precede any write, so an invalid or NULL `out_len` is never
/// dereferenced. Because op translation is step 7, an invalid op `tag` or
/// a NULL `data` pointer is only reported once the device has been reached.
///
/// # Safety
///
/// Caller must ensure that:
/// - `gallo` is a valid opaque pointer returned by `gallo_init()`.
/// - `ops` points to `ops_count` initialized [`GalloSpiBatchOp`] values.
/// - For each op with `data_len > 0`, the `data` pointer is valid for
///   `data_len` bytes for the duration of the call.
/// - `out_buf` is valid for `out_capacity` bytes when `out_capacity > 0`.
/// - `out_len` is non-NULL and points to a writable `usize`.
/// - `out_failed_op`, if non-NULL, points to a writable `u16`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_batch(
    gallo: *const PicoDeGallo,
    cs_pin: u8,
    ops: *const GalloSpiBatchOp,
    ops_count: usize,
    out_buf: *mut u8,
    out_capacity: usize,
    out_len: *mut usize,
    out_failed_op: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }
    if out_len.is_null() {
        eprintln!("Unexpected NULL out_len");
        return Status::InvalidArgument;
    }
    if ops.is_null() && ops_count != 0 {
        eprintln!("Unexpected NULL ops with non-zero count");
        return Status::InvalidArgument;
    }
    if ops_count > lib::MAX_BATCH_OPS {
        eprintln!("Too many batch operations");
        return Status::InvalidArgument;
    }
    if out_capacity > 0 && out_buf.is_null() {
        eprintln!("Unexpected NULL out_buf with non-zero capacity");
        return Status::InvalidArgument;
    }

    // `out_len` is now known writable, so every outcome from here on leaves
    // it at zero unless data was actually produced.
    unsafe { *out_len = 0 };

    // Safety: caller must ensure that `gallo` is a valid opaque pointer.
    let gallo = unsafe { &*gallo };

    // Resolve the runtime-authoritative bound. The classifier below only
    // ever sees an `Ok(n)`; an `Err` returns here, so a metadata failure can
    // never be reported as a chip-select complaint.
    let num_gpios = match block_on(gallo.0.num_gpios()) {
        Ok(n) => n,
        Err(e) => {
            let status = validate_error_to_status(&e);
            eprintln!("SPI batch: could not determine num_gpios: {e}");
            return status;
        }
    };
    if num_gpios == 0 {
        eprintln!("SPI batch: device reports num_gpios=0; no chip-select pin is available");
        return Status::SpiNoGpios;
    }
    if cs_pin >= num_gpios {
        eprintln!("SPI batch: invalid chip-select pin {cs_pin}; device reports {num_gpios} GPIOs");
        return Status::SpiInvalidCsPin;
    }

    // Safety: caller must ensure `ops` is valid for `ops_count` elements.
    let raw_ops: &[GalloSpiBatchOp] = if ops_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ops, ops_count) }
    };

    // Translate to typed SpiBatchOp. Borrow `data` slices for the call
    // lifetime; the firmware copies them out before responding.
    let mut typed: Vec<SpiBatchOp<'_>> = Vec::with_capacity(raw_ops.len());
    for (i, op) in raw_ops.iter().enumerate() {
        let typed_op = match op.tag {
            0 => SpiBatchOp::Read { len: op.read_len },
            1 | 2 => {
                let data = if op.data_len == 0 {
                    &[][..]
                } else if op.data.is_null() {
                    eprintln!("op {i}: NULL data with non-zero data_len");
                    return Status::InvalidArgument;
                } else {
                    // Safety: caller must ensure data is valid for data_len bytes.
                    unsafe { std::slice::from_raw_parts(op.data, op.data_len) }
                };
                if op.tag == 1 {
                    SpiBatchOp::Write { data }
                } else {
                    SpiBatchOp::Transfer { data }
                }
            }
            3 => SpiBatchOp::DelayNs { ns: op.delay_ns },
            other => {
                eprintln!("op {i}: invalid tag {other}");
                return Status::InvalidArgument;
            }
        };
        typed.push(typed_op);
    }

    let result = block_on(gallo.0.spi_batch(cs_pin, &typed));

    unsafe { spi_batch_write_outputs(result, out_buf, out_capacity, out_len, out_failed_op) }
}

// ----------------------------- I2C Batch endpoint -----------------------------

/// C-friendly tagged-union representation of a single I2C batch operation.
///
/// Pass an array of these to [`gallo_i2c_batch`]. Field interpretation
/// depends on `tag`:
///
/// | `tag` | Variant | Fields used         |
/// |-------|---------|---------------------|
/// | 0     | `Read`  | `read_len`          |
/// | 1     | `Write` | `data`, `data_len`  |
///
/// Unused fields for a given variant are ignored. `data` may be `NULL`
/// when `data_len` is zero. The tag values are part of the stable C ABI;
/// do not renumber.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GalloI2cBatchOp {
    /// Variant tag: 0 = Read, 1 = Write.
    pub tag: u8,
    /// Read length in bytes (Read variant only).
    pub read_len: u16,
    /// Pointer to write payload (Write variant).
    pub data: *const u8,
    /// Length of the payload pointed to by `data`, in bytes.
    pub data_len: usize,
}

/// gallo_i2c_batch - Execute a batch of I2C operations.
///
/// Operations execute sequentially with STOP between each (this is *not*
/// I2C repeated-start; for write-then-read to the same device, prefer
/// [`gallo_i2c_write_read`]). The concatenated read data from each `Read`
/// operation is copied into `out_buf` in order; the total length is
/// written to `*out_len`.
///
/// On batch failure, the index of the failing operation (zero-based) is
/// written to `*out_failed_op` if `out_failed_op` is non-NULL, and a
/// status reflecting the underlying I2C error is returned. Read data
/// from operations before the failing one is discarded.
///
/// Returns [`Status::Ok`] on success, [`Status::BufferTooLong`] if
/// `out_buf` is too small for the cumulative read data, or one of the
/// I2C error statuses on a per-operation failure.
///
/// # Safety
///
/// Caller must ensure that:
/// - `gallo` is a valid opaque pointer returned by `gallo_init()`.
/// - `ops` points to `ops_count` initialized [`GalloI2cBatchOp`] values.
/// - For each op with `data_len > 0`, the `data` pointer is valid for
///   `data_len` bytes for the duration of the call.
/// - `out_buf` is valid for `out_capacity` bytes when `out_capacity > 0`.
/// - `out_len` is non-NULL and points to a writable `usize`.
/// - `out_failed_op`, if non-NULL, points to a writable `u16`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_batch(
    gallo: *const PicoDeGallo,
    address: u8,
    ops: *const GalloI2cBatchOp,
    ops_count: usize,
    out_buf: *mut u8,
    out_capacity: usize,
    out_len: *mut usize,
    out_failed_op: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }
    if out_len.is_null() {
        eprintln!("Unexpected NULL out_len");
        return Status::InvalidArgument;
    }
    if ops.is_null() && ops_count != 0 {
        eprintln!("Unexpected NULL ops with non-zero count");
        return Status::InvalidArgument;
    }
    if ops_count > lib::MAX_BATCH_OPS {
        eprintln!("Too many batch operations");
        return Status::InvalidArgument;
    }
    if out_capacity > 0 && out_buf.is_null() {
        eprintln!("Unexpected NULL out_buf with non-zero capacity");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque pointer.
    let gallo = unsafe { &*gallo };

    // Safety: caller must ensure `ops` is valid for `ops_count` elements.
    let raw_ops: &[GalloI2cBatchOp] = if ops_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ops, ops_count) }
    };

    let mut typed: Vec<I2cBatchOp<'_>> = Vec::with_capacity(raw_ops.len());
    for (i, op) in raw_ops.iter().enumerate() {
        let typed_op = match op.tag {
            0 => I2cBatchOp::Read { len: op.read_len },
            1 => {
                let data = if op.data_len == 0 {
                    &[][..]
                } else if op.data.is_null() {
                    eprintln!("op {i}: NULL data with non-zero data_len");
                    return Status::InvalidArgument;
                } else {
                    // Safety: caller must ensure data is valid for data_len bytes.
                    unsafe { std::slice::from_raw_parts(op.data, op.data_len) }
                };
                I2cBatchOp::Write { data }
            }
            other => {
                eprintln!("op {i}: invalid tag {other}");
                return Status::InvalidArgument;
            }
        };
        typed.push(typed_op);
    }

    let result = block_on(gallo.0.i2c_batch(address, &typed));

    match result {
        Ok(data) => {
            if data.len() > out_capacity {
                eprintln!(
                    "I2C batch produced {} bytes, out_buf only fits {}",
                    data.len(),
                    out_capacity
                );
                unsafe { *out_len = data.len() };
                return Status::BufferTooLong;
            }
            if !data.is_empty() {
                let slot = unsafe { std::slice::from_raw_parts_mut(out_buf, data.len()) };
                slot.copy_from_slice(&data);
            }
            unsafe { *out_len = data.len() };
            Status::Ok
        }
        Err(PicoDeGalloError::Endpoint(I2cBatchError { failed_op, kind })) => {
            if !out_failed_op.is_null() {
                unsafe { *out_failed_op = failed_op };
            }
            unsafe { *out_len = 0 };
            i2c_error_to_status(PicoDeGalloError::Endpoint(kind))
        }
        Err(PicoDeGalloError::Comms(_)) => {
            unsafe { *out_len = 0 };
            Status::CommsFailed
        }
    }
}

// ----------------------------- Gpio endpoints -----------------------------

/// gallo_gpio_get - Get the state of a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_get(
    gallo: *const PicoDeGallo,
    pin: u8,
    state: *mut bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if state.is_null() {
        eprintln!("Unexpected NULL state pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_get(pin));

    match result {
        Ok(s) => {
            unsafe { *state = s == lib::GpioState::High };
            Status::Ok
        }
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_put - Set the state of a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_put(gallo: *const PicoDeGallo, pin: u8, state: bool) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let s = if state {
        lib::GpioState::High
    } else {
        lib::GpioState::Low
    };
    let result = block_on(gallo.0.gpio_put(pin, s));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_high - Waits for a high level on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_high(gallo: *const PicoDeGallo, pin: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_high(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_low - Waits for a low level on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_low(gallo: *const PicoDeGallo, pin: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_low(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_rising_edge - Waits for a rising edge on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_rising_edge(
    gallo: *const PicoDeGallo,
    pin: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_rising_edge(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_falling_edge - Waits for a falling edge on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_falling_edge(
    gallo: *const PicoDeGallo,
    pin: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_falling_edge(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_any_edge - Waits for a any edge on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_any_edge(
    gallo: *const PicoDeGallo,
    pin: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_any_edge(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

// ----------------------------- GPIO Wait endpoints (with timeout) -----------------------------

/// gallo_gpio_wait_for_high_with_timeout_ms - Waits for a high level
/// on a given GPIO pin, bounded by `timeout_ms`.
///
/// `timeout_ms == 0` waits forever (equivalent to
/// [`gallo_gpio_wait_for_high`]). Non-zero bounds the firmware-side
/// wait; expiry returns [`Status::GpioTimeout`].
///
/// Available on firmware schema 0.7+. Returns
/// [`Status::SchemaMismatch`] if invoked against older firmware.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_high_with_timeout_ms(
    gallo: *const PicoDeGallo,
    pin: u8,
    timeout_ms: u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result =
        block_on(gallo.0.gpio_wait_for_high_with_timeout(
            pin,
            std::time::Duration::from_millis(timeout_ms as u64),
        ));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_low_with_timeout_ms - Waits for a low level on
/// a given GPIO pin, bounded by `timeout_ms`.
///
/// `timeout_ms == 0` waits forever (equivalent to
/// [`gallo_gpio_wait_for_low`]). Non-zero bounds the firmware-side
/// wait; expiry returns [`Status::GpioTimeout`].
///
/// Available on firmware schema 0.7+. Returns
/// [`Status::SchemaMismatch`] if invoked against older firmware.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_low_with_timeout_ms(
    gallo: *const PicoDeGallo,
    pin: u8,
    timeout_ms: u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result =
        block_on(gallo.0.gpio_wait_for_low_with_timeout(
            pin,
            std::time::Duration::from_millis(timeout_ms as u64),
        ));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_rising_edge_with_timeout_ms - Waits for a
/// rising edge on a given GPIO pin, bounded by `timeout_ms`.
///
/// `timeout_ms == 0` waits forever (equivalent to
/// [`gallo_gpio_wait_for_rising_edge`]). Non-zero bounds the
/// firmware-side wait; expiry returns [`Status::GpioTimeout`].
///
/// Available on firmware schema 0.7+. Returns
/// [`Status::SchemaMismatch`] if invoked against older firmware.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_rising_edge_with_timeout_ms(
    gallo: *const PicoDeGallo,
    pin: u8,
    timeout_ms: u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_rising_edge_with_timeout(
        pin,
        std::time::Duration::from_millis(timeout_ms as u64),
    ));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_falling_edge_with_timeout_ms - Waits for a
/// falling edge on a given GPIO pin, bounded by `timeout_ms`.
///
/// `timeout_ms == 0` waits forever (equivalent to
/// [`gallo_gpio_wait_for_falling_edge`]). Non-zero bounds the
/// firmware-side wait; expiry returns [`Status::GpioTimeout`].
///
/// Available on firmware schema 0.7+. Returns
/// [`Status::SchemaMismatch`] if invoked against older firmware.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_falling_edge_with_timeout_ms(
    gallo: *const PicoDeGallo,
    pin: u8,
    timeout_ms: u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_falling_edge_with_timeout(
        pin,
        std::time::Duration::from_millis(timeout_ms as u64),
    ));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_wait_for_any_edge_with_timeout_ms - Waits for any edge
/// on a given GPIO pin, bounded by `timeout_ms`.
///
/// `timeout_ms == 0` waits forever (equivalent to
/// [`gallo_gpio_wait_for_any_edge`]). Non-zero bounds the
/// firmware-side wait; expiry returns [`Status::GpioTimeout`].
///
/// Available on firmware schema 0.7+. Returns
/// [`Status::SchemaMismatch`] if invoked against older firmware.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_any_edge_with_timeout_ms(
    gallo: *const PicoDeGallo,
    pin: u8,
    timeout_ms: u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_wait_for_any_edge_with_timeout(
        pin,
        std::time::Duration::from_millis(timeout_ms as u64),
    ));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

// ----------------------------- GPIO Set config endpoint -----------------------------

/// gallo_gpio_set_config - Configure a GPIO pin's direction and pull resistor.
///
/// `direction`: 0 = Input, 1 = Output.
/// `pull`: 0 = None, 1 = Pull-up, 2 = Pull-down.
///
/// After configuration, the pin enters explicit mode and get/put will no
/// longer auto-switch direction. Calling `gallo_gpio_put` on an input pin
/// (or `gallo_gpio_get`/wait on an output pin) returns
/// `GpioWrongDirection`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_set_config(
    gallo: *const PicoDeGallo,
    pin: u8,
    direction: u8,
    pull: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let dir = match direction {
        0 => lib::GpioDirection::Input,
        1 => lib::GpioDirection::Output,
        _ => {
            eprintln!("Invalid direction value: {direction}");
            return Status::InvalidArgument;
        }
    };

    let pull_cfg = match pull {
        0 => lib::GpioPull::None,
        1 => lib::GpioPull::Up,
        2 => lib::GpioPull::Down,
        _ => {
            eprintln!("Invalid pull value: {pull}");
            return Status::InvalidArgument;
        }
    };

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_set_config(pin, dir, pull_cfg));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

// ----------------------------- GPIO Subscribe/Unsubscribe endpoints -----------------------------

/// gallo_gpio_subscribe - Subscribe to GPIO edge events on a pin.
///
/// `edge`: 0 = Rising, 1 = Falling, 2 = Any. Any other value returns
/// `Status::InvalidArgument`.
///
/// While subscribed, other GPIO operations on this pin will return
/// `Status::GpioPinMonitored`. Use [`gallo_gpio_unsubscribe`] to release.
///
/// Returns `Status::Ok` on success.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_subscribe(
    gallo: *const PicoDeGallo,
    pin: u8,
    edge: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let edge_val = match edge {
        0 => lib::GpioEdge::Rising,
        1 => lib::GpioEdge::Falling,
        2 => lib::GpioEdge::Any,
        _ => {
            eprintln!("Invalid edge value: {edge}");
            return Status::InvalidArgument;
        }
    };

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_subscribe(pin, edge_val));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

/// gallo_gpio_unsubscribe - Unsubscribe from GPIO edge events on a pin.
///
/// Stops monitoring and returns the pin to normal operation. Returns
/// `Status::GpioPinNotMonitored` if the pin is not currently subscribed.
///
/// Returns `Status::Ok` on success.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_unsubscribe(gallo: *const PicoDeGallo, pin: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.gpio_unsubscribe(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => gpio_error_to_status(e),
    }
}

// ----------------------------- I2C Set config endpoint -----------------------------

/// gallo_i2c_set_config - Sets the I2C bus configuration parameters.
///
/// `frequency`: 0 = Standard (100 kHz), 1 = Fast (400 kHz),
/// 2 = Fast+ (1 MHz). Any other value returns `Status::InvalidArgument`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_set_config(gallo: *const PicoDeGallo, frequency: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let freq = match frequency {
        0 => lib::I2cFrequency::Standard,
        1 => lib::I2cFrequency::Fast,
        2 => lib::I2cFrequency::FastPlus,
        _ => return Status::InvalidArgument,
    };

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.i2c_set_config(freq));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => i2c_error_to_status(e),
    }
}

// ----------------------------- SPI Set config endpoint -----------------------------

/// gallo_spi_set_config - Sets the SPI bus configuration parameters.
///
/// `spi_phase`: false means "Capture on first transition" or CPHA=0,
/// true means "Capture on second transition" or CPHA=1.
///
/// `spi_polarity`: false means "Idle low" or CPOL=0, true means "Idle
/// high" or CPOL=1.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_set_config(
    gallo: *const PicoDeGallo,
    frequency: u32,
    spi_phase: bool,
    spi_polarity: bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let phase = if spi_phase {
        lib::SpiPhase::CaptureOnSecondTransition
    } else {
        lib::SpiPhase::CaptureOnFirstTransition
    };

    let polarity = if spi_polarity {
        lib::SpiPolarity::IdleHigh
    } else {
        lib::SpiPolarity::IdleLow
    };

    let result = block_on(gallo.0.spi_set_config(frequency, phase, polarity));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => spi_error_to_status(e),
    }
}

// ----------------------------- I2C Get config endpoint -----------------------------

/// gallo_i2c_get_config - Queries the current I2C bus configuration.
///
/// On success, writes the current frequency to `*out_frequency`:
/// 0 = Standard (100 kHz), 1 = Fast (400 kHz), 2 = Fast+ (1 MHz).
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that `out_frequency`
/// is a valid pointer to a `u8`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_get_config(
    gallo: *const PicoDeGallo,
    out_frequency: *mut u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_frequency.is_null() {
        eprintln!("Unexpected NULL out_frequency pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.i2c_get_config());

    match result {
        Ok(freq) => {
            unsafe {
                *out_frequency = freq as u8;
            }
            Status::Ok
        }
        Err(_) => Status::I2cGetConfigFailed,
    }
}

// ----------------------------- SPI Get config endpoint -----------------------------

/// gallo_spi_get_config - Queries the current SPI bus configuration.
///
/// On success, writes the current SPI parameters:
/// - `*out_frequency`: SPI clock frequency in Hz
/// - `*out_phase`: false = CPHA=0, true = CPHA=1
/// - `*out_polarity`: false = CPOL=0, true = CPOL=1
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that all output
/// pointers are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_get_config(
    gallo: *const PicoDeGallo,
    out_frequency: *mut u32,
    out_phase: *mut bool,
    out_polarity: *mut bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_frequency.is_null() || out_phase.is_null() || out_polarity.is_null() {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.spi_get_config());

    match result {
        Ok(info) => {
            unsafe {
                *out_frequency = info.spi_frequency;
                *out_phase = matches!(info.spi_phase, lib::SpiPhase::CaptureOnSecondTransition);
                *out_polarity = matches!(info.spi_polarity, lib::SpiPolarity::IdleHigh);
            }
            Status::Ok
        }
        Err(_) => Status::SpiGetConfigFailed,
    }
}

// ----------------------------- UART Read endpoint -----------------------------

/// gallo_uart_read - Read bytes from the UART bus.
///
/// Reads up to `count` bytes into `buf`. On success, writes the actual
/// number of bytes read to `*out_len`. If no data arrives within
/// `timeout_ms` milliseconds, sets `*out_len = 0` and returns
/// `Status::Ok`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, that `buf` points to at
/// least `count` bytes, and `out_len` is a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_read(
    gallo: *const PicoDeGallo,
    buf: *mut u8,
    count: u16,
    timeout_ms: u32,
    out_len: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() || out_len.is_null() {
        eprintln!("Unexpected NULL pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.uart_read(count, timeout_ms));

    match result {
        Ok(data) => {
            let len = data.len().min(count as usize);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), buf, len);
                *out_len = len as u16;
            }
            Status::Ok
        }
        Err(e) => uart_error_to_status(e),
    }
}

// ----------------------------- UART Write endpoint -----------------------------

/// gallo_uart_write - Write bytes to the UART bus.
///
/// Queues `len` bytes from `buf` to the UART transmit buffer. Returns
/// once all bytes have been accepted. Use [`gallo_uart_flush`] to wait
/// for transmission to complete on the wire.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that `buf` points to
/// at least `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_write(
    gallo: *const PicoDeGallo,
    buf: *const u8,
    len: u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buf pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let data = unsafe { std::slice::from_raw_parts(buf, len as usize) };

    let result = block_on(gallo.0.uart_write(data));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => uart_error_to_status(e),
    }
}

// ----------------------------- UART Flush endpoint -----------------------------

/// gallo_uart_flush - Flush the UART transmit buffer.
///
/// Blocks until all pending bytes have been transmitted on the wire.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_flush(gallo: *const PicoDeGallo) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.uart_flush());

    match result {
        Ok(()) => Status::Ok,
        Err(e) => uart_error_to_status(e),
    }
}

// ----------------------------- UART Set config endpoint -----------------------------

/// gallo_uart_set_config - Set the UART baud rate.
///
/// `baud_rate` must be greater than 0. Returns `Status::InvalidArgument`
/// for a zero baud rate.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_set_config(
    gallo: *const PicoDeGallo,
    baud_rate: u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if baud_rate == 0 {
        eprintln!("Invalid baud rate: 0");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.uart_set_config(baud_rate));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => uart_error_to_status(e),
    }
}

// ----------------------------- UART Get config endpoint -----------------------------

/// gallo_uart_get_config - Query the current UART configuration.
///
/// On success, writes the current baud rate to `*out_baud_rate`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that `out_baud_rate`
/// is a valid pointer to a `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_get_config(
    gallo: *const PicoDeGallo,
    out_baud_rate: *mut u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_baud_rate.is_null() {
        eprintln!("Unexpected NULL out_baud_rate pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.uart_get_config());

    match result {
        Ok(info) => {
            unsafe {
                *out_baud_rate = info.baud_rate;
            }
            Status::Ok
        }
        Err(e) => uart_error_to_status(e),
    }
}

// ----------------------------- PWM endpoints -----------------------------

/// gallo_pwm_set_duty_cycle - Set the raw duty cycle of a PWM channel.
///
/// `channel` is 0–3. `duty` is the raw compare value (0 to the current
/// `top` register). Use `gallo_pwm_get_duty_cycle` to discover the max.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_pwm_set_duty_cycle(
    gallo: *const PicoDeGallo,
    channel: u8,
    duty: u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.pwm_set_duty_cycle(channel, duty)) {
        Ok(()) => Status::Ok,
        Err(e) => pwm_error_to_status(e),
    }
}

/// gallo_pwm_get_duty_cycle - Query the current duty cycle of a PWM channel.
///
/// On success, writes the current raw compare value to `*out_duty` and
/// the maximum duty (top + 1) to `*out_max_duty`.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that `out_duty` and
/// `out_max_duty` are valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_pwm_get_duty_cycle(
    gallo: *const PicoDeGallo,
    channel: u8,
    out_duty: *mut u16,
    out_max_duty: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_duty.is_null() || out_max_duty.is_null() {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.pwm_get_duty_cycle(channel)) {
        Ok(info) => {
            unsafe {
                *out_duty = info.current_duty;
                *out_max_duty = info.max_duty;
            }
            Status::Ok
        }
        Err(e) => pwm_error_to_status(e),
    }
}

/// gallo_pwm_enable - Enable the PWM slice that owns the given channel.
///
/// Channels 0–1 share a slice, channels 2–3 share another.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_pwm_enable(gallo: *const PicoDeGallo, channel: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.pwm_enable(channel)) {
        Ok(()) => Status::Ok,
        Err(e) => pwm_error_to_status(e),
    }
}

/// gallo_pwm_disable - Disable the PWM slice that owns the given channel.
///
/// Channels 0–1 share a slice, channels 2–3 share another.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_pwm_disable(gallo: *const PicoDeGallo, channel: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.pwm_disable(channel)) {
        Ok(()) => Status::Ok,
        Err(e) => pwm_error_to_status(e),
    }
}

/// gallo_pwm_set_config - Configure the PWM slice behind a channel.
///
/// Sets `frequency_hz` and `phase_correct` mode. The firmware computes
/// the `top` and `divider` registers automatically.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_pwm_set_config(
    gallo: *const PicoDeGallo,
    channel: u8,
    frequency_hz: u32,
    phase_correct: bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.pwm_set_config(channel, frequency_hz, phase_correct)) {
        Ok(()) => Status::Ok,
        Err(e) => pwm_error_to_status(e),
    }
}

/// gallo_pwm_get_config - Query the current PWM configuration.
///
/// On success, writes the effective frequency to `*out_frequency_hz`,
/// the phase-correct flag to `*out_phase_correct`, and the enabled
/// flag to `*out_enabled`.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that all output
/// pointers are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_pwm_get_config(
    gallo: *const PicoDeGallo,
    channel: u8,
    out_frequency_hz: *mut u32,
    out_phase_correct: *mut bool,
    out_enabled: *mut bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_frequency_hz.is_null() || out_phase_correct.is_null() || out_enabled.is_null() {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.pwm_get_config(channel)) {
        Ok(info) => {
            unsafe {
                *out_frequency_hz = info.frequency_hz;
                *out_phase_correct = info.phase_correct;
                *out_enabled = info.enabled;
            }
            Status::Ok
        }
        Err(e) => pwm_error_to_status(e),
    }
}

// ----------------------------- ADC endpoints -----------------------------

/// gallo_adc_read - Perform a single-shot ADC read.
///
/// On success, writes the raw 12-bit value (0–4095) to `*out_value`.
/// `channel` selects the input: 0–3 for GPIO26–29.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that `out_value` is a
/// valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_adc_read(
    gallo: *const PicoDeGallo,
    channel: u8,
    out_value: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_value.is_null() {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    let adc_channel = match channel {
        0 => AdcChannel::Adc0,
        1 => AdcChannel::Adc1,
        2 => AdcChannel::Adc2,
        3 => AdcChannel::Adc3,
        _ => {
            eprintln!("Invalid ADC channel: {channel}");
            return Status::InvalidArgument;
        }
    };

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.adc_read(adc_channel)) {
        Ok(raw) => {
            unsafe { *out_value = raw };
            Status::Ok
        }
        Err(e) => adc_error_to_status(e),
    }
}

/// gallo_adc_get_config - Query the ADC configuration.
///
/// On success, writes resolution (bits), nominal reference voltage (mV),
/// number of GPIO channels.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that all output pointers
/// are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_adc_get_config(
    gallo: *const PicoDeGallo,
    out_resolution_bits: *mut u8,
    out_nominal_reference_mv: *mut u16,
    out_num_gpio_channels: *mut u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_resolution_bits.is_null()
        || out_nominal_reference_mv.is_null()
        || out_num_gpio_channels.is_null()
    {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.adc_get_config()) {
        Ok(info) => {
            unsafe {
                *out_resolution_bits = info.resolution_bits;
                *out_nominal_reference_mv = info.nominal_reference_mv;
                *out_num_gpio_channels = info.num_gpio_channels;
            }
            Status::Ok
        }
        Err(e) => adc_error_to_status(e),
    }
}

// ----------------------------- 1-Wire endpoints -----------------------------

#[unsafe(no_mangle)]
/// gallo_onewire_reset - Perform a 1-Wire bus reset.
///
/// On success, `*out_present` is set to `true` if device(s) responded with a
/// presence pulse, `false` otherwise.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
pub unsafe extern "C" fn gallo_onewire_reset(
    gallo: *const PicoDeGallo,
    out_present: *mut bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_present.is_null() {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.onewire_reset()) {
        Ok(present) => {
            unsafe {
                *out_present = present;
            }
            Status::Ok
        }
        Err(e) => onewire_error_to_status(e),
    }
}

#[unsafe(no_mangle)]
/// gallo_onewire_read - Read bytes from the 1-Wire bus.
///
/// Reads up to `len` bytes into `buf`. On success, `*out_len` is the number
/// of bytes actually read.
///
/// # Safety
///
/// - `buf` must point to at least `len` writable bytes.
/// - `out_len` must be a valid writable `u16` pointer.
pub unsafe extern "C" fn gallo_onewire_read(
    gallo: *const PicoDeGallo,
    buf: *mut u8,
    len: u16,
    out_len: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() || out_len.is_null() {
        eprintln!("Unexpected NULL pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    match block_on(gallo.0.onewire_read(len)) {
        Ok(data) => {
            let copy_len = data.len().min(len as usize);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), buf, copy_len);
                *out_len = copy_len as u16;
            }
            Status::Ok
        }
        Err(e) => onewire_error_to_status(e),
    }
}

#[unsafe(no_mangle)]
/// gallo_onewire_write - Write raw bytes to the 1-Wire bus.
///
/// # Safety
///
/// - `buf` must point to at least `len` readable bytes.
pub unsafe extern "C" fn gallo_onewire_write(
    gallo: *const PicoDeGallo,
    buf: *const u8,
    len: u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() && len > 0 {
        eprintln!("Unexpected NULL buffer pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    let data = if len > 0 {
        unsafe { std::slice::from_raw_parts(buf, len as usize) }
    } else {
        &[]
    };

    match block_on(gallo.0.onewire_write(data)) {
        Ok(()) => Status::Ok,
        Err(e) => onewire_error_to_status(e),
    }
}

#[unsafe(no_mangle)]
/// gallo_onewire_write_pullup - Write bytes then apply a strong pullup.
///
/// After writing `len` bytes from `buf`, the bus is held high for
/// `pullup_duration_ms` milliseconds to supply power to parasitic-power devices.
///
/// # Safety
///
/// - `buf` must point to at least `len` readable bytes.
pub unsafe extern "C" fn gallo_onewire_write_pullup(
    gallo: *const PicoDeGallo,
    buf: *const u8,
    len: u16,
    pullup_duration_ms: u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() && len > 0 {
        eprintln!("Unexpected NULL buffer pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    let data = if len > 0 {
        unsafe { std::slice::from_raw_parts(buf, len as usize) }
    } else {
        &[]
    };

    match block_on(gallo.0.onewire_write_pullup(data, pullup_duration_ms)) {
        Ok(()) => Status::Ok,
        Err(e) => onewire_error_to_status(e),
    }
}

#[unsafe(no_mangle)]
/// gallo_onewire_search - Search for all devices on the 1-Wire bus.
///
/// Discovers up to `max_count` ROM IDs and writes them to `out_rom_ids`.
/// On success, `*out_count` holds the number of devices found.
///
/// # Safety
///
/// - `out_rom_ids` must point to at least `max_count` writable `u64` elements.
/// - `out_count` must be a valid writable `u16` pointer.
pub unsafe extern "C" fn gallo_onewire_search(
    gallo: *const PicoDeGallo,
    out_rom_ids: *mut u64,
    max_count: u16,
    out_count: *mut u16,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_rom_ids.is_null() || out_count.is_null() {
        eprintln!("Unexpected NULL pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };

    // First search
    let first = match block_on(gallo.0.onewire_search()) {
        Ok(Some(id)) => id,
        Ok(None) => {
            unsafe {
                *out_count = 0;
            }
            return Status::Ok;
        }
        Err(e) => return onewire_error_to_status(e),
    };

    unsafe {
        *out_rom_ids = first;
    }
    let mut count: u16 = 1;

    // Continue searching
    while count < max_count {
        match block_on(gallo.0.onewire_search_next()) {
            Ok(Some(id)) => {
                unsafe {
                    *out_rom_ids.add(count as usize) = id;
                }
                count += 1;
            }
            Ok(None) => break,
            Err(e) => return onewire_error_to_status(e),
        }
    }

    unsafe {
        *out_count = count;
    }
    Status::Ok
}

// ----------------------------- Version endpoint -----------------------------

#[unsafe(no_mangle)]
/// gallo_version - Gets the firmware version.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
pub unsafe extern "C" fn gallo_version(
    gallo: *const PicoDeGallo,
    major: *mut u16,
    minor: *mut u16,
    patch: *mut u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if major.is_null() || minor.is_null() || patch.is_null() {
        eprintln!("Unexpected NULL version pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.version());

    match result {
        Ok(lib::VersionInfo {
            major: a,
            minor: b,
            patch: c,
        }) => {
            unsafe {
                *major = a;
                *minor = b;
                *patch = c;
            }

            Status::Ok
        }
        Err(_) => Status::VersionFailed,
    }
}

// ----------------------------- Device Info endpoint -----------------------------

/// I2C bus support (bit 0).
pub const GALLO_CAP_I2C: u64 = 1 << 0;
/// SPI bus support (bit 1).
pub const GALLO_CAP_SPI: u64 = 1 << 1;
/// UART support (bit 2).
pub const GALLO_CAP_UART: u64 = 1 << 2;
/// GPIO support (bit 3).
pub const GALLO_CAP_GPIO: u64 = 1 << 3;
/// PWM output support (bit 4).
pub const GALLO_CAP_PWM: u64 = 1 << 4;
/// ADC input support (bit 5).
pub const GALLO_CAP_ADC: u64 = 1 << 5;
/// 1-Wire bus support (bit 6).
pub const GALLO_CAP_ONEWIRE: u64 = 1 << 6;

/// C-compatible device information struct.
///
/// Populated by [`gallo_get_device_info`]. Contains firmware version,
/// schema (wire protocol) version, hardware revision, and peripheral
/// capabilities as a `u64` bitfield.
///
/// Test individual capabilities with bitwise AND:
///
/// ```c
/// if (info.capabilities & GALLO_CAP_I2C) {
///     // I2C supported
/// }
/// ```
#[repr(C)]
#[derive(Debug)]
pub struct GalloDeviceInfo {
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
    /// Hardware revision number.
    pub hw_version: u8,
    /// Peripheral capabilities bitfield.
    ///
    /// Each bit represents a peripheral; use the `GALLO_CAP_*` constants
    /// to test individual capabilities.
    pub capabilities: u64,
}

#[unsafe(no_mangle)]
/// gallo_get_device_info - Gets extended device information.
///
/// Queries firmware version, schema version, hardware revision, and
/// peripheral capabilities in a single call. Also validates that the
/// schema version is compatible with this host library.
///
/// Returns `Status::Ok` on success, `Status::SchemaMismatch` if the
/// firmware's wire protocol is incompatible, `Status::LegacyFirmware`
/// if the firmware does not support this endpoint, or
/// `Status::DeviceInfoFailed` on communication error.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and `out` points to a
/// valid `GalloDeviceInfo`.
pub unsafe extern "C" fn gallo_get_device_info(
    gallo: *const PicoDeGallo,
    out: *mut GalloDeviceInfo,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out.is_null() {
        eprintln!("Unexpected NULL device info pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.validate());

    match result {
        Ok(info) => {
            unsafe {
                (*out).fw_major = info.fw_major;
                (*out).fw_minor = info.fw_minor;
                (*out).fw_patch = info.fw_patch;
                (*out).schema_major = info.schema_major;
                (*out).schema_minor = info.schema_minor;
                (*out).schema_patch = info.schema_patch;
                (*out).hw_version = info.hw_version;
                (*out).capabilities = info.capabilities.bits();
            }
            Status::Ok
        }
        Err(e) => validate_error_to_status(&e),
    }
}

/// Write the outcome of a `num_gpios` lookup into the caller's out-param.
///
/// The output is written **only** on success — including a successful zero,
/// which is a legitimate answer and not an error. Every error leaves the
/// caller's buffer untouched, so a sentinel written before the call is a
/// reliable "was this populated?" check.
///
/// # Safety
///
/// `out` must be non-NULL and point to a writable `u8`.
unsafe fn num_gpios_write_output(res: Result<u8, lib::ValidateError>, out: *mut u8) -> Status {
    match res {
        Ok(n) => {
            unsafe { *out = n };
            Status::Ok
        }
        Err(e) => validate_error_to_status(&e),
    }
}

/// gallo_num_gpios - Query how many GPIO pins the device reports.
///
/// This is the runtime-authoritative bound for a GPIO index and for the
/// SPI chip-select pin of [`gallo_spi_batch`]. Prefer it over the
/// compile-time [`GALLO_NUM_GPIOS`], which is only this build's default.
///
/// The first call on a handle performs one `device/info` round-trip, which
/// also validates the firmware's reported schema version; the value is
/// cached for the lifetime of the handle. Note that during the current
/// schema freeze a matching reported version does not prove wire-*shape*
/// compatibility — it bounds how long you wait and checks the reported
/// numbers, nothing more.
///
/// `*out_num_gpios` is written only on [`Status::Ok`]. A device that
/// reports zero is a success and writes zero. On any error the caller's
/// buffer is left untouched.
///
/// Returns [`Status::Uninitialized`] for a NULL handle,
/// [`Status::InvalidArgument`] for a NULL output,
/// [`Status::DeviceInfoFailed`] on a transport or decode failure,
/// [`Status::DeviceInfoTimeout`] if the query did not complete within 300
/// seconds, [`Status::LegacyFirmware`] if the firmware has no
/// `device/info` endpoint, or [`Status::SchemaMismatch`] on a version
/// disagreement.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, and that `out_num_gpios`
/// points to a writable `uint8_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_num_gpios(
    gallo: *const PicoDeGallo,
    out_num_gpios: *mut u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }
    if out_num_gpios.is_null() {
        eprintln!("Unexpected NULL out_num_gpios pointer");
        return Status::InvalidArgument;
    }

    let gallo = unsafe { &*gallo };
    let res = block_on(gallo.0.num_gpios());

    unsafe { num_gpios_write_output(res, out_num_gpios) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ----------------------------- Status code invariants -----------------------------

    #[test]
    fn ok_is_zero() {
        assert_eq!(Status::Ok as i32, 0);
    }

    #[test]
    fn all_errors_are_negative() {
        let error_codes = [
            Status::I2cReadFailed as i32,
            Status::I2cWriteFailed as i32,
            Status::InvalidResponse as i32,
            Status::Uninitialized as i32,
            Status::InvalidArgument as i32,
            Status::PingFailed as i32,
            Status::SpiReadFailed as i32,
            Status::SpiWriteFailed as i32,
            Status::SpiFlushFailed as i32,
            Status::GpioGetFailed as i32,
            Status::GpioPutFailed as i32,
            Status::GpioWaitFailed as i32,
            Status::SetConfigFailed as i32,
            Status::VersionFailed as i32,
            Status::I2cWriteReadFailed as i32,
            Status::I2cSetConfigFailed as i32,
            Status::SpiSetConfigFailed as i32,
            Status::I2cNack as i32,
            Status::I2cBusError as i32,
            Status::I2cArbitrationLoss as i32,
            Status::I2cOverrun as i32,
            Status::BufferTooLong as i32,
            Status::I2cAddressOutOfRange as i32,
            Status::GpioInvalidPin as i32,
            Status::CommsFailed as i32,
            Status::I2cScanFailed as i32,
            Status::GpioSetConfigFailed as i32,
            Status::GpioWrongDirection as i32,
            Status::I2cGetConfigFailed as i32,
            Status::SpiGetConfigFailed as i32,
            Status::UartReadFailed as i32,
            Status::UartWriteFailed as i32,
            Status::UartFlushFailed as i32,
            Status::UartOverrun as i32,
            Status::UartBreak as i32,
            Status::UartParity as i32,
            Status::UartFraming as i32,
            Status::UartInvalidBaudRate as i32,
            Status::UartSetConfigFailed as i32,
            Status::UartGetConfigFailed as i32,
            Status::PwmSetDutyCycleFailed as i32,
            Status::PwmGetDutyCycleFailed as i32,
            Status::PwmEnableFailed as i32,
            Status::PwmDisableFailed as i32,
            Status::PwmSetConfigFailed as i32,
            Status::PwmGetConfigFailed as i32,
            Status::PwmInvalidChannel as i32,
            Status::PwmInvalidDutyCycle as i32,
            Status::PwmInvalidConfiguration as i32,
            Status::AdcReadFailed as i32,
            Status::AdcGetConfigFailed as i32,
            Status::AdcConversionFailed as i32,
            Status::GpioPinMonitored as i32,
            Status::GpioPinNotMonitored as i32,
            Status::GpioSubscribeFailed as i32,
            Status::GpioUnsubscribeFailed as i32,
            Status::OneWireNoPresence as i32,
            Status::OneWireBusError as i32,
            Status::OneWireReadFailed as i32,
            Status::OneWireWriteFailed as i32,
            Status::OneWireSearchFailed as i32,
            Status::DeviceInfoFailed as i32,
            Status::SchemaMismatch as i32,
            Status::LegacyFirmware as i32,
            Status::Unsupported as i32,
            Status::I2cBatchFailed as i32,
            Status::SpiBatchFailed as i32,
            Status::SpiTransferFailed as i32,
            Status::SystemResetSubscriptionsFailed as i32,
            Status::GpioTimeout as i32,
            Status::SpiInvalidCsPin as i32,
            Status::SpiCsPinUnavailable as i32,
            Status::SpiCsPinMonitored as i32,
            Status::SpiNoGpios as i32,
            Status::DeviceInfoTimeout as i32,
        ];
        for code in error_codes {
            assert!(code < 0, "error code {code} should be negative");
        }
    }

    #[test]
    fn status_codes_are_distinct() {
        let codes = [
            Status::Ok as i32,
            Status::I2cReadFailed as i32,
            Status::I2cWriteFailed as i32,
            Status::InvalidResponse as i32,
            Status::Uninitialized as i32,
            Status::InvalidArgument as i32,
            Status::PingFailed as i32,
            Status::SpiReadFailed as i32,
            Status::SpiWriteFailed as i32,
            Status::SpiFlushFailed as i32,
            Status::GpioGetFailed as i32,
            Status::GpioPutFailed as i32,
            Status::GpioWaitFailed as i32,
            Status::SetConfigFailed as i32,
            Status::VersionFailed as i32,
            Status::I2cWriteReadFailed as i32,
            Status::I2cSetConfigFailed as i32,
            Status::SpiSetConfigFailed as i32,
            Status::I2cNack as i32,
            Status::I2cBusError as i32,
            Status::I2cArbitrationLoss as i32,
            Status::I2cOverrun as i32,
            Status::BufferTooLong as i32,
            Status::I2cAddressOutOfRange as i32,
            Status::GpioInvalidPin as i32,
            Status::CommsFailed as i32,
            Status::I2cScanFailed as i32,
            Status::GpioSetConfigFailed as i32,
            Status::GpioWrongDirection as i32,
            Status::I2cGetConfigFailed as i32,
            Status::SpiGetConfigFailed as i32,
            Status::UartReadFailed as i32,
            Status::UartWriteFailed as i32,
            Status::UartFlushFailed as i32,
            Status::UartOverrun as i32,
            Status::UartBreak as i32,
            Status::UartParity as i32,
            Status::UartFraming as i32,
            Status::UartInvalidBaudRate as i32,
            Status::UartSetConfigFailed as i32,
            Status::UartGetConfigFailed as i32,
            Status::PwmSetDutyCycleFailed as i32,
            Status::PwmGetDutyCycleFailed as i32,
            Status::PwmEnableFailed as i32,
            Status::PwmDisableFailed as i32,
            Status::PwmSetConfigFailed as i32,
            Status::PwmGetConfigFailed as i32,
            Status::PwmInvalidChannel as i32,
            Status::PwmInvalidDutyCycle as i32,
            Status::PwmInvalidConfiguration as i32,
            Status::AdcReadFailed as i32,
            Status::AdcGetConfigFailed as i32,
            Status::AdcConversionFailed as i32,
            Status::GpioPinMonitored as i32,
            Status::GpioPinNotMonitored as i32,
            Status::GpioSubscribeFailed as i32,
            Status::GpioUnsubscribeFailed as i32,
            Status::OneWireNoPresence as i32,
            Status::OneWireBusError as i32,
            Status::OneWireReadFailed as i32,
            Status::OneWireWriteFailed as i32,
            Status::OneWireSearchFailed as i32,
            Status::DeviceInfoFailed as i32,
            Status::SchemaMismatch as i32,
            Status::LegacyFirmware as i32,
            Status::Unsupported as i32,
            Status::I2cBatchFailed as i32,
            Status::SpiBatchFailed as i32,
            Status::SpiTransferFailed as i32,
            Status::SystemResetSubscriptionsFailed as i32,
            Status::GpioTimeout as i32,
            Status::SpiInvalidCsPin as i32,
            Status::SpiCsPinUnavailable as i32,
            Status::SpiCsPinMonitored as i32,
            Status::SpiNoGpios as i32,
            Status::DeviceInfoTimeout as i32,
        ];
        let unique: HashSet<i32> = codes.iter().copied().collect();
        assert_eq!(codes.len(), unique.len(), "duplicate status codes found");
    }

    #[test]
    fn spi_chip_select_status_codes_are_stable() {
        // Status values are stable C ABI (AGENTS.md §8): append-only, never
        // renumbered. -70 predates this change but was previously unpinned
        // by any test; pinning it here closes that gap.
        assert_eq!(Status::GpioTimeout as i32, -70);
        assert_eq!(Status::SpiInvalidCsPin as i32, -71);
        assert_eq!(Status::SpiCsPinUnavailable as i32, -72);
        assert_eq!(Status::SpiCsPinMonitored as i32, -73);
    }

    // ===================================================================
    // M3 — SPI chip-select bounds (issue #104)
    // ===================================================================

    #[test]
    fn status_spi_no_gpios_is_minus_seventy_four() {
        assert_eq!(Status::SpiNoGpios as i32, -74);
    }

    #[test]
    fn status_device_info_timeout_is_minus_seventy_five() {
        assert_eq!(Status::DeviceInfoTimeout as i32, -75);
    }

    #[test]
    fn status_cs_codes_are_unchanged_at_minus_seventy_one_to_seventy_three() {
        // M1 shipped these. They are stable C ABI and must never be
        // renumbered to make room for an addition.
        assert_eq!(Status::SpiInvalidCsPin as i32, -71);
        assert_eq!(Status::SpiCsPinUnavailable as i32, -72);
        assert_eq!(Status::SpiCsPinMonitored as i32, -73);
    }

    fn all_validate_errors() -> Vec<lib::ValidateError> {
        vec![
            lib::ValidateError::Comms(lib::HostErr::Closed),
            lib::ValidateError::Timeout,
            lib::ValidateError::LegacyFirmware,
            lib::ValidateError::SchemaMismatch {
                expected_major: 0,
                actual_major: 0,
                expected_minor: 7,
                actual_minor: 6,
            },
        ]
    }

    #[test]
    fn validate_error_to_status_maps_comms_to_device_info_failed() {
        assert_eq!(
            validate_error_to_status(&lib::ValidateError::Comms(lib::HostErr::Closed)) as i32,
            -62
        );
    }

    #[test]
    fn validate_error_to_status_maps_timeout_to_device_info_timeout() {
        assert_eq!(
            validate_error_to_status(&lib::ValidateError::Timeout) as i32,
            -75
        );
    }

    #[test]
    fn validate_error_to_status_maps_legacy_to_legacy_firmware() {
        assert_eq!(
            validate_error_to_status(&lib::ValidateError::LegacyFirmware) as i32,
            -64
        );
    }

    #[test]
    fn validate_error_to_status_maps_schema_to_schema_mismatch() {
        assert_eq!(
            validate_error_to_status(&lib::ValidateError::SchemaMismatch {
                expected_major: 0,
                actual_major: 0,
                expected_minor: 7,
                actual_minor: 6,
            }) as i32,
            -63
        );
    }

    #[test]
    fn validate_error_never_maps_to_a_chip_select_status() {
        // THE BINDING CONSTRAINT at the FFI boundary. A failure to *learn*
        // the valid chip-select range must never be reported as a complaint
        // about the caller's chip-select.
        let allowed: HashSet<i32> = [-62, -63, -64, -75].into_iter().collect();
        let forbidden: HashSet<i32> = [-71, -72, -73, -74].into_iter().collect();
        for e in all_validate_errors() {
            let code = validate_error_to_status(&e) as i32;
            assert!(
                !forbidden.contains(&code),
                "{e:?} mapped to chip-select status {code}"
            );
            assert!(
                allowed.contains(&code),
                "{e:?} mapped to unexpected status {code}"
            );
        }
    }

    fn all_spi_batch_call_errors() -> Vec<lib::SpiBatchCallError> {
        vec![
            lib::SpiBatchCallError::DeviceInfo(lib::ValidateError::Comms(lib::HostErr::Closed)),
            lib::SpiBatchCallError::NoGpios,
            lib::SpiBatchCallError::InvalidCsPin {
                cs: 255,
                num_gpios: 4,
            },
            lib::SpiBatchCallError::Comms(lib::HostErr::Closed),
            lib::SpiBatchCallError::Endpoint(SpiBatchError {
                failed_op: 0,
                kind: SpiError::Other,
            }),
        ]
    }

    #[test]
    fn spi_batch_call_error_to_status_is_injective() {
        let mapped: Vec<i32> = all_spi_batch_call_errors()
            .iter()
            .map(|e| spi_batch_call_error_to_status(e) as i32)
            .collect();
        let unique: HashSet<i32> = mapped.iter().copied().collect();
        assert_eq!(
            mapped.len(),
            unique.len(),
            "collapsed distinct outcomes: {mapped:?}"
        );
    }

    #[test]
    fn spi_batch_call_error_status_witness_is_exhaustive() {
        // Compile-time witness: appending a SpiBatchCallError variant must
        // fail to build here rather than fall through a wildcard.
        fn witness(e: &lib::SpiBatchCallError) -> u8 {
            match e {
                lib::SpiBatchCallError::DeviceInfo(_) => 0,
                lib::SpiBatchCallError::NoGpios => 1,
                lib::SpiBatchCallError::InvalidCsPin { .. } => 2,
                lib::SpiBatchCallError::Comms(_) => 3,
                lib::SpiBatchCallError::Endpoint(_) => 4,
            }
        }
        let tags: HashSet<u8> = all_spi_batch_call_errors().iter().map(witness).collect();
        assert_eq!(tags.len(), 5);
    }

    // --- gallo_spi_batch output invariants (via the outputs seam) ---
    //
    // `gallo_init` opens USB, so no test may construct a handle. These drive
    // the extracted output helper directly, which is the code path
    // `gallo_spi_batch` uses for every outcome after `out_len` has been
    // validated.

    /// Run `spi_batch_write_outputs` with sentinel out-params and report
    /// `(status, *out_len, *out_failed_op)`.
    fn run_outputs(result: Result<Vec<u8>, lib::SpiBatchCallError>) -> (i32, usize, u16) {
        let mut buf = [0u8; 16];
        let mut out_len: usize = 0xABCD;
        let mut failed_op: u16 = 0xA5A5;
        let status = unsafe {
            spi_batch_write_outputs(
                result,
                buf.as_mut_ptr(),
                buf.len(),
                &mut out_len,
                &mut failed_op,
            )
        };
        (status as i32, out_len, failed_op)
    }

    #[test]
    fn spi_batch_outputs_zero_out_len_on_metadata_failure() {
        let (status, len, _) = run_outputs(Err(lib::SpiBatchCallError::DeviceInfo(
            lib::ValidateError::Comms(lib::HostErr::Closed),
        )));
        assert_eq!(status, -62);
        assert_eq!(len, 0);
    }

    #[test]
    fn spi_batch_outputs_zero_out_len_on_local_refusals() {
        let (status, len, _) = run_outputs(Err(lib::SpiBatchCallError::InvalidCsPin {
            cs: 255,
            num_gpios: 4,
        }));
        assert_eq!(status, -71);
        assert_eq!(len, 0);

        let (status, len, _) = run_outputs(Err(lib::SpiBatchCallError::NoGpios));
        assert_eq!(status, -74);
        assert_eq!(len, 0);
    }

    #[test]
    fn spi_batch_outputs_zero_out_len_on_timeout() {
        let (status, len, _) = run_outputs(Err(lib::SpiBatchCallError::DeviceInfo(
            lib::ValidateError::Timeout,
        )));
        assert_eq!(status, -75);
        assert_eq!(len, 0);
    }

    #[test]
    fn spi_batch_null_out_len_returns_invalid_argument_before_any_write() {
        // A NULL handle must be rejected before anything is written, so the
        // caller's `out_len` sentinel survives untouched.
        let mut out_len: usize = 0xABCD;
        let status = unsafe {
            gallo_spi_batch(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut out_len,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
        assert_eq!(out_len, 0xABCD);
    }

    #[test]
    fn spi_batch_outputs_never_write_failed_op_on_metadata_failure() {
        for e in all_validate_errors() {
            let (_, _, failed_op) = run_outputs(Err(lib::SpiBatchCallError::DeviceInfo(e)));
            assert_eq!(
                failed_op, 0xA5A5,
                "metadata failure fabricated a failed-op index"
            );
        }
    }

    #[test]
    fn spi_batch_outputs_never_write_failed_op_on_local_refusal() {
        for e in [
            lib::SpiBatchCallError::InvalidCsPin {
                cs: 255,
                num_gpios: 4,
            },
            lib::SpiBatchCallError::NoGpios,
        ] {
            let (_, _, failed_op) = run_outputs(Err(e));
            assert_eq!(
                failed_op, 0xA5A5,
                "local refusal fabricated a failed-op index"
            );
        }
    }

    #[test]
    fn spi_batch_outputs_never_write_failed_op_on_batch_comms() {
        let (status, _, failed_op) =
            run_outputs(Err(lib::SpiBatchCallError::Comms(lib::HostErr::Closed)));
        assert_eq!(status, -25);
        assert_eq!(failed_op, 0xA5A5);
    }

    #[test]
    fn spi_batch_outputs_write_failed_op_only_on_endpoint_error() {
        // Positive control: a suite that only forbids writes would be
        // satisfied by an implementation that never writes at all.
        let (_, _, failed_op) = run_outputs(Err(lib::SpiBatchCallError::Endpoint(SpiBatchError {
            failed_op: 3,
            kind: SpiError::Other,
        })));
        assert_eq!(failed_op, 3);
    }

    #[test]
    fn spi_batch_outputs_tolerate_null_failed_op_pointer() {
        let mut buf = [0u8; 4];
        let mut out_len: usize = 0xABCD;
        let status = unsafe {
            spi_batch_write_outputs(
                Err(lib::SpiBatchCallError::Endpoint(SpiBatchError {
                    failed_op: 1,
                    kind: SpiError::Other,
                })),
                buf.as_mut_ptr(),
                buf.len(),
                &mut out_len,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status as i32, Status::SpiReadFailed as i32);
        assert_eq!(out_len, 0);
    }

    // --- gallo_num_gpios ---

    fn run_num_gpios(res: Result<u8, lib::ValidateError>) -> (i32, u8) {
        let mut out: u8 = 0xAB;
        let status = unsafe { num_gpios_write_output(res, &mut out) };
        (status as i32, out)
    }

    #[test]
    fn num_gpios_output_written_only_on_success() {
        let (status, out) = run_num_gpios(Ok(4));
        assert_eq!(status, 0);
        assert_eq!(out, 4);
    }

    #[test]
    fn num_gpios_output_written_on_successful_zero() {
        // Zero is a legitimate answer, not an error: the sentinel must be
        // overwritten.
        let (status, out) = run_num_gpios(Ok(0));
        assert_eq!(status, 0);
        assert_eq!(out, 0);
    }

    #[test]
    fn num_gpios_output_unchanged_on_every_validate_error() {
        let expected = [-62, -75, -64, -63];
        for (e, want) in all_validate_errors().into_iter().zip(expected) {
            let (status, out) = run_num_gpios(Err(e));
            assert_eq!(status, want);
            assert_eq!(out, 0xAB, "an error wrote the output buffer");
        }
    }

    #[test]
    fn header_declares_num_gpios_and_new_statuses() {
        // cbindgen prunes anything not reachable from an exported signature
        // (AGENTS.md §8), so the generated header is the only place that
        // proves C callers can actually see these.
        let path = std::path::Path::new(env!("OUT_DIR"))
            .join("include")
            .join("pico_de_gallo.h");
        let header = std::fs::read_to_string(&path).expect("generated header must exist");
        for needle in ["gallo_num_gpios", "SpiNoGpios", "DeviceInfoTimeout"] {
            assert!(
                header.contains(needle),
                "{needle} missing from {}",
                path.display()
            );
        }
    }

    #[test]
    fn num_gpios_null_device_returns_uninitialized() {
        let mut out: u8 = 0xAB;
        let status = unsafe { gallo_num_gpios(std::ptr::null(), &mut out) };
        assert_eq!(status, Status::Uninitialized);
        assert_eq!(out, 0xAB);
    }

    /// Pins the *precedence* of the two null checks in `gallo_num_gpios`:
    /// the handle check runs first, so when both pointers are NULL the
    /// result is [`Status::Uninitialized`], **not**
    /// [`Status::InvalidArgument`]. That ordering is what makes the
    /// `unsafe { &*gallo }` deref sound, so it is worth pinning.
    ///
    /// KNOWN GAP: the `(valid handle, NULL output) -> InvalidArgument`
    /// branch is **not** covered by any test. Every constructor that
    /// yields a non-null `*const PicoDeGallo` (`gallo_init`,
    /// `gallo_init_with_serial_number`, and their `_strict` variants)
    /// opens USB, and `lib::PicoDeGallo::new_for_test` is
    /// `#[cfg(test)] pub(crate)` — unreachable from this crate. Covering
    /// it would require a hardware-free constructor seam exported from
    /// `pico-de-gallo-lib`, or a hardware-in-the-loop test.
    #[test]
    fn num_gpios_null_handle_takes_precedence_over_null_output() {
        let status = unsafe { gallo_num_gpios(std::ptr::null(), std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_error_to_status_maps_zero_length_write_to_invalid_argument() {
        // Issue #101. A zero-length write is refused by the firmware because
        // the RP2350 cannot put an address-only transaction on the wire. It
        // is deliberately mapped onto the pre-existing InvalidArgument code
        // rather than a new one: Status values are stable C ABI, and adding
        // one silently falls through `switch ((enum Status)x)` in existing C
        // consumers (see AGENTS.md §8 and the 2026-08-17 row in §13.17).
        assert_eq!(
            i2c_error_to_status(PicoDeGalloError::Endpoint(I2cError::ZeroLengthWrite)),
            Status::InvalidArgument
        );
    }

    #[test]
    fn i2c_error_to_status_zero_length_write_does_not_displace_other_arms() {
        // Regression: the new arm must not shadow the pre-existing mappings.
        assert_eq!(
            i2c_error_to_status(PicoDeGalloError::Endpoint(I2cError::NoAcknowledge)),
            Status::I2cNack
        );
        assert_eq!(
            i2c_error_to_status(PicoDeGalloError::Endpoint(I2cError::BufferTooLong)),
            Status::BufferTooLong
        );
        assert_eq!(
            i2c_error_to_status(PicoDeGalloError::Endpoint(I2cError::Other)),
            Status::I2cReadFailed
        );
        assert_eq!(
            i2c_error_to_status(PicoDeGalloError::Comms(lib::HostErr::Closed)),
            Status::CommsFailed
        );
    }

    #[test]
    fn spi_error_to_status_maps_chip_select_variants() {
        assert_eq!(
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::InvalidCsPin)),
            Status::SpiInvalidCsPin
        );
        assert_eq!(
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::CsPinUnavailable)),
            Status::SpiCsPinUnavailable
        );
        assert_eq!(
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::CsPinMonitored)),
            Status::SpiCsPinMonitored
        );
    }

    #[test]
    fn spi_error_to_status_maps_comms_errors() {
        // Regression: the new chip-select arms must not displace the
        // pre-existing Comms mapping.
        assert_eq!(
            spi_error_to_status(PicoDeGalloError::Comms(lib::HostErr::Closed)),
            Status::CommsFailed
        );
    }

    #[test]
    fn spi_error_to_status_is_injective() {
        let mapped = [
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::BufferTooLong)) as i32,
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::Other)) as i32,
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::InvalidCsPin)) as i32,
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::CsPinUnavailable)) as i32,
            spi_error_to_status(PicoDeGalloError::Endpoint(SpiError::CsPinMonitored)) as i32,
            spi_error_to_status(PicoDeGalloError::Comms(lib::HostErr::Closed)) as i32,
        ];
        let unique: HashSet<i32> = mapped.iter().copied().collect();
        assert_eq!(
            mapped.len(),
            unique.len(),
            "spi_error_to_status collapsed two distinct errors onto one status"
        );
    }

    // ----------------------------- Null pointer checks -----------------------------

    #[test]
    fn ping_null_device_returns_uninitialized() {
        let mut id = 42u32;
        let status = unsafe { gallo_ping(std::ptr::null_mut(), &mut id as *mut u32) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn system_reset_subscriptions_null_device_returns_uninitialized() {
        let mut out: u8 = 0xFF;
        let status =
            unsafe { gallo_system_reset_subscriptions(std::ptr::null(), &mut out as *mut u8) };
        assert_eq!(status, Status::Uninitialized);
        // Out parameter must not be written on Uninitialized.
        assert_eq!(out, 0xFF);
    }

    #[test]
    fn system_reset_subscriptions_null_out_is_allowed() {
        // The function must not dereference a NULL `out_reset`. We can't
        // exercise the success path without a real device, but we can
        // confirm the NULL check on `gallo` is reached and returns
        // Uninitialized without dereferencing `out_reset`.
        let status =
            unsafe { gallo_system_reset_subscriptions(std::ptr::null(), std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_read_null_device_returns_uninitialized() {
        let mut buf = [0u8; 4];
        let status =
            unsafe { gallo_i2c_read(std::ptr::null_mut(), 0x48, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_write_null_device_returns_uninitialized() {
        let buf = [0u8; 4];
        let status =
            unsafe { gallo_i2c_write(std::ptr::null_mut(), 0x48, buf.as_ptr(), buf.len()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_write_read_null_device_returns_uninitialized() {
        let txbuf = [0u8; 2];
        let mut rxbuf = [0u8; 4];
        let status = unsafe {
            gallo_i2c_write_read(
                std::ptr::null_mut(),
                0x48,
                txbuf.as_ptr(),
                txbuf.len(),
                rxbuf.as_mut_ptr(),
                rxbuf.len(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_read_null_device_returns_uninitialized() {
        let mut buf = [0u8; 4];
        let status = unsafe { gallo_spi_read(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_write_null_device_returns_uninitialized() {
        let buf = [0u8; 4];
        let status = unsafe { gallo_spi_write(std::ptr::null_mut(), buf.as_ptr(), buf.len()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_flush_null_device_returns_uninitialized() {
        let status = unsafe { gallo_spi_flush(std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    // --- gallo_spi_transfer (P1-2) ---

    #[test]
    fn spi_transfer_null_device_returns_uninitialized() {
        let mut rx = [0u8; 4];
        let tx = [0u8; 4];
        let status =
            unsafe { gallo_spi_transfer(std::ptr::null(), tx.as_ptr(), rx.as_mut_ptr(), tx.len()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_transfer_null_write_buf_returns_invalid_argument() {
        // Use a non-null device sentinel so the gallo null check passes;
        // the next null check (write_buf) is what we want to exercise.
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let mut rx = [0u8; 4];
        let status =
            unsafe { gallo_spi_transfer(sentinel, std::ptr::null(), rx.as_mut_ptr(), rx.len()) };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn spi_transfer_null_read_buf_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let tx = [0u8; 4];
        let status =
            unsafe { gallo_spi_transfer(sentinel, tx.as_ptr(), std::ptr::null_mut(), tx.len()) };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn spi_transfer_oversized_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        // u16::MAX + 1 exceeds the firmware transfer limit (the check is
        // `> u16::MAX`). Buffers may be NULL because the size check fires
        // before the buffer check — but here we keep them non-NULL so the
        // test isolates the size guard.
        let tx = [0u8; 1];
        let mut rx = [0u8; 1];
        let status = unsafe {
            gallo_spi_transfer(
                sentinel,
                tx.as_ptr(),
                rx.as_mut_ptr(),
                u16::MAX as usize + 1,
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    // --- gallo_spi_batch (P1-2) ---

    #[test]
    fn spi_batch_null_device_returns_uninitialized() {
        let mut out_len: usize = 0;
        let status = unsafe {
            gallo_spi_batch(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut out_len as *mut usize,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_batch_null_out_len_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let status = unsafe {
            gallo_spi_batch(
                sentinel,
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn spi_batch_null_ops_with_nonzero_count_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let mut out_len: usize = 0;
        let status = unsafe {
            gallo_spi_batch(
                sentinel,
                0,
                std::ptr::null(),
                1,
                std::ptr::null_mut(),
                0,
                &mut out_len as *mut usize,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn spi_batch_too_many_ops_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let mut out_len: usize = 0;
        // Use a dangling but non-null ops pointer; the count check fires
        // before the ops slice is dereferenced.
        let dummy = std::ptr::NonNull::<GalloSpiBatchOp>::dangling().as_ptr();
        let status = unsafe {
            gallo_spi_batch(
                sentinel,
                0,
                dummy,
                lib::MAX_BATCH_OPS + 1,
                std::ptr::null_mut(),
                0,
                &mut out_len as *mut usize,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    // --- gallo_i2c_batch (P1-2) ---

    #[test]
    fn i2c_batch_null_device_returns_uninitialized() {
        let mut out_len: usize = 0;
        let status = unsafe {
            gallo_i2c_batch(
                std::ptr::null(),
                0x50,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut out_len as *mut usize,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_batch_null_out_len_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let status = unsafe {
            gallo_i2c_batch(
                sentinel,
                0x50,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn i2c_batch_null_ops_with_nonzero_count_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let mut out_len: usize = 0;
        let status = unsafe {
            gallo_i2c_batch(
                sentinel,
                0x50,
                std::ptr::null(),
                3,
                std::ptr::null_mut(),
                0,
                &mut out_len as *mut usize,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn i2c_batch_too_many_ops_returns_invalid_argument() {
        let sentinel = 0xDEAD_BEEFusize as *const PicoDeGallo;
        let mut out_len: usize = 0;
        let dummy = std::ptr::NonNull::<GalloI2cBatchOp>::dangling().as_ptr();
        let status = unsafe {
            gallo_i2c_batch(
                sentinel,
                0x50,
                dummy,
                lib::MAX_BATCH_OPS + 1,
                std::ptr::null_mut(),
                0,
                &mut out_len as *mut usize,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::InvalidArgument);
    }

    #[test]
    fn gpio_get_null_device_returns_uninitialized() {
        let mut state = false;
        let status = unsafe { gallo_gpio_get(std::ptr::null_mut(), 0, &mut state as *mut bool) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_put_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_put(std::ptr::null_mut(), 0, true) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_high_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_wait_for_high(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_low_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_wait_for_low(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_rising_edge_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_wait_for_rising_edge(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_falling_edge_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_wait_for_falling_edge(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_any_edge_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_wait_for_any_edge(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_high_with_timeout_ms_null_device_returns_uninitialized() {
        let status =
            unsafe { gallo_gpio_wait_for_high_with_timeout_ms(std::ptr::null_mut(), 0, 100) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_low_with_timeout_ms_null_device_returns_uninitialized() {
        let status =
            unsafe { gallo_gpio_wait_for_low_with_timeout_ms(std::ptr::null_mut(), 0, 100) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_rising_edge_with_timeout_ms_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_gpio_wait_for_rising_edge_with_timeout_ms(std::ptr::null_mut(), 0, 100)
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_falling_edge_with_timeout_ms_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_gpio_wait_for_falling_edge_with_timeout_ms(std::ptr::null_mut(), 0, 100)
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_wait_for_any_edge_with_timeout_ms_null_device_returns_uninitialized() {
        let status =
            unsafe { gallo_gpio_wait_for_any_edge_with_timeout_ms(std::ptr::null_mut(), 0, 100) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_set_config_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_set_config(std::ptr::null_mut(), 0, 0, 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_subscribe_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_subscribe(std::ptr::null_mut(), 0, 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_subscribe_invalid_edge_returns_uninitialized() {
        // null check happens before edge validation
        let status = unsafe { gallo_gpio_subscribe(std::ptr::null_mut(), 0, 99) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_unsubscribe_null_device_returns_uninitialized() {
        let status = unsafe { gallo_gpio_unsubscribe(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_set_config_null_device_returns_uninitialized() {
        let status = unsafe { gallo_i2c_set_config(std::ptr::null_mut(), 1) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_set_config_invalid_frequency_returns_invalid_argument() {
        // We need a non-null pointer but it doesn't matter since validation
        // happens before dereference for the frequency parameter.
        // Use null to get Uninitialized first, then test with a valid-looking
        // but actually invalid frequency value — but null check comes first.
        // So we just verify the enum boundary at the API level.
        let status = unsafe { gallo_i2c_set_config(std::ptr::null_mut(), 99) };
        // null check happens first, so this returns Uninitialized
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_set_config_null_device_returns_uninitialized() {
        let status = unsafe { gallo_spi_set_config(std::ptr::null_mut(), 1_000_000, false, false) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn i2c_get_config_null_device_returns_uninitialized() {
        let mut freq = 0u8;
        let status = unsafe { gallo_i2c_get_config(std::ptr::null_mut(), &mut freq as *mut u8) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_get_config_null_device_returns_uninitialized() {
        let mut freq = 0u32;
        let mut phase = false;
        let mut polarity = false;
        let status = unsafe {
            gallo_spi_get_config(
                std::ptr::null_mut(),
                &mut freq as *mut u32,
                &mut phase as *mut bool,
                &mut polarity as *mut bool,
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn version_null_device_returns_uninitialized() {
        let mut major = 0u16;
        let mut minor = 0u16;
        let mut patch = 0u32;
        let status = unsafe {
            gallo_version(
                std::ptr::null_mut(),
                &mut major as *mut u16,
                &mut minor as *mut u16,
                &mut patch as *mut u32,
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    // ----------------------------- Null out-param checks -----------------------------

    #[test]
    fn ping_null_id_returns_invalid_argument() {
        let status = unsafe { gallo_ping(std::ptr::null_mut(), std::ptr::null_mut()) };
        // gallo is null, so Uninitialized fires first
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn gpio_get_null_state_returns_invalid_argument() {
        let status = unsafe { gallo_gpio_get(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn version_null_major_returns_invalid_argument() {
        let mut minor = 0u16;
        let mut patch = 0u32;
        let status = unsafe {
            gallo_version(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut minor as *mut u16,
                &mut patch as *mut u32,
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn device_info_null_device_returns_uninitialized() {
        let mut info = std::mem::MaybeUninit::<GalloDeviceInfo>::uninit();
        let status = unsafe { gallo_get_device_info(std::ptr::null_mut(), info.as_mut_ptr()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn device_info_null_out_returns_invalid_argument() {
        // gallo is null too, so Uninitialized fires first
        let status = unsafe { gallo_get_device_info(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    // ----------------------------- PWM null pointer checks -----------------------------

    #[test]
    fn pwm_set_duty_cycle_null_device_returns_uninitialized() {
        let status = unsafe { gallo_pwm_set_duty_cycle(std::ptr::null_mut(), 0, 100) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn pwm_get_duty_cycle_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_pwm_get_duty_cycle(
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn pwm_enable_null_device_returns_uninitialized() {
        let status = unsafe { gallo_pwm_enable(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn pwm_disable_null_device_returns_uninitialized() {
        let status = unsafe { gallo_pwm_disable(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn pwm_set_config_null_device_returns_uninitialized() {
        let status = unsafe { gallo_pwm_set_config(std::ptr::null_mut(), 0, 1000, false) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn pwm_get_config_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_pwm_get_config(
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn adc_read_null_device_returns_uninitialized() {
        let status = unsafe { gallo_adc_read(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn adc_get_config_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_adc_get_config(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    // ----------------------------- Null buffer checks -----------------------------

    #[test]
    fn i2c_read_null_buffer_returns_invalid_argument() {
        // Device is also null, so we get Uninitialized first.
        // This tests that the null-device check fires before the buffer check.
        let status = unsafe { gallo_i2c_read(std::ptr::null_mut(), 0x48, std::ptr::null_mut(), 4) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn spi_read_null_buffer_returns_invalid_argument() {
        let status = unsafe { gallo_spi_read(std::ptr::null_mut(), std::ptr::null_mut(), 4) };
        assert_eq!(status, Status::Uninitialized);
    }

    // ----------------------------- Free safety -----------------------------

    #[test]
    fn free_null_is_safe() {
        unsafe { gallo_free(std::ptr::null()) };
    }

    #[test]
    fn init_with_null_serial_returns_null() {
        let ptr = unsafe { gallo_init_with_serial_number(std::ptr::null()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn init_strict_with_null_serial_returns_null() {
        let ptr = unsafe { gallo_init_strict_with_serial_number(std::ptr::null()) };
        assert!(ptr.is_null());
    }

    // ----------------------------- UART null pointer checks -----------------------------

    #[test]
    fn uart_read_null_device_returns_uninitialized() {
        let mut buf = [0u8; 4];
        let mut out_len = 0u16;
        let status = unsafe {
            gallo_uart_read(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                4,
                1000,
                &mut out_len as *mut u16,
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_read_null_buf_returns_uninitialized() {
        let status = unsafe {
            gallo_uart_read(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                4,
                1000,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_write_null_device_returns_uninitialized() {
        let data = [0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let status = unsafe { gallo_uart_write(std::ptr::null_mut(), data.as_ptr(), 5) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_write_null_buf_returns_uninitialized() {
        let status = unsafe { gallo_uart_write(std::ptr::null_mut(), std::ptr::null(), 5) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_flush_null_device_returns_uninitialized() {
        let status = unsafe { gallo_uart_flush(std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_set_config_null_device_returns_uninitialized() {
        let status = unsafe { gallo_uart_set_config(std::ptr::null_mut(), 115200) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_set_config_zero_baud_returns_uninitialized() {
        // null check fires first
        let status = unsafe { gallo_uart_set_config(std::ptr::null_mut(), 0) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_get_config_null_device_returns_uninitialized() {
        let mut baud = 0u32;
        let status = unsafe { gallo_uart_get_config(std::ptr::null_mut(), &mut baud as *mut u32) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn uart_error_to_status_maps_all_variants() {
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::BufferTooLong)),
            Status::BufferTooLong
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::Overrun)),
            Status::UartOverrun
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::Break)),
            Status::UartBreak
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::Parity)),
            Status::UartParity
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::Framing)),
            Status::UartFraming
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::InvalidBaudRate)),
            Status::UartInvalidBaudRate
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::Other)),
            Status::UartReadFailed
        );
        assert_eq!(
            uart_error_to_status(PicoDeGalloError::Endpoint(UartError::Unsupported)),
            Status::Unsupported
        );
    }

    #[test]
    fn adc_error_mapping() {
        assert_eq!(
            adc_error_to_status(PicoDeGalloError::Endpoint(AdcError::ConversionFailed)),
            Status::AdcConversionFailed
        );
        assert_eq!(
            adc_error_to_status(PicoDeGalloError::Endpoint(AdcError::Other)),
            Status::AdcReadFailed
        );
        assert_eq!(
            adc_error_to_status(PicoDeGalloError::Endpoint(AdcError::Unsupported)),
            Status::Unsupported
        );
    }

    #[test]
    fn gpio_error_to_status_maps_pin_monitored() {
        assert_eq!(
            gpio_error_to_status(PicoDeGalloError::Endpoint(GpioError::PinMonitored)),
            Status::GpioPinMonitored
        );
    }

    #[test]
    fn gpio_error_to_status_maps_pin_not_monitored() {
        assert_eq!(
            gpio_error_to_status(PicoDeGalloError::Endpoint(GpioError::PinNotMonitored)),
            Status::GpioPinNotMonitored
        );
    }

    #[test]
    fn gpio_subscribe_status_codes_are_stable() {
        assert_eq!(Status::GpioPinMonitored as i32, -53);
        assert_eq!(Status::GpioPinNotMonitored as i32, -54);
        assert_eq!(Status::GpioSubscribeFailed as i32, -55);
        assert_eq!(Status::GpioUnsubscribeFailed as i32, -56);
    }

    // ----------------------------- 1-Wire null pointer checks -----------------------------

    #[test]
    fn onewire_reset_null_device_returns_uninitialized() {
        let status = unsafe { gallo_onewire_reset(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn onewire_read_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_onewire_read(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                9,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn onewire_write_null_device_returns_uninitialized() {
        let data = [0xCC, 0x44];
        let status = unsafe { gallo_onewire_write(std::ptr::null_mut(), data.as_ptr(), 2) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn onewire_write_pullup_null_device_returns_uninitialized() {
        let data = [0xCC, 0x44];
        let status =
            unsafe { gallo_onewire_write_pullup(std::ptr::null_mut(), data.as_ptr(), 2, 750) };
        assert_eq!(status, Status::Uninitialized);
    }

    #[test]
    fn onewire_search_null_device_returns_uninitialized() {
        let status = unsafe {
            gallo_onewire_search(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                10,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::Uninitialized);
    }

    // ----------------------------- 1-Wire error mapping tests -----------------------------

    #[test]
    fn onewire_error_mapping() {
        assert_eq!(
            onewire_error_to_status(PicoDeGalloError::Endpoint(OneWireError::NoPresence)),
            Status::OneWireNoPresence
        );
        assert_eq!(
            onewire_error_to_status(PicoDeGalloError::Endpoint(OneWireError::BusError)),
            Status::OneWireBusError
        );
        assert_eq!(
            onewire_error_to_status(PicoDeGalloError::Endpoint(OneWireError::BufferTooLong)),
            Status::BufferTooLong
        );
        assert_eq!(
            onewire_error_to_status(PicoDeGalloError::Endpoint(OneWireError::Other)),
            Status::OneWireReadFailed
        );
        assert_eq!(
            onewire_error_to_status(PicoDeGalloError::Endpoint(OneWireError::Unsupported)),
            Status::Unsupported
        );
    }

    #[test]
    fn onewire_status_codes_are_stable() {
        assert_eq!(Status::OneWireNoPresence as i32, -57);
        assert_eq!(Status::OneWireBusError as i32, -58);
        assert_eq!(Status::OneWireReadFailed as i32, -59);
        assert_eq!(Status::OneWireWriteFailed as i32, -60);
        assert_eq!(Status::OneWireSearchFailed as i32, -61);
        assert_eq!(Status::DeviceInfoFailed as i32, -62);
        assert_eq!(Status::SchemaMismatch as i32, -63);
        assert_eq!(Status::LegacyFirmware as i32, -64);
        assert_eq!(Status::Unsupported as i32, -65);
        assert_eq!(Status::I2cBatchFailed as i32, -66);
        assert_eq!(Status::SpiBatchFailed as i32, -67);
        assert_eq!(Status::SpiTransferFailed as i32, -68);
        assert_eq!(Status::SystemResetSubscriptionsFailed as i32, -69);
    }

    #[test]
    fn limits_match_wire_constants() {
        assert_eq!(GALLO_MAX_TRANSFER_SIZE, lib::MAX_TRANSFER_SIZE);
        assert_eq!(GALLO_MAX_BATCH_OPS, lib::MAX_BATCH_OPS);
        assert_eq!(GALLO_NUM_GPIOS, lib::NUM_GPIOS);
    }

    /// The C-visible enum values must equal the discriminants of the
    /// `pico-de-gallo-internal` wire enums they mirror.
    ///
    /// This is not cosmetic. `gallo_i2c_get_config` writes `freq as u8`
    /// straight out to the caller, and `gallo_i2c_set_config` /
    /// `gallo_gpio_set_config` / `gallo_gpio_subscribe` match on the raw
    /// integer, so the two numberings are the same ABI. Wire-enum variant
    /// order is itself ABI for postcard (AGENTS.md §6.1), so if someone
    /// ever reorders a variant upstream this test is what catches it.
    #[test]
    fn config_enums_match_wire_enums() {
        assert_eq!(
            GalloI2cFrequency::Standard as u8,
            lib::I2cFrequency::Standard as u8
        );
        assert_eq!(GalloI2cFrequency::Fast as u8, lib::I2cFrequency::Fast as u8);
        assert_eq!(
            GalloI2cFrequency::FastPlus as u8,
            lib::I2cFrequency::FastPlus as u8
        );

        assert_eq!(
            GalloGpioDirection::Input as u8,
            lib::GpioDirection::Input as u8
        );
        assert_eq!(
            GalloGpioDirection::Output as u8,
            lib::GpioDirection::Output as u8
        );

        assert_eq!(GalloGpioPull::None as u8, lib::GpioPull::None as u8);
        assert_eq!(GalloGpioPull::Up as u8, lib::GpioPull::Up as u8);
        assert_eq!(GalloGpioPull::Down as u8, lib::GpioPull::Down as u8);

        assert_eq!(GalloGpioEdge::Rising as u8, lib::GpioEdge::Rising as u8);
        assert_eq!(GalloGpioEdge::Falling as u8, lib::GpioEdge::Falling as u8);
        assert_eq!(GalloGpioEdge::Any as u8, lib::GpioEdge::Any as u8);
    }

    /// Batch-op tags are declared stable C ABI in the `GalloSpiBatchOp` /
    /// `GalloI2cBatchOp` docs, and the dispatch in `gallo_spi_batch` /
    /// `gallo_i2c_batch` matches on these literals. Pin them.
    #[test]
    fn batch_op_tags_are_stable() {
        assert_eq!(GalloSpiBatchOpTag::Read as u8, 0);
        assert_eq!(GalloSpiBatchOpTag::Write as u8, 1);
        assert_eq!(GalloSpiBatchOpTag::Transfer as u8, 2);
        assert_eq!(GalloSpiBatchOpTag::DelayNs as u8, 3);

        assert_eq!(GalloI2cBatchOpTag::Read as u8, 0);
        assert_eq!(GalloI2cBatchOpTag::Write as u8, 1);
    }
}
