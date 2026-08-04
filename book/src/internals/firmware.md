# The Firmware

The Pico de Gallo firmware lives in its own Cargo workspace at
`crates/pico-de-gallo-firmware/`. That separation is intentional: it targets
`thumbv8m.main-none-eabihf`, is `no_std`, and carries its own committed
`Cargo.lock`.

## Runtime model

The firmware runs on the RP2350 using Embassy:

- `embassy-executor` for async task scheduling,
- `embassy-rp` for RP2350 peripherals,
- `embassy-usb` for the USB device stack.

`postcard-rpc` sits on top of that USB transport and dispatches endpoint
handlers into async peripheral code. Requests are serialized on a shared
context, while background tasks handle work such as GPIO event publication.

> [!TIP]
> This is why the firmware can do DMA-backed transfers and interrupt-driven I/O
> without turning into a hand-written state machine maze.

### Watchdog

A dedicated `watchdog_feeder_task` arms the RP2350 hardware watchdog at
**2 seconds** and feeds it every **800 ms**. The 800 ms cadence leaves
margin for embassy scheduling jitter while keeping the worst-case
recovery time under 2 seconds when a handler genuinely wedges.

The feeder is a **separate embassy task**, not part of any RPC handler.
This is deliberate: postcard-rpc dispatches handlers serially on a
shared context, and a wedged handler would also wedge any handler-based
feed scheme. The dispatcher-wedge regression closed in
`pico-de-gallo-firmware` 0.11.0 (see CHANGELOG) is exactly the scenario
this defense covers — even with the GPIO `wait_for_*` timeout fix, any
future unbounded await in a handler will trip the watchdog within 2 s
and reset the device.

`pause_on_debug(true)` is set so an attached debugger session does not
reset the chip while you single-step. The watchdog is the same on both
`hw-rev1` and `hw-rev2` (no rev-specific code).

## USB descriptors

The device enumerates as VID `045e` / PID `067d` with
`bDeviceClass = 0xFF` (vendor-specific), so no OS class driver claims
it. `embassy_usb::Config` defaults `bcdUSB` to `0x0210`, which is what
makes hosts request the BOS descriptor at all.

### Interfaces

| # | Class | Endpoints | Purpose |
|---|-------|-----------|---------|
| 0 | `0xFF` | bulk IN + bulk OUT | The postcard-rpc transport |
| 1 | `0xFF` | none | Carries the WebUSB capability only |

Interface 0 **must** stay first. The two interfaces share an identical
class/subclass/protocol triple, so ordering is the only thing that
distinguishes them. On Linux and macOS the host claims the first
class-`0xFF` interface it finds; on Windows it claims interface 0
outright, because interfaces cannot be enumerated through WinUSB.
Interface 1 exists because
`WebUsb::configure()` cannot append a capability to an existing
interface — `bos_capability()` is only reachable through an
`InterfaceAltBuilder`, and embassy-usb exposes no public
`Builder::bos_writer()`.

### BOS capabilities

| Capability | Bytes | Written by |
|---|---:|---|
| BOS header | 5 | embassy-usb, automatically |
| `USB_2_0_EXTENSION` | 7 | embassy-usb, automatically |
| `PLATFORM` — Microsoft OS 2.0 | 28 | postcard-rpc |
| `PLATFORM` — WebUSB | 24 | firmware, via `WebUsb::configure()` |

Listed by size, not by wire order: the WebUSB capability is appended
during builder setup, while the Microsoft OS 2.0 one is written later,
inside `Builder::build()`.

Those sizes are as of embassy-usb 0.5.1 and postcard-rpc 0.12.1. That
is 64 bytes of the 256-byte BOS buffer sized by the `WireStorage` type
alias in `main.rs`. Overflowing that buffer, or the configuration
descriptor buffer, is a `"Descriptor buffer full"` panic raised at the
point the descriptor is written.

The MS OS 2.0 capability is what makes Windows bind WinUSB without an
INF file. The WebUSB capability advertises the landing page described
in [USB & OS Notes](../getting-started/usb.md).

### Vendor codes

Both platform capabilities reserve a `bRequest` value for
vendor-specific control transfers on the device:

| Descriptor | Vendor code |
|---|---|
| Microsoft OS 2.0 | `0x00` (set by postcard-rpc) |
| WebUSB | `0x01` (`WEBUSB_VENDOR_CODE` in `main.rs`) |

They are kept distinct deliberately, though not out of strict necessity.
embassy-usb answers a vendor request as an MS OS 2.0 descriptor request
only when the `bRequest` matches **and** `wIndex` is `7`; WebUSB's
`GET_URL` uses `wIndex` `2`, so the two would not actually collide even
if they shared a code. Using separate codes avoids relying on that
disambiguation.

## `no_std` and logging

This crate is `no_std`. Logging uses `defmt` over RTT.

> [!IMPORTANT]
> There is no `println!` fallback in firmware. If you need diagnostics, use
> `defmt`.

## Hardware revisions

Two feature flags select the board revision:

| Feature | Default | Board | Capabilities |
|---------|---------|-------|--------------|
| `hw-rev1` | yes | v1.0 | I<sup>2</sup>C, SPI, GPIO, PWM |
| `hw-rev2` | no | v1.1+ | I<sup>2</sup>C, SPI, UART, GPIO, PWM, ADC, 1-Wire |

On `hw-rev1`, unsupported peripherals return `Unsupported` instead of touching
unrouted hardware.

Build the two variants exactly as CI does:

```bash
cd crates/pico-de-gallo-firmware

cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf

cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2
```

## Peripheral notes

The RP2350 pin map matches the hardware docs in
[Pinout & Connector](../hardware/pinout.md):

- I<sup>2</sup>C uses I2C1 on GPIO 2/3 and runs asynchronously with Embassy.
- SPI uses SPI0 on GPIO 4/6/7 and supports DMA-backed full-duplex transfers.
- UART uses UART0 on GPIO 0/1 with buffered, interrupt-driven I/O.
- GPIO user pins are GPIO 8-11, with wait and subscribe support.
- PWM outputs are GPIO 12-15 on slices 6 and 7.
- 1-Wire uses PIO0 state machine 0 on GPIO 16.
- ADC reads are single-shot samples on GPIO 26-29 in firmware, with board
  routing exposing ADC0-2 on current hardware.

The shared transfer buffer is 4096 bytes (`MAX_TRANSFER_SIZE`), and handlers
validate lengths before indexing into it.

## Dependency pins that matter

The firmware intentionally pins `embassy-usb-driver = "=0.2.0"`.
That exact version is documented in
[`AGENTS.md`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/AGENTS.md)
because `0.2.1` pulled in an incompatible `embedded-io-async` update for the
current `embassy-usb 0.5` stack.

That documentation is part of the contributor contract: exact pins are not
supposed to look mysterious.

## Flashing

Flashing is the normal Pico UF2 flow:

1. Hold `BOOTSEL` while connecting USB.
2. Wait for the `RP2350` mass-storage device to appear.
3. Drag and drop the firmware `.uf2`.
4. The board auto-resets and reconnects with the new firmware.

After flashing, `gallo version` is the quickest sanity check because it shows
firmware version, schema version, hardware revision, and capabilities.
