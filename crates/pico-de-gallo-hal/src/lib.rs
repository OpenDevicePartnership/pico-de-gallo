//! [`embedded-hal`](https://docs.rs/embedded-hal) and
//! [`embedded-hal-async`](https://docs.rs/embedded-hal-async) implementations
//! backed by a Pico de Gallo USB bridge.
//!
//! This crate lets you run embedded Rust drivers on a host machine by
//! forwarding I2C, SPI, GPIO, PWM, ADC, 1-Wire, and delay operations to a Pico de Gallo
//! device over USB.
//!
//! # Quick Start
//!
//! ```no_run
//! use pico_de_gallo_hal::Hal;
//! use embedded_hal::i2c::I2c;
//!
//! let hal = Hal::new();
//! let mut i2c = hal.i2c();
//!
//! // Read 2 bytes from a TMP102 temperature sensor
//! let mut buf = [0u8; 2];
//! i2c.write_read(0x48, &[0x00], &mut buf).unwrap();
//! ```
//!
//! # Blocking vs. Async
//!
//! Both `embedded-hal` (blocking) and `embedded-hal-async` traits are
//! implemented. The HAL automatically detects whether it is running inside a
//! tokio runtime and adjusts its execution strategy:
//!
//! - **Inside tokio**: Uses [`tokio::task::block_in_place`] to avoid blocking
//!   the async executor while waiting for USB responses.
//! - **Outside tokio**: Blocks directly on the tokio handle.
//!
//! This means the same `Hal` instance works in both synchronous test code
//! and async application code.
//!
//! # Implemented Traits
//!
//! | Peripheral | Blocking Trait | Async Trait |
//! |------------|---------------|-------------|
//! | GPIO | [`OutputPin`](embedded_hal::digital::OutputPin), [`InputPin`](embedded_hal::digital::InputPin), [`StatefulOutputPin`](embedded_hal::digital::StatefulOutputPin) | [`Wait`](embedded_hal_async::digital::Wait) |
//! | I2C | [`I2c`](embedded_hal::i2c::I2c) | [`I2c`](embedded_hal_async::i2c::I2c) |
//! | SPI | [`SpiBus`](embedded_hal::spi::SpiBus), [`SpiDevice`](embedded_hal::spi::SpiDevice) | [`SpiBus`](embedded_hal_async::spi::SpiBus), [`SpiDevice`](embedded_hal_async::spi::SpiDevice) |
//! | PWM | [`SetDutyCycle`](embedded_hal::pwm::SetDutyCycle) | — |
//! | Delay | [`DelayNs`](embedded_hal::delay::DelayNs) | [`DelayNs`](embedded_hal_async::delay::DelayNs) |

use pico_de_gallo_lib::{
    AdcError, GpioError, GpioState, I2cError, OneWireError, PicoDeGallo, PicoDeGalloError,
    PwmError, SpiError, UartError, ValidateError,
};
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Mutex;
use tokio::task::block_in_place;

pub use pico_de_gallo_lib::{
    AdcChannel, AdcConfigurationInfo, GpioDirection, GpioEdge, GpioEvent, GpioPull, I2cFrequency,
    SpiConfigurationInfo, SpiPhase, SpiPolarity, UartConfigurationInfo,
};

/// Top-level HAL context for a Pico de Gallo device.
///
/// Holds the USB connection and tokio runtime handle. Create peripheral
/// handles using the accessor methods: [`gpio`](Self::gpio),
/// [`i2c`](Self::i2c), [`spi`](Self::spi), [`delay`](Self::delay).
pub struct Hal {
    gallo: Arc<Mutex<PicoDeGallo>>,
    _runtime: Option<Runtime>,
    handle: Handle,
}

impl Default for Hal {
    fn default() -> Self {
        Self::new()
    }
}

impl Hal {
    /// Instantiate the library context.
    pub fn new() -> Self {
        Self::new_inner(None)
    }

    /// Instantiate the library context for the device with the given
    /// `serial_number`.
    pub fn new_with_serial_number(serial_number: &str) -> Self {
        Self::new_inner(Some(serial_number))
    }

    fn new_inner(serial_number: Option<&str>) -> Self {
        let (runtime, handle) = match Handle::try_current() {
            Ok(handle) => (None, handle),
            Err(_) => {
                let runtime = Runtime::new().unwrap();
                let handle = runtime.handle().clone();
                (Some(runtime), handle)
            }
        };

        let gallo = if runtime.is_none() {
            if let Some(serial_number) = serial_number {
                PicoDeGallo::new_with_serial_number(serial_number)
            } else {
                PicoDeGallo::new()
            }
        } else {
            handle.block_on(async {
                if let Some(serial_number) = serial_number {
                    PicoDeGallo::new_with_serial_number(serial_number)
                } else {
                    PicoDeGallo::new()
                }
            })
        };

        Self {
            gallo: Arc::new(Mutex::new(gallo)),
            _runtime: runtime,
            handle,
        }
    }

