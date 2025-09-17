use color_eyre::Result;
use embedded_hal::digital::OutputPin;
use pico_de_gallo_lib::PicoDeGallo;
use std::time::Duration;

fn main() -> Result<()> {
    color_eyre::install()?;

    let gallo = PicoDeGallo::new(Default::default())?;
    let mut gpio = gallo.gpio(0);

    loop {
        gpio.set_high()?;
        std::thread::sleep(Duration::from_secs(1));
        gpio.set_low()?;
        std::thread::sleep(Duration::from_secs(1));
    }
}
