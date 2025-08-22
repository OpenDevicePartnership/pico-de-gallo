use crate::{Error, PicoDeGallo, Result};
use embedded_hal::spi;
use nusb::Interface;
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use std::io::{Read, Write};

#[allow(unused)]
pub(crate) struct Spi {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl Spi {
    pub(crate) fn new(interface: Interface) -> Result<Self> {
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

    /// SPI blocking read
    pub(crate) fn blocking_read(&mut self, words: &mut [u8]) -> Result<()> {
        let size = words.len().to_le_bytes();
        let cmd = [0x00, size[0], size[1]];

        self.writer.write_all(&cmd).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut response = vec![0; 3 + words.len()];

        let size = self
            .reader
            .read(&mut response[..(3 + words.len())])
            .map_err(|_| Error::Io)?;

        if size > 3 {
            words.copy_from_slice(&response[3..size]);
        }

        Ok(())
    }

    /// SPI blocking write
    pub(crate) fn blocking_write(&mut self, _words: &[u8]) -> Result<()> {
        todo!()
    }

    /// SPI blocking transfer
    pub(crate) fn blocking_transfer(&mut self, _read: &mut [u8], _write: &[u8]) -> Result<()> {
        todo!()
    }

    /// SPI blocking transfer in place
    pub fn blocking_transfer_in_place(&mut self, _words: &mut [u8]) -> Result<()> {
        todo!()
    }
}

impl spi::Error for Error {
    fn kind(&self) -> spi::ErrorKind {
        match *self {
            _ => spi::ErrorKind::Other,
        }
    }
}

impl spi::ErrorType for PicoDeGallo {
    type Error = Error;
}

impl spi::SpiBus for PicoDeGallo {
    fn read(&mut self, _words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        todo!()
    }

    fn write(&mut self, _words: &[u8]) -> std::result::Result<(), Self::Error> {
        todo!()
    }

    fn transfer(&mut self, _read: &mut [u8], _write: &[u8]) -> std::result::Result<(), Self::Error> {
        todo!()
    }

    fn transfer_in_place(&mut self, _words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        todo!()
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        todo!()
    }
}