    /// Set I2C bus configuration parameters.
    pub fn i2c_set_config(&mut self, frequency: I2cFrequency) -> Result<(), I2cHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.i2c_set_config_inner(frequency))
        } else {
            self.i2c_set_config_inner(frequency)
        }
    }

    fn i2c_set_config_inner(&mut self, frequency: I2cFrequency) -> Result<(), I2cHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.i2c_set_config(frequency))
            .map_err(I2cHalError::from)
    }

    /// Scan the I2C bus and return the addresses of all responding devices.
    ///
    /// The firmware probes each 7-bit address by attempting a 1-byte read.
    /// When `include_reserved` is `false`, only the standard range (0x08–0x77)
    /// is probed; when `true`, the full range (0x00–0x7F) is scanned.
    pub fn i2c_scan(&self, include_reserved: bool) -> Result<Vec<u8>, I2cHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.i2c_scan_inner(include_reserved))
        } else {
            self.i2c_scan_inner(include_reserved)
        }
    }

    fn i2c_scan_inner(&self, include_reserved: bool) -> Result<Vec<u8>, I2cHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.i2c_scan(include_reserved))
            .map_err(I2cHalError::from)
    }

    /// Set SPI bus configuration parameters.
    pub fn spi_set_config(
        &mut self,
        spi_frequency: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) -> Result<(), SpiHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.spi_set_config_inner(spi_frequency, spi_phase, spi_polarity))
        } else {
            self.spi_set_config_inner(spi_frequency, spi_phase, spi_polarity)
        }
    }

    fn spi_set_config_inner(
        &mut self,
        spi_frequency: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) -> Result<(), SpiHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.spi_set_config(spi_frequency, spi_phase, spi_polarity))
            .map_err(SpiHalError::from)
    }

    /// Query the current I2C bus configuration.
    ///
    /// Returns the [`I2cFrequency`] value active on the firmware
    /// (default: `Standard` / 100 kHz).
    pub fn i2c_get_config(&self) -> Result<I2cFrequency, I2cHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.i2c_get_config_inner())
        } else {
            self.i2c_get_config_inner()
        }
    }

    fn i2c_get_config_inner(&self) -> Result<I2cFrequency, I2cHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.i2c_get_config())
            .map_err(|e| match e {
                PicoDeGalloError::Comms(c) => I2cHalError::Comms(format!("{c:?}")),
                PicoDeGalloError::Endpoint(never) => match never {},
            })
    }

    /// Query the current SPI bus configuration.
    ///
    /// Returns a [`SpiConfigurationInfo`] with the active frequency, phase,
    /// and polarity (defaults: 1 MHz, `CaptureOnFirstTransition`, `IdleLow`).
    pub fn spi_get_config(&self) -> Result<SpiConfigurationInfo, SpiHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.spi_get_config_inner())
        } else {
            self.spi_get_config_inner()
        }
    }

    fn spi_get_config_inner(&self) -> Result<SpiConfigurationInfo, SpiHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.spi_get_config())
            .map_err(|e| match e {
                PicoDeGalloError::Comms(c) => SpiHalError::Comms(format!("{c:?}")),
                PicoDeGalloError::Endpoint(never) => match never {},
            })
    }

    /// Set the PWM configuration for a channel's slice.
    ///
    /// Configures the output `frequency_hz` and `phase_correct` mode.
    /// Existing duty-cycle values are scaled proportionally.
    pub fn pwm_set_config(
        &mut self,
        channel: u8,
        frequency_hz: u32,
        phase_correct: bool,
    ) -> Result<(), PwmHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.pwm_set_config_inner(channel, frequency_hz, phase_correct))
        } else {
            self.pwm_set_config_inner(channel, frequency_hz, phase_correct)
        }
    }

    fn pwm_set_config_inner(
        &self,
        channel: u8,
        frequency_hz: u32,
        phase_correct: bool,
    ) -> Result<(), PwmHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.pwm_set_config(channel, frequency_hz, phase_correct))
            .map_err(PwmHalError::from)
    }

    /// Query the current PWM configuration for a channel's slice.
    pub fn pwm_get_config(
        &self,
        channel: u8,
    ) -> Result<pico_de_gallo_lib::PwmConfigurationInfo, PwmHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.pwm_get_config_inner(channel))
        } else {
            self.pwm_get_config_inner(channel)
        }
    }

    fn pwm_get_config_inner(
        &self,
        channel: u8,
    ) -> Result<pico_de_gallo_lib::PwmConfigurationInfo, PwmHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.pwm_get_config(channel))
            .map_err(PwmHalError::from)
    }

    /// Gpio
    pub fn gpio(&self, pin: u8) -> Gpio {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        Gpio { pin, gallo, handle }
    }

    /// I2c
    pub fn i2c(&self) -> I2c {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        I2c { gallo, handle }
    }

    /// Spi
    pub fn spi(&self) -> Spi {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        Spi { gallo, handle }
    }

    /// Uart
    pub fn uart(&self) -> Uart {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        Uart {
            gallo,
            handle,
            timeout_ms: 1000,
        }
    }

    /// Obtain a [`PwmChannel`] handle for the given channel (0–3).
    ///
    /// Channels 0–1 are on PWM slice 6 (GPIO 12–13), channels 2–3 on
    /// slice 7 (GPIO 14–15). The returned handle implements
    /// [`SetDutyCycle`](embedded_hal::pwm::SetDutyCycle).
    pub fn pwm_channel(&self, channel: u8) -> PwmChannel {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        PwmChannel {
            channel,
            gallo,
            handle,
        }
    }

    /// Perform a single-shot ADC read on the specified channel.
    ///
    /// Returns a raw 12-bit value (0–4095). Convert to approximate voltage
    /// with `V ≈ raw × 3.3 / 4096`.
    ///
    /// There is no standard `embedded-hal` ADC trait in 1.0, so this is
    /// exposed as a project-specific method.
    pub fn adc_read(&self, channel: AdcChannel) -> Result<u16, AdcHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.adc_read_inner(channel))
        } else {
            self.adc_read_inner(channel)
        }
    }

    fn adc_read_inner(&self, channel: AdcChannel) -> Result<u16, AdcHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.adc_read(channel))
            .map_err(AdcHalError::from)
    }

    /// Query the ADC configuration (resolution, reference, channel count).
    pub fn adc_get_config(&self) -> Result<AdcConfigurationInfo, AdcHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.adc_get_config_inner())
        } else {
            self.adc_get_config_inner()
        }
    }

    fn adc_get_config_inner(&self) -> Result<AdcConfigurationInfo, AdcHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.adc_get_config())
            .map_err(|e| match e {
                PicoDeGalloError::Comms(c) => AdcHalError::Comms(format!("{c:?}")),
                PicoDeGalloError::Endpoint(e) => AdcHalError::Adc(e),
            })
    }

    /// Subscribe to GPIO edge events on a pin.
    ///
    /// Starts push-based monitoring for the specified edge type. While subscribed,
    /// the pin cannot be used by other GPIO operations. Use
    /// [`gpio_unsubscribe`](Self::gpio_unsubscribe) to release the pin.
    pub fn gpio_subscribe(&self, pin: u8, edge: GpioEdge) -> Result<(), GpioHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.gpio_subscribe_inner(pin, edge))
        } else {
            self.gpio_subscribe_inner(pin, edge)
        }
    }

    fn gpio_subscribe_inner(&self, pin: u8, edge: GpioEdge) -> Result<(), GpioHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_subscribe(pin, edge))
            .map_err(GpioHalError::from)
    }

    /// Unsubscribe from GPIO edge events on a pin.
    ///
    /// Stops monitoring and returns the pin to normal operation.
    pub fn gpio_unsubscribe(&self, pin: u8) -> Result<(), GpioHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.gpio_unsubscribe_inner(pin))
        } else {
            self.gpio_unsubscribe_inner(pin)
        }
    }

    fn gpio_unsubscribe_inner(&self, pin: u8) -> Result<(), GpioHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_unsubscribe(pin))
            .map_err(GpioHalError::from)
    }

    /// Create an [`SpiDevice`] that manages chip-select on `cs_pin`.
    ///
    /// The CS pin is driven high (deasserted) immediately. Each
    /// [`SpiDevice::transaction`](embedded_hal::spi::SpiDevice::transaction)
    /// call will assert CS low, perform the operations, flush, then deassert
    /// CS high.
    ///
    /// # Chip-select preflight
    ///
    /// `cs_pin` is validated against the device-reported GPIO count (see
    /// [`Hal::num_gpios`]) **before** the pin is driven. A refused
    /// chip-select therefore leaves the pin exactly as the caller had
    /// configured it — which is the defect issue #104 reported. The first
    /// call performs one implicit `device/info` round-trip; afterwards the
    /// count is read from a cache shared by every handle cloned from the
    /// same connection.
    ///
    /// # Errors
    ///
    /// - [`SpiHalError::DeviceInfo`] — the GPIO count could not be
    ///   established (transport, 300-second timeout, legacy firmware, or
    ///   schema mismatch). Never reported as an invalid chip-select.
    /// - [`SpiHalError::NoGpios`] — the device reports zero GPIOs.
    /// - [`SpiHalError::InvalidCsPin`] — `cs_pin` is at or beyond the count.
    /// - [`SpiHalError::Comms`] — the initial CS-high drive failed.
    pub fn spi_device(&self, cs_pin: u8) -> Result<SpiDev, SpiHalError> {
        // Bridge to async like every other device method: inside a tokio
        // runtime the inner `block_on`s must run under `block_in_place` or they
        // panic (a runtime cannot be driven from within a runtime). Regression:
        // this previously did a raw `handle.block_on` and panicked whenever it
        // was called from an async context.
        if Self::in_async_context() {
            block_in_place(|| self.spi_device_inner(cs_pin))
        } else {
            self.spi_device_inner(cs_pin)
        }
    }

    fn spi_device_inner(&self, cs_pin: u8) -> Result<SpiDev, SpiHalError> {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();

        {
            // One guard covers both the metadata lookup and the CS drive.
            // The public `Hal::num_gpios` must never be called here: it takes
            // the same lock and would deadlock.
            let guard = handle.block_on(gallo.lock());
            let bound = handle.block_on(guard.num_gpios());
            let mut drive = |pin: u8| {
                handle
                    .block_on(guard.gpio_put(pin, GpioState::High))
                    .map_err(|e| SpiHalError::Comms(format!("CS init failed: {e:?}")))
            };
            spi_device_from_bound(bound, cs_pin, &mut drive)?;
        }

        Ok(SpiDev {
            gallo,
            handle,
            cs_pin,
        })
    }

    /// Delay
    pub fn delay(&self) -> Delay {
        Delay
    }

    /// Create a 1-Wire bus handle.
    ///
    /// The returned [`OneWire`] provides blocking methods for 1-Wire bus operations
    /// (reset, read, write, search). There is no embedded-hal trait for 1-Wire, so
    /// this is a custom API.
    pub fn onewire(&self) -> OneWire {
        OneWire {
            gallo: self.gallo.clone(),
            handle: self.handle.clone(),
        }
    }

    /// Returns true if we are currently inside a tokio async context.
    fn in_async_context() -> bool {
        Handle::try_current().is_ok()
    }

    /// Instantiate the library context AND validate that the connected
    /// firmware's schema matches this HAL's compiled-in schema.
    ///
    /// Use this in production code that wants to fail loudly on a
    /// device-not-connected or firmware-version-mismatch condition
    /// rather than deferring failure to the first RPC.
    /// [`Hal::new`] constructs lazily and surfaces validation failures
    /// only on the first endpoint call.
    pub fn new_validated() -> Result<Self, HalInitError> {
        let hal = Self::new();
        hal.validate()?;
        Ok(hal)
    }

    /// Like [`Hal::new_validated`] but selects a specific device by
    /// serial number.
    pub fn new_validated_with_serial_number(serial_number: &str) -> Result<Self, HalInitError> {
        let hal = Self::new_with_serial_number(serial_number);
        hal.validate()?;
        Ok(hal)
    }

    /// Validate the connected firmware's schema version against this
    /// HAL's compiled-in schema.
    ///
    /// Returns `Ok(())` on a matching schema. Returns
    /// [`HalInitError::Validate`] on schema mismatch or communication
    /// failure.
    ///
    /// Called automatically by [`Hal::new_validated`]. Exposed here
    /// for callers that constructed via the lazy [`Hal::new`] and
    /// want to validate after the fact.
    pub fn validate(&self) -> Result<(), HalInitError> {
        if Self::in_async_context() {
            block_in_place(|| self.validate_inner())
        } else {
            self.validate_inner()
        }
    }

    fn validate_inner(&self) -> Result<(), HalInitError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.validate())
            .map(|_info| ())
            .map_err(HalInitError::Validate)
    }

    /// The number of GPIO pins the connected device reports.
    ///
    /// This is the runtime-authoritative bound for a GPIO index and for the
    /// SPI chip-select pin of [`Hal::spi_device`]. Prefer it over the
    /// compile-time `pico_de_gallo_lib::NUM_GPIOS`, which is only this
    /// build's default.
    ///
    /// The first call performs one implicit validated `device/info`
    /// round-trip, bounded at 300 seconds; afterwards the value is read from
    /// a cache shared by every handle cloned from the same connection. A
    /// reported count of zero is a legitimate, cacheable answer.
    ///
    /// Note: during the current schema freeze, a matching reported schema
    /// version does not prove wire-*shape* compatibility. Validation bounds
    /// how long you wait and checks the reported numbers; it cannot make
    /// those numbers trustworthy.
    ///
    /// # Errors
    ///
    /// [`HalInitError::Validate`] carrying the exact
    /// [`ValidateError`] — including
    /// [`ValidateError::Timeout`] when the query did not
    /// complete within 300 seconds. A failure is retryable.
    pub fn num_gpios(&self) -> Result<u8, HalInitError> {
        if Self::in_async_context() {
            block_in_place(|| self.num_gpios_inner())
        } else {
            self.num_gpios_inner()
        }
    }

    fn num_gpios_inner(&self) -> Result<u8, HalInitError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.num_gpios())
            .map_err(HalInitError::Validate)
    }

    /// Tear down any GPIO subscriptions still held by the firmware
    /// from a previous host process.
    ///
    /// Returns the number of subscriptions cleared. Idempotent and
    /// cheap when no subscriptions are active, so the recommended
    /// connect sequence after [`Hal::new_validated`] is:
    ///
    /// ```no_run
    /// use pico_de_gallo_hal::Hal;
    /// let hal = Hal::new_validated().expect("device validated");
    /// let _cleared = hal.system_reset_subscriptions()
    ///     .expect("system reset failed");
    /// // ... use hal ...
    /// ```
    ///
    /// **When you need this:** if your application uses GPIO
    /// subscriptions and a previous host process crashed without
    /// calling `gpio_unsubscribe`, the firmware will still consider
    /// those pins owned by a monitor task. This endpoint releases
    /// them so the new host can use the pins.
    pub fn system_reset_subscriptions(&self) -> Result<u8, SystemHalError> {
        if Self::in_async_context() {
            block_in_place(|| self.system_reset_subscriptions_inner())
        } else {
            self.system_reset_subscriptions_inner()
        }
    }

    fn system_reset_subscriptions_inner(&self) -> Result<u8, SystemHalError> {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.system_reset_subscriptions())
            .map_err(|e| match e {
                PicoDeGalloError::Comms(c) => SystemHalError::Comms(format!("{c:?}")),
                PicoDeGalloError::Endpoint(never) => match never {},
            })
    }
}

