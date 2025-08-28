use embedded_hal::delay::DelayNs;
use embedded_hal::digital;
use embedded_hal::i2c::{self, SevenBitAddress};
use embedded_hal::spi;
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use nusb::{MaybeFuture, list_devices};
pub use pico_de_gallo_internal::*;
use postcard::{from_bytes, to_stdvec};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;
use std::thread;
use std::time::Duration;
use thiserror::Error;

const USB_BUFFER_SIZE: usize = 4096;

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

#[derive(Clone, Debug)]
pub enum I2cError {
    NoAcknowledge,
    ArbitrationLoss,
    Other,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct PicoDeGallo {
    pub usb: Rc<RefCell<UsbIo>>,
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
        let intf = device.claim_interface(0).wait().map_err(|e| Error::Nusb(e))?;
        let writer = intf.endpoint::<Bulk, Out>(0x01).map_err(|_| Error::Io)?.writer(4096);
        let reader = intf.endpoint::<Bulk, In>(0x81).map_err(|_| Error::Io)?.reader(4096);

        let usb = Rc::new(RefCell::new(UsbIo { writer, reader }));

        Ok(Self { usb })
    }

    /// Create a new GPIO
    pub fn gpio(&self, pin: usize) -> Gpio {
        Gpio {
            pin,
            gallo: self.clone(),
        }
    }
}

pub struct UsbIo {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl UsbIo {
    /// Set config parameters
    pub fn set_config(
        &mut self,
        i2c_frequency: u32,
        spi_frequency: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) -> Result<()> {
        let request = Request::SetConfig(SetConfigRequest {
            i2c_frequency,
            spi_frequency,
            spi_phase,
            spi_polarity,
        });

        let output: Vec<u8> = to_stdvec(&request).map_err(|_| Error::Unknown)?;

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

        match response {
            Response::SetConfig(set_config_response) => {
                if set_config_response.status != Status::Success {
                    tracing::error!("Read failed!");
                    Err(Error::Unknown)
                } else {
                    Ok(())
                }
            }
            _ => {
                tracing::error!("Invalid response");
                Err(Error::Unknown)
            }
        }
    }

    /// I2c blocking read
    pub fn i2c_blocking_read(&mut self, address: u8, buf: &mut [u8]) -> Result<()> {
        let size = buf.len();

        let request = Request::I2c(I2cRequest {
            opcode: I2cOpcode::Read,
            address: u16::from(address),
            size: size as u16,
            data: None,
        });

        let output: Vec<u8> = to_stdvec(&request).map_err(|_| Error::Unknown)?;

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

        match response {
            Response::I2c(i2c_response) => {
                if i2c_response.status != Status::Success || i2c_response.data.is_none() {
                    tracing::error!("Read failed!");
                    Err(Error::Unknown)
                } else {
                    let data = i2c_response.data.unwrap();
                    buf.copy_from_slice(data);

                    Ok(())
                }
            }
            _ => {
                tracing::error!("Invalid response");
                Err(Error::Unknown)
            }
        }
    }

    /// I2c blocking write
    pub fn i2c_blocking_write(&mut self, address: u8, buf: &[u8]) -> Result<()> {
        let size = buf.len();

        let request = Request::I2c(I2cRequest {
            opcode: I2cOpcode::Write,
            address: u16::from(address),
            size: size as u16,
            data: Some(buf),
        });

        let output: Vec<u8> = to_stdvec(&request).map_err(|_| Error::Unknown)?;

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

        match response {
            Response::I2c(i2c_response) => {
                if i2c_response.status != Status::Success {
                    tracing::error!("Write failed");
                    Err(Error::Unknown)
                } else {
                    Ok(())
                }
            }
            _ => {
                tracing::error!("Invalid response");
                Err(Error::Unknown)
            }
        }
    }

