use color_eyre::Result;
use mpu6050_dmp::address::Address;
use mpu6050_dmp::sensor::Mpu6050;
use pico_de_gallo_lib::PicoDeGallo;
use std::time::Duration;

fn main() -> Result<()> {
    color_eyre::install()?;

    let gallo = PicoDeGallo::new(Default::default())?;
    let i2c = gallo.clone();
    let mut delay = gallo.clone();

    let mut sensor = Mpu6050::new(i2c, Address::default()).unwrap();

    println!("Initializing sensor");
    sensor.initialize_dmp(&mut delay).unwrap();
    println!("Initialization complete");

    let accel_data = sensor.accel().unwrap();
    println!(
        "Accelerometer [mg]: x={}, y={}, z={}",
        accel_data.x() as i32,
        accel_data.y() as i32,
        accel_data.z() as i32
    );

    let gyro_data = sensor.gyro().unwrap();
    println!(
        "Gyroscope [deg/s]: x={}, y={}, z={}",
        gyro_data.x() as i32,
        gyro_data.y() as i32,
        gyro_data.z() as i32
    );

    loop {
        let (accel, gyro, temp) = (
            sensor.accel().unwrap(),
            sensor.gyro().unwrap(),
            sensor.temperature().unwrap().celsius(),
        );
        println!("Sensor Readings:");
        println!(
            "  Accelerometer [mg]: x={}, y={}, z={}",
            accel.x() as i32,
            accel.y() as i32,
            accel.z() as i32
        );
        println!(
            "  Gyroscope [deg/s]: x={}, y={}, z={}",
            gyro.x() as i32,
            gyro.y() as i32,
            gyro.z() as i32
        );
        println!("  Temperature: {:.2}C", temp);
        std::thread::sleep(Duration::from_secs(1));
    }
}
