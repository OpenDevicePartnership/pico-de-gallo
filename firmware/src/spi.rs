use defmt::*;
use embassy_rp::peripherals::{SPI0, USB};
use embassy_rp::spi;
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointIn, EndpointOut};
use postcard::{from_bytes, to_slice};
use serde::{Deserialize, Serialize};

const USB_BUFFER_SIZE: usize = 1024;
const SPI_BUFFER_SIZE: usize = 512;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
pub enum Opcode {
    Read = 0,
    Write = 1,
    Transfer = 2,
    TransferInPlace = 3,
    Flush = 4,
    Invalid = 254,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
pub enum Status {
    Success = 0,

    InvalidOpcode = 254,
    Other = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
struct Request<'a> {
    opcode: Opcode,
    size: u8,
    data: Option<&'a [u8]>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
struct Response<'a> {
    status: Status,
    size: Option<u8>,
    data: Option<&'a [u8]>,
}

pub struct Spi<'d> {
    bus: spi::Spi<'d, SPI0, spi::Async>,
    read_ep: Endpoint<'d, USB, Out>,
    write_ep: Endpoint<'d, USB, In>,
    rx_buf: [u8; USB_BUFFER_SIZE],
    tx_buf: [u8; USB_BUFFER_SIZE],
    spi_buf: [u8; SPI_BUFFER_SIZE],
}

impl<'d> Spi<'d> {
    pub fn new(
        bus: spi::Spi<'d, SPI0, spi::Async>,
        read_ep: Endpoint<'d, USB, Out>,
        write_ep: Endpoint<'d, USB, In>,
    ) -> Self {
        Self {
            bus,
            read_ep,
            write_ep,
            rx_buf: [0; USB_BUFFER_SIZE],
            tx_buf: [0; USB_BUFFER_SIZE],
            spi_buf: [0; SPI_BUFFER_SIZE],
        }
    }

    async fn run(&mut self) {
        loop {
            self.read_ep.wait_enabled().await;

            debug!("SPI Connected");

            loop {
                match self.read_ep.read(&mut self.rx_buf).await {
                    Ok(_) => {
                        let result = from_bytes(&self.rx_buf);

                        let mut response = Response {
                            status: Status::Other,
                            size: None,
                            data: None,
                        };

                        if result.is_ok() {
                            let request: Request = result.unwrap();
                            debug!("{:?}", request);

                            match request.opcode {
                                Opcode::Read => {
                                    let result = self.bus.blocking_read(&mut self.spi_buf);

                                    if result.is_ok() {
                                        response.status = Status::Success;
                                        response.size = Some(request.size);
                                        response.data = Some(&self.spi_buf[..usize::from(request.size)]);
                                    }
                                }
                                Opcode::Write => {
                                    if request.data.is_some() {
                                        let result = self.bus.blocking_write(request.data.unwrap());

                                        if result.is_ok() {
                                            response.status = Status::Success;
                                        }
                                    }
                                }
                                Opcode::Transfer => {
                                    if request.data.is_some() {
                                        let result =
                                            self.bus.blocking_transfer(&mut self.spi_buf, request.data.unwrap());

                                        if result.is_ok() {
                                            response.status = Status::Success;
                                            response.size = Some(request.size);
                                            response.data = Some(&self.spi_buf);
                                        }
                                    }
                                }
                                Opcode::TransferInPlace => {
                                    if request.data.is_some() {
                                        self.spi_buf.copy_from_slice(request.data.unwrap());
                                        let result = self.bus.blocking_transfer_in_place(&mut self.spi_buf);

                                        if result.is_ok() {
                                            response.status = Status::Success;
                                            response.size = Some(request.size);
                                            response.data = Some(&self.spi_buf);
                                        }
                                    }
                                }
                                Opcode::Flush => {
                                    let result = self.bus.flush();

                                    if result.is_ok() {
                                        response.status = Status::Success;
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
            debug!("SPI Disconnected");
        }
    }
}

#[embassy_executor::task]
pub async fn spi_task(mut spi: Spi<'static>) {
    loop {
        spi.run().await;
    }
}
