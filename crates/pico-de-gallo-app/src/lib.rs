//! Command-line interface for the Pico de Gallo USB bridge.
//!
//! The `gallo` CLI provides direct access to I2C, SPI, UART, GPIO, PWM, ADC, and 1-Wire peripherals
//! connected through a Pico de Gallo device. It is built with
//! [clap](https://docs.rs/clap) and supports:
//!
//! - **I2C**: bus scanning, read, write, and write-then-read operations
//! - **SPI**: read, write, full-duplex transfer, and write-then-read
//! - **UART**: read, write, flush, and baud rate configuration
//! - **PWM**: duty cycle control, enable/disable, frequency/phase configuration
//! - **ADC**: single-shot reads, configuration queries
//! - **1-Wire**: reset, read, write, strong-pullup write, ROM search
//! - **GPIO**: read/write pins, edge event monitoring with subscribe/unsubscribe
//! - **Configuration**: set I2C/SPI/UART bus frequencies and SPI mode
//! - **Device management**: list connected devices, query firmware version
//!
//! # Examples
//!
//! ```console
//! $ gallo list
//! $ gallo version
//! $ gallo i2c scan
//! $ gallo i2c read -a 0x48 -c 2
//! $ gallo i2c write -a 0x50 -b 0xDE 0xAD
//! $ gallo spi transfer -b 0x01 0x02 0x03
//! $ gallo set-config --i2c-frequency 400000 --spi-frequency 1000000
//! ```
//!
//! # Output Formats
//!
//! Read data can be displayed in three formats via the `-f` / `--format` flag:
//! - `hex` (default): hexadecimal byte dump
//! - `binary`: raw bytes written to stdout
//! - `ascii`: printable characters shown, non-printable replaced with `.`

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::{Result, eyre::eyre};
use pico_de_gallo_lib::{
    AdcChannel, DeviceInfo, GpioEdge, I2cFrequency, PicoDeGallo, SpiPhase, SpiPolarity, ValidateError, list_devices,
};
use pico_de_gallo_lib::{GpioDirection, GpioPull, GpioState};
use std::num::ParseIntError;
use std::time::Duration;
use tabled::builder::Builder;
use tabled::settings::object::Rows;
use tabled::settings::{Alignment, Style};

/// How long `gallo version` waits for `device/info` before falling back to
/// the legacy `version` endpoint.
///
/// Deliberately far shorter than `pico_de_gallo_lib::DEVICE_INFO_TIMEOUT`
/// (300 s). That bound is sized for the worst-case *firmware* occupancy:
/// `spi_batch` accepts up to 64 delay operations of `u32::MAX` nanoseconds
/// each, and postcard-rpc dispatches handlers serially, so any request can
/// legitimately queue behind minutes of user-requested delay. `device/info`
/// itself has no user-controllable work: it is a fixed-size response
/// assembled from constants.
///
/// `version` is a diagnostic command, and a diagnostic that hangs is worse
/// than useless. Five seconds is generous for USB enumeration plus one
/// round trip while still failing fast, and the fallback to the legacy
/// endpoint is strictly better than waiting: it is the path that keeps
/// working when the two sides were built from different trees.
const VERSION_DEVICE_INFO_TIMEOUT: Duration = Duration::from_secs(5);

/// I2C bus clock frequency for CLI argument parsing.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cFrequencyArg {
    /// Standard mode — 100 kHz
    Standard,
    /// Fast mode — 400 kHz
    Fast,
    /// Fast+ mode — 1 MHz
    FastPlus,
}

impl From<I2cFrequencyArg> for I2cFrequency {
    fn from(arg: I2cFrequencyArg) -> Self {
        match arg {
            I2cFrequencyArg::Standard => I2cFrequency::Standard,
            I2cFrequencyArg::Fast => I2cFrequency::Fast,
            I2cFrequencyArg::FastPlus => I2cFrequency::FastPlus,
        }
    }
}

/// GPIO pin direction for CLI argument parsing.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioDirectionArg {
    /// Configure pin as input
    Input,
    /// Configure pin as output
    Output,
}

impl From<GpioDirectionArg> for GpioDirection {
    fn from(arg: GpioDirectionArg) -> Self {
        match arg {
            GpioDirectionArg::Input => GpioDirection::Input,
            GpioDirectionArg::Output => GpioDirection::Output,
        }
    }
}

/// GPIO pull resistor configuration for CLI argument parsing.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioPullArg {
    /// No internal pull resistor
    None,
    /// Internal pull-up resistor
    Up,
    /// Internal pull-down resistor
    Down,
}

impl From<GpioPullArg> for GpioPull {
    fn from(arg: GpioPullArg) -> Self {
        match arg {
            GpioPullArg::None => GpioPull::None,
            GpioPullArg::Up => GpioPull::Up,
            GpioPullArg::Down => GpioPull::Down,
        }
    }
}

/// GPIO edge detection mode for CLI argument parsing.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioEdgeArg {
    /// Trigger on rising edge (low → high)
    Rising,
    /// Trigger on falling edge (high → low)
    Falling,
    /// Trigger on any edge (rising or falling)
    Any,
}

impl From<GpioEdgeArg> for GpioEdge {
    fn from(arg: GpioEdgeArg) -> Self {
        match arg {
            GpioEdgeArg::Rising => GpioEdge::Rising,
            GpioEdgeArg::Falling => GpioEdge::Falling,
            GpioEdgeArg::Any => GpioEdge::Any,
        }
    }
}

/// GPIO output level for CLI argument parsing.
///
/// Used by `gallo gpio put --level <high|low>`. This is an explicit value
/// enum rather than a boolean flag so that both levels are settable and so
/// that no short option is derived (`-h` belongs to `--help`).
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpioLevelArg {
    /// Drive the pin high
    High,
    /// Drive the pin low
    Low,
}

impl From<GpioLevelArg> for GpioState {
    fn from(arg: GpioLevelArg) -> Self {
        match arg {
            GpioLevelArg::High => GpioState::High,
            GpioLevelArg::Low => GpioState::Low,
        }
    }
}

/// Output format for data display.
#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    /// Hexadecimal byte dump (default)
    #[default]
    Hex,
    /// Raw binary output (bytes written directly to stdout)
    Binary,
    /// ASCII representation (printable chars shown, others as '.')
    Ascii,
}

/// Top-level CLI argument parser.
///
/// Parse with [`clap::Parser::parse`] and execute with [`Cli::run`].
#[derive(Parser, Debug)]
#[command(
    name = "Pico De Gallo",
    author = "Felipe Balbi <febalbi@microsoft.com>",
    about = "Access I2C/SPI devices through Pico De Gallo",
    // Without this, clap derives `long_about` from this struct's rustdoc and
    // `gallo --help` shows the internal API documentation above instead of
    // `about`. The rustdoc is for `docs.rs` readers; `about` is for users.
    long_about = None,
    arg_required_else_help = true,
    version
)]
pub struct Cli {
    /// Select a specific board by USB serial number
    #[arg(short, long)]
    serial_number: Option<String>,

    /// Output format for read data
    #[arg(short, long, value_enum, default_value_t)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List all connected Pico de Gallo devices
    List,

    /// Check device liveness with a round-trip echo
    Ping,

    /// Get firmware version
    Version,

    /// I2C access methods
    I2c {
        /// I2C commands
        #[command(subcommand)]
        command: I2cCommands,
    },

    /// SPI access methods
    Spi {
        /// SPI commands
        #[command(subcommand)]
        command: SpiCommands,
    },

    /// GPIO access methods
    Gpio {
        /// GPIO commands
        #[command(subcommand)]
        command: GpioCommands,
    },

    /// UART access methods
    Uart {
        /// UART commands
        #[command(subcommand)]
        command: UartCommands,
    },

    /// PWM control methods
    Pwm {
        /// PWM commands
        #[command(subcommand)]
        command: PwmCommands,
    },

    /// ADC access methods
    Adc {
        /// ADC commands
        #[command(subcommand)]
        command: AdcCommands,
    },

    /// 1-Wire bus access methods
    #[command(name = "onewire")]
    OneWire {
        /// 1-Wire commands
        #[command(subcommand)]
        command: OneWireCommands,
    },
}

#[derive(Subcommand, Debug)]
enum I2cCommands {
    /// Scan I2C bus for existing devices
    Scan {
        /// Attempt reserved addresses
        #[arg(short, long, default_value_t = false)]
        reserved: bool,
    },

    /// Read bytes through the I2C bus from device at given address
    Read {
        /// I2C slave address (7-bit, 0x00–0x7F)
        #[arg(short, long, value_parser(parse_i2c_address))]
        address: u8,

        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },

