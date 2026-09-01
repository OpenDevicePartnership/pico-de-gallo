//! Pyco de Gallo — Python bindings for [Pico de
//! Gallo](https://github.com/OpenDevicePartnership/pico-de-gallo).
//!
//! This module exposes the host-side `pico_de_gallo_lib` API to Python via
//! [PyO3](https://pyo3.rs). It supports I2C, SPI, UART, GPIO, PWM, ADC, and
//! 1-Wire operations against a Pico de Gallo USB bridge device.
//!
//! All methods are synchronous from Python's perspective: the underlying
//! async calls are driven by an internal Tokio runtime owned by each
//! `PycoDeGallo` instance. The Python GIL is released around every
//! blocking I/O call, so other Python threads keep running while a USB
//! round-trip is in flight.
//!
//! Example:
//!
//! ```python
//! import pyco_de_gallo as gallo
//!
//! dev = gallo.open()
//! v = dev.version()
//! print(f"Firmware {v.major}.{v.minor}.{v.patch}")
//!
//! dev.i2c_set_config(gallo.I2cFrequency.Fast)
//! data = dev.i2c_write_read(0x50, [0x00], 16)
//! ```
//!
//! Errors from the underlying library are surfaced as Python `RuntimeError`
//! exceptions.

// PyO3's `from_py_object` macro code clones when it can copy, making clippy complain.
#![allow(clippy::clone_on_copy)]

use std::future::Future;
use std::time::Duration;

use pico_de_gallo_lib::{
    AdcChannel, AdcConfigurationInfo as LibAdcConfigurationInfo, DeviceInfo as LibDeviceInfo,
    GpioDirection as LibGpioDirection, GpioEdge as LibGpioEdge, GpioEvent as LibGpioEvent,
    GpioPull as LibGpioPull, GpioState, I2cBatchOp as LibI2cBatchOp,
    I2cFrequency as LibI2cFrequency, MultiSubscription, PicoDeGallo,
    PwmConfigurationInfo as LibPwmConfigurationInfo, PwmDutyCycleInfo as LibPwmDutyCycleInfo,
    SpiBatchOp as LibSpiBatchOp, SpiConfigurationInfo as LibSpiConfigurationInfo,
    SpiPhase as LibSpiPhase, SpiPolarity as LibSpiPolarity,
    UartConfigurationInfo as LibUartConfigurationInfo, ValidateError as LibValidateError,
    VersionInfo as LibVersionInfo,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use tokio::runtime::{Handle, Runtime};

/// List all Pico de Gallo devices currently connected to the system.
///
/// Returns:
///     list[DeviceDescription]: One entry per matching USB device. Use the
///     ``serial_number`` field with :func:`open_with_serial_number` to connect
///     to a specific board.
#[pyfunction]
fn list_devices() -> PyResult<Vec<DeviceDescription>> {
    let list = pico_de_gallo_lib::list_devices()
        .into_iter()
        .map(|dev| DeviceDescription {
            serial_number: dev.serial_number,
            manufacturer: dev.manufacturer,
            product: dev.product,
        })
        .collect::<Vec<_>>();
    Ok(list)
}

/// Open the first Pico de Gallo device available on the system.
///
/// If multiple devices are connected, this picks the first one matched by
/// the OS USB enumeration. Use :func:`open_with_serial_number` for
/// deterministic selection.
///
/// Returns:
///     PycoDeGallo: A connected device handle.
///
/// Raises:
///     RuntimeError: If a Tokio runtime could not be created.
#[pyfunction]
fn open() -> PyResult<PycoDeGallo> {
    let runtime = Runtime::new().map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;
    let gallo = {
        let _guard = runtime.enter();
        PicoDeGallo::new()
    };
    Ok(PycoDeGallo {
        inner: gallo,
        runtime,
    })
}

/// Open the Pico de Gallo device with the given USB serial number.
///
/// Args:
///     serial_number (str): The USB serial number of the target device, as
///         reported by :func:`list_devices`.
///
/// Returns:
///     PycoDeGallo: A connected device handle.
///
/// Raises:
///     RuntimeError: If a Tokio runtime could not be created.
#[pyfunction]
fn open_with_serial_number(serial_number: &str) -> PyResult<PycoDeGallo> {
    let runtime = Runtime::new().map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;
    let gallo = {
        let _guard = runtime.enter();
        PicoDeGallo::new_with_serial_number(serial_number)
    };
    Ok(PycoDeGallo {
        inner: gallo,
        runtime,
    })
}

/// Open the first Pico de Gallo device on USB AND validate the
/// firmware's schema version before returning.
///
/// Raises `RuntimeError` on device-not-found, schema version
/// mismatch, legacy firmware, or any validation error.
///
/// Prefer in production Python code: :func:`open` is lazy and
/// surfaces failures only on the first RPC call, which is harder
/// to diagnose. ``open_strict`` fails fast with a clear exception.
///
/// Returns:
///     PycoDeGallo: A connected and validated device handle.
///
/// Raises:
///     RuntimeError: If a Tokio runtime could not be created, the
///         device is not reachable, or the firmware's schema does
///         not match.
#[pyfunction]
fn open_strict() -> PyResult<PycoDeGallo> {
    let pyco = open()?;
    pyco.runtime
        .block_on(pyco.inner.validate())
        .map_err(|e| PyRuntimeError::new_err(format!("firmware validation failed: {e}")))?;
    Ok(pyco)
}

/// Open the Pico de Gallo device with the given USB serial number
/// AND validate the firmware's schema version before returning.
///
/// See :func:`open_strict` for the rationale.
///
/// Args:
///     serial_number (str): The USB serial number of the target device.
///
/// Returns:
///     PycoDeGallo: A connected and validated device handle.
///
/// Raises:
///     RuntimeError: If a Tokio runtime could not be created, the
///         device is not reachable, or the firmware's schema does
///         not match.
#[pyfunction]
fn open_strict_with_serial_number(serial_number: &str) -> PyResult<PycoDeGallo> {
    let pyco = open_with_serial_number(serial_number)?;
    pyco.runtime
        .block_on(pyco.inner.validate())
        .map_err(|e| PyRuntimeError::new_err(format!("firmware validation failed: {e}")))?;
    Ok(pyco)
}

/// Python bindings for the Pico de Gallo USB bridge.
#[pymodule]
fn pyco_de_gallo(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(list_devices, m)?)?;
    m.add_function(wrap_pyfunction!(open, m)?)?;
    m.add_function(wrap_pyfunction!(open_with_serial_number, m)?)?;
    m.add_function(wrap_pyfunction!(open_strict, m)?)?;
    m.add_function(wrap_pyfunction!(open_strict_with_serial_number, m)?)?;

    m.add_class::<AdcConfigurationInfo>()?;
    m.add_class::<DeviceDescription>()?;
    m.add_class::<DeviceInfo>()?;
    m.add_class::<GpioDirection>()?;
    m.add_class::<GpioEdge>()?;
    m.add_class::<GpioEvent>()?;
    m.add_class::<GpioEventSubscription>()?;
    m.add_class::<GpioPull>()?;
    m.add_class::<I2cBatchOp>()?;
    m.add_class::<I2cFrequency>()?;
    m.add_class::<PwmConfigurationInfo>()?;
    m.add_class::<PwmDutyCycleInfo>()?;
    m.add_class::<PycoDeGallo>()?;
    m.add_class::<SpiBatchOp>()?;
    m.add_class::<SpiConfigurationInfo>()?;
    m.add_class::<SpiPhase>()?;
    m.add_class::<SpiPolarity>()?;
    m.add_class::<UartConfigurationInfo>()?;
    m.add_class::<VersionInfo>()?;

    Ok(())
}

