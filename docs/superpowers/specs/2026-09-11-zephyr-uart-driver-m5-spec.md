# Zephyr UART driver M5 - revised core-driver and build-gate specification

Date: 2026-09-11  
Branch baseline: `issue-152` at `d701ade13246`  
Milestone: M5 of 7 - Phase B driver core; build and link only  
Status: revised after correctness and reliability rejection

## 1. Context, decision summary, and scope

Phase A is complete: the wire protocol, firmware, library, FFI, CLI, MCP and Python surfaces already carry UART baud rate, data bits, parity and stop bits. M5 consumes that established FFI to add one Zephyr UART controller beneath the existing `odp,pico-de-gallo` MFD parent. It does not reopen Phase A.

The parent owns one strict, validated host connection (`zephyr/drivers/mfd/pdg_mfd.c:71-90`); child drivers check readiness and borrow it through `pdg_mfd_ctx()` (`zephyr/drivers/mfd/pdg_mfd.h:18-33`). UART follows this ownership model: no open/close pair and no child release.

The decisive receive-timeout answer is **A**. The non-zero firmware branch still polls `AsyncRead::read`: `uart_read_handler` wraps that future in `progress::bounded` (`crates/pico-de-gallo-firmware/src/handlers/uart.rs:64-69`); Embassy's async `Read` delegates to `BufferedUartRx::read` (`embassy-rp-0.10.0/src/uart/buffered.rs:637-646`); that future calls `try_read` on every poll (`:229-240`); and `try_read` consumes `rx_error` and re-enables RX interrupts (`:262-290`). M5 therefore uses **`timeout_ms = 1`**. This preserves the load-bearing error-recovery path while changing the host timeout from 30 minutes plus slack to 1 ms plus the default 5-second call slack (`pico-de-gallo-lib/src/lib.rs:118-157,847-867,1266-1288`).

One millisecond is the smallest non-zero value expressible by the protocol. It adds up to 1 ms of firmware waiting to each genuinely empty poll, on top of the measured roughly 300 microsecond round-trip floor. Thus this USB bridge is not literally non-blocking in wall-clock terms; it is a bounded polling approximation. A lost reply can still occupy the UART mutex for approximately **5.001 seconds**; that bound is derived from the 1 ms firmware allowance and the host library's 5-second default call slack, both constants in `crates/`, and has not been observed. No M5, M6 or M7 test can drive this path to the ceiling: the M6 fake bypasses the transport, while unplug produces a fast `-ECOMM`. A transport-fault latch ensures that cost is paid at most once per device lifetime, not once per byte.

### 1.1 Inventory

**Create**

- `zephyr/drivers/serial/pdg_uart.c`
- `zephyr/drivers/serial/pdg_uart_bottom.c`
- `zephyr/drivers/serial/pdg_uart_bottom.h`
- `zephyr/drivers/serial/CMakeLists.txt`
- `zephyr/drivers/serial/Kconfig`
- `zephyr/dts/bindings/serial/odp,pico-de-gallo-uart.yaml`
- `zephyr/tests/pdg_mfd_m5/reset_subscriptions/uart-build.overlay`

The final file is build-only metadata, not an application or behavioural test.

**Modify**

- `zephyr/Kconfig`
- `zephyr/drivers/{Kconfig,CMakeLists.txt}`
- `zephyr/boards/shields/pico_de_gallo/{pico_de_gallo.overlay,shield.yml}`
- `zephyr/scripts/ci-build.sh`
- `zephyr/{README.md,CHANGELOG.md}`

No `book/` edit or chapter. `AGENTS.md` section 15.1 makes README and changelog authoritative while this module is WIP.

### 1.2 Out of scope

- Anything under `crates/`; all versions, manifests and locks.
- UART sample and `zephyr/tests/pdg_fake/uart/` (M6).
- Running an image or touching hardware (M7).
- Real interrupt, async, line-control, driver-command or wide-data support.
- Driver-side TX buffering or a polling flush policy.
- A reconnect protocol or public diagnostics API; M6 may surface M5's private counters.
- `common.c`: every UART and generic reachable status is already mapped (`zephyr/drivers/common/common.c:27,33-38,48-57,83-92`).

## 2. Verified source facts and defects found

### 2.1 Pinned Zephyr API

Verified in WSL at Zephyr `26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0`, matching `zephyr/README.md:24-29`:

- `uart_driver_api` is in `uart_internal.h:38-141`, not `uart.h`.
- `poll_out` and `poll_out_u16` dispatch without NULL checks (`:182-190`).
- runtime-config slots exist only under `CONFIG_UART_USE_RUNTIME_CONFIGURE` (`:72-76`).
- `callback_set` NULL-checks (`:477-487`), but byte async operations do not (`:496-501,526-529,536-540,564-568,588-592`).
- three wide async operations also dispatch without NULL checks (`:511-516,550-554,576-579`).
- FIFO, IRQ, line-control, driver-command and `poll_in_u16` wrappers NULL-check or compile to `-ENOTSUP` (`:165-179,230-474,598-645`). Void IRQ wrappers silently no-op when NULL; they do not return an errno.

`uart.h:119-128` confirms byte-sized config fields. `poll_in` documents 0, exact `-1`, `-ENOSYS` and `-EBUSY` (`:340-358`); `poll_out` is void/blocking (`:381-397`). `uart_console.c:546-560` drains RX tightly. `kernel/mutex.c:107-116` asserts that mutexes cannot be locked from ISR context. `k_is_pre_kernel()` exists at `kernel.h:803-812`.

### 2.2 Devicetree and early console

Pinned `uart-controller.yaml:1-48` includes `base.yaml`, declares `bus: uart`, and supplies current speed, flow control, parity, stop bits and data bits. Only parity defaults (`:17-29`); speed (`:11-13`), stop bits (`:30-38`) and data bits (`:39-48`) do not. Enum indices match Zephyr C ordinals.

