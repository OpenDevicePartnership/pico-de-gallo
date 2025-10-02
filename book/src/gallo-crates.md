# Gallo Crates

*Pico de Gallo* provides several different crates to provide a
comfortable and ergonomic experience to its users. The most relevant
crates are discussed below.

## Gallo App

The `gallo` app's main purpose is that of one-off, or batch-mode
communication with *Pico de Gallo*'s I<sup>2</sup>C and SPI buses. The
built-in help text gives us a little more information.

```console
$ gallo help
Access I2C/SPI devices through Pico De Gallo

Usage: gallo.exe [OPTIONS] [COMMAND]

Commands:
  version     Get firmware version
  i2c         I2C access methods
  spi         SPI access methods
  set-config  Set bus parameters for I2C and SPI
  help        Print this message or the help of the given subcommand(s)

Options:
  -s, --serial-number <SERIAL_NUMBER>
  -h, --help                           Print help
  -V, --version                        Print version
```

If you have more than one *Pico de Gallo* attached to your host
computer, the `gallo` app will always attempt to use the first device
it finds. Because that's non-deterministic, you can specify the exact
device you want by using the `-s, --serial-number` option.

Additionally, as noted in the help text itself, we can request for
help from a specific command, for example:

```console
$ gallo i2c help
I2C access methods

Usage: gallo.exe i2c [COMMAND]

Commands:
  scan        Scan I2C bus for existing devices
  read        Read bytes through the I2C bus from device at given address
  write       Write bytes through I2C bus to device at given address
  write-read  Write bytes follwed by read bytes
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### Communicating via I<sup>2</sup>C

The `i2c` subcommand groups all I<sup>2</sup>C functionality. We
support a regular `read`, `write`, and `write-read`
operation. Additionally, a minimal `scan` facility is implemented.

#### Scanning

> [!WARNING]
>
> Due to limitations on the I<sup>2</sup>C controller HW present on
> the RP235x MCU, scanning must be carried with read probing. That is,
> *Pico de Gallo* will intiate a 1-byte read from each and every
> address on the bus. Those which respond are considered present.
>
> This could result in unexpected side-effects on some I<sup>2</sup>C
> devices which could put the device in an unknown state afterwards.

If you have one or more I<sup>2</sup>C devices attached to *Pico de
Gallo* and would like to *discover* their addresses, you can use the
`scan` facility.

```console
$ gallo i2c scan
╭────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────╮
│    │  0 │  1 │  2 │  3 │  4 │  5 │  6 │  7 │  8 │  9 │  a │  b │  c │  d │  e │  f │
├────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┤
│ 0  │ RR │ RR │ RR │ RR │ RR │ RR │ RR │ RR │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 1  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 2  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 3  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 4  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 5  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 6  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ 68 │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 7  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ RR │ RR │ RR │ RR │ RR │ RR │ RR │ RR │
╰────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────╯
```

The `RR` annotation here means that those addresses are reserved and
shouldn't be accessed. You can request *Pico de Gallo* to access them
anyway by passing the `-r` argument.

```console
$ gallo i2c scan -r
╭────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────┬────╮
│    │  0 │  1 │  2 │  3 │  4 │  5 │  6 │  7 │  8 │  9 │  a │  b │  c │  d │  e │  f │
├────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┼────┤
│ 0  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 1  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 2  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 3  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 4  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 5  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 6  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ 68 │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
│ 7  │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │ -- │
╰────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────┴────╯
```

#### Reading

Reading requires the target device address &mdash; you can find that
out with `scan` &mdash; and a `count` of how many bytes you want to
read.

```console
$ gallo i2c read --address 0x68 -c 10
85 7f 00 51 bd 2c eb 22 fb 8f
```

#### Writing

Similarly, writing requires the target device address. However,
instead of a `count` of bytes to read, writing wants the bytes to be
written.

```console
$ gallo i2c write --address 0x68 --bytes 0x00 0x01 0x02 0x03 0x04
```

#### Write-Then-Read

It's the same as writing followed by reading.

```console
$ gallo i2c write-read --address 0x68 --bytes 0x00 -c 10
85 7f 00 51 bd 2c eb 22 fb 8f
```

### Communicating via SPI

Exactly the same as I<sup>2</sup>C but without the extra `address`
option.

> [!NOTE]
>
> At the time of this writing, the app does not **yet** support
> controlling GPIOs, meaning that if you have a SPI device attached to
> *Pico de Gallo* that only responds when a *CS* pin is pulled low,
> then you must use either *pico-de-gallo-hal* or *pico-de-gallo-lib*,
> both of which will be discussed later.

```console
$ gallo spi help
SPI access methods

