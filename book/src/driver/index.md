# Writing a Device Driver

Pico de Gallo exists for a very specific kind of loop: write a driver,
plug a real part into a board on your desk, run it from your laptop,
and iterate fast.

That means:

- no firmware flashing between every edit
- no SWD probe on your bench
- no clock-tree bring-up before you can read one register
- no linker scripts, BSP setup, or target-specific project scaffolding
- no throwing your driver away when you change MCUs later

We still write the driver against `embedded-hal`. The transport happens
to be Pico de Gallo today; the same crate can ship on an RP2350,
STM32, nRF, ESP, or any other target that implements the same traits.

In this part of the book we will build a driver for the
[TMP102](https://www.ti.com/lit/gpn/tmp102), a tiny I<sup>2</sup>C
temperature sensor from Texas Instruments. It is a good tutorial device:
small register map, readable datasheet, and just enough detail to show
where a real driver gets its shape.

We will take the chapter in the same order you would tackle the work in
practice:

1. [Explore the device with `gallo`](explore.md)
2. [Scaffold the crate with the drive-rs template](scaffold.md)
3. [Describe the register map for code generation](codegen.md)
4. [Bridge the generated code to `embedded-hal`](register-interface.md)
5. [Build an ergonomic public API](ergonomic-api.md)
6. [Test it against real hardware](testing.md)
7. [Keep blocking and async users equally happy](blocking-vs-async.md)
8. [Polish it for publishing](publishing.md)

The big idea is simple:

> [!TIP]
> The fastest way to write a solid embedded driver is often to *not*
> start on the microcontroller at all. Start on your laptop, with fast
> builds, rich tooling, and a real sensor on the wire.

By the end, we will have a driver that:

- speaks TMP102 over I<sup>2</sup>C
- uses generated register accessors for the repetitive bits
- exposes a small, type-safe API for the parts humans care about
- works with both blocking and async `embedded-hal`
- can be tested with a real sensor in hardware-in-the-loop

If you already know Rust and `embedded-hal`, this chapter should feel
like a practical walkthrough rather than a Rust tutorial. We will skip
the basics and spend our time on the interesting parts: where the
register map comes from, how to make invalid states unrepresentable,
and how Pico de Gallo changes the driver-authoring workflow.

Let's start by interrogating the sensor directly.
