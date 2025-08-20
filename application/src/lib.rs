use clap::{Parser, Subcommand};
use color_eyre::Result;
use pico_de_gallo::PicoDeGallo;
use std::num::ParseIntError;

#[derive(Parser, Debug)]
#[command(
    name = "Pico De Gallo",
    author = "Felipe Balbi <febalbi@microsoft.com>",
    about = "Access I2C/SPI devices through Pico De Gallo",
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
    WriteThenRead {
        /// Bytes to transfer
        #[arg(short, long, num_args(1..), value_parser(parse_byte))]
        bytes: Vec<u8>,

        /// Number of bytes to read
        #[arg(short, long)]
        count: usize,
    },
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        let mut pg = PicoDeGallo::new()?;

        match &self.command {
            None => Ok(()),
            Some(Commands::I2c { address, command }) => match command {
                None => Ok(()),
                Some(I2cCommands::Read { count }) => self.read(&mut pg, address, count),
                Some(I2cCommands::Write { bytes }) => self.write(&mut pg, address, bytes),
                Some(I2cCommands::WriteThenRead { bytes, count }) => {
                    self.write_then_read(&mut pg, address, bytes, count)
                }
            },
        }
    }

    fn read(&self, pg: &mut PicoDeGallo, address: &u8, count: &usize) -> Result<()> {
        let mut buf = vec![0; *count];
        pg.i2c_blocking_read(*address, &mut buf)?;
        dbg!(&buf[..*count]);
        Ok(())
    }

    fn write(&self, pg: &mut PicoDeGallo, address: &u8, bytes: &[u8]) -> Result<()> {
        pg.i2c_blocking_write(*address, bytes)?;
        Ok(())
    }

    fn write_then_read(
        &self,
        pg: &mut PicoDeGallo,
        address: &u8,
        bytes: &[u8],
        count: &usize,
    ) -> Result<()> {
        self.write(pg, address, bytes)?;
        self.read(pg, address, count)
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
