# Pinout & Connector

This is the authoritative pin map for both PCB revisions. Refer to
it whenever you wire up a peripheral.

## Firmware Pin Map

The firmware always uses the same RP2350 GPIOs, regardless of
which revision PCB they're routed to.

| Function      | RP2350 GPIO | Available on | Notes                       |
|---------------|-------------|--------------|-----------------------------|
| UART TX       | GPIO 0      | v1.1+        | UART0 TX, buffered          |
| UART RX       | GPIO 1      | v1.1+        | UART0 RX                    |
| I²C SDA       | GPIO 2      | v1.0+        | I²C1, async DMA             |
| I²C SCL       | GPIO 3      | v1.0+        |                             |
| SPI RX (MISO) | GPIO 4      | v1.0+        | SPI0, DMA full-duplex       |
| SPI_CS net    | GPIO 5      | v1.1+        | PCB net name only; firmware never claims or drives it |
| SPI SCK       | GPIO 6      | v1.0+        |                             |
| SPI TX (MOSI) | GPIO 7      | v1.0+        |                             |
| GPIO 0        | GPIO 8      | v1.0+        | User pin, in/out/edge       |
| GPIO 1        | GPIO 9      | v1.0+        | User pin, in/out/edge       |
| GPIO 2        | GPIO 10     | v1.0+        | User pin, in/out/edge       |
| GPIO 3        | GPIO 11     | v1.0+        | User pin, in/out/edge       |
| PWM 0         | GPIO 12     | v1.0+        | Slice 6 channel A           |
| PWM 1         | GPIO 13     | v1.0+        | Slice 6 channel B           |
| PWM 2         | GPIO 14     | v1.0+        | Slice 7 channel A           |
| PWM 3         | GPIO 15     | v1.0+        | Slice 7 channel B           |
| 1-Wire        | GPIO 16     | v1.1+        | PIO0/SM0, open-drain        |
| ADC 0         | GPIO 26     | v1.1+        | 12-bit, 0–3.3 V nominal     |
| ADC 1         | GPIO 27     | v1.1+        | 12-bit                      |
| ADC 2         | GPIO 28     | v1.1+        | 12-bit                      |

The user-facing GPIO numbering in the CLI, library, FFI, and Python
bindings (`0`–`3`) maps to RP2350 GPIO 8–11. Same goes for ADC
channels (`0`–`2` → GPIO 26–28) and PWM channels (`0`–`3` → GPIO
12–15).

## v1.0 Pin Headers

v1.0 uses seven separate 0.1″ pin headers, one per logical bus.
Refer to the silkscreen on the board for the exact layout. Signals
**not** brought out on v1.0: UART TX/RX, 1-Wire, and ADC 0–2. The
physical SPI_CS net to GPIO 5 is also absent; on v1.1 it is routed
to the header, but current firmware does not drive it.

## v1.1 Box Header

v1.1 consolidates everything onto a single keyed 2×12 (0.1″ pitch)
shrouded box header. Viewed from above with the USB connector
pointing **up**, pin 1 is at the **top-right**. The shroud key
notch faces right. Even-numbered pins (bottom row) are on the
left; odd-numbered pins (top row) are on the right.

```
 Pin 2  GND          ┃ VREF (+3V3) Pin 1
 Pin 4  I2C_SCL      ┃ I2C_SDA     Pin 3
 Pin 6  SPI_MOSI     ┃ SPI_MISO    Pin 5
 Pin 8  SPI_CS *      ┃ SPI_SCK     Pin 7
 Pin 10 UART_RX      ┃ UART_TX     Pin 9
 Pin 12 GPIO1        ┃ GPIO0       Pin 11
 Pin 14 GPIO3        ┃ GPIO2       Pin 13
 Pin 16 PWM1         ┃ PWM0        Pin 15
 Pin 18 PWM3         ┃ PWM2        Pin 17
 Pin 20 ADC0         ┃ ONEWIRE     Pin 19
 Pin 22 ADC2         ┃ ADC1        Pin 21
 Pin 24 GND          ┃ +3V3        Pin 23
```

`*` The silkscreen and schematic label pin 8 `SPI_CS`, but that name
describes the PCB net only. No firmware claims GPIO 5, so the pin is
**not** a working chip select — see below.

