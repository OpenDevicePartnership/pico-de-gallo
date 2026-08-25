# Pico de Gallo HAL — AI Agent Guide

Audience: AI coding agents writing host-side examples or HIL tests that
exercise an `embedded-hal` driver against real hardware via a Pico de
Gallo USB bridge.

**Source of truth:** `crates/pico-de-gallo-hal/src/lib.rs` on `main`.
If this file contradicts that file, the source wins — file an issue at
<https://github.com/OpenDevicePartnership/pico-de-gallo/issues>.

## 1. TL;DR

You are generating either a host-side **example binary** at
`examples/<chip>.rs` or a **hardware-in-the-loop test** gated behind
`#[cfg(feature = "hil")]`. Both shapes exercise an `embedded-hal`
driver against real hardware through a Pico de Gallo USB bridge.

Decide which shape to produce (§2), pick blocking or async (§3), set
up `Cargo.toml` (§4), look up which HAL accessor your device needs
(§5 decision tree → §6 per-peripheral reference), drop a fixed-format
decision-log header at the top of the generated file (format defined
in §7), and verify against the checklist in §8.

The HAL exposes I²C, SPI, GPIO, PWM, ADC, 1-Wire, UART, and Delay.
Pin range for any GPIO is **0..=3** (the firmware only exposes four
pins). The HAL is `pico-de-gallo-hal` on crates.io.

## 2. Output-shape rule (binary vs. HIL test)

**Default: binary at `examples/<chip>.rs`.**

Pick **HIL test** (`#[cfg(feature = "hil")] #[test]` inside a driver
crate, gated behind a `hil` Cargo feature) only when **at least one**
of these is true:

- The user explicitly says "test", "validate the driver", "regression
  test", "CI", or "hardware-in-the-loop".
- The user names an existing driver crate they want regression
  coverage for.

Otherwise produce the binary. "Show me X works", "make X blink",
"read a value from X", and bring-up scripts are all binary requests.

If both shapes seem to fit, default to binary; binaries are easier to
run by hand and the user can always wrap one in a test later.

The HIL-test shape pattern is:

```rust
#[cfg(feature = "hil")]
#[test]
fn <chip>_<assertion>() {
    let hal = pico_de_gallo_hal::Hal::new();
    // ... assert something the user would actually want to know
    //     is true for their hardware ...
}
```

Gate it in the driver crate's `Cargo.toml`:

```toml
[features]
default = []
hil = ["dep:pico-de-gallo-hal"]

[dev-dependencies]
pico-de-gallo-hal = { version = "*", optional = true }
```

## 3. Sync-vs-async rule

Selection algorithm, in order:

1. If the driver crate's primary API is `async fn` → **async**
   (`#[tokio::main]`).
2. If the driver crate's primary API is blocking → **blocking**.
3. If the driver crate offers both → **blocking** (simpler).
4. If the example needs GPIO edge waits (the `Wait` trait:
   `wait_for_falling_edge`, etc.) → **async** regardless of (1)–(3).
5. If both async edge waits **and** blocking driver methods are
   needed → **async**. The HAL's `block_in_place` plumbing lets
   blocking trait calls run inside a tokio task, **but read the
   warning below first**.

> ### ⚠ Mandatory: never use `current_thread` tokio
>
> If you pick async, you MUST use the **default multi-thread**
> tokio runtime:
>
> - `#[tokio::main]` — correct.
> - `#[tokio::main(flavor = "current_thread")]` — **will panic**
>   the first time a driver issues a blocking I²C, SPI, GPIO,
>   UART, PWM, ADC, or 1-Wire call.
>
> The HAL detects "in async context" but cannot detect runtime
> flavor (see the private `in_async_context` helper at `lib.rs:463`),
> so it unconditionally calls `tokio::task::block_in_place`, which
> tokio documents as panicking on single-threaded runtimes.

