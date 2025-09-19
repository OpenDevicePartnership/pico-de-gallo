#![no_std]
#![no_main]

use defmt::info;
use embassy_embedded_hal::SetConfig;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::ClockConfig;
use embassy_rp::gpio::{Flex, Level};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{I2C1, SPI0, USB};
use embassy_rp::spi::{self, Phase, Polarity, Spi};
use embassy_rp::usb::Driver;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_usb::{Config, UsbDevice};
use pico_de_gallo_internal::{
    ENDPOINT_LIST, GpioGet, GpioGetRequest, GpioGetResponse, GpioPut, GpioPutRequest, GpioPutResponse, GpioState,
    I2cRead, I2cReadFail, I2cReadRequest, I2cReadResponse, I2cWrite, I2cWriteFail, I2cWriteRequest, I2cWriteResponse,
    MICROSOFT_VID, PICO_DE_GALLO_PID, PingEndpoint, SetConfiguration, SetConfigurationFail, SetConfigurationRequest,
    SetConfigurationResponse, SpiFlush, SpiFlushFail, SpiFlushResponse, SpiPhase, SpiPolarity, SpiRead, SpiReadFail,
    SpiReadRequest, SpiReadResponse, SpiWrite, SpiWriteFail, SpiWriteRequest, SpiWriteResponse, TOPICS_IN_LIST,
    TOPICS_OUT_LIST, Version, VersionInfo,
};
use postcard_rpc::{
    define_dispatch,
    header::VarHeader,
    server::{
        Dispatch, Server,
        impls::embassy_usb_v0_5::{
            PacketBuffers,
            dispatch_impl::{WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorage, WireTxImpl},
        },
    },
};
use static_cell::ConstStaticCell;
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

// auto-generated version information from Cargo.toml
include!(concat!(env!("OUT_DIR"), "/version.rs"));

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
});

const NUM_GPIOS: usize = 8;
const BUFFER_SIZE: usize = 512;

pub struct Context {
    i2c: I2c<'static, I2C1, i2c::Async>,
    spi: Spi<'static, SPI0, spi::Async>,
    gpios: [Flex<'static>; NUM_GPIOS],
    buf: [u8; BUFFER_SIZE],
}

impl Context {
    fn new(
        i2c: I2c<'static, I2C1, i2c::Async>,
        spi: Spi<'static, SPI0, spi::Async>,
        gpio0: Flex<'static>,
        gpio1: Flex<'static>,
        gpio2: Flex<'static>,
        gpio3: Flex<'static>,
        gpio4: Flex<'static>,
        gpio5: Flex<'static>,
        gpio6: Flex<'static>,
        gpio7: Flex<'static>,
    ) -> Self {
        Self {
            i2c,
            spi,
            gpios: [gpio0, gpio1, gpio2, gpio3, gpio4, gpio5, gpio6, gpio7],
            buf: [0; BUFFER_SIZE],
        }
    }
}

type AppDriver = Driver<'static, USB>;
type AppStorage = WireStorage<ThreadModeRawMutex, AppDriver, 256, 256, 64, 256>;
type BufStorage = PacketBuffers<1024, 1024>;
type AppTx = WireTxImpl<ThreadModeRawMutex, AppDriver>;
type AppRx = WireRxImpl<AppDriver>;
type AppServer = Server<AppTx, AppRx, WireRxBuf, PicoDeGallo>;

static PBUFS: ConstStaticCell<BufStorage> = ConstStaticCell::new(BufStorage::new());
static STORAGE: AppStorage = AppStorage::new();

fn usb_config() -> Config<'static> {
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

    config
}

define_dispatch! {
    app: PicoDeGallo;
    spawn_fn: spawn_fn;
    tx_impl: AppTx;
    spawn_impl: WireSpawnImpl;
    context: Context;

    endpoints: {
        list: ENDPOINT_LIST;

        | EndpointTy       | kind     | handler            |
        | ----------       | ----     | -------            |
        | PingEndpoint     | blocking | ping_handler       |
        | I2cRead          | async    | i2c_read_handler   |
        | I2cWrite         | async    | i2c_write_handler  |
        | SpiRead          | async    | spi_read_handler   |
        | SpiWrite         | async    | spi_write_handler  |
        | SpiFlush         | async    | spi_flush_handler  |
        | GpioGet          | async    | gpio_get_handler   |
        | GpioPut          | async    | gpio_put_handler   |
        | SetConfiguration | async    | set_config_handler |
        | Version          | async    | version_handler    |
    };
    topics_in: {
        list: TOPICS_IN_LIST;

        | TopicTy                   | kind      | handler                       |
        | ----------                | ----      | -------                       |
    };
    topics_out: {
        list: TOPICS_OUT_LIST;
    };
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let config = embassy_rp::config::Config::new(ClockConfig::system_freq(150_000_000).unwrap());
    let p = embassy_rp::init(config);

    // USB/RPC INIT
    let driver = Driver::new(p.USB, Irqs);
    let pbufs = PBUFS.take();
    let config = usb_config();

    let i2c = embassy_rp::i2c::I2c::new_async(p.I2C1, p.PIN_3, p.PIN_2, Irqs, embassy_rp::i2c::Config::default());
    let spi = embassy_rp::spi::Spi::new(
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

    let context = Context::new(i2c, spi, gpio8, gpio9, gpio10, gpio11, gpio12, gpio13, gpio14, gpio15);

    let (device, tx_impl, rx_impl) = STORAGE.init(driver, config, pbufs.tx_buf.as_mut_slice());
    let dispatcher = PicoDeGallo::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let mut server: AppServer = Server::new(tx_impl, rx_impl, pbufs.rx_buf.as_mut_slice(), dispatcher, vkk);
    spawner.must_spawn(usb_task(device));

    loop {
        // If the host disconnects, we'll return an error here.
        // If this happens, just wait until the host reconnects
        let _ = server.run().await;
    }
}

/// This handles the low level USB management
#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, AppDriver>) {
    usb.run().await;
}

