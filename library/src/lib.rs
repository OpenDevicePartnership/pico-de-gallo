use embedded_hal::delay::DelayNs;
use i2c::{I2c, I2cError};
use nusb::{MaybeFuture, list_devices};
use spi::Spi;
use std::thread;
use std::time::Duration;
use thiserror::Error;

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
    #[error("SPI bus error")]
    Spi,
    #[error("unknown error")]
    Unknown,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy)]
pub struct Delay;

impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        thread::sleep(Duration::from_nanos(ns.into()));
    }
}

pub struct PicoDeGallo {
    i2c: I2c,
    spi: Spi,
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

        let i2c = I2c::new(intf0)?;
        let spi = Spi::new(intf1)?;

        Ok(Self { i2c, spi })
    }

    /// I2c blocking read
    pub fn i2c_blocking_read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()> {
        self.i2c.blocking_read(addr, buf)
    }

    /// I2c blocking write
    pub fn i2c_blocking_write(&mut self, addr: u8, buf: &[u8]) -> Result<()> {
        self.i2c.blocking_write(addr, buf)
    }

    /// SPI blocking read
    pub fn spi_blocking_read(&mut self, words: &mut [u8]) -> Result<()> {
        self.spi.blocking_read(words)
    }

    /// SPI blocking write
    pub fn spi_blocking_write(&mut self, words: &[u8]) -> Result<()> {
        self.spi.blocking_write(words)
    }

    /// SPI blocking transfer
    pub fn spi_blocking_transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<()> {
        self.spi.blocking_transfer(read, write)
    }

    /// SPI blocking transfer in place
    pub fn spi_blocking_transfer_in_place(&mut self, words: &mut [u8]) -> Result<()> {
        self.spi.blocking_transfer_in_place(words)
    }

    /// SPI blocking flush
    pub fn spi_blocking_flush(&mut self) -> Result<()> {
        self.spi.blocking_flush()
    }
}