// ----------------------------- Error Types -----------------------------

/// Error returned by [`Hal::new_validated`] and [`Hal::validate`].
///
/// Either the device is not reachable / not enumerated, or the
/// firmware's schema version does not match this HAL's compiled-in
/// schema. Both cases are blocking failures — the caller should
/// stop, surface a clear error to the user, and not attempt RPCs.
#[derive(Debug)]
pub enum HalInitError {
    /// Validation of the firmware's schema version failed (mismatch,
    /// legacy firmware, or USB transport error during the device-info
    /// round-trip). See [`pico_de_gallo_lib::ValidateError`] for the
    /// underlying detail.
    Validate(ValidateError),
}

impl core::fmt::Display for HalInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Validate(e) => write!(f, "firmware validation failed: {e}"),
        }
    }
}

impl std::error::Error for HalInitError {}

impl From<ValidateError> for HalInitError {
    fn from(e: ValidateError) -> Self {
        Self::Validate(e)
    }
}

/// Error type for system-level HAL operations such as
/// [`Hal::system_reset_subscriptions`].
#[derive(Debug)]
pub enum SystemHalError {
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for SystemHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for SystemHalError {}

/// Error type for GPIO HAL operations.
#[derive(Debug)]
pub enum GpioHalError {
    /// A GPIO-specific error from the device firmware.
    Gpio(GpioError),
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for GpioHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Gpio(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for GpioHalError {}

impl From<PicoDeGalloError<GpioError>> for GpioHalError {
    fn from(e: PicoDeGalloError<GpioError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::Gpio(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

impl embedded_hal::digital::Error for GpioHalError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

/// Error type for I2C HAL operations.
#[derive(Debug)]
pub enum I2cHalError {
    /// An I2C-specific error from the device firmware.
    I2c(I2cError),
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for I2cHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::I2c(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for I2cHalError {}

impl From<PicoDeGalloError<I2cError>> for I2cHalError {
    fn from(e: PicoDeGalloError<I2cError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::I2c(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

impl From<PicoDeGalloError<pico_de_gallo_lib::I2cBatchError>> for I2cHalError {
    fn from(e: PicoDeGalloError<pico_de_gallo_lib::I2cBatchError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(batch_err) => Self::I2c(batch_err.kind),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

impl embedded_hal::i2c::Error for I2cHalError {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match self {
            Self::I2c(I2cError::NoAcknowledge) => embedded_hal::i2c::ErrorKind::NoAcknowledge(
                embedded_hal::i2c::NoAcknowledgeSource::Unknown,
            ),
            Self::I2c(I2cError::ArbitrationLoss) => embedded_hal::i2c::ErrorKind::ArbitrationLoss,
            Self::I2c(I2cError::Bus) => embedded_hal::i2c::ErrorKind::Bus,
            Self::I2c(I2cError::Overrun) => embedded_hal::i2c::ErrorKind::Overrun,
            _ => embedded_hal::i2c::ErrorKind::Other,
        }
    }
}

/// Error type for SPI HAL operations.
///
/// Not `#[non_exhaustive]`: appending a variant must break every exhaustive
/// match so each caller makes a deliberate decision about the new case.
#[derive(Debug)]
pub enum SpiHalError {
    /// An SPI-specific error from the device firmware.
    Spi(SpiError),
    /// A USB communication error.
    Comms(String),
    /// The device-reported GPIO count could not be established, so no
    /// chip-select could be validated.
    ///
    /// Carries the exact [`ValidateError`] — transport, timeout, legacy
    /// firmware, or schema mismatch. A metadata failure is **never**
    /// reported as [`Self::InvalidCsPin`]; misattributing it would send
    /// users hunting for a bug in their own arguments (issue #104).
    DeviceInfo(ValidateError),
    /// The device reports zero GPIOs, so no chip-select pin exists.
    /// Distinct from [`Self::InvalidCsPin`]: the caller's index is not the
    /// problem.
    NoGpios,
    /// The requested chip-select index is at or beyond the device-reported
    /// GPIO count. Refused locally: no pin is driven and no RPC is sent.
    InvalidCsPin {
        /// The chip-select index the caller supplied, verbatim.
        cs: u8,
        /// The device-reported GPIO count that `cs` was checked against.
        num_gpios: u8,
    },
}

impl core::fmt::Display for SpiHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Spi(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
            Self::DeviceInfo(e) => write!(f, "failed to determine num_gpios: {e}"),
            Self::NoGpios => write!(
                f,
                "device reports num_gpios=0; no SPI chip-select pin is available"
            ),
            Self::InvalidCsPin { cs, num_gpios } => write!(
                f,
                "invalid SPI chip-select pin {cs}; device reports {num_gpios} \
                 GPIOs (valid 0..{num_gpios})"
            ),
        }
    }
}

impl std::error::Error for SpiHalError {}

impl From<PicoDeGalloError<SpiError>> for SpiHalError {
    fn from(e: PicoDeGalloError<SpiError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::Spi(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

impl From<pico_de_gallo_lib::SpiBatchCallError> for SpiHalError {
    fn from(e: pico_de_gallo_lib::SpiBatchCallError) -> Self {
        use pico_de_gallo_lib::SpiBatchCallError as E;
        match e {
            E::DeviceInfo(v) => Self::DeviceInfo(v),
            E::NoGpios => Self::NoGpios,
            E::InvalidCsPin { cs, num_gpios } => Self::InvalidCsPin { cs, num_gpios },
            E::Comms(c) => Self::Comms(format!("{c:?}")),
            E::Endpoint(batch_err) => Self::Spi(batch_err.kind),
        }
    }
}

/// Classify a chip-select index against a device-reported GPIO count.
///
/// `num_gpios` must come from an `Ok(_)` metadata read: see
/// [`spi_device_from_bound`], which is the only caller and which returns
/// early on `Err`. A count of zero is [`SpiHalError::NoGpios`] for every
/// index, so a board with no GPIOs is diagnosable as exactly that.
fn classify_cs(cs: u8, num_gpios: u8) -> Result<(), SpiHalError> {
    if num_gpios == 0 {
        return Err(SpiHalError::NoGpios);
    }
    if cs >= num_gpios {
        return Err(SpiHalError::InvalidCsPin { cs, num_gpios });
    }
    Ok(())
}

/// Decide whether to drive the chip-select line, given a metadata result.
///
/// Extracted from [`Hal::spi_device`] so the central property of issue #104
/// — *a refusal drives no pin* — is executable rather than merely argued:
/// a test can pass a counting `drive` closure and assert it was never
/// called.
///
/// `drive` is invoked exactly once, and only after `bound` resolved to an
/// `Ok(_)` **and** `cs` was accepted. An `Err(bound)` returns before
/// classification, so a communications failure can never be reported as an
/// invalid chip-select.
fn spi_device_from_bound(
    bound: Result<u8, ValidateError>,
    cs: u8,
    drive: &mut dyn FnMut(u8) -> Result<(), SpiHalError>,
) -> Result<(), SpiHalError> {
    let num_gpios = bound.map_err(SpiHalError::DeviceInfo)?;
    classify_cs(cs, num_gpios)?;
    drive(cs)
}

impl embedded_hal::spi::Error for SpiHalError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

/// Error type for UART HAL operations.
#[derive(Debug)]
pub enum UartHalError {
    /// A UART-specific error from the device firmware.
    Uart(UartError),
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for UartHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Uart(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for UartHalError {}

impl From<PicoDeGalloError<UartError>> for UartHalError {
    fn from(e: PicoDeGalloError<UartError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::Uart(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

impl embedded_io::Error for UartHalError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::Uart(UartError::Overrun) => embedded_io::ErrorKind::Other,
            Self::Uart(UartError::Break) => embedded_io::ErrorKind::Other,
            Self::Uart(UartError::Parity) => embedded_io::ErrorKind::Other,
            Self::Uart(UartError::Framing) => embedded_io::ErrorKind::Other,
            Self::Uart(UartError::InvalidBaudRate) => embedded_io::ErrorKind::InvalidInput,
            _ => embedded_io::ErrorKind::Other,
        }
    }
}

// ----------------------------- Gpio -----------------------------

/// GPIO pin handle implementing [`embedded-hal`] digital traits.
///
/// Obtained from [`Hal::gpio`]. Each `Gpio` instance is bound to a specific
/// pin number (0–3) and can be used as both an input and output.
pub struct Gpio {
    pin: u8,
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
}

impl Gpio {
    /// Configure the pin's direction and internal pull resistor.
    ///
    /// After configuration, `set_low`/`set_high` on an input pin or
    /// `is_low`/`is_high` on an output pin will return
    /// [`GpioHalError::Gpio(GpioError::WrongDirection)`].
    pub fn set_config(
        &mut self,
        direction: GpioDirection,
        pull: GpioPull,
    ) -> std::result::Result<(), GpioHalError> {
        if Hal::in_async_context() {
            block_in_place(|| self.set_config_inner(direction, pull))
        } else {
            self.set_config_inner(direction, pull)
        }
    }

    fn set_config_inner(
        &mut self,
        direction: GpioDirection,
        pull: GpioPull,
    ) -> std::result::Result<(), GpioHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_set_config(self.pin, direction, pull))
            .map_err(GpioHalError::from)
    }

    fn set_low_inner(&mut self) -> std::result::Result<(), GpioHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_put(self.pin, GpioState::Low))
            .map_err(GpioHalError::from)
    }

    fn set_high_inner(&mut self) -> std::result::Result<(), GpioHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_put(self.pin, GpioState::High))
            .map_err(GpioHalError::from)
    }

    fn is_low_inner(&mut self) -> std::result::Result<bool, GpioHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_get(self.pin))
            .map_err(GpioHalError::from)
            .map(|s| s == GpioState::Low)
    }

    fn is_high_inner(&mut self) -> std::result::Result<bool, GpioHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_get(self.pin))
            .map_err(GpioHalError::from)
            .map(|s| s == GpioState::High)
    }

    /// Wait for the pin to go high, with a host-supplied timeout.
    ///
    /// Like the [`Wait::wait_for_high`](embedded_hal_async::digital::Wait::wait_for_high)
    /// trait method, but bounded. Returns
    /// `Err(GpioHalError::Gpio(GpioError::Timeout))` if the level is not
    /// reached within `timeout`.
    ///
    /// **Why this is project-specific:** `embedded-hal-async`'s `Wait`
    /// trait does not accept a timeout. Drivers that need bounded waits
    /// (recommended in production code; see
    /// `docs/ai-agents/pico-de-gallo-hal-examples.md` §6.6 gotchas)
    /// should use these methods directly instead of the trait.
    ///
    /// Available on firmware schema 0.7+.
    pub async fn wait_for_high_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<(), GpioHalError> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_high_with_timeout(self.pin, timeout)
            .await
            .map_err(GpioHalError::from)
    }

    /// Wait for the pin to go low, with a host-supplied timeout.
    ///
    /// See [`wait_for_high_with_timeout`](Self::wait_for_high_with_timeout)
    /// for the design rationale.
    pub async fn wait_for_low_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<(), GpioHalError> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_low_with_timeout(self.pin, timeout)
            .await
            .map_err(GpioHalError::from)
    }

    /// Wait for a rising edge on the pin, with a host-supplied timeout.
    ///
    /// See [`wait_for_high_with_timeout`](Self::wait_for_high_with_timeout)
    /// for the design rationale.
    pub async fn wait_for_rising_edge_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<(), GpioHalError> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_rising_edge_with_timeout(self.pin, timeout)
            .await
            .map_err(GpioHalError::from)
    }

    /// Wait for a falling edge on the pin, with a host-supplied timeout.
    ///
    /// See [`wait_for_high_with_timeout`](Self::wait_for_high_with_timeout)
    /// for the design rationale.
    pub async fn wait_for_falling_edge_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<(), GpioHalError> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_falling_edge_with_timeout(self.pin, timeout)
            .await
            .map_err(GpioHalError::from)
    }

