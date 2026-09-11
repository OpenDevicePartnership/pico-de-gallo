# Zephyr UART driver, and a UART wire configuration that matches the hardware

- **Issue:** [#152](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/152)
- **Branch:** `issue-152`
- **Date:** 2026-09-10
- **Status:** implemented and hardware-verified with limits; see §8.3

---

## 1. Summary

The Zephyr module has `mfd`, `gpio`, `i2c` and `spi` drivers but no `uart`,
even though the FFI bottom layer has been complete for some time. This design
adds one.

Along the way it closes a gap the issue only half-identified: the UART wire
configuration carries **baud rate and nothing else**, so a Zephyr
`uart_configure()` asking for 7E1 has no way to be honoured. This design
extends the wire configuration to the full set of framing parameters the
RP2350 can actually produce, and fixes the firmware's read path so a Zephyr
polling loop is not structurally slow.

Three things this design deliberately does **not** do: bump any version, add a
`UartError` variant, or buffer transmit data inside the Zephyr driver. Each is
justified below.

---

## 2. Triage: what #152 got wrong

The issue was filed 2026-08-28. Two of its factual claims no longer hold, and
one of its instructions is right for a reason it did not give.

### 2.1 "hw-rev1 is still the default feature today" — false

`crates/pico-de-gallo-firmware/Cargo.toml` reads `default = ["hw-rev2"]`. The
companion issue it refers to has already landed. A default build has UART
enabled, and the board used for this work reports hw revision 2 with the UART
capability bit set.

The issue's premise for an init-time failure is therefore weaker than written.
The gate is still worth having — a v1.0 board can still be attached — but it
guards an unusual case, not the default one.

### 2.2 "the wire config carries baud rate only" — true, but `uart/set-config` is not a stub

The issue implies the endpoint may not do anything useful. It does.
`handlers/uart.rs:124` calls `context.uart.set_baudrate(req.baud_rate)`, which
reaches embassy's `set_baudrate_inner` and writes the RP2350 `UARTIBRD` /
`UARTFBRD` divisor registers.

This was verified on hardware rather than inferred, because loopback alone
cannot verify it: TX and RX are the same peripheral, so a divisor that never
moved would still round-trip perfectly. The discriminator was throughput. With
TX and RX shorted, 512 bytes were written and a fixed wall-clock window later
the RX ring was drained non-blockingly:

| baud | bytes caught of 512 | expected in the window |
|---|---|---|
| 9600 | 165 | ~192 |
| 115200 | 484 | capped |
| 1000000 | 512 | all |
| 9600 (repeat) | 192 | ~192 |

Same payload, same code path, only the baud differed; the repeat run dropped
back down, so it is not a warm-up artefact. The divisor demonstrably retunes.
The setting also survives host process restarts, because the state lives in
firmware.

**Consequence for the design:** baud rate needs no work. Data bits, parity and
stop bits are the actual gap.

### 2.3 "return -ENOTSUP for the async API" — right, but not for the stated reason

The issue justifies this on honesty grounds: a USB-bridged UART cannot back
interrupt semantics. True, but insufficient. The binding reason is mechanical.

`include/zephyr/drivers/uart/uart_internal.h` dispatches `tx`, `tx_abort`,
`rx_enable`, `rx_buf_rsp` and `rx_disable` (`:496`, `:526`, `:536`, `:564`,
`:588`) **with no NULL check**:

```c
static inline int z_impl_uart_tx(const struct device *dev, const uint8_t *buf,
                                 size_t len, int32_t timeout)
{
#ifdef CONFIG_UART_ASYNC_API
	return DEVICE_API_GET(uart, dev)->tx(dev, buf, len, timeout);
```

Leaving those slots NULL is a jump through a null function pointer, not an
`-ENOSYS`. `CONFIG_UART_ASYNC_API` is a global symbol: another driver in the
image can enable it and a caller can then reach ours. So the stubs are a
crash-avoidance requirement, and they must be present even though nothing in
our own configuration would call them.

`poll_out` has the same property (`:182`) and is therefore mandatory outright.

---

## 3. Hardware capability envelope

Everything below derives from what the RP2350's PL011 can produce. Confirmed
against `rp-pac-7.0.0/src/rp235x/uart/regs.rs:1078-1160`, which exposes
`set_wlen`, `set_pen`, `set_eps`, `set_stp2`, `set_sps` and `set_fen`.

| Zephyr `uart_config` value | RP2350 mechanism | Verdict |
|---|---|---|
| `UART_CFG_DATA_BITS_5/6/7/8` | `WLEN` 0b00–0b11 | supported |
| `UART_CFG_DATA_BITS_9` | none | reject `-ENOTSUP` |
| `UART_CFG_PARITY_NONE` | `PEN=0` | supported |
| `UART_CFG_PARITY_ODD` | `PEN=1, EPS=0, SPS=0` | supported |
| `UART_CFG_PARITY_EVEN` | `PEN=1, EPS=1, SPS=0` | supported |
| `UART_CFG_PARITY_MARK` | `PEN=1, EPS=0, SPS=1` | supported |
| `UART_CFG_PARITY_SPACE` | `PEN=1, EPS=1, SPS=1` | supported |
| `UART_CFG_STOP_BITS_1/2` | `STP2` | supported |
| `UART_CFG_STOP_BITS_0_5/1_5` | none | reject `-ENOTSUP` |
| `UART_CFG_FLOW_CTRL_NONE` | — | supported |
| `UART_CFG_FLOW_CTRL_RTS_CTS` | RTS/CTS pins not passed to `BufferedUart::new` (`main.rs:477-485`) | reject `-ENOTSUP` |
| `UART_CFG_FLOW_CTRL_DTR_DSR/RS485` | none | reject `-ENOTSUP` |

Mark and space parity are worth calling out: embassy's own `Parity` enum has
only `None`/`Odd`/`Even`, so they are unreachable through embassy's API. They
are reachable through the register, and the `sps` field documentation states
the mapping explicitly. Since this design writes the register directly it can
offer all five, and does.

---

## 4. Wire protocol

### 4.1 New enums

In `pico-de-gallo-internal`, sized to §3:

```rust
pub enum UartDataBits { Five, Six, Seven, Eight }
pub enum UartParity   { None, Odd, Even, Mark, Space }
pub enum UartStopBits { One, Two }
```

Each carries the standard `// WARNING: Do not reorder` comment, because
postcard serialises by variant index (AGENTS.md §6.1).

`UartParity`'s ordinals coincide with Zephyr's `UART_CFG_PARITY_*`.
`UartStopBits`'s deliberately do **not**: Zephyr numbers `0_5=0, 1=1, 1_5=2,
2=3`. Matching that would bake a foreign subsystem's ABI into our wire format
and leave two permanently unrepresentable holes at indices 0 and 2. The Zephyr
driver maps explicitly instead, which is a few lines in one place and keeps
the wire enum meaning what it says.

The coincidence in `UartParity` is therefore a coincidence, not a contract.
The Zephyr driver must not rely on it, and will map that enum explicitly too.

### 4.2 Extended structs

```rust
pub struct UartSetConfigurationRequest {
    pub baud_rate: u32,
    pub data_bits: UartDataBits,
    pub parity: UartParity,
    pub stop_bits: UartStopBits,
}

pub struct UartConfigurationInfo {
    pub baud_rate: u32,
    pub data_bits: UartDataBits,
    pub parity: UartParity,
    pub stop_bits: UartStopBits,
}
```

This also makes the doc comment at `internal:1156-1159` true for the first
time. It currently promises that "data bits, parity, and stop bits … are
reserved for future use and must be set to their default values" and names
fields that **do not exist in the struct**. That comment is deleted and
replaced with an accurate one.

### 4.3 No new `UartError` variant

The enums encode only representable values, so anything that decodes is
something the hardware can do. Unsupported combinations — 9 data bits, half
stop bits, hardware flow control — are rejected at the Zephyr and FFI
boundaries before a request is ever built, and never reach the wire.

This is a deliberate choice to leave `UartError`'s ABI untouched. It follows
the reasoning AGENTS.md §8 applies to `Status`, as exercised in the 2026-08-17
row: a new enumerator is a permanent ABI commitment, so do not spend one where
a boundary check suffices.

### 4.4 No version bump

`main` already carries an unreleased wire break:

| Crate | In tree | Last released tag |
|---|---|---|
| `internal` | 0.8.0 | `internal-v0.7.0` |
| `firmware` | 0.12.0 | `firmware-v0.11.0` |
| `lib` | 0.9.0 | `library-v0.8.0` |
| `ffi` | 0.9.0 | `ffi-v0.8.0` |

The attached board reports schema 0.8.0 while the last release was schema 0.7.
This change therefore **rides the pending unreleased bump** and moves no
`[package].version`, exactly as #135 did with the schema-0.7 bump. Versions
are bumped once, deliberately, at release time (AGENTS.md §4 rule 12).

### 4.5 Skew detection

`validate()` compares schema versions only, so it cannot distinguish firmware
built before this change from firmware built after — both report 0.8.0. That
is the 2026-09-01 hazard.

It is contained here by the endpoint key. `postcard-rpc-0.12.1/src/macros.rs:44`
derives the request key from the **request** type:

```rust
const REQ_KEY: $crate::Key = $crate::Key::for_path::<$req>($path);
const RESP_KEY: $crate::Key = $crate::Key::for_path::<$resp>($path);
```

Adding fields to `UartSetConfigurationRequest` changes its schema and therefore
re-keys `uart/set-config` on the dispatch path. A mismatched firmware does not
recognise the key and fails loudly, rather than decoding three extra varints
as garbage framing.

**This is a code reading, not a measurement.** The 2026-09-01 row records that
the response-side re-keying was confirmed by mutation and that an assumption
about `DeviceInfo` was wrong twice on that branch. The implementation must
therefore verify request-side re-keying empirically — build one image with the
old request shape and one with the new, and confirm the old firmware refuses
the new request rather than accepting it. If it turns out `REQ_KEY` does not
gate dispatch, this section is wrong and the change needs a different
mitigation.

#### Correction, from M1: the two endpoints are not symmetric

The paragraph above is right for `uart/set-config` and **wrong for
`uart/get-config`**. M1 pinned both cases in
`crates/pico-de-gallo-internal/tests/req_key_changes_with_request_shape.rs`.

`uart/get-config`'s request type is `()`. It does not change, so its `REQ_KEY`
does not move and an old firmware **dispatches the request perfectly happily**.
What moves is `RESP_KEY`, because `UartConfigurationInfo` gained fields. The old
firmware replies under the old `RESP_KEY`, the new host is waiting on the new
one, the frame is dropped unmatched, and the caller sees an opaque **timeout**
— indistinguishable from a dead board.

So the honest statement is:

| Endpoint | Request shape moved? | Skew presents as |
|---|---|---|
| `uart/set-config` | yes → `REQ_KEY` moves | firmware never dispatches; loud and specific |
| `uart/get-config` | no (`()`) → `REQ_KEY` fixed | reply dropped unmatched; **opaque timeout** |

This is the same blind spot as the `DeviceInfo` hazard in the 2026-09-01 row,
scoped smaller: the endpoint that would report the incompatibility is itself
the one that cannot. It is acceptable — a timeout is not silent corruption, and
`set-config` still fails loudly — but it must not be described as "fails
loudly" without qualification.

The *dangerous* direction is also worth naming precisely, because it is the
reverse of the intuitive one. A **new host talking to old firmware** is the
hazard: postcard ignores trailing bytes, so an old one-field decoder reads
`baud_rate` correctly, never looks at the three framing bytes, and returns
success. A caller asking for 7M2 would be left transmitting 8N1 with no error
anywhere. The reverse — an old host's 3-byte request reaching new firmware — is
three bytes short and fails the decode loudly. The endpoint key is what closes
the dangerous direction, which is why M1 gates on it.

**Still owed:** M1's evidence is static (two shapes compiled into one test
binary). The two-image firmware A/B this section originally asked for has not
been performed and belongs to a hardware milestone.

---

## 5. Firmware

### 5.1 Applying the framing parameters

`uart_set_config_handler` gains register writes for `WLEN`, `PEN`, `EPS`,
`SPS` and `STP2`.

There is no public embassy API for this. Confirmed against both the source and
the published documentation: `BufferedUart`'s complete public surface is `new`,
`new_with_rtscts`, `blocking_write`, `blocking_flush`, `blocking_read`, `busy`,
`send_break`, `set_baudrate`, `split` and `split_ref`. `Config` is consumed
only by the constructors, and `Uart` is no better — `set_baudrate` is the only
runtime reconfiguration entry point on any type in the module.

Three options were considered:

| Option | Verdict |
|---|---|
| **A.** Replicate embassy's `lcr_modify` in firmware via `embassy_rp::pac` | **chosen** |
| **B.** Upstream a `set_config` to embassy-rp | correct long-term, blocks this branch on an external release |
| **C.** Tear down and rebuild the `BufferedUart` | it owns `Peri` handles and `StaticCell` buffers; needs `Option<>` plus unsafe re-stealing, for the same result |

Option A is viable because `embassy_rp::pac` is public (`lib.rs:72`,
`pub use rp_pac as pac`) and so is `clocks::clk_peri_freq()` (`clocks.rs:1386`).

The sequence must replicate `embassy-rp-0.10.0/src/uart/mod.rs:998-1046`:

1. Read `UARTCR`.
2. If `UARTEN` is set, clear `UARTEN`/`TXE`/`RXE`, then delay 15 baud periods
   computed from `UARTIBRD`/`UARTFBRD` and `clk_peri_freq()`.
3. `UARTLCR_H.modify(...)` with the new framing bits.
4. Write the saved `UARTCR` back.

The delay is not optional and the reason is quoted in embassy's own comment:
the PL011 `BUSY` flag is OR'd with TX-FIFO-not-full, so with FIFOs enabled
there is no way to poll for end-of-character, and FIFOs cannot be disabled
mid-character without losing integrity. 15 baud periods comfortably exceeds
start + data + parity + stop.

This is the first `embassy_rp::pac` use in this firmware. It writes registers
behind embassy's back, which is a real hazard worth naming — but embassy's own
`set_baudrate` performs the identical disable/delay/restore dance on the same
registers and is already shipped, so the risk profile is that of working code.
The implementation must carry a comment citing `mod.rs:998-1046` so a future
embassy bump has an obvious review anchor.

Ordering note: `set_baudrate` itself ends with `lcr_modify(info, |_| {})` to
latch the divisor. Applying baud first and framing second is therefore safe;
applying framing first and baud second would also work, but leaves a window in
which the two disagree. The implementation should do baud, then framing, in
one handler invocation.

### 5.2 The read fast path

`uart_read_handler` currently wraps a `timeout_ms == 0` poll in
`with_timeout(Duration::from_millis(1), ...)` (`uart.rs:41`), so an empty poll
always burns a full millisecond. Measured in-process against the attached
board, so CLI process startup does not distort it:

| Operation | Measured | Meaning |
|---|---|---|
| `uart_write(1 byte)` | 335 µs | one `uart_poll_out` |
| `uart_read(1, empty ring)` | 1422 µs | one naive `uart_poll_in` |
| `uart_read(1014, 512 pending)` | 1257 µs | one bulk refill |
| amortised over a 1014-byte refill | 2.5 µs/byte | ~500× cheaper than naive |

`BufferedUart` implements `embedded_io::ReadReady`. The handler will call
`read_ready()` first and return `Ok(&[])` immediately when false, so an idle
poll costs a USB round trip rather than a round trip plus a millisecond.

This is scoped to `timeout_ms == 0`. The non-zero path keeps its existing
`progress::bounded` behaviour.

#### Correction, from M2: `read_ready()` is unsafe here

The paragraph above is wrong and the implementation does **not** do that.
Verified against `embassy-rp-0.10.0/src/uart/buffered.rs`:

- `read_ready` is `Ok(!state.rx_buf.is_empty())` and nothing more (`:337`).
- On a framing, parity, break or overrun error the ISR latches `rx_error` **and
  disables the RX interrupts**, `uartimsc().write_clear()` on `rxim`/`rtim`
  (`:588-593`).
- `try_read` is the only consumer of `rx_error` (`:275`) and the only thing
  that re-enables those interrupts (`:283-288`).

So a client that polls only with `timeout_ms == 0` would, after a single RX
error, find the ring empty forever, never reach `try_read`, and have RX dead
until reboot. That is precisely the Zephyr `uart_poll_in` consumer this section
exists to serve, and this branch *raises* the trigger probability by letting
callers select framing and get it wrong.

The shipped implementation polls the read future exactly once
(`embassy_futures::poll_once`). Static inspection of embassy's source shows
that this reaches `try_read`, which on an empty ring with an error set returns
`Poll::Ready(Err(e))` rather than `Pending` (`:274-281`), consumes `rx_error`,
and re-enables the interrupts on the way out. The performance win is measured:
the idle-read median fell from 1422 µs to 324.5 µs, 4.38× faster. The RX-error
recovery path remains hardware-untested because the campaign could not
generate a controlled error. No new dependency was needed — `embassy-futures`
was already direct.

Note also that `embassy-rp` implements `embedded_io_async::ReadReady` (`:650`),
not `embedded_io::ReadReady`, so the trait named above was the wrong generation
regardless.

#### Correction, from M2: the reconfiguration is not atomic

§5.1 said "baud, then framing, in one handler invocation" so the two never
disagree. That is false. `set_baudrate` restores an **enabled** `UARTCR` before
returning (`mod.rs:1070-1075`), so there is a brief window carrying the new
divisor with the old framing. Callers should quiesce both directions across a
reconfiguration. The handler and `Context` field docs now say so; neither claim
of atomicity survives.

The window is bounded: `set_baudrate_inner` clamps `IBRD` to 65535
(`mod.rs:1062-1068`), so the lowest achievable baud is about 143. The original
model predicted roughly 105 ms per register-update call, about 210 ms for baud
plus framing, but `set-config --baud-rate 1` took 0.565 s in host-observed RPC
latency. Those quantities are not directly comparable to the supervisor's
device-side dispatch-progress measurement, and the unexplained difference
shows that the 210 ms cost model is incomplete. The campaign observed no
supervisor reset at the lowest accepted baud: `ping` and `version` answered
immediately afterwards and the board never reset.

### 5.3 Stored configuration

`Context::uart_baud_rate: u32` (`context.rs:88`) becomes a struct holding all
four parameters, initialised to 115200 8N1 to match
`uart::Config::default()` as applied at `main.rs:484`. `uart_get_config_handler`
returns all four.

This remains a **software shadow**, not a register read-back, exactly as today.
The shadow update is adjacent to the register write, but that does not prove
that the hardware adopted the requested configuration. It also does not
reflect divisor rounding. That limitation is pre-existing and is not addressed
here.

The hw-rev1 arms of all five handlers keep returning `UartError::Unsupported`.

---

## 6. Zephyr driver

### 6.1 Placement and structure

- `zephyr/drivers/serial/pdg_uart.c` — top half, Zephyr-facing
- `zephyr/drivers/serial/pdg_uart_bottom.{c,h}` — bottom half, FFI-facing
- `zephyr/dts/bindings/serial/odp,pico-de-gallo-uart.yaml`

The top/bottom split is not stylistic. `native_sim` compiles the embedded side
against Zephyr's minimal libc and the runner side against the host libc, and
`pico_de_gallo.h` only exists in the host context. `pdg_uart_bottom.h` is the
only header both sides see and must use plain C types only.

Every bottom function is `__attribute__((weak))` — not Zephyr's `__weak`, which
would require `zephyr/toolchain.h` in a host-context file — so the recording
fake can override it.

Wiring required beyond the driver files:

- `zephyr/Kconfig:7` gains `|| DT_HAS_ODP_PICO_DE_GALLO_UART_ENABLED`, or a
  UART-only tree leaves `CONFIG_PICO_DE_GALLO` at `n` and the FFI import is
  skipped entirely.
- `zephyr/drivers/Kconfig` gains `rsource "serial/Kconfig"`.
- `zephyr/drivers/CMakeLists.txt` gains
  `add_subdirectory_ifdef(CONFIG_UART_PICO_DE_GALLO serial)` **and**
  `OR CONFIG_UART_PICO_DE_GALLO` in the `if()` that compiles `common.c`.
  Without the second, a UART-only tree links against a `common.c` that was
  never compiled — the file's own comment says exactly this.

No change to `common.c` is needed: every UART `Status` is already mapped
(`UartInvalidBaudRate` → `-EINVAL`, the rest → `-EIO`).

### 6.2 API struct

Shaped by NULL-dispatch hazards, not preference.

| Slot | Decision | Reason |
|---|---|---|
| `poll_out` | implement | `uart_internal.h:182` dispatches with no NULL check; NULL is a hard fault |
| `poll_in` | implement | must return exactly `-1` when empty, and is contractually non-blocking |
| `err_check` | implement, return 0 | see §6.5 |
| `configure`, `config_get` | implement, inside `#ifdef CONFIG_UART_USE_RUNTIME_CONFIGURE` | the slots do not exist when that symbol is `n`, so an unguarded initialiser fails to compile |
| `tx`, `tx_abort`, `rx_enable`, `rx_buf_rsp`, `rx_disable`, `callback_set` | `-ENOTSUP` stubs, inside `#ifdef CONFIG_UART_ASYNC_API` | dispatched with no NULL check; see §2.3 |
| `poll_out_u16` | stub, inside `#ifdef CONFIG_UART_WIDE_DATA` | dispatched with no NULL check |
| all `fifo_*`, all `irq_*`, `line_ctrl_*`, `drv_cmd`, `poll_in_u16` | leave NULL | all NULL-checked, degrade to `-ENOSYS` |

`config_get` is not optional in practice. `drivers/serial/uart_shell.c:199`
does read-modify-write, so `configure` without `config_get` cannot be driven
from the shell, and several in-tree drivers treat a `config_get` failure as
fatal.

### 6.3 Receive path: buffered

A driver-side ring of `GALLO_MAX_RESPONSE_PAYLOAD` (1014) bytes, refilled by a
single `gallo_uart_read(1014, timeout=0)` when empty, served byte-at-a-time to
`uart_poll_in`.

Two independent reasons, and the contract one is decisive on its own:

1. **Contract.** `uart_poll_in` is documented non-blocking, and
   `drivers/console/uart_console.c:556` drains it in a tight
   `while (uart_poll_in(...) == 0)` loop. A USB round trip per byte inside a
   call documented as non-blocking is a contract violation.
2. **Cost.** 2.5 µs/byte amortised versus 1422 µs naive (§5.2).

`uart_poll_in` returns exactly `-1` when the ring is empty and a refill yields
nothing. Not `-EAGAIN`, not `-ENODATA`: the documented value is `-1`.

An empty poll still costs one RPC, because there is no way to learn that no
data arrived without asking. §5.2's fast path is what makes that affordable.

Bytes drawn into the driver ring have been consumed from the firmware ring. A
single consumer makes that safe; the driver is the only reader.

### 6.4 Transmit path: not buffered

One `gallo_uart_write()` per `uart_poll_out()`. 335 µs per byte, about 3 kB/s.

The firmware already buffers transmit data: `main.rs:473-485` gives
`BufferedUart` a 1024-byte TX ring drained by the ISR, and `uart/write` calls
`write_all`, which returns once bytes are queued in that ring rather than once
they have left the wire. The separate `uart/flush` endpoint, backed by
`embedded_io_async::Write::flush`, is what drains it.

So an unbuffered driver hands each byte to a real transmitter — which is
precisely the weaker of the two readings of `uart_poll_out`'s
self-contradictory contract, and the one every in-tree driver implements.

A driver-side buffer would be a *second* buffer, and the only layer in the
stack with no drain of its own. The polling API has no flush hook, so it would
need a trigger: a newline trigger strands unterminated shell prompts, and a
timer needs a work queue, which is unsafe because `console_out()` can run
pre-kernel and in ISR context (`uart_console.c:615` under
`CONFIG_EARLY_CONSOLE`). Note also that `uart_poll_out` returns `void`, so a
deferred write's failure would be unreportable.

Throughput is therefore traded for never stranding a byte. This is recorded as
a decision, not an oversight; if line-oriented throughput later matters, the
place to revisit is a Kconfig opt-in with the contract cost documented.

### 6.5 Error reporting

`err_check` returns 0.

The firmware collapses every embassy UART error to `UartError::Other`
(`uart.rs:43`, `:49`), so `UartError::Overrun`, `Break`, `Parity` and `Framing`
exist on the wire but are **never produced**. Reporting `UART_ERROR_OVERRUN`
would be an invention. Returning 0 is the truthful answer, and the limitation
is documented rather than papered over.

This means a receive overrun — the firmware's 1024-byte RX ring filling — is
currently silent at every layer. That is a pre-existing firmware limitation,
outside this issue's scope, and should be filed separately.

### 6.6 Configuration mapping

`uart_configure()` validates against §3 and rejects everything outside it with
`-ENOTSUP`, then maps to `pdg_uart_bottom_set_config()`. `uart_config_get()`
returns the cached configuration.

Zephyr's `struct uart_config` fields are `uint8_t`, not the enum types, so
every field is validated rather than trusted. Rejections are individually
logged and, following the module's convention, each message ends with
`Returning -EXXX.`

Following `pdg_gpio.c:100-116`, validation is a positive allow-list with a
catch-all last, so specific diagnostics win and a newly defined Zephyr
enumerator cannot quietly acquire a meaning.

### 6.7 Devicetree

`include: [uart-controller.yaml, base.yaml]`, which supplies `current-speed`,
`parity`, `stop-bits`, `data-bits` and `hw-flow-control`, and declares
`bus: uart`.

`uart-controller.yaml`'s enum *indices* align with the C enum ordinals, so
`DT_INST_ENUM_IDX_OR(inst, data_bits, UART_CFG_DATA_BITS_8)` resolves directly.
Note that only `parity` has a binding-level default; `current-speed`,
`stop-bits` and `data-bits` have none, so all three need `_OR` forms.

Build assertions follow the established order from `pdg_spi.c:84-156` —
compatible, parent status, parent serial, Kconfig — placed **before**
`#include "pdg_mfd.h"`, because with `CONFIG_MFD_PICO_DE_GALLO=n` that header
is not on the include path.

Whether to require `serial-number` on the parent is a genuine judgement call.
SPI and GPIO require it because they actuate pins; I2C does not. UART drives
two physical pads, which puts it on the SPI/GPIO side. **This design requires
it**, on the grounds that transmitting onto the wrong board's TX pin is the
same class of mistake as driving the wrong board's chip select.

### 6.8 Capability gate at init

A new `pdg_uart_bottom_has_uart()` reads `GALLO_CAP_UART` (bit 2) from
`gallo_get_device_info` and the init function fails with `-ENODEV` if it is
clear.

Modelled on GPIO's `ngpios` cross-check (`pdg_gpio.c:604-629`), including its
observation that the parent's strict open has already populated the
handle-shared device-info cache, so this is a warm local read with no USB round
trip and no `device/info` timeout exposure on the child path.

This matters more than usual because `uart_poll_out` returns `void`. Without an
init gate, a rev1 board's first transmit fails with no channel to report it.

Init order otherwise follows I2C exactly: `k_mutex_init()` first and before any
early return, then MFD readiness, then borrow the context, then configure.
Every failure path after the borrow sets `data->ctx = NULL` as defensive
invalidation — never a reference release.

### 6.9 Locking

A plain `struct k_mutex` in driver data, as I2C and GPIO use. Not SPI's
`spi_context` semaphore, which exists to serve `spi_transceive`'s locking
semantics and has no UART analogue.

Every public callback guards `data->ctx == NULL` and returns `-ENODEV`, because
Zephyr's UART API dispatches straight into the driver with no readiness check —
the same reasoning recorded at `pdg_i2c.c:476-492`.

---

## 7. Host surfaces

| Crate | Change |
|---|---|
| `pico-de-gallo-lib` | `uart_set_config` takes the three new parameters; `uart_get_config` returns them |
| `pico-de-gallo-ffi` | `gallo_uart_set_config` takes three `uint8_t`s with range validation; `gallo_uart_get_config` gains out-parameters. New `GalloUartDataBits`/`GalloUartParity`/`GalloUartStopBits` enums |
| `pico-de-gallo-app` | `gallo uart set-config` gains `--data-bits`/`--parity`/`--stop-bits`; `get-config` prints all four |
| `gallo-mcp` | tool arguments and response mirror the above |
| `pyco-de-gallo` | enums exposed as `#[pyclass]`, deriving `Clone` |
| `pico-de-gallo-hal` | **no functional change** — see below |

**Python naming exception, from M3.** `UartParity`'s `None` variant is exposed
to Python as **`NoParity`**. `None` is a Python keyword, so a member literally
named `None` makes `UartParity.None` a `SyntaxError` at the call site —
verified, not assumed. The variant is **renamed, never renumbered**: it still
maps to and from wire index 0. This is a Python-surface spelling only; the wire
enum, the FFI enum and the C header all keep `None`.

**CLI flag exception, from M3.** The framing flags are **long-only**. Deriving
short flags would give `-s` to `--stop-bits`, colliding in meaning with the
top-level `-s`/`--serial-number`. Because that top-level flag is not `global`,
clap does *not* panic on the collision — it silently means different things at
different levels, which is worse than a crash. Pinned by a test.

`pico-de-gallo-hal` needs no code change. Its `Uart` (`lib.rs:1896`) exposes no
configuration surface at all, only `set_timeout_ms`, and its doc comment
already directs callers to depend on `pico-de-gallo-lib` and call
`PicoDeGallo::uart_set_config` instead. The only edit is to that comment
(`lib.rs:1882-1884`), which says "**Baud rate is fixed** at the firmware
default"; it becomes baud rate *and framing*. Widening the HAL's surface is a
separate decision and is not taken here.

Two traps that have bitten this repo before apply directly:

- The new `Gallo*` enums must be listed in `cbindgen.toml`'s `[export] include`.
  cbindgen prunes any type no exported signature references, and the signatures
  take `uint8_t`. Without the listing they silently vanish from the header
  (AGENTS.md §8).
- The enum values are stable C ABI and must match the wire enum discriminants,
  pinned by a test in the style of `config_enums_match_wire_enums`.

The FFI keeps `uint8_t` parameters rather than the enum types, matching how
`I2cFrequency`, `GpioDirection` and `GpioPull` are already handled.

---

## 8. Testing

### 8.1 Host

Round-trip serialisation tests for the three new enums and both extended
structs, per AGENTS.md §14. FFI range-validation tests for each new `uint8_t`
parameter. CLI parsing tests for the new flags.

### 8.2 Zephyr, hardware-free

A new `zephyr/tests/pdg_fake/uart/` suite, following `pdg_fake/i2c/`.

The critical constraint is that weak-symbol overriding is **exhaustive per
topology**. The fake must override `pdg_common_bottom_open`/`_close` *plus
every* `pdg_uart_bottom_*` function the init path can reach — at minimum
`has_uart` and `set_config` — or the fake's opaque token reaches the real FFI
as a `PicoDeGallo *` and is dereferenced.

Cross-directory headers on the `native_simulator` side must use
`target_compile_options(native_simulator INTERFACE "-I...")`, **not**
`target_include_directories`. `natsim_config.cmake` joins only
`INTERFACE_COMPILE_OPTIONS` into `NSI_BUILD_OPTIONS`; the include-directories
form silently vanishes and has already cost one red CI run.

Coverage: configuration validation and rejection of every unsupported value
from §3, `poll_in` returning `-1` on an empty ring, and the buffered refill
issuing one bulk read rather than one read per byte.

### 8.3 Hardware

The campaign used board `5256657D8A5D7F03`, hw-rev2, with TX (GPIO 0) and RX
(GPIO 1) physically shorted and no series resistors. The firmware reported
`build_id = firmware-v0.11.0-109-g6c4d42fd0796`, firmware 0.12.0 and schema
0.8.0 at both the start and end of the session. Nine experiments ran serially
in a fixed order. No STOP condition fired, and the board needed no USB
re-enumeration or reflash.

#### The predicted discriminator was invalid

The original procedure proposed transmitting under 8N1, reconfiguring to 7E1,
and then reading the queued bytes under the new setting. That cannot test the
register write: bytes in the RX ring were already decoded when captured, and a
later `UARTLCR_H` change cannot reinterpret them. Two reviewers independently
identified this defect during execution.

Two falsifiable discriminators replaced it:

1. **Stable matched-framing A/B on word length.** One arm transmitted and
   received under stable 7N1; the other did so under stable 8N1. The transmitter
   genuinely shifts only seven data bits in the first arm. A stale `UARTLCR_H`
   would therefore return the same bytes in both arms. `0x00` and `0x55` are
   fixed points under seven-bit masking and prove nothing; `0xFF` and `0xAA`
   are the discriminating probes.
2. **Frame-length timing.** At a fixed baud, 7N1 occupies 9 bit periods per
   character, 8N1 occupies 10, 8E1 and 8N2 occupy 11, and 8E2 occupies 12.
   The harness timed 512-byte in-process transfers at 4800 baud, where the
   expected transfer time differs by 106.7 ms per framing bit. It collected
   seven samples per framing and repeated 8N1 non-adjacently as an internal
   repeatability control.

#### Results

| Claim | Verdict | Evidence |
|---|---|---|
| Boot **word length** is 8 | **VERIFIED** | Before any `set-config`, `get-config` returned `115200 bps 8N1`; writing `ff aa` returned **`ff aa`**. A seven-bit boot would have returned `7f 2a`. |
| Boot **baud** and boot **parity** | **NOT VERIFIED — unverifiable on this hardware** | `get-config` reads a software shadow, not the registers, and there was no independent UART peer. This became permanently unrepeatable once later experiments issued `set-config`. |
| `WLEN` applied | **VERIFIED** | Stable 7N1 returned **`7f 00 55 2a`**; stable 8N1 returned **`ff 00 55 aa`**. Timing independently separated 9 from 10 bits per character. |
| `PEN` applied | **VERIFIED** | Timing separated 8N1 at 10 bits per character, 8E1 at 11 and 8E2 at 12. |
| `STP2` applied | **VERIFIED** | Timing separated 8N1 at 10 bits per character from 8N2 at 11; 8E2 at 12 showed that `PEN` and `STP2` were independently effective. |
| `EPS` (odd versus even) | **NOT VERIFIED — unobservable** | 8E1 and 8O1 are both 11 bits, so timing cannot separate them; matched loopback round-trips both identically. |
| `SPS` (mark versus space) | **EXERCISED but NOT PROVEN** | `--parity mark` and `--parity space` both returned rc=0, the shadow echoed `9600 8M1` and `9600 8S1`, and `0x5a` round-tripped. Both are 11-bit frames, however, and matched loopback checks only what the same transmitter generated. This is the only path that would evidence the `SPS` bit being written, and this hardware cannot observe it; proof requires an independent UART peer. |
| `apply_framing` restores `UARTCR`, no wedge | **VERIFIED** | `set-config --baud-rate 1`, the lowest accepted value, returned rc=0 in **0.565 s**. Embassy clamps IBRD to 65535, so the achieved rate was about 143 baud. `ping` and `version` still answered and `0xc3` round-tripped. `get-config` reporting `1 bps` was the shadow echoing the request, not the achieved rate. |
| Read fast path | **VERIFIED** | In-process `uart_read(1, empty ring)` had a **324.5 µs** median and 437.4 µs p95, against a **1422 µs** pre-change baseline on this board: **4.38× faster**. `uart_write(1 byte)` had a **329.4 µs** median and 447.9 µs p95, against 335 µs before the change. The nearly identical medians have the expected shape when both operations are one USB round trip with no added wait. |
| Rejection paths | **VERIFIED** | `--baud-rate 0` exited 1 with `Endpoint(InvalidBaudRate)`. `--data-bits 9` was rejected by clap before a device round trip: exit **2** in **0.378 s**, with `error: invalid value '9' for '--data-bits <DATA_BITS>'`. The software shadow was byte-identical before and after both attempts. |
| RX error recovery | **INCONCLUSIVE** | A single shorted PL011 cannot generate a controlled RX error. Reframing an in-flight character was the only available mechanism; three bounded attempts, including 8N1 to 8E2 with about 6.7 s of queued wire time, latched zero errors. The post-recovery nonce `0xd1` round-tripped, so RX was not left dead and there is no evidence of a defect, but the error path remains untested. |

The full frame-length timing result was:

| Framing | Median ms | Derived bits/char | Predicted |
|---|---:|---:|---:|
| 8N1 | 1074.4 | 10.073 | 10 |
| 7N1 | 967.8 | 9.073 | 9 |
| 8E1 | 1181.2 | 11.074 | 11 |
| 8N2 | 1181.1 | 11.073 | 11 |
| 8E2 | 1287.9 | 12.074 | 12 |
| 8N1 (repeat control) | 1074.6 | 10.074 | 10 |

The harness reported `SPREAD_MS = 0.2` against a 10.0 ms limit and
`ALL_BITS_MATCH = 1`. Every framing carried a uniform offset of about +0.073
bits per character. The offset was constant rather than proportional: about
7.8 ms of fixed transfer overhead, not a per-bit timing error. It therefore
cancelled in every framing comparison and did not weaken the discrimination.

#### Limits and work still owed

- The §4.5 two-image `REQ_KEY` A/B was **not performed**. It needs old-request-
  shape and new-request-shape firmware images and therefore a physical BOOTSEL
  press, which was unavailable. The static M1 evidence remains the only evidence
  for that claim.
- `EPS` and `SPS` proof still requires a second, independent UART peer. Matched
  one-peripheral loopback cannot distinguish odd from even or mark from space.
- RX error recovery remains untested because the setup could not generate a
  controlled error. The successful nonce only showed that the attempted
  triggers did not leave RX dead.

Two secondary observations came from the campaign. First,
`set-config --baud-rate 1` took 0.565 s where §5.2's model predicts about
210 ms, roughly 2.7 times longer. It passed comfortably, but the stated cost
model appears incomplete. Second, the `gallo` CLI's `--timeout` help still says
`0 = 1 ms non-blocking poll`; that is stale because the firmware now polls
exactly once with `poll_once`. This milestone did not fix that source-level
help-text drift.

CI still runs `zephyr.yml` build-only, so a green run proves the module compiles
and links, not that it works. The verdicts above come only from this manual
board-attached campaign and retain the stated limits.

---

## 9. Documentation

Per AGENTS.md §15.1, `zephyr/` has no book chapter by design, so
`zephyr/README.md` plus `zephyr/CHANGELOG.md` satisfy the parity rule for the
driver itself.

But this change also touches things the book *does* describe, so those need
paired edits:

- `book/src/appendix/endpoints.md` — `uart/set-config` and `uart/get-config`
  descriptions
- `book/src/internals/wire-protocol.md` — the three new enums
- `book/src/interfaces/uart.md` — runtime framing configuration
- `book/src/crates/{app,ffi,lib,mcp,python}.md` — new parameters
- `crates/*/CHANGELOG.md` for every crate touched
- `zephyr/README.md` — a UART limitation table matching the I2C and SPI style,
  every row stating a result rather than a caveat

---

## 10. Out of scope

Recorded so the boundaries are deliberate rather than accidental.

- **Overrun reporting.** The firmware collapses all embassy errors to `Other`,
  so a filled RX ring is silent. Fixing it means propagating embassy's error
  variants, which is its own wire change. File separately.
- **Baud-rate upper bound.** Nothing validates that a requested baud is
  achievable; the divisor is clamped silently by
  `set_baudrate_inner`. `uart/get-config` returns the requested value, not the
  achieved one. Pre-existing.
- **Zero-length `uart/write` guard.** `i2c/write` has one (#101); `uart/write`
  does not. `write_async_internal`'s empty-payload hang was I2C-specific, so
  there is no evidence UART has the same defect — but there is no evidence it
  does not, either. Worth a separate look.
- **Interrupt-driven and async Zephyr APIs.** Stubbed to `-ENOTSUP`, per §2.3.
- **`send_break`.** `BufferedUart::send_break` exists and `UartError::Break` is
  on the wire, but there is no endpoint. Not needed for this driver.
- **Upstreaming `set_config` to embassy-rp.** The correct long-term fix for §5.1
  option A. Worth doing, on its own timescale.

---

## 11. Risks

| Risk | Mitigation |
|---|---|
| §4.5's request-key claim is a code reading, and a similar assumption was wrong twice on the #159 branch | Verify by mutation before relying on it; if false, redesign the skew mitigation |
| Direct `UARTLCR_H` writes race the `BufferedUart` ISR | Replicate embassy's disable/delay/restore exactly; embassy's own `set_baudrate` has identical exposure and ships |
| The loopback cannot distinguish every framing bit from stale framing | Use stable matched-framing word-length A/B plus frame-length timing; `EPS` and `SPS` require an independent UART peer (§8.3) |
| A `pdg_fake/uart` suite that misses a bottom function passes the fake token to the real FFI | Enumerate every bottom entry point on the init path; the fake overrides are exhaustive per topology, not per driver |
| `zephyr.yml` is build-only and path-filtered | Behavioural claims come from the board-attached procedure only; confirm the workflow actually ran |
