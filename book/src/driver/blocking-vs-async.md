# Blocking vs async parity

One of the nicest things about building on `embedded-hal` is that you do
not have to choose one execution model forever.

`pico-de-gallo-hal::I2c` implements both the blocking
`embedded_hal::i2c::I2c` trait and the async
`embedded_hal_async::i2c::I2c` trait. So the exact same TMP102 driver
can be used from synchronous code and from async code.

## Share the hard parts

The register definitions, address handling, temperature conversions, and
configuration builder do not care whether the bus is blocking or async.
Keep those pieces shared:

```rust,noplayground
impl<I2C> Tmp102<I2C, Running> {
    fn decode_temperature(raw: [u8; 2], extended_mode: bool) -> Celsius {
        let mut value = i16::from_be_bytes(raw);
        value /= if extended_mode { 8 } else { 16 };
        Celsius(value as f32 * 0.0625)
    }

    fn encode_limit(limit: Celsius) -> [u8; 2] {
        ((limit.0 / 0.0625) as i16 * 16).to_be_bytes()
    }
}
```

Then make the blocking and async fronts as thin as possible.

## A simple parity pattern

Because some I<sup>2</sup>C types implement *both* traits, duplicating the
same inherent method names can get awkward. The least surprising pattern
for a small driver is:

- keep the ergonomic async API as the primary surface
- add thin blocking siblings with `_blocking` suffixes
- keep all encoding and decoding logic in shared helpers

```rust,noplayground
impl<I2C: embedded_hal::i2c::I2c> Tmp102<I2C, Running> {
    pub fn temperature_blocking(&mut self) -> Result<Celsius, ErrorKind> {
        let raw: [u8; 2] = self.inner.temperature().read()?.into();
        Ok(Self::decode_temperature(raw, self.extended_mode))
    }

    pub fn configure_blocking(&mut self, config: Config) -> Result<(), ErrorKind> {
        self.extended_mode = config.extended_mode == ExtendedMode::Enable;

        self.inner.configuration().modify(|reg| {
            reg.set_tm(config.thermostat_mode);
            reg.set_pol(config.polarity);
            reg.set_em(config.extended_mode);
            reg.set_cr(config.conversion_rate);
        })
    }
}

impl<I2C: embedded_hal_async::i2c::I2c> Tmp102<I2C, Running> {
    pub async fn temperature(&mut self) -> Result<Celsius, ErrorKind> {
        let raw: [u8; 2] = self.inner.temperature().read_async().await?.into();
        Ok(Self::decode_temperature(raw, self.extended_mode))
    }

    pub async fn configure(&mut self, config: Config) -> Result<(), ErrorKind> {
        self.extended_mode = config.extended_mode == ExtendedMode::Enable;

        self.inner
            .configuration()
            .modify_async(|reg| {
                reg.set_tm(config.thermostat_mode);
                reg.set_pol(config.polarity);
                reg.set_em(config.extended_mode);
                reg.set_cr(config.conversion_rate);
            })
            .await
    }
}
```

If you want identical method names on both sides, a macro-based approach
such as `maybe-async-cfg` is a good next step. For a first driver,
though, the explicit version is easier to read and maintain.

## Same driver, blocking usage

```rust,no_run
use pico_de_gallo_hal::Hal;
use tmp102::{Celsius, Tmp102, A0};

fn main() -> Result<(), embedded_hal::i2c::ErrorKind> {
    let hal = Hal::new();
    let i2c = hal.i2c();
    let mut sensor = Tmp102::continuous(i2c, A0::Gnd);

    let Celsius(temp) = sensor.temperature_blocking()?;
    println!("{temp:.2} °C");
    Ok(())
}
```

## Same driver, async usage

Same crate, same build, no extra feature flag — just an executor:

```rust,no_run
use pico_de_gallo_hal::Hal;
use tmp102::{Celsius, Tmp102, A0};

#[tokio::main]
async fn main() -> Result<(), embedded_hal::i2c::ErrorKind> {
    let hal = Hal::new();
    let i2c = hal.i2c();
    let mut sensor = Tmp102::continuous(i2c, A0::Gnd);

    let Celsius(temp) = sensor.temperature().await?;
    println!("{temp:.2} °C");
    Ok(())
}
```

That is the payoff: same driver type, same register model, same public
concepts, two execution models.

## When async is worth it

For one occasional temperature read, blocking code is often perfectly
fine.

Async starts paying for itself when your program wants to interleave
sensor access with other work, for example:

- polling several devices on one executor
- serving a network API while reading sensors
- overlapping I/O-bound work with unrelated futures
- driving a GUI or TUI without stalling the event loop

The nice part is that you do not have to guess up front. A well-shaped
TMP102 driver can serve both audiences.