### Full v1.1 Pinout Table

| Header Pin | Net      | RP2350 GPIO | Direction | Notes                     |
|-----------:|----------|-------------|-----------|---------------------------|
| 1          | VREF     | —           | Power out | 3.3 V (hardwired on v1.1) |
| 2          | GND      | —           | Power     | Ground                    |
| 3          | SDA      | GPIO 2      | Bidir     | I²C1 SDA, 4.7 kΩ pull-up  |
| 4          | SCL      | GPIO 3      | Bidir     | I²C1 SCL, 4.7 kΩ pull-up  |
| 5          | SPI_MISO | GPIO 4      | Input     | SPI0 RX                   |
| 6          | SPI_MOSI | GPIO 7      | Output    | SPI0 TX                   |
| 7          | SPI_SCK  | GPIO 6      | Output    | SPI0 SCK                  |
| 8          | SPI_CS (unused) | GPIO 5 | Not driven by firmware | PCB net name only; GPIO 5 remains in its reset state |
| 9          | UART_TX  | GPIO 0      | Output    | UART0 TX                  |
| 10         | UART_RX  | GPIO 1      | Input     | UART0 RX                  |
| 11         | GPIO0    | GPIO 8      | Bidir     | User GPIO 0               |
| 12         | GPIO1    | GPIO 9      | Bidir     | User GPIO 1               |
| 13         | GPIO2    | GPIO 10     | Bidir     | User GPIO 2               |
| 14         | GPIO3    | GPIO 11     | Bidir     | User GPIO 3               |
| 15         | PWM0     | GPIO 12     | Output    | PWM slice 6A              |
| 16         | PWM1     | GPIO 13     | Output    | PWM slice 6B              |
| 17         | PWM2     | GPIO 14     | Output    | PWM slice 7A              |
| 18         | PWM3     | GPIO 15     | Output    | PWM slice 7B              |
| 19         | ONEWIRE  | GPIO 16     | Bidir     | PIO0/SM0, open-drain      |
| 20         | ADC0     | GPIO 26     | Input     | Via 100 Ω series resistor |
| 21         | ADC1     | GPIO 27     | Input     | Via 100 Ω series resistor |
| 22         | ADC2     | GPIO 28     | Input     | Via 100 Ω series resistor |
| 23         | +3V3     | —           | Power out | Direct 3.3 V              |
| 24         | GND      | —           | Power     | Ground                    |

Header pin 8 carries the PCB's SPI_CS net to RP2350 GPIO 5, but
current firmware never claims or drives that pad. It is not a valid
`spi/batch` chip-select index or user GPIO index. Use firmware GPIO
indices 0–3, which map to RP2350 GPIO 8–11, for chip-select instead.
No pull or stable inactive level is guaranteed on GPIO 5.

The `SPI_CS` name is a **silkscreen and schematic net label, not a
capability**. It records what the net was routed for, not what the
firmware does with it, and the board offers no working dedicated
chip-select signal. The label cannot be corrected without a new board
revision, so treat this page — not the silkscreen — as authoritative.
See [issue #99](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/99).

The device currently reports `num_gpios == 4`, so host validation
refuses `--cs 5` before transmitting anything. If that validation is
bypassed, firmware returns `SpiError::InvalidCsPin` without touching
a pin.

> [!NOTE]
>
> **Pin 1 (VREF)** is hardwired to 3.3 V on v1.1. On the future v2
> board it becomes a switchable rail (1.8 V / 3.3 V / 5 V). Adapter
> boards designed today against the v1.1 header will see 3.3 V;
> they'll continue to plug into v2 with the same key orientation.

## Electrical Notes

- All digital I/O is 3.3 V CMOS. Do not drive 5 V signals directly
  into Pico de Gallo without a level translator.
- The on-board I²C pull-ups (v1.1+) are sized for moderate bus
  capacitance. For long cables or many devices, add external
  pull-ups in parallel and treat the on-board value as a minimum.
- The ADC inputs see a 100 Ω series resistor on v1.1+. Keep that
  in mind for source-impedance budgeting if you care about absolute
  accuracy.
- 3.3 V and VREF on v1.1 share the Pico 2's regulator. Don't pull
  hundreds of milliamps from the header.
