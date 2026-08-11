//! Firmware application context, interrupt bindings, and shared helpers.

use embassy_rp::bind_interrupts;
use embassy_rp::gpio::Flex;
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, I2C1, SPI0, USB};
use embassy_rp::pwm::{self, Pwm};
use embassy_rp::spi::{self, Spi};
#[cfg(feature = "hw-rev2")]
use embassy_rp::{
    adc::{self, Adc},
    peripherals::{PIO0, UART0},
    pio_programs::onewire::{PioOneWire, PioOneWireSearch},
    uart::BufferedUart,
};
#[cfg(feature = "hw-rev2")]
use pico_de_gallo_internal::NUM_ADC_GPIO_CHANNELS;
use pico_de_gallo_internal::{I2cError, I2cFrequency, MAX_TRANSFER_SIZE, SpiPhase, SpiPolarity};

// Re-exported so the rest of the firmware keeps reaching this through
// `crate::context`, while the value itself lives in the wire crate where
// the C FFI can mirror it as GALLO_NUM_GPIOS.
pub(crate) use pico_de_gallo_internal::NUM_GPIOS;

bind_interrupts!(pub(crate) struct Irqs {
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
    DMA_IRQ_0 => embassy_rp::dma::InterruptHandler<DMA_CH0>, embassy_rp::dma::InterruptHandler<DMA_CH1>;
});

// UART and PIO interrupts only needed when those peripherals are active.
#[cfg(feature = "hw-rev2")]
bind_interrupts!(pub(crate) struct Rev2Irqs {
    UART0_IRQ => embassy_rp::uart::BufferedInterruptHandler<UART0>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
});

pub(crate) const NUM_PWM_SLICES: usize = 2;

/// System clock frequency in Hz.
///
/// Must match the `ClockConfig::system_freq()` value passed to `embassy_rp::init`.
pub(crate) const SYS_CLK_HZ: u32 = 150_000_000;

/// Number of ADC channels stored in Context (4 GPIO channels).
#[cfg(feature = "hw-rev2")]
pub(crate) const NUM_ADC_CHANNELS: usize = NUM_ADC_GPIO_CHANNELS;

/// Per-pin direction mode tracked by firmware.
///
/// Pins start in `LegacyAuto` mode, which preserves backward-compatible
/// behavior: `gpio_get` auto-switches to input, `gpio_put` auto-switches
/// to output. Once configured via `gpio/set-config`, the pin enters an
/// explicit mode and direction changes are no longer automatic.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinMode {
    /// Default: auto-switch direction on get/put (backward compatible).
    LegacyAuto,
    /// Explicitly configured as input via `gpio/set-config`.
    ExplicitInput,
    /// Explicitly configured as output via `gpio/set-config`.
    ExplicitOutput,
}

