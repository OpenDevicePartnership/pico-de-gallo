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

The `watchdog_supervisor_task` arms the RP2350 hardware watchdog at 2 seconds,
but feeds it only while dispatch and transmit progress remain plausible. Two
independent slots track that progress:

- The **dispatch slot** arms after `receive()` returns a frame and disarms when
  postcard-rpc goes back to waiting. Most handlers receive the 10-second
  default budget. Slow handlers declare a longer bound; caller-supplied bounds
  are capped at 30 minutes and receive 30 seconds of reply-serialization slack.
- The **TX slot** arms while any `WireTx` sender is outstanding. Its 60-second
  budget covers TX-mutex starvation and GPIO event publishers that run outside
  the dispatcher.

The supervisor polls every 250 ms. The designed reset-latency bound is the
active slot's budget plus one poll period; this is a design target, not a
board-attached measurement. On expiry it logs the slot and frame breadcrumb,
writes them to watchdog scratch registers, and triggers a forced reset. Reset
drops USB and loses every GPIO subscription. On the next boot, RTT reports the
breadcrumb and clears it.

`pause_on_debug(true)` stops the hardware watchdog while the core is halted,
but Embassy time continues. A supervisor wake gap above 500 ms is therefore
treated as a debugger or scheduling discontinuity: live deadlines are shifted
by the gap instead of resetting immediately. This rule's board-attached
acceptance check is still pending.

The supervisor is a backstop, not complete hang detection:

- A wedge inside `receive()` itself is indistinguishable from legitimate idle
  and is not covered.
- The TX slot has no hardware acceptance trigger; its correctness rests on
  inspection.
- The single TX slot measures aggregate TX progress, not per-sender progress.
  One permanently starved sender remains masked while another sender completes
  at least once per 60-second `TX_BUDGET`.

The `wedge-test` feature provides a build-only acceptance hatch by disabling
the firmware's zero-length I<sup>2</sup>C write guard. It cannot be reached
through the normal Rust-library or MCP `i2c_write` routes because both reject
an empty payload host-side. Reproducing the wedge therefore also requires
temporarily removing one of those host guards locally, as an uncommitted test
mutation; the guards remain correct production behaviour.

The watchdog is the same on both `hw-rev1` and `hw-rev2` (no rev-specific
code). The firmware crate has no unit-test harness; none is implied by the pure
supervision policy function.

### Build identity

`crates/pico-de-gallo-firmware/build.rs` runs

```text
git describe --always --dirty --tags --match firmware-v*
```

and emits the result as a `BUILD_ID` constant, which `main()` logs at boot over
`defmt` and the `device/info` handler returns.

Every flag matters, and each has a *silent* failure mode:

| Flag | Removing it causes |
|---|---|
| `--tags` | The `firmware-v*` tags are a mix of annotated and lightweight. Without this, `git describe` sees only annotated tags and resolves hundreds of commits too far back. |
| `--match firmware-v*` | Describe picks the nearest tag in any namespace, returning an `application-v*` description. |
| `--always` | Describe fails outright when no matching tag is reachable, e.g. in a shallow CI clone. |
| `--dirty` | The locally-modified marker is lost — the single most useful part for a developer mid-bisect. |

When git is unavailable, exits non-zero, or there is no repository, the script
emits `"unknown"` and a `cargo:warning`. The build still succeeds, so a source
tarball remains buildable. Values longer than 64 bytes are truncated on a
UTF-8 character boundary.

The script re-runs on **every** build, by design. The pre-existing
`rerun-if-changed=memory.x` narrows re-runs to a single file, so without an
unconditional trigger the embedded identity would go stale across incremental
builds and keep reporting a clean tree for a dirty one. A stale identity is
worse than none, because it confirms a wrong conclusion rather than merely
failing to prevent one.

Release workflows must fetch tags (`fetch-depth: 0`), or `git describe`
degrades to a bare commit hash.

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
| `hw-rev1` | no — **deprecated** | v1.0 | I<sup>2</sup>C, SPI, GPIO, PWM |
| `hw-rev2` | yes | v1.1+ | I<sup>2</sup>C, SPI, UART, GPIO, PWM, ADC, 1-Wire |

On `hw-rev1`, unsupported peripherals return `Unsupported` instead of touching
unrouted hardware.

`hw-rev1` is deprecated: it is no longer the default, `build.rs` emits a
`cargo:warning` and `main()` logs a `defmt::warn!` at boot when it is enabled,
and it **will not be removed before 2031-09-01**. It stays fully supported and
built in CI until then. See
[Revisions: v1.0 vs v1.1](../hardware/revisions.md) for the user-facing policy.

Build the two variants exactly as CI does — note that the *default*
(no-flags) build is now `hw-rev2`:

```bash
cd crates/pico-de-gallo-firmware

# hw-rev2 (default)
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf

# hw-rev1 (deprecated — must opt in explicitly)
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1
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

### I²C batch transaction handling

`i2c/batch` uses two decode passes. The first validates the operation count,
encoding, declared count, total read size, and every write payload before the
bus is accessed. A malformed operation or zero-length write therefore reports
the exact offending index without putting a START on the bus.

After validation, the handler borrows the I<sup>2</sup>C peripheral and shared
scratch buffer as disjoint fields. Its second pass materialises the decoded
list into a `heapless::Vec<Operation, MAX_BATCH_OPS>`, carving non-overlapping
read slices from the scratch buffer while write operations borrow their
request data.

The handler then makes one
`embedded_hal_async::i2c::I2c::transaction()` call for the entire list.
Adjacent same-direction operations have no intervening STOP or repeated
START, a direction change re-addresses under a documented repeated START,
and only the last operation carries a STOP. The repeated START is documented
by the RP2350 vendor SVD: `IC_CON` resets to `0x00000065` with
`IC_RESTART_EN` (bit 5) set, and embassy-rp does not write that register.

Because a bus error is returned for the atomic transaction as a whole,
firmware reports it with `failed_op = 0`. Pre-bus validation errors retain
their exact zero-based operation index.

### SPI chip-select arbitration

`spi/batch` validates the operation count, encoding, declared count, and
total read size before inspecting its chip-select pin. These checks therefore
report their batch error before any chip-select error. Once the batch is
valid, firmware refuses a chip-select index outside
`0..DeviceInfo::num_gpios`, a pin owned by a GPIO event monitor, or a pin
explicitly configured as an input. These refusals occur before chip-select
is driven: an invalid index does not touch any pin, and the other refusals
leave the selected pin's direction, level, and pull unchanged.

A `LegacyAuto` pin or an explicitly configured output is accepted. Firmware
configures the pin as an output, drives it high, asserts it low for the batch,
and deasserts it high after execution even when an SPI operation fails. It
does not restore the prior direction or level and does not update the tracked
GPIO mode.

Changing an accepted pin to output does not alter its configured pull:
firmware changes only the SIO output-enable state, while the pull-up and
pull-down settings remain in the separate pad-control state.

The shared transfer buffer is 4096 bytes (`MAX_TRANSFER_SIZE`), and handlers
validate lengths before indexing into it. This is an internal buffer and
argument bound, not a demonstrated end-to-end application-payload guarantee;
framing and response shape reduce the deliverable size.

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