    /// Write bytes through I2C bus to device at given address
    Write {
        /// I2C slave address (7-bit, 0x00–0x7F)
        #[arg(short, long, value_parser(parse_i2c_address))]
        address: u8,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Write bytes follwed by read bytes
    WriteRead {
        /// I2C slave address (7-bit, 0x00–0x7F)
        #[arg(short, long, value_parser(parse_i2c_address))]
        address: u8,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,

        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },

    /// Set I2C bus parameters
    SetConfig {
        /// I2C frequency: standard (100 kHz), fast (400 kHz), fast-plus (1 MHz)
        #[arg(long)]
        frequency: I2cFrequencyArg,
    },

    /// Query the current I2C bus configuration
    GetConfig,

    /// Execute multiple I2C operations as a single transaction
    ///
    /// Each operation is specified with --op. Use 'read:N' to read N bytes
    /// or 'write:B1,B2,...' to write bytes (hex or decimal).
    ///
    /// The whole batch runs as one I2C transaction: a START and the address
    /// precede the first operation, adjacent operations of the same type are
    /// sent back to back with no STOP and no repeated START between them, a
    /// direction change emits a repeated START and re-addresses the target,
    /// and only the last operation is followed by a STOP. Two adjacent
    /// writes therefore form a single gather write.
    ///
    /// Zero-length writes are rejected, and the whole batch is validated
    /// before anything is driven onto the bus. Requires firmware from
    /// schema 0.7 or newer; older firmware executes each operation as its
    /// own transaction.
    ///
    /// Example: gallo i2c batch -a 0x50 --op write:0x00,0x10 --op read:16
    Batch {
        /// I2C slave address (7-bit, 0x00–0x7F)
        #[arg(short, long, value_parser(parse_i2c_address))]
        address: u8,

        /// Operations: read:N or write:B1,B2,...
        #[arg(long, num_args(1..), required = true)]
        op: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum SpiCommands {
    /// Read bytes through SPI bus
    Read {
        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },

    /// Write bytes through SPI bus
    Write {
        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Full-duplex SPI transfer (simultaneous write and read)
    Transfer {
        /// Bytes to send (received data will be the same length)
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Write bytes followed by read bytes (half-duplex)
    WriteRead {
        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Set SPI bus parameters
    SetConfig {
        /// SPI frequency in Hz
        #[arg(long)]
        frequency: u32,

        /// SPI mode 0-3, the conventional (CPOL, CPHA) pairing.
        ///
        /// 0 = CPOL 0 / CPHA 0, 1 = CPOL 0 / CPHA 1,
        /// 2 = CPOL 1 / CPHA 0, 3 = CPOL 1 / CPHA 1.
        /// Defaults to 0, matching the firmware's power-on configuration.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
        mode: u8,
    },

    /// Query the current SPI bus configuration
    GetConfig,

    /// Execute multiple SPI operations atomically under chip-select
    ///
    /// Each operation is specified with --op. Use 'read:N', 'write:B1,B2,...',
    /// 'transfer:B1,B2,...', or 'delay:NS'.
    ///
    /// Example: gallo spi batch --cs 0 --op write:0x9F --op read:3
    Batch {
        /// GPIO pin to use as chip-select
        ///
        /// Checked at run time against the GPIO count the connected device
        /// reports, not against a fixed range.
        #[arg(long)]
        cs: u8,

        /// Operations: read:N, write:B1,B2,..., transfer:B1,B2,..., delay:NS
        #[arg(long, num_args(1..), required = true)]
        op: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum GpioCommands {
    /// Read the current level of a GPIO pin
    Get {
        /// GPIO pin number (0–7)
        #[arg(short, long)]
        pin: u8,
    },

    /// Set a GPIO pin to a specific level
    Put {
        /// GPIO pin number (0–7)
        #[arg(short, long)]
        pin: u8,

        /// Desired level: high or low
        ///
        /// Deliberately has no short option: `-h` is reserved for `--help`.
        #[arg(long)]
        level: GpioLevelArg,
    },

    /// Configure a GPIO pin's direction and pull resistor
    SetConfig {
        /// GPIO pin number (0–7)
        #[arg(short, long)]
        pin: u8,

        /// Pin direction: input or output
        #[arg(short, long)]
        direction: GpioDirectionArg,

        /// Internal pull resistor: none, up, or down
        #[arg(long, default_value = "none")]
        pull: GpioPullArg,
    },

    /// Monitor a GPIO pin for edge events (Ctrl+C to stop)
    Monitor {
        /// GPIO pin number (0–3)
        #[arg(short, long)]
        pin: u8,

        /// Edge detection mode
        #[arg(short, long, default_value = "any")]
        edge: GpioEdgeArg,
    },
}

#[derive(Subcommand, Debug)]
enum UartCommands {
    /// Read bytes from the UART bus
    Read {
        /// Number of bytes to read (up to 4096)
        #[arg(short, long)]
        count: u16,

        /// Read timeout in milliseconds (0 = non-blocking)
        #[arg(short, long, default_value_t = 1000)]
        timeout: u32,
    },

    /// Write bytes to the UART bus
    Write {
        /// Bytes to send
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Flush the UART transmit buffer
    Flush,

    /// Set UART bus parameters
    SetConfig {
        /// Baud rate in bits per second (e.g. 9600, 115200)
        #[arg(long)]
        baud_rate: u32,
    },

    /// Query the current UART bus configuration
    GetConfig,
}

#[derive(Subcommand, Debug)]
enum PwmCommands {
    /// Set the duty cycle of a PWM channel (raw value)
    SetDuty {
        /// PWM channel (0–3)
        #[arg(short, long)]
        channel: u8,

        /// Raw duty cycle value (0 to top)
        #[arg(short, long)]
        duty: u16,
    },

    /// Query the current duty cycle of a PWM channel
    GetDuty {
        /// PWM channel (0–3)
        #[arg(short, long)]
        channel: u8,
    },

    /// Enable a PWM slice (both channels on the slice)
    Enable {
        /// PWM channel (0–3). The parent slice is enabled.
        #[arg(short, long)]
        channel: u8,
    },

    /// Disable a PWM slice (both channels on the slice)
    Disable {
        /// PWM channel (0–3). The parent slice is disabled.
        #[arg(short, long)]
        channel: u8,
    },

    /// Configure PWM frequency and phase-correct mode
    SetConfig {
        /// PWM channel (0–3). The parent slice is configured.
        #[arg(short, long)]
        channel: u8,

        /// Desired output frequency in Hz
        #[arg(short, long)]
        frequency: u32,

        /// Enable phase-correct mode
        #[arg(short, long, default_value_t = false)]
        phase_correct: bool,
    },

    /// Query the current PWM configuration
    GetConfig {
        /// PWM channel (0–3)
        #[arg(short, long)]
        channel: u8,
    },
}

#[derive(Subcommand, Debug)]
enum AdcCommands {
    /// Read a single ADC sample (raw 12-bit value)
    Read {
        /// ADC channel: 0–3 for GPIO26–29
        #[arg(short, long)]
        channel: u8,
    },

    /// Query ADC configuration (resolution, reference, channels)
    Info,
}

#[derive(Subcommand, Debug)]
enum OneWireCommands {
    /// Reset the 1-Wire bus and detect device presence
    Reset,

    /// Read bytes from the 1-Wire bus
    Read {
        /// Number of bytes to read
        #[arg(short, long)]
        len: u16,
    },

    /// Write raw bytes to the 1-Wire bus
    Write {
        /// Hex-encoded data bytes (e.g., cc44)
        #[arg(short, long, value_parser(parse_hex_string))]
        data: Vec<u8>,
    },

    /// Write bytes with a strong pullup for parasitic-power devices
    WritePullup {
        /// Hex-encoded data bytes (e.g., cc44)
        #[arg(short, long, value_parser(parse_hex_string))]
        data: Vec<u8>,

        /// Duration of strong pullup in milliseconds
        #[arg(short = 't', long, default_value_t = 750)]
        duration: u16,
    },

    /// Search for all devices on the 1-Wire bus
    Search,
}

/// Render `device/info` as two tables.
///
/// Pure and returning `String` rather than printing, so the formatting is
/// testable. Two tables rather than one because capabilities are naturally a
/// wide boolean row; packing seven ticks into a single value cell is what the
/// previous hand-rolled output did badly.
fn render_device_info(info: &DeviceInfo) -> String {
    use pico_de_gallo_lib::Capabilities;

    let mut summary = Builder::with_capacity(5, 2);
    summary.push_record([
        "Firmware".to_string(),
        format!("v{}.{}.{}", info.fw_major, info.fw_minor, info.fw_patch),
    ]);
    summary.push_record([
        "Schema".to_string(),
        format!("v{}.{}.{}", info.schema_major, info.schema_minor, info.schema_patch),
    ]);
    summary.push_record(["HW revision".to_string(), info.hw_version.to_string()]);
    summary.push_record(["GPIOs".to_string(), info.num_gpios.to_string()]);
    // Last, mirroring `build_id` being the last wire field.
    summary.push_record(["Build".to_string(), info.build_id().to_string()]);

    let mut summary = summary.build();
    // Key/value rows, not a header plus data: `Builder` would otherwise
    // render record 0 as a header and draw a rule under `Firmware`.
    summary.with(Style::rounded().remove_horizontals());

    let caps = [
        ("I2C", Capabilities::I2C),
        ("SPI", Capabilities::SPI),
        ("UART", Capabilities::UART),
        ("GPIO", Capabilities::GPIO),
        ("PWM", Capabilities::PWM),
        ("ADC", Capabilities::ADC),
        ("1-Wire", Capabilities::ONEWIRE),
    ];

    let mut caps_table = Builder::with_capacity(2, caps.len());
    caps_table.push_record(caps.iter().map(|(name, _)| (*name).to_string()));
    caps_table.push_record(caps.iter().map(|(_, flag)| {
        if info.capabilities.contains(*flag) {
            "✓".to_string()
        } else {
            "✗".to_string()
        }
    }));

    let mut caps_table = caps_table.build();
    caps_table.with(Style::rounded());

    format!("{summary}\n{caps_table}")
}

fn print_data(data: &[u8], format: &OutputFormat) {
    match format {
        OutputFormat::Hex => {
            for (i, b) in data.iter().enumerate() {
                if i > 0 && i % 16 == 0 {
                    println!();
                }
                print!("{:02x} ", b);
            }
            println!();
        }
        OutputFormat::Binary => {
            use std::io::Write;
            std::io::stdout().write_all(data).unwrap();
        }
        OutputFormat::Ascii => {
            for (i, b) in data.iter().enumerate() {
                if i > 0 && i % 16 == 0 {
                    println!();
                }
                let ch = if b.is_ascii_graphic() || *b == b' ' {
                    *b as char
                } else {
                    '.'
                };
                print!("{ch}");
            }
            println!();
        }
    }
}

impl Cli {
    fn connect(&self) -> PicoDeGallo {
        if let Some(serial_number) = &self.serial_number {
            PicoDeGallo::new_with_serial_number(serial_number)
        } else {
            PicoDeGallo::new()
        }
    }

    /// Validate the connected firmware's schema version up-front so a
    /// mismatch is surfaced with a clear, actionable error message
    /// rather than as a confusing `CommsFailed` on the first RPC.
    ///
    /// Validation runs on the shared connection opened by [`Cli::run`],
    /// for every subcommand except `list` (no device needed), `version`
    /// (the diagnostic subcommand that explicitly reports the schema
    /// skew), and `ping` (the transport-level liveness check, which must
    /// stay answerable on a board whose schema does not match).
    ///
    /// Returns the [`pico_de_gallo_lib::DeviceInfo`] so `run` can retain
    /// the device-reported GPIO count and hand it to the handlers that
    /// need a runtime-authoritative pin bound, without a second query.
    ///
    /// The `device/info` round-trip is bounded at 300 seconds by the
    /// library; on expiry the message below reports a validation timeout
    /// rather than hanging forever.
    ///
    /// Closes Category A finding #4 (reviewer R4) at the CLI layer.
    async fn validate_firmware(&self, pg: &PicoDeGallo) -> Result<DeviceInfo> {
        pg.validate().await.map_err(|e| eyre!(validation_failure_message(&e)))
    }

    /// Execute the CLI command.
    ///
    /// Dispatches to the appropriate handler based on the parsed subcommand.
    /// Returns `Ok(())` on success or an error via `color_eyre`.
    ///
    /// A single USB connection is opened here and shared by reference with
    /// every device-touching handler. Opening a second connection while the
    /// first is still tearing down its background `nusb` worker triggers a
    /// WinUSB `Access is denied` error on Windows, where WinUSB grants
    /// exclusive access to one session per interface. Sharing one `pg`
    /// avoids that race. See the `CHANGELOG.md` entry for the regression.
    pub async fn run(&self) -> Result<()> {
        // `list` enumerates devices without opening (claiming) any of
        // them, so handle it before establishing a connection — it must
        // work with zero or multiple devices attached.
        if let Commands::List = &self.command {
            return Self::list_devices();
        }

        // Open exactly one connection for the whole invocation.
        let pg = self.connect();

        // Validate the firmware schema up-front for every subcommand that
        // touches the device, except `version` (the diagnostic subcommand
        // that reports schema skew itself) and `ping` (the transport-level
        // liveness check — validating first would report a schema error on
        // a board whose USB path is exactly what the operator is trying to
        // test). Without this, a schema mismatch would manifest as a
        // confusing CommsFailed on the first RPC. See Category A finding #4.
        //
        // The returned metadata is retained so handlers that need the
        // device-reported GPIO count get it without a second query.
        let info = if matches!(self.command, Commands::Version | Commands::Ping) {
            None
        } else {
            Some(self.validate_firmware(&pg).await?)
        };

        match &self.command {
            Commands::List => unreachable!("handled before connecting"),
            Commands::Ping => self.ping(&pg).await,
            Commands::Version => self.version(&pg).await,
            Commands::I2c { command } => match command {
                I2cCommands::Scan { reserved } => self.i2c_scan(&pg, *reserved).await,
                I2cCommands::Read { address, count } => self.i2c_read(&pg, address, count).await,
                I2cCommands::Write { address, bytes } => self.i2c_write(&pg, address, bytes).await,
                I2cCommands::WriteRead { address, bytes, count } => {
                    self.i2c_write_then_read(&pg, address, bytes, count).await
                }
                I2cCommands::SetConfig { frequency } => self.i2c_set_config(&pg, (*frequency).into()).await,
                I2cCommands::GetConfig => self.i2c_get_config(&pg).await,
                I2cCommands::Batch { address, op } => self.i2c_batch(&pg, *address, op).await,
            },
            Commands::Spi { command } => match command {
                SpiCommands::Read { count } => self.spi_read(&pg, count).await,
                SpiCommands::Write { bytes } => self.spi_write(&pg, bytes).await,
                SpiCommands::Transfer { bytes } => self.spi_transfer(&pg, bytes).await,
                SpiCommands::WriteRead { count, bytes } => self.spi_write_then_read(&pg, bytes, count).await,
                SpiCommands::SetConfig { frequency, mode } => self.spi_set_config(&pg, *frequency, *mode).await,
                SpiCommands::GetConfig => self.spi_get_config(&pg).await,
                SpiCommands::Batch { cs, op } => {
                    let num_gpios = info
                        .as_ref()
                        .expect("validation runs for every subcommand except version")
                        .num_gpios;
                    self.spi_batch(&pg, *cs, op, num_gpios).await
                }
            },
            Commands::Gpio { command } => match command {
                GpioCommands::Get { pin } => self.gpio_get(&pg, *pin).await,
                GpioCommands::Put { pin, level } => self.gpio_put(&pg, *pin, *level).await,
                GpioCommands::SetConfig { pin, direction, pull } => {
                    self.gpio_set_config(&pg, *pin, *direction, *pull).await
                }
                GpioCommands::Monitor { pin, edge } => self.gpio_monitor(&pg, *pin, *edge).await,
            },
            Commands::Uart { command } => match command {
                UartCommands::Read { count, timeout } => self.uart_read(&pg, *count, *timeout).await,
                UartCommands::Write { bytes } => self.uart_write(&pg, bytes).await,
                UartCommands::Flush => self.uart_flush(&pg).await,
                UartCommands::SetConfig { baud_rate } => self.uart_set_config(&pg, *baud_rate).await,
                UartCommands::GetConfig => self.uart_get_config(&pg).await,
            },
            Commands::Pwm { command } => match command {
                PwmCommands::SetDuty { channel, duty } => self.pwm_set_duty(&pg, *channel, *duty).await,
                PwmCommands::GetDuty { channel } => self.pwm_get_duty(&pg, *channel).await,
                PwmCommands::Enable { channel } => self.pwm_enable(&pg, *channel).await,
                PwmCommands::Disable { channel } => self.pwm_disable(&pg, *channel).await,
                PwmCommands::SetConfig {
                    channel,
                    frequency,
                    phase_correct,
                } => self.pwm_set_config(&pg, *channel, *frequency, *phase_correct).await,
                PwmCommands::GetConfig { channel } => self.pwm_get_config(&pg, *channel).await,
            },
            Commands::Adc { command } => match command {
                AdcCommands::Read { channel } => self.adc_read(&pg, *channel).await,
                AdcCommands::Info => self.adc_get_info(&pg).await,
            },
            Commands::OneWire { command } => match command {
                OneWireCommands::Reset => self.onewire_reset(&pg).await,
                OneWireCommands::Read { len } => self.onewire_read(&pg, *len).await,
                OneWireCommands::Write { data } => self.onewire_write(&pg, data).await,
                OneWireCommands::WritePullup { data, duration } => {
                    self.onewire_write_pullup(&pg, data, *duration).await
                }
                OneWireCommands::Search => self.onewire_search(&pg).await,
            },
        }
    }

    fn list_devices() -> Result<()> {
        let devices = list_devices();
        if devices.is_empty() {
            println!("No Pico de Gallo devices found.");
            return Ok(());
        }

        for dev in &devices {
            let product = dev.product.as_deref().unwrap_or("(unknown product)");
            let serial = dev.serial_number.as_deref().unwrap_or("(unknown)");
            println!(" - {product} - {serial}");
        }
        Ok(())
    }

    /// Print device information, falling back to the legacy `version`
    /// endpoint.
    ///
    /// The `device/info` call is bounded by [`VERSION_DEVICE_INFO_TIMEOUT`]
    /// rather than left unbounded. `PicoDeGallo::device_info` carries no
    /// timeout of its own — only `validate()` applies
    /// `DEVICE_INFO_TIMEOUT` — so without a bound here the `Err` arm below
    /// is unreachable and `gallo version` hangs forever whenever the reply
    /// never arrives. That happens for a build mismatch: postcard-rpc keys
    /// endpoints by response-type schema, so a firmware built from a
    /// different tree answers under a different key and the frame is
    /// dropped unmatched. The legacy `version` endpoint is unaffected in
    /// that case (`VersionInfo`'s schema, and therefore its key, has not
    /// changed), so the fallback genuinely recovers.
    async fn version(&self, pg: &PicoDeGallo) -> Result<()> {
        // Try the new device/info endpoint first; fall back to legacy version.
        match tokio::time::timeout(VERSION_DEVICE_INFO_TIMEOUT, pg.device_info()).await {
            Ok(Ok(info)) => {
                println!("{}", render_device_info(&info));
                Ok(())
            }
            // Elapsed and Err are treated identically: in both cases
            // device/info gave us nothing usable, and the legacy endpoint is
            // the better answer.
            Ok(Err(_)) | Err(_) => {
                // Fall back to legacy version endpoint
                match pg.version().await {
                    Ok(version) => {
                        println!(
                            "Pico de Gallo FW v{}.{}.{}",
                            version.major, version.minor, version.patch
                        );
                        println!("(legacy firmware — no schema/hw/capabilities info)");
                        Ok(())
                    }
                    Err(_) => Err(eyre!("Failed to get version")),
                }
            }
        }
    }

    /// Round-trip a random nonce through the firmware's `ping` endpoint.
    ///
    /// This is the lowest-level liveness check `gallo` offers: it exercises
    /// USB enumeration, postcard-rpc framing, and the firmware dispatch loop
    /// without touching a peripheral. Like `version`, it deliberately runs
    /// without the up-front schema validation [`Cli::run`] applies to the
    /// other device subcommands, so a schema-skewed board still reports
    /// whether the transport itself works.
    ///
    /// The payload is randomised so that a stale, duplicated, or
    /// default-initialised response cannot pass as a healthy round trip.
    async fn ping(&self, pg: &PicoDeGallo) -> Result<()> {
        let sent = rand::random::<u32>();
        let echoed = pg
            .ping(sent)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("ping failed"))?;

        check_ping_echo(sent, echoed)?;

        println!("Ping OK");
        Ok(())
    }

    async fn i2c_scan(&self, pg: &PicoDeGallo, reserved: bool) -> Result<()> {
        let addresses = pg
            .i2c_scan(reserved)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("i2c_scan failed"))?;

        let mut builder = Builder::with_capacity(17, 8);
        builder.push_record(
            (0..=16)
                .map(|i| if i == 0 { String::new() } else { format!("{:x}", i - 1) })
                .collect::<Vec<_>>(),
        );

        for hi in 0u8..=7 {
            let mut row = vec![format!("{:x} ", hi)];

            for lo in 0u8..=15 {
                let address = hi << 4 | lo;
                let stat = match address {
                    0x00..=0x07 | 0x78..=0x7f if !reserved => "RR".to_string(),
                    _ => {
                        if addresses.contains(&address) {
                            format!("{:02x}", address)
                        } else {
                            "--".to_string()
                        }
                    }
                };

                row.push(stat);
            }

            builder.push_record(row);
        }

        let mut table = builder.build();
        table.modify(Rows::first(), Alignment::right());
        table.with(Style::rounded());

        println!("{}", table);

        Ok(())
    }

    async fn i2c_read(&self, pg: &PicoDeGallo, address: &u8, count: &usize) -> Result<()> {
        let buf = match pg.i2c_read(*address, *count as u16).await {
            Ok(data) => data,
            Err(e) => return Err(eyre!("{:?}", e).wrap_err("i2c_read failed")),
        };

        print_data(&buf, &self.format);

        Ok(())
    }

    async fn i2c_write(&self, pg: &PicoDeGallo, address: &u8, bytes: &[u8]) -> Result<()> {
        pg.i2c_write(*address, bytes)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("i2c_write failed"))
    }

    async fn i2c_write_then_read(&self, pg: &PicoDeGallo, address: &u8, bytes: &[u8], count: &usize) -> Result<()> {
        let buf = match pg.i2c_write_read(*address, bytes, *count as u16).await {
            Ok(data) => data,
            Err(e) => return Err(eyre!("{:?}", e).wrap_err("i2c_write_read failed")),
        };

        print_data(&buf, &self.format);

        Ok(())
    }

    async fn spi_read(&self, pg: &PicoDeGallo, count: &usize) -> Result<()> {
        let buf = match pg.spi_read(*count as u16).await {
            Ok(data) => data,
            Err(e) => return Err(eyre!("{:?}", e).wrap_err("spi_read failed")),
        };

        print_data(&buf, &self.format);

        Ok(())
    }

    async fn spi_write(&self, pg: &PicoDeGallo, bytes: &[u8]) -> Result<()> {
        pg.spi_write(bytes)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("spi_write failed"))
    }

    async fn spi_transfer(&self, pg: &PicoDeGallo, bytes: &[u8]) -> Result<()> {
        let buf = pg
            .spi_transfer(bytes)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("spi_transfer failed"))?;

        print_data(&buf, &self.format);

        Ok(())
    }

    async fn spi_write_then_read(&self, pg: &PicoDeGallo, bytes: &[u8], count: &usize) -> Result<()> {
        self.spi_write(pg, bytes).await?;
        self.spi_read(pg, count).await
    }

    async fn i2c_set_config(&self, pg: &PicoDeGallo, frequency: I2cFrequency) -> Result<()> {
        pg.i2c_set_config(frequency)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("i2c set-config failed"))
    }

    async fn i2c_get_config(&self, pg: &PicoDeGallo) -> Result<()> {
        let freq = pg
            .i2c_get_config()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("i2c get-config failed"))?;

        let label = match freq {
            I2cFrequency::Standard => "Standard (100 kHz)",
            I2cFrequency::Fast => "Fast (400 kHz)",
            I2cFrequency::FastPlus => "Fast+ (1 MHz)",
        };
        println!("I2C frequency: {label}");
        Ok(())
    }

    async fn spi_set_config(&self, pg: &PicoDeGallo, frequency: u32, mode: u8) -> Result<()> {
        let (spi_phase, spi_polarity) = spi_mode(mode)?;

        pg.spi_set_config(frequency, spi_phase, spi_polarity)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("spi set-config failed"))
    }

    async fn spi_get_config(&self, pg: &PicoDeGallo) -> Result<()> {
        let info = pg
            .spi_get_config()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("spi get-config failed"))?;

        let phase = match info.spi_phase {
            SpiPhase::CaptureOnFirstTransition => "CaptureOnFirstTransition (CPHA=0)",
            SpiPhase::CaptureOnSecondTransition => "CaptureOnSecondTransition (CPHA=1)",
        };
        let polarity = match info.spi_polarity {
            SpiPolarity::IdleLow => "IdleLow (CPOL=0)",
            SpiPolarity::IdleHigh => "IdleHigh (CPOL=1)",
        };
        println!("SPI frequency: {} Hz", info.spi_frequency);
        println!("SPI phase:     {phase}");
        println!("SPI polarity:  {polarity}");
        Ok(())
    }

