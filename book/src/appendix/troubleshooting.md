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

### `DeviceInfoTimeout` (status code −75)

`device/info` produced no answer within the 300-second bound. The
obvious cause is a board that stopped responding, but it is not the
only one, and the host cannot tell them apart.

postcard-rpc derives every endpoint's key from the *schema of its
response type*. Changing the shape of `DeviceInfo` — adding, removing,
renaming, or retyping a field — therefore changes the `device/info`
key. If the host and the firmware were built from different trees, the
firmware answers under the other side's key, the receiving dispatcher
finds no match, and the frame is dropped without ever being decoded.
No error is raised, because nothing failed; the call simply never
completes. Retrying or replugging cannot help, since the mismatch is
fixed at compile time.

If you are running a development build of either side, rebuild both
from the same tree. `gallo version` still works in this situation: it
bounds `device/info` at five seconds and then falls back to the legacy
`version` endpoint, whose response type is unchanged and therefore
still matches.

### The board reset itself

The firmware's dispatch supervisor deliberately resets the board when an
in-flight request or transmit path exceeds its budget. USB disconnects and
re-enumerates, and active GPIO subscriptions are lost.

Attach an RTT reader and look for these `defmt` messages:

```text
supervisor: <slot> slot expired (key=<frame-key>) — resetting
previous boot ended in a supervisor-forced reset: slot=<slot> key=<frame-key>
```

`dispatch` identifies a request handler that did not complete within its
default or declared budget. `tx` identifies absence of aggregate transmit
completion. The hexadecimal key is the first four frame-header bytes retained
as a breadcrumb; include it with the preceding request when reporting a bug.

If RTT instead reports `previous boot ended in a watchdog reset not raised by
the supervisor`, the executor itself stopped making enough progress to feed the
2-second hardware watchdog. A cold boot has neither message.

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

The argument exceeds one of three independent ceilings. Choose the ceiling by
the direction of the bytes and by whether the call is a batch, not by the
endpoint:

| Direction | Constant | Value | Examples |
|---|---|---:|---|
| device → host | `MAX_RESPONSE_PAYLOAD` | 1014 | reads, the read half of I²C write-read, batch returning operations, all of full-duplex SPI transfer |
| host → device | `MAX_TRANSFER_SIZE` | 4096 | writes and the write half of I²C write-read |
| host → device, whole frame | `MAX_REQUEST_FRAME` | 5119 | a batch's aggregate outgoing bytes, header and encoding included |

`MAX_RESPONSE_PAYLOAD` is derived from postcard-rpc's host inbound USB buffer:
1024 − 7 bytes of response header (1 discriminant, 2 key, 4 sequence) − 1
postcard `Result` discriminant − 2 bytes of varint length prefix = 1014. It is a
host-transport property that firmware cannot observe. `spi_transfer` makes the
distinction visible: it is full duplex, so its whole argument is limited to
1014, while send-only `spi_write` accepts 4096. Issue #158 measured
`spi/write` succeeding at 1015 bytes exactly where `spi/transfer` fails.

