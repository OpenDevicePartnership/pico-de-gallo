use embedded_hal::i2c::{self, SevenBitAddress};
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use nusb::{list_devices, Interface, MaybeFuture};
use std::io::{self, Read, Write};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("nusb error")]
    Nusb(#[from] nusb::Error),
    #[error("io error")]
    Io(#[from] io::Error),
    #[error("device not found")]
    NotFound,
    #[error("unknown error")]
    Unknown,
}

pub type Result<T> = std::result::Result<T, Error>;

struct I2c {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl I2c {
    fn new(interface: Interface) -> Result<Self> {
        let writer = interface.endpoint::<Bulk, Out>(0x01)?.writer(4096);
        let reader = interface.endpoint::<Bulk, In>(0x81)?.reader(4096);

        Ok(Self { writer, reader })
    }

    /// I2c blocking read
    fn blocking_read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()> {
        let size = buf.len().to_le_bytes();

        let cmd = [0x00, addr, size[0], size[1]];
        dbg!(&cmd);

        self.writer.write_all(&cmd)?;
        self.writer.flush()?;

        // REVISIT: make sure response contains "SUCCESS"
        let mut response = [0; 512];
        let size = self.reader.read(&mut response[..(4 + buf.len())])?;

        // REMOVE THIS
        println!("READ: Got {size} bytes");

        buf.copy_from_slice(&response[4..size]);

        Ok(())
    }

    /// I2c blocking write
    fn blocking_write(&mut self, addr: u8, buf: &[u8]) -> Result<()> {
        let size = buf.len().to_le_bytes();

        let mut cmd = vec![0x01, addr, size[0], size[1]];
        cmd.extend_from_slice(buf);
        dbg!(&cmd);

        self.writer.write_all(&cmd)?;
        self.writer.flush()?;

        // REVISIT: make sure response contains "SUCCESS"
        let mut response = [0; 512];
        let size = self.reader.read(&mut response)?;

        // REMOVE THIS
        println!("Got {size} bytes");

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

struct Spi {
    interface: Interface,
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl Spi {
    fn new(interface: Interface) -> Result<Self> {
        let writer = interface.endpoint::<Bulk, Out>(0x02)?.writer(4096);
        let reader = interface.endpoint::<Bulk, In>(0x82)?.reader(4096);

        Ok(Self {
            interface,
            writer,
            reader,
        })
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
            .wait()?
            .find(|dev| dev.vendor_id() == 0x045e && dev.product_id() == 0x7069)
            .ok_or(Error::NotFound)?;

        let device = device.open().wait()?;

        let intf0 = device.claim_interface(0).wait()?;
        let intf1 = device.claim_interface(1).wait()?;

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
