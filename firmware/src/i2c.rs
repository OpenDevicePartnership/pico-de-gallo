use crate::Opcode;
use defmt::*;
use embassy_rp::i2c;
use embassy_rp::peripherals::{I2C1, USB};
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointIn, EndpointOut};

#[repr(u8)]
enum Response {
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

impl From<i2c::Error> for Response {
    fn from(value: i2c::Error) -> Self {
        match value {
            i2c::Error::Abort(i2c::AbortReason::NoAcknowledge) => Self::NoAcknowledge,
            i2c::Error::Abort(i2c::AbortReason::ArbitrationLoss) => Self::ArbitrationLoss,
            i2c::Error::Abort(i2c::AbortReason::TxNotEmpty(_)) => Self::TxNotEmpty,
            i2c::Error::InvalidReadBufferLength => Self::InvalidReadBufferLength,
            i2c::Error::InvalidWriteBufferLength => Self::InvalidWriteBufferLength,
            i2c::Error::AddressOutOfRange(_) => Self::AddressOutOfRange,
            _ => Self::Other,
        }
    }
}

impl From<Response> for u8 {
    fn from(value: Response) -> Self {
        value as _
    }
}

pub struct I2c<'d> {
    bus: i2c::I2c<'d, I2C1, i2c::Async>,
    read_ep: Endpoint<'d, USB, Out>,
    write_ep: Endpoint<'d, USB, In>,
}

impl<'d> I2c<'d> {
    pub fn new(
        bus: i2c::I2c<'d, I2C1, i2c::Async>,
        read_ep: Endpoint<'d, USB, Out>,
        write_ep: Endpoint<'d, USB, In>,
    ) -> Self {
        Self {
            bus,
            read_ep,
            write_ep,
        }
    }

    async fn run(&mut self) {
        loop {
            self.read_ep.wait_enabled().await;

            debug!("I2C Connected");
            loop {
                let mut data = [0; 512];

                match self.read_ep.read(&mut data[..508]).await {
                    Ok(_) => {
                        let opcode: Opcode = data[0].into();
                        let addr = data[1];
                        let size = u16::from_le_bytes(data[2..4].try_into().unwrap_or([0, 0]));

                        match opcode {
                            Opcode::Read => {
                                let result = self
                                    .bus
                                    .blocking_read(addr, &mut data[4..(usize::from(size) + 4)]);

                                match result {
                                    Ok(()) => {
                                        data[0] = Response::Success.into();
                                        self.write_ep
                                            .write(&data[..(usize::from(size) + 4)])
                                            .await
                                            .ok();
                                    }
                                    Err(e) => {
                                        data[0] = Response::from(e).into();
                                        self.write_ep.write(&data[..4]).await.ok();
                                    }
                                }
                            }
                            Opcode::Write => {
                                let result = self
                                    .bus
                                    .blocking_write(addr, &data[4..(usize::from(size) + 4)]);

                                let response = match result {
                                    Ok(()) => Response::Success.into(),
                                    Err(e) => Response::from(e).into(),
                                };

                                data[0] = response;
                                self.write_ep.write(&data[..4]).await.ok();
                            }
                            Opcode::Invalid => {
                                data[0] = Response::InvalidOpcode.into();
                                self.write_ep.write(&data[..4]).await.ok();
                            }
                        }
                    }
                    Err(_) => break,
                }
            }

            debug!("I2C Disconnected");
        }
    }
}

#[embassy_executor::task]
pub async fn i2c_task(mut i2c: I2c<'static>) {
    loop {
        i2c.run().await;
    }
}