    async fn i2c_batch(&self, pg: &PicoDeGallo, address: u8, ops: &[String]) -> Result<()> {
        use pico_de_gallo_lib::I2cBatchOp;

        let batch_ops = parse_i2c_batch_ops(ops)?;
        let refs: Vec<I2cBatchOp<'_>> = batch_ops
            .iter()
            .map(|(kind, data)| match kind {
                I2cBatchKind::Read(len) => I2cBatchOp::Read { len: *len },
                I2cBatchKind::Write => I2cBatchOp::Write { data },
            })
            .collect();

        let result = pg
            .i2c_batch(address, &refs)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("i2c batch failed"))?;

        if result.is_empty() {
            println!("Batch complete (no read data)");
        } else {
            println!("Read data ({} bytes):", result.len());
            print_hex_dump(&result);
        }
        Ok(())
    }

    /// Execute an SPI batch.
    ///
    /// `num_gpios` is the count the connected device reported during the
    /// up-front validation in [`Cli::run`]; the chip-select is classified
    /// against it *before* any operation string is parsed, so a bad
    /// chip-select is reported as such rather than as a parse error, and
    /// nothing is transmitted.
    async fn spi_batch(&self, pg: &PicoDeGallo, cs: u8, ops: &[String], num_gpios: u8) -> Result<()> {
        use pico_de_gallo_lib::SpiBatchOp;

        classify_cs(cs, num_gpios)?;

        let batch_ops = parse_spi_batch_ops(ops)?;

        let refs: Vec<SpiBatchOp<'_>> = batch_ops
            .iter()
            .map(|(kind, data)| match kind {
                SpiBatchKind::Read(len) => SpiBatchOp::Read { len: *len },
                SpiBatchKind::Write => SpiBatchOp::Write { data },
                SpiBatchKind::Transfer => SpiBatchOp::Transfer { data },
                SpiBatchKind::DelayNs(ns) => SpiBatchOp::DelayNs { ns: *ns },
            })
            .collect();

        let result = pg
            .spi_batch(cs, &refs)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("spi batch failed"))?;

        if result.is_empty() {
            println!("Batch complete (no read data)");
        } else {
            println!("Read data ({} bytes):", result.len());
            print_hex_dump(&result);
        }
        Ok(())
    }

    async fn gpio_get(&self, pg: &PicoDeGallo, pin: u8) -> Result<()> {
        let level = pg
            .gpio_get(pin)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("gpio get failed"))?;

        let label = match level {
            GpioState::High => "HIGH",
            GpioState::Low => "LOW",
        };
        println!("GPIO pin {pin}: {label}");
        Ok(())
    }

    async fn gpio_put(&self, pg: &PicoDeGallo, pin: u8, level: GpioLevelArg) -> Result<()> {
        let state: GpioState = level.into();
        pg.gpio_put(pin, state)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("gpio put failed"))?;

        println!(
            "GPIO pin {pin} set to {}",
            match level {
                GpioLevelArg::High => "HIGH",
                GpioLevelArg::Low => "LOW",
            }
        );
        Ok(())
    }

    async fn gpio_set_config(
        &self,
        pg: &PicoDeGallo,
        pin: u8,
        direction: GpioDirectionArg,
        pull: GpioPullArg,
    ) -> Result<()> {
        pg.gpio_set_config(pin, direction.into(), pull.into())
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("gpio set-config failed"))?;

        println!("GPIO pin {pin} configured as {direction:?} with pull {pull:?}");
        Ok(())
    }

    async fn gpio_monitor(&self, pg: &PicoDeGallo, pin: u8, edge: GpioEdgeArg) -> Result<()> {
        pg.gpio_subscribe(pin, edge.into())
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("gpio subscribe failed"))?;

        println!("Monitoring GPIO pin {pin} for {edge:?} edges (Ctrl+C to stop)...");

        let mut sub = pg
            .subscribe_gpio_events(4)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("failed to open event subscription"))?;

        let result = loop {
            tokio::select! {
                event = sub.recv() => {
                    match event {
                        Ok(event) => {
                            println!(
                                "[{:>12} µs] pin={} edge={:?}",
                                event.timestamp_us, event.pin, event.edge,
                            );
                        }
                        Err(_) => {
                            break Err(eyre!("event subscription closed"));
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    break Ok(());
                }
            }
        };

        pg.gpio_unsubscribe(pin)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("gpio unsubscribe failed"))?;

        println!("Stopped monitoring GPIO pin {pin}");
        result
    }

    async fn uart_read(&self, pg: &PicoDeGallo, count: u16, timeout_ms: u32) -> Result<()> {
        let data = pg
            .uart_read(count, timeout_ms)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("uart read failed"))?;

        if data.is_empty() {
            println!("(no data received within timeout)");
        } else {
            print_data(&data, &self.format);
        }
        Ok(())
    }

    async fn uart_write(&self, pg: &PicoDeGallo, bytes: &[u8]) -> Result<()> {
        pg.uart_write(bytes)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("uart write failed"))?;

        println!("Wrote {} byte(s)", bytes.len());
        Ok(())
    }

    async fn uart_flush(&self, pg: &PicoDeGallo) -> Result<()> {
        pg.uart_flush()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("uart flush failed"))?;

        println!("UART TX buffer flushed");
        Ok(())
    }

    async fn uart_set_config(&self, pg: &PicoDeGallo, baud_rate: u32) -> Result<()> {
        pg.uart_set_config(baud_rate)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("uart set-config failed"))?;

        println!("UART baud rate set to {baud_rate}");
        Ok(())
    }

    async fn uart_get_config(&self, pg: &PicoDeGallo) -> Result<()> {
        let info = pg
            .uart_get_config()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("uart get-config failed"))?;

        println!("UART baud rate: {} bps", info.baud_rate);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // PWM
    // -----------------------------------------------------------------------

    async fn pwm_set_duty(&self, pg: &PicoDeGallo, channel: u8, duty: u16) -> Result<()> {
        pg.pwm_set_duty_cycle(channel, duty)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("pwm set-duty failed"))?;
        println!("PWM channel {channel}: duty set to {duty}");
        Ok(())
    }

    async fn pwm_get_duty(&self, pg: &PicoDeGallo, channel: u8) -> Result<()> {
        let info = pg
            .pwm_get_duty_cycle(channel)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("pwm get-duty failed"))?;
        println!(
            "PWM channel {channel}: duty={} / max={}",
            info.current_duty, info.max_duty
        );
        Ok(())
    }

    async fn pwm_enable(&self, pg: &PicoDeGallo, channel: u8) -> Result<()> {
        pg.pwm_enable(channel)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("pwm enable failed"))?;
        println!("PWM channel {channel}: slice enabled");
        Ok(())
    }

    async fn pwm_disable(&self, pg: &PicoDeGallo, channel: u8) -> Result<()> {
        pg.pwm_disable(channel)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("pwm disable failed"))?;
        println!("PWM channel {channel}: slice disabled");
        Ok(())
    }

    async fn pwm_set_config(
        &self,
        pg: &PicoDeGallo,
        channel: u8,
        frequency_hz: u32,
        phase_correct: bool,
    ) -> Result<()> {
        pg.pwm_set_config(channel, frequency_hz, phase_correct)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("pwm set-config failed"))?;
        println!("PWM channel {channel}: frequency={frequency_hz} Hz, phase_correct={phase_correct}");
        Ok(())
    }

    async fn pwm_get_config(&self, pg: &PicoDeGallo, channel: u8) -> Result<()> {
        let info = pg
            .pwm_get_config(channel)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("pwm get-config failed"))?;
        println!(
            "PWM channel {channel}: frequency={} Hz, phase_correct={}, enabled={}",
            info.frequency_hz, info.phase_correct, info.enabled
        );
        Ok(())
    }

    async fn adc_read(&self, pg: &PicoDeGallo, channel: u8) -> Result<()> {
        let adc_channel = match channel {
            0 => AdcChannel::Adc0,
            1 => AdcChannel::Adc1,
            2 => AdcChannel::Adc2,
            3 => AdcChannel::Adc3,
            _ => return Err(eyre!("invalid ADC channel {channel}: expected 0–3")),
        };
        let raw = pg
            .adc_read(adc_channel)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("adc read failed"))?;
        let voltage_mv = (raw as u32) * 3300 / 4096;
        println!("ADC channel {channel} ({adc_channel}): raw={raw}, ~{voltage_mv} mV");
        Ok(())
    }

    async fn adc_get_info(&self, pg: &PicoDeGallo) -> Result<()> {
        let info = pg
            .adc_get_config()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("adc get-config failed"))?;
        println!("ADC configuration:");
        println!("  Resolution:       {} bits", info.resolution_bits);
        println!("  Nominal ref:      {} mV", info.nominal_reference_mv);
        println!("  GPIO channels:    {}", info.num_gpio_channels);
        Ok(())
    }

    // ---- 1-Wire methods ----

    async fn onewire_reset(&self, pg: &PicoDeGallo) -> Result<()> {
        let present = pg
            .onewire_reset()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("1-Wire reset failed"))?;
        if present {
            println!("Device(s) present on the 1-Wire bus");
        } else {
            println!("No device detected on the 1-Wire bus");
        }
        Ok(())
    }

    async fn onewire_read(&self, pg: &PicoDeGallo, len: u16) -> Result<()> {
        let data = pg
            .onewire_read(len)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("1-Wire read failed"))?;
        print_data(&data, &self.format);
        Ok(())
    }

    async fn onewire_write(&self, pg: &PicoDeGallo, data: &[u8]) -> Result<()> {
        pg.onewire_write(data)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("1-Wire write failed"))?;
        println!("Wrote {} byte(s)", data.len());
        Ok(())
    }

    async fn onewire_write_pullup(&self, pg: &PicoDeGallo, data: &[u8], duration_ms: u16) -> Result<()> {
        pg.onewire_write_pullup(data, duration_ms)
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("1-Wire write-pullup failed"))?;
        println!("Wrote {} byte(s) with {}ms strong pullup", data.len(), duration_ms);
        Ok(())
    }

    async fn onewire_search(&self, pg: &PicoDeGallo) -> Result<()> {
        let mut rom_ids = Vec::new();

        // First search
        match pg
            .onewire_search()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("1-Wire search failed"))?
        {
            Some(id) => rom_ids.push(id),
            None => {
                println!("No devices found on the 1-Wire bus");
                return Ok(());
            }
        }

        // Continue searching
        while let Some(id) = pg
            .onewire_search_next()
            .await
            .map_err(|e| eyre!("{:?}", e).wrap_err("1-Wire search-next failed"))?
        {
            rom_ids.push(id);
        }

        println!("Found {} device(s):", rom_ids.len());
        for (i, id) in rom_ids.iter().enumerate() {
            let family = (*id & 0xFF) as u8;
            println!("  {}: ROM ID 0x{:016X} (family 0x{:02X})", i + 1, id, family);
        }
        Ok(())
    }
}