    /// Wait for any edge on the pin, with a host-supplied timeout.
    ///
    /// See [`wait_for_high_with_timeout`](Self::wait_for_high_with_timeout)
    /// for the design rationale.
    pub async fn wait_for_any_edge_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<(), GpioHalError> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_any_edge_with_timeout(self.pin, timeout)
            .await
            .map_err(GpioHalError::from)
    }
}

impl embedded_hal::digital::ErrorType for Gpio {
    type Error = GpioHalError;
}

impl embedded_hal::digital::OutputPin for Gpio {
    fn set_low(&mut self) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.set_low_inner())
        } else {
            self.set_low_inner()
        }
    }

    fn set_high(&mut self) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.set_high_inner())
        } else {
            self.set_high_inner()
        }
    }
}

impl embedded_hal::digital::InputPin for Gpio {
    fn is_low(&mut self) -> std::result::Result<bool, Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.is_low_inner())
        } else {
            self.is_low_inner()
        }
    }

    fn is_high(&mut self) -> std::result::Result<bool, Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.is_high_inner())
        } else {
            self.is_high_inner()
        }
    }
}

impl embedded_hal::digital::StatefulOutputPin for Gpio {
    fn is_set_low(&mut self) -> std::result::Result<bool, Self::Error> {
        self.is_low_inner()
    }

    fn is_set_high(&mut self) -> std::result::Result<bool, Self::Error> {
        self.is_high_inner()
    }
}

impl embedded_hal_async::digital::Wait for Gpio {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_high(self.pin)
            .await
            .map_err(GpioHalError::from)
    }

    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_low(self.pin)
            .await
            .map_err(GpioHalError::from)
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_rising_edge(self.pin)
            .await
            .map_err(GpioHalError::from)
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_falling_edge(self.pin)
            .await
            .map_err(GpioHalError::from)
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_any_edge(self.pin)
            .await
            .map_err(GpioHalError::from)
    }
}

// ----------------------------- I2c -----------------------------

/// I2C bus handle implementing [`embedded-hal`] I2C traits.
///
/// Obtained from [`Hal::i2c`]. Supports 7-bit addressing. The I2C bus clock
/// frequency can be changed at runtime with [`Hal::i2c_set_config`].
pub struct I2c {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
}

impl I2c {
    fn transaction_inner(
        &mut self,
        address: embedded_hal::i2c::SevenBitAddress,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> std::result::Result<(), I2cHalError> {
        use pico_de_gallo_lib::I2cBatchOp;

        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());

        // Build batch ops
        let batch_ops: Vec<I2cBatchOp<'_>> = operations
            .iter()
            .map(|op| match op {
                embedded_hal::i2c::Operation::Read(buf) => I2cBatchOp::Read {
                    len: buf.len() as u16,
                },
                embedded_hal::i2c::Operation::Write(data) => I2cBatchOp::Write { data },
            })
            .collect();

        let result = handle
            .block_on(gallo.i2c_batch(address, &batch_ops))
            .map_err(I2cHalError::from)?;

        // Distribute read data back into the caller's buffers
        let mut offset = 0usize;
        for op in operations.iter_mut() {
            if let embedded_hal::i2c::Operation::Read(buf) = op {
                let len = buf.len();
                buf.copy_from_slice(&result[offset..offset + len]);
                offset += len;
            }
        }

        Ok(())
    }
}

