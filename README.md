[![check](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/check.yml/badge.svg)](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/check.yml)
[![no-std](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/nostd.yml/badge.svg)](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/nostd.yml)
[![book](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/gh-pages.yml/badge.svg)](https://github.com/OpenDevicePartnership/pico-de-gallo/actions/workflows/gh-pages.yml)
[![crates.io](https://img.shields.io/crates/v/pico-de-gallo-internal?label=pico-de-gallo-internal)](https://crates.io/crates/pico-de-gallo-internal)
[![crates.io](https://img.shields.io/crates/v/pico-de-gallo-lib?label=pico-de-gallo-lib)](https://crates.io/crates/pico-de-gallo-lib)
[![crates.io](https://img.shields.io/crates/v/pico-de-gallo-hal?label=pico-de-gallo-hal)](https://crates.io/crates/pico-de-gallo-hal)
[![crates.io](https://img.shields.io/crates/v/pico-de-gallo-ffi?label=pico-de-gallo-ffi)](https://crates.io/crates/pico-de-gallo-ffi)
[![crates.io](https://img.shields.io/crates/v/gallo?label=gallo)](https://crates.io/crates/gallo)
[![crates.io](https://img.shields.io/crates/v/gallo-mcp?label=gallo-mcp)](https://crates.io/crates/gallo-mcp)

# Pico de Gallo

A USB bridge that turns a [Raspberry Pi Pico 2](https://www.raspberrypi.com/products/raspberry-pi-pico-2/)
into a host-accessible I²C, SPI, UART, GPIO, PWM, ADC, and 1-Wire interface. Write and test
embedded Rust drivers on your development machine without cross-compiling
or flashing firmware to a target board.

## Overview

Pico de Gallo provides:

- **I²C**: read, write, write-then-read, bus scanning, and transaction batching
- **SPI**: read, write, full-duplex transfer, write-then-read, and transaction batching
- **UART**: read, write, flush, and baud rate configuration
- **GPIO**: digital input/output, edge event monitoring (subscribe/unsubscribe)
- **PWM**: 4-channel PWM output with configurable frequency and duty cycle
- **ADC**: 4 GPIO-based analog inputs (12-bit)
- **1-Wire**: bus reset, read, write, strong pullup, and ROM search (PIO hardware)
- **Configuration**: runtime I²C/SPI/UART/PWM frequency and SPI mode changes

All communication uses [postcard-rpc](https://docs.rs/postcard-rpc) over
USB — a compact, typed, binary RPC protocol.

## Book

The [Pico de Gallo Book](https://felipebalbi.github.io/pico-de-gallo/)
covers hardware assembly, firmware flashing, and a step-by-step guide to
writing an embedded device driver using Pico de Gallo.

## Crates

| Crate | Description |
|-------|-------------|
| [`pico-de-gallo-firmware`](crates/pico-de-gallo-firmware) | Embassy-rs firmware for the RP2350 |
| [`pico-de-gallo-lib`](crates/pico-de-gallo-lib) | Async host library (requires tokio) |
| [`pico-de-gallo-hal`](crates/pico-de-gallo-hal) | `embedded-hal` + `embedded-hal-async` implementation |
| [`pico-de-gallo-ffi`](crates/pico-de-gallo-ffi) | C FFI bindings (shared library + header) |
| [`gallo`](crates/pico-de-gallo-app) | CLI application for batch-mode access |
| [`gallo-mcp`](crates/pico-de-gallo-mcp) | MCP server exposing the device to AI agents |
| [`pico-de-gallo-internal`](crates/pico-de-gallo-internal) | Shared wire-protocol types (internal) |

### Firmware

Embassy-rs firmware for the RP2350 that exposes I²C, SPI, UART, and GPIO
peripherals over USB via postcard-rpc endpoints.

### Library

Async host-side library wrapping the postcard-rpc transport. Provides
typed methods for every firmware endpoint. Requires the tokio runtime.

### HAL

Implements both `embedded-hal` and `embedded-hal-async` traits — including
`I2c`, `SpiBus`, `SpiDevice`, GPIO digital I/O, delay, and `embedded-io`
Read/Write for UART — so embedded device drivers written against those traits
can be tested on a host machine with real hardware attached to the Pico de Gallo.

### App

Command-line tool (`gallo`) for interactive and batch-mode I²C/SPI/UART/GPIO
access. Supports hex, binary, and ASCII output formats.

### FFI

C-compatible shared library wrapping the Rust host library. Generates a
C header via cbindgen. Suitable for integration into C/C++/Python/etc.
projects.

### Internal

Shared wire-protocol crate defining all postcard-rpc endpoints, request
and response types, and constants. Used by both firmware and host crates.

## Hardware

KiCAD schematic and PCB design for a Pico 2 daughter board. The v1.1
board features a single keyed 2x12 shrouded header that routes all 20
firmware signals with on-board I²C pull-ups and ADC input protection.

## Case

3D-printable snap-fit enclosure (FreeCAD) in two parts — body and lid.