Default tokio dep when async is chosen:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
```

Do **not** add `"current_thread"` to the feature list and do **not**
override the flavor.

## 4. Cargo setup

This file does not pin versions. Resolve current versions from
crates.io when you edit `Cargo.toml`. The crates you need are:

- `pico-de-gallo-hal` — the HAL itself. Goes in `[dependencies]` for
  binaries, `[dev-dependencies]` (optional, gated by the `hil`
  feature) for HIL tests.
- `embedded-hal` — required if your driver returns
  `embedded_hal::*::Error` types.
- `embedded-hal-async` — required only if you use async traits
  (`embedded_hal_async::digital::Wait`, etc.).
- `tokio` — required only if async, with features
  `["rt-multi-thread", "macros", "time"]`. **Do not** pick
  `current_thread` — see §3.
- `pico-de-gallo-lib` — required directly only when you need
  `PicoDeGallo::uart_set_config` (UART baud-rate setter; not on the
  HAL). As of HAL 0.7, `AdcChannel`, `AdcConfigurationInfo`,
  `GpioDirection`, `GpioEdge`, and `GpioPull` are re-exported by
  `pico_de_gallo_hal` and no longer require depending on the lib
  directly.
- The driver crate(s) for the device you're exercising.

Re-exported by `pico_de_gallo_hal` (no extra dependency needed):
`AdcChannel`, `AdcConfigurationInfo`, `GpioDirection`, `GpioEdge`,
`GpioEvent`, `GpioPull`, `I2cFrequency`, `SpiConfigurationInfo`,
`SpiPhase`, `SpiPolarity`, `UartConfigurationInfo`.

## 5. Peripheral decision tree

| Your device…                                          | Jump to       |
|-------------------------------------------------------|---------------|
| Talks I²C (sensor, EEPROM, expander)                  | §6.1          |
| Talks SPI, driver manages CS itself                   | §6.2          |
| Talks SPI, you provide a CS pin (typical)             | §6.3          |
| Is an LED, relay, or reset line (drives a level)      | §6.4          |
| Is a button or switch you poll                        | §6.5          |
| Is a button you wait on (edge-triggered)              | §6.6          |
| Needs streamed edge events (rotary encoder, etc.)     | §6.7          |
| Is a motor or LED needing a duty cycle                | §6.8          |
| Is an analog input (potentiometer, thermistor)        | §6.9          |
| Is a 1-Wire device (DS18B20, DS2401)                  | §6.10         |
| Talks UART or serial                                  | §6.11         |
| The driver needs `&mut impl DelayNs`                  | §6.12         |

Most drivers combine 2–3 of these (e.g. SPI device + GPIO output for
reset + Delay). Pull each one in from its subsection independently.

## 6. Per-peripheral reference

### 6.1 I²C

**When to use:** the device is on an I²C bus and uses 7-bit addressing.

**HAL accessor:** `hal.i2c()` → returns `I2c`.

**Traits implemented:** `embedded_hal::i2c::I2c`,
`embedded_hal_async::i2c::I2c`.

#### Snippet — binary form

```rust
// examples/<chip>.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   sync (reason: driver is blocking)
//   peripherals:  i2c
//   hal version:  <crate version observed at generation time>

use embedded_hal::i2c::I2c;
use pico_de_gallo_hal::Hal;

