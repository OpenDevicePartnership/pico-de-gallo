# Changelog

All notable changes to the Pico de Gallo Zephyr module will be documented in
this file.

The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- **`i2c_burst_write()` no longer returns `-ENOTSUP`.** The I2C controller
  accepted a `I2C_MSG_STOP`-delimited message group only when it held one
  message, or a write followed by a repeated-start read. Zephyr's
  `i2c_burst_write()` emits two *writes* — `I2C_MSG_WRITE`, then
  `I2C_MSG_WRITE | I2C_MSG_STOP` — which matched neither and was rejected. So
  was every hand-rolled gather write, which is where most affected in-tree
  sensor, EEPROM and display drivers actually are; `i2c_burst_write()` and
  `i2c_burst_write_dt()` were the only `i2c.h` helpers that produced the
  rejected shape.
  ([#102](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/102))

  A group's write messages now concatenate into a single payload and reach the
  bus as one transaction: one START, one address phase, all the bytes, one
  STOP. That is what a real controller emits for this message sequence. Three
  shapes are supported — N writes, a single read, and N writes followed by one
  read. Rejections are narrower but still explicit: a read that is not the last
  message of its group, and more than one read in a group, remain `-ENOTSUP`,
  because the FFI has no shape for reading and then continuing within one
  transaction and splitting the group would insert a STOP the caller never
  asked for.

  Requiring `I2C_MSG_RESTART` on the terminating read of a multi-message group
  is **unchanged**: a group that was rejected for lacking it before is still
  rejected for lacking it now.

  This was deliberately **not** fixed by routing through the `i2c/batch`
  firmware endpoint. Zephyr's SPI driver moved *away* from `spi/batch` for
  unrelated reasons, and more importantly `i2c/batch` only became atomic
  recently (#128) inside unreleased schema 0.7, where `validate()` cannot tell
  the two firmware behaviours apart. Depending on it would have converted a
  loud `-ENOTSUP` into silent register corruption against a firmware built
  before that fix. `gallo_i2c_write()` has no such ambiguity.

- **The transfer size limit is now checked per group, not per message.** Two
  4096-byte writes each passed the old per-message check and would have
  concatenated to 8192. The check is now an overflow-safe running total over a
  group's writes, with the group's terminating read bounded separately, both
  against the same 4096-byte figure and both returning `-EMSGSIZE`.

  This does not narrow what was previously accepted: the reachable payload
  range was already `[0, 4096]` through a single `i2c_msg`, and it still is.

  **4096 remains unmeasured.** It is `pico_de_gallo_internal::MAX_TRANSFER_SIZE`,
  the firmware's declared argument bound, and not a demonstrated end-to-end
  ceiling for this transport. `PDG_SPI_MAX_BUFFER` started from the same figure
  and had to be lowered to 1013 after 4096 was measured to fail `-ECOMM` and
  1015 to wedge the firmware dispatcher device-wide. `i2c/write` carries its
  payload in the request frame exactly as `spi/transfer` does. It was left at
  4096 rather than lowered by analogy, because a bound measured on a
  differently framed endpoint is a guess and lowering it would reject
  single-message writes that work today.
  ([#146](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/146))

- **A `NULL` message buffer is now refused only when the message claims to
  carry bytes.** Zephyr's I2C API permits `msg.buf == NULL` whenever
  `msg.len == 0`, and both `i2c_write(dev, NULL, 0, addr)` and
  `i2c_write_read(dev, addr, NULL, 0, rx, n)` produce exactly that. The driver
  rejected every `NULL` buffer with `-EINVAL` regardless of length.
  ([#137](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/137))

  Relaxing that check alone would have accomplished nothing. The FFI rejects a
  `NULL` pointer unconditionally, *before* any length check, because
  `slice::from_raw_parts(NULL, 0)` is undefined behaviour in Rust. So the
  pointer had to be kept away from it rather than merely allowed past the
  driver's own guard.

- **A validated group is now reduced to the operation its data bytes actually
  require.** A new `classify_group_()` judges a group by #147's `write_len` —
  already the concatenated total of its writes — and the length of its
  terminating read, and no message carrying zero bytes is forwarded:

  | group | operation |
  |---|---|
  | writes total > 0, read > 0 | write-then-read (unchanged) |
  | writes total > 0, no read or read == 0 | plain write; the empty read phase is dropped |
  | writes total == 0, read > 0 | plain read; the empty writes are dropped |
  | writes total == 0, no read or read == 0 | address-only probe |

  This composes with #147's gather rather than replacing it: `W(0 NULL) + W(3)`
  still gathers to a 3-byte write, and an empty write consumes none of the
  per-group size budget. No `NULL` pointer and no zero length reaches the FFI.

- **An address-only probe is refused with `-ENOTSUP`,** in the validation
  pre-pass — before the controller mutex and before any group reaches the bus,
  preserving the "validation is a complete pre-pass, no partial bus traffic"
  property #147 relies on.

  The refusal is a hardware limitation, not a policy choice. The RP2040/RP2350
  `DW_apb_i2c` block drives the address phase only as a side effect of pushing
  bytes into `IC_DATA_CMD`, so `START + ADDR + STOP` is physically unreachable
  ([rp-rs/rp-hal#678](https://github.com/rp-rs/rp-hal/issues/678),
  [embassy-rs/embassy#4474](https://github.com/embassy-rs/embassy/issues/4474)).

  This also corrects a **live wrong answer** that was independent of the `NULL`
  question. Zephyr's shell `i2c scan` (`drivers/i2c/i2c_shell.c`) probes each
  address with a single `{ buf = &dst, len = 0, flags = I2C_MSG_WRITE |
  I2C_MSG_STOP }` message. `buf` is non-`NULL` there, so the old check never
  blocked it: it reached `gallo_i2c_write(..., 0)` and, since the host-surface
  guard in #136, returned `InvalidArgument` for every address. **`i2c scan`
  prints `0 devices found` on a populated bus today.** At the new default it
  still reports every address absent, but for a stated reason rather than
  silently; with the new Kconfig below it produces a correct answer.

### Added

- `CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ` (default `n`). Substitutes a
  1-byte read to the same address for an address-only probe: an ACK means the
  device is present, a NACK surfaces as `-ENXIO`. That makes Zephyr's shell
  `i2c scan` work, at one blocking USB round trip per address — about 116 for
  the `0x04..0x77` range the shell scans.
  ([#137](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/137))

  Selected with `IS_ENABLED()` inside an ordinary `if`, never `#ifdef`, so both
  arms compile in every configuration.

  It is off by default for two reasons that must be documented rather than
  discovered. A read probe is **not** semantically identical to a write probe:
  a write-only device may acknowledge its write address while NACKing its read
  address, so a scan performed this way can under-report — the same known
  limitation the firmware's own `i2c/scan` carries. And the substituted read
  consumes a byte, which can pop a FIFO or step an EEPROM address pointer;
  Zephyr's own `i2c_shell.c` carries the same warning, that scanning "can
  confuse your I2C bus, cause data loss, and is known to corrupt the Atmel
  AT24RF08 EEPROM". A bus scan is a deliberate act, whereas a driver silently
  turning every zero-length write into a read is not.

  The supported way to scan through this bridge remains `gallo i2c scan`, or
  the firmware's `i2c/scan` endpoint, which probes the whole bus in one round
  trip.

- `CONFIG_HEAP_MEM_POOL_ADD_SIZE_PDG_I2C` (default 8192). The I2C driver now
  `k_malloc()`s a merge buffer for a group holding two or more non-empty
  writes, at most `PDG_I2C_MAX_BUFFER` (4096) bytes and at most one live at a
  time. Declaring the contribution is mandatory, not optional: `k_malloc()`
  lives in `kernel/mempool.c`, which Zephyr compiles only when the system heap
  is non-empty, and `CONFIG_HEAP_MEM_POOL_SIZE` defaults to 0 — omitting it
  reproduces #111 exactly, as an undefined reference to `k_malloc` at link
  time.

  Nothing allocates on the common paths. A single message, a single write
  followed by a read, and a group whose writes are all empty are each sent
  straight from the caller's buffers with no copy. Excluding the all-empty case
  from the merge path is deliberate: an empty payload has nothing to merge, and
  allocating for it would introduce an `-ENOMEM` failure mode on a call that
  can otherwise not fail locally. An all-empty group no longer reaches the
  firmware at all: #137 reduces it to an address-only probe and refuses it
  locally, described under *Fixed* above.

- `tests/pdg_i2c_burst`, a board-attached regression image for #102. Covers
  `i2c_burst_write()`, a three-message hand-rolled gather, a gathered
  write-then-read, the three rejection shapes, and the per-group running total
  — including that the oversize rejection emits no bus traffic at all. Every
  assertion compares against a reference read-back through a single-message
  `i2c_write()` rather than against the bytes written, so it holds on parts
  that mask register bits. Registered in `zephyr/scripts/ci-build.sh`, which
  brings the CI target count to nine; that gate builds and links it but, like
  every other target, never runs it.

### Verification

- **#137 is built-only, not behaviourally verified.** No Zephyr environment on
  the authoring machine, no host-side unit-test harness in the module, and no
  board. `.github/workflows/zephyr.yml` gates the PR (path-filtered,
  `zephyr/**` touched) but is **build-only** — it never executes a produced
  binary — and it builds only the default configuration, so the
  `CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ=y` arm compiles by construction
  (`IS_ENABLED`, not `#ifdef`) and is never exercised.

  One thing *was* executed. `struct pdg_i2c_group`, `enum pdg_i2c_op`,
  `validate_group_()` and `classify_group_()` were extracted **verbatim** from
  `pdg_i2c.c` into a throwaway host harness and driven through **18 group
  shapes**: the pre-existing shapes; #147's gather shapes crossed with zero
  lengths (`W(1)+W(0 NULL)`, `W(0 NULL)+W(3)`, `W(0)+W(0)`, `W(0)+W(0)+R(2)`,
  `W(2)+W(1)+R(0 NULL)`); the Zephyr shell scan probe; both `NULL`-buffer
  forms; #147's three rejection shapes, which still reject; and the per-group
  running total at and over the 4096-byte limit. All 18 matched, clean under
  `-Wall -Wextra -Werror -Wswitch-enum -Wformat=2` at `-O0`, `-Os`, `-O2` and
  `-O3`.

  The harness is **not committed and is not a test suite**. It covers four
  items and says nothing about `pdg_i2c_transfer()`, `gather_writes_()`, the
  Kconfig wiring, or any transport behaviour, and it cannot fail in CI. Note
  that `tests/pdg_i2c_burst` (added for #102) is board-attached and is not run
  by CI either.

- **Upstream `tests/drivers/spi/spi_loopback` on `native_sim/native/64`:
  41 PASS / 12 SKIP / 1 FAIL / 2 NOT BUILT.** This is **not** a clean upstream
  pass, and must not be read as one.

  The single FAIL is upstream's own `test_spi_complete_multiple_timed`. It is
  **unrunnable on this target rather than a driver defect**: `spi.c:406` asserts
  `time_spent_us >= minimum_transfer_time_us`, a **lower** bound, measured with
  the Zephyr clock — which on `native_sim` does not advance while the host
  thread is blocked inside a USB call, so every transfer measures 0 µs.
  `CONFIG_SPI_IDEAL_TRANSFER_DURATION_SCALING` bounds only the **upper** limit,
  so **no multiplier value can affect this assertion**. It fails on SLOW and
  passes on FAST purely because SLOW's theoretical minimum (432 µs) is larger;
  that is structural, not flaky.

  The 12 SKIPs are all expected and were verified exactly: five word sizes
  rejected by the driver with `-ENOTSUP`, `test_spi_deinit` (no
  `miso-gpios`/`mosi-gpios` on `zephyr,user`) and `test_spi_hold_on_cs`
  (HOLD without LOCK is unsupported), across two spec iterations. The 2 NOT
  BUILT are the async cases, which require `CONFIG_SPI_ASYNC=y`; this driver
  `BUILD_ASSERT`s that off.

### Known Issues

- **A 1015-byte TX-only `spi/transfer` never returns and wedges the firmware
  dispatcher device-wide.** Deterministic, reproduced across two byte-identical
  consecutive runs on board `5256657D8A5D7F03`.

  Once triggered, **every** subsequent RPC hangs — including from a freshly
  started host process, and including `system/reset-subscriptions`, which is
  the endpoint that exists to recover orphaned state. The condition therefore
  survives host process death entirely.

  The 2 s watchdog does **not** catch it: the dedicated feeder task keeps
  feeding while a request handler blocks, which is precisely the gap left open
  by the serial-dispatch hazard recorded in AGENTS.md §13.17 (2026-06-03).

  In the reproduced tests, the device resumed responding after USB
  re-enumeration. On Windows/WSL this was `usbipd detach` followed by attach.
  This is an observed procedure, not proof that detach directly cancels the
  blocked handler, and it has not been generalized to other dispatcher-wedge
  triggers. On Linux/macOS use cable reconnect or USB unbind/rebind; power-cycle
  if re-enumeration is unavailable or ineffective. The blocked dispatcher cannot
  service `system/reset-subscriptions`.

  Root cause is in the firmware/wire layer (`crates/`) and is out of scope for
  this module. `PDG_SPI_MAX_BUFFER = 1013` puts the hang out of reach *through
  this driver* by rejecting 1014 and above locally with `-EMSGSIZE` before any
  transport call, but that is containment, not a fix, and it does not prove
  that no other hang window exists below 1013.

### Breaking Changes

- **A zero-length I2C write now returns `-ENOTSUP` rather than `-EINVAL`.**
  Callers that passed a non-`NULL` buffer with `len == 0` previously reached
  `gallo_i2c_write(..., 0)` and got `InvalidArgument` → `-EINVAL` back from the
  host-surface guard (#136), or `I2cError::ZeroLengthWrite` from the firmware
  before it (#101). The group is now classified as an address-only probe and
  refused locally, with an errno that names the reason. Callers that were
  probing this way should either enable
  `CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ` or use a 1-byte read.
  ([#137](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/137))

- **The Zephyr SPI driver now rejects transfers over 1013 bytes.** Transfers
  above it return `-EMSGSIZE`
  locally, before any allocation, controller lock, set-config or chip-select
  edge. Applications transferring more than 1013 bytes in one call must split.
  If you are designing around large SPI transfers through this bridge, plan for
  this.

  `PDG_SPI_MAX_BUFFER` was previously documented as
  `pico_de_gallo_internal::MAX_TRANSFER_SIZE`, the "firmware single-transfer
  limit". That was wrong, and the wrong mental model produced two wrong values
  in succession. The constant is a **packet-buffer budget** that must hold the
  payload *plus* the postcard-rpc header, the length varint and the COBS
  framing, and it must cover the **request** frame *and* the **response**
  frame, so usable payload sits strictly below it:

  - **4096** (`MAX_TRANSFER_SIZE`) — 4096 TX-only passes the local check,
    reaches the transport and fails `-ECOMM`.
  - **3072** — a "conservative" guess reasoned from the firmware's
    `PacketBuffers<MAX_TRANSFER_SIZE + 1024>` headroom. 3072 **full duplex**
    also fails `-ECOMM`. That reasoning considered only one direction.
  - **1013** — the largest TX-only length measured to work on hardware.

  Every observed failure was `-ECOMM` and never `-EMSGSIZE`, so the transport
  was always the limiter and the compiled constant never was.

  **1013 is measured, not derived, and the picture is incomplete:** the TX-only
  boundary is unresolved between 1013 and 1015 (1014 was never probed, and 1015
  hangs); full duplex succeeded at 512, failed at 3072, and was not tested from
  513 through 1013; and while 1013 sits just under 1024 in a way
  that would be consistent with a ~1 KiB budget and ~11 bytes of framing, there
  is no evidence for that decomposition and it must not be relied on.

  Still owed: derive the usable `spi/transfer` payload ceiling from the
  worst-case request and response framing, express it as one generated or
  shared contract instead of a constant duplicated per consumer, and pin limit
  and limit+1 tests against it. That requires a wire-crate change with schema
  and lockstep-release implications, so it is out of scope for this module.

  Applications needing a documented-safe duplex size must use 512 bytes or
  less. `PDG_SPI_MAX_BUFFER = 1013` is containment, not a duplex-capacity
  guarantee, and the 4096 protocol constant is a packet-buffer/argument bound,
  not a demonstrated end-to-end application-payload guarantee.

- The `odp,pico-de-gallo-i2c` and `odp,pico-de-gallo-spi` controllers must
  now be **direct children of an enabled `odp,pico-de-gallo` parent**. A
  controller at the devicetree root, under a disabled parent, under an
  unrelated parent, or nested more than one level deep is rejected at build
  time with an explanatory static assertion rather than an unresolved device
  ordinal at link time.

- `serial-number` **moved from the controllers to the parent**. It is no
  longer declared on either controller binding, so a leftover child property
  fails devicetree processing with an undeclared-property error naming the
  node and the binding; it is not silently ignored. The parent's selection
  applies to all of its children. Omitting it remains safe only when exactly
  one matching board is physically attached: the host API cannot report which
  board it chose, and the build-time multi-parent check counts devicetree
  parents, not attached USB devices.

- The shield's controller nodes were renamed and reparented: `/pdg-i2c` and
  `/pdg-spi` are now `/pico-de-gallo/i2c` and `/pico-de-gallo/spi`. **Absolute
  devicetree node paths and the generated identifiers and dependency ordinals
  derived from them change accordingly.** The `pdg0`, `pdg_i2c0` and
  `pdg_spi0` labels are unchanged, so overlays that use `&pdg_i2c0` /
  `&pdg_spi0` continue to resolve; overlays or code referring to the absolute
  paths must be updated. Sample and application overlays must now also enable
  `&pdg0`.

- The parent is now the sole owner of the board's USB handle; the controllers
  borrow it and never close it. Happy-path I2C and SPI transfer behaviour is
  unchanged. Initialization ownership, failure coupling, failure location and
  worst-case boot latency change: one parent validation now gates both
  children. Physical USB opens were already deduplicated to one and remain
  one; registry calls and references drop from three to one. A parent open
  failure now fails both controllers coherently, at the parent's
  `POST_KERNEL/CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY` (default
  `KERNEL_INIT_PRIORITY_DEFAULT`, currently 40) rather than the controllers'
  `POST_KERNEL/50`, and the
  worst case falls from as many as three independent five-minute strict opens
  to one attempt plus two fast child failures. The cost is that a controller
  can no longer initialize independently of the parent.

- The SPI controller now uses **standard Zephyr `cs-gpios`**, and the
  temporary Pico-de-Gallo-specific chip-select index property has been
  **deleted**. `cs-gpios` is
  **required** on every enabled controller — a missing property fails
  devicetree processing, because the bridge has no native chip select to fall
  back to — and an SPI child node's `reg` regains its ordinary Zephyr meaning
  as an index into that array. Every entry must target an **enabled**
  `odp,pico-de-gallo-gpio` controller under the **same** `odp,pico-de-gallo`
  parent; a foreign controller, a disabled sibling, and a Pico de Gallo GPIO
  controller belonging to a *different* parent are each rejected at build time
  with an assertion naming the `cs-gpios` array index. The cross-parent case is
  the reason the check exists: it is a real, enabled Pico de Gallo GPIO port on
  a *different physical board*. The parent of an enabled SPI controller must
  now also define `serial-number`, as the GPIO child already required, because
  chip select actuates a physical pin. Enabling `&pdg_gpio0` is consequently a
  prerequisite for SPI. `GPIO_ACTIVE_HIGH` is permitted in `cs-gpios`;
  `SPI_CS_ACTIVE_HIGH` remains `-ENOTSUP`. This supersedes the temporary
  index-mapping property introduced for #104, which never shipped in a
  release; the underlying "`reg` is not a firmware GPIO index" fix is preserved
  by `cs-gpios` giving `reg` its standard meaning.

- **Chip select is no longer atomic with the data phase.** The Zephyr module
  stopped using the `spi/batch` firmware endpoint and now issues
  `spi/set-config`, `gpio/put` (assert), `spi/transfer` and `gpio/put`
  (deassert) — four USB round trips on an ordinary successful transceive, each
  independently fallible. Host death after the assert can leave chip select
  asserted; recovery is a fresh session that deasserts the pin, or a
  power-cycle. Only RPCs that return have defined behaviour: one that never
  returns leaves the call pending forever with no errno, no cleanup and the SPI
  lock still held. The non-Zephyr `spi/batch` APIs (`gallo` CLI, Rust, C,
  Python, MCP) are unchanged and remain fully supported.

  Zephyr also collapses `spi-cs-setup-delay-ns` and `spi-cs-hold-delay-ns` into
  a single `DIV_ROUND_UP(MAX(setup_ns, hold_ns), 1000)` microsecond value
  applied at *both* edges, instead of the batch path's separately honoured
  setup and hold delays. Read-only and write-only transfers are now full-duplex
  transfers of `max(tx_len, rx_len)` bytes with zero-filled TX or discarded RX.

- `SPI_HOLD_ON_CS` is now supported, but **requires `SPI_LOCK_ON`** and returns
  `-ENOTSUP` without it: holding chip select while another configuration could
  select a second slave would leave two peripherals selected at once. A
  successful hold commits received data and retains both the asserted line and
  the bus lock until `spi_release()` is called with that same configuration.
  `SPI_LOCK_ON` is likewise now supported; both were previously rejected.

- Added a **chip-select fault latch**. If a forced deassert returns an error
  the driver cannot tell whether the line went inactive, so the controller
  latches and every later transceive returns `-EHOSTDOWN` before issuing any
  configuration, chip-select edge or clocking. Only a `spi_release()` whose
  checked deassert succeeds clears it; a failed release still releases software
  ownership so nothing wedges, but retains the latch and the exact
  configuration pointer so recovery can be retried. Received data is committed
  only after an acknowledged deassert or a successful deliberate hold — a
  transfer that succeeds but whose deassert fails returns the deassert errno
  and does not commit RX.

- The SPI controller now initializes at
  `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY` (new, default 50) instead of
  `CONFIG_SPI_INIT_PRIORITY`, and **configures every declared chip-select pin
  as an explicit inactive output at init** — two USB round trips per pin, in
  ascending array order. That couples SPI initialization to the GPIO child:
  the SPI priority must be greater than
  `CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY` (45), which must be greater than
  `CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY` (40). Kconfig cannot check the
  arithmetic, so runtime readiness is authoritative: an inversion returns
  `-ENODEV` before configuring any pin. There is no rollback on failure —
  earlier entries are acknowledged inactive, the failing entry is
  indeterminate, and later entries were never issued. `-EBUSY` means a firmware
  GPIO event subscription owns the pin; reset it explicitly with
  `gallo_system_reset_subscriptions()` after a strict open, or power-cycle.

- A declared chip-select pin must be owned **exclusively** by SPI. The GPIO
  child being the sole *driver path* for the pin's mode is not an ownership
  reservation; a direct GPIO consumer can reconfigure or drive it between SPI
  operations and nothing detects it. This is an application obligation.

### Added

- CI gate (`.github/workflows/zephyr.yml`) building the module against a pinned
  Zephyr revision on `native_sim/native/64`, driven by
  `zephyr/scripts/ci-build.sh`. Covers the two viable samples, baseline-failure
  assertions for the two IS31 samples, and the four M5 test applications.
  Build-only: no produced binary is executed, so this adds no runtime coverage.
  ([#130](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/130))

- Added the `odp,pico-de-gallo-gpio` devicetree compatible and its Zephyr GPIO
  controller driver. The controller is a direct child of an enabled
  `odp,pico-de-gallo` parent, borrows the parent's USB connection and never
  releases it, and initializes at `POST_KERNEL/45` — after the parent
  (`KERNEL_INIT_PRIORITY_DEFAULT`, currently 40) and
  before the I2C and SPI controllers (50). The shield ships one **disabled**
  `pdg_gpio0` node with `ngpios = <4>`.

  - The parent of every enabled GPIO child **must** define `serial-number`; a
    missing selector is rejected at build time, because GPIO actuates physical
    pins and a selector-less connection cannot report which attached board it
    chose. Presence is not uniqueness — two parents naming the same serial
    still alias. The configured serial is logged on successful initialization.
  - `ngpios` is bounded to 1..32 at build time and must equal the
    firmware-reported `device/info.num_gpios` at initialization; a mismatch is
    a local devicetree/firmware configuration error and fails with `-EINVAL`.
  - Six API slots are implemented: `pin_configure`, `port_get_raw`,
    `port_set_masked_raw`, `port_set_bits_raw`, `port_clear_bits_raw` and
    `port_toggle_bits`.
  - Every operation that reaches hardware is a blocking USB round trip. Calls from interrupt context
    return `-EWOULDBLOCK`; transport failure is reported as `-EIO`.
  - Flag mapping is a positive allow-list, so no flag is silently ignored.
    `GPIO_DISCONNECTED`, `GPIO_INPUT | GPIO_OUTPUT`, single-ended /
    open-source / open-drain, interrupt-mode flags including `GPIO_INT_WAKEUP`,
    and any unknown bit return `-ENOTSUP`; both pulls, both output init levels,
    and an init level without `GPIO_OUTPUT` return `-EINVAL`. `GPIO_ACTIVE_LOW`
    is supported through Zephyr's common GPIO layer.
  - Multi-pin writes and output initialization are explicitly **non-atomic**.
    On a partial failure the acknowledged prefix definitely changed, the failed
    pin is indeterminate because its request may have executed with only the
    response lost, and later selected pins were never issued. The driver logs
    the operation, the failed pin, the requested mask and value, and the
    acknowledged prefix, and never rolls back.
  - Reads are scoped to input pins: a pin the firmware records as an explicit
    output contributes a zero bit and the scan continues, matching Zephyr's
    reference `gpio_emul` controller. This is coupled to the rejection of
    `GPIO_INPUT | GPIO_OUTPUT` and the two must not be changed independently.
  - `port_toggle_bits` dispatches but returns `-ENOTSUP`: an explicit output
    cannot be read back and no pin state is cached. Generic toggle consumers,
    including blinky, the GPIO shell, the TPS382x watchdog and the LS0xx
    display, do not work with this controller.
  - Interrupt configuration, callback management and the pending-interrupt
    query are deliberately not implemented and return `-ENOSYS`.

- Added the `odp,pico-de-gallo` devicetree compatible: a multi-function
  device parent node representing one physical USB-attached board. It owns
  the host connection for that board and exposes its opaque context to child
  peripheral controller drivers. The shield ships one disabled `pdg0` node,
  with the I2C and SPI controllers as its direct children; they borrow the
  parent's connection and carry no selector of their own. At most
  one enabled parent may omit `serial-number`; multiple enabled parents
  without it are rejected at build time.

- Added validation against the firmware-reported GPIO count and explicit
  mappings for all #104 statuses: `-71` to `-EINVAL`, `-72` to `-EACCES`,
  `-73` to `-EBUSY`, `-74` to `-ENODEV`, and `-75` to `-ETIMEDOUT`. The
  mapping uses `switch ((enum Status)status)` with no `default:` label inside
  the switch and is enforced by `-Werror=switch`. At that point full
  `cs-gpios` support was rejected because it would split one atomic `spi/batch`
  into four USB round trips and stop holding chip-select atomically. Closes
  #104. **Superseded in the same Unreleased development cycle:** SP1 later added
  a real GPIO controller and replaced that temporary mapping with required,
  same-parent standard `cs-gpios`; the Zephyr path now uses checked GPIO edges
  around `spi/transfer`, while non-Zephyr `spi/batch` remains supported.
