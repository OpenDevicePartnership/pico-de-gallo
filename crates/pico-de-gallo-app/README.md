# Gallo

[![crates.io](https://img.shields.io/crates/v/gallo.svg)](https://crates.io/crates/gallo)

Batch mode application to communicate with a *Pico de Gallo* device.

# Usage

`gallo` is built with [clap](https://crates.io/crates/clap),
therefore, the built-in help is as useful as possible.

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

# License

Licensed under the terms of the MIT license
(http://opensource.org/licenses/MIT).

# Contribution

Any contribution intentionally submitted for inclusion in the work by
you shall be licensed under the terms of the same MIT license, without
any additional terms or conditions.