/// SPI clock phase (when the bus samples data relative to the clock edge).
#[pyclass(from_py_object, eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpiPhase {
    /// Sample on the first clock transition (CPHA = 0).
    CaptureOnFirstTransition = 0,
    /// Sample on the second clock transition (CPHA = 1).
    CaptureOnSecondTransition = 1,
}

impl From<LibSpiPhase> for SpiPhase {
    fn from(p: LibSpiPhase) -> Self {
        match p {
            LibSpiPhase::CaptureOnFirstTransition => Self::CaptureOnFirstTransition,
            LibSpiPhase::CaptureOnSecondTransition => Self::CaptureOnSecondTransition,
        }
    }
}

impl From<SpiPhase> for LibSpiPhase {
    fn from(p: SpiPhase) -> Self {
        match p {
            SpiPhase::CaptureOnFirstTransition => Self::CaptureOnFirstTransition,
            SpiPhase::CaptureOnSecondTransition => Self::CaptureOnSecondTransition,
        }
    }
}

/// SPI clock polarity (the idle level of the SCK line).
#[pyclass(from_py_object, eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpiPolarity {
    /// Clock idles low (CPOL = 0).
    IdleLow = 0,
    /// Clock idles high (CPOL = 1).
    IdleHigh = 1,
}

impl From<LibSpiPolarity> for SpiPolarity {
    fn from(p: LibSpiPolarity) -> Self {
        match p {
            LibSpiPolarity::IdleLow => Self::IdleLow,
            LibSpiPolarity::IdleHigh => Self::IdleHigh,
        }
    }
}

impl From<SpiPolarity> for LibSpiPolarity {
    fn from(p: SpiPolarity) -> Self {
        match p {
            SpiPolarity::IdleLow => Self::IdleLow,
            SpiPolarity::IdleHigh => Self::IdleHigh,
        }
    }
}

/// Supported I2C bus frequencies. Discriminant values are the bus rate in Hz.
#[pyclass(from_py_object, eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum I2cFrequency {
    /// Standard mode — 100 kHz.
    Standard = 100_000,
    /// Fast mode — 400 kHz.
    Fast = 400_000,
    /// Fast-Plus mode — 1 MHz.
    FastPlus = 1_000_000,
}

impl From<LibI2cFrequency> for I2cFrequency {
    fn from(f: LibI2cFrequency) -> Self {
        match f {
            LibI2cFrequency::Standard => Self::Standard,
            LibI2cFrequency::Fast => Self::Fast,
            LibI2cFrequency::FastPlus => Self::FastPlus,
        }
    }
}

impl From<I2cFrequency> for LibI2cFrequency {
    fn from(f: I2cFrequency) -> Self {
        match f {
            I2cFrequency::Standard => Self::Standard,
            I2cFrequency::Fast => Self::Fast,
            I2cFrequency::FastPlus => Self::FastPlus,
        }
    }
}

/// A single operation in an I2C batch sequence.
///
/// Pass a list of these to :meth:`PycoDeGallo.i2c_batch` to run several
/// reads and writes as one I2C transaction in a single USB round-trip.
/// Adjacent operations of the same type are not separated by a STOP, so
/// two adjacent writes form one gather write. Zero-length writes are
/// rejected.
#[pyclass(from_py_object)]
#[derive(Clone)]
enum I2cBatchOp {
    /// Read ``read_count`` bytes from the target address.
    Read { read_count: u16 },
    /// Write ``write_data`` to the target address.
    Write { write_data: Vec<u8> },
}

impl<'a> From<&'a I2cBatchOp> for LibI2cBatchOp<'a> {
    fn from(op: &'a I2cBatchOp) -> Self {
        match op {
            I2cBatchOp::Read { read_count } => LibI2cBatchOp::Read { len: *read_count },
            I2cBatchOp::Write { write_data } => LibI2cBatchOp::Write { data: write_data },
        }
    }
}

/// A single operation in an SPI batch sequence.
///
/// Pass a list of these to :meth:`PycoDeGallo.spi_batch` to execute a
/// multi-step SPI transaction atomically under chip-select.
#[pyclass(from_py_object)]
#[derive(Clone)]
enum SpiBatchOp {
    /// Read ``read_count`` bytes from the SPI bus.
    Read { read_count: u16 },
    /// Write ``write_data`` to the SPI bus.
    Write { write_data: Vec<u8> },
    /// Full-duplex transfer: send ``write_data`` and receive the same number of bytes.
    Transfer { write_data: Vec<u8> },
    /// Insert a delay of ``duration_ns`` nanoseconds between operations.
    DelayNs { duration_ns: u32 },
}

impl<'a> From<&'a SpiBatchOp> for LibSpiBatchOp<'a> {
    fn from(op: &'a SpiBatchOp) -> Self {
        match op {
            SpiBatchOp::Read { read_count } => LibSpiBatchOp::Read { len: *read_count },
            SpiBatchOp::Write { write_data } => LibSpiBatchOp::Write { data: write_data },
            SpiBatchOp::Transfer { write_data } => LibSpiBatchOp::Transfer { data: write_data },
            SpiBatchOp::DelayNs { duration_ns } => LibSpiBatchOp::DelayNs { ns: *duration_ns },
        }
    }
}

/// Edge direction selector for :meth:`PycoDeGallo.gpio_subscribe`.
///
/// Picks which transitions on a monitored pin produce a :class:`GpioEvent`.
#[pyclass(from_py_object, eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum GpioEdge {
    /// Notify on low-to-high transitions only.
    Rising = 0,
    /// Notify on high-to-low transitions only.
    Falling = 1,
    /// Notify on any (rising or falling) transition.
    Any = 2,
}

impl From<LibGpioEdge> for GpioEdge {
    fn from(e: LibGpioEdge) -> Self {
        match e {
            LibGpioEdge::Rising => Self::Rising,
            LibGpioEdge::Falling => Self::Falling,
            LibGpioEdge::Any => Self::Any,
        }
    }
}

impl From<GpioEdge> for LibGpioEdge {
    fn from(e: GpioEdge) -> Self {
        match e {
            GpioEdge::Rising => Self::Rising,
            GpioEdge::Falling => Self::Falling,
            GpioEdge::Any => Self::Any,
        }
    }
}

/// Direction selector for :meth:`PycoDeGallo.gpio_set_config`.
///
/// Determines whether a pin drives the line (output) or samples it (input).
#[pyclass(from_py_object, eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum GpioDirection {
    /// Configure the pin as an input.
    Input = 0,
    /// Configure the pin as an output.
    Output = 1,
}

impl From<LibGpioDirection> for GpioDirection {
    fn from(d: LibGpioDirection) -> Self {
        match d {
            LibGpioDirection::Input => Self::Input,
            LibGpioDirection::Output => Self::Output,
        }
    }
}

impl From<GpioDirection> for LibGpioDirection {
    fn from(d: GpioDirection) -> Self {
        match d {
            GpioDirection::Input => Self::Input,
            GpioDirection::Output => Self::Output,
        }
    }
}

/// Internal pull resistor selector for :meth:`PycoDeGallo.gpio_set_config`.
///
/// On the RP2350, each GPIO has internal pull-up and pull-down resistors
/// that can be enabled independently to set a default state when the pin
/// is left floating.
#[pyclass(from_py_object, eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum GpioPull {
    /// No pull resistor enabled (high-impedance input when used as input).
    Disabled = 0,
    /// Internal pull-up resistor enabled.
    Up = 1,
    /// Internal pull-down resistor enabled.
    Down = 2,
}

impl From<LibGpioPull> for GpioPull {
    fn from(p: LibGpioPull) -> Self {
        match p {
            LibGpioPull::None => Self::Disabled,
            LibGpioPull::Up => Self::Up,
            LibGpioPull::Down => Self::Down,
        }
    }
}

