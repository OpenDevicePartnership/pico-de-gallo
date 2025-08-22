#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::{I2C1, USB};
use embassy_usb::msos::{self, windows_version};
use embassy_usb::types::StringIndex;
use embassy_usb::{Builder, Config, Handler};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

mod i2c;
mod spi;

use i2c::{I2c, i2c_task};
use spi::{Spi, spi_task};

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

static STRING_HANDLER: StaticCell<StringHandler> = StaticCell::new();
static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
static MSOS_DESCRIPTOR: StaticCell<[u8; 512]> = StaticCell::new();
static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

#[repr(u8)]
pub enum Opcode {
    Read = 0,
    Write = 1,
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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let driver = embassy_rp::usb::Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let mut config = Config::new(0x045e, 0x7069);
    config.manufacturer = Some("Microsoft");
    config.product = Some("Pico de Gallo");
    config.serial_number = Some("123456789");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

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

    let i2c_str = builder.string();
    let spi_str = builder.string();
    let handler = STRING_HANDLER.init(StringHandler::new(i2c_str, spi_str));

    // Add a vendor-specific function (class 0xff), and corresponding interface,
    // that uses our custom handler.
    let mut function = builder.function(0xff, 0xff, 0xff);

    function.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    function.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
        "DeviceInterfaceGUIDs",
        msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    ));

    // I2C
    let mut interface = function.interface();
    let mut alt = interface.alt_setting(0xff, 0xff, 0xff, Some(i2c_str));
    let read_ep = alt.endpoint_bulk_out(None, 64);
    let write_ep = alt.endpoint_bulk_in(None, 64);
    let i2c_bus = embassy_rp::i2c::I2c::new_async(p.I2C1, p.PIN_3, p.PIN_2, Irqs, embassy_rp::i2c::Config::default());
    let i2c = I2c::new(i2c_bus, read_ep, write_ep);

    drop(function);

    // Add a vendor-specific function (class 0xff), and corresponding interface,
    // that uses our custom handler.
    let mut function = builder.function(0xff, 0xff, 0xff);

    function.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    function.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
        "DeviceInterfaceGUIDs",
        msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    ));

    // SPI
    let mut interface = function.interface();
    let mut alt = interface.alt_setting(0xff, 0xff, 0xff, Some(spi_str));
    let read_ep = alt.endpoint_bulk_out(None, 64);
    let write_ep = alt.endpoint_bulk_in(None, 64);
    let spi_bus = embassy_rp::spi::Spi::new(
        p.SPI0,
        p.PIN_6,
        p.PIN_7,
        p.PIN_4,
        p.DMA_CH0,
        p.DMA_CH1,
        embassy_rp::spi::Config::default(),
    );
    let spi = Spi::new(spi_bus, read_ep, write_ep);

    drop(function);

    builder.handler(handler);

    // Build the builder.
    let mut usb = builder.build();

    spawner.must_spawn(i2c_task(i2c));
    spawner.must_spawn(spi_task(spi));

    loop {
        usb.run().await;
    }
}

struct StringHandler {
    i2c_str: StringIndex,
    spi_str: StringIndex,
}

impl StringHandler {
    fn new(i2c_str: StringIndex, spi_str: StringIndex) -> Self {
        Self { i2c_str, spi_str }
    }
}

impl Handler for StringHandler {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        if index == self.i2c_str {
            Some("Pico de Gallo I2C Interface")
        } else if index == self.spi_str {
            Some("Pico de Gallo SPI Interface")
        } else {
            warn!("Unknown string index requested");
            None
        }
    }
}
