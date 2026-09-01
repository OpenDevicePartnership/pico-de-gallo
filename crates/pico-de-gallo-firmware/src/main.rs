#![no_std]
#![no_main]
//! Pico de Gallo firmware for the Raspberry Pi Pico 2 (RP2350).
//!
//! This firmware implements a USB bridge that exposes I2C, SPI, UART, GPIO,
//! PWM, ADC, and 1-Wire peripherals to a host computer via [postcard-rpc](https://docs.rs/postcard-rpc)
//! endpoints. It runs on the [Embassy](https://embassy.dev) async runtime.
//!
//! # Peripheral Mapping
//!
//! | Function | RP2350 Pins | Notes |
//! |----------|-------------|-------|
//! | UART0 TX | GPIO 0 | Buffered, interrupt-driven |
//! | UART0 RX | GPIO 1 | |
//! | I2C1 SDA | GPIO 2 | 7-bit addressing, async mode |
//! | I2C1 SCL | GPIO 3 | |
//! | SPI0 SCK | GPIO 6 | DMA-backed full-duplex |
//! | SPI0 TX  | GPIO 7 | |
//! | SPI0 RX  | GPIO 4 | |
//! | GPIO 0–3 | GPIO 8–11 | Input/output/edge-wait |
//! | PWM 0    | GPIO 12 | Slice 6 channel A |
//! | PWM 1    | GPIO 13 | Slice 6 channel B |
//! | PWM 2    | GPIO 14 | Slice 7 channel A |
//! | PWM 3    | GPIO 15 | Slice 7 channel B |
//! | 1-Wire   | GPIO 16 | PIO0/SM0, open-drain |
//! | ADC 0    | GPIO 26 | 12-bit, 0–3.3 V nominal |
//! | ADC 1    | GPIO 27 | 12-bit, 0–3.3 V nominal |
//! | ADC 2    | GPIO 28 | 12-bit, 0–3.3 V nominal |
//! | ADC 3    | GPIO 29 | 12-bit, 0–3.3 V nominal |
//! | USB      | Native USB | postcard-rpc transport |
//!
//! # Endpoints
//!
//! See [`pico_de_gallo_internal::ENDPOINT_LIST`] for the full list of
//! supported endpoints and their request/response types.
//!
//! # Buffer Size
//!
//! The firmware uses a shared buffer of [`MAX_TRANSFER_SIZE`](pico_de_gallo_internal::MAX_TRANSFER_SIZE)
//! (4096) bytes for I2C and SPI data. Requests exceeding this limit are
//! rejected with an error.

#[cfg(all(feature = "hw-rev1", feature = "hw-rev2"))]
compile_error!("Features `hw-rev1` and `hw-rev2` are mutually exclusive");
#[cfg(not(any(feature = "hw-rev1", feature = "hw-rev2")))]
compile_error!("One of `hw-rev1` or `hw-rev2` must be enabled");

mod context;
mod handlers;

