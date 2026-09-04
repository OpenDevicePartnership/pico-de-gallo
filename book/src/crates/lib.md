# `pico-de-gallo-lib`

[`pico-de-gallo-lib`](https://docs.rs/pico-de-gallo-lib) is the main Rust host
library. It gives you a typed async client, `PicoDeGallo`, for every endpoint
exposed by the firmware.

If you are writing a Rust application, this is usually the crate you want.
`gallo`, the HAL crate, the FFI crate, and the Python bindings all build on top
of it.

## Connection Model

`PicoDeGallo::new()` and `PicoDeGallo::new_with_serial_number()` are **synchronous
constructors**. They enumerate USB when called and **panic** if no matching board
is present or the interface cannot be claimed; the fallible `PicoDeGallo::try_new()`
and `PicoDeGallo::try_new_with_serial_number()` return a `Result<PicoDeGallo, String>`
instead. They do not perform an async handshake up front — once constructed, the
client completes the connection in the background and per-RPC calls (not the
constructor) fail if the link drops later.

That gives you a simple startup story:

- create the client synchronously,
- call async methods for real work,
- optionally `validate()` once if you want a strict compatibility check.

### Constructors and discovery

| Item | What it does |
|---|---|
| `PicoDeGallo::new()` | Targets the first matching board the host sees |
| `PicoDeGallo::new_with_serial_number(serial)` | Targets one specific board by USB serial |
| `PicoDeGallo::try_new()` | Fallible `new()` — `Err` instead of panic when absent/unclaimable |
| `PicoDeGallo::try_new_with_serial_number(serial)` | Fallible `new_with_serial_number()` |
| `list_devices()` | Returns `DeviceDescription` values for every attached board |
| `wait_closed().await` | Resolves when the underlying USB connection closes |

```rust,no_run
use pico_de_gallo_lib::{PicoDeGallo, list_devices};

fn main() {
    for dev in list_devices() {
        println!(
            "serial={:?} manufacturer={:?} product={:?}",
            dev.serial_number,
            dev.manufacturer,
            dev.product,
        );
    }

    let _first = PicoDeGallo::new();
    let _named = PicoDeGallo::new_with_serial_number("E6633861A34B8C24");
}
```

## Minimal Example

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gallo = PicoDeGallo::new();

    let echoed = gallo.ping(0x1234_5678).await?;
    println!("ping: 0x{echoed:08x}");

    let version = gallo.version().await?;
    println!(
        "firmware v{}.{}.{}",
        version.major,
        version.minor,
        version.patch,
    );

    Ok(())
}
```

> [!NOTE]
> The library is async because USB I/O is async. The constructor is not. Put the
> client inside your async application and await the operations that actually hit
> the device.

## Error Model

Most methods return `Result<T, PicoDeGalloError<E>>`.

That split is deliberate:

- `PicoDeGalloError::Comms(...)` means the transport failed: disconnect,
  wire decode issue, closed connection, and similar host-side problems.
- `PicoDeGalloError::Endpoint(E)` means the request made it to firmware and the
  endpoint itself reported an error.
- `PicoDeGalloError::Timeout { waited }` means the request was transmitted but
  no reply arrived within this call's bound. See [Call Timeouts](#call-timeouts).

The endpoint-specific `E` is one of the protocol error enums:

- `I2cError`
- `SpiError`
- `UartError`
- `GpioError`
- `PwmError`
- `AdcError`
- `OneWireError`
- plus `I2cBatchError` / `SpiBatchError` for batched operations

That means you can match exactly the layer you care about:

```rust,no_run
use pico_de_gallo_lib::{I2cError, PicoDeGallo, PicoDeGalloError};