    /// SPI blocking transfer
    pub fn spi_blocking_transfer(&mut self, read: Option<&mut [u8]>, write: Option<&[u8]>) -> Result<()> {
        if read.is_some() && write.is_some() {
            let read = read.unwrap();

            let request = Request::Spi(SpiRequest {
                opcode: SpiOpcode::Transfer,
                size: Some(read.len() as u16),
                data: write,
            });

            let output: Vec<u8> = to_stdvec(&request).map_err(|_| Error::Unknown)?;

            self.writer.write_all(&output).map_err(|_| Error::Io)?;
            self.writer.flush().map_err(|_| Error::Io)?;

            let mut rx_buf = vec![0; USB_BUFFER_SIZE];
            let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

            let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

            match response {
                Response::Spi(spi_response) => {
                    if spi_response.status != Status::Success || spi_response.data.is_none() {
                        tracing::error!("Write failed");
                        Err(Error::Unknown)
                    } else {
                        read.copy_from_slice(spi_response.data.unwrap());
                        Ok(())
                    }
                }
                _ => {
                    tracing::error!("Invalid response");
                    Err(Error::Unknown)
                }
            }
        } else if read.is_none() && write.is_some() {
            let request = Request::Spi(SpiRequest {
                opcode: SpiOpcode::Transfer,
                size: None,
                data: write,
            });

            let output: Vec<u8> = to_stdvec(&request).map_err(|_| Error::Unknown)?;

            self.writer.write_all(&output).map_err(|_| Error::Io)?;
            self.writer.flush().map_err(|_| Error::Io)?;

            let mut rx_buf = vec![0; USB_BUFFER_SIZE];
            let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

            let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

            match response {
                Response::Spi(spi_response) => {
                    if spi_response.status != Status::Success {
                        tracing::error!("Write failed");
                        Err(Error::Unknown)
                    } else {
                        Ok(())
                    }
                }
                _ => {
                    tracing::error!("Invalid response");
                    Err(Error::Unknown)
                }
            }
        } else if read.is_some() && write.is_none() {
            let read = read.unwrap();

            let request = Request::Spi(SpiRequest {
                opcode: SpiOpcode::Transfer,
                size: Some(read.len() as u16),
                data: None,
            });

            let output: Vec<u8> = to_stdvec(&request).map_err(|_| Error::Unknown)?;

            self.writer.write_all(&output).map_err(|_| Error::Io)?;
            self.writer.flush().map_err(|_| Error::Io)?;

            let mut rx_buf = vec![0; USB_BUFFER_SIZE];
            let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

            let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

            match response {
                Response::Spi(spi_response) => {
                    if spi_response.status != Status::Success || spi_response.data.is_none() {
                        tracing::error!("Write failed");
                        Err(Error::Unknown)
                    } else {
                        read.copy_from_slice(spi_response.data.unwrap());
                        Ok(())
                    }
                }
                _ => {
                    tracing::error!("Invalid response");
                    Err(Error::Unknown)
                }
            }
        } else {
            Err(Error::Unknown)
        }
    }

    /// SPI blocking flush
    pub fn spi_blocking_flush(&mut self) -> Result<()> {
        let request = Request::Spi(SpiRequest {
            opcode: SpiOpcode::Flush,
            size: None,
            data: None,
        });

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        self.writer.write_all(&output).map_err(|_| Error::Io)?;
        self.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = self.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).map_err(|_| Error::Unknown)?;

