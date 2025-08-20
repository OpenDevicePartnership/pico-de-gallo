use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::{self, SevenBitAddress};
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use nusb::{list_devices, Interface, MaybeFuture};
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Clone, Copy, Debug)]
pub enum Error {
    #[error("io error")]
    Io,
    #[error("device not found")]
    NotFound,
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

struct I2c {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl I2c {
    fn new(interface: Interface) -> Result<Self> {
        let writer = interface
            .endpoint::<Bulk, Out>(0x01)
            .map_err(|_| Error::Io)?
            .writer(4096);
        let reader = interface
            .endpoint::<Bulk, In>(0x81)
            .map_err(|_| Error::Io)?
            .reader(4096);

        Ok(Self { writer, reader })
    }

    /// I2c blocking read
    fn blocking_read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()> {
        let size = buf.len().to_le_bytes();

        let cmd = [0x00, addr, size[0], size[1]];

        self.writer.write_all(&cmd).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        // REVISIT: make sure response contains "SUCCESS"
        let mut response = [0; 512];
        let size = self
            .reader
            .read(&mut response[..(4 + buf.len())])
            .map_err(|_| Error::Io)?;

        if size > 4 {
            buf.copy_from_slice(&response[4..size]);
        }

        Ok(())
    }

    /// I2c blocking write
    fn blocking_write(&mut self, addr: u8, buf: &[u8]) -> Result<()> {
        let size = buf.len().to_le_bytes();

        let mut cmd = vec![0x01, addr, size[0], size[1]];
        cmd.extend_from_slice(buf);

        self.writer.write_all(&cmd).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        // REVISIT: make sure response contains "SUCCESS"
        let mut response = [0; 512];
        self.reader.read(&mut response).map_err(|_| Error::Io)?;

        Ok(())
    }
}

impl i2c::Error for Error {
    fn kind(&self) -> i2c::ErrorKind {
        match *self {
            _ => i2c::ErrorKind::Bus,
        }
    }
}

impl i2c::ErrorType for PicoDeGallo {
    type Error = Error;
}

impl i2c::I2c<SevenBitAddress> for PicoDeGallo {
    fn transaction(
        &mut self,
        address: SevenBitAddress,
        operations: &mut [i2c::Operation<'_>],
    ) -> Result<()> {
        let address = address.into();

        for op in operations {
            match op {
                i2c::Operation::Read(read) => self.i2c_blocking_read(address, read)?,
                i2c::Operation::Write(write) => self.i2c_blocking_write(address, write)?,
            }
        }

        Ok(())
    }
}

#[allow(unused)]
struct Spi {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl Spi {
    fn new(interface: Interface) -> Result<Self> {
        let writer = interface
            .endpoint::<Bulk, Out>(0x02)
            .map_err(|_| Error::Io)?
            .writer(4096);
        let reader = interface
            .endpoint::<Bulk, In>(0x82)
            .map_err(|_| Error::Io)?
            .reader(4096);

        Ok(Self { writer, reader })
    }
}

pub struct PicoDeGallo {
    i2c: I2c,
    #[allow(unused)]
    spi: Spi,
}

impl PicoDeGallo {
    /// Create a new instance for the Pico de Gallo device.
    pub fn new() -> Result<Self> {
        let device = list_devices()
            .wait()
            .map_err(|_| Error::Io)?
            .find(|dev| dev.vendor_id() == 0x045e && dev.product_id() == 0x7069)
            .ok_or(Error::NotFound)?;

        let device = device.open().wait().map_err(|_| Error::Io)?;

        let intf0 = device.claim_interface(0).wait().map_err(|_| Error::Io)?;
        let intf1 = device.claim_interface(1).wait().map_err(|_| Error::Io)?;

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
}
