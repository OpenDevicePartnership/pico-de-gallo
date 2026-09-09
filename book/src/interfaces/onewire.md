# 1-Wire Bus

> **Hardware revision note:** 1-Wire requires **hw-rev2** firmware. On v1
> hardware, 1-Wire endpoints return `OneWireError::Unsupported`.

Pico de Gallo provides 1-Wire bus support through the RP2350's PIO (Programmable
I/O) state machine hardware. The 1-Wire data pin is on **GPIO 16**, configured
in open-drain mode.

## Operations

| Operation | Description |
|-----------|-------------|
| **Reset** | Resets the bus and detects device presence |
| **Read** | Reads N bytes from the bus |
| **Write** | Writes raw bytes to the bus |
| **Write Pullup** | Writes bytes then applies strong pullup for parasitic-power devices |
| **Search** | Starts a new ROM search and returns the first device |
| **Search Next** | Continues the current ROM search |

Direction determines the payload ceiling. `onewire_read` may return at most
`MAX_RESPONSE_PAYLOAD` (1014 bytes). `onewire_write` and
`onewire_write_pullup` may send at most `MAX_TRANSFER_SIZE` (4096 bytes).
Every host surface checks these limits before transmitting and reports
`BufferTooLong` for an over-ceiling call.

## DS18B20 Temperature Sensor Example

The DS18B20 is the most popular 1-Wire device. Here's how to read its
temperature using each interface.

### Protocol Refresher

- **Skip ROM** (`0xCC`): addresses all devices (single-device bus)
- **Convert T** (`0x44`): starts temperature conversion (needs 750ms with
  strong pullup for parasitic power)
- **Read Scratchpad** (`0xBE`): reads 9 bytes of sensor data
- Temperature is in bytes 0–1 (signed 16-bit, little-endian, 1/16°C resolution)

### CLI

```bash
# 1. Discover devices on the bus
gallo onewire search

# 2. Reset the bus
gallo onewire reset

# 3. Start temperature conversion with 750ms strong pullup
gallo onewire write-pullup --data cc44 --duration 750

# 4. Reset again before reading
gallo onewire reset

# 5. Send Read Scratchpad command
gallo onewire write --data ccbe

# 6. Read 9-byte scratchpad
gallo onewire read --len 9

# Parse temperature from the first 2 bytes:
# e.g., bytes [50, 01] → 0x0150 = 336 → 336 / 16.0 = 21.0°C
```

### Rust Library

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

async fn read_ds18b20_temperature(gallo: &PicoDeGallo) -> f32 {
    // Reset bus, check presence
    let present = gallo.onewire_reset().await.unwrap();
    assert!(present, "No device on bus");

    // Skip ROM + Convert Temperature, strong pullup for 750ms
    gallo.onewire_write_pullup(&[0xCC, 0x44], 750).await.unwrap();

    // Reset again
    gallo.onewire_reset().await.unwrap();

    // Skip ROM + Read Scratchpad
    gallo.onewire_write(&[0xCC, 0xBE]).await.unwrap();

    // Read 9-byte scratchpad
    let data = gallo.onewire_read(9).await.unwrap();

    // Temperature is in bytes 0–1 (little-endian, 12-bit signed fixed-point)
    let raw = i16::from_le_bytes([data[0], data[1]]);
    raw as f32 / 16.0
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

float read_ds18b20(PicoDeGallo *gallo) {
    bool present;
    gallo_onewire_reset(gallo, &present);
    if (!present) {
        fprintf(stderr, "No device on bus\n");
        return -999.0f;
    }

    // Skip ROM + Convert T with 750ms strong pullup
    uint8_t convert_cmd[] = {0xCC, 0x44};
    gallo_onewire_write_pullup(gallo, convert_cmd, 2, 750);

    // Reset before reading
    gallo_onewire_reset(gallo, &present);

    // Skip ROM + Read Scratchpad
    uint8_t read_cmd[] = {0xCC, 0xBE};
    gallo_onewire_write(gallo, read_cmd, 2);

    // Read 9-byte scratchpad
    uint8_t buf[9];
    uint16_t out_len;
    gallo_onewire_read(gallo, buf, 9, &out_len);

    // Temperature from bytes 0–1
    int16_t raw = (int16_t)(buf[0] | (buf[1] << 8));
    return raw / 16.0f;
}
```

### HAL

```rust,no_run
use pico_de_gallo_hal::Hal;

fn read_ds18b20_blocking(hal: &Hal) -> f32 {
    let ow = hal.onewire();

    let present = ow.reset().unwrap();
    assert!(present, "No device on bus");

    ow.write_pullup(&[0xCC, 0x44], 750).unwrap();
    ow.reset().unwrap();
    ow.write(&[0xCC, 0xBE]).unwrap();
    let data = ow.read(9).unwrap();

    let raw = i16::from_le_bytes([data[0], data[1]]);
    raw as f32 / 16.0
}
```

## Bus Scanning

To enumerate all devices on the 1-Wire bus:

```bash
gallo onewire search
```

Output:
```
Found 2 device(s):
  1: ROM ID 0x28FF123456780012 (family 0x28)
  2: ROM ID 0x28FF9ABCDE340056 (family 0x28)
```

The family code `0x28` identifies DS18B20 sensors. Each ROM ID is a unique
64-bit address: family code (1 byte) + serial number (6 bytes) + CRC (1 byte).

## Hardware Setup

Connect a DS18B20 (or other 1-Wire device) to **GPIO 16** with a 4.7kΩ
pull-up resistor to 3.3V:

```
3.3V ──┬── 4.7kΩ ──┬── GPIO 16 (data)
       │            │
       │          DS18B20
       │            │
      GND ─────── GND
```

For parasitic power mode (no separate VDD), use the `write-pullup` command
which drives the data line high after sending commands to supply power
through the bus itself.