`CONFIG_EARLY_CONSOLE` installs the UART console hook at `PRE_KERNEL_1` (`uart_console.c:613-618`), but this driver initializes `POST_KERNEL`. Console init checks `device_is_ready()` first (`:602-610`), so the PDG UART cannot serve as an early console. Add a per-instance BUILD_ASSERT rejecting the configuration only when `CONFIG_EARLY_CONSOLE=y` **and** the chosen `zephyr,console` is this PDG instance. Do not globally forbid early console on an unrelated UART. Runtime pre-kernel guards remain mandatory even with the assertion.

### 2.3 FFI and generated header

`crates/pico-de-gallo-ffi/build.rs:5-11` writes `$OUT_DIR/include/pico_de_gallo.h`; Zephyr's active Corrosion build generates its own copy (`zephyr/CMakeLists.txt:74-96`). Current observed header `target/debug/build/pico-de-gallo-ffi-97a8949eaf79088a/out/include/pico_de_gallo.h` confirms C11/C17 `typedef uint8_t GalloUart*` (`:679-769`), capability bit 2 (`:59-62`), and response limit 1014 (`:103-123`). Exact prototypes, including widths:

```c
Status gallo_uart_read(const struct PicoDeGallo *gallo,
                       uint8_t *buf,
                       uint16_t count,
                       uint32_t timeout_ms,
                       uint16_t *out_len);
Status gallo_uart_write(const struct PicoDeGallo *gallo,
                        const uint8_t *buf,
                        uint16_t len);
Status gallo_uart_flush(const struct PicoDeGallo *gallo);
Status gallo_uart_set_config(const struct PicoDeGallo *gallo,
                             uint32_t baud_rate,
                             uint8_t data_bits,
                             uint8_t parity,
                             uint8_t stop_bits);
Status gallo_uart_get_config(const struct PicoDeGallo *gallo,
                             uint32_t *out_baud_rate,
                             uint8_t *out_data_bits,
                             uint8_t *out_parity,
                             uint8_t *out_stop_bits);
```

Observed at header `:1890-2005`, matching Rust `lib.rs:2596-2602,2654-2658,2700,2791-2797,2862-2868`.

### 2.4 Capability cost - accepted M5 tradeoff, follow-up escalation

`gallo_get_device_info()` unconditionally calls `validate()` (`ffi/lib.rs:3565-3599`). `validate()` performs a fresh `device/info` request (`lib/lib.rs:1636-1657`) under `DEVICE_INFO_TIMEOUT = 300 s` (`:101-116`) and caches only `num_gpios` (`:1669-1746`). Consequently `pdg_uart_bottom_has_uart()` adds a **second device-info RPC** after parent strict open; it is not warm. M6 can prove that this second RPC occurs. Its **300-second ceiling is derived from the `DEVICE_INFO_TIMEOUT` constant in `crates/`, not observed**, and no M5, M6 or M7 test can drive the capability probe to that ceiling.

M5 accepts this documented boot-time cost because the capability gate prevents silent output loss on hw-rev1 and no cache-backed capability accessor exists within allowed scope. This is the same bounded operation class already paid by parent validation, but it is a second exposure. A follow-up should change `pico-de-gallo-lib` to cache the full validated `DeviceInfo` in a clone-shared `OnceLock<DeviceInfo>` (or equivalent), make both `validate()` and capability consumers reuse it, and expose an FFI cached capability query; that requires `crates/` changes and is not authorized here. The child preserves distinct outcomes: RPC failure returns its mapped errno; successful clear bit returns `-ENODEV`.

### 2.5 DEFECTS FOUND

1. **Three omitted wide async hazards.** Design §6.2 omitted `tx_u16`, `rx_enable_u16`, `rx_buf_rsp_u16`; pinned wrappers have no NULL checks (`uart_internal.h:511-579`). M5 adds nested async+wide stubs.
2. **Capability warmth claim was false.** Design §6.8 says local/no USB (`design.md:567-576`), contradicted by §2.4's fresh `validate()` path.
3. **Zero-timeout host bound was misread.** Firmware zero is a one-poll operation, but host `bounded_for(0)` selects 1,800,000 ms plus call slack (`lib/lib.rs:847-867`; test `:3064-3073`). M5 uses firmware timeout 1 ms.
4. **`callback_set` hazard was overstated.** It NULL-checks (`uart_internal.h:477-487`). It remains NULL so Zephyr returns `-ENOSYS`; no stub is needed.
5. **Existing native include lines are redundant, not broken.** I2C/GPIO/SPI bottom headers are same-directory, so ineffective `target_include_directories` entries do not break them. Cross-directory `common.h` and generated FFI paths are already supplied via compile options (`drivers/CMakeLists.txt:32-34`, `zephyr/CMakeLists.txt:79-96`). UART needs no same-directory `-I`; any future cross-directory native include must use `target_compile_options`.
6. **One generated header copy is stale.** Build hash `e2c65c3c3be1c5af` still says 1 ms at header `:1940`; current hash `97a8949eaf79088a` says a single poll. Never select arbitrary target output by glob order; use the active Corrosion header.

## 3. Top/bottom architecture

| File | Allowed | Forbidden |
|---|---|---|
| `pdg_uart.c` | Zephyr headers, bottom header, MFD header after assertions | `pico_de_gallo.h`, `common.h` |
| `pdg_uart_bottom.c` | standard C, FFI header, `common.h`, own header | Zephyr headers, `__weak` |
| `pdg_uart_bottom.h` | `stdbool.h`, `stdint.h` only | Zephyr/FFI headers |

Every bottom function is `__attribute__((weak))` so M6 can supply strong recording fakes. The exact shared surface is:

```c
#define PDG_UART_RX_BUFFER_SIZE 1014U
/* Eleven neutral selector constants listed in §4. */
int pdg_uart_bottom_read(void *, uint8_t *, uint16_t, uint32_t, uint16_t *);
int pdg_uart_bottom_write(void *, const uint8_t *, uint16_t);
int pdg_uart_bottom_set_config(void *, uint32_t, uint8_t, uint8_t, uint8_t);
int pdg_uart_bottom_has_uart(void *, bool *);
```

No open/close, flush, or get-config wrapper. Top cache serves `config_get`; polling API has no flush callback.