impl From<GpioPull> for LibGpioPull {
    fn from(p: GpioPull) -> Self {
        match p {
            GpioPull::Disabled => Self::None,
            GpioPull::Up => Self::Up,
            GpioPull::Down => Self::Down,
        }
    }
}

/// USB descriptor of a connected Pico de Gallo device. Returned by
/// :func:`list_devices`.
#[pyclass]
struct DeviceDescription {
    /// USB serial number (unique per board, derived from the chip ID).
    #[pyo3(get)]
    serial_number: Option<String>,
    /// USB manufacturer string.
    #[pyo3(get)]
    manufacturer: Option<String>,
    /// USB product string.
    #[pyo3(get)]
    product: Option<String>,
}

/// Firmware version triple reported by :meth:`PycoDeGallo.version`.
#[pyclass]
struct VersionInfo {
    /// Major version.
    #[pyo3(get)]
    major: u16,
    /// Minor version.
    #[pyo3(get)]
    minor: u16,
    /// Patch version.
    #[pyo3(get)]
    patch: u32,
}

impl From<LibVersionInfo> for VersionInfo {
    fn from(v: LibVersionInfo) -> Self {
        Self {
            major: v.major,
            minor: v.minor,
            patch: v.patch,
        }
    }
}

/// Extended device information returned by :meth:`PycoDeGallo.device_info`
/// and :meth:`PycoDeGallo.validate`.
#[pyclass]
struct DeviceInfo {
    /// Firmware major version.
    #[pyo3(get)]
    fw_major: u16,
    /// Firmware minor version.
    #[pyo3(get)]
    fw_minor: u16,
    /// Firmware patch version.
    #[pyo3(get)]
    fw_patch: u32,
    /// Wire protocol schema major version.
    #[pyo3(get)]
    schema_major: u16,
    /// Wire protocol schema minor version. Pre-1.0 bumps are breaking.
    #[pyo3(get)]
    schema_minor: u16,
    /// Wire protocol schema patch version.
    #[pyo3(get)]
    schema_patch: u32,
    /// Hardware revision number.
    #[pyo3(get)]
    hw_version: u8,
    /// Bitfield of supported peripheral capabilities.
    #[pyo3(get)]
    capabilities: u64,
    /// Number of GPIO pins the device reports.
    ///
    /// This is the runtime-authoritative bound for a GPIO index and for the
    /// SPI chip-select pin of :meth:`PycoDeGallo.spi_batch`. Also available
    /// directly as :meth:`PycoDeGallo.num_gpios`.
    #[pyo3(get)]
    num_gpios: u8,
    /// Firmware build identity.
    ///
    /// The firmware's ``git describe --always --dirty --tags --match
    /// firmware-v*`` output captured at build time, or ``"unknown"`` when git
    /// was unavailable. A trailing ``-dirty`` means the firmware was built
    /// from a modified working tree.
    ///
    /// Informational only: it names the running image and never affects
    /// whether a call succeeds. Two firmware builds can report the same
    /// firmware and schema version and still behave differently, so this is
    /// the field to log when reproducing a result.
    #[pyo3(get)]
    build_id: String,
}

impl From<LibDeviceInfo> for DeviceInfo {
    fn from(info: LibDeviceInfo) -> Self {
        Self {
            fw_major: info.fw_major,
            fw_minor: info.fw_minor,
            fw_patch: info.fw_patch,
            schema_major: info.schema_major,
            schema_minor: info.schema_minor,
            schema_patch: info.schema_patch,
            hw_version: info.hw_version,
            capabilities: info.capabilities.0,
            num_gpios: info.num_gpios,
            build_id: info.build_id().to_string(),
        }
    }
}

/// Message for a failed `num_gpios` lookup.
///
/// GIL-free so the exact wording is testable without a Python interpreter.
/// Deliberately disjoint from the chip-select messages below: a failure to
/// *learn* the valid range must never read as a complaint about the
/// caller's pin (issue #104).
fn num_gpios_error_message(e: &LibValidateError) -> String {
    format!("failed to determine num_gpios: {e}")
}

/// Classify a chip-select index against a device-reported GPIO count.
///
/// `num_gpios` must come from a successful lookup; every failure path
/// returns before this is called, so the two message families never mix.
fn classify_cs(cs: u8, num_gpios: u8) -> Result<(), String> {
    if num_gpios == 0 {
        return Err("device reports num_gpios=0; no SPI chip-select pin is available".to_string());
    }
    if cs >= num_gpios {
        return Err(format!(
            "invalid SPI chip-select pin {cs}; device reports {num_gpios} \
             GPIOs (valid 0..{num_gpios})"
        ));
    }
    Ok(())
}

/// Current UART configuration returned by :meth:`PycoDeGallo.uart_get_config`.
#[pyclass]
struct UartConfigurationInfo {
    /// Active UART baud rate, in bits per second.
    #[pyo3(get)]
    baud_rate: u32,
}

impl From<LibUartConfigurationInfo> for UartConfigurationInfo {
    fn from(c: LibUartConfigurationInfo) -> Self {
        Self {
            baud_rate: c.baud_rate,
        }
    }
}

/// Current SPI configuration returned by :meth:`PycoDeGallo.spi_get_config`.
#[pyclass]
struct SpiConfigurationInfo {
    /// SPI clock frequency, in Hz.
    #[pyo3(get)]
    spi_frequency: u32,
    /// SPI clock phase (CPHA).
    #[pyo3(get)]
    spi_phase: SpiPhase,
    /// SPI clock polarity (CPOL).
    #[pyo3(get)]
    spi_polarity: SpiPolarity,
}

impl From<LibSpiConfigurationInfo> for SpiConfigurationInfo {
    fn from(c: LibSpiConfigurationInfo) -> Self {
        Self {
            spi_frequency: c.spi_frequency,
            spi_phase: c.spi_phase.into(),
            spi_polarity: c.spi_polarity.into(),
        }
    }
}

/// PWM duty-cycle information returned by :meth:`PycoDeGallo.pwm_get_duty_cycle`.
#[pyclass]
struct PwmDutyCycleInfo {
    /// Full-scale compare value (the slice's ``top`` register + 1).
    #[pyo3(get)]
    max_duty: u16,
    /// Current raw compare value, in ``0..=max_duty``.
    #[pyo3(get)]
    current_duty: u16,
}

impl From<LibPwmDutyCycleInfo> for PwmDutyCycleInfo {
    fn from(c: LibPwmDutyCycleInfo) -> Self {
        Self {
            max_duty: c.max_duty,
            current_duty: c.current_duty,
        }
    }
}

/// PWM slice configuration returned by :meth:`PycoDeGallo.pwm_get_config`.
#[pyclass]
struct PwmConfigurationInfo {
    /// Effective PWM frequency, in Hz.
    #[pyo3(get)]
    frequency_hz: u32,
    /// Whether phase-correct mode is enabled.
    #[pyo3(get)]
    phase_correct: bool,
    /// Whether the slice is currently enabled.
    #[pyo3(get)]
    enabled: bool,
}

impl From<LibPwmConfigurationInfo> for PwmConfigurationInfo {
    fn from(c: LibPwmConfigurationInfo) -> Self {
        Self {
            frequency_hz: c.frequency_hz,
            phase_correct: c.phase_correct,
            enabled: c.enabled,
        }
    }
}

/// ADC configuration returned by :meth:`PycoDeGallo.adc_get_config`.
#[pyclass]
struct AdcConfigurationInfo {
    /// ADC sample resolution, in bits (12 on RP2350).
    #[pyo3(get)]
    resolution_bits: u8,
    /// Nominal reference voltage, in millivolts.
    #[pyo3(get)]
    nominal_reference_mv: u16,
    /// Number of GPIO-routed ADC channels available.
    #[pyo3(get)]
    num_gpio_channels: u8,
}