        match response {
            Response::Spi(spi_response) => {
                if spi_response.status != Status::Success {
                    tracing::error!("Write failed");
                    Err(Error::Unknown)
                } else {
                    Ok(())
                }
            }
            _ => {
                tracing::error!("Invalid response");
                Err(Error::Unknown)
            }
        }
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

impl i2c::ErrorType for PicoDeGallo {
    type Error = Error;
}

impl i2c::I2c<SevenBitAddress> for PicoDeGallo {
    fn transaction(&mut self, address: SevenBitAddress, operations: &mut [i2c::Operation<'_>]) -> Result<()> {
        let address = address.into();
        let mut usb = self.usb.borrow_mut();

        for op in operations {
            match op {
                i2c::Operation::Read(read) => usb.i2c_blocking_read(address, read)?,
                i2c::Operation::Write(write) => usb.i2c_blocking_write(address, write)?,
            }
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

impl spi::ErrorType for PicoDeGallo {
    type Error = Error;
}

impl spi::SpiBus for PicoDeGallo {
    fn read(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        let mut usb = self.usb.borrow_mut();
        usb.spi_blocking_transfer(Some(words), None)
    }

    fn write(&mut self, words: &[u8]) -> std::result::Result<(), Self::Error> {
        let mut usb = self.usb.borrow_mut();
        usb.spi_blocking_transfer(None, Some(words))
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> std::result::Result<(), Self::Error> {
        let mut usb = self.usb.borrow_mut();
        usb.spi_blocking_transfer(Some(read), Some(write))
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        let mut usb = self.usb.borrow_mut();
        let mut read = vec![0; words.len()];
        usb.spi_blocking_transfer(Some(&mut read), Some(words))?;
        words.copy_from_slice(&read);
        Ok(())
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        let mut usb = self.usb.borrow_mut();
        usb.spi_blocking_flush()
    }
}

impl DelayNs for PicoDeGallo {
    fn delay_ns(&mut self, ns: u32) {
        thread::sleep(Duration::from_nanos(ns.into()));
    }
}

pub struct Gpio {
    pin: usize,
    gallo: PicoDeGallo,
}

impl Gpio {
    /// GPIO set state
    pub fn blocking_set_state(&mut self, state: GpioState) -> Result<()> {
        let request = Request::Gpio(GpioRequest {
            opcode: GpioOpcode::SetState,
            pin: Pin { index: self.pin as u8 },
            state: Some(state),
        });

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        let mut usb = self.gallo.usb.borrow_mut();

        usb.writer.write_all(&output).map_err(|_| Error::Io)?;
        usb.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = usb.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        match response {
            Response::Gpio(gpio_response) => {
                if gpio_response.status != Status::Success {
                    eprintln!("Write failed");
                    Err(Error::Unknown)
                } else {
                    Ok(())
                }
            }
            _ => {
                tracing::error!("Invalid response");
                Err(Error::Unknown)
            }
        }
    }

    /// GPIO get state
    pub fn blocking_get_state(&mut self) -> Result<GpioState> {
        let request = Request::Gpio(GpioRequest {
            opcode: GpioOpcode::GetState,
            pin: Pin { index: self.pin as u8 },
            state: None,
        });

        let output: Vec<u8> = to_stdvec(&request).unwrap();

        let mut usb = self.gallo.usb.borrow_mut();

        usb.writer.write_all(&output).map_err(|_| Error::Io)?;
        usb.writer.flush().map_err(|_| Error::Io)?;

        let mut rx_buf = vec![0; USB_BUFFER_SIZE];
        let size = usb.reader.read(&mut rx_buf).map_err(|_| Error::Io)?;

        let response: Response = from_bytes(&rx_buf[..size]).unwrap();

        match response {
            Response::Gpio(gpio_response) => {
                if gpio_response.status != Status::Success || gpio_response.state.is_none() {
                    eprintln!("Write failed");
                    Err(Error::Unknown)
                } else {
                    Ok(gpio_response.state.unwrap())
                }
            }
            _ => {
                tracing::error!("Invalid response");
                Err(Error::Unknown)
            }
        }
    }
}

impl digital::Error for Error {
    fn kind(&self) -> digital::ErrorKind {
        digital::ErrorKind::Other
    }
}

impl digital::ErrorType for Gpio {
    type Error = Error;
}

impl digital::OutputPin for Gpio {
    fn set_low(&mut self) -> std::result::Result<(), Self::Error> {
        self.blocking_set_state(GpioState::Low)
    }

    fn set_high(&mut self) -> std::result::Result<(), Self::Error> {
        self.blocking_set_state(GpioState::High)
    }
}

impl digital::InputPin for Gpio {
    fn is_high(&mut self) -> std::result::Result<bool, Self::Error> {
        let state = self.blocking_get_state()?;
        Ok(state == GpioState::High)
    }

    fn is_low(&mut self) -> std::result::Result<bool, Self::Error> {
        let state = self.blocking_get_state()?;
        Ok(state == GpioState::Low)
    }
}
