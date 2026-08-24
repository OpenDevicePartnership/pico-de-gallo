# Revisions: v1.0 vs v1.1

The Pico de Gallo PCB has shipped in two revisions, with a third
planned. The revision determines two things:

1. **Which firmware feature flag** you flash (`hw-rev1` or
   `hw-rev2`).
2. **Which peripherals** the firmware exposes.

| Revision        | Feature flag        | Connector                                       | Capabilities                           |
|-----------------|---------------------|-------------------------------------------------|----------------------------------------|
| **v1.0**        | `hw-rev1` (default) | 7 separate pin headers                          | I²C, SPI, GPIO, PWM                    |
| **v1.1**        | `hw-rev2`           | one keyed 2×12 shrouded header                  | I²C, SPI, UART, GPIO, PWM, ADC, 1-Wire |
| **v2** (future) | `hw-rev2`           | 2×12 shrouded header + level translators        | same as v1.1, plus variable VREF rail  |

> [!IMPORTANT]
>
> The capability set is **enforced by firmware**, not by the
> hardware. Flashing `hw-rev1` firmware onto a v1.1 board still
> gives you only the v1.0 capability set. Match the firmware to
> the board you actually have.

## Which One Should I Pick?

- **You only need I²C, SPI, GPIO, or PWM** → v1.0 is fine and
  cheaper to fabricate.
- **You need UART, ADC, or 1-Wire** → you want v1.1 (or later).
  Calling an unsupported endpoint on v1.0 firmware returns
  `Unsupported`.
- **You're starting from scratch today** → fabricate v1.1. It's
  the same effort and a strict superset.

## What v1.1 Adds Over v1.0

<p align="center">
<img src="../pico-de-gallo-rev1.1.png" width="480" alt="Pico de Gallo v1.1">
</p>

- **Single keyed box header.** One cable, one orientation, no
  ambiguity. Adapter boards (informally called "toppings") can be
  designed against a stable connector instead of wrangling seven
  separate jumper bundles.
- **All implemented peripheral signals routed.** UART, 1-Wire, and
  the three ADC inputs are physically absent on v1.0. v1.1 also
  routes the PCB's SPI_CS net from RP2350 GPIO 5, but current
  firmware never claims or drives that pad.
- **On-board passives.**
  - 4.7 kΩ pull-ups on I²C (no more dangling resistors).
  - 100 Ω series resistors on each ADC input for protection and a
    mild RC roll-off.
  - 100 nF decoupling on VREF.

## What v2 Will Add

Planned but not yet released:

- **Variable VREF rail** on header pin 1: selectable 1.8 V / 3.3 V /
  5 V. On v1.1 this pin is hardwired to 3.3 V.
- **Level translators** on the digital signals so the same board
  can talk to 1.8 V, 3.3 V, and 5 V peripherals without an external
  level shifter.
- Same firmware feature flag as v1.1 (`hw-rev2`).

> [!NOTE]
>
> Adapter boards designed for the v1.1 box-header pinout will plug
> into v2 unchanged. On v1.1 they will see 3.3 V on pin 1; on v2
> they will see whatever VREF is set to. Design your topping with
> that in mind.

## v1.0 — Best Effort

<p align="center">
<img src="../pico-de-gallo-rev1.png" width="480" alt="Pico de Gallo v1.0">
</p>

The v1.0 board predates the consolidated header and lacks routing
for UART (GPIO 0-1), 1-Wire (GPIO 16), and ADC (GPIO 26-28).
Firmware validates those endpoints and returns `Unsupported`, so
calling them won't crash — you just won't get data.

If you're stuck on v1.0 and need one of the missing signals, you
can solder a wire directly to the corresponding Pico 2 castellated
pad. It's not pretty, but it works for UART, 1-Wire, and ADC.

SPI_CS is different. Its physical GPIO 5 net is present on the v1.1
header but absent on v1.0, and current firmware does not claim or
drive GPIO 5 on either revision. Soldering to that pad does not make
it usable as firmware-controlled chip-select; use a user GPIO index
reported by `device/info` instead.

## Identifying Your Board

The fastest way to tell what revision firmware you're running:

```console
$ gallo version
Pico de Gallo FW v0.8.0
Schema v0.4.0
HW revision 2
Capabilities: I2C ✓ | SPI ✓ | UART ✓ | GPIO ✓ | PWM ✓ | ADC ✓ | 1-Wire ✓
```

`HW revision 1` corresponds to `hw-rev1`; `HW revision 2` to
`hw-rev2`. The capability line tells you exactly which peripherals
this firmware will serve.

## Migrating from v1.0 to v1.1

Code-wise, **nothing changes**. The wire protocol is the same; the
host crates are the same; the CLI is the same. The only thing that
moves is which physical pins your peripheral cables plug into — see
[Pinout & Connector](./pinout.md).

If you have driver code that detects capabilities at runtime, use
`device_info()` (host) / `gallo_get_device_info` (FFI) and gate on
the `capabilities` bitfield. That way the same binary works
unmodified on both boards.