fn main() {
    let hal = Hal::new();
    let mut i2c = hal.i2c();

    let mut buf = [0u8; 2];
    i2c.write_read(0x48, &[0x00], &mut buf).unwrap();
    println!("raw: {:02x?}", buf);
}
```

#### Snippet — HIL-test form

```rust
#[cfg(feature = "hil")]
#[test]
fn tmp102_responds_on_default_address() {
    use embedded_hal::i2c::I2c;
    let hal = pico_de_gallo_hal::Hal::new();
    let mut i2c = hal.i2c();
    let mut buf = [0u8; 2];
    // A successful write_read with an ACK proves the device is wired.
    i2c.write_read(0x48, &[0x00], &mut buf).unwrap();
}
```

#### Gotchas

- 7-bit addressing only. Pass `0x48`, not `0x90`.
- `I2c::transaction()` is one USB round-trip and one bus transaction:
  adjacent same-direction operations continue without a STOP, direction
  changes use a repeated START, and only the final operation gets a STOP.
  The default `read`, `write`, and `write_read` methods all route through it.
  This contract requires schema-0.7-or-newer firmware; older firmware put a
  STOP after every operation, silently breaking gather writes and
  `write_read`. Prefer `Hal::new_validated()` so older firmware is rejected.
- Default bus frequency is 100 kHz. Bump with
  `hal.i2c_set_config(I2cFrequency::Fast)?` for 400 kHz or
  `I2cFrequency::FastPlus` for 1 MHz.
- Async usage: see §3 for the mandatory `current_thread` warning.

#### Config knobs

- `hal.i2c_set_config(I2cFrequency)` — bus frequency. Default
  `Standard` (100 kHz). Variants: `Standard`, `Fast`, `FastPlus`.
- `hal.i2c_get_config() -> Result<I2cFrequency, _>` — read current
  frequency.
- `hal.i2c_scan(include_reserved: bool) -> Result<Vec<u8>, _>` —
  scan for devices. `false` scans `0x08..=0x77`, `true` scans the
  full `0x00..=0x7F`.

### 6.2 SPI bus (no CS)

**When to use:** rarely — only when the driver crate manages chip
select itself (passes its own GPIO handle, talks to a non-CS device,
or daisy-chains). Otherwise prefer §6.3.

**HAL accessor:** `hal.spi()` → returns `Spi`.

**Traits implemented:** `embedded_hal::spi::SpiBus`,
`embedded_hal_async::spi::SpiBus`.

```rust
use embedded_hal::spi::SpiBus;
use pico_de_gallo_hal::Hal;

fn main() {
    let hal = Hal::new();
    let mut spi = hal.spi();

    let tx = [0xAA, 0x55];
    let mut rx = [0u8; 2];
    spi.transfer(&mut rx, &tx).unwrap();
    println!("rx: {:02x?}", rx);
}
```

#### Gotchas

- No automatic CS. The bus is always live.
- Config knobs are shared with §6.3: `hal.spi_set_config(...)` and
  `hal.spi_get_config()`.
- Async usage: see §3 for the mandatory `current_thread` warning.

### 6.3 SPI device (with CS)

**When to use:** the device is on an SPI bus and you provide a GPIO
pin as chip-select. This is the common case — most drivers expect an
`SpiDevice` that wraps CS handling for them.

**HAL accessor:** `hal.spi_device(cs_pin)` → returns
`Result<SpiDev, SpiHalError>`. CS pin range: `0..num_gpios`, where
`num_gpios` is the count the connected board reports — read it with
`hal.num_gpios()`. On current hardware that is 4, but do not hardcode it.

`spi_device` validates `cs_pin` against that count **before** it drives
the pin, so an out-of-range chip-select leaves the pin exactly as you
configured it. The distinct failures are:

- `SpiHalError::InvalidCsPin { cs, num_gpios }` — index at or beyond the
  reported count;
- `SpiHalError::NoGpios` — the board reports zero GPIOs;
- `SpiHalError::DeviceInfo(_)` — the count could not be established
  (transport failure, 300-second `device/info` timeout, legacy firmware,
  or schema mismatch). This is **never** reported as an invalid
  chip-select; treat it as a connectivity/compatibility problem, not an
  argument problem.

**Traits implemented:** `embedded_hal::spi::SpiDevice`,
`embedded_hal_async::spi::SpiDevice`.

#### Snippet — binary form

```rust
// examples/<chip>.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   sync (reason: driver is blocking)
//   peripherals:  spi_device(cs=0)
//   hal version:  <crate version observed at generation time>

use pico_de_gallo_hal::Hal;
use <driver_crate>::Driver;

