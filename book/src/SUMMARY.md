# Summary

[Introduction](./introduction.md)

# Part I — The Hardware

- [Hardware Overview](./hardware/overview.md)
- [Revisions: v1.0 vs v1.1](./hardware/revisions.md)
- [Pinout & Connector](./hardware/pinout.md)
- [Assembly & Flashing](./hardware/assembly.md)

# Part II — Getting Started

- [Installing the Toolchain](./getting-started/install.md)
- [Verifying Your Device](./getting-started/verify.md)
- [USB & OS Notes](./getting-started/usb.md)

# Part III — The Interfaces

- [I²C](./interfaces/i2c.md)
- [SPI](./interfaces/spi.md)
- [UART](./interfaces/uart.md)
- [GPIO](./interfaces/gpio.md)
- [PWM](./interfaces/pwm.md)
- [ADC](./interfaces/adc.md)
- [1-Wire](./interfaces/onewire.md)
- [Transaction Batching](./interfaces/batching.md)

# Part IV — The Crates

- [Crate Map](./crates/overview.md)
- [`gallo` CLI](./crates/app.md)
- [`pico-de-gallo-lib`](./crates/lib.md)
- [`pico-de-gallo-hal`](./crates/hal.md)
- [`pico-de-gallo-ffi`](./crates/ffi.md)
- [`pyco-de-gallo`](./crates/python.md)
- [`gallo-mcp` MCP server](./crates/mcp.md)

# Part V — Writing a Device Driver

- [Overview](./driver/index.md)
- [Exploring with `gallo`](./driver/explore.md)
- [Scaffolding the Crate](./driver/scaffold.md)
- [Describing the Device in DDSL](./driver/codegen.md)
- [Implementing the Register Interface](./driver/register-interface.md)
- [Designing an Ergonomic API](./driver/ergonomic-api.md)
- [Testing with `pico-de-gallo-hal`](./driver/testing.md)
- [Blocking vs Async Parity](./driver/blocking-vs-async.md)
- [Publishing Your Driver](./driver/publishing.md)

# Part VI — Under the Hood

- [Architecture](./internals/architecture.md)
- [Wire Protocol & Schema Versioning](./internals/wire-protocol.md)
- [The Firmware](./internals/firmware.md)
- [Releases & Compatibility](./internals/releases.md)

---

# Appendices

- [Status Code Reference](./appendix/status-codes.md)
- [Endpoint Catalog](./appendix/endpoints.md)
- [Troubleshooting](./appendix/troubleshooting.md)
