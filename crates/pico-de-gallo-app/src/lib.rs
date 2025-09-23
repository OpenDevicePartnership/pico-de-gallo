use clap::{Parser, Subcommand};
use color_eyre::{Result, eyre::eyre};
use pico_de_gallo_lib::{PicoDeGallo, SpiPhase, SpiPolarity};
use std::num::ParseIntError;
use tabled::builder::Builder;
use tabled::settings::object::Rows;
use tabled::settings::{Alignment, Style};

#[derive(Parser, Debug)]
#[command(
    name = "Pico De Gallo",
    author = "Felipe Balbi <febalbi@microsoft.com>",
    about = "Access I2C/SPI devices through Pico De Gallo",
    arg_required_else_help = true,
    version
)]
pub struct Cli {
    #[arg(short, long)]
    serial_number: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Get firmware version
    Version,

    /// I2C access methods
    I2c {
        /// I2C commands
        #[command(subcommand)]
        command: Option<I2cCommands>,
    },

    /// SPI access methods
    Spi {
        /// SPI commands
        #[command(subcommand)]
        command: Option<SpiCommands>,
    },

    /// Set bus parameters for I2C and SPI
    SetConfig {
        /// I2C frequency
        #[arg(long)]
        i2c_frequency: u32,

        /// SPI frequency
        #[arg(long)]
        spi_frequency: u32,

        /// SPI phase first transition
        #[arg(long, default_value_t)]
        spi_first_transition: bool,

        /// SPI polarity idle low
        #[arg(long, default_value_t)]
        spi_idle_low: bool,
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
        /// I2C slave address
        #[arg(short, long, value_parser(parse_byte))]
        address: u8,

        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },

    /// Write bytes through I2C bus to device at given address
    Write {
        /// I2C slave address
        #[arg(short, long, value_parser(parse_byte))]
        address: u8,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Write bytes follwed by read bytes
    WriteRead {
        /// I2C slave address
        #[arg(short, long, value_parser(parse_byte))]
        address: u8,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,

        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
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

    /// Write bytes followed by read bytes
    WriteRead {
        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },
}

impl Cli {
    pub async fn run(&self) -> Result<()> {
        match &self.command {
            None => Ok(()),
            Some(Commands::Version) => self.version().await,
            Some(Commands::I2c { command }) => match command {
                None => Ok(()),
                Some(I2cCommands::Scan { reserved }) => self.i2c_scan(*reserved).await,
                Some(I2cCommands::Read { address, count }) => self.i2c_read(address, count).await,
                Some(I2cCommands::Write { address, bytes }) => self.i2c_write(address, bytes).await,
                Some(I2cCommands::WriteRead { address, bytes, count }) => {
                    self.i2c_write_then_read(address, bytes, count).await
                }
            },
            Some(Commands::Spi { command }) => match command {
                None => Ok(()),
                Some(SpiCommands::Read { count }) => self.spi_read(count).await,
                Some(SpiCommands::Write { bytes }) => self.spi_write(bytes).await,
                Some(SpiCommands::WriteRead { count, bytes }) => self.spi_write_then_read(bytes, count).await,
            },
            Some(Commands::SetConfig {
                i2c_frequency,
                spi_frequency,
                spi_first_transition,
                spi_idle_low,
            }) => {
                self.set_config(*i2c_frequency, *spi_frequency, *spi_first_transition, *spi_idle_low)
                    .await
            }
        }
    }

    async fn version(&self) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        match pg.version().await {
            Ok(version) => {
                println!(
                    "Pico de Gallo FW v{}.{}.{}",
                    version.major, version.minor, version.patch
                );
                Ok(())
            }
            Err(_) => Err(eyre!("Failed to get version")),
        }
    }

    async fn i2c_scan(&self, reserved: bool) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        let mut builder = Builder::with_capacity(17, 8);
        builder.push_record(
            (0..=16)
                .map(|i| if i == 0 { String::new() } else { format!("{:x}", i - 1) })
                .collect::<Vec<_>>(),
        );

        for hi in 0..=7 {
            let mut row = vec![format!("{:x} ", hi)];

            for lo in 0..=15 {
                let address = hi << 4 | lo;
                let stat = match address {
                    0x00..=0x07 | 0x78..=0x7f => {
                        if reserved {
                            match pg.i2c_read(address, 1).await {
                                Ok(_) => format!("{:02x}", address),
                                Err(_) => "--".to_string(),
                            }
                        } else {
                            "RR".to_string()
                        }
                    }
                    _ => match pg.i2c_read(address, 1).await {
                        Ok(_) => format!("{:02x}", address),
                        Err(_) => "--".to_string(),
                    },
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

    async fn i2c_read(&self, address: &u8, count: &usize) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        let buf = match pg.i2c_read(*address, *count as u16).await {
            Ok(data) => data,
            Err(_) => return Err(eyre!("i2c_read failed")),
        };

        for (i, b) in buf.iter().enumerate() {
            if i > 0 && i % 16 == 0 {
                println!();
            }

            print!("{:02x} ", b);
        }

        println!();

        Ok(())
    }

    async fn i2c_write(&self, address: &u8, bytes: &[u8]) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        if pg.i2c_write(*address, bytes).await.is_ok() {
            Ok(())
        } else {
            Err(eyre!("i2c_write failed"))
        }
    }

    async fn i2c_write_then_read(&self, address: &u8, bytes: &[u8], count: &usize) -> Result<()> {
        self.i2c_write(address, bytes).await?;
        self.i2c_read(address, count).await
    }

    async fn spi_read(&self, count: &usize) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        let buf = match pg.spi_read(*count as u16).await {
            Ok(data) => data,
            Err(_) => return Err(eyre!("spi read failed")),
        };

        for (i, b) in buf.iter().enumerate() {
            if i > 0 && i % 16 == 0 {
                println!();
            }

            print!("{:02x} ", b);
        }

        println!();

        Ok(())
    }

    async fn spi_write(&self, bytes: &[u8]) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        if pg.spi_write(bytes).await.is_ok() {
            Ok(())
        } else {
            Err(eyre!("spi write failed"))
        }
    }

    async fn spi_write_then_read(&self, bytes: &[u8], count: &usize) -> Result<()> {
        self.spi_write(bytes).await?;
        self.spi_read(count).await
    }

    async fn set_config(
        &self,
        i2c_frequency: u32,
        spi_frequency: u32,
        spi_first_transition: bool,
        spi_idle_low: bool,
    ) -> Result<()> {
        let pg = if self.serial_number.is_some() {
            PicoDeGallo::new_with_serial_number(self.serial_number.as_ref().unwrap())
        } else {
            PicoDeGallo::new()
        };

        let spi_polarity = if spi_idle_low {
            SpiPolarity::IdleLow
        } else {
            SpiPolarity::IdleHigh
        };

        let spi_phase = if spi_first_transition {
            SpiPhase::CaptureOnFirstTransition
        } else {
            SpiPhase::CaptureOnSecondTransition
        };

        if pg
            .set_config(i2c_frequency, spi_frequency, spi_phase, spi_polarity)
            .await
            .is_err()
        {
            Err(eyre!("set config failed"))
        } else {
            Ok(())
        }
    }
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
