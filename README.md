[![check](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/check.yml/badge.svg)](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/check.yml)
[![no-std](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/nostd.yml/badge.svg)](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/nostd.yml)

# Pico de Gallo

A collection of tools that make it easier to write and test embedded
rust drivers for discrete I2C/SPI components.

## Crates

All relevant crates are housed in this folder: firmware, app, lib, a C
FFI, etc.

### Firmware

The firmware proper to run on Pico de Gallo hardware. Written using
`embassy` and `postcard-rpc`, it provides a REST-style endpoint-based
API for communication with the host.

### Lib

The host-side library crate to communicate with Pico de Gallo using
the REST-style API described previously. The library depends on the
`tokio` async runtime.

### Internal

A library crate shared between Firmware and Lib. This library defined
all endpoints, request types, response types, serialization, and
deserialization schemes.

### App

An application using `pico-de-gallo-lib` for batch-style communication
with Pico de Gallo.

### HAL

This library crate implements both `embedded-hal` and
`embedded-hal-async` traits to make it easy to write and validate rust
embedded drivers for I2C and SPI devices. GPIO traits are also
supported in case there's a need for them.

### FFI

Originated from an unexpected need, this library binds
`pico-de-gallo-lib` to C environments and provides access to all
endpoints exposed by that.

## Hardware

This folder contains the schematic and PCB design, designed with
[KiCAD](https://kicad.org), for a daughterboard with a footprint for a
regular Pico2 board.

## Case

This folder contains a 3D printable case, designed with
[FreeCAD](https://freecad.org), that can house the board. The case is
designed in two parts &mdash; body and lid &mdash; which snap together.

A simple tool for testing Embedded Rust drivers for discrete I2C/SPI components.
