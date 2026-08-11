# pico-de-gallo Zephyr Module

This README describes the `pico-de-gallo/zephyr` Zephyr module, including
how to download it and test it out. I put this README here instead of the
main `pico-de-gallo/book` directory because this module is still kind of in
a work-in-progress state (and may not even live in this repo in the long term).

Anyhow, `pico-de-gallo/zephyr` is a Zephyr module that allows Zephyr applications
to run on the pico-de-gallo platform. It provides the `pico_de_gallo` [shield](https://docs.zephyrproject.org/latest/hardware/porting/shields.html),
which runs on `native-sim` (default is `native-sim/native/64`) to link the Zephyr API to the `pico-de-gallo-ffi` API. For
some example applications, see the `pico-de-gallo/zephyr/samples` directory.

## Installation

The most straightforward way to install, test out, and set up development for
`pico-de-gallo/zephyr` is to clone this repo into your Zephyr workspace:
```
# in my-workspace/ or whatever your workspace is called
git clone -b zephyr git@github.com:bjackson312006/pico-de-gallo.git
cd pico-de-gallo/zephyr
```

If you already have `pico-de-gallo` cloned somewhere else you could technically just pull this fork
from there, but being outside of a Zephyr workspace makes it kind of annoying to build/run stuff.

Note that this installation process could be much more streamlined in the future, this is just
for those interested in developing in `pico-de-gallo/zephyr` directly. For someone looking to just
use pico-de-gallo on Zephyr in general (with this project as an external Zephyr module they pull in), I'm pretty
sure a syntax somewhat like this could be used:
```
west init -m https://github.com/OpenDevicePartnership/pico-de-gallo --mr zephyr-v0.1.0 workspace
west update
```
This could probably be even more turnkey depending on the method of upstreaming or publishing.

## Testing with Samples

As of writing this, `pico-de-gallo/zephyr` supports I2C and SPI devices. So, there's some sample applications
that can be built and ran with a pico-de-gallo board.

For example, to run the `i2c_bridge` sample, just `cd` into `pico-de-gallo/zephyr/samples/i2c_bridge` and run:
```
west build -t run
```
Or if you need to clean build for whatever reason:
```
west build -p always -t run
```

For the `i2c_bridge` sample, any checkout of the main `zephyr` repo in your workspace should work. However, for
`spi_bridge` and `combined_i2c_spi_bridge`, you'll need to pull the branch on `bjackson312006/zephyr` that
has the IS31FL3743B LED Matrix driver (or ideally if my driver gets merged, you'd just need to pull the
latest Zephyr `main`).

If interested, you can also create your own samples if you want with whatever I2C/SPI devices you have on hand. As demonstrated
in `samples/i2c_bridge`, `samples/spi_bridge`, and `samples/combined_i2c_spi_bridge`, you aren't just limited
to raw calls to the Zephyr I2C amd SPI APIs. You can use the higher-level device-specific APIs (like the sensor
API, LED API, and probably also the fuel gauge API) as long as the underlying device is a I2C/SPI device.