## 4. Two-stage configuration mapping gate

### 4.1 Neutral-to-FFI gate

The shared header declares neutral constants:

```c
PDG_UART_DATA_BITS_5=0, _6=1, _7=2, _8=3
PDG_UART_PARITY_NONE=0, _ODD=1, _EVEN=2, _MARK=3, _SPACE=4
PDG_UART_STOP_BITS_1=0, _2=1
```

`pdg_uart_bottom.c`, the only TU seeing both vocabularies, carries all eleven assertions plus the size assertion:

```c
_Static_assert(PDG_UART_RX_BUFFER_SIZE == GALLO_MAX_RESPONSE_PAYLOAD, ...);
_Static_assert(PDG_UART_DATA_BITS_5 == GalloUartDataBits_Five, ...);
_Static_assert(PDG_UART_DATA_BITS_6 == GalloUartDataBits_Six, ...);
_Static_assert(PDG_UART_DATA_BITS_7 == GalloUartDataBits_Seven, ...);
_Static_assert(PDG_UART_DATA_BITS_8 == GalloUartDataBits_Eight, ...);
_Static_assert(PDG_UART_PARITY_NONE == GalloUartParity_None, ...);
_Static_assert(PDG_UART_PARITY_ODD == GalloUartParity_Odd, ...);
_Static_assert(PDG_UART_PARITY_EVEN == GalloUartParity_Even, ...);
_Static_assert(PDG_UART_PARITY_MARK == GalloUartParity_Mark, ...);
_Static_assert(PDG_UART_PARITY_SPACE == GalloUartParity_Space, ...);
_Static_assert(PDG_UART_STOP_BITS_1 == GalloUartStopBits_One, ...);
_Static_assert(PDG_UART_STOP_BITS_2 == GalloUartStopBits_Two, ...);
```

These are exhaustive for today's 4+5+2 FFI variants, but cannot detect a future appended FFI variant: no generated symbol exports variant count. Closing that residual requires a `crates/`/header contract change and is outside M5. Existing Rust ABI tests and review remain the append gate.

### 4.2 Zephyr-to-neutral gate

The top TU must not contain an independently hand-written switch mapping. Define one constant-expression mapping macro per field; the runtime helper and compile-time assertions both consume that macro:

```c
#define PDG_UART_MAP_DATA_BITS(v) \
        ((v) == UART_CFG_DATA_BITS_5 ? PDG_UART_DATA_BITS_5 : \
         (v) == UART_CFG_DATA_BITS_6 ? PDG_UART_DATA_BITS_6 : \
         (v) == UART_CFG_DATA_BITS_7 ? PDG_UART_DATA_BITS_7 : \
         (v) == UART_CFG_DATA_BITS_8 ? PDG_UART_DATA_BITS_8 : UINT8_MAX)
/* Equivalent complete forms for PARITY and STOP_BITS. */
```

The runtime helper computes mapped = PDG_UART_MAP_*(input), rejects UINT8_MAX, and otherwise forwards exactly mapped; it does not repeat the mapping in a switch. Field-specific diagnostics may classify invalid values, but may not choose the wire byte. Emit one BUILD_ASSERT(PDG_UART_MAP_*(ZEPHYR_VALUE) == NEUTRAL_VALUE, ...) for each accepted pair. Thus changing only the mapping used at runtime changes the assertion result too. This matters for stop bits: UART_CFG_STOP_BITS_1 is ordinal 1 but maps to neutral 0, so raw ordinal equality is forbidden. M6 fake tests record the byte emitted for each accepted pair; M7 observes the resulting framing with an independent peer.

The complete accepted pairs are:

```text
DATA:   (UART_CFG_DATA_BITS_5, PDG_UART_DATA_BITS_5) ... through 8
PARITY: (NONE,NONE), (ODD,ODD), (EVEN,EVEN), (MARK,MARK), (SPACE,SPACE)
STOP:   (UART_CFG_STOP_BITS_1, PDG_UART_STOP_BITS_1),
        (UART_CFG_STOP_BITS_2, PDG_UART_STOP_BITS_2)
```

Together, the top Zephyr-to-neutral mapping expressions and bottom neutral-to-FFI assertions form a consistency gate: the runtime mapping and compile-time assertions cannot diverge. They do **not** verify that the accepted pairs themselves are correct; a self-consistent wrong edit to a macro arm and its matching assertion still compiles. Transcription correctness is M6 evidence from the fake recording the emitted byte and M7 evidence from an independent peer observing the framing. Future appended variants remain outside this gate.

## 5. Driver state, context policy, and observability

```c
struct pdg_uart_config {
        const struct device *mfd;
        const char *serial_number;
        struct uart_config initial;
};
struct pdg_uart_data {
        void *ctx;
        struct k_mutex lock;
        struct uart_config current;
        uint8_t rx_buf[PDG_UART_RX_BUFFER_SIZE];
        uint16_t rx_pos;
        uint16_t rx_len;
        atomic_t tx_dropped;
        atomic_t rx_errors;
        atomic_t last_errno;
        atomic_t link_failed;
};
```

`current` is the last successfully requested configuration, matching firmware's software shadow. `rx_pos <= rx_len <= 1014`. Atomic diagnostics are driver-private in M5 and inspectable by debugger/source-level test access; M6 owns a shell/stat or driver-specific API if maintainers want runtime user access. `last_errno` stores the most recent negative errno; `link_failed` means a confirmed transport-class failure (`-ECOMM` or `-ETIMEDOUT`), not a UART line error.

### 5.1 Thread-context-only policy

Every callback tests `k_is_in_isr()` and `k_is_pre_kernel()` before mutex or FFI. This controller is thread-context-only; a spinlock across USB is forbidden.

