# Pico de Gallo — Roadmap to 1.0

> Last updated: 2026-04-24

This document lays out the path to a 1.0 release of the pico-de-gallo
project. Changes are grouped into phases and listed in ascending order of
complexity. Each entry explains *what*, *why*, and *what it unlocks*.

---

## Table of Contents

- [Where We Are Today](#where-we-are-today)
- [Design Philosophy](#design-philosophy)
- [Phase 1 — Polish What Exists](#phase-1--polish-what-exists)
- [Phase 2 — New Protocols (Software Only)](#phase-2--new-protocols-software-only)
- [Phase 3 — Advanced Features](#phase-3--advanced-features)
- [Phase 4 — Hardware Rev 2](#phase-4--hardware-rev-2)
- [Phase 5 — 1.0 Release Criteria](#phase-5--10-release-criteria)
- [Should the RP2350 Stay?](#should-the-rp2350-stay)
- [Appendix A — embedded-hal Trait Coverage Matrix](#appendix-a--embedded-hal-trait-coverage-matrix)
- [Appendix B — Competitive Landscape](#appendix-b--competitive-landscape)
- [Appendix C — RP2350 Peripheral Budget](#appendix-c--rp2350-peripheral-budget)
- [Conventions — How to Update This File](#conventions--how-to-update-this-file)

---

## Progress Overview

| Phase | Description        | Items | Done | Status                            |
|-------|--------------------|-------|------|-----------------------------------|
| **1** | Polish What Exists | 6     | 6    | ✅ Complete                       |
| **2** | New Protocols      | 4     | 3    | 🟡 In progress (2.6 blocked)      |
| **3** | Advanced Features  | 6     | 3    | 🟡 In progress (3.4–3.6 deferred) |
| **4** | Hardware Rev 2     | 6     | 1    | 🟡 In progress                    |

---

## Where We Are Today

| Area            | Status                                                                                                                                                                                                                       |
|-----------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **I2C**         | 1 bus (I2C1), 7-bit addressing, read/write/write-read/scan, configurable frequency (Standard/Fast/Fast+)                                                                                                                     |
| **SPI**         | 1 bus (SPI0), read/write/flush/transfer, configurable polarity/phase, DMA-backed                                                                                                                                             |
| **UART**        | 1 bus (UART0), read/write/flush, configurable baud rate, interrupt-driven with 1024-byte TX/RX buffers. **hw-rev2 only** (the default).                                                                                      |
| **GPIO**        | 4 pins (GPIO8–11), input/output/wait-for-edge, push-based edge event monitoring                                                                                                                                              |
| **USB**         | Full Speed (12 Mbps), postcard-rpc over raw USB bulk                                                                                                                                                                         |
| **HAL traits**  | `I2c`, `SpiBus`, `InputPin`, `OutputPin`, `StatefulOutputPin`, `Wait`, `DelayNs`, `embedded_io::{Read,Write}` (sync + async)                                                                                                 |
| **Hardware**    | v1.0 landing board (7 connectors, 13/20 signals); v1.1 landing board (2×12 header, all 20 signals, I²C pull-ups, ADC protection). `hw-rev1`/`hw-rev2` feature flags gate peripheral availability; `hw-rev2` is the default and `hw-rev1` is deprecated (removal no earlier than 2031-09-01). No level shifters, no ESD. |
| **Host crates** | internal (protocol), lib (high-level API), hal (embedded-hal bridge), ffi (C bindings), app (CLI)                                                                                                                            |
| **Endpoints**   | 47 total (ping, version, device/info, system×1, I2C×7, SPI×7, UART×5, GPIO×10, PWM×6, ADC×2, 1-Wire×6). UART/ADC/1-Wire return `Unsupported` on the deprecated hw-rev1.                                                      |
| **Tests**       | 298 unit + 5 doctests, CI on every push (both hardware revisions)                                                                                                                                                            |

### What's Missing

Compared to what embedded developers routinely need, pico-de-gallo is
still missing:

- **10-bit I2C** — some devices use extended addressing (blocked on upstream embassy-rp)
- **Voltage flexibility** — hardware is 3.3 V only; no path to 1.8 V or 5 V
- **Target power** — users must externally power their target

---

## Design Philosophy

Pico de Gallo's unique value proposition is not raw speed or protocol
coverage — commercial bridges like the FT2232H and Total Phase Aardvark
will always win on throughput and polish. The value is:

> **Any driver on crates.io that uses `embedded-hal` traits can be
> developed and tested on a host PC using pico-de-gallo as the hardware
> backend.**

Every design decision should maximize embedded-hal trait coverage and
driver compatibility. If a driver author uses `I2c + OutputPin + DelayNs`
(the most common combination), pico-de-gallo should Just Work™.

The priority order for trait implementations is driven by how commonly
each trait appears in crates.io drivers:

1. `I2c` — ✅ done
2. `SpiDevice` / `SpiBus` — ✅ done
3. `OutputPin` / `InputPin` — ✅ done
4. `DelayNs` — ✅ done
5. `embedded-io Read/Write` (UART) — ✅ done
6. `SetDutyCycle` (PWM) — ✅ done

---

## Phase 1 — Polish What Exists

*Complexity: low. No new hardware. Mostly non-breaking changes.*

These are quality improvements that make the existing feature set more
reliable and complete before adding new capabilities.

|   | Item                                                            | Tracking                                                              |
|---|-----------------------------------------------------------------|-----------------------------------------------------------------------|
| ☑ | [1.1 Rich Error Types](#11-rich-error-types)                    | [#1](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/1) |
| ☑ | [1.2 SpiDevice Trait](#12-spidevice-trait-implementation)       | [#2](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/2) |
| ☑ | [1.3 I2C Bus Scan](#13-i2c-bus-scan-endpoint)                   | [#3](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/3) |
| ☑ | [1.4 GPIO Direction Control](#14-gpio-direction-control)        | [#4](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/4) |
| ☑ | [1.5 Config Query Endpoints](#15-configuration-query-endpoints) | [#5](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/5) |
| ☑ | [1.6 MAX_TRANSFER_SIZE Audit](#16-max_transfer_size-audit)      | [#6](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/6) |

### 1.1 Rich Error Types

**What:** Replace unit-struct failures (`I2cReadFail`, `SpiWriteFail`, etc.)
with enums carrying error detail from the firmware.

**Why:** When an I2C write fails, the user currently gets `I2cWriteFail` with
no indication whether it was a NACK, bus timeout, arbitration loss, or
firmware bug. Real embedded-hal `ErrorKind` variants (`NoAcknowledge`,
`Bus`, `ArbitrationLoss`, `Overrun`) should propagate from firmware through
the host crates.

**Impact:** Breaking change to internal + lib + hal. This is the single most
impactful quality improvement possible.

```
Firmware  →  I2cError::Nack  →  postcard  →  lib  →  hal  →  ErrorKind::NoAcknowledge
```

### 1.2 SpiDevice Trait Implementation

**What:** Implement `embedded_hal::spi::SpiDevice` (and the async variant)
in the HAL crate, with firmware-managed CS assertion.

**Why:** Most SPI drivers on crates.io use `SpiDevice`, not `SpiBus`.
`SpiDevice` wraps a full CS-assert → transfer → CS-deassert transaction.
Without it, users must manually wrap `SpiBus` with `embedded-hal-bus`
adapters, which adds friction and can be error-prone over USB.

**Design:** Add a firmware endpoint `spi/transaction` that holds CS low
across a sequence of operations, or use one of the existing GPIO pins as a
firmware-controlled CS with automatic assertion on each SPI endpoint call.

### 1.3 I2C Bus Scan Endpoint

**What:** Add an `i2c/scan` endpoint that probes all 7-bit addresses
(0x08–0x77) and returns a list of responding addresses.

**Why:** This is the first thing every embedded developer does when hooking
up an I2C bus. Every competing tool has it. The CLI app should print a
nice matrix like `i2cdetect`.

### 1.4 GPIO Direction Control

**What:** Add endpoints to configure individual GPIO pins as input or
output at runtime, with optional pull-up/pull-down configuration.

**Why:** Currently the firmware decides pin direction. Users should be able
to reconfigure pins dynamically — this is especially important for
open-drain protocols and bidirectional signaling.

### 1.5 Configuration Query Endpoints

**What:** Add `i2c/get-config` and `spi/get-config` endpoints that return
the current bus configuration.

**Why:** When writing multi-step automation scripts or debugging, it's
useful to confirm what configuration is active without relying on
local state.

### 1.6 MAX_TRANSFER_SIZE Audit

**What:** `pico-de-gallo-internal` defines `MAX_TRANSFER_SIZE = 4096`, but
comments in `pico-de-gallo-lib` still reference `512`. Audit and
synchronize all documentation and buffer sizes.

**Note (M5, measured):** `MAX_TRANSFER_SIZE` is a **packet-buffer budget**,
not usable payload. The buffer must also hold the postcard-rpc header, the
length varint and COBS framing, and the budget covers the request frame *and*
the response frame. On hardware the largest TX-only `spi/transfer` payload
observed to work is **1013 bytes**, not 4096: 4096 TX-only and 3072 full duplex
both fail `-ECOMM` at the transport. Full duplex is documented safe only at 512
bytes or less. Treating 4096 as generally usable payload is therefore wrong,
and this audit should derive the real ceiling from worst-case framing rather
than restating the constant.

**Why:** Mismatched expectations between crates could cause silent data
truncation.

---

## Phase 2 — New Protocols (Software Only)

*Complexity: medium. No new hardware required (pins are already broken out
on the Pico 2 module), but requires new firmware drivers and new
endpoint families.*

|   | Item                                                   | Tracking                                                                |
|---|--------------------------------------------------------|-------------------------------------------------------------------------|
| ☑ | [2.1 UART Support](#21-uart-support)                   | [#7](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/7)   |
| ☑ | [2.2 PWM Support](#22-pwm-support)                     | [#8](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/8)   |
| ☑ | [2.3 ADC Support](#23-adc-support)                     | [#9](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/9)   |
| ☐ | [2.4 10-Bit I2C Addressing](#24-10-bit-i2c-addressing) | [#12](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/12) |

### 2.1 UART Support

**What:** Implement UART bridging using RP2350's UART0 peripheral. Add
endpoints for read, write, and configuration (baud rate, data bits,
parity, stop bits). Implement `embedded_io::Read` and `embedded_io::Write`
(plus async variants) in the HAL crate.

**Why:** UART is the most universal embedded interface. It's used for:
- Serial console access
- Bootloader communication
- GPS modules, cellular modems, Bluetooth modules
- Debug logging
- Firmware upload/recovery

Not having UART is the single biggest functional gap compared to every
competing tool. Even the $4 CH340 does UART.

**Pin assignment:** Use GPIO0 (TX) and GPIO1 (RX) — these are the default
UART0 pins and are directly accessible on the Pico 2 header.

**Hardware note:** UART doesn't need board changes. The Pico 2's GPIO0/1
are already on the module header. A future board revision could add a
dedicated UART header/connector, but it's not required.

**Baud rates:** Support standard rates: 9600, 19200, 38400, 57600, 115200,
230400, 460800, 921600. Expose as a plain `u32` — the RP2350 will silently
clamp out-of-range values. Data bits, parity, and stop bits are deferred
to a future version (requires unsafe PAC register writes while interrupts
are active).

### 2.2 PWM Support

**What:** Expose 2–4 PWM outputs using RP2350's PWM slices. Add endpoints
for setting frequency, duty cycle, enable/disable. Implement the
`embedded_hal::pwm::SetDutyCycle` trait in the HAL crate.

**Why:** PWM is essential for:
- Servo control
- LED brightness (backlight controllers, RGB LEDs)
- Motor drivers
- Buzzer/audio tone generation
- Power supply enable/control

The `SetDutyCycle` trait is the last "common" embedded-hal trait not
implemented. Adding it means pico-de-gallo covers every standard trait
in embedded-hal 1.0.

**Pin assignment:** Repurpose 2–4 of the current GPIO pins (GPIO8–15 map
to PWM slices 4–7), or use additional Pico 2 header pins. PWM and GPIO
can coexist — a pin configured for PWM simply stops being a
general-purpose GPIO until reconfigured.

### 2.3 ADC Support

**What:** Expose RP2350's ADC channels (GPIO26–29 = ADC0–3).
Add endpoints for single-shot reads and
optionally continuous sampling. There is no standard embedded-hal ADC trait
in 1.0, so expose a project-specific API.

**Why:** ADC enables:
- Voltage monitoring (power rails, battery levels)
- Analog sensor reading (thermistors, potentiometers, light sensors)
- Signal level debugging

**Resolution:** RP2350 has a 12-bit ADC with 500 ksps. Single-shot reads
are simple; continuous streaming would benefit from DMA but pushes against
USB FS bandwidth limits for high sample rates.

**Hardware note:** GPIO26–29 are on the Pico 2 module header. No board
change needed, but a future revision should break these out to dedicated
ADC-labeled headers.

### 2.4 10-Bit I2C Addressing

**What:** Support `TenBitAddress` in addition to `SevenBitAddress` for I2C
operations.

**Why:** Some devices (particularly EEPROMs with large address spaces) use
10-bit addressing. The `embedded_hal::i2c::I2c` trait is generic over
address type, so the HAL should implement both.

---

## Phase 3 — Advanced Features

*Complexity: high. Software only, but requires deeper firmware
architecture changes.*

|    | Item                                                              | Tracking                                                                                       |
|----|-------------------------------------------------------------------|------------------------------------------------------------------------------------------------|
| ☑  | [3.1 GPIO Event Topics](#31-gpio-event-topics-push-notifications) | [#13](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/13)                        |
| ☑  | [3.2 Transaction Batching](#32-i2cspi-transaction-batching)       | [#14](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/14)                        |
| ☑  | [3.3 1-Wire via PIO](#33-1-wire-support-via-pio)                  | [#15](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/15)                        |
| ⏳ | [3.4 Protocol Sniffing](#34-protocol-sniffing--logic-capture)     | [#16](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/16) — deferred to post-1.0 |
| ⏳ | [3.5 Config Persistence](#35-configuration-persistence)           | [#17](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/17) — deferred to post-1.0 |
| ⏳ | [3.6 Multi-Device Host](#36-multi-device-host-support)            | [#18](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/18) — deferred to post-1.0 |

### 3.1 GPIO Event Topics (Push Notifications)

**What:** Use postcard-rpc's topic mechanism (currently unused —
`TOPICS_IN_LIST` and `TOPICS_OUT_LIST` are empty) to push GPIO state
changes from firmware to host without polling.

**Why:** Polling `gpio/wait-*` endpoints ties up a USB transfer for each
pin being monitored. With topics, the firmware can notify the host of edge
events asynchronously. This enables interrupt-driven workflows and is
closer to how real embedded systems handle GPIO interrupts.

**Design:** Define a `GpioEvent` topic containing `{ pin: u8, state:
GpioState, timestamp_us: u64 }`. The host subscribes and receives events
as they occur.

### 3.2 I2C/SPI Transaction Batching

**What:** Allow a sequence of I2C or SPI operations to be sent as a single
USB transfer and executed atomically on the firmware side.

**Why:** Each USB round-trip adds 1–2 ms of latency. For multi-register
reads or device initialization sequences that require dozens of
operations, the cumulative latency dominates. Batching could speed up
EEPROM programming by 10–50×.

**Design:** A new `i2c/batch` endpoint that accepts a `Vec<I2cOperation>`
and returns `Vec<I2cResult>`. Similar for SPI.

### 3.3 1-Wire Support via PIO

**What:** Implement 1-Wire protocol using RP2350's PIO (Programmable I/O)
state machine. Embassy-rp already has a PIO 1-Wire program.

**Why:** 1-Wire is used by DS18B20 temperature sensors (extremely popular),
iButton devices, and some ID/authentication chips. It's a niche but
well-loved protocol that few USB bridges support — this would be a genuine
differentiator.

**Complexity:** PIO programming is non-trivial, but embassy-rp's existing
`pio_programs/onewire.rs` provides a starting point.

### 3.4 Protocol Sniffing / Logic Capture

**What:** Use a PIO state machine to passively monitor an I2C or SPI bus
and stream decoded frames to the host.

**Why:** This transforms pico-de-gallo from a pure master/controller into
a debugging tool. Being able to sniff what's happening on a bus (even one
driven by a different controller) is invaluable for hardware debugging.

**Limitations:** USB FS bandwidth caps continuous capture at ~700 KB/s
raw throughput. This is adequate for I2C (max 1 MHz = 125 KB/s) and
moderate SPI, but not for high-speed SPI or comprehensive logic analysis.
This is NOT a Saleae replacement — it's a "quick look at what's on the
bus" tool.

### 3.5 Configuration Persistence

**What:** Store the current bus configurations (I2C frequency, SPI mode,
GPIO directions, UART baud rate) in RP2350's flash so they survive
power cycles.

**Why:** Users who always work with the same target shouldn't have to
reconfigure on every plug-in. Useful for production test fixtures that
need deterministic startup.

### 3.6 Multi-Device Host Support

**What:** Allow multiple host applications to connect to separate
interfaces on the same pico-de-gallo simultaneously (e.g., one app uses
I2C, another uses UART).

**Why:** In complex setups, different tools may need to access different
buses. This requires firmware-side resource locking and multiple USB
endpoints or interfaces.

---

## Phase 4 — Hardware Rev 2

*Complexity: high. Requires PCB re-spin, component sourcing, and
potentially case redesign.*

This is the section that addresses the board re-spin directly. Changes are
ordered by impact-to-cost ratio.

|    | Item                                                             | Tracking                                                                                  |
|----|------------------------------------------------------------------|-------------------------------------------------------------------------------------------|
| ☐  | [4.1 Voltage Level Translators](#41-voltage-level-translators)   | [#20](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/20)                   |
| ☐  | [4.2 Target Power Output](#42-target-power-output)               | [#21](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/21)                   |
| ✅ | [4.3 Dedicated Connector Layout](#43-dedicated-connector-layout) | [#22](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/22) — shipped in v1.1 |
| ☐  | [4.4 ESD Protection](#44-esd-protection)                         | [#23](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/23)                   |
| ☐  | [4.5 Activity LEDs](#45-activity-leds)                           | [#24](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/24)                   |
| ☐  | [4.6 Board Size and Mounting](#46-board-size-and-mounting)       | [#25](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/25)                   |

### 4.1 Voltage Level Translators

**Priority: CRITICAL — the single most impactful hardware change.**

**What:** Add bidirectional voltage level translators between the RP2350
(3.3 V) and the target connectors, with a user-selectable target voltage
(VREF) pin.

**Why:** The embedded world runs at many voltages:

| Voltage | Common Uses                                                    |
|---------|----------------------------------------------------------------|
| 1.8 V   | Modern SoCs, LPDDR interfaces, low-power sensors               |
| 2.5 V   | Some FPGAs, legacy parts                                       |
| 3.3 V   | Most microcontrollers, sensors, EEPROMs                        |
| 5.0 V   | Arduino ecosystem, RS-232 level, automotive, legacy industrial |

Without level translators, pico-de-gallo is limited to 3.3 V targets.
This excludes a huge portion of the embedded ecosystem, especially:
- 5 V Arduino shields and legacy sensors
- 1.8 V modern SoC I2C buses (increasingly common)
- Mixed-voltage designs where the bridge must match the target

**Recommended parts:**

| Bus        | Part                            | Notes                                                                                                                   |
|------------|---------------------------------|-------------------------------------------------------------------------------------------------------------------------|
| I2C        | **PCA9306** or **TXS0102**      | Bidirectional, open-drain compatible, designed for I2C. Do NOT use TXB0108 for I2C — it fights the open-drain pull-ups. |
| SPI / GPIO | **TXB0108** or **SN74LVC8T245** | Unidirectional or auto-direction. The SN74LVC8T245 is more predictable for high-speed SPI.                              |
| UART       | **SN74LVC2T45**                 | 2-channel, dual-supply. TX is always host→target, RX is target→host, so direction is fixed.                             |

**Design approach:**
- Add a **VREF pin** on the target connector. The user supplies target
  voltage on this pin (or a jumper selects from 3.3 V / 5 V).
- Level translators reference VREF on the target side and 3.3 V on the
  RP2350 side.
- If VREF is not supplied, the translators can be powered from the
  on-board 3.3 V rail (passthrough mode, no translation).

### 4.2 Target Power Output

**What:** Provide switchable 3.3 V and 5 V power outputs to power small
target boards directly from USB.

**Why:** When prototyping with a single sensor or small target board,
needing a separate power supply is friction. The Total Phase Aardvark
provides target power and it's one of its most-loved features.

**Implementation:**
- **5 V:** Directly from USB VBUS through a current-limited switch
  (e.g., TPS2051B, 500 mA limit).
- **3.3 V:** From the Pico 2's on-board regulator or a dedicated LDO,
  also current-limited.
- **Control:** A GPIO pin enables/disables target power via firmware. Add
  a `power/enable` endpoint.
- **Protection:** Include a polyfuse or electronic fuse to protect against
  target short circuits.

**Budget:** USB 2.0 provides 500 mA total. The Pico 2 itself draws
~30–50 mA. Safely offer 200–300 mA to targets.

### 4.3 Dedicated Connector Layout

**What:** Redesign the connector layout with labeled, purpose-specific
headers:

| Connector | Type                                      | Pins                                     |
|-----------|-------------------------------------------|------------------------------------------|
| **I2C**   | JST SH 4-pin (Qwiic/STEMMA QT compatible) | VCC, GND, SDA, SCL                       |
| **SPI**   | 2×4 pin header                            | VCC, GND, SCK, MOSI, MISO, CS0, CS1, CS2 |
| **UART**  | 1×4 pin header                            | VCC, GND, TX, RX                         |
| **GPIO**  | 2×5 pin header                            | 8× GPIO + GND + VREF                     |
| **ADC**   | 1×6 pin header                            | 4× ADC channels + GND + VREF             |
| **PWM**   | 1×4 pin header                            | 2–4× PWM outputs + GND                   |
| **Power** | 1×3 pin header                            | 5V, 3.3V, GND                            |

**Why the Qwiic/STEMMA QT connector matters:** The JST SH 4-pin I2C
connector is a de facto standard in the maker/prototyping ecosystem.
SparkFun (Qwiic) and Adafruit (STEMMA QT) both use it. Adding one means
users can plug in any Qwiic/STEMMA sensor board with a single cable — no
wiring, no breadboard.

### 4.4 ESD Protection

**What:** Add ESD protection diodes (e.g., TPD4E05U06 for 4-channel, or
PRTR5V0U2X for USB/I2C) on all external-facing signal lines.

**Why:** USB bridges get plugged and unplugged constantly, often into
unknown targets. A single ESD event can destroy the RP2350. This is
table stakes for any tool that's meant to be handled daily.

**Cost:** Negligible — TVS diode arrays are $0.10–0.30 each.

### 4.5 Activity LEDs

**What:** Add per-bus activity LEDs:

| LED   | Color       | Indicates             |
|-------|-------------|-----------------------|
| Power | Green       | Board powered         |
| USB   | Blue        | USB enumerated        |
| I2C   | Yellow      | I2C bus activity      |
| SPI   | Orange      | SPI bus activity      |
| UART  | Red         | UART TX/RX activity   |
| Error | Red (blink) | Last operation failed |

**Why:** Visual feedback is surprisingly important for debugging. "Is it
even talking to the bus?" is a question users ask constantly. A quick
glance at blinking LEDs answers it immediately.

**Implementation:** Use remaining GPIOs to drive LEDs. The firmware
toggles them in the endpoint handlers. Total current draw: ~12 mA for
6 LEDs at 2 mA each.

### 4.6 Board Size and Mounting

**What:** Consider whether the case design needs updating for the new
connectors. The current design uses M3 mounting holes; keep those but
potentially expand the PCB footprint to accommodate level translators and
additional connectors.

**Tradeoff:** A larger board costs more to fabricate and doesn't fit the
current 3D-printed case, but a cramped board with too-small connectors is
worse.

---

## Phase 5 — 1.0 Release Criteria

The 1.0 release should be declared when the items below are complete.
Each references the phase item where the work is tracked — check progress
there, not here.

### Must Have (1.0 blockers)

| Requirement                                                        | Phase Item |
|--------------------------------------------------------------------|------------|
| All `embedded-hal` 1.0 sync + async traits implemented             | 1.2, 2.2   |
| `embedded-io` Read/Write for UART                                  | 2.1        |
| Rich error types mapping firmware errors to `ErrorKind`            | 1.1        |
| I2C bus scan                                                       | 1.3        |
| GPIO direction + pull configuration                                | 1.4        |
| UART support (at least one UART)                                   | 2.1        |
| Configuration query endpoints                                      | 1.5        |
| All public API types documented with rustdoc                       | — (ongoing)|
| Book updated with all interfaces and examples                      | — (ongoing)|
| Stable wire protocol (no breaking serialization changes after 1.0) | — (policy) |

### Should Have (target for 1.0, can slip)

| Requirement                                            | Phase Item   |
|--------------------------------------------------------|--------------|
| PWM support                                            | 2.2          |
| ADC support (at least single-shot reads)               | 2.3          |
| GPIO event topics                                      | 3.1          |
| I2C transaction batching                               | 3.2          |
| 10-bit I2C addressing                                  | 2.4          |
| CLI app with bus scan, UART terminal, interactive GPIO | — (app work) |

### Nice to Have (post-1.0)

| Requirement               | Phase Item |
|---------------------------|------------|
| 1-Wire via PIO            | 3.3        |
| Protocol sniffing         | 3.4        |
| Configuration persistence | 3.5        |
| Multi-device host support | 3.6        |
| SPI target mode           | — (future) |
| Hardware Rev 2 features   | Phase 4    |

---

## Should the RP2350 Stay?

**Short answer: Yes.**

### The Case For RP2350

| Factor              | Assessment                                                                                                               |
|---------------------|--------------------------------------------------------------------------------------------------------------------------|
| **Price**           | ~$1 chip, ~$5 Pico 2 board. Unbeatable.                                                                                  |
| **Embassy support** | First-class. Active upstream development.                                                                                |
| **Peripheral set**  | 2× I2C, 2× SPI, 2× UART, 12× PWM, 4× ADC, 3× PIO — more than enough.                                                     |
| **PIO**             | Unique advantage. Enables 1-Wire, protocol sniffing, custom protocols. No competitor at this price has anything similar. |
| **SRAM**            | 520 KB. Plenty for USB buffers and protocol state.                                                                       |
| **Dual-core**       | Available if needed for parallel bus monitoring.                                                                         |
| **Community**       | Huge Raspberry Pi ecosystem. Easy to source globally.                                                                    |
| **Tooling**         | UF2 flashing, SWD debug, picotool — all mature.                                                                          |

### The USB Full Speed Question

The RP2350 only supports USB Full Speed (12 Mbps). In practice, bulk
transfers achieve ~700 KB/s to ~1 MB/s. Is this a problem?

**For I2C:** No. I2C Fast+ (the fastest mode pico-de-gallo supports) runs
at 1 MHz = 125 KB/s. USB FS has 5–8× headroom.

**For SPI:** Marginal. SPI at 8 MHz full-duplex = 1 MB/s, which is right
at the USB FS ceiling. For sustained streaming (e.g., display refresh),
USB becomes the bottleneck. For transactional workloads (register reads,
EEPROM pages), it's fine because the per-transfer overhead dominates
anyway.

**For UART:** No. Even at 921600 baud, that's only ~92 KB/s. Trivial for
USB FS.

**For ADC:** Depends on sample rate. Single-shot reads are fine.
Continuous streaming at 500 ksps × 12-bit = 750 KB/s — right at the
limit. A practical continuous mode would need to decimate or buffer.

**For protocol sniffing:** USB FS is the real constraint. I2C sniffing is
comfortable; SPI sniffing at high clock rates would need on-device
buffering and burst transfers.

**The Total Phase Aardvark also uses USB Full Speed** and is the industry
standard at $375. If it's good enough for professional engineers paying
$375, it's good enough for pico-de-gallo.

### What Would USB High Speed Buy?

If USB HS were critical, the alternatives are:

| MCU             | USB                       | Price         | Embassy Support     | Notes                                                 |
|-----------------|---------------------------|---------------|---------------------|-------------------------------------------------------|
| **STM32F723**   | HS with internal PHY      | ~$8           | Yes (embassy-stm32) | Rare chip with built-in HS PHY. Limited availability. |
| **STM32H7xx**   | HS with external ULPI PHY | ~$12 + $3 PHY | Yes (embassy-stm32) | Complex layout, BGA packages, 4+ layer PCB.           |
| **ESP32-S3**    | FS only                   | ~$3           | No (no embassy)     | Same FS limitation, worse ecosystem.                  |
| **NXP i.MX RT** | HS with internal PHY      | ~$10          | Experimental        | Overkill. Cortex-M7, complex clock tree.              |

**Verdict:** Moving to USB HS would 3-5× the BOM cost, require a 4-layer
PCB, and add significant design complexity — all for a throughput gain
that only matters in a few niche scenarios (continuous high-speed SPI
streaming, high-rate ADC capture). The RP2350's PIO capability, price,
and Embassy support make it the right choice for a tool focused on
driver development and hardware prototyping.

**Recommendation:** Keep RP2350. If a "Pro" variant is ever warranted,
the STM32F723 (HS PHY built-in) would be the most practical upgrade path,
but that's a separate product, not a replacement.

---

## Appendix A — embedded-hal Trait Coverage Matrix

| Trait                  | Crate                | Blocking | Async | Status    |
|------------------------|----------------------|----------|-------|-----------|
| `I2c<SevenBitAddress>` | `embedded-hal`       | ✅       | ✅    | Done      |
| `I2c<TenBitAddress>`   | `embedded-hal`       | ❌       | ❌    | Phase 2.4 |
| `SpiBus`               | `embedded-hal`       | ✅       | ✅    | Done      |
| `SpiDevice`            | `embedded-hal`       | ✅       | ✅    | Done      |
| `InputPin`             | `embedded-hal`       | ✅       | —     | Done      |
| `OutputPin`            | `embedded-hal`       | ✅       | —     | Done      |
| `StatefulOutputPin`    | `embedded-hal`       | ✅       | —     | Done      |
| `Wait`                 | `embedded-hal-async` | —        | ✅    | Done      |
| `DelayNs`              | `embedded-hal`       | ✅       | ✅    | Done      |
| `SetDutyCycle`         | `embedded-hal`       | ✅       | —     | Done      |
| `Read`                 | `embedded-io`        | ✅       | ✅    | Done      |
| `Write`                | `embedded-io`        | ✅       | ✅    | Done      |
| `ReadReady`            | `embedded-io`        | ❌       | —     | Future    |
| `WriteReady`           | `embedded-io`        | ❌       | —     | Future    |

The `embedded-io` rows cover both majors: `pico-de-gallo-hal`'s
`embedded-io-06` feature remains enabled by default, and
`embedded-io-07` can be enabled additively for drivers targeting 0.7.

**At 1.0:** every cell should be ✅ or have a documented reason for
exclusion.

---

## Appendix B — Competitive Landscape

| Feature          | Pico de Gallo (current) | Pico de Gallo (1.0 target) | Total Phase Aardvark ($375) | FTDI FT2232H (~$30) | Bus Pirate (~$40) | MCP2221A (~$5) |
|------------------|-------------------------|----------------------------|-----------------------------|---------------------|-------------------|----------------|
| **I2C**          | ✅ (+ scan, batch)      | ✅ (+ 10-bit)              | ✅ (+ slave)                | ✅ (MPSSE)          | ✅                | ✅             |
| **SPI**          | ✅ (+ SpiDevice)        | ✅                         | ✅ (+ slave)                | ✅ (MPSSE)          | ✅                | ❌             |
| **UART**         | ✅                      | ✅                         | ❌                          | ✅ (dual)           | ✅                | ✅             |
| **GPIO**         | ✅ (8 pins)             | ✅                         | ✅ (6 pins)                 | ✅ (limited)        | ✅                | ✅ (4 pins)    |
| **PWM**          | ✅                      | ✅                         | ✅                          | ✅                  | ✅                | ✅             |
| **ADC**          | ✅ (4-ch, 12-bit)       | ✅                         | ❌                          | ❌                  | ✅                | ✅ (3-ch)      |
| **1-Wire**       | ✅ (PIO)                | ✅                         | ❌                          | ❌                  | ✅                | ❌             |
| **Level shift**  | ❌                      | Rev 2                      | ❌ (accessory)              | ❌ (5V tolerant)    | ✅                | ❌             |
| **Target power** | ❌                      | Rev 2                      | ✅                          | ❌                  | ✅                | ❌             |
| **embedded-hal** | ✅                      | ✅ (complete)              | ❌                          | ❌                  | ❌                | ❌             |
| **Rust-native**  | ✅                      | ✅                         | ❌                          | ❌                  | ❌                | ❌             |
| **USB speed**    | FS                      | FS                         | FS                          | HS                  | FS                | FS             |
| **Open source**  | ✅                      | ✅                         | ❌                          | ❌                  | ✅                | ❌             |
| **Price**        | ~$5                     | ~$10–15 (Rev 2)            | $375                        | ~$30                | ~$40              | ~$5            |

**Unique differentiators at 1.0:**
1. Only bridge with native `embedded-hal` trait support
2. Only bridge that's both open-source hardware AND Rust-native
3. Price/feature ratio competitive with Bus Pirate, with far better
   Rust integration
4. PIO enables custom protocols no other bridge at this price can match

---

## Appendix C — RP2350 Peripheral Budget

The RP2350 (as used on Pico 2) has the following peripherals. This table
shows current usage and planned allocation:

| Peripheral | Total Available   | Currently Used                        | Planned (1.0)    | Notes                                     |
|------------|-------------------|---------------------------------------|------------------|-------------------------------------------|
| **I2C**    | 2 (I2C0, I2C1)    | 1 (I2C1)                              | 1                | Second bus dropped                        |
| **SPI**    | 2 (SPI0, SPI1)    | 1 (SPI0)                              | 1                | Second bus dropped                        |
| **UART**   | 2 (UART0, UART1)  | 1 (UART0)                             | 1 (UART0)        | GPIO0/1                                   |
| **PWM**    | 12 slices (24 ch) | 4 slices                              | 4 slices         | GPIO8–15, PWM slices 4–7                  |
| **ADC**    | 4 GPIO            | 4 (GPIO26–29)                         | 4 GPIO           | GPIO26–29                                 |
| **PIO**    | 3 (PIO0–2)        | 1 (1-Wire)                            | 1–2              | 1-Wire, sniffing                          |
| **DMA**    | 16 channels       | 2 (SPI)                               | 4–6              | ADC continuous, UART, SPI                 |
| **GPIO**   | 30 (on Pico 2)    | 10 (I2C: 2, SPI: 3, user: 8, USB: ~0) | ~24              | Plenty of headroom                        |
| **USB**    | 1 (FS)            | 1                                     | 1                | Fully committed                           |
| **Flash**  | 4 MB (on Pico 2)  | ~256 KB firmware                      | ~256 KB + config | Config persistence in reserved sector     |
| **SRAM**   | 520 KB            | ~64 KB (estimated)                    | ~128 KB          | Buffer growth for batching/ADC            |

**Conclusion:** The RP2350 has substantial unused peripheral capacity.
We can implement every feature in this roadmap without running out of
resources. The main constraint is USB bandwidth, not peripheral count.

---

## Summary

The path to 1.0 is primarily a **software effort**. The RP2350 hardware
is capable of far more than we currently use. The recommended sequence:

1. **Phase 1** (polish) — can start immediately, mostly non-breaking
2. **Phase 2** (new protocols) — the big value add; UART is the priority
3. **Phase 3** (advanced) — differentiating features; topics and batching
   have the highest impact
4. **Phase 4** (hardware) — plan the Rev 2 board in parallel with Phase 2
   software work; order prototypes during Phase 3
5. **Phase 5** (release) — 1.0 when all must-have criteria are met

The hardware re-spin should be done **once** with all changes in this
document. Level translators and ESD protection are the non-negotiable
additions. Target power and the Qwiic connector are high-value, low-cost
additions that should be included. Activity LEDs are nice to have if board
space permits.

The RP2350 is the right MCU. Keep it.

---

## Conventions — How to Update This File

### Checking off items

When a phase item is complete, update its row in the phase summary table:

```
| ☑ | [1.3 I2C Bus Scan](#13-i2c-bus-scan-endpoint) | #42 |
```

Then update the [Progress Overview](#progress-overview) table: increment
the "Done" column and update the status emoji:

- 🔴 Not started (0 done)
- 🟡 In progress (some done)
- 🟢 Complete (all done)

### Linking issues and PRs

Use the **Tracking** column to record the GitHub issue or PR number that
implements the item. Use `#N` format for issues/PRs in this repo:

```
| ☐ | [2.1 UART Support](#21-uart-support) | #57, #63 |
```

### Adding new items

If a new work item is discovered:

1. Add it to the appropriate phase as a new subsection (e.g., `### 2.7 ...`)
2. Add a row to that phase's summary table
3. Update the item count in the Progress Overview
4. If it's a 1.0 requirement, add a row to Phase 5

### Updating the date

Update the `> Last updated:` line at the top whenever you commit changes
to this file.
