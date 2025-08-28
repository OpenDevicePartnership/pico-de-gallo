use color_eyre::Result;
use embedded_hal_bus::spi::ExclusiveDevice;
use is31fl3743b_driver::{CSy, Is31fl3743b, SWx};
use pico_de_gallo::PicoDeGallo;
use std::time::Duration;

const COLS: [SWx; 11] = [
    SWx::SW1,
    SWx::SW2,
    SWx::SW3,
    SWx::SW4,
    SWx::SW5,
    SWx::SW6,
    SWx::SW7,
    SWx::SW8,
    SWx::SW9,
    SWx::SW10,
    SWx::SW11,
];
const ROWS: [CSy; 18] = [
    CSy::CS1,
    CSy::CS2,
    CSy::CS3,
    CSy::CS4,
    CSy::CS5,
    CSy::CS6,
    CSy::CS7,
    CSy::CS8,
    CSy::CS9,
    CSy::CS10,
    CSy::CS11,
    CSy::CS12,
    CSy::CS13,
    CSy::CS14,
    CSy::CS15,
    CSy::CS16,
    CSy::CS17,
    CSy::CS18,
];

fn main() -> Result<()> {
    color_eyre::install()?;

    let gallo = PicoDeGallo::new()?;
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

    // Driver is fully set up, we can now start turning on LEDs!
    // Create a white breathing effect
    loop {
        for brightness in (0..=80).step_by(10).chain((0..=80).step_by(10).rev()) {
            for i in ROWS.into_iter() {
                for j in COLS.into_iter() {
                    // Set scaling register to max current
                    let _ = driver.set_led_peak_current(j, i, 100);
                    // Set PWM brightness register
                    let _ = driver.set_led_brightness(j, i, brightness);
                }
            }
            std::thread::sleep(Duration::from_micros(1));
        }
    }
}