- Integer callbacks other than polling RX return `-EWOULDBLOCK` in ISR and `-EAGAIN` pre-kernel. This includes runtime config, config get and err check.
- `poll_in` returns exactly `-1` in ISR/pre-kernel, NULL/dead/link-failed, empty and any refill-error case. It never exposes arbitrary transport errno through the polling contract and leaves `*p_char` untouched.
- `poll_out` drops and increments `tx_dropped` in ISR/pre-kernel, NULL/dead/link-failed and write-error cases.
- Unsupported async integer stubs remain side-effect-free and return `-ENOSYS` for `callback_set`, `-ENOTSUP` for no-NULL operation slots. They need no mutex/FFI; when called on failed init they first return `-ENODEV`, and ISR/pre-kernel checks still precede context inspection for one uniform public-callback policy.
- Void `poll_out_u16` drops/counts without logging.

Failed init makes `device_is_ready()` false (`kernel/device.c:18-60,186-197`), but UART wrappers still dispatch through the API table. Therefore context guards are mandatory primary protection, not defence in depth.

The device cannot be an early console. `CONFIG_EARLY_CONSOLE` hooks at PRE_KERNEL_1 while this device initializes POST_KERNEL; readiness fails and the hook is not installed (`uart_console.c:602-618`). The chosen-console-specific BUILD_ASSERT in §7 turns this into a readable build incompatibility. Pre-kernel guards remain because direct callers can still dispatch.

### 5.2 Fault latching and no recursive logging

No `LOG_*` call appears in `poll_out` or in anything it calls, before or after unlocking; this is checkable by review and grep. Logging through the same UART would recurse per character, self-deadlock if the mutex remains held, or amplify indefinitely after unlock. Instead:

- every dropped byte atomically increments `tx_dropped`;
- any operation error stores `last_errno`;
- `-ECOMM` or `-ETIMEDOUT` atomically sets `link_failed` and discards staged RX (`rx_pos = rx_len = 0` while locked where applicable);
- later `poll_in` returns `-1` and later `poll_out` drops/counts without another RPC.

There is no reconnect path in `pdg_mfd.c:50-103` or `gallo_registry.c:143-248`; fail-fast lasts for the static device lifetime. A future reconnect design must be parent-owned and generation-aware. Do not permanently latch endpoint `-EIO`: it can represent a UART error, not transport death.

A timed-out/communication-failed write may already have queued its byte before the response was lost. Never retry automatically. Capability gating only rejects unsupported hardware at init; after init bytes can still be lost or indeterminate on unplug, supervisor reset, half-open transport, timeout/lost ACK, endpoint error, schema skew or stale context. README must say so.

## 6. RX and TX data paths

### 6.1 RX: 1014-byte refill with 1 ms firmware timeout

Invariant **RX-RECOVERY**: every empty-ring refill calls `gallo_uart_read(..., timeout_ms=1)`, never `read_ready()` and never a separate readiness probe. Both zero and non-zero firmware branches reach Embassy `try_read`; the non-zero evidence is `uart.rs:64-69` -> `buffered.rs:637-646` -> `:229-240` -> `:262-290`. `try_read` consumes `rx_error` and re-enables RX interrupts. M7 must prove this recovery with controlled fault injection; static evidence is not hardware evidence.

Under mutex:

1. If a staged byte exists, return it without RPC.
2. Otherwise reset indices and issue one `bottom_read(ctx, rx_buf, 1014, 1, &rx_len)`.
3. Success with zero bytes returns `-1`.
4. Success with impossible `rx_len > 1014` increments `rx_errors`, stores `-EIO`, empties ring, returns `-1`.
5. Any endpoint or transport failure increments `rx_errors`, stores exact errno, empties ring and returns `-1`.
6. Transport failure additionally latches `link_failed`; later polls fail fast with `-1`.
7. Success serves byte zero and advances position.

This deliberately narrows the public `poll_in` result to Zephyr's polling contract: 0 for one byte, otherwise `-1` for no byte, invalid context, context prohibition or failure. Exact diagnostics remain in private atomics. `p_char` is untouched unless returning 0.

Repeated endpoint errors such as `-EIO` are not permanently latched, because the first read is required to consume Embassy's RX-error latch. To avoid unlimited immediate RPCs in a tight poll loop, after any non-transport refill error set a monotonic retry deadline **10 ms** in the future. Until that deadline, return `-1` without RPC. Ten milliseconds is bounded, larger than the 1 ms firmware poll, and short enough for interactive polling; M6 fake tests use `k_sleep` past the backoff deadline to pin one RPC per backoff interval. `native_sim` advances its clock on `k_sleep`, so no injected-time capability is required. Successful refill clears backoff. Use Zephyr uptime (`k_uptime_get`) only in thread context after the context guards.

The mutex is held across the read RPC. The finite blocking bound is approximately 5.001 seconds (1 ms firmware allowance plus default 5 s host slack), derived from constants in `crates/` and not observed; the M6 fake bypasses the transport, and unplug produces a fast `-ECOMM`, so no M5, M6 or M7 test can drive this path to the ceiling. Configuration/TX can wait behind it. This priority inversion is accepted and documented; a future asynchronous architecture would be needed to remove it.

### 6.2 TX: deliberately unbuffered

`poll_out` performs one `bottom_write(ctx, &byte, 1)` under mutex after context guards. Firmware already owns a 1024-byte ISR-drained ring and FFI returns when queued (`lib.rs:2640-2644`). A second ring has no polling flush hook; newline strands prompts, timer/workqueue is unsafe for early console/ISR, and void output cannot report deferred failures.

One write has the ordinary non-duration host bound of 5 seconds (`lib/lib.rs:118-157`). On return, unlock before any non-poll-out diagnostic action; however `poll_out` itself never logs. Failure updates atomics as §5.2 specifies. There is no retry.

Multiple reader/writer threads are serialized but not message-demultiplexed. Concurrent readers consume distinct bytes in scheduler order; applications requiring message ownership must serialize above the driver.

## 7. Configuration, topology, and initialization

### 7.1 Validation and baud policy

`struct uart_config` fields are bytes; use the shared mapping expressions in section 4.2 and positive allow-list validation following `pdg_gpio.c:100-116,208-251`. Every rejection log ends `Returning -EXXX.`

