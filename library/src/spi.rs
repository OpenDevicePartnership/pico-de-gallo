use crate::{Error, Result};
use embedded_hal::spi;
use nusb::Interface;
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use postcard::{from_bytes, to_stdvec};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const USB_BUFFER_SIZE: usize = 1024;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum Opcode {
    Read = 0,
    Write = 1,
    Transfer = 2,
    TransferInPlace = 3,
    Flush = 4,
    Invalid = 254,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum Status {
    Success = 0,

    InvalidOpcode = 254,
    Other = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct Request<'a> {
    opcode: Opcode,
    size: u8,
    data: Option<&'a [u8]>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct Response<'a> {
    status: Status,
    size: Option<u8>,
    data: Option<&'a [u8]>,
}

pub struct Spi {
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
    pub fn blocking_read(&mut self, words: &mut [u8]) -> Result<()> {
        let size = words.len();

        let request = Request {
            opcode: Opcode::Read,
            size: size as u8,
            data: None,
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Read failed!");
        } else {
            let data = response.data.unwrap();
            words.copy_from_slice(data);
        }

        Ok(())
    }

    /// SPI blocking write
    pub fn blocking_write(&mut self, words: &[u8]) -> Result<()> {
        let size = words.len();

        let request = Request {
            opcode: Opcode::Write,
            size: size as u8,
            data: Some(words),
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Write failed!");
        }

        Ok(())
    }

    /// SPI blocking transfer
    pub fn blocking_transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<()> {
        let size = write.len();

        let request = Request {
            opcode: Opcode::Transfer,
            size: size as u8,
            data: Some(write),
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Read failed!");
        } else {
            let data = response.data.unwrap();
            read.copy_from_slice(data);
        }

        Ok(())
    }

    /// SPI blocking transfer in place
    pub fn blocking_transfer_in_place(&mut self, words: &mut [u8]) -> Result<()> {
        let mut buf = vec![0; words.len()];
        self.blocking_transfer(&mut buf, words).map_err(|_| Error::Io)?;
        words.copy_from_slice(&buf);
        Ok(())
    }

    /// SPI blocking flush
    pub fn blocking_flush(&mut self) -> Result<()> {
        let request = Request {
            opcode: Opcode::Flush,
            size: 0,
            data: None,
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Flush failed!");
        }

        Ok(())
    }
}

impl spi::Error for Error {
    fn kind(&self) -> spi::ErrorKind {
        match *self {
            _ => spi::ErrorKind::Other,
        }
    }
}

impl spi::ErrorType for Spi {
    type Error = Error;
}

impl spi::SpiBus for Spi {
    fn read(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        self.blocking_read(words)
    }

    fn write(&mut self, words: &[u8]) -> std::result::Result<(), Self::Error> {
        self.blocking_write(words)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> std::result::Result<(), Self::Error> {
        self.blocking_transfer(read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        self.blocking_transfer_in_place(words)
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        self.blocking_flush()
    }
}
