use defmt::*;
use embassy_rp::gpio::{Flex, Level};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointIn, EndpointOut};
use postcard::{from_bytes, to_slice};
use serde::{Deserialize, Serialize};

const NUM_GPIOS: usize = 8;
const USB_BUFFER_SIZE: usize = 16;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
pub enum State {
    Low = 0,
    High = 1,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
pub enum Opcode {
    GetState = 0,
    SetState = 1,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
enum Status {
    Success = 0,
    Other = 255,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
struct Request {
    opcode: Opcode,
    pin: u8,
    data: Option<State>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, defmt::Format)]
struct Response {
    status: Status,
    pin: u8,
    data: Option<State>,
}

pub struct Gpio<'d> {
    gpios: [Flex<'d>; NUM_GPIOS],
    read_ep: Endpoint<'d, USB, Out>,
    write_ep: Endpoint<'d, USB, In>,
    rx_buf: [u8; USB_BUFFER_SIZE],
    tx_buf: [u8; USB_BUFFER_SIZE],
}

impl<'d> Gpio<'d> {
    pub fn new(gpios: [Flex<'d>; NUM_GPIOS], read_ep: Endpoint<'d, USB, Out>, write_ep: Endpoint<'d, USB, In>) -> Self {
        Self {
            gpios,
            read_ep,
            write_ep,
            rx_buf: [0; USB_BUFFER_SIZE],
            tx_buf: [0; USB_BUFFER_SIZE],
        }
    }

    async fn run(&mut self) {
        loop {
            self.read_ep.wait_enabled().await;

            debug!("GPIO Connected");

            loop {
                match self.read_ep.read(&mut self.rx_buf).await {
                    Ok(_) => {
                        let result = from_bytes(&self.rx_buf);

                        let mut response = Response {
                            status: Status::Other,
                            pin: 0,
                            data: None,
                        };

                        if result.is_ok() {
                            let request: Request = result.unwrap();
                            debug!("{:?}", &request);

                            match request.opcode {
                                Opcode::GetState => {
                                    let level = self.gpios[usize::from(request.pin)].get_level();

                                    response.status = Status::Success;
                                    response.pin = request.pin;
                                    response.data = Some(if level == Level::High { State::High } else { State::Low });
                                }

                                Opcode::SetState => {
                                    let state = request.data.unwrap();
                                    self.gpios[usize::from(request.pin)].set_level(if state == State::Low {
                                        Level::Low
                                    } else {
                                        Level::High
                                    });

                                    response.status = Status::Success;
                                    response.pin = request.pin;
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
        }
    }
}

#[embassy_executor::task]
pub async fn gpio_task(mut gpio: Gpio<'static>) {
    loop {
        gpio.run().await;
    }
}
