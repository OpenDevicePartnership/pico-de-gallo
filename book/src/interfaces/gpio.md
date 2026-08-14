# GPIO

Pico de Gallo exposes 4 general-purpose I/O pins (**GPIO 0–3**) mapped to
RP2350 **GPIO 8–11**.

## Pin Mapping

| Gallo Pin | RP2350 GPIO |
|-----------|-------------|
| 0         | 8           |
| 1         | 9           |
| 2         | 10          |
| 3         | 11          |

## Operations

| Operation | Description |
|-----------|-------------|
| **Get** | Read the current pin state (High or Low) |
| **Put** | Drive a pin High or Low |
| **Set Config** | Configure pin direction (input/output) and pull resistor (none/up/down) |
| **Monitor** | Subscribe to edge events on a pin (rising, falling, or any) |

## Pin Configuration

Before using a GPIO pin, configure its direction and pull resistor. Pins
default to input with no pull resistor after power-on.

### CLI

```bash
# Configure pin 0 as input with pull-up
gallo gpio set-config --pin 0 --direction input --pull up

# Configure pin 2 as output with no pull
gallo gpio set-config --pin 2 --direction output --pull none
```

### Rust Library

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, GpioDirection, GpioPull};

fn configure_pins(gallo: &PicoDeGallo) {
    // Configure pin 0 as input with pull-up
    smol::block_on(async {
        gallo
            .gpio_set_config(0, GpioDirection::Input, GpioPull::Up)
            .await
            .unwrap();

        // Configure pin 2 as output with no pull
        gallo
            .gpio_set_config(2, GpioDirection::Output, GpioPull::None)
            .await
            .unwrap();
    });
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"

void configure_pins(PicoDeGallo *gallo) {
    /* Configure pin 0 as input with pull-up */
    GalloStatus rc = gallo_gpio_set_config(
        gallo, 0, GpioDirection_Input, GpioPull_Up
    );
    if (rc != GalloStatus_Ok) {
        fprintf(stderr, "set-config failed: %d\n", rc);
    }

    /* Configure pin 2 as output with no pull */
    gallo_gpio_set_config(gallo, 2, GpioDirection_Output, GpioPull_None);
}
```

## Reading and Writing Pins

### CLI

```bash
# Read the state of pin 0
gallo gpio get --pin 0
# Output: Pin 0: High

# Drive pin 2 high
gallo gpio put --pin 2 --level high

# Drive pin 2 low
gallo gpio put --pin 2 --level low
```

`--level` is required and takes `high` or `low`; there is no short option
for it, because `-h` is `--help`.

### Rust Library

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, GpioState};

async fn read_write(gallo: &PicoDeGallo) {
    // Read pin 0
    let state = gallo.gpio_get(0).await.unwrap();
    println!("Pin 0 is {:?}", state);

    // Drive pin 2 high
    gallo.gpio_put(2, GpioState::High).await.unwrap();

    // Drive pin 2 low
    gallo.gpio_put(2, GpioState::Low).await.unwrap();
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

void read_write(PicoDeGallo *gallo) {
    /* Read pin 0 */
    bool high;
    GalloStatus rc = gallo_gpio_get(gallo, 0, &high);
    if (rc == GalloStatus_Ok) {
        printf("Pin 0: %s\n", high ? "High" : "Low");
    }

    /* Drive pin 2 high */
    gallo_gpio_put(gallo, 2, true);

    /* Drive pin 2 low */
    gallo_gpio_put(gallo, 2, false);
}
```

## Waiting for Pin State Changes

The library provides async methods that block until a pin reaches the
requested state or edge transition. These are useful for waiting on
external signals without polling.

### Rust Library

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

async fn wait_for_button(gallo: &PicoDeGallo) {
    // Wait until pin 1 goes high
    gallo.gpio_wait_for_high(1).await.unwrap();
    println!("Pin 1 is now high");

    // Wait until pin 1 goes low
    gallo.gpio_wait_for_low(1).await.unwrap();
    println!("Pin 1 is now low");

    // Wait for a rising edge on pin 1
    gallo.gpio_wait_for_rising_edge(1).await.unwrap();
    println!("Rising edge detected on pin 1");

    // Wait for a falling edge on pin 1
    gallo.gpio_wait_for_falling_edge(1).await.unwrap();
    println!("Falling edge detected on pin 1");

    // Wait for any edge on pin 1
    gallo.gpio_wait_for_any_edge(1).await.unwrap();
    println!("Edge detected on pin 1");
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

void wait_for_button(PicoDeGallo *gallo) {
    /* These calls block until the requested edge/state occurs */
    gallo_gpio_wait_for_high(gallo, 1);
    printf("Pin 1 is now high\n");

    gallo_gpio_wait_for_low(gallo, 1);
    printf("Pin 1 is now low\n");

    gallo_gpio_wait_for_rising_edge(gallo, 1);
    printf("Rising edge detected on pin 1\n");

    gallo_gpio_wait_for_falling_edge(gallo, 1);
    printf("Falling edge detected on pin 1\n");

    gallo_gpio_wait_for_any_edge(gallo, 1);
    printf("Edge detected on pin 1\n");
}
```

## Edge Event Monitoring

For continuous monitoring, subscribe to GPIO edge events on a pin. The
firmware streams `GpioEvent` structs to the host whenever the subscribed
edge is detected. Each event carries:

```rust,ignore
pub struct GpioEvent {
    pub pin: u8,
    pub edge: GpioEdge,
    pub state: GpioState,
    pub timestamp_us: u64,
}
```

- **`pin`** — the Gallo pin number (0–3)
- **`edge`** — the edge that triggered the event (`Rising` or `Falling`)
- **`state`** — the pin state after the edge
- **`timestamp_us`** — firmware timestamp in microseconds

### CLI

The `monitor` subcommand subscribes to edge events and prints them until
you press **Ctrl+C**:

```bash
# Monitor rising edges on pin 0
gallo gpio monitor --pin 0 --edge rising

# Monitor any edge on pin 1
gallo gpio monitor --pin 1 --edge any
```

Example output:

```
[  12345 µs] Pin 0: Rising  → High
[  12890 µs] Pin 0: Rising  → High
[  45012 µs] Pin 0: Rising  → High
^C
Unsubscribed from pin 0.
```

### Rust Library

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, GpioEdge};

async fn monitor_pin(gallo: &PicoDeGallo) {
    // Subscribe to rising edges on pin 0
    gallo.gpio_subscribe(0, GpioEdge::Rising).await.unwrap();

    // Open a subscription to receive GpioEvent values (buffer depth 16)
    let mut sub = gallo.subscribe_gpio_events(16).await.unwrap();

    // Process events
    for _ in 0..100 {
        let event = sub.recv().await.unwrap();
        println!(
            "[{:>8} µs] Pin {}: {:?} → {:?}",
            event.timestamp_us, event.pin, event.edge, event.state
        );
    }

    // Unsubscribe when done
    gallo.gpio_unsubscribe(0).await.unwrap();
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdio.h>

void monitor_pin(PicoDeGallo *gallo) {
    /* Subscribe to rising edges on pin 0 */
    gallo_gpio_subscribe(gallo, 0, GpioEdge_Rising);

    /* ... receive events via topic subscription API ... */

    /* Unsubscribe when done */
    gallo_gpio_unsubscribe(gallo, 0);
}
```

## HAL Usage

The `pico-de-gallo-hal` crate implements the standard `embedded-hal`
traits over the GPIO pins, providing a familiar interface for portable
device drivers.

### Blocking Traits

The HAL implements these blocking traits from `embedded-hal`:

- **`OutputPin`** — `set_high()` / `set_low()`
- **`InputPin`** — `is_high()` / `is_low()`
- **`StatefulOutputPin`** — `is_set_high()` / `is_set_low()`

```rust,no_run
use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};
use pico_de_gallo_hal::Hal;

fn blink_and_read(hal: &Hal) {
    let mut led = hal.output_pin(2);
    let button = hal.input_pin(0);

    // Drive pin 2 high
    led.set_high().unwrap();

    // Read pin 0
    if button.is_high().unwrap() {
        println!("Button pressed");
    }

    // Check what we're currently driving
    if led.is_set_high().unwrap() {
        println!("LED is on");
    }

    led.set_low().unwrap();
}
```

### Async Trait

The HAL implements the `embedded-hal-async` **`Wait`** trait for
non-blocking edge/level detection:

```rust,no_run
use embedded_hal_async::digital::Wait;
use pico_de_gallo_hal::Hal;

async fn wait_for_signal(hal: &Hal) {
    let mut pin = hal.input_pin(1);

    pin.wait_for_high().await.unwrap();
    println!("Pin went high");

    pin.wait_for_rising_edge().await.unwrap();
    println!("Rising edge detected");
}
```

## Complete Example: Button-Controlled LED

This example configures pin 0 as an input (button with pull-up) and
pin 2 as an output (LED). It toggles the LED on each button press.

### CLI

```bash
# Configure pins
gallo gpio set-config --pin 0 --direction input --pull up
gallo gpio set-config --pin 2 --direction output --pull none

# Read button, toggle LED manually
STATE=$(gallo gpio get --pin 0)
gallo gpio put --pin 2 --level high

# Or monitor button presses
gallo gpio monitor --pin 0 --edge falling
```

### Rust Library

```rust,no_run
use pico_de_gallo_lib::{
    PicoDeGallo, GpioDirection, GpioPull, GpioState, GpioEdge,
    PicoDeGalloError, GpioError,
};

async fn button_led() -> Result<(), PicoDeGalloError<GpioError>> {
    let gallo = PicoDeGallo::new();

    // Configure pin 0 as input with pull-up (button)
    gallo.gpio_set_config(0, GpioDirection::Input, GpioPull::Up).await?;

    // Configure pin 2 as output (LED)
    gallo.gpio_set_config(2, GpioDirection::Output, GpioPull::None).await?;

    let mut led_on = false;

    loop {
        // Wait for button press (falling edge because of pull-up)
        gallo.gpio_wait_for_falling_edge(0).await?;

        led_on = !led_on;
        let state = if led_on { GpioState::High } else { GpioState::Low };
        gallo.gpio_put(2, state).await?;
    }
}
```

### C (FFI)

```c
#include "pico_de_gallo.h"
#include <stdbool.h>

int button_led(void) {
    PicoDeGallo *gallo = gallo_new();
    if (!gallo) return -1;

    /* Configure pin 0 as input with pull-up (button) */
    gallo_gpio_set_config(gallo, 0, GpioDirection_Input, GpioPull_Up);

    /* Configure pin 2 as output (LED) */
    gallo_gpio_set_config(gallo, 2, GpioDirection_Output, GpioPull_None);

    bool led_on = false;

    for (;;) {
        /* Wait for button press (falling edge) */
        gallo_gpio_wait_for_falling_edge(gallo, 0);

        led_on = !led_on;
        gallo_gpio_put(gallo, 2, led_on);
    }

    return 0;
}
```

## Error Handling

All GPIO operations return errors through the standard `PicoDeGalloError`
wrapper. GPIO-specific errors are represented as
`PicoDeGalloError<GpioError>`:

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, PicoDeGalloError, GpioError, GpioState};

async fn safe_read(gallo: &PicoDeGallo) {
    match gallo.gpio_get(0).await {
        Ok(state) => println!("Pin 0: {:?}", state),
        Err(PicoDeGalloError::Rpc(e)) => {
            eprintln!("RPC error: {e:?}");
        }
        Err(PicoDeGalloError::Endpoint(gpio_err)) => {
            eprintln!("GPIO error: {gpio_err:?}");
        }
        Err(e) => {
            eprintln!("Other error: {e:?}");
        }
    }
}
```

## API Reference

### Lib Methods

All methods are async and available on `PicoDeGallo`:

| Method | Returns | Description |
|--------|---------|-------------|
| `gpio_get(pin: u8)` | `Result<GpioState, ...>` | Read pin state |
| `gpio_put(pin: u8, state: GpioState)` | `Result<(), ...>` | Set pin state |
| `gpio_set_config(pin, direction, pull)` | `Result<(), ...>` | Configure direction and pull |
| `gpio_wait_for_high(pin: u8)` | `Result<(), ...>` | Wait until pin is high |
| `gpio_wait_for_low(pin: u8)` | `Result<(), ...>` | Wait until pin is low |
| `gpio_wait_for_rising_edge(pin: u8)` | `Result<(), ...>` | Wait for low→high transition |
| `gpio_wait_for_falling_edge(pin: u8)` | `Result<(), ...>` | Wait for high→low transition |
| `gpio_wait_for_any_edge(pin: u8)` | `Result<(), ...>` | Wait for any transition |
| `gpio_subscribe(pin: u8, edge: GpioEdge)` | `Result<(), ...>` | Subscribe to edge events on a pin |
| `gpio_unsubscribe(pin: u8)` | `Result<(), ...>` | Unsubscribe from edge events |
| `subscribe_gpio_events(depth)` | `Result<Subscription<GpioEvent>, ...>` | Open a subscription to receive GPIO events |

### FFI Functions

All functions return `GalloStatus`:

| Function | Description |
|----------|-------------|
| `gallo_gpio_get(gallo, pin, out_high)` | Read pin state into `*out_high` |
| `gallo_gpio_put(gallo, pin, high)` | Set pin state |
| `gallo_gpio_set_config(gallo, pin, direction, pull)` | Configure direction and pull |
| `gallo_gpio_wait_for_high(gallo, pin)` | Block until pin is high |
| `gallo_gpio_wait_for_low(gallo, pin)` | Block until pin is low |
| `gallo_gpio_wait_for_rising_edge(gallo, pin)` | Block until rising edge |
| `gallo_gpio_wait_for_falling_edge(gallo, pin)` | Block until falling edge |
| `gallo_gpio_wait_for_any_edge(gallo, pin)` | Block until any edge |
| `gallo_gpio_subscribe(gallo, pin, edge)` | Subscribe to edge events |
| `gallo_gpio_unsubscribe(gallo, pin)` | Unsubscribe from edge events |

### CLI Commands

| Command | Description |
|---------|-------------|
| `gallo gpio get --pin N` | Read pin state |
| `gallo gpio put --pin N --level high` | Drive pin high |
| `gallo gpio put --pin N --level low` | Drive pin low |
| `gallo gpio set-config --pin N --direction DIR --pull PULL` | Configure pin |
| `gallo gpio monitor --pin N --edge EDGE` | Stream edge events until Ctrl+C |

## Limitations

- **4 pins only** — GPIO 0–3 (RP2350 GPIO 8–11).
- **Shared with Logic Capture** — pins used by an active capture session
  cannot be used for GPIO operations. They are returned automatically
  when capture stops.
- **No analog** — all pins are digital only.
- **Edge event timestamps** come from the firmware's microsecond timer,
  not the host clock. Events are timestamped when the edge is detected
  on the RP2350.