use core::sync::atomic::{AtomicU16, Ordering};
use defmt::{debug, warn};
use embassy_executor::Spawner;
#[cfg(feature = "hw-rev2")]
use embassy_rp::adc::{self, Adc};
use embassy_rp::clocks::ClockConfig;
#[cfg(feature = "hw-rev2")]
use embassy_rp::gpio::Pull;
use embassy_rp::gpio::{Flex, Level};
#[cfg(feature = "hw-rev2")]
use embassy_rp::peripherals::PIO0;
#[cfg(feature = "hw-rev2")]
use embassy_rp::pio;
#[cfg(feature = "hw-rev2")]
use embassy_rp::pio_programs::onewire::{PioOneWire, PioOneWireProgram};
use embassy_rp::pwm::{self, Pwm};
#[cfg(feature = "hw-rev2")]
use embassy_rp::uart::{self, BufferedUart};
use embassy_rp::usb::Driver;
use embassy_rp::watchdog::Watchdog;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::class::web_usb::{Config as WebUsbConfig, State as WebUsbState, Url, WebUsb};
use embassy_usb::{Config, UsbDevice};
use pico_de_gallo_internal::{
    AdcGetConfiguration, AdcRead, ENDPOINT_LIST, GetDeviceInfo, GpioEdge, GpioEvent, GpioEventTopic, GpioGet, GpioPut,
    GpioSetConfiguration, GpioState, GpioSubscribe, GpioUnsubscribe, GpioWaitForAny, GpioWaitForFalling,
    GpioWaitForHigh, GpioWaitForLow, GpioWaitForRising, I2cBatch, I2cGetConfiguration, I2cRead, I2cScan,
    I2cSetConfiguration, I2cWrite, I2cWriteRead, MAX_TRANSFER_SIZE, MICROSOFT_VID, OneWireRead, OneWireReset,
    OneWireSearch, OneWireSearchNext, OneWireWrite, OneWireWritePullup, PICO_DE_GALLO_PID, PingEndpoint, PwmDisable,
    PwmEnable, PwmGetConfiguration, PwmGetDutyCycle, PwmSetConfiguration, PwmSetDutyCycle, SpiBatch, SpiFlush,
    SpiGetConfiguration, SpiRead, SpiSetConfiguration, SpiTransfer, SpiWrite, SystemResetSubscriptions, TOPICS_IN_LIST,
    TOPICS_OUT_LIST, UartFlush, UartGetConfiguration, UartRead, UartSetConfiguration, UartWrite, Version,
};
use postcard_rpc::{
    define_dispatch,
    header::VarKeyKind,
    server::{
        Dispatch, Sender, Server,
        impls::embassy_usb_v0_5::{
            PacketBuffers, USB_FS_MAX_PACKET_SIZE,
            dispatch_impl::{WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorage, WireTxImpl},
        },
    },
};
use static_cell::ConstStaticCell;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

use context::*;

use handlers::*;

/// Binary info entries for `picotool info` identification.
///
/// These entries are placed in the `.bi_entries` linker section and allow
/// `picotool` to display the firmware name, description, version, and
/// build info without running the firmware.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"Pico de Gallo"),
    embassy_rp::binary_info::rp_program_description!(c"USB bridge to various buses such as I2C, SPI, UART, and ADC"),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

// auto-generated version information from Cargo.toml
include!(concat!(env!("OUT_DIR"), "/version.rs"));

// Bounds the generated value against the wire capacity: a BUILD_ID that
// would not fit `DeviceInfo::build_id` is a build failure here rather than
// a runtime `unwrap_or_default()` silently reporting an empty identity.
//
// Note what this does NOT do. `build.rs` duplicates the capacity literal
// because a build script cannot import from the crate it is building, and
// this assertion does not compare the two literals -- it only checks the
// value that was actually produced. A build.rs truncating at 128 would
// still pass here whenever `git describe` happened to emit under 64 bytes.
const _: () = assert!(BUILD_ID.len() <= pico_de_gallo_internal::BUILD_ID_CAPACITY);

/// Hardware revision. Bump when the physical board layout changes.
#[cfg(feature = "hw-rev1")]
pub(crate) const HW_VERSION: u8 = 1;
#[cfg(feature = "hw-rev2")]
pub(crate) const HW_VERSION: u8 = 2;

/// WebUSB landing page advertised in the URL descriptor.
///
/// Chrome surfaces this as a notification when the device is plugged in.
/// [`Url::new`] strips the scheme prefix and asserts that the remainder is at
/// most 252 bytes.
const WEBUSB_LANDING_URL: &str = "https://balbi.sh/pico-de-gallo/";

/// `Url::new` asserts the post-scheme-strip length fits the URL descriptor. Catch
/// an over-long URL at build time rather than as a boot panic: a panic in `main()`
/// happens before `watchdog_feeder_task` is ever polled, so the watchdog would
/// never arm and the device would hang dead.
const _: () = assert!(WEBUSB_LANDING_URL.len() <= 252);

/// `bRequest` value for WebUSB vendor control transfers.
///
/// Deliberately not `0`: postcard-rpc registers the Microsoft OS 2.0
/// descriptor at vendor code `0` (`init_without_build` calls
/// `msos_descriptor(WIN8_1, 0)`). embassy-usb 0.5.1 only intercepts that
/// request when `bRequest == 0 && wIndex == 7`, so `0` would not actually
/// collide with WebUSB's `GET_URL` (`wIndex == 2`) today, but keeping the two
/// vendor codes distinct avoids depending on that `wIndex` disambiguation.
const WEBUSB_VENDOR_CODE: u8 = 0x01;