Usage: gallo.exe spi [COMMAND]

Commands:
  read        Read bytes through SPI bus
  write       Write bytes through SPI bus
  write-read  Write bytes followed by read bytes
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

#### Reading

```console
$ gallo spi read --count 10
00 00 00 00 00 00 00 00 00 00
```

#### Writing

```console
$ gallo spi write --bytes 0x00 0x01 0x02 0x03 0x04 0x05
```

#### Write-Then-Read

```console
$ gallo spi write-read --bytes 0x00 0x01 0x02 0x03 0x04 0x05 --count 10
00 00 00 00 00 00 00 00 00 00
```


## Gallo Lib

The library implements a series of methods to communicate with the
*Pico de Gallo* device, in fact the app described in the previous
chapter relies on the library for much of its functionality.

We are given two public constructors for *Pico de Gallo* devices:

- `new()`: attempts to find a *Pico de Gallo* and opens a connection
  to the first one it finds;
- `new_with_serial_number()`: attempts to find a *Pico de Gallo* with
  the given *serial number* and opens a connection to it.
  
Because serial numbers are guaranteed to be unique,
`new_with_serial_number()` will always find the exact device you're
looking for if that is connected to your host computer.

Once a connection is successfully initiated, we get access to all
endpoints exposed by the firmware. The table below provides a summary.

| **Endpoint**                 | **Arguments**                                                 | **Descrition**                                                             |
|------------------------------|---------------------------------------------------------------|----------------------------------------------------------------------------|
| `ping`                       | `id`                                                          | Sends the `id` to *Pico de Gallo* and waits for a response.                |
| `i2c_read`                   | `address`, `count`                                            | Attempts to read `count` bytes from the I<sup>2</sup>C device at `address` |
| `i2c_write`                  | `address`, `contents`                                         | Attempts to write `contents` to the  I<sup>2</sup>C device at `address`    |
| `spi_read`                   | `count`                                                       | Attempts to read `count` bytes via SPI                                     |
| `spi_write`                  | `contents`                                                    | Attempts to write `contents` via SPI                                       |
| `spi_flush`                  |                                                               | Flushes the SPI bus                                                        |
| `gpio_get`                   | `pin`                                                         | Reads the current state of the GPIO #`pin`                                 |
| `gpio_put`                   | `pin`, `state`                                                | Sets the state of GPIO #`pin` to `state`                                   |
| `gpio_wait_for_high`         | `pin`                                                         | Waits until GPIO #`pin` reaches a high state                               |
| `gpio_wait_for_low`          | `pin`                                                         | Waits until GPIO #`pin` reaches a low state                                |
| `gpio_wait_for_rising_edge`  | `pin`                                                         | Waits until a rising edge is seen on GPIO #`pin`                           |
| `gpio_wait_for_falling_edge` | `pin`                                                         | Waits until a falling edge is seen on GPIO #`pin`                          |
| `gpio_wait_for_any_edge`     | `pin`                                                         | Waits until any edge is seen on GPIO #`pin`                                |
| `set_config`                 | `i2c_frequency`, `spi_frequency`, `spi_phase`, `spi_polarity` | Sets configuration parameters for I<sup>2</sup>C and SPI buses             |
| `version`                    |                                                               | Reads the current Firmware version from *Pico de Gallo*                    |

## Gallo Hal

`pico-de-gallo-hal` implements all `embedded-hal` and
`embedded-hal-async` traits. Because of that, we can write peripheral
drivers polymorphic on those traits and easily test them without
having to setup an entire MCU platform. That is, we can skip linker
scripts, `probe-rs`, clock configuration, pin mux configuration, and a
lot more by relying on *Pico de Gallo* to do the heavy-lifting.

To clarify what we mean, a driver *crate* would add
`pico-de-gallo-hal` as a dev dependency and use it for tests and
examples.

Here's a minimalistic example of how to use `pico-de-gallo-hal` to
communicate with an I<sup>2</sup>C device.

```rust,noplayground
use embedded_hal::i2c::I2c;
use pico_de_gallo_hal::Hal;

struct Driver<I2C: I2c>;

fn main() {
    let hal = Hal::new();
    let i2c = hal.i2c();
    let delay = hal.Delay();

    let mut drv = Driver::new(i2c, delay);

    println!("Device ID: {:02x}", drv.read_manufacturer_id());
}
```
