use crate::{Error, Result};
use embedded_hal::i2c::{self, SevenBitAddress};
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
    Invalid = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum Status {
    Success = 0,
    NoAcknowledge = 1,
    ArbitrationLoss = 2,

    InvalidOpcode = 254,
    Other = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Request<'a> {
    pub opcode: Opcode,
    pub address: u8,
    pub size: u8,
    pub data: Option<&'a [u8]>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Response<'a> {
    pub status: Status,
    pub address: Option<u8>,
    pub size: Option<u8>,
    pub data: Option<&'a [u8]>,
}

#[derive(Clone, Debug)]
pub enum I2cError {
    NoAcknowledge,
    ArbitrationLoss,
    Other,
}

pub struct I2c {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl I2c {
    pub(crate) fn new(interface: Interface) -> Result<Self> {
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
    pub fn blocking_read(&mut self, address: u8, buf: &mut [u8]) -> Result<()> {
        let size = buf.len();

        let request = Request {
            opcode: Opcode::Read,
            address,
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
            buf.copy_from_slice(data);
        }

        Ok(())
    }

    /// I2c blocking write
    pub fn blocking_write(&mut self, address: u8, buf: &[u8]) -> Result<()> {
        let size = buf.len();

        let request = Request {
            opcode: Opcode::Write,
            address,
            size: size as u8,
            data: Some(buf),
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Write failed");
        }

        Ok(())
    }
}

impl i2c::Error for Error {
    fn kind(&self) -> i2c::ErrorKind {
        match *self {
            Error::I2c(I2cError::NoAcknowledge) => i2c::ErrorKind::NoAcknowledge(i2c::NoAcknowledgeSource::Address),
            Error::I2c(I2cError::ArbitrationLoss) => i2c::ErrorKind::ArbitrationLoss,
            _ => i2c::ErrorKind::Other,
        }
    }
}

impl i2c::ErrorType for I2c {
    type Error = Error;
}

impl i2c::I2c<SevenBitAddress> for I2c {
    fn transaction(&mut self, address: SevenBitAddress, operations: &mut [i2c::Operation<'_>]) -> Result<()> {
        let address = address.into();

        for op in operations {
            match op {
                i2c::Operation::Read(read) => self.blocking_read(address, read)?,
                i2c::Operation::Write(write) => self.blocking_write(address, write)?,
            }
        }

        Ok(())
    }
}
