# `pico-de-gallo-hal`

[`pico-de-gallo-hal`](https://docs.rs/pico-de-gallo-hal) lets you run real
`embedded-hal` driver code against a Pico de Gallo board on your laptop.

That is the whole value proposition:

- write your driver against standard traits,
- swap in `pico-de-gallo-hal` during host-side testing,
- iterate without cross-compiling, flashing, linker scripts, or probe tools.

If your driver already speaks `embedded-hal`, this crate turns Pico de Gallo
into a host-side transport layer instead of a custom test harness.

## Runtime Model

`Hal::new()` works in both sync and async host code.

- **Inside a Tokio runtime**, the crate uses `tokio::task::block_in_place()` for
  blocking trait calls.
- **Outside Tokio**, it creates and owns its own runtime.

That means one `Hal` value can back ordinary tests, examples, and async host
applications.

## Construction

| Method | Purpose |
|---|---|
| `Hal::new()` | Connect to the first matching board |
| `Hal::new_with_serial_number(serial)` | Connect to one specific board |

## Accessors and Helpers

The current public API is:

| Method | Returns | Purpose |
|---|---|---|
| `i2c()` | `I2c` | I<sup>2</sup>C bus handle implementing blocking and async traits |
| `spi()` | `Spi` | Raw SPI bus handle |
| `spi_device(cs_pin)` | `Result<SpiDev, SpiHalError>` | SPI device handle that manages chip-select for you; validates `cs_pin` before driving it |
| `num_gpios()` | `Result<u8, HalInitError>` | Device-reported GPIO count — the runtime bound for a pin index or chip select |
| `uart()` | `Uart` | UART handle implementing `embedded_io` and `embedded_io_async` |
| `gpio(pin)` | `Gpio` | GPIO pin handle implementing digital traits |
| `pwm_channel(channel)` | `PwmChannel` | PWM channel handle implementing `SetDutyCycle` |
| `delay()` | `Delay` | Delay provider |
| `onewire()` | `OneWire` | Project-specific 1-Wire handle |
| `adc_read(channel)` | `Result<u16, AdcHalError>` | Single-shot ADC read |
| `adc_get_config()` | `Result<AdcConfigurationInfo, AdcHalError>` | ADC capabilities/configuration |
| `i2c_set_config(frequency)` | `Result<(), I2cHalError>` | Set I<sup>2</sup>C frequency |
| `i2c_get_config()` | `Result<I2cFrequency, I2cHalError>` | Read I<sup>2</sup>C frequency |
| `spi_set_config(freq, phase, polarity)` | `Result<(), SpiHalError>` | Set SPI mode and clock |
| `spi_get_config()` | `Result<SpiConfigurationInfo, SpiHalError>` | Read SPI configuration |
| `pwm_set_config(channel, freq, phase_correct)` | `Result<(), PwmHalError>` | Set PWM slice configuration |
| `pwm_get_config(channel)` | `Result<PwmConfigurationInfo, PwmHalError>` | Read PWM slice configuration |
| `gpio_subscribe(pin, edge)` | `Result<(), GpioHalError>` | Start firmware-side GPIO monitoring |
| `gpio_unsubscribe(pin)` | `Result<(), GpioHalError>` | Stop GPIO monitoring |

> [!NOTE]
> The source-of-truth API currently exposes `gpio(pin)` and `uart()`. There are
> **not** separate `output_pin()`, `input_pin()`, or `uart_async()` constructors;
> the returned handles implement the relevant blocking and async traits directly.

## Implemented Traits

| Peripheral | Blocking trait | Async trait |
|---|---|---|
| GPIO | `OutputPin`, `InputPin`, `StatefulOutputPin` | `Wait` |
| I<sup>2</sup>C | `embedded_hal::i2c::I2c` | `embedded_hal_async::i2c::I2c` |
| SPI | `SpiBus`, `SpiDevice` | `SpiBus`, `SpiDevice` |
| UART | `embedded_io::Read`, `embedded_io::Write` | `embedded_io_async::Read`, `embedded_io_async::Write` |
| PWM | `SetDutyCycle` | — |
| Delay | `DelayNs` | `DelayNs` |

And two project-specific surfaces sit alongside the trait-based ones:

| Type / method | Why it exists |
|---|---|
| `OneWire` via `hal.onewire()` | there is no standard `embedded-hal` 1-Wire trait |
| `adc_read()` / `adc_get_config()` | there is no stable `embedded-hal` ADC trait in 1.0 |

## Minimal Example

```rust,no_run
use embedded_hal::i2c::I2c;
use pico_de_gallo_hal::Hal;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hal = Hal::new();
    let mut i2c = hal.i2c();

    let mut buf = [0u8; 2];
    i2c.write_read(0x48, &[0x00], &mut buf)?;

    println!("raw bytes: {:02x?}", buf);
    Ok(())
}
```

That same pattern is why this crate is so useful in driver development: the
code above looks like ordinary embedded Rust because it is ordinary embedded
Rust.

## Transparent Transaction Batching

Two methods matter a lot for performance:

- `I2c::transaction()`
- `SpiDevice::transaction()`

The HAL does not turn those into several USB round-trips. Instead, it encodes
the operations into Pico de Gallo batch requests and sends them in one shot.

So if your driver already uses the transaction APIs from `embedded-hal`, you get
Pico de Gallo's batching support automatically.

```rust,no_run
use embedded_hal::i2c::{I2c, Operation};
use pico_de_gallo_hal::Hal;

fn read_register(hal: &Hal) -> Result<[u8; 2], Box<dyn std::error::Error>> {
    let mut i2c = hal.i2c();
    let mut buf = [0u8; 2];

    i2c.transaction(
        0x48,
        &mut [
            Operation::Write(&[0x00]),
            Operation::Read(&mut buf),
        ],
    )?;

    Ok(buf)
}
```

For SPI devices, `spi_device(cs_pin)` wraps the same idea with automatic CS
assert/deassert around the whole transaction.

## Chip-select bounds

`spi_device(cs_pin)` validates `cs_pin` against the GPIO count the
connected board reports **before** it drives the pin, so a refused
chip-select leaves the pin exactly as you configured it. Read the count
yourself with `num_gpios()`; prefer it over the compile-time
`pico_de_gallo_lib::NUM_GPIOS`, which is only this build's default.

The first lookup performs one implicit validated `device/info`
round-trip, bounded at 300 seconds; the value is then cached and shared
by every handle cloned from the same connection. `SpiHalError` keeps the
outcomes disjoint:

| Variant | Meaning |
|---|---|
| `InvalidCsPin { cs, num_gpios }` | index at or beyond the reported count |
| `NoGpios` | the board reports zero GPIOs |
| `DeviceInfo(ValidateError)` | the count could not be established — transport, 300-second timeout, legacy firmware, or schema mismatch |
| `Comms(String)` | the CS-high drive or the batch transport failed |
| `Spi(SpiError)` | the firmware refused or failed the transfer |

`DeviceInfo(_)` is never reported as an invalid chip-select: a failure to
learn the valid range is a connectivity or compatibility problem, not an
argument problem.

`num_gpios()` returns `HalInitError::Validate(_)` on failure, including
`ValidateError::Timeout`. Failures are not cached, so retrying is fine.

> **Schema-freeze caveat.** On this branch a successful validation does
> not prove wire-*shape* compatibility: `DeviceInfo` changed while both
> host and firmware still report schema 0.6.1.

## When to Reach for This Crate

Use `pico-de-gallo-hal` when you want to:

- validate a driver crate against real hardware behavior,
- keep one code path for host-side tests and MCU targets,
- avoid writing custom mocks before you know the driver is correct.

For a full walk-through, jump ahead to
[Testing with `pico-de-gallo-hal`](../driver/testing.md) in Part V.
