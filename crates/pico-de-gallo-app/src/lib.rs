use clap::{Parser, Subcommand};
use color_eyre::{Result, eyre::eyre};
use pico_de_gallo_lib::PicoDeGallo;
use std::num::ParseIntError;

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

    /// I2C accessor
    I2c {
        /// I2C commands
        #[command(subcommand)]
        command: Option<I2cCommands>,
    },

    /// SPI accessor
    Spi {
        /// SPI commands
        #[command(subcommand)]
        command: Option<SpiCommands>,
    },

    /// Set bus parameters for I2C and SPI
    SetConfig {
        /// I2C frequency
        #[arg(short = 'i', long)]
        i2c_frequency: u32,

        /// SPI frequency
        #[arg(short = 's', long)]
        spi_frequency: u32,

        /// SPI phase first transition
        #[arg(short = 'p', long, default_value_t)]
        spi_first_transition: bool,

        /// SPI polarity idle low
        #[arg(short = 'o', long, default_value_t)]
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

    /// Read
    Read {
        /// I2C slave address
        #[arg(short, long, value_parser(parse_byte))]
        address: u8,

        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },

    /// Write
    Write {
        /// I2C slave address
        #[arg(short, long, value_parser(parse_byte))]
        address: u8,

        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Write then read
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
    /// Read
    Read {
        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },

    /// Write
    Write {
        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,
    },

    /// Write followed by read.
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

        let mut high = 0;
        print!(
            r#"
   0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f
{:x} "#,
            high
        );

        for address in 0..=0x7f_u8 {
            match address {
                0x00..=0x07 | 0x78..=0x7f => {
                    if reserved {
                        let result = pg.i2c_read(address, 1).await;

                        if result.is_ok() {
                            print!("{:02x} ", address);
                        } else {
                            print!("-- ");
                        }
                    } else {
                        print!("RR ");
                    }
                }

                _ => {
                    let result = pg.i2c_read(address, 1).await;
                    if result.is_ok() {
                        print!("{:02x} ", address);
                    } else {
                        print!("-- ");
                    }
                }
            }

            if address & 0x0f == 0x0f {
                high += 1;
                print!("\n");

                if high < 8 {
                    print!("{:x} ", high);
                }
            }
        }
        print!("\n");

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
                print!("\n");
            }

            print!("{:02x} ", b);
        }

        print!("\n");

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
                print!("\n");
            }

            print!("{:02x} ", b);
        }

        print!("\n");

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
        _i2c_frequency: u32,
        _spi_frequency: u32,
        _spi_first_transition: bool,
        _spi_idle_low: bool,
    ) -> Result<()> {
        // let pg = PicoDeGallo::new(Default::default())?;
        // let mut io = pg.usb.borrow_mut();

        // let spi_polarity = if spi_idle_low {
        //     SpiPolarity::IdleLow
        // } else {
        //     SpiPolarity::IdleHigh
        // };

        // let spi_phase = if spi_first_transition {
        //     SpiPhase::CaptureOnFirstTransition
        // } else {
        //     SpiPhase::CaptureOnSecondTransition
        // };

        // io.set_config(i2c_frequency, spi_frequency, spi_phase, spi_polarity)?;

        Ok(())
    }
}

fn parse_byte(s: &str) -> Result<u8, ParseIntError> {
    if s.starts_with("0x") {
        u8::from_str_radix(&s[2..], 16)
    } else if s.starts_with("0b") {
        u8::from_str_radix(&s[2..], 2)
    } else {
        u8::from_str_radix(&s[2..], 10)
    }
}