| Field | Accepted | Rejection |
|---|---|---|
| baud | every non-zero `u32`, forwarded unchanged | zero `-EINVAL` |
| data | 5, 6, 7, 8 | 9 and unknown `-ENOTSUP` |
| parity | none, odd, even, mark, space | unknown `-ENOTSUP` |
| stop | 1, 2 | 0.5, 1.5 and unknown `-ENOTSUP` |
| flow | none | `UART_CFG_FLOW_CTRL_RTS_CTS`, `UART_CFG_FLOW_CTRL_DTR_DSR`, `UART_CFG_FLOW_CTRL_RS485`, unknown: `-ENOTSUP` |

Nonzero does not mean achievable. Firmware silently clamps the divisor to an approximately 143 baud minimum; a request such as 1 is forwarded and `config_get` reports requested 1, not achieved rate (`design.md:389-397,790-793`). Preserve this limitation.

`configure` rejects NULL, validates before locking, then sets under mutex. On acknowledged success it updates `current` and clears local RX staging (`rx_pos = rx_len = 0`). Callers must quiesce and drain both directions before configure: firmware-ring bytes already decoded under old framing cannot be distinguished and may still arrive after local clearing. On failed set, cache and local ring stay unchanged unless failure is transport-class; transport failure establishes a new stream-generation boundary, clears ring and latches link failure. No rollback.

`config_get` copies cache under mutex. Both slots/functions are guarded by `CONFIG_UART_USE_RUNTIME_CONFIGURE` (`uart_internal.h:72-76`).

DT initial state:

```c
.baudrate = DT_INST_PROP_OR(inst, current_speed, 115200),
.parity = DT_INST_ENUM_IDX_OR(inst, parity, UART_CFG_PARITY_NONE),
.stop_bits = DT_INST_ENUM_IDX_OR(inst, stop_bits, UART_CFG_STOP_BITS_1),
.data_bits = DT_INST_ENUM_IDX_OR(inst, data_bits, UART_CFG_DATA_BITS_8),
.flow_ctrl = DT_INST_PROP(inst, hw_flow_control)
             ? UART_CFG_FLOW_CTRL_RTS_CTS : UART_CFG_FLOW_CTRL_NONE,
```

### 7.2 API object

Use designated initializers only. Implement `poll_in`, `poll_out`, `err_check`; conditionally implement `configure`/`config_get`. Under async, leave `callback_set = NULL` so Zephyr returns `-ENOSYS`; provide `-ENOTSUP` stubs for `tx`, `tx_abort`, `rx_enable`, `rx_buf_rsp`, `rx_disable`, and nested async+wide `tx_u16`, `rx_enable_u16`, `rx_buf_rsp_u16`. Under wide, provide void `poll_out_u16` drop/count stub. Leave `poll_in_u16`, FIFO, IRQ, line-control and command NULL.

Slot-to-stub correctness is review-only. Several `uart_driver_api` slots share a signature—for example, `tx_abort` and `rx_disable` are both `int (*)(const struct device *)`—so swapping their designated initializers type-checks, and identical `-ENOTSUP` bodies make the swap behaviourally invisible in both M5 and M6. This becomes a real hazard as soon as either stub gains distinct behaviour.

Void IRQ wrappers (`irq_tx_enable/disable`, `irq_rx_enable/disable`, `irq_err_enable/disable`, `irq_update`) silently no-op when NULL (`uart_internal.h:302-322,344-364,400-448`). Integer FIFO/IRQ/callback wrappers return `-ENOSYS`; disabled global APIs return `-ENOTSUP`. `err_check` reports 0 only for healthy initialized thread-context calls; it reports context errors as §5.1 specifies but never invents line-error bits.

### 7.3 Binding and assertions

Binding: `compatible: "odp,pico-de-gallo-uart"`, `include: [uart-controller.yaml, base.yaml]`. Prose specifies direct MFD child, borrowed ownership, required parent serial, thread-context-only blocking behaviour, RX/TX design, supported configuration, non-atomicity, unsupported surfaces, error observability, capability gate and second metadata query whose 300-second ceiling is derived from the `crates/` constant, not observed.

Before including `pdg_mfd.h`, per-instance assertions in exact order:

1. parent compatible;
2. parent status okay;
3. parent has `serial_number`;
4. MFD Kconfig enabled;
5. if `CONFIG_EARLY_CONSOLE`, this instance is not `DT_CHOSEN(zephyr_console)`.

The fifth is instance-scoped so another UART may be early console. Guard chosen-node evaluation with `DT_HAS_CHOSEN(zephyr_console)` to avoid malformed expansion when absent. Source order is normative. UART drives TX, so selector-less operation is rejected like GPIO/SPI.

### 7.4 Initialization

At `POST_KERNEL/CONFIG_UART_PICO_DE_GALLO_INIT_PRIORITY`:

1. Initialize mutex and all atomics/ring state before return.
2. Parent readiness, then borrow; failures `-ENODEV`.
3. Call `bottom_has_uart`: this is a fresh `device/info` RPC after parent already paid validation. Its 300 s bound is derived from the `DEVICE_INFO_TIMEOUT` constant in `crates/`, not observed or reachable in M5/M6/M7 tests.
4. Failure clears child context and returns mapped error; clear bit returns distinct `-ENODEV`.
5. Validate DT config; reject before set.
6. Set config; on failure invalidate child.
7. Cache config, log parent serial, return 0.

Use plain `DEVICE_DT_INST_DEFINE`. Failed-init wrappers still dispatch, so NULL guards are primary safety.

## 8. Build and shield wiring

`serial/Kconfig`: driver bool/default y, depends DT compatible and ARCH_POSIX, selects `SERIAL` and `SERIAL_HAS_DRIVER`; priority int/default 50 after MFD 40. Do not select async/IRQ/wide.

`serial/CMakeLists.txt`: Zephyr library/top source/local include; bottom on `native_simulator INTERFACE`. Same-directory quoted bottom header needs no `-I`. If any genuinely cross-directory native header is added, use `target_compile_options(... "-I...")`; `target_include_directories` is redundant for current same-directory headers and absent from `NSI_BUILD_OPTIONS`.

Root wiring:

1. UART DT symbol in `zephyr/Kconfig:7`, or UART-only skips Corrosion.
2. `rsource "serial/Kconfig"` in driver Kconfig.
3. serial subdirectory and UART in common/registry guard in driver CMake.