impl From<LibAdcConfigurationInfo> for AdcConfigurationInfo {
    fn from(c: LibAdcConfigurationInfo) -> Self {
        Self {
            resolution_bits: c.resolution_bits,
            nominal_reference_mv: c.nominal_reference_mv,
            num_gpio_channels: c.num_gpio_channels,
        }
    }
}

/// A single GPIO edge event delivered by :meth:`GpioEventSubscription.poll`.
#[pyclass]
struct GpioEvent {
    /// GPIO pin (0–3) that triggered the event.
    #[pyo3(get)]
    pin: u8,
    /// Edge type that was detected.
    #[pyo3(get)]
    edge: GpioEdge,
    /// Pin level sampled immediately after the edge. May differ from the
    /// triggering edge if the input bounced.
    #[pyo3(get)]
    state: bool,
    /// Monotonic timestamp in microseconds since firmware boot.
    #[pyo3(get)]
    timestamp_us: u64,
}

impl From<LibGpioEvent> for GpioEvent {
    fn from(e: LibGpioEvent) -> Self {
        Self {
            pin: e.pin,
            edge: e.edge.into(),
            state: matches!(e.state, GpioState::High),
            timestamp_us: e.timestamp_us,
        }
    }
}

/// A live subscription to firmware-pushed GPIO edge events.
///
/// Returned by :meth:`PycoDeGallo.subscribe_gpio_events`. Use
/// :meth:`poll` to wait for individual events, or iterate over the
/// subscription to consume a stream of events::
///
///     with gallo.subscribe_gpio_events() as events:
///         gallo.gpio_subscribe(pin=8, edge=pyco_de_gallo.GpioEdge.Falling)
///         for event in events:
///             print(event.pin, event.edge, event.state, event.timestamp_us)
///
/// The subscription must be live before the firmware will buffer events;
/// any events delivered while no subscription exists are dropped.
///
/// The Python GIL is released while waiting for events, so other Python
/// threads keep running.
#[pyclass]
struct GpioEventSubscription {
    inner: Option<MultiSubscription<LibGpioEvent>>,
    handle: Handle,
}

impl GpioEventSubscription {
    fn recv_blocking(&mut self, py: Python<'_>, timeout: Option<Duration>) -> Option<GpioEvent> {
        let sub = self.inner.as_mut()?;
        let handle = self.handle.clone();
        py.detach(|| {
            handle.block_on(async {
                match timeout {
                    Some(d) => match tokio::time::timeout(d, sub.recv()).await {
                        Ok(Ok(ev)) => Some(ev.into()),
                        Ok(Err(_)) => None,
                        Err(_) => None,
                    },
                    None => match sub.recv().await {
                        Ok(ev) => Some(ev.into()),
                        Err(_) => None,
                    },
                }
            })
        })
    }
}

