use color_eyre::Result;
use pico_de_gallo::PicoDeGallo;
use shtcx::{PowerMode, shtc3};

fn main() -> Result<()> {
    color_eyre::install()?;

    let gallo = PicoDeGallo::new()?;
    let i2c = gallo.clone();
    let mut delay = gallo.clone();

    let mut sht = shtc3(i2c);

    let temperature = sht.measure_temperature(PowerMode::NormalMode, &mut delay).unwrap();
    let humidity = sht.measure_humidity(PowerMode::NormalMode, &mut delay).unwrap();
    let combined = sht.measure(PowerMode::NormalMode, &mut delay).unwrap();

    println!("Temperature: {} Â°C", temperature.as_degrees_celsius());
    println!("Humidity: {} %RH", humidity.as_percent());
    println!(
        "Combined: {} Â°C / {} %RH",
        combined.temperature.as_degrees_celsius(),
        combined.humidity.as_percent()
    );

    Ok(())
}