/// Decode an SPI mode number into the `(phase, polarity)` pair the wire
/// protocol carries.
///
/// SPI mode is the conventional encoding of the `(CPOL, CPHA)` tuple, with
/// CPOL in bit 1 and CPHA in bit 0:
///
/// | Mode | CPOL | CPHA | Idle clock | Sample edge |
/// |------|------|------|------------|-------------|
/// | 0    | 0    | 0    | low        | first       |
/// | 1    | 0    | 1    | low        | second      |
/// | 2    | 1    | 0    | high       | first       |
/// | 3    | 1    | 1    | high       | second      |
///
/// Returns an error rather than masking to `mode & 0b11`, so an
/// out-of-range value can never be silently reinterpreted as a valid but
/// different bus configuration. The CLI's own range validator makes that
/// path unreachable in practice; the check exists so the function is total
/// without a panic.
fn spi_mode(mode: u8) -> Result<(SpiPhase, SpiPolarity)> {
    match mode {
        0 => Ok((SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleLow)),
        1 => Ok((SpiPhase::CaptureOnSecondTransition, SpiPolarity::IdleLow)),
        2 => Ok((SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleHigh)),
        3 => Ok((SpiPhase::CaptureOnSecondTransition, SpiPolarity::IdleHigh)),
        _ => Err(eyre!("invalid SPI mode {mode}: expected 0–3")),
    }
}

