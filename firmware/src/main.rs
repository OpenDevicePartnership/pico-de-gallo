#![no_std]
#![no_main]

use defmt::*;
use embassy_embedded_hal::SetConfig;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Flex, Level};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{I2C1, SPI0, USB};
use embassy_rp::spi::{self, Phase, Polarity, Spi};
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_usb::driver::{Endpoint as _, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::msos::{self, windows_version};
use embassy_usb::{Builder, Config};
use pico_de_gallo_internal::*;
use postcard::{from_bytes, to_slice};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// Program metadata for `picotool info`.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"Pico de Gallo"),
    embassy_rp::binary_info::rp_program_description!(c"USB bridge to various buses such as I2C, SPI, and UART"),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
});

// This is a randomly generated GUID to allow clients on Windows to find our device
const DEVICE_INTERFACE_GUIDS: &[&str] = &["{F41916C1-5335-4DB1-91F5-D023CB63AC67}"];

static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
static MSOS_DESCRIPTOR: StaticCell<[u8; 512]> = StaticCell::new();
static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

const USB_BUFFER_SIZE: usize = 4096;
const BUS_BUFFER_SIZE: usize = 3072;
const NUM_GPIOS: usize = 8;

struct PicoDeGallo<'a> {
    i2c: I2c<'a, I2C1, i2c::Async>,
    spi: Spi<'a, SPI0, spi::Async>,
    gpios: [Flex<'a>; NUM_GPIOS],
    read_ep: Endpoint<'a, USB, Out>,
    write_ep: Endpoint<'a, USB, In>,
}

impl<'a> PicoDeGallo<'a> {
    fn new(
        i2c: I2c<'a, I2C1, i2c::Async>,
        spi: Spi<'a, SPI0, spi::Async>,
        gpios: [Flex<'a>; NUM_GPIOS],
        read_ep: Endpoint<'a, USB, Out>,
        write_ep: Endpoint<'a, USB, In>,
    ) -> Self {
        Self {
            i2c,
            spi,
            gpios,
            read_ep,
            write_ep,
        }
    }

    async fn wait_for_connection(&mut self) {
        join(self.read_ep.wait_enabled(), self.write_ep.wait_enabled()).await;
    }

    async fn read_data(&mut self, data: &mut [u8]) -> Result<usize, EndpointError> {
        let mut n = 0;

        loop {
            let i = self.read_ep.read(&mut data[n..]).await?;
            n += i;
            if i < self.read_ep.info().max_packet_size as usize {
                return Ok(n);
            }
        }
    }

    async fn write_data(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        for chunk in data.chunks(self.write_ep.info().max_packet_size as usize) {
            self.write_ep.write(chunk).await?;
        }
        if data.len() % self.write_ep.info().max_packet_size as usize == 0 {
            self.write_ep.write(&[]).await?;
        }
        Ok(())
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let driver = embassy_rp::usb::Driver::new(p.USB, Irqs);

    // Obtain the flash ID
    let unique_id: u64 = embassy_rp::otp::get_chipid().unwrap();
    static SERIAL_STRING: StaticCell<[u8; 16]> = StaticCell::new();
    let mut ser_buf = [b' '; 16];
    // This is a simple number-to-hex formatting
    unique_id
        .to_be_bytes()
        .iter()
        .zip(ser_buf.chunks_exact_mut(2))
        .for_each(|(b, chs)| {
            let mut b = *b;
            for c in chs {
                *c = match b >> 4 {
                    v @ 0..10 => b'0' + v,
                    v @ 10..16 => b'A' + (v - 10),
                    _ => b'X',
                };
                b <<= 4;
            }
        });
    let ser_buf = SERIAL_STRING.init(ser_buf);
    let ser_buf = core::str::from_utf8(ser_buf.as_slice()).unwrap();

    // Create embassy-usb Config
    let mut config = Config::new(MICROSOFT_VID, PICO_DE_GALLO_PID);
    config.manufacturer = Some("Microsoft");
    config.product = Some("Pico de Gallo");
    config.serial_number = Some(ser_buf);
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    config.self_powered = false;
    config.composite_with_iads = false;
    config.device_class = 0xff;
    config.device_sub_class = 0xff;
    config.device_protocol = 0xff;

    // Create embassy-usb DeviceBuilder using the driver and config.
    //
    // It needs some buffers for building the descriptors.
    let config_descriptor = CONFIG_DESCRIPTOR.init([0; 256]);
    let bos_descriptor = BOS_DESCRIPTOR.init([0; 256]);
    let msos_descriptor = MSOS_DESCRIPTOR.init([0; 512]);
    let control_buf = CONTROL_BUF.init([0; 64]);

    let mut builder = Builder::new(
        driver,
        config,
        config_descriptor,
        bos_descriptor,
        msos_descriptor,
        control_buf,
    );

    builder.msos_descriptor(windows_version::WIN8_1, 0);
    builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
        "DeviceInterfaceGUIDs",
        msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    ));