#[pymethods]
impl GpioEventSubscription {
    /// Wait for the next GPIO event.
    ///
    /// Args:
    ///     timeout (float | None): Maximum time to wait, in seconds. ``None``
    ///         (the default) blocks until an event arrives or the
    ///         subscription is closed.
    ///
    /// Returns:
    ///     GpioEvent | None: The next event, or ``None`` if the timeout
    ///     elapsed without an event, or if the subscription has been closed.
    #[pyo3(signature = (timeout=None))]
    fn poll(&mut self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<GpioEvent>> {
        let dur = match timeout {
            Some(t) if t.is_finite() && t >= 0.0 => Some(Duration::from_secs_f64(t)),
            Some(_) => {
                return Err(PyRuntimeError::new_err(
                    "timeout must be a non-negative finite number or None",
                ));
            }
            None => None,
        };
        Ok(self.recv_blocking(py, dur))
    }

    /// Close the subscription and release any buffered events.
    ///
    /// Subsequent calls to :meth:`poll` return ``None`` and iteration
    /// terminates. Calling :meth:`close` more than once is a no-op.
    fn close(&mut self) {
        self.inner.take();
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &mut self,
        _exc_type: Option<Py<PyAny>>,
        _exc_val: Option<Py<PyAny>>,
        _exc_tb: Option<Py<PyAny>>,
    ) -> bool {
        self.close();
        false
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<GpioEvent> {
        match self.recv_blocking(py, None) {
            Some(ev) => Ok(ev),
            None => Err(pyo3::exceptions::PyStopIteration::new_err(())),
        }
    }
}

/// A connected Pico de Gallo device handle.
///
/// Construct with :func:`open` or :func:`open_with_serial_number`. All methods
/// on this class are synchronous; the underlying async client is driven by an
/// internal Tokio runtime owned by the instance. The Python GIL is released
/// while the runtime is blocked on USB I/O so other Python threads can run.
#[pyclass]
struct PycoDeGallo {
    inner: PicoDeGallo,
    runtime: Runtime,
}

impl PycoDeGallo {
    /// Run an async future on the owned runtime, releasing the Python GIL
    /// for the duration of the wait so other Python threads can make
    /// progress.
    fn block<F>(&self, py: Python<'_>, fut: F) -> F::Output
    where
        F: Future + Send,
        F::Output: Send,
    {
        py.detach(|| self.runtime.block_on(fut))
    }
}

#[pymethods]
impl PycoDeGallo {
    /// Block until the underlying USB connection is closed.
    fn wait_closed(&self, py: Python<'_>) -> PyResult<()> {
        self.block(py, self.inner.wait_closed());
        Ok(())
    }

    /// Echo a 32-bit integer back from the firmware.
    ///
    /// Useful for testing connectivity. The firmware returns the same value
    /// that was sent.
    ///
    /// Args:
    ///     id (int): Arbitrary 32-bit unsigned integer.
    ///
    /// Returns:
    ///     int: The same integer the firmware received.
    fn ping(&self, py: Python<'_>, id: u32) -> PyResult<u32> {
        self.block(py, self.inner.ping(id))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Read ``count`` bytes from the I2C device at ``address``.
    ///
    /// The firmware buffer is limited to 4096 bytes; reads exceeding this
    /// limit will be truncated.
    ///
    /// Args:
    ///     address (int): 7-bit I2C target address.
    ///     count (int): Number of bytes to read.
    ///
    /// Returns:
    ///     bytes: The bytes read from the bus.
    fn i2c_read(&self, py: Python<'_>, address: u8, count: u16) -> PyResult<Vec<u8>> {
        self.block(py, self.inner.i2c_read(address, count))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Write ``data`` to the I2C device at ``address``.
    ///
    /// Args:
    ///     address (int): 7-bit I2C target address.
    ///     data (bytes | list[int]): Bytes to write.
    fn i2c_write(&self, py: Python<'_>, address: u8, data: Vec<u8>) -> PyResult<()> {
        self.block(py, self.inner.i2c_write(address, &data))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Write ``write_data`` to the I2C device at ``address``, then read
    /// back ``read_count`` bytes in a single transaction (without releasing
    /// the bus).
    ///
    /// The firmware buffer is limited to 4096 bytes; reads exceeding this
    /// limit will be truncated.
    ///
    /// Args:
    ///     address (int): 7-bit I2C target address.
    ///     write_data (bytes | list[int]): Bytes to write first.
    ///     read_count (int): Number of bytes to read back.
    ///
    /// Returns:
    ///     bytes: The bytes read from the bus.
    fn i2c_write_read(
        &self,
        py: Python<'_>,
        address: u8,
        write_data: Vec<u8>,
        read_count: u16,
    ) -> PyResult<Vec<u8>> {
        self.block(
            py,
            self.inner.i2c_write_read(address, &write_data, read_count),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Scan the I2C bus and return the addresses of all responding devices.
    ///
    /// Probes each 7-bit address with a 1-byte read. Addresses that ACK are
    /// returned in ascending order.
    ///
    /// Args:
    ///     include_reserved (bool): If False (default-equivalent), only the
    ///         standard range ``0x08..=0x77`` is probed. If True, the full
    ///         range ``0x00..=0x7F`` is scanned.
    ///
    /// Returns:
    ///     list[int]: 7-bit addresses that responded, ascending.
    fn i2c_scan(&self, py: Python<'_>, include_reserved: bool) -> PyResult<Vec<u8>> {
        self.block(py, self.inner.i2c_scan(include_reserved))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Execute a batch of I2C operations as a single transaction.
    ///
    /// The whole batch is one I2C transaction: a START and the address
    /// precede the first operation, adjacent operations of the same type
    /// are sent back to back with no STOP and no repeated START between
    /// them, a direction change emits a repeated START and re-addresses
    /// the target, and only the last operation is followed by a STOP. Two
    /// adjacent writes therefore form a single gather write, which is the
    /// usual register-address-then-payload idiom — unlike
    /// :meth:`i2c_write_read`, which is fixed at one write followed by one
    /// read.
    ///
    /// Requires firmware built from schema 0.7 or newer. Older firmware
    /// executes each operation as its own transaction.
    ///
    /// Args:
    ///     address (int): 7-bit I2C target address.
    ///     ops (list[I2cBatchOp]): Sequence of read/write operations,
    ///         executed in order within one transaction.
    ///
    /// Returns:
    ///     bytes: Concatenated data from every ``Read`` operation, in order.
    ///
    /// Raises:
    ///     RuntimeError: On a bus failure, or if any ``Write`` operation is
    ///         zero-length. The whole batch is validated before anything is
    ///         driven onto the bus, so a rejected batch performs no I2C
    ///         traffic at all.
    fn i2c_batch(&self, py: Python<'_>, address: u8, ops: Vec<I2cBatchOp>) -> PyResult<Vec<u8>> {
        let lib_ops: Vec<LibI2cBatchOp<'_>> = ops.iter().map(Into::into).collect();
        self.block(py, self.inner.i2c_batch(address, &lib_ops))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Read ``count`` bytes from the SPI bus.
    ///
    /// The firmware buffer is limited to 4096 bytes; reads exceeding this
    /// limit will be truncated.
    fn spi_read(&self, py: Python<'_>, count: u16) -> PyResult<Vec<u8>> {
        self.block(py, self.inner.spi_read(count))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Write ``data`` to the SPI bus.
    fn spi_write(&self, py: Python<'_>, data: Vec<u8>) -> PyResult<()> {
        self.block(py, self.inner.spi_write(&data))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Flush the SPI interface.
    fn spi_flush(&self, py: Python<'_>) -> PyResult<()> {
        self.block(py, self.inner.spi_flush())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Perform a full-duplex SPI transfer.
    ///
    /// Simultaneously sends ``write_data`` and receives the same number of
    /// bytes. The firmware buffer is limited to 4096 bytes; transfers
    /// exceeding this limit will be rejected.
    ///
    /// Returns:
    ///     bytes: Bytes received during the transfer.
    fn spi_transfer(&self, py: Python<'_>, write_data: Vec<u8>) -> PyResult<Vec<u8>> {
        self.block(py, self.inner.spi_transfer(&write_data))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Execute a batch of SPI operations atomically under chip-select.
    ///
    /// The firmware asserts CS on ``cs_pin`` before the first operation and
    /// deasserts it after the last (or on error).
    ///
    /// ``cs_pin`` is checked against the device-reported GPIO count (see
    /// :meth:`num_gpios`) before anything is transmitted, so a bad
    /// chip-select drives no pin. The first call performs one implicit
    /// ``device/info`` round-trip; afterwards the count is cached.
    ///
    /// Args:
    ///     cs_pin (int): GPIO pin number to use as chip-select. Must be in
    ///         ``0..num_gpios``. Values outside ``0..=255`` raise
    ///         ``OverflowError`` during argument extraction, before the
    ///         device is contacted at all.
    ///     ops (list[SpiBatchOp]): Sequence of read/write/transfer/delay ops.
    ///
    /// Returns:
    ///     bytes: Concatenated data from all ``Read`` and ``Transfer`` ops, in order.
    ///
    /// Raises:
    ///     RuntimeError: If the GPIO count could not be determined (the
    ///         message starts with ``failed to determine num_gpios``), if
    ///         the device reports zero GPIOs, if ``cs_pin`` is at or beyond
    ///         the reported count, or if the batch itself fails.
    fn spi_batch(&self, py: Python<'_>, cs_pin: u8, ops: Vec<SpiBatchOp>) -> PyResult<Vec<u8>> {
        // Resolve the bound before converting the operation objects: a
        // refused chip-select must cost nothing and transmit nothing.
        let num_gpios = self
            .block(py, self.inner.num_gpios())
            .map_err(|e| PyRuntimeError::new_err(num_gpios_error_message(&e)))?;
        classify_cs(cs_pin, num_gpios).map_err(PyRuntimeError::new_err)?;

        let lib_ops: Vec<LibSpiBatchOp<'_>> = ops.iter().map(Into::into).collect();
        self.block(py, self.inner.spi_batch(cs_pin, &lib_ops))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query how many GPIO pins the device reports.
    ///
    /// This is the runtime-authoritative bound for a GPIO index and for the
    /// SPI chip-select pin of :meth:`spi_batch`. The first call performs one
    /// implicit validated ``device/info`` round-trip, bounded at 300
    /// seconds; afterwards the value is cached. A reported count of zero is
    /// a legitimate answer, not an error.
    ///
    /// Note: during the current wire-schema freeze a matching reported
    /// schema version does not prove wire-*shape* compatibility. Validation
    /// bounds how long you wait and checks the reported numbers; it cannot
    /// make those numbers trustworthy.
    ///
    /// Returns:
    ///     int: The device-reported GPIO count.
    ///
    /// Raises:
    ///     RuntimeError: If the count could not be determined — transport
    ///         failure, 300-second timeout, legacy firmware, or schema
    ///         mismatch. The message starts with
    ///         ``failed to determine num_gpios``. Retrying is allowed.
    fn num_gpios(&self, py: Python<'_>) -> PyResult<u8> {
        self.block(py, self.inner.num_gpios())
            .map_err(|e| PyRuntimeError::new_err(num_gpios_error_message(&e)))
    }

    /// Read up to ``count`` bytes from the UART bus.
    ///
    /// Waits up to ``timeout_ms`` milliseconds for at least one byte. Returns
    /// whatever bytes are available (1 to ``count``), or an empty list on timeout.
    ///
    /// Args:
    ///     count (int): Maximum number of bytes to read.
    ///     timeout_ms (int): Timeout in milliseconds.
    ///
    /// Returns:
    ///     bytes: Bytes received, possibly fewer than ``count``.
    fn uart_read(&self, py: Python<'_>, count: u16, timeout_ms: u32) -> PyResult<Vec<u8>> {
        self.block(py, self.inner.uart_read(count, timeout_ms))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Write ``data`` to the UART bus.
    fn uart_write(&self, py: Python<'_>, data: Vec<u8>) -> PyResult<()> {
        self.block(py, self.inner.uart_write(&data))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Flush any buffered UART transmit data.
    fn uart_flush(&self, py: Python<'_>) -> PyResult<()> {
        self.block(py, self.inner.uart_flush())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Get the current state of the GPIO numbered by ``pin``.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// Returns:
    ///     bool: True if the pin reads high, False otherwise.
    fn gpio_get(&self, py: Python<'_>, pin: u8) -> PyResult<bool> {
        self.block(py, self.inner.gpio_get(pin))
            .map(|state| matches!(state, GpioState::High))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Drive the GPIO numbered by ``pin`` to ``state``.
    ///
    /// Pico de Gallo offers 4 total GPIOs, numbered 0 through 3.
    ///
    /// Args:
    ///     pin (int): GPIO pin number (0–3).
    ///     state (bool): True for high, False for low.
    fn gpio_put(&self, py: Python<'_>, pin: u8, state: bool) -> PyResult<()> {
        self.block(py, self.inner.gpio_put(pin, state.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until the GPIO numbered by ``pin`` reads high.
    fn gpio_wait_for_high(&self, py: Python<'_>, pin: u8) -> PyResult<()> {
        self.block(py, self.inner.gpio_wait_for_high(pin))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until the GPIO numbered by ``pin`` reads low.
    fn gpio_wait_for_low(&self, py: Python<'_>, pin: u8) -> PyResult<()> {
        self.block(py, self.inner.gpio_wait_for_low(pin))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until a rising edge is detected on the GPIO numbered by ``pin``.
    fn gpio_wait_for_rising_edge(&self, py: Python<'_>, pin: u8) -> PyResult<()> {
        self.block(py, self.inner.gpio_wait_for_rising_edge(pin))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until a falling edge is detected on the GPIO numbered by ``pin``.
    fn gpio_wait_for_falling_edge(&self, py: Python<'_>, pin: u8) -> PyResult<()> {
        self.block(py, self.inner.gpio_wait_for_falling_edge(pin))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until any (rising or falling) edge is detected on the GPIO
    /// numbered by ``pin``.
    fn gpio_wait_for_any_edge(&self, py: Python<'_>, pin: u8) -> PyResult<()> {
        self.block(py, self.inner.gpio_wait_for_any_edge(pin))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until the GPIO numbered by ``pin`` reads high, with a
    /// host-supplied timeout.
    ///
    /// Args:
    ///     pin (int): GPIO pin number (0–3).
    ///     timeout_ms (int): Timeout in milliseconds. 0 means wait
    ///         forever (equivalent to ``gpio_wait_for_high(pin)``).
    ///         Non-zero bounds the firmware-side wait.
    ///
    /// Raises:
    ///     RuntimeError: If the timeout expires (``GpioError::Timeout``)
    ///         or any other firmware error occurs.
    ///
    /// Available on firmware schema 0.7+.
    fn gpio_wait_for_high_with_timeout(
        &self,
        py: Python<'_>,
        pin: u8,
        timeout_ms: u32,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner.gpio_wait_for_high_with_timeout(
                pin,
                std::time::Duration::from_millis(timeout_ms as u64),
            ),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until the GPIO numbered by ``pin`` reads low, with a
    /// host-supplied timeout.
    ///
    /// See :py:meth:`gpio_wait_for_high_with_timeout` for argument
    /// semantics.
    fn gpio_wait_for_low_with_timeout(
        &self,
        py: Python<'_>,
        pin: u8,
        timeout_ms: u32,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner.gpio_wait_for_low_with_timeout(
                pin,
                std::time::Duration::from_millis(timeout_ms as u64),
            ),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until a rising edge is detected on the GPIO numbered by
    /// ``pin``, with a host-supplied timeout.
    ///
    /// See :py:meth:`gpio_wait_for_high_with_timeout` for argument
    /// semantics.
    fn gpio_wait_for_rising_edge_with_timeout(
        &self,
        py: Python<'_>,
        pin: u8,
        timeout_ms: u32,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner.gpio_wait_for_rising_edge_with_timeout(
                pin,
                std::time::Duration::from_millis(timeout_ms as u64),
            ),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until a falling edge is detected on the GPIO numbered by
    /// ``pin``, with a host-supplied timeout.
    ///
    /// See :py:meth:`gpio_wait_for_high_with_timeout` for argument
    /// semantics.
    fn gpio_wait_for_falling_edge_with_timeout(
        &self,
        py: Python<'_>,
        pin: u8,
        timeout_ms: u32,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner.gpio_wait_for_falling_edge_with_timeout(
                pin,
                std::time::Duration::from_millis(timeout_ms as u64),
            ),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Block until any (rising or falling) edge is detected on the GPIO
    /// numbered by ``pin``, with a host-supplied timeout.
    ///
    /// See :py:meth:`gpio_wait_for_high_with_timeout` for argument
    /// semantics.
    fn gpio_wait_for_any_edge_with_timeout(
        &self,
        py: Python<'_>,
        pin: u8,
        timeout_ms: u32,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner.gpio_wait_for_any_edge_with_timeout(
                pin,
                std::time::Duration::from_millis(timeout_ms as u64),
            ),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Configure a GPIO pin's direction and internal pull resistor.
    ///
    /// After configuration, the pin enters explicit mode: :meth:`gpio_get`
    /// and :meth:`gpio_put` will no longer auto-switch direction.
    ///
    /// Args:
    ///     pin (int): GPIO pin number (0–3).
    ///     direction (GpioDirection): Input or output.
    ///     pull (GpioPull): Internal pull resistor selection.
    fn gpio_set_config(
        &self,
        py: Python<'_>,
        pin: u8,
        direction: GpioDirection,
        pull: GpioPull,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner
                .gpio_set_config(pin, direction.into(), pull.into()),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Subscribe a GPIO pin to firmware-pushed edge events.
    ///
    /// Once subscribed, the pin is owned by the firmware monitor task and
    /// regular GPIO operations on it (:meth:`gpio_get`, :meth:`gpio_put`,
    /// :meth:`gpio_set_config`, the ``gpio_wait_for_*`` methods) will fail
    /// until :meth:`gpio_unsubscribe` is called.
    ///
    /// Events are delivered through a :class:`GpioEventSubscription` returned
    /// by :meth:`subscribe_gpio_events`. Call ``subscribe_gpio_events`` first
    /// (or in parallel) so the host has a place to buffer events.
    ///
    /// Args:
    ///     pin (int): GPIO pin number (0–3).
    ///     edge (GpioEdge): Which transitions to monitor.
    fn gpio_subscribe(&self, py: Python<'_>, pin: u8, edge: GpioEdge) -> PyResult<()> {
        self.block(py, self.inner.gpio_subscribe(pin, edge.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Stop monitoring edge events on a GPIO pin previously passed to
    /// :meth:`gpio_subscribe`.
    ///
    /// Args:
    ///     pin (int): GPIO pin number (0–3).
    fn gpio_unsubscribe(&self, py: Python<'_>, pin: u8) -> PyResult<()> {
        self.block(py, self.inner.gpio_unsubscribe(pin))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Open a subscription to firmware-pushed GPIO edge events.
    ///
    /// Returns a :class:`GpioEventSubscription` that buffers up to ``depth``
    /// events on the host. Call :meth:`gpio_subscribe` afterwards to tell
    /// the firmware which pins to monitor. The subscription should be closed
    /// by calling :meth:`GpioEventSubscription.close` (or by using the
    /// subscription as a context manager) when no longer needed.
    ///
    /// Args:
    ///     depth (int): Host-side buffer depth in events. Defaults to 16.
    ///
    /// Returns:
    ///     GpioEventSubscription: A handle that yields :class:`GpioEvent` on
    ///     :meth:`~GpioEventSubscription.poll` or iteration.
    #[pyo3(signature = (depth=16))]
    fn subscribe_gpio_events(
        &self,
        py: Python<'_>,
        depth: usize,
    ) -> PyResult<GpioEventSubscription> {
        let sub = self
            .block(py, self.inner.subscribe_gpio_events(depth))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;
        Ok(GpioEventSubscription {
            inner: Some(sub),
            handle: self.runtime.handle().clone(),
        })
    }

    /// Set the I2C bus clock frequency.
    ///
    /// Takes effect immediately before the next I2C operation.
    ///
    /// Args:
    ///     frequency_hz (I2cFrequency): One of ``Standard`` (100 kHz),
    ///         ``Fast`` (400 kHz), or ``FastPlus`` (1 MHz).
    fn i2c_set_config(&self, py: Python<'_>, frequency_hz: I2cFrequency) -> PyResult<()> {
        self.block(py, self.inner.i2c_set_config(frequency_hz.into()))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Set the SPI bus clock frequency, phase, and polarity.
    ///
    /// Takes effect immediately before the next SPI operation.
    ///
    /// Args:
    ///     frequency_hz (int): SPI clock frequency in Hz.
    ///     spi_phase (SpiPhase): Clock phase (CPHA).
    ///     spi_polarity (SpiPolarity): Clock polarity (CPOL).
    fn spi_set_config(
        &self,
        py: Python<'_>,
        frequency_hz: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner
                .spi_set_config(frequency_hz, spi_phase.into(), spi_polarity.into()),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Get the firmware version from the connected device.
    ///
    /// Returns:
    ///     VersionInfo: Major / minor / patch version triple.
    fn version(&self, py: Python<'_>) -> PyResult<VersionInfo> {
        self.block(py, self.inner.version())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query extended device information, including firmware version, wire
    /// schema version, hardware revision, peripheral capabilities, and the
    /// informational firmware build identity.
    ///
    /// Returns:
    ///     DeviceInfo: Device identity and capability snapshot.
    fn device_info(&self, py: Python<'_>) -> PyResult<DeviceInfo> {
        self.block(py, self.inner.device_info())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Tear down any GPIO subscriptions left over from a previous host
    /// session.
    ///
    /// Subscriptions are server-side state that survives the USB transport.
    /// If a previous Python process crashed or was killed without calling
    /// :meth:`gpio_unsubscribe`, the firmware still considers those pins
    /// owned by a monitor task; calling this method on connect releases
    /// them. The call is idempotent and safe to issue on a fresh device.
    ///
    /// Returns:
    ///     int: Number of subscriptions that were torn down (0 if none
    ///         were active).
    ///
    /// Raises:
    ///     RuntimeError: If the underlying transport call fails.
    fn system_reset_subscriptions(&self, py: Python<'_>) -> PyResult<u8> {
        self.block(py, self.inner.system_reset_subscriptions())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Validate that the connected firmware is wire-compatible with this
    /// host library, and return the device info on success.
    ///
    /// Raises:
    ///     RuntimeError: If the firmware is too old (no ``device/info``
    ///         endpoint), if the schema versions disagree, or on a
    ///         communication failure.
    fn validate(&self, py: Python<'_>) -> PyResult<DeviceInfo> {
        self.block(py, self.inner.validate())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query the current I2C bus frequency.
    ///
    /// Returns:
    ///     I2cFrequency: The active I2C frequency. Defaults to ``Standard``
    ///     (100 kHz) on firmware boot.
    fn i2c_get_config(&self, py: Python<'_>) -> PyResult<I2cFrequency> {
        self.block(py, self.inner.i2c_get_config())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query the current SPI bus configuration.
    ///
    /// Returns:
    ///     SpiConfigurationInfo: Active SPI frequency, phase, and polarity.
    ///     Defaults are 1 MHz, ``CaptureOnFirstTransition``, ``IdleLow``.
    fn spi_get_config(&self, py: Python<'_>) -> PyResult<SpiConfigurationInfo> {
        self.block(py, self.inner.spi_get_config())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Set the UART baud rate.
    ///
    /// Takes effect immediately before the next UART operation. The default
    /// baud rate is 115200.
    fn uart_set_config(&self, py: Python<'_>, baud_rate: u32) -> PyResult<()> {
        self.block(py, self.inner.uart_set_config(baud_rate))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query the current UART configuration.
    ///
    /// Returns:
    ///     UartConfigurationInfo: Active baud rate. Default is 115200.
    ///
    /// Raises:
    ///     RuntimeError: If the firmware's hardware revision does not support UART.
    fn uart_get_config(&self, py: Python<'_>) -> PyResult<UartConfigurationInfo> {
        self.block(py, self.inner.uart_get_config())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Set the raw duty cycle of a PWM channel.
    ///
    /// ``duty_cycle`` is a raw compare value in ``0..=max_duty``. Use
    /// :meth:`pwm_get_duty_cycle` to discover the channel's ``max_duty``.
    ///
    /// Channels 0–1 share PWM slice 6, channels 2–3 share PWM slice 7.
    ///
    /// Args:
    ///     channel (int): PWM channel (0–3).
    ///     duty_cycle (int): Raw compare value.
    fn pwm_set_duty_cycle(&self, py: Python<'_>, channel: u8, duty_cycle: u16) -> PyResult<()> {
        self.block(py, self.inner.pwm_set_duty_cycle(channel, duty_cycle))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query the current duty cycle of a PWM channel.
    ///
    /// Returns:
    ///     PwmDutyCycleInfo: ``current_duty`` (raw compare value) and
    ///     ``max_duty`` (full-scale value, equal to ``top + 1``).
    fn pwm_get_duty_cycle(&self, py: Python<'_>, channel: u8) -> PyResult<PwmDutyCycleInfo> {
        self.block(py, self.inner.pwm_get_duty_cycle(channel))
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Enable the PWM slice that owns ``channel`` (0–3).
    ///
    /// Because PWM slices drive two channels, enabling channel 0 also enables
    /// channel 1 (and vice versa). Same for channels 2 and 3.
    fn pwm_enable(&self, py: Python<'_>, channel: u8) -> PyResult<()> {
        self.block(py, self.inner.pwm_enable(channel))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Disable the PWM slice that owns ``channel`` (0–3).
    ///
    /// Because PWM slices drive two channels, disabling channel 0 also
    /// disables channel 1 (and vice versa). Same for channels 2 and 3.
    fn pwm_disable(&self, py: Python<'_>, channel: u8) -> PyResult<()> {
        self.block(py, self.inner.pwm_disable(channel))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Configure the PWM slice behind ``channel`` (0–3).
    ///
    /// Sets the output frequency and phase-correct mode. The firmware
    /// computes ``top`` and ``divider`` automatically. Existing duty-cycle
    /// compare values are scaled proportionally to the new ``top``.
    ///
    /// Channels 0–1 share a slice (channels 2–3 likewise), so configuring
    /// one channel also affects its sibling.
    ///
    /// Args:
    ///     channel (int): PWM channel (0–3).
    ///     frequency_hz (int): Desired PWM frequency in Hz.
    ///     phase_correct (bool): Whether to enable phase-correct mode.
    fn pwm_set_config(
        &self,
        py: Python<'_>,
        channel: u8,
        frequency_hz: u32,
        phase_correct: bool,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner
                .pwm_set_config(channel, frequency_hz, phase_correct),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query the current configuration of the PWM slice behind ``channel``.
    ///
    /// Returns:
    ///     PwmConfigurationInfo: Effective frequency, phase-correct flag,
    ///     and enabled state.
    fn pwm_get_config(&self, py: Python<'_>, channel: u8) -> PyResult<PwmConfigurationInfo> {
        self.block(py, self.inner.pwm_get_config(channel))
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Perform a single-shot ADC read on the specified channel.
    ///
    /// Returns a raw 12-bit value (0–4095). Convert to voltage with
    /// ``V ≈ raw * 3.3 / 4096`` (approximate; depends on ADC_AVDD).
    ///
    /// Args:
    ///     channel (int): ADC channel (0–3).
    ///
    /// Raises:
    ///     RuntimeError: If ``channel`` is outside the range 0–3, or if the
    ///         firmware reports an ADC error.
    fn adc_read(&self, py: Python<'_>, channel: u8) -> PyResult<u16> {
        let channel = match channel {
            0 => AdcChannel::Adc0,
            1 => AdcChannel::Adc1,
            2 => AdcChannel::Adc2,
            3 => AdcChannel::Adc3,
            _ => {
                return Err(PyRuntimeError::new_err(
                    "ADC channel must be between 0 and 3",
                ));
            }
        };
        self.block(py, self.inner.adc_read(channel))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Query the ADC configuration (resolution, reference voltage, channel count).
    ///
    /// Returns:
    ///     AdcConfigurationInfo: Fixed values for the RP2350 ADC.
    ///
    /// Raises:
    ///     RuntimeError: If the firmware's hardware revision does not support ADC.
    fn adc_get_config(&self, py: Python<'_>) -> PyResult<AdcConfigurationInfo> {
        self.block(py, self.inner.adc_get_config())
            .map(Into::into)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Perform a 1-Wire bus reset and detect device presence.
    ///
    /// Returns:
    ///     bool: True if one or more devices responded with a presence pulse.
    fn onewire_reset(&self, py: Python<'_>) -> PyResult<bool> {
        self.block(py, self.inner.onewire_reset())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Read ``len`` bytes from the 1-Wire bus.
    ///
    /// The firmware sends ``0xFF`` read slots and captures the response bits.
    fn onewire_read(&self, py: Python<'_>, len: u16) -> PyResult<Vec<u8>> {
        self.block(py, self.inner.onewire_read(len))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Write raw bytes to the 1-Wire bus.
    fn onewire_write(&self, py: Python<'_>, data: Vec<u8>) -> PyResult<()> {
        self.block(py, self.inner.onewire_write(&data))
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Write bytes to the 1-Wire bus, then apply a strong pullup.
    ///
    /// Required for parasitic-power devices like the DS18B20 during
    /// temperature conversion. The bus is held high for
    /// ``pullup_duration_ms`` milliseconds after the last bit is sent.
    fn onewire_write_pullup(
        &self,
        py: Python<'_>,
        data: Vec<u8>,
        pullup_duration_ms: u16,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner.onewire_write_pullup(&data, pullup_duration_ms),
        )
        .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Start a new 1-Wire ROM search and return the first device address.
    ///
    /// Returns:
    ///     int | None: The first 64-bit ROM ID found, or ``None`` if no
    ///     devices are on the bus. Call :meth:`onewire_search_next` to
    ///     enumerate the rest.
    fn onewire_search(&self, py: Python<'_>) -> PyResult<Option<u64>> {
        self.block(py, self.inner.onewire_search())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }

    /// Continue the current 1-Wire ROM search.
    ///
    /// Returns:
    ///     int | None: The next device's 64-bit ROM ID, or ``None`` when
    ///     all devices have been enumerated.
    fn onewire_search_next(&self, py: Python<'_>) -> PyResult<Option<u64>> {
        self.block(py, self.inner.onewire_search_next())
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // M3 — SPI chip-select bounds (issue #104)
    // ===================================================================
    //
    // `pyo3`'s `auto-initialize` feature is not enabled and enabling it
    // would be a manifest change, so no test here may hold a `Python<'_>`
    // token. Everything below is therefore GIL-free: the `From` conversion
    // and the message/classification helpers. The `#[pymethods]` dispatch
    // for `num_gpios()`, and the ordering claim that an out-of-`u8` Python
    // int raises `OverflowError` before the bound is resolved, are not
    // executable in Rust and remain unproven here.

    fn lib_info(num_gpios: u8) -> LibDeviceInfo {
        LibDeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 0,
            schema_minor: 6,
            schema_patch: 1,
            hw_version: 1,
            capabilities: pico_de_gallo_lib::Capabilities::NONE,
            num_gpios,
            build_id: "test-build".try_into().unwrap(),
        }
    }

    #[test]
    fn device_info_conversion_carries_num_gpios() {
        let py_info: DeviceInfo = lib_info(7).into();
        assert_eq!(py_info.num_gpios, 7);
    }

    #[test]
    fn device_info_conversion_carries_build_id() {
        for id in ["", "unknown", "firmware-v0.11.0-27-gdeadbee-dirty"] {
            let mut info = lib_info(4);
            info.build_id = id.try_into().unwrap();
            let converted: DeviceInfo = info.into();
            assert_eq!(converted.build_id, id);
        }
    }

    #[test]
    fn device_info_conversion_carries_num_gpios_across_the_full_u8_range() {
        // Sweeping the range is what separates "converted the field" from
        // "hardcoded a plausible constant".
        for n in 0..=u8::MAX {
            let py_info: DeviceInfo = lib_info(n).into();
            assert_eq!(py_info.num_gpios, n);
        }
    }

    fn all_validate_errors() -> Vec<LibValidateError> {
        vec![
            LibValidateError::Comms(pico_de_gallo_lib::host_client::HostErr::Closed),
            LibValidateError::Timeout,
            LibValidateError::LegacyFirmware,
            LibValidateError::SchemaMismatch {
                expected_major: 0,
                actual_major: 0,
                expected_minor: 7,
                actual_minor: 6,
            },
        ]
    }

    #[test]
    fn num_gpios_error_message_prefix_is_exact() {
        let msg = num_gpios_error_message(&LibValidateError::Comms(
            pico_de_gallo_lib::host_client::HostErr::Closed,
        ));
        assert!(
            msg.starts_with("failed to determine num_gpios: "),
            "got: {msg}"
        );
    }

    #[test]
    fn num_gpios_timeout_message_names_the_endpoint() {
        let msg = num_gpios_error_message(&LibValidateError::Timeout);
        assert!(msg.contains("failed to determine num_gpios"), "got: {msg}");
        assert!(msg.contains("device/info"), "got: {msg}");
        assert!(msg.contains("300"), "got: {msg}");
    }

    #[test]
    fn num_gpios_messages_are_disjoint_from_chip_select_messages() {
        // The binding constraint at the Python boundary.
        let cs_messages = [
            classify_cs(4, 4).expect_err("out of range"),
            classify_cs(0, 0).expect_err("zero bound"),
        ];
        for e in all_validate_errors() {
            let msg = num_gpios_error_message(&e);
            assert!(
                !msg.to_lowercase().contains("chip-select"),
                "metadata message leaked CS wording: {msg}"
            );
        }
        for msg in &cs_messages {
            assert!(
                !msg.contains("failed to determine num_gpios"),
                "CS message leaked metadata wording: {msg}"
            );
        }
    }

    #[test]
    fn spi_batch_out_of_range_message_is_exact() {
        assert_eq!(
            classify_cs(4, 4).expect_err("out of range"),
            "invalid SPI chip-select pin 4; device reports 4 GPIOs (valid 0..4)"
        );
        // A truncating implementation would report pin 3 here — and accept it.
        assert_eq!(
            classify_cs(255, 4).expect_err("out of range"),
            "invalid SPI chip-select pin 255; device reports 4 GPIOs (valid 0..4)"
        );
    }

    #[test]
    fn spi_batch_zero_bound_message_is_exact() {
        assert_eq!(
            classify_cs(0, 0).expect_err("no pin is valid at n = 0"),
            "device reports num_gpios=0; no SPI chip-select pin is available"
        );
    }

    #[test]
    fn classify_cs_boundaries_at_n_four_and_n_seven() {
        classify_cs(3, 4).expect("pin 3 is valid at n = 4");
        assert!(classify_cs(4, 4).is_err());
        classify_cs(6, 7).expect("pin 6 is valid at n = 7");
        assert!(classify_cs(7, 7).is_err());
    }
}