fn main() {
    let hal = Hal::new();
    let spi = hal.spi_device(0).expect("spi_device failed");
    let mut driver = Driver::new(spi);
    // ... use driver ...
}
```

#### Snippet — HIL-test form

```rust
#[cfg(feature = "hil")]
#[test]
fn driver_who_am_i_matches_datasheet() {
    let hal = pico_de_gallo_hal::Hal::new();
    let spi = hal.spi_device(0).unwrap();
    let mut driver = <DriverCrate>::new(spi);
    assert_eq!(driver.who_am_i().unwrap(), 0xAB);
}
```

#### Gotchas

- CS pin range is `0..num_gpios` (read it with `hal.num_gpios()`; do not
  assume 4). The pin **must not** also be used as a
  `Gpio` elsewhere in the program (no concurrent ownership).
- The async `SpiDevice::transaction` is **not** cancellation-safe:
  if the future is dropped after CS is asserted low but before it is
  deasserted, CS stays low. Match the behavior of
  `embedded-hal-bus::ExclusiveDevice`.
- `Operation::TransferInPlace(buf)` works, but the implementation
  allocates a `Vec` the same size as `buf` per occurrence in a
  transaction. For large in-place transfers prefer
  `Operation::Transfer(read, write)` with two buffers.
- Async usage: see §3 for the mandatory `current_thread` warning.

#### Config knobs

- `hal.spi_set_config(freq_hz, SpiPhase, SpiPolarity)` — clock
  frequency, phase, polarity. Defaults: 1 MHz,
  `SpiPhase::CaptureOnFirstTransition`, `SpiPolarity::IdleLow`
  (mode 0).
- `hal.spi_get_config() -> Result<SpiConfigurationInfo, _>` — read
  back current configuration.

### GPIO subsections (§§6.4–6.7) — read first

**Pin range:** `0..=3` (firmware exposes four pins). Any pin number
outside this range returns `GpioError::InvalidPin`.

**Pin state machine** — at any moment a pin is in exactly one of
these states, and only the listed ops are valid:

| State              | Reached by                            | Allowed ops                                                    |
|--------------------|---------------------------------------|----------------------------------------------------------------|
| Unconfigured       | freshly-booted firmware               | `Gpio::set_config(...)` only                                   |
| Output             | `set_config(GpioDirection::Output,_)` | `OutputPin::set_low/high`, `StatefulOutputPin::is_set_low/high`|
| Input              | `set_config(GpioDirection::Input,_)`  | `InputPin::is_low/high`, async `Wait::wait_for_*`              |
| Subscribed (Input) | `hal.gpio_subscribe(pin, edge)`       | receive `GpioEvent`s; **no other ops** until `gpio_unsubscribe`|

Cross-state transitions: call `set_config(...)` again to flip
direction; call `gpio_unsubscribe(pin)` to leave Subscribed. **A pin
that is Subscribed cannot also be wait()'d on or read/written**: the
firmware returns `GpioError::PinMonitored`.

`use pico_de_gallo_hal::{GpioDirection, GpioPull, GpioEdge};` —
re-exported from `pico-de-gallo-lib` (HAL 0.7+).

### 6.4 GPIO output

**When to use:** drive a line high or low — LED, relay, reset line,
manual CS pin (only if you're **not** using `hal.spi_device(cs)`).

**HAL accessor:** `hal.gpio(pin)` → returns `Gpio`. Pin range
`0..=3`.

**Traits implemented:** `embedded_hal::digital::OutputPin`,
`embedded_hal::digital::StatefulOutputPin`.

#### Snippet — binary form

```rust
// examples/blink.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   sync (reason: trivial blocking loop)
//   peripherals:  gpio(0)
//   hal version:  <crate version observed at generation time>

use embedded_hal::digital::OutputPin;
use pico_de_gallo_hal::Hal;
use pico_de_gallo_hal::{GpioDirection, GpioPull};
use std::time::Duration;

fn main() {
    let hal = Hal::new();
    let mut gpio = hal.gpio(0);
    gpio.set_config(GpioDirection::Output, GpioPull::None).unwrap();

    loop {
        gpio.set_high().unwrap();
        std::thread::sleep(Duration::from_secs(1));
        gpio.set_low().unwrap();
        std::thread::sleep(Duration::from_secs(1));
    }
}
```

#### Gotchas

- Pin range `0..=3`.
- Call `set_config(Output, _)` before driving the pin — pins boot
  unconfigured.
- The CS pin you pass to `hal.spi_device(cs)` is **not** usable here.
  Don't share pin numbers.
- Async usage: see §3 for the mandatory `current_thread` warning.

#### Config knobs

- `Gpio::set_config(GpioDirection, GpioPull)` — direction + internal
  pull resistor. Variants: `GpioDirection::{Input, Output}`,
  `GpioPull::{None, Up, Down}`. Default after firmware boot: input,
  no pull.

### 6.5 GPIO input

**When to use:** poll a digital line — a switch, a status pin, a
strap.

**HAL accessor:** `hal.gpio(pin)` → returns `Gpio`. Pin range
`0..=3`.

**Traits implemented:** `embedded_hal::digital::InputPin`.

#### Snippet — binary form

```rust
// examples/<chip>.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   sync (reason: polled, no edge wait)
//   peripherals:  gpio(1)
//   hal version:  <crate version observed at generation time>

