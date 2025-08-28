use color_eyre::Result;
use embedded_hal_bus::spi::ExclusiveDevice;
use is31fl3743b_driver::{CSy, Is31fl3743b, SWx};
use pico_de_gallo::{Config, PicoDeGallo};
use std::time::Duration;

fn main() -> Result<()> {
    color_eyre::install()?;

    let gallo = PicoDeGallo::new(Config {
        spi_frequency: 10_000_000,
        ..Default::default()
    })?;
    let spi = gallo.clone();
    let delay = gallo.clone();
    let cs = gallo.gpio(0);

    // One SPI device only on the SPI bus
    let spi_dev = ExclusiveDevice::new(spi, cs, delay).unwrap();

    // Instantiate IS31FL3743B device
    let mut driver = Is31fl3743b::new(spi_dev).unwrap();

    // Enable phase delay to help reduce power noise
    let _ = driver.enable_phase_delay();
    // Set global current, check method documentation for more info
    let _ = driver.set_global_current(90);

    let _ = driver.set_led_peak_current_bulk(SWx::SW1, CSy::CS1, &vec![100; 11 * 18]);

    // Driver is fully set up, we can now start turning on LEDs!
    // Create a white breathing effect
    loop {
        for brightness in (0..=255_u8).chain((0..=255).rev()) {
            let _ = driver.set_led_brightness_bulk(SWx::SW1, CSy::CS1, &vec![brightness; 11 * 18]);
            std::thread::sleep(Duration::from_micros(1));
        }
    }
}
