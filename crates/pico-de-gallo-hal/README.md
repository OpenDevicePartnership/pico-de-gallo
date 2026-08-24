# Pico de Gallo HAL

[![crates.io](https://img.shields.io/crates/v/pico-de-gallo-hal.svg)](https://crates.io/crates/pico-de-gallo-hal)
[![docs.rs](https://docs.rs/pico-de-gallo-hal/badge.svg)](https://docs.rs/pico-de-gallo-hal)

[embedded-hal](https://crates.io/crates/embedded-hal) and
[embedded-hal-async](https://crates.io/crates/embedded-hal-async)
implementation backed by a [Pico de Gallo](https://github.com/OpenDevicePartnership/pico-de-gallo)
USB bridge.

Run embedded Rust drivers on your host machine by forwarding I²C, SPI,
GPIO, and delay operations to real hardware via USB.

## Quick Start

```rust
use pico_de_gallo_hal::Hal;
use embedded_hal::i2c::I2c;

let hal = Hal::new();
let mut i2c = hal.i2c();

let mut buf = [0u8; 2];
i2c.write_read(0x48, &[0x00], &mut buf).unwrap();
```

Check out the
[examples](https://github.com/OpenDevicePartnership/pico-de-gallo/tree/main/crates/pico-de-gallo-hal/examples)
for more usage patterns.

## Implemented Traits

| Peripheral | Blocking                                     | Async                 |
|------------|----------------------------------------------|-----------------------|
| GPIO       | `OutputPin`, `InputPin`, `StatefulOutputPin` | `Wait`                |
| I²C       | `I2c`                                        | `I2c`                 |
| SPI        | `SpiBus`, `SpiDevice`                        | `SpiBus`, `SpiDevice` |
| Delay      | `DelayNs`                                    | `DelayNs`             |

`SpiDevice` manages chip-select (CS) automatically via a GPIO pin.
Use `hal.spi_device(cs_pin)` to create one. It returns
`Result<SpiDev, SpiHalError>` after validating the pin against the
device-reported GPIO count. For raw bus access without CS management,
use `hal.spi()`.

# License

Licensed under the terms of the [MIT license](http://opensource.org/licenses/MIT).

# Contribution

Any contribution intentionally submitted for inclusion in the work by
you shall be licensed under the terms of the same MIT license, without
any additional terms or conditions.