Shield adds disabled `pdg_uart0` between I2C/SPI with explicit 115200, data `<8>`, parity `"none"`, stop `"1"`, no flow; `shield.yml` adds `uart`.

## 9. CI build-gate resolution

No existing overlay enables the disabled UART node. Add UART to both two-sided universes:

```text
PDG_ALL_DRIVER_TUS += pdg_uart.c
PDG_ALL_DRIVER_KCONFIGS += CONFIG_UART_PICO_DE_GALLO
```

This gives all nine old targets negative UART coverage without changing their rows.

Add the tenth row **immediately after `m5_reset`**:

```text
uart_driver|pass|zephyr/tests/pdg_mfd_m5/reset_subscriptions|zephyr/tests/pdg_mfd_m5/reset_subscriptions/uart-build.overlay|pdg_mfd.c,pdg_uart.c|gallo_registry,pdg_uart_bottom,m5_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_UART_PICO_DE_GALLO
```

The new overlay includes fixture identity, enables parent/UART, explicitly disables GPIO/I2C/SPI, and adds no UART consumer. The existing reset app depends only on MFD/reset shim and already asserts GPIO/SPI disabled (`reset_subscriptions/src/main.c:23-46`); its config does not disable UART (`prj.conf:8-11`). The binary is never run.

Concrete script updates:

- insert row after array index 4 (`m5_reset`); existing positional checks `[0]`, `[2]`, `[4]` keep meaning;
- change table-count assertion 9 to 10;
- change "all nine" text to ten and exact string to insert `uart_driver` immediately after `m5_reset`;
- change four-driver comments/universes to five;
- leave every old expected set and pass/basefail kind untouched.

Alternatives rejected: enabling UART in an old target changes its contract; creating an application crosses M6. This overlay is build metadata only.

## 10. Documentation contract

README updates overview, topology and CI count; adds UART enablement with explicit parent serial; documents:

- thread-context-only; ISR/pre-kernel RX returns `-1`, TX drops/counts;
- incompatibility with choosing PDG UART as `CONFIG_EARLY_CONSOLE`;
- 1014-byte RX staging and 1 ms firmware poll, with an approximately 5.001 s lost-response mutex bound derived from `crates/` constants and not observed or reachable in M5/M6/M7 tests;
- one-RPC-per-byte unbuffered TX and complete loss/indeterminate-delivery modes;
- no logging from `poll_out`; M5 private `tx_dropped`, `rx_errors`, `last_errno`, `link_failed`, public surfacing deferred M6;
- transport fault is permanent/fail-fast until process/device reinitialization; no reconnect;
- 10 ms non-transport RX-error backoff;
- successful configure clears local RX but cannot identify old-framing bytes still in firmware; quiesce/drain first;
- complete non-atomic configuration, requested-versus-achieved baud shadow and approximately 143 baud minimum;
- capability lookup adds a second device-info RPC at init; M6 can prove the call occurs, while its up-to-300-second ceiling is derived from a `crates/` constant and is not observed or reachable in M5/M6/M7 tests;
- `err_check` reports no line detail and separate private health state exists;
- capability gate only handles unsupported hardware at init. Runtime TX can be lost on unplug, supervisor reset, half-open transport, timeout/lost ACK, endpoint error, schema skew or stale handle. Timed-out write may already be queued; never retry blindly.

Result table separates:

- integer IRQ/FIFO/callback NULL slots -> `-ENOSYS` (or global-off `-ENOTSUP`);
- void IRQ NULL slots -> silent no-op;
- `poll_in_u16` -> `-ENOSYS` when wide on, `-ENOTSUP` when off;
- no-NULL async slots -> explicit `-ENOTSUP` stubs;
- `callback_set` -> NULL and `-ENOSYS`.

Changelog Unreleased/Added records binding/controller, RX/TX architecture, context policy, mapping gates, fault/backoff diagnostics, capability cost, tenth target, and M6/M7 deferrals. No book/crate changelog.

## 11. Invariants and failure modes

### 11.1 Invariants

1. Parent owns the handle; child never closes it. The no-close claim is review-only; no M5 assertion or M6 fake check enforces it.
2. Top/bottom contexts never include each other's platform headers.
3. Every bottom function is weak and maps status.
4. Current mapping coverage is two-stage: shared-expression Zephyr-to-neutral plus eleven neutral-to-FFI assertions; appended variants remain a documented residual.
5. Every callback checks ISR/pre-kernel before lock/FFI; controller is thread-context-only.
6. No `LOG_*` call appears in `poll_out` or anything it calls; it never retries or buffers, and every drop is counted.
7. `poll_in` returns only 0 or `-1`, writes output only on 0, and latches exact diagnostics privately.
8. **RX-RECOVERY:** every refill reaches Embassy `try_read`; no `read_ready()` shortcut.
9. Firmware read timeout is exactly 1 ms; maximum host wait is approximately 5.001 s with default call slack, derived from constants in `crates/` and not observed or reachable in M5/M6/M7 tests.
10. First transport failure discards local RX and permanently fails fast; pre-lock plus post-lock latch checks prevent both fresh and already-queued callers from issuing another RPC.
11. Endpoint RX errors receive one RPC per 10 ms backoff interval, not unlimited immediate retry.
12. Successful configure clears local staging; callers quiesce/drain due to firmware residue.
13. Cache updates only after acknowledged complete configuration; configuration is not atomic.
14. Mutex is initialized before every init return; failed-init context guards are primary protection.
15. Capability lookup is explicitly a second exposure, not warm; M6 can prove the RPC occurs, while the 300-second ceiling is derived from a `crates/` constant and is not observed or reachable in M5/M6/M7 tests.
16. Enabled UART requires parent serial and cannot be chosen early console.
17. Tenth CI target positively covers UART; old nine negatively cover it unchanged.

Scope: M5 claims build/link only; this is not an invariant.

### 11.2 Failure table

