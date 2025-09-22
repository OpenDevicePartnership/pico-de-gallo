use embedded_hal::digital::OutputPin;
use pico_de_gallo_hal::Hal;
use std::time::Duration;

fn main() {
    let hal = Hal::new();
    let mut gpio = hal.gpio(0);

    loop {
        gpio.set_high().unwrap();
        std::thread::sleep(Duration::from_secs(1));
        gpio.set_low().unwrap();
        std::thread::sleep(Duration::from_secs(1));
    }
}