    let mut function = builder.function(0xff, 0xff, 0xff);

    // I2C
    let mut interface = function.interface();
    let mut alt = interface.alt_setting(0xff, 0xff, 0xff, None);
    let read_ep = alt.endpoint_bulk_out(None, 64);
    let write_ep = alt.endpoint_bulk_in(None, 64);
    let i2c_bus = embassy_rp::i2c::I2c::new_async(p.I2C1, p.PIN_3, p.PIN_2, Irqs, embassy_rp::i2c::Config::default());
    let spi_bus = embassy_rp::spi::Spi::new(
        p.SPI0,
        p.PIN_6,
        p.PIN_7,
        p.PIN_4,
        p.DMA_CH0,
        p.DMA_CH1,
        embassy_rp::spi::Config::default(),
    );
    let gpio8 = embassy_rp::gpio::Flex::new(p.PIN_8);
    let gpio9 = embassy_rp::gpio::Flex::new(p.PIN_9);
    let gpio10 = embassy_rp::gpio::Flex::new(p.PIN_10);
    let gpio11 = embassy_rp::gpio::Flex::new(p.PIN_11);
    let gpio12 = embassy_rp::gpio::Flex::new(p.PIN_12);
    let gpio13 = embassy_rp::gpio::Flex::new(p.PIN_13);
    let gpio14 = embassy_rp::gpio::Flex::new(p.PIN_14);
    let gpio15 = embassy_rp::gpio::Flex::new(p.PIN_15);

    let gallo = PicoDeGallo::new(
        i2c_bus,
        spi_bus,
        [gpio8, gpio9, gpio10, gpio11, gpio12, gpio13, gpio14, gpio15],
        read_ep,
        write_ep,
    );

    drop(function);

    // Build the builder.
    let mut usb = builder.build();

    spawner.spawn(gallo_task(gallo).unwrap());

    loop {
        usb.run().await;
    }
}