impl embedded_hal::i2c::ErrorType for I2c {
    type Error = I2cHalError;
}

impl embedded_hal::i2c::I2c<embedded_hal::i2c::SevenBitAddress> for I2c {
    fn transaction(
        &mut self,
        address: embedded_hal::i2c::SevenBitAddress,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.transaction_inner(address, operations))
        } else {
            self.transaction_inner(address, operations)
        }
    }
}

impl embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress> for I2c {
    async fn transaction(
        &mut self,
        address: embedded_hal_async::i2c::SevenBitAddress,
        operations: &mut [embedded_hal_async::i2c::Operation<'_>],
    ) -> std::result::Result<(), Self::Error> {
        use pico_de_gallo_lib::I2cBatchOp;

        let gallo = self.gallo.lock().await;

        let batch_ops: Vec<I2cBatchOp<'_>> = operations
            .iter()
            .map(|op| match op {
                embedded_hal_async::i2c::Operation::Read(buf) => I2cBatchOp::Read {
                    len: buf.len() as u16,
                },
                embedded_hal_async::i2c::Operation::Write(data) => I2cBatchOp::Write { data },
            })
            .collect();

        let result = gallo
            .i2c_batch(address, &batch_ops)
            .await
            .map_err(I2cHalError::from)?;

        let mut offset = 0usize;
        for op in operations.iter_mut() {
            if let embedded_hal_async::i2c::Operation::Read(buf) = op {
                let len = buf.len();
                buf.copy_from_slice(&result[offset..offset + len]);
                offset += len;
            }
        }

        Ok(())
    }
}

// ----------------------------- Spi -----------------------------

/// SPI bus handle implementing [`embedded-hal`] SPI traits.
///
/// Obtained from [`Hal::spi`]. Supports full-duplex transfers. The SPI clock
/// frequency, phase, and polarity can be changed at runtime with
/// [`Hal::spi_set_config`].
pub struct Spi {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
}

impl Spi {
    fn read_inner(&mut self, words: &mut [u8]) -> std::result::Result<(), SpiHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        let contents = handle
            .block_on(gallo.spi_read(words.len() as u16))
            .map_err(SpiHalError::from)?;
        words.copy_from_slice(&contents);
        Ok(())
    }

    fn write_inner(&mut self, words: &[u8]) -> std::result::Result<(), SpiHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.spi_write(words))
            .map_err(SpiHalError::from)
    }

    fn transfer_inner(
        &mut self,
        read: &mut [u8],
        write: &[u8],
    ) -> std::result::Result<(), SpiHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        let contents = handle
            .block_on(gallo.spi_transfer(write))
            .map_err(SpiHalError::from)?;
        let len = read.len().min(contents.len());
        read[..len].copy_from_slice(&contents[..len]);
        Ok(())
    }

    fn flush_inner(&mut self) -> std::result::Result<(), SpiHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.spi_flush())
            .map_err(SpiHalError::from)
    }
}

impl embedded_hal::spi::ErrorType for Spi {
    type Error = SpiHalError;
}

impl embedded_hal::spi::SpiBus for Spi {
    fn read(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.read_inner(words))
        } else {
            self.read_inner(words)
        }
    }

    fn write(&mut self, words: &[u8]) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.write_inner(words))
        } else {
            self.write_inner(words)
        }
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.transfer_inner(read, write))
        } else {
            self.transfer_inner(read, write)
        }
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| {
                let write_copy = words.to_vec();
                self.transfer_inner(words, &write_copy)
            })
        } else {
            let write_copy = words.to_vec();
            self.transfer_inner(words, &write_copy)
        }
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.flush_inner())
        } else {
            self.flush_inner()
        }
    }
}

impl embedded_hal_async::spi::SpiBus for Spi {
    async fn read(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        let contents = gallo
            .spi_read(words.len() as u16)
            .await
            .map_err(SpiHalError::from)?;
        words.copy_from_slice(&contents);
        Ok(())
    }

    async fn write(&mut self, words: &[u8]) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo.spi_write(words).await.map_err(SpiHalError::from)
    }

    async fn transfer(
        &mut self,
        read: &mut [u8],
        write: &[u8],
    ) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        let contents = gallo.spi_transfer(write).await.map_err(SpiHalError::from)?;
        let len = read.len().min(contents.len());
        read[..len].copy_from_slice(&contents[..len]);
        Ok(())
    }

    async fn transfer_in_place(
        &mut self,
        words: &mut [u8],
    ) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        let write_copy = words.to_vec();
        let contents = gallo
            .spi_transfer(&write_copy)
            .await
            .map_err(SpiHalError::from)?;
        let len = words.len().min(contents.len());
        words[..len].copy_from_slice(&contents[..len]);
        Ok(())
    }

    async fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo.spi_flush().await.map_err(SpiHalError::from)
    }
}

// ----------------------------- SpiDevice ----------------------------

/// SPI device handle implementing [`embedded-hal`] [`SpiDevice`](embedded_hal::spi::SpiDevice) traits.
///
/// Obtained from [`Hal::spi_device`]. Wraps the SPI bus with firmware-managed
/// chip-select (CS) assertion via a GPIO pin. Each call to
/// [`transaction`](embedded_hal::spi::SpiDevice::transaction) will:
///
/// 1. Assert CS (drive low)
/// 2. Perform all requested operations
/// 3. Flush the bus
/// 4. Deassert CS (drive high)
///
/// # Cancellation Safety
///
/// The async [`SpiDevice`](embedded_hal_async::spi::SpiDevice) implementation
/// is **not** cancellation-safe. If the future returned by `transaction()` is
/// dropped after CS is asserted but before it is deasserted, the CS line will
/// remain low. This matches the behavior of `embedded-hal-bus::ExclusiveDevice`.
///
/// # CS Pin Ownership
///
/// The caller is responsible for ensuring that the CS pin is not used
/// concurrently by other [`Gpio`] handles or [`SpiDev`] instances.
pub struct SpiDev {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
    cs_pin: u8,
}

