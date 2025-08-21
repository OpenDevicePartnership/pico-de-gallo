use crate::{Error, PicoDeGallo, Result};
use embedded_hal::i2c::{self, SevenBitAddress};
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use nusb::Interface;
use std::io::{Read, Write};

pub(crate) struct I2c {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

#[repr(u8)]
pub enum Response {
    Success = 0,
    NoAcknowledge = 1,
    ArbitrationLoss = 2,
    TxNotEmpty = 3,
    InvalidReadBufferLength = 4,
    InvalidWriteBufferLength = 5,
    AddressOutOfRange = 6,

    InvalidOpcode = 254,
    Other = 255,
}

#[derive(Clone, Debug)]
pub enum I2cError {
    NoAcknowledge,
    ArbitrationLoss,
    Other,
}

impl Response {
    fn into_result(self) -> Result<()> {
        match self {
            Response::Success => Ok(()),
            Response::NoAcknowledge => Err(Error::I2c(I2cError::NoAcknowledge)),
            Response::ArbitrationLoss => Err(Error::I2c(I2cError::ArbitrationLoss)),
            _ => Err(Error::I2c(I2cError::Other)),
        }
    }
}

impl From<u8> for Response {
    fn from(value: u8) -> Self {
        match value {
            0 => Response::Success,
            1 => Response::NoAcknowledge,
            2 => Response::ArbitrationLoss,
            3 => Response::TxNotEmpty,
            4 => Response::InvalidReadBufferLength,
            5 => Response::InvalidWriteBufferLength,
            6 => Response::AddressOutOfRange,
            7 => Response::InvalidOpcode,
            _ => Response::Other,
        }
    }
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
    pub(crate) fn blocking_read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()> {
        let size = buf.len().to_le_bytes();

        let cmd = [0x00, addr, size[0], size[1]];

        self.writer.write_all(&cmd).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut response = [0; 512];

        let size = self
            .reader
            .read(&mut response[..(4 + buf.len())])
            .map_err(|_| Error::Io)?;

        let response_code: Response = response[0].into();
        response_code.into_result()?;

        if size > 4 {
            buf.copy_from_slice(&response[4..size]);
        }

        Ok(())
    }

    /// I2c blocking write
    pub(crate) fn blocking_write(&mut self, addr: u8, buf: &[u8]) -> Result<()> {
        let size = buf.len().to_le_bytes();

        let mut cmd = vec![0x01, addr, size[0], size[1]];
        cmd.extend_from_slice(buf);

        self.writer.write_all(&cmd).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut response = [0; 512];

        let response_code: Response = response[0].into();
        response_code.into_result()?;

        self.reader.read(&mut response).map_err(|_| Error::Io)?;

        Ok(())
    }
}

impl i2c::Error for Error {
    fn kind(&self) -> i2c::ErrorKind {
        match *self {
            Error::I2c(I2cError::NoAcknowledge) => {
                i2c::ErrorKind::NoAcknowledge(i2c::NoAcknowledgeSource::Address)
            }
            Error::I2c(I2cError::ArbitrationLoss) => i2c::ErrorKind::ArbitrationLoss,
            _ => i2c::ErrorKind::Other,
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
