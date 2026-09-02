# Make invalid states unrepresentable

The generated API is accurate, but it is not the API we want to hand to
users. Driver users do not want to think in pointer bytes and raw 16-bit
register images; they want to ask for a temperature, configure alert
limits, and move the device between running and shutdown modes.

This is the point where we decide what the *public* crate feels like.

## Start with a temperature type

A plain `f32` works, but it tells the caller nothing about units. A tiny
newtype fixes that immediately:

```rust,noplayground
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Celsius(pub f32);
```

Now a return value of `Celsius(21.5)` is obviously a temperature and not
an arbitrary calibration constant.

## Collect configuration into a builder

Instead of making users pass four unrelated arguments into
`configure(...)`, give them a small builder with good defaults. The four
fields are enums `ddc` generated from the manifest.

```rust,noplayground
use registers::{ConversionRate, ExtendedMode, Polarity, ThermostatMode};

#[derive(Clone, Copy, Debug)]
pub struct Config {
    thermostat_mode: ThermostatMode,
    polarity: Polarity,
    extended_mode: ExtendedMode,
    conversion_rate: ConversionRate,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            thermostat_mode: ThermostatMode::Comparator,
            polarity: Polarity::ActiveLow,
            extended_mode: ExtendedMode::Disable,
            conversion_rate: ConversionRate::FourHz,
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn thermostat_mode(mut self, mode: ThermostatMode) -> Self {
        self.thermostat_mode = mode;
        self
    }

    pub fn polarity(mut self, polarity: Polarity) -> Self {
        self.polarity = polarity;
        self
    }

    pub fn extended_mode(mut self, mode: ExtendedMode) -> Self {
        self.extended_mode = mode;
        self
    }

    pub fn conversion_rate(mut self, rate: ConversionRate) -> Self {
        self.conversion_rate = rate;
        self
    }
}
```

That lets the call site read cleanly:

```rust,noplayground
let config = Config::new()
    .extended_mode(ExtendedMode::Enable)
    .conversion_rate(ConversionRate::EightHz);
```

## Use typestate for run vs shutdown

TMP102 has two materially different operating states:

- **running**, where the sensor converts continuously
- **shutdown**, where it sleeps until explicitly kicked

A single `Tmp102` type with a runtime boolean would work, but typestate
lets us express the distinction in the type system.

```rust,noplayground
use core::marker::PhantomData;

use registers::Tmp102Registers;

pub struct Running;
pub struct Shutdown;

pub struct Tmp102<I2C, State = Running> {
    inner: Tmp102Registers<Interface<I2C>>,
    extended_mode: bool,
    _state: PhantomData<State>,
}
```

The extra state parameter means we can say "this method only exists when
the sensor is running" and let the compiler enforce it.

A small helper keeps state transitions tidy:

```rust,noplayground
impl<I2C, State> Tmp102<I2C, State> {
    fn change_state<Next>(self) -> Tmp102<I2C, Next> {
        Tmp102 {
            inner: self.inner,
            extended_mode: self.extended_mode,
            _state: PhantomData,
        }
    }
}
```

## Constructors and shared helpers

We will make continuous-conversion mode the default constructor:

```rust,noplayground
impl<I2C> Tmp102<I2C, Running> {
    pub fn new(i2c: I2C, a0: A0) -> Self {
        Self::continuous(i2c, a0)
    }

    pub fn continuous(i2c: I2C, a0: A0) -> Self {
        Self {
            inner: Tmp102Registers::new(Interface::new(i2c, a0)),
            extended_mode: false,
            _state: PhantomData,
        }
    }

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

Those helpers keep all the datasheet math in one place instead of
smearing it across every high-level method.

## The high-level async API

Here is the shape we are after for the async path:

```rust,noplayground
use registers::ShutdownMode;

impl<I2C: AsyncI2c> Tmp102<I2C, Running> {
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

    pub async fn set_low_limit(&mut self, limit: Celsius) -> Result<(), ErrorKind> {
        let raw = Self::encode_limit(limit);
        self.inner.tlow().write_async(|reg| *reg = raw.into()).await
    }

    pub async fn set_high_limit(&mut self, limit: Celsius) -> Result<(), ErrorKind> {
        let raw = Self::encode_limit(limit);
        self.inner.thigh().write_async(|reg| *reg = raw.into()).await
    }

    pub async fn temperature(&mut self) -> Result<Celsius, ErrorKind> {
        let raw: [u8; 2] = self.inner.temperature().read_async().await?.into();
        Ok(Self::decode_temperature(raw, self.extended_mode))
    }

    pub async fn shutdown(mut self) -> Result<Tmp102<I2C, Shutdown>, ErrorKind> {
        self.inner
            .configuration()
            .modify_async(|reg| reg.set_sd(ShutdownMode::PowerOff))
            .await?;

        Ok(self.change_state())
    }
}

impl<I2C: AsyncI2c> Tmp102<I2C, Shutdown> {
    pub async fn run(mut self) -> Result<Tmp102<I2C, Running>, ErrorKind> {
        self.inner
            .configuration()
            .modify_async(|reg| reg.set_sd(ShutdownMode::Running))
            .await?;

        Ok(self.change_state())
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

The important part is not the exact spelling; it is the contract:

- temperatures come back as `Celsius`
- alert thresholds are set in `Celsius`
- `configure()` accepts one coherent settings object
- `shutdown()` consumes `Tmp102<_, Running>`
- `run()` gives you back `Tmp102<_, Running>`

That makes illegal flows hard to write. You cannot accidentally call the
"running-only" API on a shutdown sensor because the type no longer
matches.

## Final public shape

This is the surface we want readers to remember:

```rust,noplayground
pub enum A0 { Gnd, Vplus, Sda, Scl }
pub struct Celsius(pub f32);
pub struct Config { /* builder-style setters */ }
pub struct Running;
pub struct Shutdown;
pub struct Tmp102<I2C, State = Running> { /* private fields */ }

impl<I2C> Tmp102<I2C, Running> {
    pub fn new(i2c: I2C, a0: A0) -> Self;
    pub fn continuous(i2c: I2C, a0: A0) -> Self;
}

impl<I2C: AsyncI2c> Tmp102<I2C, Running> {
    pub async fn configure(&mut self, config: Config) -> Result<(), ErrorKind>;
    pub async fn set_high_limit(&mut self, limit: Celsius) -> Result<(), ErrorKind>;
    pub async fn set_low_limit(&mut self, limit: Celsius) -> Result<(), ErrorKind>;
    pub async fn temperature(&mut self) -> Result<Celsius, ErrorKind>;
    pub async fn shutdown(self) -> Result<Tmp102<I2C, Shutdown>, ErrorKind>;
}

impl<I2C: AsyncI2c> Tmp102<I2C, Shutdown> {
    pub async fn run(self) -> Result<Tmp102<I2C, Running>, ErrorKind>;
}
```

> [!TIP]
> The alternative is a single `Tmp102<I2C>` with a runtime `bool` that
> tracks shutdown state. That keeps the type simpler, but it also pushes
> more mistakes to runtime. For a tiny driver like TMP102, the extra
> typestate surface is worth it.

Next we put that API to work against real hardware.
