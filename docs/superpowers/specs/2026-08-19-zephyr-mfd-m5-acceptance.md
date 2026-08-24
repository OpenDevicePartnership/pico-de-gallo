# Zephyr MFD restructure M5 — loopback integration and hardware acceptance

Date: 2026-08-19
Branch: `zephyr`, baseline `cfb7f4245ed3`
Milestone: M5 — runtime verification after standard `cs-gpios`
Related implementation: `0affe9206553`
Status: amended after architecture and reliability review

---

## 1. Context, scope and stop conditions

M1–M4 established the MFD topology, GPIO child and standard SPI `cs-gpios`, but
runtime behaviour remains largely unproved. M5 must independently establish:

1. the physical fixture is valid;
2. chip select asserts, holds and releases through GPIO 2↔3;
3. synchronous SPI data is bit-exact on the MOSI↔MISO short;
4. the sanctioned payload ceiling fails locally rather than at transport; and
5. the checked-deassert failure and process-local latch behave as specified.

The fixture is board `5256657D8A5D7F03`, hardware revision 2, attached to WSL by
usbipd. Windows cannot access it while attached. Every build and run is inside
WSL. One process may own USB and one build may run at a time.

### 1.1 Hard boundaries

- Nothing under `crates/`; no wire, firmware, version or lockfile change.
- No firmware flashing, `probe-rs`, GPIO interrupts, or `gallo_*` MCP call.
- No production-driver edit except `PDG_SPI_MAX_BUFFER` and its adjacent comment.
- Test applications may call existing C FFI entry points directly. They may not
  add, alter or emulate an FFI entry point.
- Any other driver defect is stop-and-report, never a silent repair or weakened
  expectation.
- Every hardware process is wall-clock bounded. Timeout is **infrastructure
  failure**, never a test result.
- Any abnormal hardware phase restarts through §9.4. Later phases never consume
  residue from an abnormal predecessor.

---

## 2. Architecture and complete file set

Normal acceptance is serialized:

```text
reset_subscriptions -> jumper_preflight -> acceptance -> spi_loopback -> recovery_teardown
```

The images share only the parent-identity DTS fragment. Their topology overlays
are deliberately different. A phase advances only after zero exit, its exact
success marker, and verified USB release.

### 2.1 Authorized M5 files

| Action | Path | Responsibility |
| --- | --- | --- |
| Create | `docs/superpowers/specs/2026-08-19-zephyr-mfd-m5-acceptance.md` | This specification. |
| Create | `zephyr/tests/pdg_mfd_m5/fixture-identity.dtsi` | Explicit lab-fixture serial only. |
| Create | `zephyr/tests/pdg_mfd_m5/common/m5_bottom.{h,c}` | Host-context shim: `reset_subscriptions`, `gpio_subscribe`, `gpio_unsubscribe`, `spi_get_config`. |
| Create | `zephyr/tests/pdg_mfd_m5/reset_subscriptions/{CMakeLists.txt,prj.conf,reset.overlay,src/main.c}` | Parent-only reset image. |
| Create | `zephyr/tests/pdg_mfd_m5/jumper_preflight/{CMakeLists.txt,prj.conf,jumper.overlay,src/main.c}` | Parent+GPIO image; SPI disabled. |
| Create | `zephyr/tests/pdg_mfd_m5/acceptance/{CMakeLists.txt,prj.conf,acceptance.overlay,src/main.c}` | Parent+GPIO+SPI behavioural acceptance, including a payload-before-only execution mode. |
| Create | `zephyr/tests/pdg_mfd_m5/recovery_teardown/{CMakeLists.txt,prj.conf,recovery.overlay,src/main.c}` | Parent+GPIO+SPI recovery and final-state report. |
| Create | `zephyr/tests/pdg_mfd_m5/run-m5.sh` | Serialized probes, bounded runners, restart handling and aggregate verdict. |
| Create | `zephyr/tests/pdg_mfd_m5/spi_loopback.overlay` | Upstream slow/fast children and CS 2. |
| Create | `zephyr/tests/pdg_mfd_m5/spi_loopback.conf` | Async off, 512-byte large buffer, measured latency scale. |
| Modify | `zephyr/drivers/spi/pdg_spi.c` | Constant and adjacent comment only. |
| Modify | `zephyr/drivers/spi/Kconfig` | Keep the additive heap default at 8192; correct the help text to the final ceiling plus allocator metadata/alignment margin. |
| Modify | `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml` | Both SPI ceiling statements. |
| Modify | `zephyr/README.md` | SPI ceiling row at current line 582. |
| Modify | `book/src/interfaces/spi.md` | Zephyr SPI error ceiling at current lines 212–214. |
| Modify | `zephyr/CHANGELOG.md` | Corrected ceiling and M5 acceptance evidence. |

Grep found other `4096` references in `book/`, but they describe the FFI,
batching, firmware buffers, or generic firmware APIs and remain valid. The I2C
constant and `zephyr/README.md:564` are also unchanged. Same-change parity for
the six Zephyr SPI files above is mandatory; M6 deferral is prohibited.