async fn read_sensor(gallo: &PicoDeGallo) {
    match gallo.i2c_read(0x48, 2).await {
        Ok(bytes) => println!("got {bytes:?}"),
        Err(PicoDeGalloError::Endpoint(I2cError::NoAcknowledge)) => {
            eprintln!("device did not ACK");
        }
        Err(PicoDeGalloError::Comms(_)) => {
            eprintln!("USB or transport problem");
        }
        Err(err) => eprintln!("other error: {err}"),
    }
}
```

## Call Timeouts

Every RPC is bounded. Without a bound, a request whose response is never
produced parks the caller forever, because there is no lower layer that gives
up. That is not hypothetical: a request frame larger than the firmware's
receive buffer is dropped before the dispatcher ever sees it, so the board
stays perfectly healthy — measured answering 40 of 40 concurrent pings — while
the call waits indefinitely.

The default is `DEFAULT_CALL_TIMEOUT`, **5 seconds**, roughly twice the
slowest measured legitimate operation (a 4096-byte 1-Wire read, about 2.4 s).

Calls that can legitimately take longer are not bounded by that default. They
derive their own bound from what the firmware itself allows, plus the
configured call timeout as transport slack:

| Call | Firmware-side term |
|---|---|
| `i2c_scan` | `UNDECLARED_DISPATCH_BUDGET_MS` (10 s) — it probes up to 128 addresses |
| `uart_read` | the caller's `timeout_ms` |
| `gpio_wait_*_with_timeout` | the caller's `timeout_ms` |
| `gpio_wait_*` (no timeout) | `MAX_HANDLER_TIMEOUT_MS` (30 min) |
| `onewire_write_pullup` | the caller's `pullup_duration_ms` |
| `spi_batch` | the sum of its `DelayNs` operations |
| `device_info`, `validate` | `DEVICE_INFO_TIMEOUT` (300 s) |

Note that a `timeout_ms` of `0` selects the firmware's 30-minute ceiling
rather than meaning "wait forever"; the host mirrors that clamp exactly.

Override the default with `with_call_timeout`, which also widens every
derived bound:

``rust,no_run
use pico_de_gallo_lib::PicoDeGallo;
use std::time::Duration;

let gallo = PicoDeGallo::try_new()?.with_call_timeout(Duration::from_secs(30));
# Ok::<(), String>(())
``

A timed-out call leaves the handle usable: abandoning it drops the reply
waiter, and sequence numbers are never reused, so a late reply is discarded
rather than mismatched onto a later call.

### Concurrency caveat

Firmware dispatch is **serial** — one handler runs at a time. A call issued
while another is in flight waits for that one to finish before its own handler
starts, and the host cannot observe when that happens.

So if you share one handle across tasks and one of them issues a long call (a
`gpio_wait_*`, or an `spi_batch` carrying large `DelayNs` operations),
concurrent calls can exceed the default through no fault of the device.

Sequential use — which is what the `embedded-hal` implementations produce,
and the overwhelmingly common pattern — is unaffected. If you do drive a board
concurrently alongside long calls, raise the bound.

## `validate()` and Schema Compatibility
`validate().await` is the strict compatibility gate.

It calls the `device/info` endpoint and checks the firmware's schema version
against the host library's compiled-in schema version from
`pico-de-gallo-internal`.

Pre-1.0, **schema minor version must match**. If host and firmware were built
against different wire schemas, `validate()` fails instead of letting you debug
mysterious decoding problems later.

`validate()` returns the `DeviceInfo` on success, so you can immediately inspect
firmware version, hardware revision, capability bits, and the informational
firmware identity through `DeviceInfo::build_id()`. The build identity answers
which image is running, but `validate()` deliberately ignores it: only schema
major/minor compatibility determines whether validation succeeds.

The `device/info` round-trip is bounded at **300 seconds**. On expiry
`validate()` returns `ValidateError::Timeout`, which is deliberately
distinct from `ValidateError::Comms`: nothing failed at the transport
layer, the request is simply still outstanding, so there is no transport
error to carry. The bound is generous because firmware dispatch is serial
and a legal maximum-length SPI batch can occupy it for about 275 seconds;
the point is that the wait is finite, not that it is short.

```rust,no_run
use pico_de_gallo_lib::PicoDeGallo;

