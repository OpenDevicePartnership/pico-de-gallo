use color_eyre::Result;
use pico_de_gallo::PicoDeGallo;

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut gallo = PicoDeGallo::new()?;

    gallo.i2c_blocking_write(0x48, &[0x0f])?;

    let mut buf = [0; 2];
    gallo.i2c_blocking_read(0x48, &mut buf)?;

    println!("{:02x} {:02x}", buf[0], buf[1]);

    Ok(())
}
