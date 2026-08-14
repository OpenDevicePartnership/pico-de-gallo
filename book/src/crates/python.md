# `pyco-de-gallo`

`pyco-de-gallo` exposes Pico de Gallo to Python as the `pyco_de_gallo` module.
It is built with **PyO3 + maturin**, and its API is intentionally boring in the
best way: open a device, call methods, get Python values back.

The key design point is that the Python surface is **synchronous**. Each
`PycoDeGallo` instance owns an internal Tokio runtime and drives the underlying
async Rust client for you.

That means you get Python-friendly code without giving up the Rust transport
layer underneath.

## Installation

`pyproject.toml` declares `requires-python = ">=3.8"`.

### From PyPI

When wheels are published, install it like any other Python package:

```console
$ pip install pyco-de-gallo
```

### From source with maturin

```console
$ cd crates/pyco-de-gallo
$ python -m pip install maturin
$ maturin develop --release
```

If you want a wheel instead of an editable/development install:

```console
$ cd crates/pyco-de-gallo
$ maturin build --release
```

## Opening a Device

At module level you get five entry points:

- `list_devices()`
- `open()` — lazy; failures surface on the first RPC
- `open_with_serial_number(serial_number)` — lazy, selects by serial
- `open_strict()` — validates the firmware's schema version before
  returning; raises `RuntimeError` on mismatch or device-not-found
- `open_strict_with_serial_number(serial_number)` — strict variant
  with serial selection (recommended for production)

```python
import pyco_de_gallo as gallo

for dev in gallo.list_devices():
    print(dev.serial_number, dev.manufacturer, dev.product)

pg = gallo.open()
# or:
# pg = gallo.open_with_serial_number("E6633861A34B8C24")
```

The returned object is `PycoDeGallo`.

## The `PycoDeGallo` Class

`PycoDeGallo` mirrors the Rust library closely.

- methods are synchronous from Python,
- Rust async work runs on an internal runtime,
- the GIL is released while the binding waits on USB I/O,
- most transport and endpoint failures become Python `RuntimeError`.

That gives you a straightforward, script-friendly surface:

```python
import pyco_de_gallo as gallo

pg = gallo.open()
print(pg.ping(123))
print(pg.version().major, pg.version().minor, pg.version().patch)
print(pg.device_info().hw_version)
print(pg.device_info().num_gpios)
```

## GPIO count and the SPI chip select

`DeviceInfo.num_gpios` and `PycoDeGallo.num_gpios()` both report the GPIO
count the connected board advertises. That is the runtime-authoritative
bound for a pin index and for `spi_batch`'s `cs_pin`; do not hardcode 4.

```python
n = pg.num_gpios()          # one implicit device/info round-trip, then cached
data = pg.spi_batch(0, ops) # cs_pin checked against n before anything is sent
```

`spi_batch` resolves the count and classifies `cs_pin` before it converts
the operation objects, so a refused chip-select costs nothing and drives
no pin. All failures raise `RuntimeError`, with disjoint messages:

```text
invalid SPI chip-select pin 7; device reports 4 GPIOs (valid 0..4)
device reports num_gpios=0; no SPI chip-select pin is available
failed to determine num_gpios: device/info did not respond within 300 seconds ...
```

Messages beginning `failed to determine num_gpios` mean the host could
not establish the valid range at all — transport failure, the 300-second
`device/info` timeout, legacy firmware, or a schema mismatch. They are
never phrased as a chip-select complaint. A failed lookup is not cached,
so retrying is allowed.

A `cs_pin` outside `0..=255` raises `OverflowError` during PyO3 argument
extraction, before the device is contacted.

> **Schema-freeze caveat.** On this branch a successful `validate()` or
> `open_strict()` does not prove wire-*shape* compatibility: `DeviceInfo`
> changed while both host and firmware still report schema 0.6.1.

## Enums and Value Types

The public Python names intentionally do **not** carry a `Py` prefix. You use
plain Python-facing names like:

- `I2cFrequency`
- `SpiPhase`
- `SpiPolarity`
- `GpioDirection`
- `GpioPull`
- `GpioEdge`
- `VersionInfo`
- `DeviceInfo`
- `UartConfigurationInfo`
- `SpiConfigurationInfo`
- `PwmDutyCycleInfo`
- `PwmConfigurationInfo`
- `AdcConfigurationInfo`

Example:

```python
import pyco_de_gallo as gallo

pg = gallo.open()
pg.i2c_set_config(gallo.I2cFrequency.Fast)
pg.spi_set_config(
    1_000_000,
    gallo.SpiPhase.CaptureOnFirstTransition,
    gallo.SpiPolarity.IdleLow,
)
```

## Example: I<sup>2</sup>C Register Read

```python
import pyco_de_gallo as gallo

pg = gallo.open()
pg.i2c_set_config(gallo.I2cFrequency.Fast)

data = pg.i2c_write_read(0x48, [0x00], 2)
raw = int.from_bytes(data, byteorder="big")
print(f"raw=0x{raw:04x}")
```

## Example: GPIO Blink

```python
import time
import pyco_de_gallo as gallo

pg = gallo.open()
pg.gpio_set_config(0, gallo.GpioDirection.Output, gallo.GpioPull.Disabled)

for _ in range(10):
    pg.gpio_put(0, True)
    time.sleep(0.1)
    pg.gpio_put(0, False)
    time.sleep(0.1)
```

## Example: ADC Read

```python
import pyco_de_gallo as gallo

pg = gallo.open()
raw = pg.adc_read(0)
config = pg.adc_get_config()
voltage_mv = raw * config.nominal_reference_mv / 4095

print(f"ADC0 raw={raw} ~{voltage_mv:.1f} mV")
```

## GPIO Event Subscriptions

GPIO push events are exposed through `subscribe_gpio_events()` and
`gpio_subscribe()`.

```python
import pyco_de_gallo as gallo

pg = gallo.open()
sub = pg.subscribe_gpio_events(depth=16)
pg.gpio_subscribe(0, gallo.GpioEdge.Any)

event = sub.poll(timeout=1.0)
if event is not None:
    print(event.pin, event.edge, event.state)

pg.gpio_unsubscribe(0)
sub.close()
```

The subscription object also supports iteration and context-manager cleanup.

## Error Handling

Rust-side errors are converted to `RuntimeError`.

```python
import pyco_de_gallo as gallo

pg = gallo.open()

try:
    pg.uart_set_config(0)
except RuntimeError as exc:
    print(f"operation failed: {exc}")
```

That includes transport failures, schema-validation failures, and peripheral
errors reported by the firmware.

## When to Use Python

Reach for `pyco-de-gallo` when you want quick experiments, production-test glue,
lab automation, or notebook-style investigation without writing a Rust binary.

If you outgrow the synchronous Python surface, the next layer down is
[`pico-de-gallo-lib`](./lib.md), which exposes the full async Rust API.