use embedded_hal::digital::InputPin;
use pico_de_gallo_hal::Hal;
use pico_de_gallo_hal::{GpioDirection, GpioPull};

fn main() {
    let hal = Hal::new();
    let mut button = hal.gpio(1);
    button.set_config(GpioDirection::Input, GpioPull::Up).unwrap();

    if button.is_low().unwrap() {
        println!("button pressed");
    }
}
```

#### Snippet — HIL-test form

```rust
#[cfg(feature = "hil")]
#[test]
fn strap_pin_reads_low_with_pullup() {
    use embedded_hal::digital::InputPin;
    use pico_de_gallo_hal::{GpioDirection, GpioPull};
    let hal = pico_de_gallo_hal::Hal::new();
    let mut pin = hal.gpio(2);
    pin.set_config(GpioDirection::Input, GpioPull::Up).unwrap();
    // Strapped low → pull-up loses, pin reads low.
    assert!(pin.is_low().unwrap());
}
```

#### Gotchas

- Pin range `0..=3`.
- Default pull is `None`. For a button-to-ground use
  `GpioPull::Up`; for a button-to-VCC use `GpioPull::Down`.
- For edge-triggered waiting use §6.6; for streamed events §6.7.
- Async usage: see §3 for the mandatory `current_thread` warning.

### 6.6 GPIO async wait

**When to use:** block the current async task until a GPIO edge or
level happens. Common for button presses, IRQ lines from sensors,
SPI BUSY/READY pins. **This subsection is the only reason most
examples need to be async.**

**HAL accessor:** `hal.gpio(pin)` → returns `Gpio`. Pin range
`0..=3`.

**Traits implemented:** `embedded_hal_async::digital::Wait` —
methods `wait_for_high`, `wait_for_low`, `wait_for_rising_edge`,
`wait_for_falling_edge`, `wait_for_any_edge`.

#### Snippet — binary form

```rust
// examples/button.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   async (reason: GPIO Wait trait is async-only)
//   peripherals:  gpio(0)
//   hal version:  <crate version observed at generation time>

use embedded_hal_async::digital::Wait;
use pico_de_gallo_hal::Hal;
use pico_de_gallo_hal::{GpioDirection, GpioPull};

#[tokio::main]               // multi-thread — DO NOT use current_thread
async fn main() {
    let hal = Hal::new();
    let mut button = hal.gpio(0);
    button.set_config(GpioDirection::Input, GpioPull::Up).unwrap();

    loop {
        button.wait_for_falling_edge().await.unwrap();
        println!("pressed");
        button.wait_for_rising_edge().await.unwrap();
        println!("released");
    }
}
```

#### Gotchas

- Pin range `0..=3`.
- The pin must be configured as Input first. The default firmware
  state is unconfigured; call `set_config(Input, _)`.
- The pin **must not** also be Subscribed (§6.7) — pick one
  mechanism per pin.
- `#[tokio::main]` only — see §3 for the mandatory `current_thread`
  warning.
- The `Wait` trait methods wait forever. Production code should
  prefer the bounded inherent methods `Gpio::wait_for_*_with_timeout(Duration)`
  (HAL 0.7+) which return `GpioHalError::Gpio(GpioError::Timeout)`
  on expiry. The trait methods exist for compatibility with
  embedded-hal drivers; the inherent methods exist because
  `embedded-hal-async::Wait` does not accept a timeout.

### 6.7 GPIO subscribe (push events)

**When to use:** the application needs a stream of edge events from
a pin (rotary encoders, repeated button events) without polling and
without holding an async `wait_for_*` future per edge.

**HAL accessors:**

- `hal.gpio_subscribe(pin, edge)` → `Result<(), GpioHalError>` —
  start firmware-side monitoring. `edge` is `GpioEdge::Rising`,
  `Falling`, or `Any`.