#[tokio::main]
async fn main() {
    let gallo = PicoDeGallo::new();

    match gallo.validate().await {
        Ok(info) => println!(
            "fw {}.{}.{} schema {}.{}.{} hw={} capabilities={:?}",
            info.fw_major,
            info.fw_minor,
            info.fw_patch,
            info.schema_major,
            info.schema_minor,
            info.schema_patch,
            info.hw_version,
            info.capabilities,
        ),
        Err(err) => eprintln!("compatibility check failed: {err}"),
    }
}
```

The failure modes are explicit:

- `ValidateError::Comms` — the host could not talk to the device,
- `ValidateError::LegacyFirmware` — firmware is too old for `device/info`,
- `ValidateError::SchemaMismatch` — host and firmware do not agree on the wire
  schema.

## `num_gpios()` and the SPI Chip-Select Bound

`num_gpios().await` returns the GPIO count the connected device reports.
That is the runtime-authoritative bound for a chip-select index; prefer it
over the compile-time `NUM_GPIOS`, which is only this build's default.

The value is resolved lazily. A cache miss performs one implicit
`validate()` — so the same 300-second bound and the same schema check
apply — and stores the result. Handles cloned from the same connection
share that cache, so a warm lookup costs no USB traffic. A failed lookup
is not cached, so the next call retries. A reported count of zero is a
legitimate, cacheable answer rather than a miss.

`spi_batch(cs_pin, ops)` uses that bound to refuse a bad chip-select
*before* it encodes anything or transmits an `spi/batch` request, and
returns `SpiBatchCallError`:

| Variant | Meaning |
|---|---|
| `DeviceInfo(ValidateError)` | The count could not be established — transport, 300-second timeout, legacy firmware, or schema mismatch. **Never** a chip-select complaint. |
| `NoGpios` | The device reports zero GPIOs. Distinct from an out-of-range index. |
| `InvalidCsPin { cs, num_gpios }` | `cs` is at or beyond the reported count. Carries the caller's index verbatim. |
| `Comms(HostErr)` | The `spi/batch` request itself failed at the transport layer. |
| `Endpoint(SpiBatchError)` | The firmware executed the batch and refused or failed an operation, with `failed_op`. |

A local refusal transmits nothing, drives no pin, and never fabricates a
`failed_op` index. A cached count belongs to the handle that learned it:
if the board is unplugged, that handle keeps the byte and a
plausible-looking request will reach the batch RPC and fail with `Comms`.
The client never rebinds itself to a different board, and a freshly
constructed handle starts cold.

## GPIO Topic Subscriptions

GPIO edge events are push-based topics, not request/response endpoints.

The flow is:

1. open a host-side subscription with `subscribe_gpio_events(depth).await`,
2. tell firmware which pin to monitor with `gpio_subscribe(pin, edge).await`,
3. receive `GpioEvent` values from the returned `MultiSubscription<GpioEvent>`,
4. call `gpio_unsubscribe(pin).await` when you are done.

```rust,no_run
use pico_de_gallo_lib::{GpioEdge, PicoDeGallo};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let gallo = PicoDeGallo::new();
    let mut events = gallo.subscribe_gpio_events(16).await?;

    gallo.gpio_subscribe(0, GpioEdge::Any).await?;

    if let Ok(event) = events.recv().await {
        println!("pin {} -> {:?}", event.pin, event.edge);
    }

    gallo.gpio_unsubscribe(0).await?;
    Ok(())
}
```

> [!TIP]
> Open the topic subscription before you start monitoring pins. That way the
> host already has a buffer waiting when the first edge arrives.

## Endpoint Catalog

The library exposes one typed async method per firmware capability.

| Method | Arguments | Purpose |
|---|---|---|
| `ping` | `id` | Echo a `u32` back from firmware |
| `i2c_read` | `address`, `count` | Read bytes from an I<sup>2</sup>C target |
| `i2c_write` | `address`, `contents` | Write bytes to an I<sup>2</sup>C target |
| `i2c_write_read` | `address`, `contents`, `count` | Write, then read with a repeated start |
| `i2c_scan` | `include_reserved` | Scan the I<sup>2</sup>C bus for responding addresses |
| `i2c_batch` | `address`, `ops` | Execute one bus transaction: no STOP between adjacent same-direction operations, repeated START on direction changes, and one final STOP |
| `i2c_set_config` | `frequency` | Set the I<sup>2</sup>C clock frequency |
| `i2c_get_config` | — | Read back the active I<sup>2</sup>C frequency |
| `spi_read` | `count` | Read bytes from the SPI bus |
| `spi_write` | `contents` | Write bytes to the SPI bus |
| `spi_transfer` | `contents` | Full-duplex SPI transfer |
| `spi_flush` | — | Flush pending SPI traffic |
| `spi_batch` | `cs_pin`, `ops` | Execute atomic multi-step SPI traffic under chip-select (see below) |
| `spi_set_config` | `spi_frequency`, `spi_phase`, `spi_polarity` | Set SPI timing and mode |
| `spi_get_config` | — | Read back the active SPI configuration |
| `uart_read` | `count`, `timeout_ms` | Read up to `count` bytes; `0` is a 1 ms poll, oversized non-zero timeouts clamp at 30 minutes |
| `uart_write` | `contents` | Queue bytes for UART transmit |
| `uart_flush` | — | Wait until UART TX has drained |
| `uart_set_config` | `baud_rate` | Set UART baud rate |
| `uart_get_config` | — | Read back the active UART configuration |
| `gpio_get` | `pin` | Read a GPIO level |
| `gpio_put` | `pin`, `state` | Drive a GPIO high or low |
| `gpio_wait_for_high` | `pin` | Wait until a pin reads high, bounded by the 30-minute firmware ceiling |
| `gpio_wait_for_low` | `pin` | Wait until a pin reads low, bounded by the 30-minute firmware ceiling |
| `gpio_wait_for_rising_edge` | `pin` | Wait for a rising edge, bounded by the 30-minute firmware ceiling |
| `gpio_wait_for_falling_edge` | `pin` | Wait for a falling edge, bounded by the 30-minute firmware ceiling |
| `gpio_wait_for_any_edge` | `pin` | Wait for either edge, bounded by the 30-minute firmware ceiling |
| `gpio_wait_for_*_with_timeout` | `pin`, `timeout` | Use a shorter GPIO wait; zero and oversized values select the 30-minute ceiling, and expiry returns `GpioError::Timeout` |
| `gpio_set_config` | `pin`, `direction`, `pull` | Set GPIO direction and pull resistor |
| `gpio_subscribe` | `pin`, `edge` | Ask firmware to monitor a pin for edge events |
| `gpio_unsubscribe` | `pin` | Stop firmware-side monitoring |
| `version` | — | Read the firmware version |
| `device_info` | — | Read firmware version, schema version, HW revision, capabilities, runtime GPIO count, and build identity |
| `validate` | — | Perform a strict schema compatibility check and return `DeviceInfo` |
| `num_gpios` | — | Read the device-reported GPIO count; validates lazily on the first call, then caches |
| `pwm_set_duty_cycle` | `channel`, `duty` | Set a raw PWM duty-cycle value |
| `pwm_get_duty_cycle` | `channel` | Read current and maximum PWM duty |
| `pwm_enable` | `channel` | Enable the PWM slice behind a channel |
| `pwm_disable` | `channel` | Disable the PWM slice behind a channel |
| `pwm_set_config` | `channel`, `frequency_hz`, `phase_correct` | Set PWM frequency and phase-correct mode |
| `pwm_get_config` | `channel` | Read the active PWM configuration |
| `adc_read` | `channel` | Read one ADC sample |
| `adc_get_config` | — | Read ADC capabilities and constants |
| `onewire_reset` | — | Reset the 1-Wire bus and detect presence |
| `onewire_read` | `len` | Read raw 1-Wire bytes |
| `onewire_write` | `data` | Write raw 1-Wire bytes |
| `onewire_write_pullup` | `data`, `pullup_duration_ms` | Write, then hold the line high for parasitic-power devices |
| `onewire_search` | — | Start ROM search and return the first device |
| `onewire_search_next` | — | Continue the current ROM search |

> [!WARNING]
> The Rust library is not protected by the Zephyr driver's 1013-byte
> containment. A 1015-byte TX-only SPI request reproduced a device-wide
> firmware-dispatcher wedge. Keep individual SPI payloads at or below 512 bytes;
> see [troubleshooting](../appendix/troubleshooting.md#buffertoolong-22).

For the full API surface, field docs, and current signatures, use the crate
reference on [docs.rs](https://docs.rs/pico-de-gallo-lib).
