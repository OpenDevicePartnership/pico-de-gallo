use delay::Delay;
use gpio::Gpio;
use i2c::{I2c, I2cError};
use nusb::{MaybeFuture, list_devices};
use spi::Spi;
use thiserror::Error;

pub mod delay;
pub mod gpio;
pub mod i2c;
pub mod spi;

#[derive(Error, Clone, Debug)]
pub enum Error {
    #[error("nusb error: {0}")]
    Nusb(nusb::Error),
    #[error("io error")]
    Io,
    #[error("device not found")]
    DeviceNotFound,
    #[error("I2C bus error")]
    I2c(I2cError),
    #[error("GPIO error")]
    Gpio,
    #[error("SPI bus error")]
    Spi,
    #[error("unknown error")]
    Unknown,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct PicoDeGallo {
    i2c: I2c,
    spi: Spi,
    gpio: Gpio,
    delay: Delay,
}

impl PicoDeGallo {
    /// Create a new instance for the Pico de Gallo device.
    pub fn new() -> Result<Self> {
        let device = list_devices()
            .wait()
            .map_err(|e| Error::Nusb(e))?
            .find(|dev| dev.vendor_id() == 0x045e && dev.product_id() == 0x7069)
            .ok_or(Error::DeviceNotFound)?;

        let device = device.open().wait().map_err(|e| Error::Nusb(e))?;

        let intf0 = device.claim_interface(0).wait().map_err(|e| Error::Nusb(e))?;
        let intf1 = device.claim_interface(1).wait().map_err(|e| Error::Nusb(e))?;
        let intf2 = device.claim_interface(2).wait().map_err(|e| Error::Nusb(e))?;

        let i2c = I2c::new(intf0)?;
        let spi = Spi::new(intf1)?;
        let gpio = Gpio::new(intf2)?;
        let delay = Delay;

        Ok(Self { i2c, spi, gpio, delay })
    }

    /// Split into its components
    pub fn split(self) -> (I2c, Spi, Gpio, Delay) {
        (self.i2c, self.spi, self.gpio, self.delay)
    }

    /// Get the underlying I2c component
    pub fn i2c(self) -> I2c {
        self.i2c
    }

    /// Get the underlying Spi component
    pub fn spi(self) -> Spi {
        self.spi
    }

    /// Get the underlying Gpio component
    pub fn gpio(self) -> Gpio {
        self.gpio
    }

    /// Get the underlying Delay component
    pub fn delay(self) -> Delay {
        self.delay
    }
}