impl SpiDev {
    /// Execute a blocking SPI transaction with CS management.
    fn transaction_inner(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> std::result::Result<(), SpiHalError> {
        use pico_de_gallo_lib::SpiBatchOp;

        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());

        // Build batch ops. TransferInPlace needs a copy of the write data
        // so we can reference it in the batch. Track which indices need fixup.
        let mut in_place_bufs: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut batch_ops: Vec<SpiBatchOp<'_>> = operations
            .iter()
            .enumerate()
            .map(|(i, op)| match op {
                embedded_hal::spi::Operation::Read(buf) => SpiBatchOp::Read {
                    len: buf.len() as u16,
                },
                embedded_hal::spi::Operation::Write(data) => SpiBatchOp::Write { data },
                embedded_hal::spi::Operation::Transfer(_read, write) => {
                    SpiBatchOp::Transfer { data: write }
                }
                embedded_hal::spi::Operation::TransferInPlace(buf) => {
                    in_place_bufs.push((i, buf.to_vec()));
                    SpiBatchOp::Transfer { data: &[] }
                }
                embedded_hal::spi::Operation::DelayNs(ns) => SpiBatchOp::DelayNs { ns: *ns },
            })
            .collect();

        // Fix up TransferInPlace references to point at the saved copies
        for (idx, buf) in &in_place_bufs {
            batch_ops[*idx] = SpiBatchOp::Transfer { data: buf };
        }

        let result = handle
            .block_on(gallo.spi_batch(self.cs_pin, &batch_ops))
            .map_err(SpiHalError::from)?;

        // Distribute read/transfer data back into the caller's buffers
        let mut offset = 0usize;
        for op in operations.iter_mut() {
            match op {
                embedded_hal::spi::Operation::Read(buf) => {
                    let len = buf.len();
                    buf.copy_from_slice(&result[offset..offset + len]);
                    offset += len;
                }
                embedded_hal::spi::Operation::Transfer(read, _write) => {
                    let len = read.len();
                    read.copy_from_slice(&result[offset..offset + len]);
                    offset += len;
                }
                embedded_hal::spi::Operation::TransferInPlace(buf) => {
                    let len = buf.len();
                    buf.copy_from_slice(&result[offset..offset + len]);
                    offset += len;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

impl embedded_hal::spi::ErrorType for SpiDev {
    type Error = SpiHalError;
}

impl embedded_hal::spi::SpiDevice for SpiDev {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.transaction_inner(operations))
        } else {
            self.transaction_inner(operations)
        }
    }
}

impl embedded_hal_async::spi::SpiDevice for SpiDev {
    async fn transaction(
        &mut self,
        operations: &mut [embedded_hal_async::spi::Operation<'_, u8>],
    ) -> std::result::Result<(), Self::Error> {
        use pico_de_gallo_lib::SpiBatchOp;

        let gallo = self.gallo.lock().await;

        // Build batch ops. TransferInPlace needs a copy of the write data.
        let mut in_place_bufs: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut batch_ops: Vec<SpiBatchOp<'_>> = operations
            .iter()
            .enumerate()
            .map(|(i, op)| match op {
                embedded_hal_async::spi::Operation::Read(buf) => SpiBatchOp::Read {
                    len: buf.len() as u16,
                },
                embedded_hal_async::spi::Operation::Write(data) => SpiBatchOp::Write { data },
                embedded_hal_async::spi::Operation::Transfer(_read, write) => {
                    SpiBatchOp::Transfer { data: write }
                }
                embedded_hal_async::spi::Operation::TransferInPlace(buf) => {
                    in_place_bufs.push((i, buf.to_vec()));
                    SpiBatchOp::Transfer { data: &[] }
                }
                embedded_hal_async::spi::Operation::DelayNs(ns) => SpiBatchOp::DelayNs { ns: *ns },
            })
            .collect();

        // Fix up TransferInPlace references to point at the saved copies
        for (idx, buf) in &in_place_bufs {
            batch_ops[*idx] = SpiBatchOp::Transfer { data: buf };
        }

        let result = gallo
            .spi_batch(self.cs_pin, &batch_ops)
            .await
            .map_err(SpiHalError::from)?;

        // Distribute read/transfer data back into the caller's buffers
        let mut offset = 0usize;
        for op in operations.iter_mut() {
            match op {
                embedded_hal_async::spi::Operation::Read(buf) => {
                    let len = buf.len();
                    buf.copy_from_slice(&result[offset..offset + len]);
                    offset += len;
                }
                embedded_hal_async::spi::Operation::Transfer(read, _write) => {
                    let len = read.len();
                    read.copy_from_slice(&result[offset..offset + len]);
                    offset += len;
                }
                embedded_hal_async::spi::Operation::TransferInPlace(buf) => {
                    let len = buf.len();
                    buf.copy_from_slice(&result[offset..offset + len]);
                    offset += len;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// ----------------------------- Delay -----------------------------

/// Delay provider using host-side timers.
///
/// Obtained from [`Hal::delay`]. Uses [`std::thread::sleep`] for blocking
/// delays and [`tokio::time::sleep`] for async delays.
pub struct Delay;

impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        std::thread::sleep(std::time::Duration::from_nanos(ns.into()))
    }
}

impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        tokio::time::sleep(tokio::time::Duration::from_nanos(ns.into())).await
    }
}

// ----------------------------- Uart -----------------------------

/// UART handle implementing [`embedded-io`] traits.
///
/// Obtained from [`Hal::uart`]. Supports blocking and async read/write.
/// **Baud rate is fixed at the firmware default** and cannot be changed
/// through this HAL — to change baud, depend on `pico-de-gallo-lib`
/// directly and call `PicoDeGallo::uart_set_config`.
///
/// **Read timeout**: UART reads use a configurable timeout (in
/// milliseconds) to avoid blocking the USB bridge indefinitely. The
/// default timeout is 1000 ms. Adjust with [`Uart::set_timeout_ms`].
pub struct Uart {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
    timeout_ms: u32,
}

impl Uart {
    /// Set the read timeout in milliseconds.
    ///
    /// This controls how long [`embedded_io::Read::read`] waits for
    /// data before returning an empty result.  A value of 0 means
    /// non-blocking: return whatever is buffered immediately.
    pub fn set_timeout_ms(&mut self, timeout_ms: u32) {
        self.timeout_ms = timeout_ms;
    }

    fn read_inner(&mut self, buf: &mut [u8]) -> std::result::Result<usize, UartHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        let contents = handle
            .block_on(gallo.uart_read(buf.len() as u16, self.timeout_ms))
            .map_err(UartHalError::from)?;
        let n = contents.len().min(buf.len());
        buf[..n].copy_from_slice(&contents[..n]);
        Ok(n)
    }

    fn write_inner(&mut self, buf: &[u8]) -> std::result::Result<usize, UartHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.uart_write(buf))
            .map_err(UartHalError::from)?;
        Ok(buf.len())
    }

    fn flush_inner(&mut self) -> std::result::Result<(), UartHalError> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.uart_flush())
            .map_err(UartHalError::from)
    }
}

impl embedded_io::ErrorType for Uart {
    type Error = UartHalError;
}

impl embedded_io::Read for Uart {
    fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.read_inner(buf))
        } else {
            self.read_inner(buf)
        }
    }
}

impl embedded_io::Write for Uart {
    fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.write_inner(buf))
        } else {
            self.write_inner(buf)
        }
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        if Hal::in_async_context() {
            block_in_place(|| self.flush_inner())
        } else {
            self.flush_inner()
        }
    }
}

impl embedded_io_async::Read for Uart {
    async fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, Self::Error> {
        let gallo = self.gallo.lock().await;
        let contents = gallo
            .uart_read(buf.len() as u16, self.timeout_ms)
            .await
            .map_err(UartHalError::from)?;
        let n = contents.len().min(buf.len());
        buf[..n].copy_from_slice(&contents[..n]);
        Ok(n)
    }
}

impl embedded_io_async::Write for Uart {
    async fn write(&mut self, buf: &[u8]) -> std::result::Result<usize, Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo.uart_write(buf).await.map_err(UartHalError::from)?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo.uart_flush().await.map_err(UartHalError::from)
    }
}

// ---------------------------------------------------------------------------
// PWM
// ---------------------------------------------------------------------------

