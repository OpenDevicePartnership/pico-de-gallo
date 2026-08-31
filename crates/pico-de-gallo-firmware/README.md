# Pico de Gallo Firmware

Embassy-rs firmware for the Raspberry Pi Pico 2 (RP2350) that turns it into
a USB bridge for I2C, SPI, and GPIO access.

## Building

```console
$ rustup target add thumbv8m.main-none-eabihf
```

For rev2 hardware (v1.1 and later) — this is the default:

```console
$ cargo build --release --manifest-path crates/pico-de-gallo-firmware/Cargo.toml --target thumbv8m.main-none-eabihf
```

For rev1 hardware (v1.0):

```console
$ cargo build --release --manifest-path crates/pico-de-gallo-firmware/Cargo.toml --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1
```

> **`hw-rev1` is deprecated.** It is no longer the default and must be
> requested explicitly. It stays fully supported and built in CI, and
> **will not be removed before 2031-09-01**. Enabling it prints a
> `cargo:warning` at build time and logs a `defmt::warn!` at every boot.
> On rev1 the UART, ADC, and 1-Wire endpoints return `Unsupported`,
> because those signals are not routed on the v1.0 landing board.

## Flashing

Convert the ELF to UF2 and copy to the Pico 2 in BOOTSEL mode:

```console
$ picotool uf2 convert crates/pico-de-gallo-firmware/target/thumbv8m.main-none-eabihf/release/pico-de-gallo-firmware -t elf firmware.uf2
$ picotool load firmware.uf2
```

Or simply drag `firmware.uf2` onto the `RP2350` USB mass storage drive.

## Peripheral Mapping

| Function | RP2350 Pins | Notes                        |
|----------|-------------|------------------------------|
| I2C1 SDA | GPIO 2      | 7-bit addressing, async mode |
| I2C1 SCL | GPIO 3      |                              |
| SPI0 SCK | GPIO 6      | DMA-backed full-duplex       |
| SPI0 TX  | GPIO 7      |                              |
| SPI0 RX  | GPIO 4      |                              |
| GPIO 0–3 | GPIO 8–11   | Input/output/edge-wait       |
| PWM 0–3  | GPIO 12–15  | PWM slices 6–7               |
| USB      | Native USB  | postcard-rpc transport       |

## License

Licensed under the terms of the [MIT license](http://opensource.org/licenses/MIT).

## Contribution

Any contribution intentionally submitted for inclusion in the work by
you shall be licensed under the terms of the same MIT license, without
any additional terms or conditions.