/// Firmware application context holding all peripheral handles.
///
/// NOTE: `buf` is shared between I2C and SPI handlers. This is safe because
/// postcard-rpc dispatches handlers serially (one at a time). If concurrent
/// dispatch is ever enabled, separate buffers would be required.
pub struct Context {
    pub(crate) i2c: I2c<'static, I2C1, i2c::Async>,
    pub(crate) spi: Spi<'static, SPI0, spi::Async>,
    #[cfg(feature = "hw-rev2")]
    pub(crate) uart: BufferedUart,
    pub(crate) gpios: [Option<Flex<'static>>; NUM_GPIOS],
    pub(crate) pin_modes: [PinMode; NUM_GPIOS],
    pub(crate) pwm_slices: [Pwm<'static>; NUM_PWM_SLICES],
    pub(crate) pwm_configs: [pwm::Config; NUM_PWM_SLICES],
    #[cfg(feature = "hw-rev2")]
    pub(crate) adc: Adc<'static, adc::Blocking>,
    #[cfg(feature = "hw-rev2")]
    pub(crate) adc_channels: [adc::Channel<'static>; NUM_ADC_CHANNELS],
    pub(crate) i2c_frequency: I2cFrequency,
    pub(crate) spi_frequency: u32,
    pub(crate) spi_phase: SpiPhase,
    pub(crate) spi_polarity: SpiPolarity,
    #[cfg_attr(not(feature = "hw-rev2"), allow(dead_code))]
    pub(crate) uart_baud_rate: u32,
    #[cfg(feature = "hw-rev2")]
    pub(crate) onewire: PioOneWire<'static, PIO0, 0>,
    #[cfg(feature = "hw-rev2")]
    pub(crate) onewire_search: PioOneWireSearch,
    pub(crate) buf: [u8; MAX_TRANSFER_SIZE],
}

impl Context {
    #[cfg(feature = "hw-rev2")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        i2c: I2c<'static, I2C1, i2c::Async>,
        spi: Spi<'static, SPI0, spi::Async>,
        uart: BufferedUart,
        gpios: [Flex<'static>; NUM_GPIOS],
        pwm_slices: [Pwm<'static>; NUM_PWM_SLICES],
        pwm_configs: [pwm::Config; NUM_PWM_SLICES],
        adc: Adc<'static, adc::Blocking>,
        adc_channels: [adc::Channel<'static>; NUM_ADC_CHANNELS],
        onewire: PioOneWire<'static, PIO0, 0>,
    ) -> Self {
        let [g0, g1, g2, g3] = gpios;
        Self {
            i2c,
            spi,
            uart,
            gpios: [Some(g0), Some(g1), Some(g2), Some(g3)],
            pin_modes: [PinMode::LegacyAuto; NUM_GPIOS],
            pwm_slices,
            pwm_configs,
            adc,
            adc_channels,
            i2c_frequency: I2cFrequency::Standard,
            spi_frequency: 1_000_000,
            spi_phase: SpiPhase::CaptureOnFirstTransition,
            spi_polarity: SpiPolarity::IdleLow,
            uart_baud_rate: 115_200,
            onewire,
            onewire_search: PioOneWireSearch::new(),
            buf: [0; MAX_TRANSFER_SIZE],
        }
    }

    #[cfg(not(feature = "hw-rev2"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        i2c: I2c<'static, I2C1, i2c::Async>,
        spi: Spi<'static, SPI0, spi::Async>,
        gpios: [Flex<'static>; NUM_GPIOS],
        pwm_slices: [Pwm<'static>; NUM_PWM_SLICES],
        pwm_configs: [pwm::Config; NUM_PWM_SLICES],
    ) -> Self {
        let [g0, g1, g2, g3] = gpios;
        Self {
            i2c,
            spi,
            gpios: [Some(g0), Some(g1), Some(g2), Some(g3)],
            pin_modes: [PinMode::LegacyAuto; NUM_GPIOS],
            pwm_slices,
            pwm_configs,
            i2c_frequency: I2cFrequency::Standard,
            spi_frequency: 1_000_000,
            spi_phase: SpiPhase::CaptureOnFirstTransition,
            spi_polarity: SpiPolarity::IdleLow,
            uart_baud_rate: 115_200,
            buf: [0; MAX_TRANSFER_SIZE],
        }
    }
}

/// Maps an embassy-rp I2C error to our wire protocol error type.
pub(crate) fn map_i2c_error(e: i2c::Error) -> I2cError {
    match e {
        i2c::Error::Abort(i2c::AbortReason::NoAcknowledge) => I2cError::NoAcknowledge,
        i2c::Error::Abort(i2c::AbortReason::ArbitrationLoss) => I2cError::ArbitrationLoss,
        i2c::Error::Abort(i2c::AbortReason::TxNotEmpty(_)) => I2cError::Overrun,
        i2c::Error::Abort(i2c::AbortReason::Other(_)) => I2cError::Bus,
        i2c::Error::InvalidReadBufferLength => I2cError::BufferTooLong,
        i2c::Error::InvalidWriteBufferLength => I2cError::BufferTooLong,
        i2c::Error::AddressOutOfRange(_) => I2cError::AddressOutOfRange,
        #[allow(deprecated)]
        i2c::Error::AddressReserved(_) => I2cError::AddressOutOfRange,
    }
}
