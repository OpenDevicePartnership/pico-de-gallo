# ADC (Analog-to-Digital Converter)

> **Hardware revision note:** ADC requires **hw-rev2** firmware. On v1
> hardware, ADC endpoints return `AdcError::Unsupported`.

Pico de Gallo exposes the RP2350's ADC peripheral for single-shot analog
reads. Four GPIO-based channels are available, each providing 12-bit
resolution over a 0–3.3 V nominal input range.

## Channel Mapping

| Channel | GPIO | Enum Variant |
|---------|------|--------------|
| 0       | 26   | `Adc0`       |
| 1       | 27   | `Adc1`       |
| 2       | 28   | `Adc2`       |
| 3       | 29   | `Adc3`       |

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `NUM_ADC_GPIO_CHANNELS` | 4 | Number of GPIO-based ADC channels |
| `ADC_RESOLUTION_BITS` | 12 | Bits of resolution per sample |
| `ADC_NOMINAL_REFERENCE_MV` | 3300 | Nominal reference voltage in millivolts |

## Operations

| Operation | Description |
|-----------|-------------|
| **Read** | Reads a single 12-bit sample from the specified channel |
| **Info** | Returns ADC configuration (resolution, reference voltage, channel count) |

## Voltage Conversion

The ADC returns a raw 12-bit unsigned value (0–4095). Convert to
millivolts with:

```
voltage_mv = (raw * 3300) / 4095
```

For example, a raw reading of `2048` corresponds to approximately
**1649 mV**.

## Types

### AdcChannel

```rust,no_run
enum AdcChannel {
    Adc0,
    Adc1,
    Adc2,
    Adc3,
}
```

### AdcConfigurationInfo

```rust,no_run
struct AdcConfigurationInfo {
    resolution_bits: u8,
    nominal_reference_mv: u16,
    num_gpio_channels: u8,
}
```

### AdcError

```rust,no_run
enum AdcError {
    ConversionFailed,
    Other,
    Unsupported,
}
```

## CLI

```bash
# Read a single sample from channel 0
gallo adc read --channel 0

# Read from channel 2
gallo adc read --channel 2

# Show ADC configuration
gallo adc info
```

Example output for `gallo adc read --channel 0`:

```
ADC channel 0: raw 2048 (≈ 1649 mV)
```

Example output for `gallo adc info`:

```
ADC Configuration:
  Resolution:       12 bits
  Reference:        3300 mV
  GPIO channels:    4
```

## Rust Library

All library methods are async. `PicoDeGallo::new()` is **not** async.

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, AdcChannel};

fn main() {
    let gallo = PicoDeGallo::new().unwrap();

    smol::block_on(async {
        // Read a single sample from channel 0
        let raw = gallo.adc_read(AdcChannel::Adc0).await.unwrap();
        let voltage_mv = (raw as u32 * 3300) / 4095;
        println!("ADC0: raw {raw}, ~{voltage_mv} mV");

        // Query ADC configuration
        let config = gallo.adc_get_config().await.unwrap();
        println!(
            "Resolution: {} bits, Reference: {} mV, Channels: {}",
            config.resolution_bits,
            config.nominal_reference_mv,
            config.num_gpio_channels,
        );
    });
}
```

### Error Handling

`adc_read` returns `Result<u16, PicoDeGalloError<AdcError>>`. The
`AdcError` variants are:

- **`ConversionFailed`** — the ADC hardware reported a conversion error.
- **`Other`** — an unspecified ADC error.
- **`Unsupported`** — ADC is not available on this hardware revision.

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, AdcChannel, AdcError, PicoDeGalloError};

async fn read_adc(gallo: &PicoDeGallo) {
    match gallo.adc_read(AdcChannel::Adc1).await {
        Ok(raw) => println!("ADC1: {raw}"),
        Err(PicoDeGalloError::Endpoint(AdcError::ConversionFailed)) => {
            eprintln!("ADC conversion failed");
        }
        Err(e) => eprintln!("Unexpected error: {e:?}"),
    }
}
```

## C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

void read_adc(PicoDeGallo *gallo) {
    uint16_t raw;
    GalloStatus rc = gallo_adc_read(gallo, ADC_CHANNEL_0, &raw);
    if (rc != GALLO_STATUS_OK) {
        fprintf(stderr, "ADC read failed: %d\n", rc);
        return;
    }

    uint32_t voltage_mv = ((uint32_t)raw * 3300) / 4095;
    printf("ADC0: raw %u, ~%u mV\n", raw, voltage_mv);
}

void adc_info(PicoDeGallo *gallo) {
    GalloAdcConfigurationInfo info;
    GalloStatus rc = gallo_adc_get_config(gallo, &info);
    if (rc != GALLO_STATUS_OK) {
        fprintf(stderr, "ADC config failed: %d\n", rc);
        return;
    }

    printf("Resolution: %u bits\n", info.resolution_bits);
    printf("Reference:  %u mV\n", info.nominal_reference_mv);
    printf("Channels:   %u\n", info.num_gpio_channels);
}
```

## HAL

At the HAL layer the ADC is accessed directly on the `Hal` struct
(no sub-object needed):

```rust,no_run
use pico_de_gallo_hal::Hal;
use pico_de_gallo_internal::AdcChannel;

fn read_adc(hal: &Hal) {
    let raw = hal.adc_read(AdcChannel::Adc0).unwrap();
    let voltage_mv = (raw as u32 * 3300) / 4095;
    println!("ADC0: raw {raw}, ~{voltage_mv} mV");

    let config = hal.adc_get_config().unwrap();
    println!("Resolution: {} bits", config.resolution_bits);
}
```
