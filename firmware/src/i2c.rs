use defmt::*;
use embassy_rp::i2c;
use embassy_rp::peripherals::{I2C1, USB};
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointIn, EndpointOut};
use postcard::{from_bytes, to_slice};
use serde::{Deserialize, Serialize};

const USB_BUFFER_SIZE: usize = 1024;
const I2C_BUFFER_SIZE: usize = 512;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
pub enum Opcode {
    Read = 0,
    Write = 1,
    Invalid = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
enum Status {
    Success = 0,
    NoAcknowledge = 1,
    ArbitrationLoss = 2,

    InvalidOpcode = 254,
    Other = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
struct Request<'a> {
    opcode: Opcode,
    address: u8,
    size: u8,
    data: Option<&'a [u8]>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
struct Response<'a> {
    status: Status,
    address: Option<u8>,
    size: Option<u8>,
    data: Option<&'a [u8]>,
}

pub struct I2c<'d> {
    bus: i2c::I2c<'d, I2C1, i2c::Async>,
    read_ep: Endpoint<'d, USB, Out>,
    write_ep: Endpoint<'d, USB, In>,
    rx_buf: [u8; USB_BUFFER_SIZE],
    tx_buf: [u8; USB_BUFFER_SIZE],
    i2c_buf: [u8; I2C_BUFFER_SIZE],
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
            rx_buf: [0; USB_BUFFER_SIZE],
            tx_buf: [0; USB_BUFFER_SIZE],
            i2c_buf: [0; I2C_BUFFER_SIZE],
        }
    }

    async fn run(&mut self) {
        loop {
            self.read_ep.wait_enabled().await;

            debug!("I2C Connected");

            loop {
                match self.read_ep.read(&mut self.rx_buf).await {
                    Ok(_) => {
                        let result = from_bytes(&self.rx_buf);

                        let mut response = Response {
                            status: Status::Other,
                            address: None,
                            size: None,
                            data: None,
                        };

                        if result.is_ok() {
                            let request: Request = result.unwrap();
                            debug!("{:?}", &request);

                            match request.opcode {
                                Opcode::Read => {
                                    let result = self.bus.blocking_read(request.address, &mut self.i2c_buf);

                                    if result.is_ok() {
                                        response.status = Status::Success;
                                        response.address = Some(request.address);
                                        response.size = Some(request.size);
                                        response.data = Some(&self.i2c_buf[..usize::from(request.size)]);
                                    }
                                }
                                Opcode::Write => {
                                    if request.data.is_some() {
                                        let result = self.bus.blocking_write(request.address, request.data.unwrap());
                                        if result.is_ok() {
                                            response.status = Status::Success;
                                            response.address = Some(request.address);
                                            response.size = None;
                                            response.data = None;
                                        }
                                    }
                                }
                                Opcode::Invalid => {
                                    response.status = Status::InvalidOpcode;
                                }
                            }
                        }

                        debug!("{:?}", response);
                        let output = to_slice(&response, &mut self.tx_buf).unwrap();
                        self.write_ep.write(&output).await.ok();
                    }
                    Err(e) => {
                        error!("Unable to receive request: '{}'", e);
                        break;
                    }
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