/// Compare a `ping` echo against the payload that was sent.
///
/// Split out of [`Cli::ping`] so the comparison policy is unit-testable
/// without a board attached, mirroring the `check_schema_compatible` split
/// in `pico-de-gallo-lib`.
///
/// A mismatch is a protocol-integrity failure rather than a transport
/// failure — the round trip completed, but the firmware answered with the
/// wrong bytes — so it is reported separately from `CommsFailed` and names
/// both values, which are the only evidence available for diagnosing a
/// framing or dispatch fault.
fn check_ping_echo(sent: u32, echoed: u32) -> Result<()> {
    if echoed != sent {
        return Err(eyre!("ping echo mismatch: sent 0x{sent:08x}, received 0x{echoed:08x}"));
    }
    Ok(())
}

fn parse_byte(s: &str) -> Result<u8, ParseIntError> {
    if let Some(hex) = s.strip_prefix("0x") {
        u8::from_str_radix(hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b") {
        u8::from_str_radix(bin, 2)
    } else {
        s.parse::<u8>()
    }
}

/// Parse an I2C 7-bit address (0x00–0x7F).
fn parse_i2c_address(s: &str) -> Result<u8, String> {
    let byte = parse_byte(s).map_err(|e| e.to_string())?;
    if byte > 0x7F {
        return Err(format!("I2C address {s} exceeds 7-bit range (max 0x7F)"));
    }
    Ok(byte)
}

/// Parse a hex string (e.g., "cc44" or "0xCC44") into a Vec<u8>.
fn parse_hex_string(s: &str) -> Result<Vec<u8>, String> {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if !s.len().is_multiple_of(2) {
        return Err(format!("hex string must have even length, got {}", s.len()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("invalid hex at position {i}: {e}")))
        .collect()
}

/// Parse a comma-separated list of bytes, supporting hex and decimal.
fn parse_byte_list(s: &str) -> Result<Vec<u8>> {
    s.split(',')
        .map(|b| {
            let b = b.trim();
            parse_byte(b).map_err(|e| eyre!("invalid byte '{b}': {e}"))
        })
        .collect()
}

/// Intermediate I2C batch op representation (owns data).
enum I2cBatchKind {
    Read(u16),
    Write,
}

/// Intermediate SPI batch op representation (owns data).
enum SpiBatchKind {
    Read(u16),
    Write,
    Transfer,
    DelayNs(u32),
}

/// Parse I2C batch operation strings into owned intermediate values.
///
/// Format: `read:N` or `write:B1,B2,...`
fn parse_i2c_batch_ops(ops: &[String]) -> Result<Vec<(I2cBatchKind, Vec<u8>)>> {
    ops.iter()
        .map(|op| {
            if let Some(n) = op.strip_prefix("read:") {
                let len: u16 = n.trim().parse().map_err(|e| eyre!("invalid read length '{n}': {e}"))?;
                Ok((I2cBatchKind::Read(len), Vec::new()))
            } else if let Some(data) = op.strip_prefix("write:") {
                let bytes = parse_byte_list(data)?;
                Ok((I2cBatchKind::Write, bytes))
            } else {
                Err(eyre!("unknown I2C batch op '{op}'. Use 'read:N' or 'write:B1,B2,...'"))
            }
        })
        .collect()
}

/// Parse SPI batch operation strings into owned intermediate values.
///
/// Format: `read:N`, `write:B1,B2,...`, `transfer:B1,B2,...`, or `delay:NS`
/// Classify an SPI chip-select index against the device-reported GPIO count.
///
/// `num_gpios` comes from the up-front `device/info` validation in
/// [`Cli::run`], so it is always a value the device actually reported: a
/// validation failure aborts before any handler runs and is surfaced as a
/// validation error, never as an invalid chip-select (issue #104).
///
/// A count of zero is its own message for every index, so a board with no
/// GPIOs is diagnosable as exactly that.
fn classify_cs(cs: u8, num_gpios: u8) -> Result<()> {
    if num_gpios == 0 {
        return Err(eyre!("device reports num_gpios=0; no SPI chip-select pin is available"));
    }
    if cs >= num_gpios {
        return Err(eyre!(
            "invalid SPI chip-select pin {cs}; device reports {num_gpios} GPIOs (valid 0..{num_gpios})"
        ));
    }
    Ok(())
}

/// Render the user-facing message for a failed firmware validation.
///
/// Extracted from [`Cli::validate_firmware`] so the exact text — including
/// the 300-second [`ValidateError::Timeout`] diagnostic — is testable
/// without a device.
///
/// A [`ValidateError::Timeout`] gets extra text. It is not necessarily an
/// unresponsive board: postcard-rpc derives each endpoint key from the
/// response type's schema, so a host and firmware built from different
/// trees exchange `device/info` under different keys and the reply is
/// dropped as unmatched rather than decoded. Retrying or replugging cannot
/// help with that, because the mismatch is fixed at compile time. The host
/// cannot distinguish the two causes, so the message names both.
fn validation_failure_message(e: &ValidateError) -> String {
    let mut msg = format!(
        "firmware validation failed: {e}\n\n\
         Re-flash the firmware to a version matching this `gallo` build, \
         or install a `gallo` build matching the firmware. \
         Run `gallo version` for the current device-reported schema."
    );
    if matches!(e, ValidateError::Timeout) {
        msg.push_str(
            "\n\nA timeout here does not prove the board is unresponsive. \
             If host and firmware were built from different trees, the \
             `device/info` reply carries a different endpoint key and is \
             dropped without ever being decoded, which looks identical to \
             silence. Retrying or replugging will not fix that; rebuild \
             both sides from the same tree. `gallo version` falls back to \
             the legacy `version` endpoint after a short timeout, so it \
             still reports the firmware version in that case.",
        );
    }
    msg
}

fn parse_spi_batch_ops(ops: &[String]) -> Result<Vec<(SpiBatchKind, Vec<u8>)>> {
    ops.iter()
        .map(|op| {
            if let Some(n) = op.strip_prefix("read:") {
                let len: u16 = n.trim().parse().map_err(|e| eyre!("invalid read length '{n}': {e}"))?;
                Ok((SpiBatchKind::Read(len), Vec::new()))
            } else if let Some(data) = op.strip_prefix("write:") {
                let bytes = parse_byte_list(data)?;
                Ok((SpiBatchKind::Write, bytes))
            } else if let Some(data) = op.strip_prefix("transfer:") {
                let bytes = parse_byte_list(data)?;
                Ok((SpiBatchKind::Transfer, bytes))
            } else if let Some(ns) = op.strip_prefix("delay:") {
                let nanos: u32 = ns
                    .trim()
                    .parse()
                    .map_err(|e| eyre!("invalid delay nanoseconds '{ns}': {e}"))?;
                Ok((SpiBatchKind::DelayNs(nanos), Vec::new()))
            } else {
                Err(eyre!(
                    "unknown SPI batch op '{op}'. Use 'read:N', 'write:B1,B2,...', 'transfer:B1,B2,...', or 'delay:NS'"
                ))
            }
        })
        .collect()
}

/// Print a hex dump of data in the common `offset: hex  ascii` format.
fn print_hex_dump(data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        let offset = i * 16;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  {offset:04x}: {:<48}  {ascii}", hex.join(" "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ----------------------------- parse_byte tests -----------------------------

    #[test]
    fn parse_byte_decimal() {
        assert_eq!(parse_byte("0").unwrap(), 0);
        assert_eq!(parse_byte("255").unwrap(), 255);
        assert_eq!(parse_byte("42").unwrap(), 42);
    }

    #[test]
    fn parse_byte_hex() {
        assert_eq!(parse_byte("0x00").unwrap(), 0x00);
        assert_eq!(parse_byte("0xFF").unwrap(), 0xFF);
        assert_eq!(parse_byte("0x48").unwrap(), 0x48);
        assert_eq!(parse_byte("0xab").unwrap(), 0xAB);
    }

    #[test]
    fn parse_byte_binary() {
        assert_eq!(parse_byte("0b00000000").unwrap(), 0);
        assert_eq!(parse_byte("0b11111111").unwrap(), 255);
        assert_eq!(parse_byte("0b10101010").unwrap(), 0xAA);
    }

    #[test]
    fn parse_byte_overflow_fails() {
        assert!(parse_byte("256").is_err());
        assert!(parse_byte("0x100").is_err());
        assert!(parse_byte("0b100000000").is_err());
    }

    #[test]
    fn parse_byte_invalid_fails() {
        assert!(parse_byte("xyz").is_err());
        assert!(parse_byte("0xGG").is_err());
        assert!(parse_byte("0b2").is_err());
        assert!(parse_byte("").is_err());
    }

    // ----------------------------- CLI parsing tests -----------------------------

    #[test]
    fn cli_no_args_requires_help() {
        // arg_required_else_help = true means no-args should fail
        let result = Cli::try_parse_from(["gallo"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_version_subcommand() {
        let cli = Cli::try_parse_from(["gallo", "version"]).unwrap();
        assert!(matches!(cli.command, Commands::Version));
        assert!(cli.serial_number.is_none());
    }

    #[test]
    fn cli_list_subcommand() {
        let cli = Cli::try_parse_from(["gallo", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::List));
    }

    #[test]
    fn cli_version_with_serial() {
        let cli = Cli::try_parse_from(["gallo", "-s", "ABCD1234", "version"]).unwrap();
        assert_eq!(cli.serial_number.as_deref(), Some("ABCD1234"));
        assert!(matches!(cli.command, Commands::Version));
    }

    #[test]
    fn cli_i2c_read() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "read", "-a", "0x48", "-c", "4"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::Read { address, count },
            } => {
                assert_eq!(address, 0x48);
                assert_eq!(count, 4);
            }
            _ => panic!("expected I2c Read command"),
        }
    }

    #[test]
    fn cli_i2c_write() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "write", "-a", "0x50", "-b", "0xDE", "0xAD"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::Write { address, bytes },
            } => {
                assert_eq!(address, 0x50);
                assert_eq!(bytes, vec![0xDE, 0xAD]);
            }
            _ => panic!("expected I2c Write command"),
        }
    }

    #[test]
    fn cli_i2c_write_read() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "write-read", "-a", "0x68", "-b", "0x01", "-c", "6"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::WriteRead { address, bytes, count },
            } => {
                assert_eq!(address, 0x68);
                assert_eq!(bytes, vec![0x01]);
                assert_eq!(count, 6);
            }
            _ => panic!("expected I2c WriteRead command"),
        }
    }

    #[test]
    fn cli_i2c_scan() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "scan"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::Scan { reserved },
            } => {
                assert!(!reserved);
            }
            _ => panic!("expected I2c Scan command"),
        }
    }

    #[test]
    fn cli_i2c_scan_reserved() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "scan", "-r"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::Scan { reserved },
            } => {
                assert!(reserved);
            }
            _ => panic!("expected I2c Scan command"),
        }
    }

    #[test]
    fn cli_spi_read() {
        let cli = Cli::try_parse_from(["gallo", "spi", "read", "-c", "16"]).unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::Read { count },
            } => {
                assert_eq!(count, 16);
            }
            _ => panic!("expected Spi Read command"),
        }
    }

    #[test]
    fn cli_spi_write() {
        let cli = Cli::try_parse_from(["gallo", "spi", "write", "-b", "0xCA", "0xFE"]).unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::Write { bytes },
            } => {
                assert_eq!(bytes, vec![0xCA, 0xFE]);
            }
            _ => panic!("expected Spi Write command"),
        }
    }

    #[test]
    fn cli_spi_transfer() {
        let cli = Cli::try_parse_from(["gallo", "spi", "transfer", "-b", "0x01", "0x02", "0x03"]).unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::Transfer { bytes },
            } => {
                assert_eq!(bytes, vec![0x01, 0x02, 0x03]);
            }
            _ => panic!("expected Spi Transfer command"),
        }
    }

    #[test]
    fn cli_i2c_set_config() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "set-config", "--frequency", "fast"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::SetConfig { frequency },
            } => {
                assert_eq!(frequency, I2cFrequencyArg::Fast);
            }
            _ => panic!("expected I2c SetConfig command"),
        }
    }

    #[test]
    fn cli_i2c_set_config_fast_plus() {
        let cli = Cli::try_parse_from(["gallo", "i2c", "set-config", "--frequency", "fast-plus"]).unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::SetConfig { frequency },
            } => {
                assert_eq!(frequency, I2cFrequencyArg::FastPlus);
            }
            _ => panic!("expected I2c SetConfig command"),
        }
    }

    #[test]
    fn cli_i2c_set_config_invalid_frequency_fails() {
        let result = Cli::try_parse_from(["gallo", "i2c", "set-config", "--frequency", "ultra-fast"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_spi_set_config() {
        let cli = Cli::try_parse_from(["gallo", "spi", "set-config", "--frequency", "1000000", "--mode", "3"]).unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::SetConfig { frequency, mode },
            } => {
                assert_eq!(frequency, 1_000_000);
                assert_eq!(mode, 3);
            }
            _ => panic!("expected Spi SetConfig command"),
        }
    }

    #[test]
    fn cli_i2c_set_config_missing_frequency_fails() {
        let result = Cli::try_parse_from(["gallo", "i2c", "set-config"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_spi_set_config_missing_frequency_fails() {
        let result = Cli::try_parse_from(["gallo", "spi", "set-config"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_i2c_read_missing_address_fails() {
        let result = Cli::try_parse_from(["gallo", "i2c", "read", "-c", "4"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["gallo", "uart"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_i2c_without_subcommand_fails() {
        let result = Cli::try_parse_from(["gallo", "i2c"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_spi_without_subcommand_fails() {
        let result = Cli::try_parse_from(["gallo", "spi"]);
        assert!(result.is_err());
    }

    // ----------------------------- batch CLI tests -----------------------------

    #[test]
    fn cli_i2c_batch() {
        let cli = Cli::try_parse_from([
            "gallo",
            "i2c",
            "batch",
            "-a",
            "0x50",
            "--op",
            "write:0x00,0x10",
            "--op",
            "read:16",
        ])
        .unwrap();
        match cli.command {
            Commands::I2c {
                command: I2cCommands::Batch { address, op },
            } => {
                assert_eq!(address, 0x50);
                assert_eq!(op, vec!["write:0x00,0x10", "read:16"]);
            }
            _ => panic!("expected I2c Batch command"),
        }
    }

    #[test]
    fn cli_i2c_batch_requires_ops() {
        let result = Cli::try_parse_from(["gallo", "i2c", "batch", "-a", "0x50"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_spi_batch() {
        let cli = Cli::try_parse_from([
            "gallo",
            "spi",
            "batch",
            "--cs",
            "0",
            "--op",
            "write:0x9F",
            "--op",
            "read:3",
        ])
        .unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::Batch { cs, op },
            } => {
                assert_eq!(cs, 0);
                assert_eq!(op, vec!["write:0x9F", "read:3"]);
            }
            _ => panic!("expected Spi Batch command"),
        }
    }

    #[test]
    fn cli_spi_batch_with_transfer_and_delay() {
        let cli = Cli::try_parse_from([
            "gallo",
            "spi",
            "batch",
            "--cs",
            "1",
            "--op",
            "transfer:0x01,0x02",
            "--op",
            "delay:1000",
        ])
        .unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::Batch { cs, op },
            } => {
                assert_eq!(cs, 1);
                assert_eq!(op, vec!["transfer:0x01,0x02", "delay:1000"]);
            }
            _ => panic!("expected Spi Batch command"),
        }
    }

    #[test]
    fn cli_spi_batch_requires_ops() {
        let result = Cli::try_parse_from(["gallo", "spi", "batch", "--cs", "0"]);
        assert!(result.is_err());
    }

    // ----------------------------- batch op parser tests -----------------------------

    #[test]
    fn parse_i2c_batch_ops_read() {
        let ops = vec!["read:16".to_string()];
        let parsed = parse_i2c_batch_ops(&ops).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(matches!(parsed[0].0, I2cBatchKind::Read(16)));
    }

    #[test]
    fn parse_i2c_batch_ops_write() {
        let ops = vec!["write:0xDE,0xAD".to_string()];
        let parsed = parse_i2c_batch_ops(&ops).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(matches!(parsed[0].0, I2cBatchKind::Write));
        assert_eq!(parsed[0].1, vec![0xDE, 0xAD]);
    }

    #[test]
    fn parse_i2c_batch_ops_mixed() {
        let ops = vec!["write:0x00,0x10".to_string(), "read:32".to_string()];
        let parsed = parse_i2c_batch_ops(&ops).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn parse_i2c_batch_ops_invalid() {
        let ops = vec!["transfer:0x01".to_string()];
        assert!(parse_i2c_batch_ops(&ops).is_err());
    }

    #[test]
    fn parse_spi_batch_ops_all_types() {
        let ops = vec![
            "read:4".to_string(),
            "write:0x9F".to_string(),
            "transfer:0x01,0x02".to_string(),
            "delay:1000".to_string(),
        ];
        let parsed = parse_spi_batch_ops(&ops).unwrap();
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn parse_spi_batch_ops_invalid() {
        let ops = vec!["nope:1".to_string()];
        assert!(parse_spi_batch_ops(&ops).is_err());
    }

    #[test]
    fn parse_byte_list_hex_and_decimal() {
        let result = parse_byte_list("0x0A,20,0xFF").unwrap();
        assert_eq!(result, vec![0x0A, 20, 0xFF]);
    }

    #[test]
    fn print_hex_dump_does_not_panic() {
        print_hex_dump(&[0x00, 0x41, 0x42, 0x7F, 0x80, 0xFF]);
    }

    // ----------------------------- ping tests -----------------------------

    #[test]
    fn cli_ping_subcommand() {
        let cli = Cli::try_parse_from(["gallo", "ping"]).unwrap();
        assert!(matches!(cli.command, Commands::Ping));
    }

    #[test]
    fn cli_ping_with_serial() {
        let cli = Cli::try_parse_from(["gallo", "-s", "E6633861A34B8C24", "ping"]).unwrap();
        assert_eq!(cli.serial_number.as_deref(), Some("E6633861A34B8C24"));
        assert!(matches!(cli.command, Commands::Ping));
    }

    #[test]
    fn cli_ping_rejects_arguments() {
        // The documented surface is a bare `gallo ping` (book/src/getting-started/verify.md).
        // Lock it so a payload flag cannot be added without also updating the book.
        assert!(Cli::try_parse_from(["gallo", "ping", "--id", "7"]).is_err());
        assert!(Cli::try_parse_from(["gallo", "ping", "7"]).is_err());
    }

    #[test]
    fn check_ping_echo_accepts_a_matching_echo() {
        assert!(check_ping_echo(0xDEAD_BEEF, 0xDEAD_BEEF).is_ok());
        assert!(check_ping_echo(0, 0).is_ok());
        assert!(check_ping_echo(u32::MAX, u32::MAX).is_ok());
    }

    #[test]
    fn check_ping_echo_rejects_a_mismatched_echo() {
        let err = check_ping_echo(0xDEAD_BEEF, 0x0BAD_F00D).unwrap_err();
        let msg = format!("{err}");
        // Both values must appear: the difference between them is the only
        // evidence available for diagnosing a framing or dispatch fault.
        assert!(msg.contains("deadbeef"), "sent value missing from {msg:?}");
        assert!(msg.contains("0badf00d"), "echoed value missing from {msg:?}");
    }

    #[test]
    fn check_ping_echo_rejects_a_zero_echo_of_a_nonzero_nonce() {
        // A firmware that answers with a default-initialised buffer is the
        // most likely real-world mismatch, so pin it explicitly.
        assert!(check_ping_echo(0x1234_5678, 0).is_err());
    }

    // ----------------------------- SPI mode tests -----------------------------

    #[test]
    fn spi_mode_decodes_the_four_standard_modes() {
        // (CPOL, CPHA) per the conventional numbering: CPOL is bit 1,
        // CPHA is bit 0.
        assert_eq!(
            spi_mode(0).unwrap(),
            (SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleLow)
        );
        assert_eq!(
            spi_mode(1).unwrap(),
            (SpiPhase::CaptureOnSecondTransition, SpiPolarity::IdleLow)
        );
        assert_eq!(
            spi_mode(2).unwrap(),
            (SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleHigh)
        );
        assert_eq!(
            spi_mode(3).unwrap(),
            (SpiPhase::CaptureOnSecondTransition, SpiPolarity::IdleHigh)
        );
    }

    #[test]
    fn spi_mode_rejects_out_of_range_values() {
        // Unreachable through clap's range validator, but a silent
        // truncation to `mode & 0b11` would be a nasty way to find out.
        assert!(spi_mode(4).is_err());
        assert!(spi_mode(u8::MAX).is_err());
    }

    #[test]
    fn cli_spi_set_config_defaults_to_mode_0() {
        // Regression test for the CLI defaulting to mode 3 while the
        // firmware booted in mode 0, so setting the clock silently
        // changed the mode. Asserts the resulting wire values, not just
        // the parsed flag: the previous test pinned the flags and so
        // never noticed the mode was wrong.
        let cli = Cli::try_parse_from(["gallo", "spi", "set-config", "--frequency", "500000"]).unwrap();
        match cli.command {
            Commands::Spi {
                command: SpiCommands::SetConfig { frequency, mode },
            } => {
                assert_eq!(frequency, 500_000);
                assert_eq!(mode, 0);
                assert_eq!(
                    spi_mode(mode).unwrap(),
                    (SpiPhase::CaptureOnFirstTransition, SpiPolarity::IdleLow),
                    "a bare set-config must select mode 0, matching the firmware default"
                );
            }
            _ => panic!("expected Spi SetConfig command"),
        }
    }

    // ===================================================================
    // M3 — SPI chip-select bounds (issue #104)
    // ===================================================================

    fn parsed_batch_cs(args: &[&str]) -> Option<u8> {
        match Cli::try_parse_from(args).ok()?.command {
            Commands::Spi {
                command: SpiCommands::Batch { cs, .. },
            } => Some(cs),
            _ => None,
        }
    }

    #[test]
    fn cli_spi_set_config_accepts_every_mode() {
        for m in 0u8..=3 {
            let cli = Cli::try_parse_from([
                "gallo",
                "spi",
                "set-config",
                "--frequency",
                "1000000",
                "--mode",
                &m.to_string(),
            ])
            .unwrap();
            match cli.command {
                Commands::Spi {
                    command: SpiCommands::SetConfig { mode, .. },
                } => assert_eq!(mode, m),
                _ => panic!("expected Spi SetConfig command"),
            }
        }
    }

    #[test]
    fn cli_spi_set_config_rejects_an_invalid_mode() {
        for bad in ["4", "255", "-1"] {
            assert!(
                Cli::try_parse_from(["gallo", "spi", "set-config", "--frequency", "1000000", "--mode", bad]).is_err(),
                "--mode {bad} should be rejected"
            );
        }
    }

    #[test]
    fn cli_spi_batch_accepts_max_u8_cs_at_the_clap_layer() {
        // 255 must survive parsing so the *runtime* bound check reports it,
        // with the real device-reported count in the message.
        assert_eq!(
            parsed_batch_cs(&["gallo", "spi", "batch", "--cs", "255", "--op", "read:1"]),
            Some(255)
        );
    }

    #[test]
    fn cli_spi_batch_accepts_zero_cs_at_the_clap_layer() {
        assert_eq!(
            parsed_batch_cs(&["gallo", "spi", "batch", "--cs", "0", "--op", "read:1"]),
            Some(0)
        );
    }

    #[test]
    fn cli_spi_batch_rejects_cs_above_u8_at_the_clap_layer() {
        assert!(Cli::try_parse_from(["gallo", "spi", "batch", "--cs", "256", "--op", "read:1"]).is_err());
    }

    #[test]
    fn spi_batch_help_does_not_claim_a_fixed_zero_to_three_range() {
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("spi")
            .expect("spi subcommand")
            .find_subcommand_mut("batch")
            .expect("batch subcommand")
            .render_long_help()
            .to_string();
        assert!(!help.contains("0–3"), "stale fixed range in help:\n{help}");
        assert!(!help.contains("0-3"), "stale fixed range in help:\n{help}");
        assert!(
            help.to_lowercase().contains("gpio count"),
            "help must point at the device-reported GPIO count:\n{help}"
        );
    }

    #[test]
    fn classify_cs_out_of_range_message_is_exact() {
        let e = classify_cs(4, 4).expect_err("pin 4 is out of range at n = 4");
        assert_eq!(
            format!("{e}"),
            "invalid SPI chip-select pin 4; device reports 4 GPIOs (valid 0..4)"
        );
    }

    #[test]
    fn classify_cs_max_u8_message_reports_two_five_five() {
        // A `cs & 3` truncation would report pin 3 — and, worse, accept it.
        let e = classify_cs(255, 4).expect_err("pin 255 is out of range at n = 4");
        let msg = format!("{e}");
        assert!(msg.contains("255"), "got: {msg}");
        assert!(!msg.contains("pin 3"), "truncated the caller's index: {msg}");
    }

    #[test]
    fn classify_cs_zero_bound_message_is_exact() {
        for cs in [0u8, 255u8] {
            let e = classify_cs(cs, 0).expect_err("no pin is valid at n = 0");
            assert_eq!(
                format!("{e}"),
                "device reports num_gpios=0; no SPI chip-select pin is available"
            );
        }
    }

    #[test]
    fn classify_cs_boundaries_at_n_four_and_n_seven() {
        classify_cs(3, 4).expect("pin 3 is valid at n = 4");
        assert!(classify_cs(4, 4).is_err());
        classify_cs(6, 7).expect("pin 6 is valid at n = 7");
        assert!(classify_cs(7, 7).is_err());
    }

    #[test]
    fn validation_timeout_text_appears_under_firmware_validation_failed() {
        let msg = validation_failure_message(&ValidateError::Timeout);
        assert!(msg.contains("firmware validation failed"), "got: {msg}");
        assert!(msg.contains("device/info"), "got: {msg}");
        assert!(msg.contains("300"), "got: {msg}");
    }

    /// A `device/info` timeout is at least as likely to be a host/firmware
    /// build mismatch (different endpoint key, reply dropped unmatched) as
    /// an unresponsive board, and no retry can fix the former. The message
    /// must say so, and must not send the user back to a command that used
    /// to hang.
    #[test]
    fn validation_timeout_message_names_build_mismatch() {
        let msg = validation_failure_message(&ValidateError::Timeout);
        assert!(msg.contains("endpoint key"), "got: {msg}");
        assert!(msg.contains("same tree"), "got: {msg}");
        assert!(!msg.contains("replugging will fix"), "got: {msg}");
    }

    /// The build-mismatch paragraph is specific to `Timeout`; a schema
    /// mismatch is already self-explanatory and must not acquire it.
    #[test]
    fn non_timeout_validation_message_omits_build_mismatch_text() {
        let msg = validation_failure_message(&ValidateError::LegacyFirmware);
        assert!(!msg.contains("endpoint key"), "got: {msg}");
    }

    #[test]
    fn list_and_version_subcommands_still_parse_unchanged() {
        assert!(matches!(
            Cli::try_parse_from(["gallo", "list"]).unwrap().command,
            Commands::List
        ));
        assert!(matches!(
            Cli::try_parse_from(["gallo", "version"]).unwrap().command,
            Commands::Version
        ));
    }

    /// The bug this fixes: `#[arg(short, long)] high: bool` derived `-h`,
    /// which collides with clap's auto-generated `-h` for `--help`. That is
    /// a builder assertion, so it fired before any parsing and made
    /// `gallo gpio put` unusable. This guards every subcommand, not just it.
    #[test]
    fn cli_command_builder_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_gpio_put_level_high_parses() {
        let cli = Cli::try_parse_from(["gallo", "gpio", "put", "--pin", "2", "--level", "high"]).unwrap();
        match cli.command {
            Commands::Gpio {
                command: GpioCommands::Put { pin, level },
            } => {
                assert_eq!(pin, 2);
                assert_eq!(level, GpioLevelArg::High);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_gpio_put_level_low_parses() {
        let cli = Cli::try_parse_from(["gallo", "gpio", "put", "--pin", "2", "--level", "low"]).unwrap();
        match cli.command {
            Commands::Gpio {
                command: GpioCommands::Put { pin, level },
            } => {
                assert_eq!(pin, 2);
                assert_eq!(level, GpioLevelArg::Low);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_gpio_put_rejects_an_invalid_level() {
        assert!(Cli::try_parse_from(["gallo", "gpio", "put", "--pin", "2", "--level", "true"]).is_err());
    }

    #[test]
    fn cli_gpio_put_requires_level() {
        // `--level` has no default: omitting it is a parse error, never a
        // silent "drive it high".
        assert!(Cli::try_parse_from(["gallo", "gpio", "put", "--pin", "2"]).is_err());
    }

    #[test]
    fn cli_gpio_put_short_h_is_help_not_level() {
        let err = Cli::try_parse_from(["gallo", "gpio", "put", "-h"]).expect_err("-h prints help");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = err.to_string();
        assert!(help.contains("--level"), "help must list --level:\n{help}");
        assert!(
            help.contains("<HIGH|LOW>") || help.contains("high"),
            "help must list the choices:\n{help}"
        );
    }

    // -------------------------- render_device_info tests -------------------------

    fn sample_device_info() -> pico_de_gallo_lib::DeviceInfo {
        pico_de_gallo_lib::DeviceInfo {
            fw_major: 0,
            fw_minor: 11,
            fw_patch: 0,
            schema_major: 0,
            schema_minor: 7,
            schema_patch: 0,
            hw_version: 2,
            capabilities: pico_de_gallo_lib::Capabilities::I2C | pico_de_gallo_lib::Capabilities::SPI,
            num_gpios: 4,
            build_id: "firmware-v0.11.0-27-gdeadbee-dirty".try_into().unwrap(),
        }
    }

    /// Index of the rendered line holding `label`, for same-row assertions.
    ///
    /// Asserting a label and its value as unrelated substring searches proves
    /// almost nothing: both would still pass if the values were swapped
    /// between rows. Locating the row is what makes it a real pin.
    fn summary_row(out: &str, label: &str) -> (usize, String) {
        let (idx, line) = out
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .unwrap_or_else(|| panic!("no row for label {label:?}:\n{out}"));
        (idx, line.to_string())
    }

    fn assert_summary_row(out: &str, label: &str, value: &str) {
        let (_, line) = summary_row(out, label);
        assert!(
            line.contains(value),
            "row for {label:?} does not carry value {value:?}:\n{line}\n\nfull output:\n{out}"
        );
    }

    #[test]
    fn render_device_info_reports_every_field() {
        let out = render_device_info(&sample_device_info());
        // Label AND value, on the same rendered row, for every summary field.
        assert_summary_row(&out, "Firmware", "v0.11.0");
        assert_summary_row(&out, "Schema", "v0.7.0");
        assert_summary_row(&out, "HW revision", "2");
        assert_summary_row(&out, "GPIOs", "4");
        assert_summary_row(&out, "Build", "firmware-v0.11.0-27-gdeadbee-dirty");
        // The summary table is key/value, so it must have no internal rule:
        // `Builder` renders record 0 as a header unless horizontals are
        // removed, which would draw a rule under `Firmware` and imply it is a
        // column heading. Counting the left tee across both tables is the
        // robust pin: exactly one may appear, the capabilities table's own
        // genuine header rule. It fails if the summary rule returns (2) and
        // also if someone strips the capabilities header rule (0), without
        // hard-coding either table's full box-drawing layout.
        assert_eq!(
            out.matches('├').count(),
            1,
            "expected exactly one header rule (capabilities only):\n{out}"
        );
    }

    #[test]
    fn render_device_info_puts_build_last() {
        // `Build` is rendered last to mirror `build_id` being the last field
        // of the wire type. Comparing row indices pins the ordering without
        // hard-coding the whole table layout.
        let out = render_device_info(&sample_device_info());
        let (build, _) = summary_row(&out, "Build");
        for label in ["Firmware", "Schema", "HW revision", "GPIOs"] {
            let (other, _) = summary_row(&out, label);
            assert!(
                build > other,
                "`Build` (row {build}) must come after {label:?} (row {other}):\n{out}"
            );
        }
    }

    #[test]
    fn render_device_info_marks_capabilities() {
        let out = render_device_info(&sample_device_info());
        // Every capability column must be present and in the documented order.
        let names = ["I2C", "SPI", "UART", "GPIO", "PWM", "ADC", "1-Wire"];
        let (header_idx, header) = summary_row(&out, "1-Wire");
        for name in names {
            assert!(
                header.contains(name),
                "capability column {name:?} missing from header:\n{header}\n\nfull output:\n{out}"
            );
        }
        // The marks row is two lines below the header (header, rule, marks).
        // Assert its exact contents rather than merely that some tick exists:
        // the fixture sets I2C and SPI only, so dropping or miswiring any
        // column changes this sequence.
        let marks: Vec<&str> = out
            .lines()
            .nth(header_idx + 2)
            .unwrap_or_else(|| panic!("no marks row after header:\n{out}"))
            .split('│')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect();
        assert_eq!(
            marks,
            ["✓", "✓", "✗", "✗", "✗", "✗", "✗"],
            "capability marks do not match the fixture (I2C+SPI only):\n{out}"
        );
    }

    #[test]
    fn render_device_info_covers_every_known_capability() {
        // `Capabilities` is an extensible u64 newtype and `render_device_info`
        // keeps its own display list, so a bit added to the wire crate would
        // otherwise be silently missing from `gallo version` with nothing
        // failing. If this test breaks, a capability was added upstream: add
        // it to the table in `render_device_info` and extend this mask.
        use pico_de_gallo_lib::Capabilities;
        let rendered = Capabilities::I2C
            | Capabilities::SPI
            | Capabilities::UART
            | Capabilities::GPIO
            | Capabilities::PWM
            | Capabilities::ADC
            | Capabilities::ONEWIRE;
        assert_eq!(
            rendered.bits(),
            0x7F,
            "a capability bit was added to pico-de-gallo-internal but not to \
             the `gallo version` capability table"
        );
    }

    #[test]
    fn render_device_info_shows_dirty_marker_verbatim() {
        // The `-dirty` suffix is the most valuable part of the build id for a
        // bisecting developer, so make sure nothing strips or reformats it.
        let out = render_device_info(&sample_device_info());
        assert!(out.contains("-dirty"), "dirty marker lost:\n{out}");
    }
}