- `hal.gpio_unsubscribe(pin)` → `Result<(), GpioHalError>` — stop
  monitoring and return the pin to ordinary use.

Push events arrive on the `"gpio/event"` topic as `GpioEvent` values.
**Receiving them from the HAL is not currently exposed as a typed
stream**; for end-to-end push handling you may need to drop down to
`pico_de_gallo_lib::PicoDeGallo` directly.

**Traits implemented:** n/a (project-specific).

#### Snippet — binary form

```rust
// examples/<chip>.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   sync (reason: subscribe/unsubscribe are blocking calls)
//   peripherals:  gpio_subscribe(2)
//   hal version:  <crate version observed at generation time>

use pico_de_gallo_hal::Hal;
use pico_de_gallo_hal::{GpioDirection, GpioEdge, GpioPull};

fn main() {
    let hal = Hal::new();

    // Configure the pin as Input first.
    let mut pin = hal.gpio(2);
    pin.set_config(GpioDirection::Input, GpioPull::Up).unwrap();
    drop(pin);  // release the Gpio handle so the subscription owns it

    hal.gpio_subscribe(2, GpioEdge::Falling).unwrap();
    // Consume GpioEvents via pico_de_gallo_lib::PicoDeGallo if needed.
    // ... eventually:
    hal.gpio_unsubscribe(2).unwrap();
}
```

#### Gotchas

- Pin range `0..=3`.
- While subscribed, the pin **cannot** be `is_low`/`is_high`/`set_*`/`wait_for_*`'d.
  Those calls return `GpioError::PinMonitored`. Call
  `hal.gpio_unsubscribe(pin)` first.
- If a previous host process died while holding a subscription,
  subsequent GPIO operations on that pin fail with `PinMonitored`
  until the firmware is reset. **Recovery:** call
  `hal.system_reset_subscriptions()?` after connect (recommended on
  every `Hal::new_validated()` flow). As of HAL 0.7, the recovery
  endpoint is exposed directly; older HALs required dropping to
  `pico-de-gallo-lib`. Power-cycling the board also works.
- The HAL does not currently surface push events as a stream. For
  full producer/consumer behavior you'll need
  `pico-de-gallo-lib` directly.

### 6.8 PWM

**When to use:** the device needs a duty-cycle waveform — motor
ESCs, dimmable LEDs, servo control.

**HAL accessor:** `hal.pwm_channel(channel)` → returns `PwmChannel`.
Channels `0..=3` (channels 0/1 share slice 6 on GPIO 12/13, channels
2/3 share slice 7 on GPIO 14/15).

```rust
use embedded_hal::pwm::SetDutyCycle;
use pico_de_gallo_hal::Hal;

fn main() {
    let mut hal = Hal::new();
    hal.pwm_set_config(0, 1_000, false).unwrap();  // 1 kHz, edge-aligned
    let mut ch = hal.pwm_channel(0);
    ch.set_duty_cycle_percent(50).unwrap();
}
```

