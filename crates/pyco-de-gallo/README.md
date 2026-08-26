# pyco-de-gallo

Python bindings for [Pico de Gallo](https://github.com/OpenDevicePartnership/pico-de-gallo),
a USB-attached protocol bridge built on the
[Raspberry Pi Pico 2](https://www.raspberrypi.com/products/raspberry-pi-pico-2/)
(RP2350). Talk **I²C, SPI, UART, GPIO, PWM, ADC, and 1-Wire** straight from
Python — no cross-compiling, no flashing on every change. Develop and test
your embedded drivers on your laptop and let the Pico do the wiggling.

The native extension wraps the async Rust host library (`pico-de-gallo-lib`)
with [PyO3](https://pyo3.rs). Every method is **synchronous** from Python's
point of view; the async USB I/O is driven by a Tokio runtime owned by each
device handle, and the GIL is released for the duration of each round-trip so
your other Python threads keep running.

## Requirements

`pip install` gives you the host-side library. To do anything useful you also
need the hardware: a Raspberry Pi Pico 2 (or a Pico de Gallo landing board)
running the [Pico de Gallo firmware](https://github.com/OpenDevicePartnership/pico-de-gallo).

- **Python:** CPython 3.8+ (and PyPy). Prebuilt wheels are published for
  CPython 3.8–3.14.
- **Platforms:** Linux (x86_64 / aarch64, manylinux + musllinux),
  Windows (x64 / x86), macOS (Apple silicon).

## Installation

```sh
pip install pyco-de-gallo
```

The Python import name is `pyco_de_gallo` (underscores), while the
distribution name on PyPI is `pyco-de-gallo` (hyphens).

## Quick start

```python
import pyco_de_gallo as gallo

# Connect to the first attached board and verify the firmware is
# wire-compatible with this library. Raises RuntimeError on mismatch.
dev = gallo.open_strict()

v = dev.version()
print(f"Firmware {v.major}.{v.minor}.{v.patch}")

# I²C: switch to 400 kHz, then write a register pointer and read it back.
dev.i2c_set_config(gallo.I2cFrequency.Fast)
data = dev.i2c_write_read(0x50, [0x00], 16)   # e.g. read 16 bytes from an EEPROM
print(list(data))
```

If several boards are connected, pick one deterministically by serial number:

```python
import pyco_de_gallo as gallo

for d in gallo.list_devices():
    print(d.serial_number, d.manufacturer, d.product)

dev = gallo.open_strict_with_serial_number("E6625C087B...")
```

> `open()` / `open_with_serial_number()` are the lazy variants: they connect
> but only surface failures on the first RPC call. Prefer the `*_strict`
> variants in real code — they fail fast with a clear exception if the device
> is unreachable or the firmware schema doesn't match.

## Peripherals

| Bus    | Highlights                                                                       |
|--------|----------------------------------------------------------------------------------|
| I²C    | `i2c_read` / `i2c_write` / `i2c_write_read`, `i2c_scan`, `i2c_batch`, 100 kHz–1 MHz |
| SPI    | `spi_read` / `spi_write` / `spi_transfer`, `spi_batch` under chip-select, configurable phase/polarity |
| UART   | `uart_read` (with timeout) / `uart_write` / `uart_flush`, configurable baud rate |
| GPIO   | read/write, blocking edge waits (with optional timeout), and push-based edge events |
| PWM    | per-channel duty cycle, frequency, phase-correct, enable/disable                 |
| ADC    | single-shot 12-bit reads on 4 channels                                            |
| 1-Wire | reset/presence, read/write, strong-pullup, and ROM search                         |

> Some peripherals (ADC, UART, 1-Wire) require **hw-rev2** firmware, and not
> every signal is broken out on the V1 landing board. See the firmware and
> hardware docs for the routing details.

### Blink an LED (GPIO)

```python
import time
import pyco_de_gallo as gallo

dev = gallo.open_strict()
dev.gpio_set_config(0, gallo.GpioDirection.Output, gallo.GpioPull.Disabled)

while True:
    dev.gpio_put(0, True)
    time.sleep(0.5)
    dev.gpio_put(0, False)
    time.sleep(0.5)
```

### React to GPIO edges (push events)

```python
import pyco_de_gallo as gallo

dev = gallo.open_strict()

# The subscription must be live before the firmware will buffer events.
with dev.subscribe_gpio_events() as events:
    dev.gpio_subscribe(0, gallo.GpioEdge.Falling)
    for event in events:
        print(event.pin, event.edge, event.state, event.timestamp_us)
```

### Scan the I²C bus

```python
import pyco_de_gallo as gallo

dev = gallo.open_strict()
dev.i2c_set_config(gallo.I2cFrequency.Standard)

for addr in dev.i2c_scan(include_reserved=False):
    print(f"0x{addr:02x}")
```

## Error handling

Failures from the underlying transport and firmware are raised as Python
`RuntimeError` exceptions with a descriptive message (for example, an I²C
NACK, a GPIO timeout, or a firmware schema mismatch).

## More examples

Runnable scripts for every peripheral — including a live TMP108 temperature
graph, a PWM sine-wave fade, a SPI loopback test, and a 1-Wire ROM search —
live in the
[`examples/`](https://github.com/OpenDevicePartnership/pico-de-gallo/tree/main/crates/pyco-de-gallo/examples)
directory.

## Documentation

- Project book: <https://balbi.sh/pico-de-gallo/>
- Source and issues: <https://github.com/OpenDevicePartnership/pico-de-gallo>

## License

MIT
