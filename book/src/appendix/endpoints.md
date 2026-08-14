# Endpoint Catalog

This is the canonical list of postcard-rpc endpoints and topics
exposed by the Pico de Gallo firmware. The source of truth is the
`endpoints!` and `topics!` invocations in
[`crates/pico-de-gallo-internal/src/lib.rs`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/crates/pico-de-gallo-internal/src/lib.rs).

> [!IMPORTANT]
>
> **Variant order in shared enums is part of the wire ABI.**
> postcard encodes enum variants by their index (0, 1, 2…), not by
> the discriminant value. Reordering or removing a variant is a
> breaking schema change. New variants must be appended at the end.
> See `AGENTS.md` §6 for the full rule.

Each schema break bumps `SCHEMA_VERSION_MINOR` (pre-1.0). The host
library refuses to talk to a firmware whose major/minor version
doesn''t match — that''s where `Status::SchemaMismatch` (−63) comes
from.

## Endpoints

| Path                      | Description                                                  |
|---------------------------|--------------------------------------------------------------|
| `ping`                    | Echo a `u32`. Useful for liveness checks.                    |
| `version`                 | Firmware version triple (major / minor / patch).             |
| `device/info`             | Firmware version + schema version + capability bitfield + runtime GPIO count. |
| `i2c/read`                | Read N bytes from a target address.                          |
| `i2c/write`               | Write bytes to a target address.                             |
| `i2c/write-read`          | Write then read on the same target (repeated start).         |
| `i2c/scan`                | Probe every address on the bus.                              |
| `i2c/batch`               | Sequence of I²C ops in one USB round-trip.                   |
| `i2c/set-config`          | Set I²C frequency (`I2cFrequency` enum).                     |
| `i2c/get-config`          | Query current I²C frequency.                                 |
| `spi/read`                | Clock in N bytes (MISO).                                     |
| `spi/write`               | Clock out bytes (MOSI).                                      |
| `spi/transfer`            | Full-duplex transfer of equal-length TX and RX.              |
| `spi/flush`               | Wait for any in-flight DMA SPI ops to complete.              |
| `spi/batch`               | Sequence of SPI ops under chip-select in one round-trip. See the chip-select contract below. |
| `spi/set-config`          | Set frequency, CPHA, and CPOL.                               |
| `spi/get-config`          | Query current SPI configuration.                             |
| `uart/read`               | Read with timeout.                                           |
| `uart/write`              | Write bytes.                                                 |
| `uart/flush`              | Drain the TX FIFO.                                           |
| `uart/set-config`         | Set baud rate.                                               |
| `uart/get-config`         | Query current UART configuration.                            |
| `gpio/get`                | Read a pin.                                                  |
| `gpio/put`                | Write a pin.                                                 |
| `gpio/wait-high`          | Block until pin is high.                                     |
| `gpio/wait-low`           | Block until pin is low.                                      |
| `gpio/wait-rising`        | Block until rising edge.                                     |
| `gpio/wait-falling`       | Block until falling edge.                                    |
| `gpio/wait-any`           | Block until any edge.                                        |
| `gpio/set-config`         | Set direction and pull resistor.                             |
| `gpio/subscribe`          | Begin push-based edge events on a pin.                       |
| `gpio/unsubscribe`        | Stop push-based events on a pin.                             |
| `pwm/set-duty-cycle`      | Set raw compare value.                                       |
| `pwm/get-duty-cycle`      | Query current compare value and max.                         |
| `pwm/enable`              | Enable the PWM slice owning a channel.                       |
| `pwm/disable`             | Disable the PWM slice owning a channel.                      |
| `pwm/set-config`          | Set frequency and phase-correct mode.                        |
| `pwm/get-config`          | Query PWM configuration.                                     |
| `adc/read`                | Single-shot ADC read.                                        |
| `adc/get-config`          | Query ADC capabilities (channel count, resolution).          |
| `onewire/reset`           | 1-Wire reset + presence detection.                           |
| `onewire/read`            | Read N bytes from the 1-Wire bus.                            |
| `onewire/write`           | Write bytes on the 1-Wire bus.                               |
| `onewire/write-pullup`    | Write bytes then assert strong pullup (parasitic power).     |
| `onewire/search`          | Start a ROM search (returns first ROM).                      |
| `onewire/search-next`     | Continue a ROM search.                                       |
| `system/reset-subscriptions` | Tear down any GPIO subscriptions left over from a prior host. |

### `spi/batch` chip-select contract

A valid `cs_pin` is inside `0..DeviceInfo::num_gpios`, is not being
monitored for GPIO events, and is not explicitly configured as an input.
Refusals return `InvalidCsPin`, `CsPinMonitored`, and `CsPinUnavailable`
respectively, and **leave the pin unchanged**. An accepted pin is driven
as an output and left deasserted high.

## Topics (server → client push)

| Path           | Message     | Description                                              |
|----------------|-------------|----------------------------------------------------------|
| `gpio/event`   | `GpioEvent` | Push stream of edge events for subscribed GPIO pins.     |

The `gpio/event` topic is a **single multiplexed stream** carrying
events for every subscribed pin (each `GpioEvent` carries a
`pin: u8` field so the host can demultiplex). `gpio/subscribe(pin,
edge)` enables firmware-side monitoring of one pin; `gpio/unsubscribe(pin)`
tears it down. Events for any subscribed pin arrive on the shared
stream — there is no per-pin sub-channel and no implicit per-pin
stream-open/close.

Subscriptions are server-side state that outlives the USB transport,
so a host whose process crashed or was killed without unsubscribing
will leave the pin owned by a firmware monitor task. Hosts should
call `system/reset-subscriptions` once on connect (after
`device/info`) to reclaim any such pins. The endpoint is idempotent
and returns the number of subscriptions that were torn down.

Stale events: after `gpio/unsubscribe(pin)` returns `Ok`, a
`GpioEvent` for that pin may still arrive (the firmware best-effort
publishes events that were in-flight at unsubscribe time). Consumers
should filter against their current subscription set and drop
unknown-pin events without erroring.

## Adding Endpoints

If you''re contributing a new endpoint, the recipe touches six
crates plus tests and documentation. Don''t forget the
schema-version bump and the lockstep release of internal +
firmware + every host crate.