/// Error type for PWM HAL operations.
#[derive(Debug)]
pub enum PwmHalError {
    /// A PWM-specific error from the device firmware.
    Pwm(PwmError),
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for PwmHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Pwm(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for PwmHalError {}

impl From<PicoDeGalloError<PwmError>> for PwmHalError {
    fn from(e: PicoDeGalloError<PwmError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::Pwm(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

impl embedded_hal::pwm::Error for PwmHalError {
    fn kind(&self) -> embedded_hal::pwm::ErrorKind {
        embedded_hal::pwm::ErrorKind::Other
    }
}

/// A single PWM channel on the Pico de Gallo board.
///
/// Obtained from [`Hal::pwm_channel`]. Implements the [`embedded_hal::pwm::SetDutyCycle`]
/// trait.
///
/// Channels 0–1 share PWM slice 6, channels 2–3 share slice 7.
/// Enable/disable and configuration changes affect the entire slice.
pub struct PwmChannel {
    channel: u8,
    gallo: Arc<tokio::sync::Mutex<PicoDeGallo>>,
    handle: Handle,
}

impl embedded_hal::pwm::ErrorType for PwmChannel {
    type Error = PwmHalError;
}

impl embedded_hal::pwm::SetDutyCycle for PwmChannel {
    fn max_duty_cycle(&self) -> u16 {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.pwm_get_duty_cycle(self.channel))
            .map(|info| info.max_duty)
            .unwrap_or(u16::MAX)
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Self::Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.pwm_set_duty_cycle(self.channel, duty))
            .map_err(PwmHalError::from)
    }
}

// ---------------------------------------------------------------------------
// ADC
// ---------------------------------------------------------------------------

/// Error type for ADC HAL operations.
#[derive(Debug)]
pub enum AdcHalError {
    /// An ADC-specific error from the device firmware.
    Adc(AdcError),
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for AdcHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Adc(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for AdcHalError {}

impl From<PicoDeGalloError<AdcError>> for AdcHalError {
    fn from(e: PicoDeGalloError<AdcError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::Adc(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

/// Error type for 1-Wire HAL operations.
#[derive(Debug)]
pub enum OneWireHalError {
    /// A 1-Wire-specific error from the device firmware.
    OneWire(OneWireError),
    /// A USB communication error.
    Comms(String),
}

impl core::fmt::Display for OneWireHalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OneWire(e) => write!(f, "{e}"),
            Self::Comms(msg) => write!(f, "communication error: {msg}"),
        }
    }
}

impl std::error::Error for OneWireHalError {}

impl From<PicoDeGalloError<OneWireError>> for OneWireHalError {
    fn from(e: PicoDeGalloError<OneWireError>) -> Self {
        match e {
            PicoDeGalloError::Endpoint(e) => Self::OneWire(e),
            PicoDeGalloError::Comms(c) => Self::Comms(format!("{c:?}")),
        }
    }
}

/// 1-Wire bus handle backed by PIO hardware on the firmware.
///
/// There is no standard embedded-hal trait for 1-Wire. This type provides
/// blocking methods that mirror the async methods on [`PicoDeGallo`](pico_de_gallo_lib::PicoDeGallo).
pub struct OneWire {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
}

impl OneWire {
    /// Perform a bus reset and detect device presence.
    ///
    /// Returns `true` if one or more devices responded with a presence pulse.
    pub fn reset(&self) -> Result<bool, OneWireHalError> {
        if Hal::in_async_context() {
            block_in_place(|| self.handle.block_on(self.reset_inner()))
        } else {
            self.handle.block_on(self.reset_inner())
        }
    }

    async fn reset_inner(&self) -> Result<bool, OneWireHalError> {
        self.gallo
            .lock()
            .await
            .onewire_reset()
            .await
            .map_err(OneWireHalError::from)
    }

    /// Read `len` bytes from the 1-Wire bus.
    pub fn read(&self, len: u16) -> Result<Vec<u8>, OneWireHalError> {
        if Hal::in_async_context() {
            block_in_place(|| self.handle.block_on(self.read_inner(len)))
        } else {
            self.handle.block_on(self.read_inner(len))
        }
    }

    async fn read_inner(&self, len: u16) -> Result<Vec<u8>, OneWireHalError> {
        self.gallo
            .lock()
            .await
            .onewire_read(len)
            .await
            .map_err(OneWireHalError::from)
    }

    /// Write raw bytes to the 1-Wire bus.
    pub fn write(&self, data: &[u8]) -> Result<(), OneWireHalError> {
        if Hal::in_async_context() {
            block_in_place(|| self.handle.block_on(self.write_inner(data)))
        } else {
            self.handle.block_on(self.write_inner(data))
        }
    }

    async fn write_inner(&self, data: &[u8]) -> Result<(), OneWireHalError> {
        self.gallo
            .lock()
            .await
            .onewire_write(data)
            .await
            .map_err(OneWireHalError::from)
    }

    /// Write bytes then apply a strong pullup for parasitic-power devices.
    pub fn write_pullup(
        &self,
        data: &[u8],
        pullup_duration_ms: u16,
    ) -> Result<(), OneWireHalError> {
        if Hal::in_async_context() {
            block_in_place(|| {
                self.handle
                    .block_on(self.write_pullup_inner(data, pullup_duration_ms))
            })
        } else {
            self.handle
                .block_on(self.write_pullup_inner(data, pullup_duration_ms))
        }
    }

    async fn write_pullup_inner(
        &self,
        data: &[u8],
        pullup_duration_ms: u16,
    ) -> Result<(), OneWireHalError> {
        self.gallo
            .lock()
            .await
            .onewire_write_pullup(data, pullup_duration_ms)
            .await
            .map_err(OneWireHalError::from)
    }

    /// Start a new ROM search and return the first device address.
    pub fn search(&self) -> Result<Option<u64>, OneWireHalError> {
        if Hal::in_async_context() {
            block_in_place(|| self.handle.block_on(self.search_inner()))
        } else {
            self.handle.block_on(self.search_inner())
        }
    }

    async fn search_inner(&self) -> Result<Option<u64>, OneWireHalError> {
        self.gallo
            .lock()
            .await
            .onewire_search()
            .await
            .map_err(OneWireHalError::from)
    }

    /// Continue the current ROM search and return the next device address.
    pub fn search_next(&self) -> Result<Option<u64>, OneWireHalError> {
        if Hal::in_async_context() {
            block_in_place(|| self.handle.block_on(self.search_next_inner()))
        } else {
            self.handle.block_on(self.search_next_inner())
        }
    }

    async fn search_next_inner(&self) -> Result<Option<u64>, OneWireHalError> {
        self.gallo
            .lock()
            .await
            .onewire_search_next()
            .await
            .map_err(OneWireHalError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Error kind tests ---

    #[test]
    fn digital_error_kind_is_other() {
        use embedded_hal::digital::Error as _;
        let err = GpioHalError::Gpio(GpioError::Other);
        assert_eq!(err.kind(), embedded_hal::digital::ErrorKind::Other);
    }

    #[test]
    fn i2c_error_kind_nack() {
        use embedded_hal::i2c::Error as _;
        let err = I2cHalError::I2c(I2cError::NoAcknowledge);
        assert_eq!(
            err.kind(),
            embedded_hal::i2c::ErrorKind::NoAcknowledge(
                embedded_hal::i2c::NoAcknowledgeSource::Unknown
            )
        );
    }

    #[test]
    fn i2c_error_kind_bus() {
        use embedded_hal::i2c::Error as _;
        let err = I2cHalError::I2c(I2cError::Bus);
        assert_eq!(err.kind(), embedded_hal::i2c::ErrorKind::Bus);
    }

    #[test]
    fn i2c_error_kind_arbitration_loss() {
        use embedded_hal::i2c::Error as _;
        let err = I2cHalError::I2c(I2cError::ArbitrationLoss);
        assert_eq!(err.kind(), embedded_hal::i2c::ErrorKind::ArbitrationLoss);
    }

    #[test]
    fn i2c_error_kind_overrun() {
        use embedded_hal::i2c::Error as _;
        let err = I2cHalError::I2c(I2cError::Overrun);
        assert_eq!(err.kind(), embedded_hal::i2c::ErrorKind::Overrun);
    }

    #[test]
    fn i2c_error_kind_other_for_comms() {
        use embedded_hal::i2c::Error as _;
        let err = I2cHalError::Comms("USB disconnected".into());
        assert_eq!(err.kind(), embedded_hal::i2c::ErrorKind::Other);
    }

    #[test]
    fn spi_error_kind_is_other() {
        use embedded_hal::spi::Error as _;
        let err = SpiHalError::Spi(SpiError::Other);
        assert_eq!(err.kind(), embedded_hal::spi::ErrorKind::Other);
    }

    #[test]
    fn spi_device_error_type_matches_spi_bus() {
        // SpiDev and Spi share the same error type (SpiHalError),
        // so drivers can mix SpiBus and SpiDevice errors.
        fn assert_error_type<T: embedded_hal::spi::ErrorType<Error = SpiHalError>>() {}
        assert_error_type::<Spi>();
        assert_error_type::<SpiDev>();
    }

    #[test]
    fn spi_device_comms_error_kind_is_other() {
        use embedded_hal::spi::Error as _;
        let err = SpiHalError::Comms("CS assert failed".into());
        assert_eq!(err.kind(), embedded_hal::spi::ErrorKind::Other);
    }

    #[test]
    fn uart_error_kind_invalid_baud_rate() {
        use embedded_io::Error as _;
        let err = UartHalError::Uart(UartError::InvalidBaudRate);
        assert_eq!(err.kind(), embedded_io::ErrorKind::InvalidInput);
    }

    #[test]
    fn uart_error_kind_overrun() {
        use embedded_io::Error as _;
        let err = UartHalError::Uart(UartError::Overrun);
        assert_eq!(err.kind(), embedded_io::ErrorKind::Other);
    }

    #[test]
    fn uart_error_kind_comms() {
        use embedded_io::Error as _;
        let err = UartHalError::Comms("USB disconnected".into());
        assert_eq!(err.kind(), embedded_io::ErrorKind::Other);
    }

    #[test]
    fn uart_hal_error_display() {
        let err = UartHalError::Uart(UartError::Framing);
        assert_eq!(format!("{err}"), "UART framing error");

        let err = UartHalError::Comms("timeout".into());
        assert_eq!(format!("{err}"), "communication error: timeout");
    }

    #[test]
    fn uart_hal_error_from_endpoint() {
        let e: UartHalError = PicoDeGalloError::Endpoint(UartError::Break).into();
        assert!(matches!(e, UartHalError::Uart(UartError::Break)));
    }

    #[test]
    fn uart_hal_error_from_comms() {
        let e = UartHalError::Comms("USB disconnected".into());
        assert!(matches!(e, UartHalError::Comms(_)));
    }

    // --- PWM error tests ---

    #[test]
    fn pwm_error_kind_is_other() {
        use embedded_hal::pwm::Error as _;
        let err = PwmHalError::Pwm(PwmError::InvalidChannel);
        assert_eq!(err.kind(), embedded_hal::pwm::ErrorKind::Other);
    }

    #[test]
    fn pwm_comms_error_kind_is_other() {
        use embedded_hal::pwm::Error as _;
        let err = PwmHalError::Comms("timeout".into());
        assert_eq!(err.kind(), embedded_hal::pwm::ErrorKind::Other);
    }

    #[test]
    fn pwm_hal_error_display_endpoint() {
        let err = PwmHalError::Pwm(PwmError::InvalidChannel);
        assert_eq!(format!("{err}"), "invalid PWM channel");
    }

    #[test]
    fn pwm_hal_error_display_comms() {
        let err = PwmHalError::Comms("USB gone".into());
        assert_eq!(format!("{err}"), "communication error: USB gone");
    }

    // --- Runtime detection tests ---

    #[test]
    fn handle_try_current_fails_outside_tokio() {
        // Outside any tokio runtime, try_current should fail.
        // This is the code path that causes Hal::new_inner to create
        // its own Runtime.
        let result = Handle::try_current();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn handle_try_current_succeeds_inside_tokio() {
        // Inside a tokio runtime, try_current should succeed.
        // This is the code path where Hal::new_inner reuses the
        // existing runtime handle.
        let result = Handle::try_current();
        assert!(result.is_ok());
    }

    // --- Delay unit tests ---

    #[test]
    fn delay_ns_does_not_panic() {
        use embedded_hal::delay::DelayNs;
        let mut delay = Delay;
        // Just verify it doesn't panic for a tiny delay
        delay.delay_ns(1);
    }

    #[tokio::test]
    async fn async_delay_ns_does_not_panic() {
        use embedded_hal_async::delay::DelayNs;
        let mut delay = Delay;
        delay.delay_ns(1).await;
    }

    // --- ADC error tests ---

    #[test]
    fn adc_hal_error_display_conversion_failed() {
        let err = AdcHalError::Adc(AdcError::ConversionFailed);
        assert_eq!(format!("{err}"), "ADC conversion failed");
    }

    #[test]
    fn adc_hal_error_display_comms() {
        let err = AdcHalError::Comms("timeout".into());
        assert_eq!(format!("{err}"), "communication error: timeout");
    }

    #[test]
    fn adc_hal_error_from_endpoint() {
        let e: PicoDeGalloError<AdcError> = PicoDeGalloError::Endpoint(AdcError::Other);
        let hal_err = AdcHalError::from(e);
        match hal_err {
            AdcHalError::Adc(AdcError::Other) => {}
            other => panic!("expected Adc(Other), got {other:?}"),
        }
    }

    // --- 1-Wire error tests ---

    #[test]
    fn onewire_hal_error_display_no_presence() {
        let err = OneWireHalError::OneWire(OneWireError::NoPresence);
        assert_eq!(format!("{err}"), "no device present on 1-Wire bus");
    }

    #[test]
    fn onewire_hal_error_display_comms() {
        let err = OneWireHalError::Comms("timeout".into());
        assert_eq!(format!("{err}"), "communication error: timeout");
    }

    #[test]
    fn onewire_hal_error_from_endpoint() {
        let e: PicoDeGalloError<OneWireError> = PicoDeGalloError::Endpoint(OneWireError::BusError);
        let hal_err = OneWireHalError::from(e);
        match hal_err {
            OneWireHalError::OneWire(OneWireError::BusError) => {}
            other => panic!("expected OneWire(BusError), got {other:?}"),
        }
    }

    // ===================================================================
    // M3 — SPI chip-select bounds (issue #104)
    // ===================================================================

    fn all_validate_errors() -> Vec<ValidateError> {
        vec![
            ValidateError::Comms(pico_de_gallo_lib::HostErr::Closed),
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

    fn all_spi_hal_errors() -> Vec<SpiHalError> {
        vec![
            SpiHalError::Spi(SpiError::Other),
            SpiHalError::Comms("closed".to_string()),
            SpiHalError::DeviceInfo(ValidateError::Timeout),
            SpiHalError::NoGpios,
            SpiHalError::InvalidCsPin {
                cs: 255,
                num_gpios: 4,
            },
        ]
    }

    #[test]
    fn spi_hal_error_witness_is_exhaustive() {
        // Compile-time witness: appending a SpiHalError variant must fail to
        // *compile* here rather than fall through a wildcard.
        fn witness(e: &SpiHalError) -> u8 {
            match e {
                SpiHalError::Spi(_) => 0,
                SpiHalError::Comms(_) => 1,
                SpiHalError::DeviceInfo(_) => 2,
                SpiHalError::NoGpios => 3,
                SpiHalError::InvalidCsPin { .. } => 4,
            }
        }
        let tags: std::collections::HashSet<u8> =
            all_spi_hal_errors().iter().map(witness).collect();
        assert_eq!(tags.len(), 5);
    }

    #[test]
    fn classify_cs_accepts_zero_and_upper_boundary() {
        for (cs, n) in [(0u8, 4u8), (3, 4), (0, 7), (6, 7)] {
            classify_cs(cs, n).unwrap_or_else(|e| panic!("cs {cs} at n {n} must be valid: {e}"));
        }
    }

    #[test]
    fn classify_cs_rejects_bound_and_max_u8() {
        for (cs, n) in [(4u8, 4u8), (7, 7), (255, 4)] {
            match classify_cs(cs, n) {
                Err(SpiHalError::InvalidCsPin { cs: got, num_gpios }) => {
                    // The payload must echo the caller's index verbatim: a
                    // truncating implementation would report 3 for 255.
                    assert_eq!(got, cs);
                    assert_eq!(num_gpios, n);
                }
                other => panic!("cs {cs} at n {n} must be InvalidCsPin, got {other:?}"),
            }
        }
    }

    #[test]
    fn classify_cs_zero_bound_is_no_gpios_for_every_pin() {
        for cs in 0..=u8::MAX {
            match classify_cs(cs, 0) {
                Err(SpiHalError::NoGpios) => {}
                other => panic!("cs {cs} at n=0 must be NoGpios, got {other:?}"),
            }
        }
    }

    #[test]
    fn spi_hal_error_from_device_info_preserves_validate_variant() {
        use pico_de_gallo_lib::SpiBatchCallError;
        for e in all_validate_errors() {
            let want = format!("{e}");
            match SpiHalError::from(SpiBatchCallError::DeviceInfo(e)) {
                SpiHalError::DeviceInfo(v) => assert_eq!(format!("{v}"), want),
                other => panic!("metadata failure must stay DeviceInfo, got {other:?}"),
            }
        }
    }

    #[test]
    fn spi_hal_error_display_variants_are_pairwise_distinct() {
        let msgs: Vec<String> = all_spi_hal_errors()
            .iter()
            .map(|e| format!("{e}"))
            .collect();
        let unique: std::collections::HashSet<&String> = msgs.iter().collect();
        assert_eq!(unique.len(), msgs.len(), "collision in {msgs:?}");

        let timeout = format!("{}", SpiHalError::DeviceInfo(ValidateError::Timeout));
        assert!(timeout.contains("device/info"), "got: {timeout}");
    }

    #[test]
    fn spi_hal_error_kind_is_defined_for_every_variant() {
        use embedded_hal::spi::Error;
        for e in all_spi_hal_errors() {
            let _ = e.kind();
        }
    }

    #[test]
    fn spi_device_rejects_out_of_range_cs_without_driving_a_pin() {
        // The #104 property itself, executable: a refusal must never reach
        // the `gpio_put` that would reconfigure the caller's input pin as a
        // push-pull output.
        let mut drives: Vec<u8> = Vec::new();
        {
            let mut drive = |pin: u8| {
                drives.push(pin);
                Ok(())
            };
            assert!(matches!(
                spi_device_from_bound(Ok(4), 4, &mut drive),
                Err(SpiHalError::InvalidCsPin {
                    cs: 4,
                    num_gpios: 4
                })
            ));
            assert!(matches!(
                spi_device_from_bound(Ok(4), 255, &mut drive),
                Err(SpiHalError::InvalidCsPin { cs: 255, .. })
            ));
            assert!(matches!(
                spi_device_from_bound(Ok(0), 0, &mut drive),
                Err(SpiHalError::NoGpios)
            ));
            assert!(matches!(
                spi_device_from_bound(Err(ValidateError::Timeout), 0, &mut drive),
                Err(SpiHalError::DeviceInfo(ValidateError::Timeout))
            ));
        }
        assert!(drives.is_empty(), "a refusal drove pins {drives:?}");

        // Positive control: an accepted chip-select drives exactly its pin.
        let mut driven: Vec<u8> = Vec::new();
        {
            let mut drive = |pin: u8| {
                driven.push(pin);
                Ok(())
            };
            spi_device_from_bound(Ok(4), 3, &mut drive).expect("pin 3 is valid at n = 4");
        }
        assert_eq!(driven, vec![3]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn classify_cs_is_identical_inside_and_outside_async_context() {
        let cases = [(0u8, 4u8), (3, 4), (4, 4), (255, 4), (6, 7), (7, 7), (0, 0)];
        let plain: Vec<String> = cases
            .iter()
            .map(|(c, n)| format!("{:?}", classify_cs(*c, *n)))
            .collect();
        let inside: Vec<String> = block_in_place(|| {
            cases
                .iter()
                .map(|(c, n)| format!("{:?}", classify_cs(*c, *n)))
                .collect()
        });
        assert_eq!(plain, inside);
    }

    #[test]
    fn num_gpios_error_propagates_as_hal_init_error_validate() {
        let e = HalInitError::Validate(ValidateError::Timeout);
        let msg = format!("{e}");
        assert!(msg.contains("firmware validation failed"), "got: {msg}");
        assert!(msg.contains("device/info"), "got: {msg}");
        assert!(msg.contains("300"), "got: {msg}");
    }
}