Gotchas: `pwm_set_config(channel, freq_hz, phase_correct)` affects
the whole slice — channels 0/1 cannot use different frequencies, and
neither can 2/3. Async usage: see §3 for the mandatory
`current_thread` warning. See
[`pico-de-gallo-hal` docs.rs](https://docs.rs/pico-de-gallo-hal) for
the full surface.

### 6.9 ADC

**When to use:** read an analog input — potentiometer, thermistor,
battery voltage.

**HAL accessor:** `hal.adc_read(channel)` →
`Result<u16, AdcHalError>`. There is **no `embedded-hal` 1.0 ADC
trait**, so this is a project-specific method.

**Traits implemented:** n/a.

```rust
use pico_de_gallo_hal::{AdcChannel, Hal}; // AdcChannel re-exported by HAL 0.7+

fn main() {
    let hal = Hal::new();
    let raw = hal.adc_read(AdcChannel::Adc0).unwrap();
    let volts = raw as f32 * 3.3 / 4096.0;
    println!("raw: {raw}, V≈{volts:.3}");
}
```

#### Snippet — HIL-test form

```rust
#[cfg(feature = "hil")]
#[test]
fn adc0_reads_in_valid_range() {
    use pico_de_gallo_hal::AdcChannel;
    let hal = pico_de_gallo_hal::Hal::new();
    let raw = hal.adc_read(AdcChannel::Adc0).unwrap();
    assert!(raw <= 4095, "12-bit ADC must be ≤ 4095, got {raw}");
}
```

#### Gotchas

- `AdcChannel` is re-exported by `pico_de_gallo_hal` as of HAL 0.7.
  Older HALs required `use pico_de_gallo_lib::AdcChannel;`.
- 12-bit raw value (`0..=4095`). Convert with `raw × 3.3 / 4096`.
- Use `hal.adc_get_config()` for resolution/reference details
  (returns `AdcConfigurationInfo`, also re-exported as of HAL 0.7).
- Async usage: see §3 for the mandatory `current_thread` warning.

### 6.10 1-Wire

**When to use:** the device is on a Dallas/Maxim 1-Wire bus — DS18B20
temperature sensor, DS2401 silicon serial number, etc.

**HAL accessor:** `hal.onewire()` → returns `OneWire`. No
`embedded-hal` 1-Wire trait exists.

**Traits implemented:** n/a.

```rust
use pico_de_gallo_hal::Hal;

fn main() {
    let hal = Hal::new();
    let ow = hal.onewire();

    let present = ow.reset().unwrap();
    println!("device present: {present}");

    // Issue ROM-skip + convert-T to all devices on the bus.
    ow.write(&[0xCC, 0x44]).unwrap();
}
```

Available methods: `reset()`, `read(len)`, `write(data)`,
`write_pullup(data, pullup_ms)`, `search()`, `search_next()`.

#### Snippet — HIL-test form

```rust
#[cfg(feature = "hil")]
#[test]
fn onewire_bus_has_at_least_one_device() {
    let hal = pico_de_gallo_hal::Hal::new();
    let ow = hal.onewire();
    assert!(ow.reset().unwrap(), "no 1-Wire device responded");
}
```

#### Gotchas

- Enumerate devices with `search()` for the first address, then
  `search_next()` until it returns `None`.
- Parasitic-power parts (DS18B20 in 2-wire mode) need
  `write_pullup(data, pullup_ms)` to hold the line high after the
  convert command.
- Async usage: see §3 for the mandatory `current_thread` warning.

### 6.11 UART

**When to use:** the device speaks serial — a GPS module, a
debug-port-on-UART chip, AT-command modem.

**HAL accessor:** `hal.uart()` → returns `Uart`. Implements
`embedded_io::{Read,Write}` and `embedded_io_async::{Read,Write}`.
The `embedded-io-06` Cargo feature remains enabled by default, while
`embedded-io-07` can be enabled additively when a driver targets 0.7
or both majors must be supported.

```rust
use embedded_io::Write as _;
use pico_de_gallo_hal::Hal;

fn main() {
    let hal = Hal::new();
    let mut uart = hal.uart();
    uart.write_all(b"AT\r\n").unwrap();
}
```

Gotchas: read uses a timeout (default 1000 ms). Set with
`uart.set_timeout_ms(0)` for non-blocking. **Baud rate is fixed at
the firmware default. The HAL does not expose a baud-rate setter** —
to change baud, depend on `pico-de-gallo-lib` and call
`PicoDeGallo::uart_set_config(...)` directly. Async usage: see §3
for the mandatory `current_thread` warning. See
[`pico-de-gallo-hal` docs.rs](https://docs.rs/pico-de-gallo-hal) for
the full surface.

### 6.12 Delay

**When to use:** the driver wants `&mut impl DelayNs` for
register-settle delays, reset sequences, sensor warm-up.

**HAL accessor:** `hal.delay()` → returns `Delay`. Implements
`embedded_hal::delay::DelayNs` and
`embedded_hal_async::delay::DelayNs`.

```rust
use pico_de_gallo_hal::Hal;

fn main() {
    let hal = Hal::new();
    let i2c = hal.i2c();
    let mut delay = hal.delay();
    let mut sensor = MyDriver::new(i2c);
    sensor.init(&mut delay).unwrap();
}
```

Gotchas: blocking `Delay` uses `std::thread::sleep`; async `Delay`
uses `tokio::time::sleep`. Pass `&mut delay` by-mutable-reference to
driver methods. See
[`pico-de-gallo-hal` docs.rs](https://docs.rs/pico-de-gallo-hal) for
the full surface.

## 7. Worked end-to-end example

A complete `examples/shtc3.rs` for the Sensirion SHTC3 temperature
+ humidity sensor. The four-line decision log at the top is
**mandatory** in every generated example — same prefixes, same
order, four lines exactly.

```rust
// examples/shtc3.rs
// pico-de-gallo decision log:
//   shape:        binary
//   sync/async:   sync (reason: shtcx is blocking; no edge wait needed)
//   peripherals:  i2c, delay
//   hal version:  0.6

use pico_de_gallo_hal::Hal;
use shtcx::{shtc3, PowerMode};

fn main() {
    let hal = Hal::new();
    let i2c = hal.i2c();           // §6.1
    let mut delay = hal.delay();   // §6.12

    let mut sensor = shtc3(i2c);
    let m = sensor.measure(PowerMode::NormalMode, &mut delay).unwrap();

    println!(
        "{:.2} °C / {:.2} %RH",
        m.temperature.as_degrees_celsius(),
        m.humidity.as_percent(),
    );
}
```

Decision-log format — **literal, four lines, exact prefixes**:

```text
// pico-de-gallo decision log:
//   shape:        binary | hil-test
//   sync/async:   sync | async (reason: …)
//   peripherals:  i2c, gpio(3)
//   hal version:  <crate version observed at generation time>
```

Free-form variants are not permitted. The fixed format is
grep-checkable by the maintainer drift-prevention hooks (see §9).

## 8. Completion checklist

Before declaring the example done, verify every one of these:

- [ ] The `Cargo.toml` entries from §4 are present and correct.
- [ ] The chosen output shape (binary vs HIL test) matches the
      user's intent per §2.
- [ ] The sync/async choice follows §3, and if async, the runtime is
      multi-thread (`#[tokio::main]` with **no** `current_thread`
      flavor).
- [ ] Every HAL accessor used appears in the approved list below
      (i.e. is one of `hal.i2c`, `hal.spi`, `hal.spi_device`,
      `hal.gpio`, `hal.gpio_subscribe`, `hal.gpio_unsubscribe`,
      `hal.pwm_channel`, `hal.adc_read`, `hal.uart`, `hal.delay`,
      `hal.onewire`, or one of their associated `*_set_config`/
      `*_get_config` siblings). No invented APIs.
- [ ] Pin numbers are in range: GPIO `0..=3`, PWM channel `0..=3`,
      ADC channel is an `AdcChannel` enum variant.
- [ ] If using `spi_device(cs)`, the `cs` pin is **not** also used
      as a `Gpio` elsewhere.
- [ ] If using `gpio_subscribe(pin, …)`, a matching
      `gpio_unsubscribe(pin)` exists on every exit path.
- [ ] The mandatory four-line decision-log comment block is at the
      top of the example, with the literal prefixes from §7.
- [ ] If the host can run `cargo check` against the generated file,
      it passes.

## 9. Drift-prevention note (for maintainers)

This file documents the public surface of `pico-de-gallo-hal`. It is
listed in `AGENTS.md` §15.1 as a parity target for
`crates/pico-de-gallo-hal/src/`. Any PR that changes the HAL public
surface (accessor added/removed/renamed, gotcha discovered, trait
impl changed) must update this file in the same commit. Reviewers
treat omissions as a blocker, not a nit.

**Recommended CI guard (not yet implemented):** a regex check that
extracts every `hal\.\w+\(` and `Hal::\w+\b` from this file and
asserts the symbol exists as a `pub fn` in
`crates/pico-de-gallo-hal/src/lib.rs`. A one-liner that catches the
common drift mode:

```bash
diff <(grep -oE '(hal\.[a-z0-9_]+\()|(Hal::[a-z0-9_]+\b)' \
        docs/ai-agents/pico-de-gallo-hal-examples.md | sort -u) \
     <(grep -oE 'pub fn [a-z0-9_]+' \
        crates/pico-de-gallo-hal/src/lib.rs | sort -u)
```

If the file disagrees with the source, the **source wins** — file an
issue at
<https://github.com/OpenDevicePartnership/pico-de-gallo/issues>.
