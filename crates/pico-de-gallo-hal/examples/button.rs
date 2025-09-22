use embedded_hal_async::digital::Wait;
use pico_de_gallo_hal::Hal;

#[tokio::main]
async fn main() {
    let hal = Hal::new();
    let mut gpio = hal.gpio(0);

    loop {
        gpio.wait_for_falling_edge().await.unwrap();
        println!("Button pressed");
        gpio.wait_for_rising_edge().await.unwrap();
        println!("Button released");
    }
}