**Amendment (coordinator, during implementation): `common/m5_bottom.{h,c}`.**
`native_sim` splits into an embedded context (the applications' `src/main.c`) and
a host context (the native simulator runner). `pico_de_gallo.h` is on the host
include path only — `zephyr/CMakeLists.txt` adds it through
`target_compile_options(native_simulator INTERFACE "-I…")` — which is precisely
why every production driver has a `pdg_*_bottom.c`. An embedded translation unit
therefore cannot call `gallo_*` directly, so §1.1's "test applications may call
existing C FFI entry points directly" is unimplementable as written for the four
entry points that have no production bottom half: `gallo_system_reset_subscriptions`
(§3, §10 step 1), `gallo_gpio_subscribe` and `gallo_gpio_unsubscribe` (§6.2 steps
2, 5 and 9), and `gallo_spi_get_config` (§10 step 4). `gallo_gpio_set_config`
(§6.2 step 6) is unaffected: `pdg_gpio_bottom_set_config()` already exposes it.
The shim is wired per-application with `target_sources(native_simulator INTERFACE
…)`, the same mechanism `zephyr/drivers/CMakeLists.txt` already uses; it adds no
FFI entry point, alters none, emulates none, and modifies no production driver.

**Amendment (coordinator): tester escalation E2 needs no value change.**
`CONFIG_HEAP_MEM_POOL_ADD_SIZE_PDG_SPI` in `zephyr/drivers/spi/Kconfig` already
defaults to **8192**, not the 6144 E2 assumed; E2 read the upstream test's
default rather than this module's. The driver's two `k_malloc`s (`pdg_spi.c:457`,
`:464`) leave roughly 1.9 KiB for `sys_heap` metadata, the heap header and
alignment. Only the Kconfig **help text** is wrong — it still says "4096" and
"about 8 KiB" — and that is a §4 documentation-parity fix, not a configuration
change. Do not "fix" the value a second time.

### 2.2 Topology contracts

- **Reset:** parent `okay`; GPIO and SPI explicitly `disabled`.
- **Jumper:** parent and GPIO `okay`; SPI explicitly `disabled`.
- **Acceptance/recovery:** parent, GPIO and SPI `okay`; CS index 2.
- **Loopback:** same controllers plus `slow@0` and `fast@0` at selector 0.

Each build must assert generated devicetree status, not infer topology from
translation units. In particular, reset must prove both children disabled before
its executable may run.

### 2.3 Fixture identity policy

Commit serial `5256657D8A5D7F03`. It is lab-fixture metadata, not a credential.
Because `zephyr/module.yml:11-12` exports `zephyr/tests`, it **ships in clones and
source archives**. It is intentionally excluded only from portable samples,
which retain `REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL`. The identity fragment must
say in a comment: “M5 lab fixture only; do not copy this serial into user
overlays.”

### 2.4 No interrupt witness

Do not set `cs-loopback-gpios`. Current upstream `spi.c:140` guards the complete
witness implementation; omission selects no-op helpers at 242–246. Defining it
reaches interrupt configuration at 233 and callback registration at 240. No other
file under the suite's `src/` uses GPIO interrupts.

---

## 3. Reset sequencing and endpoint limits

The reset image is the resolution of the init-order conflict. Parent init performs
strict open before `main()` directly calls
`gallo_system_reset_subscriptions(ctx, &count)`. GPIO/SPI are absent, so monitored
pin 2 cannot fail SPI init first.

Required reset behaviour:

1. require parent readiness and a non-NULL borrowed context;
2. call reset exactly once and compare its return against the symbolic success
   enumerator from the **actual generated cbindgen header**;
3. print reset count and `M5_RESET_PASS` only after success;
4. any failed check terminates nonzero.

Do not write `Status_Ok`, `GalloStatus_Ok`, bare `0`, or another guessed spelling
in the specification-derived implementation. Inspect the generated header during
the build and use its symbolic enumerator. Current cbindgen configuration keeps
`Status` variants unprefixed and `common.c:19` uses `Ok`, but the generated header
is authoritative.

“Idempotent” is conditional: reset is safe when the firmware dispatcher can
service the endpoint. It cannot preempt an outstanding serial, zero-timeout
`gpio/wait-*`; such a wait wedges dispatch device-wide. Reset affects
**subscriptions only**. It does not reset pin modes, pulls, output levels, SPI
configuration, or a Zephyr process's lock/latch state.

Rejected alternatives remain: choosing pin 3 evades cleanup; a parent/GPIO/SPI
hook hides a destructive global mutation; ztest setup runs after SPI init; power
cycle does not exercise the required endpoint.

---

## 4. Payload ceiling — RESOLVED at 1013 bytes

**RESOLVED. The controlled experiment is CLOSED, and the ceiling is 1013.**

The before/after pair completed:

- **before**, at compiled `4096U`: a 4096-byte TX-only transfer returned
  `-ECOMM` (`-70`) from the **transport** — the call left the host;
- **after**, at compiled `1013U`: the same 4096-byte call returns `-EMSGSIZE`
  from the **local** check at `pdg_spi.c:438`, with the witness read HIGH
  immediately before and immediately after, which is direct electrical evidence
  that **no chip-select edge was issued and no bus transaction began**.

That closes the regression named in §12.1. The final ceiling is **1013 bytes**,
the largest length measured to work on hardware (TX-only), not 3072 and not
4096. Full duplex is proven to work at 512 (T5e); the duplex **ceiling** remains
unmeasured. See `PDG_SPI_MAX_BUFFER` in `pdg_spi.c` for the complete record of
what is and is not known, including the 1015-byte firmware hang that the
ceiling now puts out of reach.

**FALSIFIED BY EXECUTION — 3072 is also over the line.** Measured on board
`5256657D8A5D7F03`: a 3072-byte **full-duplex** transfer (3072 TX + 3072 RX)
passed the local check and failed at transport with `-ECOMM`, exactly the
failure mode this section was written to eliminate, merely relocated.

The asymmetry no prior pass accounted for is that the packet budget must cover
the **request** frame *and* the **response** frame. The §4.1 "before" control at
4096 was **TX-only**; T5a is full duplex. Every estimate to date — including the
`PacketBuffers<MAX_TRANSFER_SIZE + 1024>` reasoning below — considered a single
direction. Two failing data points (4096 TX-only, 3072 full duplex) do not
isolate the constraint: there is no measurement of 3072 TX-only, nor of anything
below 3072.

The requirement "make 3072 succeed" is therefore **withdrawn**. The ceiling must
be set from a measurement, not a third guess. The acceptance application gains a
normative `--ceiling-sweep` mode that binary-searches the largest working length
independently for the TX-only and full-duplex shapes, logs every probe with its
length and exact errno, verifies RX byte-exactness on every success, and reports
whether it was bounded by the transport or by the compiled constant. Only after
that sweep may `PDG_SPI_MAX_BUFFER` and `CONFIG_SPI_LARGE_BUFFER_SIZE` be set,
and `CONFIG_SPI_LARGE_BUFFER_SIZE` must sit strictly **below** the measured
full-duplex ceiling, since `spi.c:605-611` transfers it full duplex.

The rest of this section is retained for its framing rationale, which remains
correct; only the specific value 3072 is falsified.

**SUPERSEDED BY EXECUTION — the prescription below is historical.** It
instructed a 3072-byte constant; the shipped value is `1013U`. The
packet-buffer-budget *model* it states is correct and was retained; only the
number was wrong. Read the comment on `PDG_SPI_MAX_BUFFER` in `pdg_spi.c` for
the authoritative text. The original instruction was:

> Change only the existing comment and constant to the equivalent of:
>
> ```c
> /* The packet-buffer budget covers payload plus postcard-rpc header and COBS
>  * framing. Keep the usable payload strictly below that budget; 3072 bytes is a
>  * temporary conservative compatibility ceiling with deliberate framing margin.
>  */
> #define PDG_SPI_MAX_BUFFER 3072U
> ```

“Packet-buffer budget” is normative; “wire limit” is too loose. Firmware uses
`PacketBuffers<MAX_TRANSFER_SIZE + 1024>` at `firmware/src/main.rs:157-158`, and
the transfer request adds a varint plus postcard-rpc/COBS framing. That
1024-byte-margin reasoning was itself the trap: it considered a single
direction, and the budget must cover the request frame **and** the response
frame. 3072 was **not derived**, and neither is 1013 — 1013 is simply the
largest length *measured* to work.

M5 must:

**SUPERSEDED BY EXECUTION.** The two bullets below prescribed 3072/3073 and are
withdrawn; the live requirements are 1013 accepted TX-only and 1014 rejected
locally. Retained only to show what was asked for:

- ~~make 3072 succeed~~;
- ~~make 3073 return `-EMSGSIZE`~~ before allocation, lock, set-config, CS or transfer;
- update every parity file in §2.1; and
- file a follow-up issue before M5 closes.

**Required issue text:** derive the usable `spi/transfer` payload ceiling from the
worst-case request/response framing, expose one generated/shared contract, and
pin limit/limit+1 tests. Evaluate wire/schema and lockstep-release implications;
do not replace the measured value with another guessed round number.

### 4.1 Required before/after evidence and milestone ordering

The 4096 regression requires a genuine control, not a source mutation. Ordering
is normative:

1. The coder first lands/materialises every M5 test, runner, overlay, heap and
   documentation change **except** the `PDG_SPI_MAX_BUFFER` constant/comment and
   the final wording that claims the after-result. The working tree still has
   `4096U`.
2. Build the acceptance image from that tree and invoke its normative
   `--payload-before-only` mode. It runs only the 4096-byte case, emits
   `M5_PAYLOAD_BEFORE_RESULT=<errno>`, and performs no T2/T3/T4 work. The bounded
   runner captures merged output, executable status and errno in
   `/tmp/m5-payload-before.log`; copy its SHA-256 and verbatim result into the
   aggregate evidence. Expected result is `-ECOMM` (`-70`).
3. If the before result is anything other than `-ECOMM`, stop M5 for re-analysis.
   Do not explain it away and do not apply the fix: the sanctioned root-cause
   premise has not reproduced.
4. Only after a valid before reading does the coder change the comment/constant to
   the measured ceiling, finish parity wording, rebuild, and run tester
   T5a–T5e: 1013 accepted TX-only; 1014 rejected; 4096 rejected (T5c);
   accumulated `512 + 502` rejected; and 512 duplex accepted as a shape check.
   Capture `/tmp/m5-payload-after.log`, SHA-256 and verbatim results. T5c must
   return `-EMSGSIZE`, include the compiled-ceiling warning, and retain the known
   HIGH state unchanged across the rejected call.
5. Report the pair as `payload_boundary.before_4096` and
   `payload_boundary.after_4096`; neither alone satisfies the gate.

This is the sole authoritative before/after description. It supersedes the
historical shorthand in §12.1; tester T5c is the after half, while this section
adds its mandatory pre-fix control.
---

## 5. Fixture and data-path preflight

Every application check returns nonzero immediately on failure. Each application
prints exactly one final success marker only after all of its checks pass:

- `M5_RESET_PASS`
- `M5_JUMPER_PASS`
- `M5_ACCEPTANCE_PASS`
- `M5_TEARDOWN_PASS`

The runner requires both zero exit and the marker. Logging a failure and returning
zero is prohibited.

### 5.1 Physical jumper proof

The GPIO-only image runs after reset:

1. pin 2 input/pull-up;
2. pin 3 output LOW;
3. pin 2 must read LOW against its own pull-up — only the fitted 2↔3 jumper and
   active drive can produce this;
4. while the node is forced LOW, change pin 3 to input/pull-down, then pin 2 to
   input/pull-down and require both LOW.

Step 4 is **not a second jumper proof**. It validates the RP2350 pre-charge/hold
baseline used by later electrical reasoning: pull-down holds a node already LOW,
whereas a floating node drifts HIGH and a pull-down cannot reliably pull an
already-HIGH node LOW.

The image performs no rollback. Any abnormal or failed jumper phase restarts at
process 1; the next jumper phase explicitly establishes its own initial modes.
If dispatch is unresponsive, power-cycle first, then restart at process 1.

### 5.2 Empirical MOSI↔MISO semantics

Acceptance sends a non-palindromic, non-uniform pattern such as:

```c
{ 0x96, 0x2d, 0xe1, 0x4b, 0x73 }
```

for modes 0–3, with RX poisoned to `0x3C` as specified by test-design §4.2. It
classifies whole-byte lag, one-bit left/right shifts with cross-byte carry, bit
reversal, stuck-at values, and poison intact. Exact echo passes. A deterministic
shift is a mode/fixture limitation and makes M5 **INCONCLUSIVE**, not pass. Any
other mismatch or transport errno is failure.

The MOSI↔MISO short is mode-blind: this sweep proves only that modes 0–3 are
accepted and byte-exact. It does **not** prove CPOL/CPHA wire mapping.

---

## 6. Chip-select lifecycle and test-application fault induction

CS is firmware GPIO 2, active LOW. GPIO 3 is the independent pull-up witness over
the fitted jumper. The acceptance process must not reconfigure pin 2 before SPI
use; SPI init parks it inactive HIGH.

### 6.1 Normal HOLD+LOCK lifecycle

1. configure witness 3 input/pull-up and record the pre-call HIGH baseline;
2. transfer exact-echo data with one stable config containing
   `SPI_HOLD_ON_CS | SPI_LOCK_ON`;
3. require return 0 and witness LOW — the strong assertion evidence;
4. `spi_release()` with the same config address returns 0;
5. require the witness **LOW→HIGH transition across steps 3–5, with both readings
   in this process**; standalone HIGH is necessary but never sufficient because
   high-Z, monitoring or a missing jumper can also read HIGH;
6. second release returns `-EINVAL`;
7. ordinary transfer with a different config succeeds;
8. HOLD without LOCK returns `-ENOTSUP`; assert the already-established HIGH is
   unchanged across the call, not an isolated HIGH reading.

### 6.2 Definitive zero-driver-change fault route: **YES**

The existing C FFI exports `gallo_gpio_subscribe(ctx, pin, edge)` and
`gallo_gpio_unsubscribe(ctx, pin)` (`ffi/src/lib.rs:2067-2141`). Firmware subscribe
takes the pin from `Context` and the monitor task sets it input
(`firmware/main.rs:203-211`). This is sufficient to induce a checked deassert
failure from test application code:

1. perform HOLD+LOCK on CS 2; witness is LOW;
2. directly call `gallo_gpio_subscribe(ctx, 2, <Any>)`; require symbolic success;
   the monitor now owns pin 2 and changes it to input;
3. call `spi_release()` with the retained config; its `gpio_pin_set_dt()` must
   return `-EBUSY`, the driver must return `-EBUSY`, release software ownership,
   retain the config, and set its process-local fault latch;
4. call another `spi_transceive()`; require `-EHOSTDOWN`;
5. directly unsubscribe pin 2; require symbolic success;
6. because the monitor changed the physical pad to input while firmware still
   tracks `ExplicitOutput`, directly call existing `gallo_gpio_set_config()` to
   restore pin 2 as output; require symbolic success;
7. retry `spi_release()` with the retained config; require 0, latch clear, and
   require the witness LOW→HIGH transition whose LOW was observed at step 1 in
   this same process; post-recovery HIGH alone is not deassert proof;
8. an ordinary transfer then succeeds.

This behaviourally proves D10's checked-deassert error propagation, latch entry,
subsequent `-EHOSTDOWN`, failed-release software unlock, and successful-release
recovery—without a driver edit. It does **not** prove that the `-EHOSTDOWN` branch
issued no invisible RPC, preserve two distinct first errnos, or count duplicate
identical GPIO writes.

The latch is process RAM and vanishes on process death. Physical firmware GPIO
mode/level survives. A process killed during HOLD may leave CS 2
`ExplicitOutput` LOW even though the next process has no latch or owner record.
Therefore loopback may never follow an abnormal acceptance exit.

---

## 7. Upstream `spi_loopback`

### 7.1 Configuration

`CONFIG_SPI_ASYNC=n` is mandatory: `pdg_spi.c:58-60` rejects async because the API
slot is absent. Async tests are not built.

Set `CONFIG_SPI_LARGE_BUFFER_SIZE=512`. `spi.c:605-611` transfers it **full
duplex**, and the duplex ceiling has never been measured, so it must sit well
below the measured TX-only ceiling of 1013 rather than near it. Keep the
production additive heap default at **8192**, which covers this many times over. An accidental
second-allocation `-ENOMEM` would fail `large_transfers` as a false data-path
defect because upstream converts only `-EINVAL`/`-ENOTSUP` to skip. Kconfig help
must preserve this allocator margin explicitly.

**Timing: RESOLVED on hardware.** The host-clock path is validated — SLOW p50
2322 µs, FAST p50 1729 µs, selected multiplier **47**, inside the required
1–256 band with FAST binding. Timing is a real measurement on this target. The
`NOT_MEASURABLE` path below is retained as a fallback, not as the expected
outcome.

**Timing is NON-GATING, and the clock source is normative.** On `native_sim`
Zephyr's clock measures *simulated* time, which does not advance while the host
thread is blocked inside a USB call — and every operation here is exactly such a
call. Measured with `k_cycle_get_32()`: p50 = p95 = p99 = max = **0 µs** across
25 real, correctly-echoing transfers at each of two frequencies, driving the
derived multiplier to `ceil(1.25 × 0 / t) = 0` and aborting the run before
loopback. Elapsed time must therefore be taken from the **host** monotonic
clock. If a usable multiplier still cannot be derived, timing is reported
`NOT_MEASURABLE` with a documented fixed fallback multiplier, the run continues,
and the milestone is at worst INCONCLUSIVE — never FAIL, and never a large
multiplier presented as measured. The same limitation makes upstream's
`test_spi_complete_multiple_timed` pass vacuously on this target; that is
recorded in `explicitly_untested`, not counted as coverage.

Do not guess the timing multiplier. The overlay declares exact
`spi-max-frequency` literals for `slow@0` and `fast@0`; acceptance uses those same
literals in its `spi_config.frequency` fields. Measure at least 20 healthy 54-byte
ordinary transfers at each exact frequency, recording p50, p95, p99 and maximum
separately. Compute each required
multiplier as:

```text
ceil((1.25 * observed_max_us) / theoretical_minimum_us)
```

Choose the smallest integer satisfying both modes, normally expected in 64–128.
The upstream suite has one global value, so preserve both measurements and use
their maximum. The ratio is USB-latency dominated: wall time is roughly four USB
round trips while theoretical time shrinks as frequency rises. If FAST requires
a multiplier above 256, lower the `fast@0` frequency in the overlay, remeasure at
that exact literal, and select the smallest stable multiplier. Never exceed 256;
a larger multiplier recreates a vacuous timing test.

### 7.2 Expected case ledger, re-derived from current `spi.c`

The `spi_loopback` suite runs twice, SLOW then FAST. Require log markers
`Testing loopback spec: SLOW` **and** `Testing loopback spec: FAST`.

| Cases | Verdict | Reason |
| --- | --- | --- |
| complete multiple; modes 0–3; null TX; RX prefix/suffix/every-4; RX>TX; 512 large; null TX/RX sets; zero-length; write-back; same-buffer command | PASS ×2 | Supported 8-bit synchronous flatten/transfer/unflatten semantics. |
| timed | **FAIL ×1 (expected)** | Upstream lower-bound assert against the simulated Zephyr clock; no multiplier value can affect it. Known-unrunnable on `native_sim`, not a driver defect — see test design §8.6. |
| word sizes 7, 9, 16, 24, 32 | SKIP ×2 | Driver returns `-ENOTSUP`; common wrapper skips invalid configuration. |
| concurrent same-config and distinct-config | PASS ×2 | Controller lock serializes callers. |
| deinit | SKIP ×2 | `spi.c:913-916` skips immediately because `zephyr,user` lacks `miso-gpios`/`mosi-gpios`; `device_deinit()` is **not exercised**. |
| async signal and callback | NOT BUILT | `CONFIG_SPI_ASYNC=n`. |
| lock/release | PASS ×2 | LOCK then release permits FAST transfer; bespoke §6 provides electrical CS evidence. |
| HOLD generic case | SKIP ×2 | Upstream sets HOLD without LOCK and converts PDG `-ENOTSUP` to skip; bespoke §6 tests the supported pair. |

Both suites iterate twice because `spi.c:1210` passes `suite_iter =
ARRAY_SIZE(loopback_specs) = 2`. `spi_extra_api_features` does not consult
`spec_idx`, so its second iteration repeats its hard-coded SLOW/FAST operations.
**Observed totals are 41 PASS, 12 SKIP, 1 FAIL, 2 NOT BUILT.** The earlier
"26 PASS, 12 SKIP, 2 NOT BUILT" was source-derived and wrong — it counted test
functions rather than the results ztest reports, and ignored ztest re-attempting
a failing test. The single FAIL is the expected upstream timed case. **The SKIP
count of 12 is exact and is the anti-vacuity check**; the runner asserts it and
fails the phase on any other value. Any other deviation stops M5.
Loopback success never substitutes for CS witness evidence.

---

## 8. Four-property assurance ledger

**Correction to plan §11.4/§11.5:** M5 does not make all four properties cease to
be claims. Current scope permits behavioural progress on latch/error propagation,
but three internal absence/identity properties and the non-returning row remain
below behavioural assurance. The plan must absorb this boundary explicitly.

| Property | What proof requires | M5 action and reason | Remaining assurance |
| --- | --- | --- | --- |
| First-errno preservation | Two deassert failures with distinct selectable errnos while latched. | Direct subscribe induces only `-EBUSY`; no FFI endpoint injects a chosen GPIO errno. Driver fault shim is prohibited. | Source-shape only; **not behaviourally proved**. |
| No second GPIO edge | Count calls because a duplicate deassert is electrically identical. | Requires `pdg_gpio_bottom_put` instrumentation; prohibited driver edit. | Source-shape only; **not behaviourally proved**. |
| `-EHOSTDOWN` before any I/O | Create latch, then count set-config/GPIO/transfer calls. | §6.2 proves returned `-EHOSTDOWN`, but no application-visible FFI counter exists and witness cannot prove absence. | Return/latch behaviour proved; “before any I/O” remains source-shape only. |
| Non-returning-RPC row | Deliberately create a call that cannot return, then externally contain/recover it. | Not induced: destructive, non-terminating and indistinguishable from infrastructure loss inside one process. | Documented failure boundary only; **untested**. |

No driver-source instrumentation or mutation control is built. Mutation evidence
would be valuable, but source mutation is prohibited and could survive into a
commit. Selectable faults would require a test-only FFI entry point that makes the
Nth GPIO put return a chosen errno; absence proofs need an FFI-visible operation
counter. Neither exists. M5 also does not deliberately induce the
HOLD+LOCK-versus-different-config deadlock from plan §11.1: it is unbounded and
bench-destructive for the same reason as the non-returning-RPC row.

---

## 9. Execution, containment and recovery

### 9.1 Pre-phase infrastructure probe

Before every phase, without claiming USB:

1. require exactly one `045e:067d` in `lsusb` and resolve its `/dev/bus/usb` node;
2. require the node readable and writable by the non-root executor;
3. require `fuser`/`lsof` to report no owner from a previous phase;
4. capture merged stdout/stderr because registry/FFI cause is printed on host
   stderr while Zephyr may collapse board-absent and board-busy to `-ENODEV`.

Enumeration/permission/ownership failures are `INFRASTRUCTURE`, not test failure.
At most one retry is allowed after a logged two-second release wait and repeated
probe. A second failure aborts; blind retry loops are prohibited.

### 9.2 Bounded runner and exit integrity

Every phase uses a wrapper equivalent to:

```bash
set -o pipefail
log=/tmp/m5_<phase>.run.log
timeout --signal=TERM --kill-after=10s 420s /tmp/m5_<phase>/zephyr/zephyr.exe 2>&1 | tee "$log"
rc=${PIPESTATUS[0]}
if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then
    if [ "$phase" = acceptance ] &&
       grep -qx 'M5_T4_STEP3_RELEASE=-EBUSY' "$log" &&
       ! grep -q '^M5_T4_STEP4_RESULT=' "$log"; then
        echo 'M5_PHASE_VERDICT=FAIL_LOCK_LEAK'
        exit 1
    fi
    echo 'M5_PHASE_VERDICT=INFRASTRUCTURE_TIMEOUT'
    exit 2
fi
if [ "$rc" -ne 0 ] || ! grep -qx 'M5_<PHASE>_PASS' "$log"; then
    echo 'M5_PHASE_VERDICT=FAIL'
    exit 1
fi
echo 'M5_PHASE_VERDICT=PASS'
```

`420s` covers one 300-second strict-open bound plus execution margin. TERM gets a
10-second grace, then KILL. The wrapper preserves status through `tee` and aborts
on nonzero. The acceptance app flushes the exact step-3 marker after observing
`-EBUSY` and prints step 4's result marker immediately on return. A timeout with
step 3 present and step 4 absent is mechanically **FAIL: possible controller lock
leak (plan §11.1)**, is never retried, and sets `fault_latch=FAIL`. Other timeouts
remain infrastructure and follow the one bounded retry rule.

The upstream suite has no custom marker; require zero executable status, both
SLOW/FAST markers, and the expected ztest summary. Its wrapper prints
`M5_LOOPBACK_PASS` only after checking all three.

### 9.3 Exact builds

All commands retain plan §4's environment. At most one is active:

```powershell
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/m5_reset -b native_sim/native/64 zephyr/tests/pdg_mfd_m5/reset_subscriptions -- -DEXTRA_ZEPHYR_MODULES=/mnt/d/workspace/pico-de-gallo -DSHIELD=pico_de_gallo -DDTC_OVERLAY_FILE=/mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_mfd_m5/reset_subscriptions/reset.overlay'
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/m5_jumper -b native_sim/native/64 zephyr/tests/pdg_mfd_m5/jumper_preflight -- -DEXTRA_ZEPHYR_MODULES=/mnt/d/workspace/pico-de-gallo -DSHIELD=pico_de_gallo -DDTC_OVERLAY_FILE=/mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_mfd_m5/jumper_preflight/jumper.overlay'
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/m5_acceptance -b native_sim/native/64 zephyr/tests/pdg_mfd_m5/acceptance -- -DEXTRA_ZEPHYR_MODULES=/mnt/d/workspace/pico-de-gallo -DSHIELD=pico_de_gallo -DDTC_OVERLAY_FILE=/mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_mfd_m5/acceptance/acceptance.overlay'
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/m5_spi_loopback -b native_sim/native/64 ~/zephyrproject/zephyr/tests/drivers/spi/spi_loopback -- -DEXTRA_ZEPHYR_MODULES=/mnt/d/workspace/pico-de-gallo -DSHIELD=pico_de_gallo -DDTC_OVERLAY_FILE=/mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_mfd_m5/spi_loopback.overlay -DEXTRA_CONF_FILE=/mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_mfd_m5/spi_loopback.conf\;/tmp/m5-measured.conf'
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/m5_teardown -b native_sim/native/64 zephyr/tests/pdg_mfd_m5/recovery_teardown -- -DEXTRA_ZEPHYR_MODULES=/mnt/d/workspace/pico-de-gallo -DSHIELD=pico_de_gallo -DDTC_OVERLAY_FILE=/mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_mfd_m5/recovery_teardown/recovery.overlay'
```

The loopback build occurs after acceptance records latency. The executor generates
`/tmp/m5-measured.conf` containing only the selected multiplier and passes it
after committed `spi_loopback.conf`; execution never rewrites a tracked config.

### 9.4 Restart and physical-residue rule

Any abnormal reset, jumper, acceptance, loopback or teardown phase restarts at
**process 1**. If the dispatcher does not answer reset within the process bound,
power-cycle first and record that fact.

- Failed jumper preflight leaves pin modes/pulls mutated with no rollback: restart
  at reset, then rerun jumper from its explicit initial configuration.
- Abnormal acceptance or loopback may leave physical CS LOW. Never start loopback
  after abnormal acceptance. Run reset, then the dedicated recovery process;
  recovery requires SPI init ready and witness HIGH. Only then restart the full
  sequence from reset.
- Process-local latch/lock/owner vanish on death; physical firmware pin state does
  not. Never infer recovery from a fresh process lacking the latch.

### 9.5 Non-vacuity and bottom-half proof

After each build, assert `.config`, generated statuses and required TUs with the
safe digit-aware pattern. For native-simulator bottom halves, which do not appear
in `compile_commands.json`, require their object files and `nm` symbols:

- reset: MFD/common bottom only;
- jumper: GPIO and common bottoms;
- acceptance/recovery: GPIO, SPI and common bottoms;
- loopback: SPI, GPIO and common bottoms.

Loopback additionally requires `CONFIG_SPI_ASYNC` unset,
`CONFIG_SPI_LARGE_BUFFER_SIZE=512`, the measured timing multiplier, serial,
CS 2, and both test compatibles.

---

## 10. Teardown, state report and aggregate verdict

The normal final recovery process:

1. strict-opens, explicitly resets subscriptions and reports count;
2. requires SPI and GPIO ready (SPI init parks CS 2 inactive HIGH);
3. configures witness 3 input/pull-up and requires HIGH;
4. queries actual SPI mode/frequency through existing `gallo_spi_get_config()`;
5. reports pin 2 as `ExplicitOutput/HIGH` based on acknowledged SPI init plus
   independent witness, and pin 3 as `Input/PullUp` based on acknowledged setup;
6. records whether any power cycle occurred; and
7. requires operator attestation that MOSI↔MISO and GPIO 2↔3 jumpers remain fitted.

There is no GPIO mode-query FFI, so the report must distinguish **acknowledged
commanded mode** from directly queried state. Missing state is `unknown`, never a
guess. Teardown emits `M5_TEARDOWN_PASS` only after the complete report.

The executor writes this aggregate verdict verbatim in JSON shape:

```json
{
  "milestone": "M5",
  "fixture_validity": "PASS|FAIL|INFRASTRUCTURE|INCONCLUSIVE",
  "cs_lifecycle": "PASS|FAIL|INFRASTRUCTURE|INCONCLUSIVE",
  "spi_data_path": "PASS|FAIL|INFRASTRUCTURE|INCONCLUSIVE",
  "timing": {
X
    "slow_p50_us": 0,
    "slow_p95_us": 0,
    "slow_p99_us": 0,
    "slow_max_us": 0,
    "fast_p50_us": 0,
    "fast_p95_us": 0,
    "fast_p99_us": 0,
    "fast_max_us": 0,
    "slow_frequency_hz": 0,
    "fast_frequency_hz": 0,
    "selected_multiplier": 0
  },
  "payload_boundary": {
X
    "before_4096": "-ECOMM|-70|other|not-run",
    "before_log_sha256": "",
    "after_4096": "-EMSGSIZE|other|not-run",
    "after_log_sha256": ""
  },
  "fault_latch": "PASS|FAIL|INFRASTRUCTURE|INCONCLUSIVE",
  "explicitly_untested": [
    "first_errno_preservation_with_distinct_errnos",
    "no_second_gpio_put",
    "ehostdown_before_any_io",
    "non_returning_rpc_residue",
    "cpol_cpha_wire_mapping",
    "driver_source_mutation_controls",
    "hold_lock_different_config_deadlock",
    "upstream_timed_transfer_bounds_on_native_sim",
    "spi_transfer_duration_when_timing_not_measurable",
    "spi_full_duplex_payload_ceiling",
    "payload_ceiling_boundary_between_1013_and_1015",
    "firmware_dispatcher_hang_at_1015_byte_transfer"
  ],
  "teardown": {
    "subscriptions_reset": 0,
    "pin2_mode": "acknowledged ExplicitOutput|unknown",
    "pin2_level": "witnessed HIGH|LOW|unknown",
    "pin3_mode_pull": "acknowledged Input/PullUp|unknown",
    "spi_mode": "mode0|mode1|mode2|mode3|unknown",
    "spi_frequency_hz": 0,
    "power_cycle_occurred": false,
    "mosi_miso_jumper_fitted": true,
    "gpio2_gpio3_jumper_fitted": true
  },
  "overall": "PASS|FAIL|INFRASTRUCTURE|INCONCLUSIVE"
}
```

Overall is PASS only when fixture, CS lifecycle, data path, timing, boundary,
fault latch and teardown all pass. Missing, shifted, contradictory or
inconclusive CS witness evidence makes overall **INCONCLUSIVE**, even if every
loopback byte matches. Any infrastructure phase makes overall INFRASTRUCTURE.
The explicitly untested list is mandatory and cannot be empty for M5.

---

## 11. Repository verification and file hygiene

### 11.0 Evidence provenance — read before trusting the aggregate verdict

The M5 evidence was not all produced by one build of the tree being shipped.
Stated explicitly so nobody over-reads it:

- **The phase-final evidence for the loopback and timing phases predates the
  symbolic-errno formatting change.** At the time those phases ran, the C apps
  printed bare numeric errnos (`M5_T4_STEP4_RESULT=-112`) and the runner matched
  numeric literals that were wrong for the host libc.
- **That change is output-format and matching only.** The apps emit
  `sym=<NAME>` alongside the same numeric value, and the runner matches the
  symbol. No control flow, no assertion, no transfer length and no devicetree
  changed. The driver behaviour under test is byte-for-byte unaffected.
- **The runner's symbolic matching, ledger check and expected-failure list have
  never executed.** They were written from reported output. They are shell, so
  `bash -n` is the only static checking available.
- **A single acceptance-phase re-run validates the format.** That is the
  proportionate scope: it exercises the new `sym=` lines and the payload/fault
  verdict paths. The loopback ledger logic remains exercised only by whatever
  full run follows.

Any deviation observed in that re-run should be suspected of being a
`grep` pattern defect in `run-m5.sh` before it is treated as a driver defect.

```powershell
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && cargo test --workspace --locked'
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && mdbook build book'
```

Also run `git diff --check`, verify no path under `crates/`, and verify the `4096`
grep disposition in §2.1. Run `dos2unix` only on files the implementer created or
modified in M5, before staging; never mutate unrelated tracked files.

---

## 12. Prior-document corrections

Distinguish factual errors from later superseding decisions:

### 12.1 Factually wrong or stale

- Design §4.3 says the parent directly calls `pdg_registry_open`; current
  `pdg_mfd.c:76` correctly uses `pdg_common_bottom_open`.
- Design §4.4 suggests get-then-set toggle; `pdg_gpio.c:510-545` correctly rejects
  it because output readback is unavailable.
- Design/plan cite stale `gpio.h:933` for pin configure; plan §10.4 records the
  correction.
- Design §5 says `3+` RPCs; an ordinary successful transfer uses four.
- M4 acceptance C-18 calls 4096 a local boundary; observed 4096 reaches transport (CLOSED: with the ceiling at 1013U, 4096 is now rejected locally with -EMSGSIZE and the witness proves no chip-select edge -- see §4)
  and fails `-ECOMM`.
- ROADMAP §1.6 treats 4096 as generally usable payload rather than packet-buffer
  budget. The required follow-up issue must correct that framing.

### 12.2 Superseded by later decisions

- Design §7.2's `cs-loopback-gpios` witness is superseded by D5 and plan §10.1;
  omit it because current upstream implementation requires interrupts.
- Design D9/plan §10.2's HOLD-alone sequence is superseded by M4's required
  HOLD+LOCK pairing.
- Plan R7's power-cycle-only cleanup is superseded by the explicit reset endpoint,
  subject to §3's dispatcher-serviceability precondition.
- Design §6's “sole writer” wording is superseded by M4 §11.3: GPIO is the sole
  driver path, while applications still owe exclusive ownership.
- Plan §11.5's expectation that all four hard properties become behavioural in
  M5 is superseded by the hard no-instrumentation boundary and §8's ledger.

---

## 13. Failure domains and alternatives

- Board absent/busy/permissions: infrastructure; no actuation evidence.
- Reset timeout: possibly wedged dispatcher; power-cycle and restart at reset.
- Jumper failure: fixture invalid; measurements void.
- Abnormal HOLD process: physical CS may remain LOW; use recovery rule.
- Loopback-only success: data evidence only, never CS evidence.

Rejected: upstream suite alone (no explicit reset or supported CS witness), a
single executable (SPI init ordering conflict), generated `/tmp` identity
(unreviewed target), real serial in samples (non-portable), and driver counters or
fault shims (explicitly prohibited).

No unresolved ruling blocks implementation. The framing-overhead follow-up issue
is a required M5 deliverable, not an optional open question.
