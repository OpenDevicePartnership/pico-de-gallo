use crate::{Opcode, Response};
use defmt::*;
use embassy_rp::i2c;
use embassy_rp::peripherals::{I2C1, USB};
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointIn, EndpointOut};

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
                    Ok(n) => {
                        trace!("Received {} bytes", n);

                        let opcode: Opcode = data[0].into();
                        let addr = data[1];
                        let size = u16::from_le_bytes(data[2..4].try_into().unwrap_or([0, 0]));

                        match opcode {
                            Opcode::Read => {
                                debug!("Read {} bytes from I2C address {:02x}", size, addr);

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
                                    Err(_) => {
                                        data[0] = Response::Fail.into();
                                        self.write_ep.write(&data[..4]).await.ok();
                                    }
                                }
                            }
                            Opcode::Write => {
                                debug!("Write {} bytes to I2C address {:02x}", size, addr);

                                let result = self
                                    .bus
                                    .blocking_write(addr, &data[4..(usize::from(size) + 4)]);

                                if result.is_ok() {
                                    data[0] = Response::Success.into();
                                } else {
                                    data[0] = Response::Fail.into();
                                }

                                self.write_ep.write(&data[..4]).await.ok();
                            }
                            Opcode::Invalid => {
                                trace!("Invalid opcode");
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