// ---

fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping");
    rqst
}

async fn i2c_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: I2cReadRequest,
) -> I2cReadResponse<'a> {
    if usize::from(req.count) > BUFFER_SIZE {
        return Err(I2cReadFail);
    }

    let len = ..usize::from(req.count);
    context
        .i2c
        .blocking_read(req.address, &mut context.buf[len])
        .map_err(|_| I2cReadFail)
        .map(|_| &context.buf[len])
}

async fn i2c_write_handler<'a>(
    context: &mut Context,
    _header: VarHeader,
    req: I2cWriteRequest<'a>,
) -> I2cWriteResponse {
    context
        .i2c
        .blocking_write(req.address, req.contents)
        .map_err(|_| I2cWriteFail)
}

async fn spi_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: SpiReadRequest,
) -> SpiReadResponse<'a> {
    if usize::from(req.count) > BUFFER_SIZE {
        return Err(SpiReadFail);
    }

    let len = ..usize::from(req.count);
    context
        .spi
        .blocking_read(&mut context.buf[len])
        .map_err(|_| SpiReadFail)
        .map(|_| &context.buf[len])
}

async fn spi_write_handler<'a>(
    context: &mut Context,
    _header: VarHeader,
    req: SpiWriteRequest<'a>,
) -> SpiWriteResponse {
    context.spi.blocking_write(req.contents).map_err(|_| SpiWriteFail)
}

async fn spi_flush_handler(context: &mut Context, _header: VarHeader, _req: ()) -> SpiFlushResponse {
    context.spi.flush().map_err(|_| SpiFlushFail)
}

async fn gpio_get_handler(context: &mut Context, _header: VarHeader, req: GpioGetRequest) -> GpioGetResponse {
    let pin = req.pin;
    let gpio = &mut context.gpios[usize::from(pin)];

    gpio.set_as_input();
    match gpio.get_level() {
        Level::Low => Ok(GpioState::Low),
        Level::High => Ok(GpioState::High),
    }
}

async fn gpio_put_handler(context: &mut Context, _header: VarHeader, req: GpioPutRequest) -> GpioPutResponse {
    let pin = req.pin;
    let gpio = &mut context.gpios[usize::from(pin)];

    let level = match req.state {
        GpioState::Low => Level::Low,
        GpioState::High => Level::High,
    };

    gpio.set_as_output();
    gpio.set_level(level);

    Ok(())
}

async fn set_config_handler(
    context: &mut Context,
    _header: VarHeader,
    req: SetConfigurationRequest,
) -> SetConfigurationResponse {
    let mut i2c_config = i2c::Config::default();
    let mut spi_config = spi::Config::default();

    i2c_config.frequency = req.i2c_frequency;
    spi_config.frequency = req.spi_frequency;
    spi_config.phase = match req.spi_phase {
        SpiPhase::CaptureOnFirstTransition => Phase::CaptureOnFirstTransition,
        SpiPhase::CaptureOnSecondTransition => Phase::CaptureOnSecondTransition,
    };
    spi_config.polarity = match req.spi_polarity {
        SpiPolarity::IdleLow => Polarity::IdleLow,
        SpiPolarity::IdleHigh => Polarity::IdleHigh,
    };

    context.spi.set_config(&spi_config);
    context.i2c.set_config(&i2c_config).map_err(|_| SetConfigurationFail)
}

async fn version_handler(_context: &mut Context, _header: VarHeader, _req: ()) -> VersionInfo {
    VersionInfo {
        major: VERSION_MAJOR,
        minor: VERSION_MINOR,
        patch: VERSION_PATCH,
    }
}