| Event | Driver result and residue |
|---|---|
| ISR/pre-kernel RX | `-1`, no lock/RPC |
| ISR/pre-kernel TX | drop/count, no log/lock/RPC |
| Parent/borrow init failure | not ready `-ENODEV`, no UART RPC |
| Capability RPC failure | not ready, mapped error; 300 s ceiling derived from a `crates/` constant, not observed or test-reachable |
| Capability clear | distinct `-ENODEV` |
| Invalid initial config | not ready, no set RPC |
| Initial set failure | not ready; remote may have applied before lost ACK; no rollback |
| RX empty | one 1 ms firmware poll, `-1` |
| RX endpoint error | exact errno/count latched, ring empty, `-1`, 10 ms backoff |
| RX transport error | exact errno/count latched, ring discarded, permanent fail-fast, `-1` |
| TX endpoint error | byte dropped/count, last errno, no retry/log; future traffic allowed |
| TX transport error | byte delivery indeterminate, dropped/count, permanent fail-fast; no retry |
| Successful configure | cache update and local ring clear; firmware old-decoded bytes may remain |
| Failed configure | cache unchanged; transport failure also establishes dead generation |
| Direct call after failed init | wrapper still dispatches; context guard returns/drops without FFI |

## 12. Testability and deferred work

### 12.1 M5 build-time

Verify binding; parent/serial/early-console assertions; conditional API table including three wide async stubs; top shared mapping-expression assertions; bottom eleven enum and limit assertions; top TU/bottom object; exact ten-target sets; all old outcomes; self-test. No image runs.

`ci-build.sh` has no target kind that can establish that a compile-time assertion fires. Its `basefail` kind is hard-wired to a runner-link failure: it requires `zephyr/zephyr.elf` and exactly one undefined `__device_dts_ord_N` resolving to `is31fl3743b`. A failed `BUILD_ASSERT` produces neither the ELF nor that ordinal, so the script reports it as the wrong kind of failure rather than a pass. The M5 compile-time assertions are therefore evidenced by review plus the recorded one-line mutation: changing the first `PDG_UART_MAP_STOP_BITS` arm to `PDG_UART_STOP_BITS_2` failed with the expected static assertion, while a diagnostic-string-only negative control still built. A general compile-failure target kind could improve the gate later, but is outside M5.

The existing `-Werror=switch` remains load-bearing for `common.c`, but provides no new UART coverage. `struct uart_config` fields are `uint8_t`, so switching on them gives no enum-exhaustiveness diagnostic; §4.2 forbids a top-half mapping switch; and the bottom half delegates status handling to `pdg_common_status_to_errno()` rather than adding a UART `Status` switch.

### 12.2 M6 recording fake and diagnostics

Deferred sample plus fake coverage:

- ISR/pre-kernel guards never lock/call FFI;
- empty poll uses timeout 1 and returns `-1`;
- one refill serves many bytes; impossible length;
- transport failure latches once, clears ring, later and already-mutex-queued calls make zero RPCs;
- endpoint error backoff uses `k_sleep` past the deadline, allows one RPC per 10 ms and recovers after success;
- one write per thread-context output; no TX logging/retry/buffer;
- drop/error counters and health state;
- successful config clears local ring; failed config cache semantics;
- fake records the emitted byte for each accepted stop/parity/data route;
- failed-init direct dispatch and all async/wide stubs;
- exhaustive strong overrides for every bottom symbol.

M6 decides how to surface private diagnostics through shell/stat/driver API.

### 12.3 M7 hardware risk register

- hw-rev2 ready and hw-rev1 capability refusal;
- verify the 1 ms non-zero firmware branch still consumes controlled framing/parity/overrun error and RX resumes without reboot;
- loopback polling/throughput and approximately 1 ms idle cost;
- independent peer for parity and framing distinctions;
- unplug/reset during read/write/configure: bounded return, one-time fault latch, no recursive output/logging, ring discard;
- timed-out TX lost-ACK ambiguity with no duplicate;
- configuration boundary after quiesce/drain.

No M5 build result proves runtime behaviour.

## 13. Review-resolution matrix

| ID | Resolution | Spec location |
|---|---|---|
| C-B1 / R-B2 ISR/pre-kernel | Thread-context-only; guards before lock/FFI; RX `-1`, TX drop/count; chosen-console early-console BUILD_ASSERT | §§2.2,5.1,7.3 |
| C-B2 mapping gate | Keep eleven bottom asserts; add shared-expression top mapping and assertions | §4 |
| R-B1 30-minute poll | **A**: non-zero 1 ms still reaches `try_read`; host bound ~5.001 s is derived from `crates/` constants, not observed; transport latch pays once | §§1,6.1 |
| R-B3 recursive logging | No logs from `poll_out`; atomics and fail-fast latch | §§5.2,6.2 |
| R-B4 capability cache | False warm claim removed; second RPC is M6-provable, while its 300 s ceiling is derived from a `crates/` constant and not observed; cache follow-up escalated | §2.4 |
| C-S1 / R-S6 poll-in contract | 0 only for byte; all other outcomes `-1`; exact errno private | §§5.1,6.1 |
| C-S2 callback_set | Leave NULL -> `-ENOSYS` | §7.2 |
| C-S3 IRQ wrappers | Void no-op separated from integer `-ENOSYS` | §§2.1,7.2,10 |
| C-S4 flow enums | RTS_CTS, DTR_DSR, RS485 named rejects | §7.1 |
| C-S5 baud policy | Every nonzero forwarded; ~143 minimum; requested shadow | §§7.1,10 |
| C-S6 append limitation | Explicit residual; no count symbol; requires crates change | §4.1 |
| R-S1 transport latch | `-ECOMM`/`-ETIMEDOUT` latch, ring discard, later no RPC | §§5.2,6 |
| R-S2 health channel | Private atomics M5; public surfacing M6 | §§5,10,12.2 |
| R-S3 config ring | Clear local ring on success; firmware residue documented | §7.1 |
| R-S4 disconnect residue | Transport failure is stream-generation boundary | §§5.2,11.2 |
| R-S5 RX backoff | 10 ms endpoint-error backoff; transport permanent | §6.1 |
| R-S7 TX loss | Complete loss and lost-ACK/no-retry documentation | §§5.2,10 |
| R-S8 RX recovery invariant | Named RX-RECOVERY and M7 controlled-fault gate | §§6.1,11.1,12.3 |
| R-S9 mutex bound | ~5.001 s derived from `crates/` constants, not observed; accepted priority inversion stated | §§1,6.1 |
| R-S10 failed-init dispatch | NULL guards are primary protection | §§5.1,7.4 |
| Nit: include directories | Existing entries redundant, not broken; cross-directory rule retained | §§2.5,8 |
| Nit: prototypes | Exact widths and parameter shapes preserved | §2.3 |

