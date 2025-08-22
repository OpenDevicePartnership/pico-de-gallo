use defmt::*;
use embassy_rp::peripherals::{SPI0, USB};
use embassy_rp::spi;
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointIn, EndpointOut};

#[repr(u8)]
pub enum Opcode {
    Read = 0,
    Write = 1,
    Transfer = 2,
    TransferInPlace = 3,
    Flush = 4,
    Invalid = 254,
}

impl From<Opcode> for u8 {
    fn from(value: Opcode) -> Self {
        value as _
    }
}

impl From<u8> for Opcode {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Read,
            1 => Self::Write,
            _ => Self::Invalid,
        }
    }
}

#[repr(u8)]
pub enum Response {
    Success = 0,
    InvalidOpcode = 254,
    Fail = 255,
}

impl From<Response> for u8 {
    fn from(value: Response) -> Self {
        value as _
    }
}

pub struct Spi<'d> {
    bus: spi::Spi<'d, SPI0, spi::Async>,
    read_ep: Endpoint<'d, USB, Out>,
    write_ep: Endpoint<'d, USB, In>,
}

impl<'d> Spi<'d> {
    pub fn new(
        bus: spi::Spi<'d, SPI0, spi::Async>,
        read_ep: Endpoint<'d, USB, Out>,
        write_ep: Endpoint<'d, USB, In>,
    ) -> Self {
        Self { bus, read_ep, write_ep }
    }

    async fn run(&mut self) {
        loop {
            self.read_ep.wait_enabled().await;

            debug!("SPI Connected");
            loop {
                let mut data = [0; 512];

                match self.read_ep.read(&mut data[..509]).await {
                    Ok(_n) => {
                        let opcode: Opcode = data[0].into();
                        let size = u16::from_le_bytes(data[1..3].try_into().unwrap_or([0, 0]));

                        match opcode {
                            Opcode::Read => {
                                debug!("Read {} bytes", size);

                                let result = self.bus.blocking_read(&mut data[4..(usize::from(size) + 4)]);

                                match result {
                                    Ok(()) => {
                                        data[0] = Response::Success.into();
                                        self.write_ep.write(&data[..(usize::from(size) + 3)]).await.ok();
                                    }
                                    Err(_) => {
                                        data[0] = Response::Fail.into();
                                        self.write_ep.write(&data[..3]).await.ok();
                                    }
                                }
                            }
                            Opcode::Write => {
                                debug!("Write {} bytes", size);

                                let result = self.bus.blocking_write(&data[3..(usize::from(size) + 3)]);

                                if result.is_ok() {
                                    data[0] = Response::Success.into();
                                } else {
                                    data[0] = Response::Fail.into();
                                }

                                self.write_ep.write(&data[..3]).await.ok();
                            }
                            Opcode::Transfer => {
                                debug!("Transfer {} bytes", size);

                                let mut read = [0; 512];

                                read[1] = data[1];
                                read[2] = data[2];

                                // TODO: make sure transfer size is less than 508.

                                let result = self.bus.blocking_transfer(&mut read, &data[3..(usize::from(size) + 3)]);
                                match result {
                                    Ok(()) => {
                                        read[0] = Response::Success.into();
                                        self.write_ep.write(&read[..(usize::from(size) + 3)]).await.ok();
                                    }
                                    Err(_) => {
                                        read[0] = Response::Fail.into();
                                        self.write_ep.write(&read[..3]).await.ok();
                                    }
                                }
                            }
                            Opcode::TransferInPlace => {
                                debug!("Transfer in place {} bytes", size);

                                let result = self
                                    .bus
                                    .blocking_transfer_in_place(&mut data[3..(usize::from(size) + 3)]);
                                match result {
                                    Ok(()) => {
                                        data[0] = Response::Success.into();
                                        self.write_ep.write(&data[..(usize::from(size) + 3)]).await.ok();
                                    }
                                    Err(_) => {
                                        data[0] = Response::Fail.into();
                                        self.write_ep.write(&data[..3]).await.ok();
                                    }
                                }
                            }
                            Opcode::Flush => {
                                debug!("Flush");

                                let result = self.bus.flush();

                                match result {
                                    Ok(()) => {
                                        self.write_ep.write(&[Response::Success.into()]).await.ok();
                                    }
                                    Err(_) => {
                                        self.write_ep.write(&[Response::Fail.into()]).await.ok();
                                    }
                                }
                            }
                            Opcode::Invalid => {
                                trace!("Invalid opcode");
                                data[0] = Response::InvalidOpcode.into();
                                self.write_ep.write(&data[..3]).await.ok();
                            }
                        }
                    }
                    Err(_) => break,
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
