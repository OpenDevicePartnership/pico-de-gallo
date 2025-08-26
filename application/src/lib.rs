use clap::{Parser, Subcommand};
use color_eyre::Result;
use pico_de_gallo::PicoDeGallo;
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
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// I2C accessor
    I2c {
        /// I2C slave address
        #[arg(short, long, value_parser(parse_byte))]
        address: u8,

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
}

#[derive(Subcommand, Debug)]
enum I2cCommands {
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

    /// Write then read
    WriteRead {
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
    pub fn run(&self) -> Result<()> {
        let mut pg = PicoDeGallo::new()?;

        match &self.command {
            None => Ok(()),
            Some(Commands::I2c { address, command }) => match command {
                None => Ok(()),
                Some(I2cCommands::Read { count }) => self.i2c_read(&mut pg, address, count),
                Some(I2cCommands::Write { bytes }) => self.i2c_write(&mut pg, address, bytes),
                Some(I2cCommands::WriteRead { bytes, count }) => {
                    self.i2c_write_then_read(&mut pg, address, bytes, count)
                }
            },
            Some(Commands::Spi { command }) => match command {
                None => Ok(()),
                Some(SpiCommands::Read { count }) => self.spi_read(&mut pg, count),
                Some(SpiCommands::Write { bytes }) => self.spi_write(&mut pg, bytes),
                Some(SpiCommands::WriteRead { count, bytes }) => self.spi_write_then_read(&mut pg, bytes, count),
            },
        }
    }

    fn i2c_read(&self, pg: &mut PicoDeGallo, address: &u8, count: &usize) -> Result<()> {
        let mut buf = vec![0; *count];
        pg.i2c_blocking_read(*address, &mut buf)?;

        for (i, b) in buf.iter().enumerate() {
            if i > 0 && i % 16 == 0 {
                print!("\n");
            }

            print!("{:02x} ", b);
        }

        print!("\n");

        Ok(())
    }

    fn i2c_write(&self, pg: &mut PicoDeGallo, address: &u8, bytes: &[u8]) -> Result<()> {
        pg.i2c_blocking_write(*address, bytes)?;
        Ok(())
    }

    fn i2c_write_then_read(&self, pg: &mut PicoDeGallo, address: &u8, bytes: &[u8], count: &usize) -> Result<()> {
        self.i2c_write(pg, address, bytes)?;
        self.i2c_read(pg, address, count)
    }

    fn spi_read(&self, pg: &mut PicoDeGallo, count: &usize) -> Result<()> {
        let mut buf = vec![0; *count];
        pg.spi_blocking_read(&mut buf)?;

        for (i, b) in buf.iter().enumerate() {
            if i > 0 && i % 16 == 0 {
                print!("\n");
            }

            print!("{:02x} ", b);
        }

        print!("\n");

        Ok(())
    }

    fn spi_write(&self, pg: &mut PicoDeGallo, bytes: &[u8]) -> Result<()> {
        pg.spi_blocking_write(bytes)?;
        Ok(())
    }

    fn spi_write_then_read(&self, pg: &mut PicoDeGallo, bytes: &[u8], count: &usize) -> Result<()> {
        let mut buf = vec![0; *count];
        pg.spi_blocking_transfer(&mut buf, bytes)?;

        for (i, b) in buf.iter().enumerate() {
            if i > 0 && i % 16 == 0 {
                print!("\n");
            }

            print!("{:02x} ", b);
        }

        print!("\n");

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