#[embassy_executor::task]
async fn gallo_task(mut gallo: PicoDeGallo<'static>) {
    let mut rx_buf: [u8; USB_BUFFER_SIZE] = [0; USB_BUFFER_SIZE];
    let mut tx_buf: [u8; USB_BUFFER_SIZE] = [0; USB_BUFFER_SIZE];
    let mut bus_buf: [u8; BUS_BUFFER_SIZE] = [0; BUS_BUFFER_SIZE];

    loop {
        gallo.wait_for_connection().await;

        debug!("Pico De Gallo connected");

        loop {
            let result = gallo.read_data(&mut rx_buf).await;

            let response = if result.is_err() {
                Response::InvalidRequest
            } else {
                let rx_size = result.unwrap();

                let result = from_bytes(&rx_buf[..rx_size]);

                if result.is_err() {
                    error!("Failed to deserialize request");
                    break;
                } else {
                    let request = result.unwrap();

                    trace!("Received request: {}", request);

                    match request {
                        Request::I2c(i2c_request) => match i2c_request.opcode {
                            I2cOpcode::Read => {
                                let result = gallo.i2c.blocking_read(i2c_request.address, &mut bus_buf);

                                if result.is_err() {
                                    error!("Failed to read from I2C address {:02x}", i2c_request.address);
                                    Response::InvalidRequest
                                } else {
                                    Response::I2c(I2cResponse {
                                        status: Status::Success,
                                        address: Some(i2c_request.address),
                                        size: Some(i2c_request.size),
                                        data: Some(&bus_buf[..usize::from(i2c_request.size)]),
                                    })
                                }
                            }
                            I2cOpcode::Write => {
                                if i2c_request.data.is_none() {
                                    error!("Missing 'data' field");
                                    Response::InvalidRequest
                                } else {
                                    let result =
                                        gallo.i2c.blocking_write(i2c_request.address, i2c_request.data.unwrap());

                                    if result.is_err() {
                                        error!("Failed to write to I2C address {:02x}", i2c_request.address);
                                        Response::InvalidRequest
                                    } else {
                                        Response::I2c(I2cResponse {
                                            status: Status::Success,
                                            address: Some(i2c_request.address),
                                            size: None,
                                            data: None,
                                        })
                                    }
                                }
                            }
                        },

                        Request::Spi(spi_request) => match spi_request.opcode {
                            SpiOpcode::Transfer => {
                                if spi_request.data.is_none() && spi_request.size.is_some() {
                                    let size = usize::from(spi_request.size.unwrap());
                                    let result = gallo.spi.blocking_read(&mut bus_buf[..size]);

                                    if result.is_err() {
                                        Response::InvalidRequest
                                    } else {
                                        Response::Spi(SpiResponse {
                                            status: Status::Success,
                                            size: spi_request.size,
                                            data: Some(&bus_buf[..size]),
                                        })
                                    }
                                } else if spi_request.data.is_some() && spi_request.size.is_none() {
                                    let data = spi_request.data.unwrap();
                                    let result = gallo.spi.blocking_write(&data);

                                    if result.is_err() {
                                        Response::InvalidRequest
                                    } else {
                                        Response::Spi(SpiResponse {
                                            status: Status::Success,
                                            size: None,
                                            data: None,
                                        })
                                    }
                                } else if spi_request.data.is_some() && spi_request.size.is_some() {
                                    let size = usize::from(spi_request.size.unwrap());
                                    let data = spi_request.data.unwrap();

                                    let result = gallo.spi.blocking_transfer(&mut bus_buf[..size], data);

                                    if result.is_err() {
                                        Response::InvalidRequest
                                    } else {
                                        Response::Spi(SpiResponse {
                                            status: Status::Success,
                                            size: spi_request.size,
                                            data: Some(&bus_buf[..size]),
                                        })
                                    }
                                } else {
                                    Response::InvalidRequest
                                }
                            }
                            SpiOpcode::Flush => {
                                let result = gallo.spi.flush();

                                if result.is_err() {
                                    Response::InvalidRequest
                                } else {
                                    Response::Spi(SpiResponse {
                                        status: Status::Success,
                                        size: None,
                                        data: None,
                                    })
                                }
                            }
                        },

                        Request::Gpio(gpio_request) => match gpio_request.opcode {
                            GpioOpcode::GetState => {
                                let pin = gpio_request.pin;
                                let gpio = &mut gallo.gpios[usize::from(pin.index)];

                                gpio.set_as_input();
                                let level = gpio.get_level();
                                let state = if level == Level::High {
                                    GpioState::High
                                } else {
                                    GpioState::Low
                                };

                                Response::Gpio(GpioResponse {
                                    status: Status::Success,
                                    pin,
                                    state: Some(state),
                                })
                            }
                            GpioOpcode::SetState => {
                                if gpio_request.state.is_none() {
                                    error!("Missing 'state' field");
                                    Response::InvalidRequest
                                } else {
                                    let pin = gpio_request.pin;
                                    let gpio = &mut gallo.gpios[usize::from(pin.index)];
                                    let state = gpio_request.state.unwrap();

                                    gpio.set_as_output();
                                    gpio.set_level(if state == GpioState::Low {
                                        Level::Low
                                    } else {
                                        Level::High
                                    });

                                    Response::Gpio(GpioResponse {
                                        status: Status::Success,
                                        pin,
                                        state: None,
                                    })
                                }
                            }
                        },
                        Request::SetConfig(set_config_request) => {
                            let mut i2c_config = i2c::Config::default();
                            i2c_config.frequency = set_config_request.i2c_frequency;

                            let mut spi_config = spi::Config::default();
                            spi_config.frequency = set_config_request.spi_frequency;
                            spi_config.phase = if set_config_request.spi_phase == SpiPhase::CaptureOnFirstTransition {
                                Phase::CaptureOnFirstTransition
                            } else {
                                Phase::CaptureOnSecondTransition
                            };
                            spi_config.polarity = if set_config_request.spi_polarity == SpiPolarity::IdleLow {
                                Polarity::IdleLow
                            } else {
                                Polarity::IdleHigh
                            };

                            let result = gallo.i2c.set_config(&i2c_config);
                            gallo.spi.set_config(&spi_config);

                            if result.is_err() {
                                Response::InvalidRequest
                            } else {
                                Response::SetConfig(SetConfigResponse {
                                    status: Status::Success,
                                })
                            }
                        }
                    }
                }
            };
            trace!("Prepared response: {}", response);

            let ser = to_slice(&response, &mut tx_buf);

            if result.is_err() {
                error!("Failed to serialize response");
                break;
            } else {
                let result = gallo.write_data(ser.unwrap()).await;

                if result.is_err() {
                    error!("Failed to send response");
                    break;
                }
            }
        }

        debug!("Pico De Gallo disconnected");
    }
}
