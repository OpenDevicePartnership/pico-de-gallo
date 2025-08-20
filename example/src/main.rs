use color_eyre::Result;
use pico_de_gallo::{Delay, PicoDeGallo};
use shtcx::{shtc3, PowerMode};

fn main() -> Result<()> {
    color_eyre::install()?;

    let gallo = PicoDeGallo::new()?;
    let mut delay = Delay;
    let mut sht = shtc3(gallo);

    let temperature = sht
        .measure_temperature(PowerMode::NormalMode, &mut delay)
        .unwrap();
    let humidity = sht
        .measure_humidity(PowerMode::NormalMode, &mut delay)
        .unwrap();
    let combined = sht.measure(PowerMode::NormalMode, &mut delay).unwrap();

    println!("Temperature: {} °C", temperature.as_degrees_celsius());
    println!("Humidity: {} %RH", humidity.as_percent());
    println!(
        "Combined: {} °C / {} %RH",
        combined.temperature.as_degrees_celsius(),
        combined.humidity.as_percent()
    );

    Ok(())
}
