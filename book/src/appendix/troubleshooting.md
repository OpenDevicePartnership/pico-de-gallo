# Troubleshooting

## Device Doesn''t Show Up

### `gallo list` finds nothing

1. Confirm the LED on the Pico 2 is lit. If not, check the USB
   cable — many USB-C cables are power-only.
2. Confirm the firmware is flashed. Hold **BOOTSEL** while
   plugging in. If the board mounts as a `RPI-RP2` mass-storage
   device, the firmware **is not** running — see
   [Assembly & Flashing](../hardware/assembly.md).
3. **Linux**: install the udev rule from
   [USB & OS Notes](../getting-started/usb.md). Without it,
   `nusb` can''t claim the interface as a regular user.
4. **Windows**: install the WinUSB driver via Zadig, selecting
   **interface 0**. The default Windows USB driver does not expose
   vendor-specific endpoints to user space, and Pico de Gallo's
   second interface exists only for WebUSB descriptors.
5. Check `dmesg` (Linux), Device Manager (Windows), or
   `system_profiler SPUSBDataType` (macOS) for VID `045E` and
   PID `067D`.

### `gallo version` fails with a comms error

A successful `gallo list` followed by a failing `gallo version`
usually means another process has the device open — typically a
previous `gallo` instance that didn''t exit cleanly, or a Python
script holding a `PycoDeGallo`. Close it.

### `SchemaMismatch` (status code −63)

The host library was built against a different
`SCHEMA_VERSION_MINOR` than the running firmware. Re-flash the
matching firmware release, or upgrade/downgrade the host crates
to match. See [Releases & Compatibility](../internals/releases.md).

### `LegacyFirmware` (status code −64)

The firmware is too old to answer `device/info`. Re-flash a
recent firmware build.

## Peripheral Errors

### `Unsupported` (status code −65)

The peripheral exists in the protocol but isn''t wired on this
hardware revision. Confirm discovery with `gallo list` and firmware
communication with `gallo version`, then see
[Revisions](../hardware/revisions.md).

### `GpioTimeout` (status code −70)

A bounded GPIO wait expired before the requested level or edge arrived.
Check the wiring and increase the timeout only if the expected event is slow.

### `SpiInvalidCsPin` (status code −71)

The chip-select index is outside `0..num_gpios`. Query the connected device's
runtime GPIO count and choose an index below it.

### `SpiCsPinUnavailable` (status code −72)

The selected chip-select pin is explicitly configured as an input. Configure
it as an output, or select another user GPIO.

### `SpiCsPinMonitored` (status code −73)

The selected chip-select pin has an active GPIO event subscription. Unsubscribe
it before using the pin as chip-select.

### `SpiNoGpios` (status code −74)

The device reports zero user GPIOs, so no firmware-managed SPI chip-select is
available. Use a device or firmware build that exposes user GPIOs.

### `DeviceInfoTimeout` (status code −75)

The `device/info` query did not complete within 300 seconds. Check the USB
connection and peer compatibility before retrying.

### I²C `Nack` (−18)

The target didn''t acknowledge. Common causes:

- Wrong address. `gallo i2c scan` confirms which addresses ACK.
- Missing pull-ups. v1.1 boards have on-board 4.7 kΩ pull-ups;
  v1.0 does not.
- Target powered off, or VCC level mismatch (Pico de Gallo runs
  at 3.3 V on the bus pads).

### I²C `BusError` (−19) / `ArbitrationLoss` (−20)

Usually a wiring issue (long stub leads, no pull-ups, multi-master
collisions). Try a slower clock with `gallo i2c set-config --frequency standard`.

### `BufferTooLong` (−22)

Two different limits are involved here, and conflating them is a common
source of confusion:

- **The protocol constant, 4096 bytes** (`MAX_TRANSFER_SIZE`). This is the
  firmware's per-packet **buffer budget**, not a usable payload size — the
  same buffer must also hold the postcard-rpc header, the length varint and
  the COBS framing, and it covers the request frame *and* the response
  frame.
- **The usable payload, which is smaller and shape-dependent.** In measured SPI
  tests, 4096-byte TX-only and 3072-byte full-duplex requests passed their local
  checks, reached the transport, and failed `-ECOMM`.

Split larger transfers. Batch operations still occupy one framed request and
response; batching does not make an otherwise undeliverable aggregate fit.

The Zephyr SPI driver rejects clocked lengths above 1013 bytes. That is
containment, not a duplex-capacity guarantee: TX-only 1013 succeeded, TX-only
1015 wedges the dispatcher, and 1014 was not tested. Full duplex succeeded at
512, failed at 3072, and was not tested from 513 through 1013. Applications
needing a documented-safe duplex size must use 512 bytes or less. Do not infer
1013-byte duplex support from `PDG_SPI_MAX_BUFFER`.

The Zephyr limit does not protect CLI, Rust, C, Python, or MCP callers. In the
reproduced SPI tests the device resumed after USB re-enumeration (`usbipd
detach`/attach on Windows/WSL). This is an observation, not proof that detach
cancels the handler. On Linux/macOS reconnect the cable or use USB
unbind/rebind; power-cycle if unavailable or ineffective.
`system/reset-subscriptions` cannot run while dispatch is blocked.

### GPIO `WrongDirection` (−28)

You read from a pin configured as output (or vice versa). Call
`gpio_set_config` first with the matching direction.

## Firmware Build Issues

### `embassy-usb-driver 0.2.1` breaks the build

A known regression. Pin `embassy-usb-driver = "=0.2.0"` in the
firmware `Cargo.toml`. See `AGENTS.md` §13.10 for the full story.

### `elf2uf2-rs 2.2.0` from crates.io is stale

The CI installs it from git because the crates.io version is
missing the `--family` flag. Use `picotool` or install from git.

## Driver Development

### "It worked over USB but not on the real MCU"

A few things to check:

- **Timing.** USB introduces ~1 ms round-trip latency. If your
  driver relies on tight inter-byte timing (some 1-Wire and
  WS2812-style protocols), the on-host loop will look fine while
  the on-target loop fails.
- **Clock speed.** Pico de Gallo''s I²C / SPI clocks are
  configurable but discrete. Confirm the target MCU''s HAL can
  produce the same speed.
- **Pull-ups and levels.** Voltage-level mismatches show up as
  intermittent NACKs.

### `transaction()` is slower than expected

If you call `embedded_hal::i2c::I2c::write_read()` or
`SpiDevice::transaction()` and it issues multiple USB round-trips,
the HAL didn''t catch the batchable case. File an issue with the
Operations list — coverage for newer trait shapes is an active
work area.

## Where to Get Help

- File issues at
  [github.com/OpenDevicePartnership/pico-de-gallo](https://github.com/OpenDevicePartnership/pico-de-gallo/issues).
- Discussions:
  [github.com/OpenDevicePartnership/pico-de-gallo/discussions](https://github.com/OpenDevicePartnership/pico-de-gallo/discussions).
- The `AGENTS.md` file at the repo root has §13 "Common Gotchas"
  written from real regressions — worth a read before opening an
  issue.