/// USB driver type for the RP2350.
type AppDriver = Driver<'static, embassy_rp::peripherals::USB>;
/// postcard-rpc wire storage with ThreadMode mutex.
type AppStorage = WireStorage<ThreadModeRawMutex, AppDriver, 256, 256, 64, 256>;
/// Packet buffer storage sized for [`MAX_TRANSFER_SIZE`] plus protocol overhead.
type BufStorage = PacketBuffers<{ MAX_TRANSFER_SIZE + 1024 }, { MAX_TRANSFER_SIZE + 1024 }>;
/// postcard-rpc transmit implementation.
type AppTx = WireTxImpl<ThreadModeRawMutex, AppDriver>;
/// postcard-rpc receive implementation.
type AppRx = WireRxImpl<AppDriver>;
/// The complete postcard-rpc server type.
type AppServer = Server<AppTx, AppRx, WireRxBuf, PicoDeGallo>;

static PBUFS: ConstStaticCell<BufStorage> = ConstStaticCell::new(BufStorage::new());
static STORAGE: AppStorage = AppStorage::new();

// --- GPIO event monitoring infrastructure ---

/// Channel for sending a (Flex, GpioEdge) pair to start monitoring pin `n`.
static GPIO_MONITOR_START: [Channel<CriticalSectionRawMutex, (Flex<'static>, GpioEdge), 1>; NUM_GPIOS] =
    [Channel::new(), Channel::new(), Channel::new(), Channel::new()];

/// Signal to stop monitoring pin `n`.
static GPIO_MONITOR_STOP: [Signal<CriticalSectionRawMutex, ()>; NUM_GPIOS] =
    [Signal::new(), Signal::new(), Signal::new(), Signal::new()];

/// Channel for returning the Flex pin after monitoring stops.
static GPIO_MONITOR_RETURN: [Channel<CriticalSectionRawMutex, Flex<'static>, 1>; NUM_GPIOS] =
    [Channel::new(), Channel::new(), Channel::new(), Channel::new()];

/// Ack channel: monitor task signals it is armed and listening.
static GPIO_MONITOR_ARMED: [Channel<CriticalSectionRawMutex, (), 1>; NUM_GPIOS] =
    [Channel::new(), Channel::new(), Channel::new(), Channel::new()];

/// Monotonic sequence counter for published GPIO events.
static GPIO_EVENT_SEQ: AtomicU16 = AtomicU16::new(0);

/// Background task that monitors GPIO pin `N` for edge events.
///
/// The task waits for a `(Flex, GpioEdge)` on `GPIO_MONITOR_START[N]`,
/// sends an armed ack, then loops detecting edges and publishing
/// `GpioEvent` topics until `GPIO_MONITOR_STOP[N]` is signalled.
/// After stopping, the `Flex` pin is returned via `GPIO_MONITOR_RETURN[N]`.
///
/// Edge detection is best-effort: if the pin changes faster than the
/// monitor loop cadence, intermediate transitions may be missed.
#[embassy_executor::task(pool_size = 4)]
async fn gpio_monitor_task(slot: usize, tx: AppTx, vkk: VarKeyKind) {
    let sender = Sender::new(tx, vkk);

    loop {
        // Wait for a subscribe request to hand us a pin
        let (mut flex, edge) = GPIO_MONITOR_START[slot].receive().await;

        // Configure as input for edge detection
        flex.set_as_input();

        // Signal that we are armed and listening
        GPIO_MONITOR_ARMED[slot].send(()).await;

        debug!("gpio monitor[{}]: armed, edge={=u8}", slot, edge as u8);

        // Monitor loop: wait for edge or stop signal
        loop {
            use embassy_futures::select::{Either, select};

            let edge_future = async {
                match edge {
                    GpioEdge::Rising => flex.wait_for_rising_edge().await,
                    GpioEdge::Falling => flex.wait_for_falling_edge().await,
                    GpioEdge::Any => flex.wait_for_any_edge().await,
                }
            };

            match select(edge_future, GPIO_MONITOR_STOP[slot].wait()).await {
                Either::First(()) => {
                    let state = match flex.get_level() {
                        Level::Low => GpioState::Low,
                        Level::High => GpioState::High,
                    };
                    let timestamp_us = Instant::now().as_micros();
                    let seq = GPIO_EVENT_SEQ.fetch_add(1, Ordering::Relaxed);

                    let event = GpioEvent {
                        pin: slot as u8,
                        edge,
                        state,
                        timestamp_us,
                    };

                    debug!(
                        "gpio monitor[{}]: edge detected, state={=u8}, ts={=u64}",
                        slot, state as u8, timestamp_us
                    );

                    let _ = sender.publish::<GpioEventTopic>(seq.into(), &event).await;
                }
                Either::Second(()) => {
                    debug!("gpio monitor[{}]: stop requested", slot);
                    GPIO_MONITOR_RETURN[slot].send(flex).await;
                    break;
                }
            }
        }
    }
}

/// Build the USB device configuration.
///
/// Reads the chip unique ID from OTP to generate a deterministic serial
/// number string. Falls back to all-zeros if OTP read fails.
fn usb_config() -> Config<'static> {
    // Obtain the chip unique ID for USB serial number.
    // Falls back to "UNKNOWN" if OTP read fails (e.g. on some dev boards).
    let unique_id: u64 = embassy_rp::otp::get_chipid().unwrap_or_else(|e| {
        warn!("Failed to read chip ID: {:?}, using fallback serial", e);
        0
    });

    static SERIAL_STRING: StaticCell<[u8; 16]> = StaticCell::new();
    let mut ser_buf = [b'0'; 16];
    // `as_chunks_mut::<2>()` rather than `chunks_exact_mut(2)`: the chunk
    // size is a compile-time constant, so this yields `&mut [[u8; 2]]` with
    // no runtime remainder to discard. 16 is an exact multiple of 2, so the
    // remainder is statically empty.
    let (ser_pairs, _) = ser_buf.as_chunks_mut::<2>();
    unique_id.to_be_bytes().iter().zip(ser_pairs).for_each(|(b, chs)| {
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
    // Safety: ser_buf contains only ASCII hex digits, which are valid UTF-8.
    let ser_buf = core::str::from_utf8(ser_buf.as_slice()).unwrap();

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

        | EndpointTy           | kind     | handler                       |
        | ----------           | ----     | -------                       |
        | PingEndpoint         | blocking | ping_handler                  |
        | I2cRead              | async    | i2c_read_handler              |
        | I2cWrite             | async    | i2c_write_handler             |
        | I2cWriteRead         | async    | i2c_write_read_handler        |
        | SpiRead              | async    | spi_read_handler              |
        | SpiWrite             | async    | spi_write_handler             |
        | SpiFlush             | async    | spi_flush_handler             |
        | SpiTransfer          | async    | spi_transfer_handler          |
        | GpioGet              | async    | gpio_get_handler              |
        | GpioPut              | async    | gpio_put_handler              |
        | GpioWaitForHigh      | async    | gpio_wait_for_high_handler    |
        | GpioWaitForLow       | async    | gpio_wait_for_low_handler     |
        | GpioWaitForRising    | async    | gpio_wait_for_rising_handler  |
        | GpioWaitForFalling   | async    | gpio_wait_for_falling_handler |
        | GpioWaitForAny       | async    | gpio_wait_for_any_handler     |
        | GpioSetConfiguration | async    | gpio_set_config_handler       |
        | GpioSubscribe        | async    | gpio_subscribe_handler        |
        | GpioUnsubscribe      | async    | gpio_unsubscribe_handler      |
        | I2cSetConfiguration  | async    | i2c_set_config_handler        |
        | I2cScan              | async    | i2c_scan_handler              |
        | I2cBatch             | async    | i2c_batch_handler             |
        | SpiSetConfiguration  | async    | spi_set_config_handler        |
        | I2cGetConfiguration  | blocking | i2c_get_config_handler        |
        | SpiGetConfiguration  | blocking | spi_get_config_handler        |
        | SpiBatch             | async    | spi_batch_handler             |
        | UartRead             | async    | uart_read_handler             |
        | UartWrite            | async    | uart_write_handler            |
        | UartFlush            | async    | uart_flush_handler            |
        | UartSetConfiguration | async    | uart_set_config_handler       |
        | UartGetConfiguration | blocking | uart_get_config_handler       |
        | PwmSetDutyCycle      | blocking | pwm_set_duty_cycle_handler    |
        | PwmGetDutyCycle      | blocking | pwm_get_duty_cycle_handler    |
        | PwmEnable            | blocking | pwm_enable_handler            |
        | PwmDisable           | blocking | pwm_disable_handler           |
        | PwmSetConfiguration  | blocking | pwm_set_config_handler        |
        | PwmGetConfiguration  | blocking | pwm_get_config_handler        |
        | AdcRead              | blocking | adc_read_handler              |
        | AdcGetConfiguration  | blocking | adc_get_config_handler        |
        | OneWireReset         | async    | onewire_reset_handler         |
        | OneWireRead          | async    | onewire_read_handler          |
        | OneWireWrite         | async    | onewire_write_handler         |
        | OneWireWritePullup   | async    | onewire_write_pullup_handler  |
        | OneWireSearch        | async    | onewire_search_handler        |
        | OneWireSearchNext    | async    | onewire_search_next_handler   |
        | Version              | async    | version_handler               |
        | GetDeviceInfo        | blocking | device_info_handler           |
        | SystemResetSubscriptions | async | system_reset_subscriptions_handler |
    };
    topics_in: {
        list: TOPICS_IN_LIST;

        | TopicTy                   | kind      | handler                 |
        | ----------                | ----      | -------                 |
    };
    topics_out: {
        list: TOPICS_OUT_LIST;
    };
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let config = embassy_rp::config::Config::new(ClockConfig::system_freq(150_000_000).unwrap());
    let p = embassy_rp::init(config);

    // Arm the hardware watchdog as defence-in-depth against handler hangs.
    spawner.must_spawn(watchdog_feeder_task(Watchdog::new(p.WATCHDOG)));

    // Logged unconditionally at boot so an RTT capture records exactly which
    // image is running, independent of any host-side query.
    defmt::info!("build {}", BUILD_ID);

    // `hw-rev1` is deprecated. Warn on every boot so a device flashed with it
    // is identifiable from RTT alone, not only from the build log. Emitted
    // after the watchdog is armed so nothing runs ahead of it.
    #[cfg(feature = "hw-rev1")]
    warn!(
        "hw-rev1 firmware is DEPRECATED (v1.0 landing board): UART, ADC and 1-Wire return Unsupported. \
         Removal no earlier than 2031-09-01. Build the default hw-rev2 image for v1.1+ boards."
    );

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
        Irqs,
        embassy_rp::spi::Config::default(),
    );

    let gpios = [
        Flex::new(p.PIN_8),
        Flex::new(p.PIN_9),
        Flex::new(p.PIN_10),
        Flex::new(p.PIN_11),
    ];

    // PWM on GPIO12-15 (slices 6-7)
    let pwm_config = pwm::Config::default();
    let pwm_slices = [
        Pwm::new_output_ab(p.PWM_SLICE6, p.PIN_12, p.PIN_13, pwm_config.clone()),
        Pwm::new_output_ab(p.PWM_SLICE7, p.PIN_14, p.PIN_15, pwm_config.clone()),
    ];
    let pwm_configs = [pwm::Config::default(), pwm::Config::default()];

    // UART0 on GPIO0 (TX) and GPIO1 (RX) — default Pico 2 header pins
    #[cfg(feature = "hw-rev2")]
    let uart = {
        static UART_TX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();
        static UART_RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();
        let uart_tx_buf = UART_TX_BUF.init([0u8; 1024]);
        let uart_rx_buf = UART_RX_BUF.init([0u8; 1024]);
        BufferedUart::new(
            p.UART0,
            p.PIN_0,
            p.PIN_1,
            Rev2Irqs,
            uart_tx_buf,
            uart_rx_buf,
            uart::Config::default(),
        )
    };

    // ADC — blocking mode for single-shot reads (GPIO26–29)
    #[cfg(feature = "hw-rev2")]
    let adc = Adc::new_blocking(p.ADC, adc::Config::default());
    #[cfg(feature = "hw-rev2")]
    let adc_channels = [
        adc::Channel::new_pin(p.PIN_26, Pull::None),
        adc::Channel::new_pin(p.PIN_27, Pull::None),
        adc::Channel::new_pin(p.PIN_28, Pull::None),
        adc::Channel::new_pin(p.PIN_29, Pull::None),
    ];

    // 1-Wire — PIO0/SM0 on GPIO16
    #[cfg(feature = "hw-rev2")]
    let onewire = {
        let pio::Pio { mut common, sm0, .. } = pio::Pio::new(p.PIO0, Rev2Irqs);
        static OW_PROGRAM: StaticCell<PioOneWireProgram<'static, PIO0>> = StaticCell::new();
        let ow_program = OW_PROGRAM.init(PioOneWireProgram::new(&mut common));
        PioOneWire::new(&mut common, sm0, p.PIN_16, ow_program)
    };

    #[cfg(feature = "hw-rev2")]
    let context = Context::new(
        i2c,
        spi,
        uart,
        gpios,
        pwm_slices,
        pwm_configs,
        adc,
        adc_channels,
        onewire,
    );

    #[cfg(not(feature = "hw-rev2"))]
    let context = Context::new(i2c, spi, gpios, pwm_slices, pwm_configs);

    let (mut builder, tx_impl, rx_impl) =
        STORAGE.init_without_build(driver, config, pbufs.tx_buf.as_mut_slice(), USB_FS_MAX_PACKET_SIZE);

    // Advertise WebUSB so browsers can discover the device and surface the
    // landing page.
    //
    // This MUST run after init_without_build(): postcard-rpc's bulk interface
    // has to stay interface 0, because every host transport selects the FIRST
    // class-0xFF interface — postcard-rpc 0.12.1
    // `host_client::raw_nusb::try_new_raw_nusb` (first 0xFF on Linux/macOS,
    // hardcoded interface 0 on Windows) and `host_client::webusb::from_device`
    // (first 0xFF, then requires IN+OUT endpoints). WebUsb::configure() appends
    // a second class-0xFF interface with an identical class/subclass/protocol
    // triple and no endpoints, so ordering is the only thing distinguishing
    // them; if it ran first, the browser would fail with "Failed to find usable
    // interface" and Linux/macOS would silently claim the wrong one.
    //
    // Descriptor budgets (BOS, config) are set by the AppStorage type alias
    // above; overflowing either is a "Descriptor buffer full" panic.
    static WEBUSB_STATE: StaticCell<WebUsbState<'static>> = StaticCell::new();
    static WEBUSB_CONFIG: StaticCell<WebUsbConfig<'static>> = StaticCell::new();
    let webusb_config = WEBUSB_CONFIG.init(WebUsbConfig {
        // Inert: WebUsb::configure() creates no endpoints, and embassy-usb
        // 0.5.1 never reads this field. It has no Default, so it must be set.
        max_packet_size: USB_FS_MAX_PACKET_SIZE as u16,
        landing_url: Some(Url::new(WEBUSB_LANDING_URL)),
        vendor_code: WEBUSB_VENDOR_CODE,
    });
    WebUsb::configure(&mut builder, WEBUSB_STATE.init(WebUsbState::new()), webusb_config);

    let device = builder.build();
    let dispatcher = PicoDeGallo::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();

    // Spawn GPIO monitor tasks before creating the server.
    // EUsbWireTx is Clone, so we clone tx_impl for each task.
    for slot in 0..NUM_GPIOS {
        spawner.must_spawn(gpio_monitor_task(slot, tx_impl.clone(), vkk));
    }

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

/// Dedicated watchdog feeder task.
///
/// Feeds the embassy-rp watchdog every 800 ms with a 2-second timeout.
/// The 800 ms cadence leaves margin for the embassy executor's scheduling
/// jitter while keeping the worst-case recovery time under 2 seconds when
/// a handler genuinely wedges (e.g., a 1-Wire PIO program stalling on a
/// shorted bus, an embassy-rp peripheral bug).
///
/// This task MUST be the only feeder. RPC handlers cannot be trusted to
/// feed — the dispatcher-wedge regression closed in commit fdb3ba15e64d
/// means a wedged handler would also wedge any handler-based feed scheme.
/// See docs/superpowers/specs/2026-06-03-pico-de-gallo-category-a-review-synthesis.md
/// finding #3.
#[embassy_executor::task]
async fn watchdog_feeder_task(mut watchdog: Watchdog) {
    watchdog.start(Duration::from_secs(2));
    watchdog.pause_on_debug(true);
    loop {
        Timer::after(Duration::from_millis(800)).await;
        watchdog.feed(Duration::from_secs(2));
    }
}
