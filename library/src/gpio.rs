use crate::{Error, Result};
use embedded_hal::digital;
use nusb::Interface;
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use postcard::{from_bytes, to_stdvec};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const USB_BUFFER_SIZE: usize = 16;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum State {
    Low = 0,
    High = 1,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum Opcode {
    GetState = 0,
    SetState = 1,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
enum Status {
    Success = 0,
    Other = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct Request {
    opcode: Opcode,
    pin: u8,
    data: Option<State>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
struct Response {
    status: Status,
    pin: u8,
    data: Option<State>,
}

pub struct Gpio {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl Gpio {
    pub(crate) fn new(interface: Interface) -> Result<Self> {
        let writer = interface
            .endpoint::<Bulk, Out>(0x03)
            .map_err(|_| Error::Io)?
            .writer(4096);
        let reader = interface
            .endpoint::<Bulk, In>(0x83)
            .map_err(|_| Error::Io)?
            .reader(4096);

        Ok(Self { writer, reader })
    }

    /// GPIO set state
    pub fn blocking_set_state(&mut self, pin: u8, state: State) -> Result<()> {
        let request = Request {
            opcode: Opcode::SetState,
            pin,
            data: Some(state),
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Set State failed!");
        }

        Ok(())
    }

    /// GPIO get state
    pub fn blocking_get_state(&mut self, pin: u8) -> Result<State> {
        let request = Request {
            opcode: Opcode::GetState,
            pin,
            data: None,
        };

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        if response.status != Status::Success {
            eprintln!("Write failed!");
            Err(Error::Gpio)
        } else {
            Ok(response.data.unwrap())
        }
    }
}

impl digital::Error for Error {
    fn kind(&self) -> digital::ErrorKind {
        match *self {
            _ => digital::ErrorKind::Other,
        }
    }
}

impl digital::ErrorType for Gpio {
    type Error = Error;
}

impl digital::OutputPin for Gpio {
    fn set_low(&mut self) -> std::result::Result<(), Self::Error> {
        self.blocking_set_state(0, State::Low)
    }

    fn set_high(&mut self) -> std::result::Result<(), Self::Error> {
        self.blocking_set_state(0, State::High)
    }
}

impl digital::InputPin for Gpio {
    fn is_high(&mut self) -> std::result::Result<bool, Self::Error> {
        let state = self.blocking_get_state(0)?;
        Ok(state == State::High)
    }

    fn is_low(&mut self) -> std::result::Result<bool, Self::Error> {
        let state = self.blocking_get_state(0)?;
        Ok(state == State::Low)
    }
}