No reviewer finding is rejected.

## 14. File-by-file task breakdown

One coding agent per file. Waves define ordering and parallelism; only integration is serialized.

### Wave 0 - contracts and topology, parallel

1. **`zephyr/drivers/serial/pdg_uart_bottom.h` - create; scope grew.** Own four-function plain-C surface, response literal, eleven neutral values. Includes bool/int only. Traps: open/close, wrong widths, Zephyr/FFI leak, computed limit.
2. **`zephyr/dts/bindings/serial/odp,pico-de-gallo-uart.yaml` - create; scope grew.** Own direct-parent/serial contract, thread-only policy, early-console incompatibility, 1 ms/5.001 s bound, 300 s capability cost, loss/backoff/config semantics. Mark both ceilings as derived from `crates/` constants and not observed or test-reachable. Traps: warm claim, atomic claim, achieved-baud claim.
3. **`zephyr/drivers/serial/Kconfig` - create; scope grew.** Own driver, SERIAL/SERIAL_HAS_DRIVER, priority, thread-only/non-atomic help. Do not select async/IRQ/wide.
4. **Shield overlay - modify.** Add disabled explicit 115200 8N1 child only.
5. **`shield.yml` - modify.** Add UART feature only.
6. **`reset_subscriptions/uart-build.overlay` - create.** Fixture identity; parent/UART enabled; other controllers disabled; no consumer.

### Wave 1 - driver implementation, parallel after bottom header

7. **`pdg_uart_bottom.c` - create.** Weak read/write/set/capability adapters, status mapping, eleven enum and size assertions. Capability output only after successful `gallo_get_device_info`. Traps: warm assumption, raw Status, non-weak, reading uninitialized info.
8. **`pdg_uart.c` - create; scope substantially grew.** Own context guards, early-console assertion, data/ring/config, atomics, 10 ms backoff, transport latch, no-log `poll_out`, 1 ms refill, shared runtime/compile mapping expressions, all API slots, capability/init. Traps: any log in output callback; any mutex before ISR/pre-kernel check; zero timeout; arbitrary poll-in errno; permanent latch on `-EIO`; preserving ring on transport/config success; hand-written mapping separate from assertions.
9. **Serial CMake - create; wording corrected.** Add top library and bottom host source. No same-directory native `-I` needed. Use compile-options only if a genuine cross-directory native header is introduced.

### Wave 2 - root wiring and gate, parallel

10. **`zephyr/Kconfig` - modify.** Add UART DT discovery only.
11. **`drivers/Kconfig` - modify.** Source serial Kconfig.
12. **`drivers/CMakeLists.txt` - modify.** Add serial directory and UART common guard.
13. **`ci-build.sh` - modify.** Insert tenth row after `m5_reset`; update both five-driver universes, comments, 10 count and exact all-target string; leave old rows unchanged.

### Wave 3 - documentation, parallel after behaviour fixed

14. **README - modify; scope substantially grew.** Implement §10 including context policy, bounds, permanent transport fault, diagnostics, complete TX loss, capability delay, IRQ split, baud truth and deferred evidence.
15. **Changelog - modify; scope grew.** Unreleased/Added entry with safety/reliability semantics and M6/M7 deferrals.

### Wave 4 - serialized integration/tester

16. Integrator runs `ci-build.sh --self-test`, then coordinator-owned full ten-target build. Inspect TU/Kconfig/object gates and unchanged old basefail categories. Never run binary.
17. Tester provides structural/build acceptance and schedules M6 fake cases from §12.2. No USB.

## 15. Alternatives considered

- Keep timeout zero: rejected because host bound is 30 minutes plus slack; non-zero still reaches `try_read`.
- More than 1 ms: increases every empty poll with no recovery benefit. One is protocol minimum.
- Claim true non-blocking: rejected; the derived, unobserved worst-case bound is 5.001 s and normal empty adds up to 1 ms firmware wait.
- Retry transport per character: no reconnect path; multiplies stalls. Permanent fail-fast instead.
- Latch every `-EIO`: rejected; endpoint line errors may recover after `try_read` consumes latch.
- Log output failures: recursive console deadlock/flood. Atomics only.
- Spinlock for ISR: holding across USB is worse. Drop/return before lock.
- Preserve local ring across configure/disconnect: crosses framing/stream generations. Clear it.
- Raw ordinal asserts: invalid for stop bits and detached from switches. Shared mapping expression.
- Capability gate removal: would make hw-rev1 output silently disappear. Accept second metadata RPC and escalate cache.
- New sample for CI: crosses M6. Existing reset source plus overlay suffices.
- Book chapter: contradicts AGENTS.md §15.1.

## 16. Open questions and escalations

No M5-blocking `crates/` change is required because answer A preserves RX-error recovery with timeout 1.

Formal follow-up escalation: cache full validated `DeviceInfo`/capabilities per handle in `pico-de-gallo-lib`, populate it during strict open/validate, and expose a cached FFI capability query. This removes the UART child's second metadata RPC, whose 300-second ceiling is derived from a `crates/` constant rather than observed. It requires `crates/` changes and is not part of M5.

A second optional future contract is an exported FFI enum variant count/sentinel so C can fail compilation when Rust appends a UART variant. Current M5 can only assert every current value; it cannot detect append without a generated symbol.

M6 must choose whether private UART diagnostics become a public driver-specific API, shell command, or remain debugger/test-only. M5 defines storage and semantics but does not widen public Zephyr APIs.