`MAX_REQUEST_FRAME` applies only to `i2c/batch` and `spi/batch`, because every
other endpoint is kept a kilobyte clear of it by `MAX_TRANSFER_SIZE`. It bounds
the aggregate, so a batch of individually modest writes can trip it; and it
does *not* cap an individual batch `Write` at 4096, because a batch write never
enters the firmware's scratch buffer. See
[Transaction Batching](../interfaces/batching.md#the-request-frame-ceiling),
and use `i2c_batch_request_frame_len` / `spi_batch_request_frame_len` rather
than modelling the overhead by hand.

Every host surface now rejects an over-ceiling argument locally before
transmitting. Rust and MCP report `BufferTooLong`; C returns
`Status::BufferTooLong`; Python raises `RuntimeError`; Zephyr returns
`-EMSGSIZE`. No new C `Status` value was added. To fix the call, split or chunk
the transfer. A batch's returning operations count in aggregate, so dividing a
large read into operations inside one batch does not evade the response limit;
the same is true of its outgoing operations and the frame limit.

The historical measurements explain why these guards exist. Before issue #179,
a batch returning 1015–4096 bytes executed its bus operations and only then lost
the response with `Postcard(DeserializeUnexpectedEnd)`; #179 added the aggregate
batch ceiling. Before #158, plain I²C reads showed the same 1014/1015 edge, while
I²C write requests reached 4096 and initiated a transaction at an unpopulated
address. That verifies request framing, not successful clocking of a 4096-byte
payload, and no I²C hang found in that range proves none exists. Before #186 an
over-ceiling batch *frame* produced no error at all — see the next section.

These figures come from the issue #158 measurements on board
`5256657D8A5D7F03` and the issue #179 and #186 measurements on board
`49742081C885AC69`.

The older Zephyr SPI guidance capped transfers at 1013 and documented full
duplex only through 512 because 1014 and the 513–1013 range had not been tested.
Issue #158 superseded it: `spi/transfer` was measured at 1014 successfully, and
1015 now fails cleanly. `PDG_SPI_MAX_BUFFER` is therefore 1014 and is tied to
`GALLO_MAX_RESPONSE_PAYLOAD` by a `_Static_assert`.

An earlier 1015-byte TX-only request did reproduce a device-wide dispatcher
wedge. Issue #158 could not reproduce it on firmware `62dd64e710fd`, after
issue #157 added the watchdog supervisor and issue #178 bounded every host RPC:
the call returned a clean error in 12 ms and the board stayed responsive. This
is a non-reproduction on one firmware build, not proof the earlier observation
never occurred.

If you are running firmware older than the #157 watchdog supervisor and do hit
a wedge, the recovery recorded at the time was USB re-enumeration: `usbipd
detach` followed by attach on Windows/WSL. That is an observed procedure, not
proof that detaching cancels the blocked handler. On Linux and macOS,
reconnect the cable or use USB unbind/rebind, and power-cycle if
re-enumeration is unavailable or ineffective. `system/reset-subscriptions`
cannot run while dispatch is blocked, so it is not a way out.

### A large request times out instead of returning `BufferTooLong`

There is a third ceiling, and crossing it produces silence rather than an
error. The whole request **frame** — postcard-rpc header plus encoded body —
must fit in the firmware's 5120-byte receive buffer, and the usable maximum is
5119 bytes. A longer frame is discarded by the server with no reply, so the
call ends in `Timeout` (or, on firmware older than the #178 host-side bounds,
hangs). The board stays fully responsive; this is not a wedge.

**Since issue #186 no supported call should reach this.** `MAX_TRANSFER_SIZE`
keeps every single-argument call clear of it — 4096 bytes of payload plus a
header and the request struct's own encoding lands a little over 4110 bytes —
and the batch endpoints, which were the one remaining route, are now bounded
against `MAX_REQUEST_FRAME` host-side and report `BufferTooLong` instead. If
you still see a timeout in this range, you are either on firmware and host
built from different trees, or you have found a new route into the ceiling
worth reporting.

Two details are worth knowing if you are measuring this yourself, because both
have produced wrong answers before:

- The usable payload is **six bytes smaller until the connection has received
  its first reply**. postcard-rpc's `HostClient` starts with an 8-byte key in
  the request header and narrows to the server's 2-byte key only after a reply
  arrives. A request that is itself dropped narrows nothing. This is also why
  the batch guard assumes the wide header and so refuses up to six bytes
  earlier than a warm connection strictly requires.
- A probe that does not reproduce the real client's header measures a
  different protocol. See
  [Wire Protocol](../internals/wire-protocol.md#the-request-frame-ceiling).

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

### An I²C `transaction()` gather write misbehaves

Firmware with schema 0.7 or newer executes the complete operation list as
one I²C transaction. Adjacent operations in the same direction run back to
back without a STOP or repeated START, so adjacent writes form one gather
write. A direction change emits a repeated START, and only the final
operation is followed by a STOP. Zero-length writes are rejected.

Older firmware executed every operation as an independent transaction with
its own START and STOP. If a gather write is split on the bus or the target
interprets adjacent writes separately, check `device/info` and update to
firmware reporting schema 0.7 or newer.

## Where to Get Help

- File issues at
  [github.com/OpenDevicePartnership/pico-de-gallo](https://github.com/OpenDevicePartnership/pico-de-gallo/issues).
- Discussions:
  [github.com/OpenDevicePartnership/pico-de-gallo/discussions](https://github.com/OpenDevicePartnership/pico-de-gallo/discussions).
- The `AGENTS.md` file at the repo root has §13 "Common Gotchas"
  written from real regressions — worth a read before opening an
  issue.
