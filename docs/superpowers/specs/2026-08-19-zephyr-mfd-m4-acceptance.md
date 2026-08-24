# Zephyr MFD restructure M4 — adversarial probe suite and acceptance specification

Date: 2026-08-19
Branch: `zephyr`, baseline `8147e207efd2`
Milestone: M4 — delete `cs-gpio-indices`, adopt standard `cs-gpios`
Source of truth: `docs/superpowers/specs/2026-08-19-zephyr-mfd-m4-cs-gpios.md`

Written black-box against the specification. **No M4 implementation was read;
none exists.** The pre-M4 `zephyr/drivers/spi/pdg_spi.c` was read only to
establish what the current tree does *not* enforce, which is what makes the
Class A probes below non-vacuous.

---

## 0. Purpose and assurance boundary — read this first

M4 is a **compile-time-only** milestone (spec §1, §12 invariant 14). Nothing on
this branch has ever executed; M5 is the first milestone that runs anything.
There is no `zephyr/tests/` directory, and M2 resolved "invent a test framework"
as out of inventory. This suite is therefore a **probe and acceptance design**,
not a harness: overlays and fragments are materialised into `/tmp` at execution
time and nothing is added to the repository.

Every probe carries exactly one class:

| Class | Meaning | Strength |
| --- | --- | --- |
| **A** | Compile-time provable. A build that must succeed with verifiable object/symbol/config evidence, or must fail with a **named** diagnostic substring. | Real gate. |
| **B** | Source-shape only. A `grep`/`rg`/offset assertion that a required construct exists or a forbidden one does not. | Catches deletion and rot. **Blind to present-but-wrong.** |
| **C** | Requires execution. **Zero assurance in M4.** Written as an executable-in-M5 design so M5 inherits it rather than reinventing it. | None, in M4. |

**No Class C case is disguised as A or B.** Where a runtime property has a
structural shadow, the shadow is listed as B and the behaviour is *separately*
listed as C. Passing the B probe is not evidence for the C case.

### 0.1 Precedent this suite is bound by

- **M1 T10** was criticised for grepping a bare word that an under-documented
  binding satisfied by luck (plan §8.3). Every Class B probe here asserts on
  **structure with positional constraints** — function extents, byte offsets,
  exact counts — never a lone token in prose.
- **Plan §9.1**: `pdg_[a-z_]*\.c` can never match `pdg_i2c.c`; the digitless
  pattern produced a false negative that made an earlier milestone's TU check
  meaningless. Every TU grep here uses **`pdg_[a-z0-9_]*\.c`**.
- **Plan §9.2**: ordinals are never compared as literal strings. M4 renumbers
  them, and enabling `pdg_gpio0` in three overlays shifts them further. Compare
  **symbol + resolved node path + count**, resolved from *each build's own*
  `devicetree_generated.h`.
- **#104 acceptance** confirmed its suite by re-introducing the bug and watching
  3 of 7 tests fail. §5 specifies the same mutation controls here; §3 goes one
  better and records the **pre-M4 tree itself as the confirmed RED baseline**
  for the two devicetree gates.

### 0.2 Hardware and safety constraints binding this suite

- **No `gallo_*` MCP tool may be invoked** (plan R1). Real hardware is attached;
  a mismatched call hangs forever holding the exclusive WinUSB interface.
- **No binary may be executed** — no `probe-rs`, no `cargo run -p gallo`, no
  built sample. Building is safe; running is not.
- **One build at a time**, `-d /tmp/m4_test_<name>`, deleted by its owner.
  `/tmp` is a 16 GB tmpfs at ~200 MB per build.
- Probe overlays live in `/tmp`, never in the repository: `zephyr/.gitignore`
  covers only `zephyr/build/`.

### 0.3 Traps that make a probe pass against broken code

Recorded here because three of them would have silently invalidated probes in
this suite had they not been checked.

1. **`port_get_raw` returns bit 0 for an output pin by design** (M3 §6.2,
   mirroring `gpio_emul.c:525`). **Never verify a CS edge by reading back your
   own output pin through the same controller.** It will confidently report the
   wrong thing. Every C-class CS-edge probe below uses a *witness pin* or an
   instrumented call counter, never a readback.
2. **`gpio/get` on a `LegacyAuto` pin is not state-neutral** — it calls
   `set_as_input()`. A read is not a query. This also means a "did CS move?"
   probe can itself change the pin's mode.
3. **RP2350 pull-downs cannot pull an already-high node low** (plan R2,
   measured), and a floating pad drifts high within seconds. Any witness probe
   that sets a pull-down and expects LOW *without first forcing the node low*
   is invalid and **passes against broken code**. Pre-drive low, release to
   pull-down, verify the baseline — or use a pull-up and invert.
4. **`spi/batch` deletion cannot be checked via `compile_commands.json`.**
   `pdg_spi_bottom.c` is built by the native-simulator Makefile and **never
   appears** there. Verify by object file and `nm` (A-08).

### 0.4 Confirmed baseline relied on, not re-measured

| Fact | Value |
| --- | --- |
| `i2c_bridge` | clean, 116/116. TUs `pdg_i2c.c`, `pdg_mfd.c` |
| `spi_nor_id` | clean, 117/117. TUs `pdg_mfd.c`, `pdg_spi.c` |
| `spi_bridge` | `zephyr.elf` links clean; native-simulator runner fails on exactly **1** undefined `__device_dts_ord_49` in `.text.main` → `/pico-de-gallo/spi/is31fl3743b@0` (`issi,is31fl3743b`) |
| `combined_i2c_spi_bridge` | same, `__device_dts_ord_50`, in `.text.spi_worker`, same resolved node |
| Priorities | `CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY=40`, `CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY=45`; no PDG SPI priority symbol exists yet |
| `cargo test --workspace --locked` | 561 passed, 0 failed, 7 ignored |
| `pdg_spi_bottom.o` symbols | `pdg_spi_bottom_batch`, `_close`, `_num_gpios`, `_open`, `_set_config` |

A post-M4 failure at the `Linking C executable zephyr/zephyr.elf` step is a
genuinely **new** failure class, not R5.

### 0.5 The build command (do not substitute)

Plain `native_sim` is 32-bit; `zephyr/Kconfig:6` has `depends on 64BIT`, so
`CONFIG_PICO_DE_GALLO=n` and the whole module is elided. Every gate run that way
is vacuous.

```bash
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/m4_test_<NAME> -b native_sim/native/64 zephyr/samples/<SAMPLE> -- -DEXTRA_DTC_OVERLAY_FILE=/tmp/<FILE>.overlay'
```

From PowerShell use **single** quotes around the bash string. Capture as
`> /tmp/m4_<ID>.log 2>&1; echo "EXIT=$?"`.

**A probe that passes on any nonzero exit is worthless.** Two samples in this
tree already fail for unrelated pre-existing reasons. Every negative probe below
therefore requires a **specific diagnostic substring**, and most additionally
require the *absence* of a substring or of `compile_commands.json`.

---

## 1. Probe table

`SRC=zephyr/drivers/spi/pdg_spi.c`, `BOT=zephyr/drivers/spi/pdg_spi_bottom.c`,
`BOTH=zephyr/drivers/spi/pdg_spi_bottom.h`,
`BIND=zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml`.

`extent <file> <fn>` and `off <file> <regex>` are the M3 helpers
(`2026-08-17-zephyr-mfd-m3-gpio-tests.md` §2), reused verbatim. **`extent`
failing to find a function is a probe FAILURE, not a skip** — that is the
anti-vacuity guard for the entire Class B set.

### 1.1 Class A — compile-time gates

| ID | Property under test | Mechanism | Expected observable |
| --- | --- | --- | --- |
| **A-01** | Positive control / non-vacuity: the module is compiled and both drivers are embedded | `spi_nor_id` post-M4, unmodified sample | `EXIT=0`; `CONFIG_PICO_DE_GALLO=y`, `CONFIG_MFD_PICO_DE_GALLO=y`, `CONFIG_SPI_PICO_DE_GALLO=y`, `CONFIG_GPIO_PICO_DE_GALLO=y`, `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY=50`, `CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY=45`; `grep -o 'pdg_[a-z0-9_]*\.c' <b>/compile_commands.json \| sort -u` contains **both `pdg_spi.c` and `pdg_gpio.c`** plus `pdg_mfd.c` |
| **A-02** | `cs-gpios` on a **foreign** (non-PDG) controller is rejected at compile time | `spi_nor_id` + `/tmp/m4_foreign_cs.overlay` (`cs-gpios = <&gpio0 0 GPIO_ACTIVE_LOW>`, `gpio0` = native_sim `zephyr,gpio-emul`) | `EXIT != 0`; log contains the registered `DT_NODE_HAS_COMPAT` diagnostic naming `odp,pico-de-gallo-gpio` **and the CS array index**; log does **not** contain `undefined reference to \`__device_dts_ord_` |
| **A-03** | `cs-gpios` on a valid, enabled PDG GPIO under a **different MFD parent** is rejected | `spi_nor_id` + `/tmp/m4_crossparent_cs.overlay` (second `odp,pico-de-gallo` node `pdg1` with its own enabled `pdg_gpio1`) | `EXIT != 0`; log contains the registered `DT_SAME_NODE(DT_PARENT(ctlr), DT_INST_PARENT(inst))` diagnostic **and the CS array index**; log does **not** contain the A-02 compatible diagnostic |
| **A-04** | `cs-gpios` on a **disabled** PDG GPIO sibling is rejected | as A-01 plus `&pdg_gpio0 { status = "disabled"; };` and `cs-gpios = <&pdg_gpio0 0 GPIO_ACTIVE_LOW>;` | `EXIT != 0`; log contains the registered `DT_NODE_HAS_STATUS_OKAY` diagnostic; log does **not** contain the A-02 or A-03 diagnostics |
| **A-05** | `cs-gpios` is `required: true`; an enabled controller without it fails **at devicetree processing** | `spi_nor_id` + overlay enabling `pdg_spi0` with no `cs-gpios` | `EXIT != 0`; log contains ``'cs-gpios' is marked as required in 'properties:' in`` **and** `odp,pico-de-gallo-spi.yaml`; **`compile_commands.json` is absent from the build dir** (anti-vacuity: it failed before compilation, not at link) |
| **A-06** | The **disabled** shield node is unaffected by `required: true` | `i2c_bridge` unmodified (shield `pdg_spi0` stays `status = "disabled"`) | `EXIT=0`, 116/116; TU set is exactly `pdg_i2c.c`, `pdg_mfd.c` — `pdg_spi.c` absent |
| **A-07** | `GPIO_ACTIVE_HIGH` on a valid sibling CS is **permitted** (spec §3) | as A-01 with `cs-gpios = <&pdg_gpio0 0 GPIO_ACTIVE_HIGH>;` | `EXIT=0`. **Mandatory specificity control for A-02/A-03/A-04**: without it those three cannot distinguish "rejects the wrong controller" from "rejects any `cs-gpios` at all" |
| **A-08** | `spi/batch` is genuinely gone from the Zephyr module, not merely unused | `find <b> -name 'pdg_spi_bottom*.o' -exec nm --defined-only {} \;` | Symbol set is **exactly** `pdg_spi_bottom_set_config`, `pdg_spi_bottom_transfer`. `pdg_spi_bottom_batch`, `_open`, `_close`, `_num_gpios` **absent**. (Baseline measured today: `batch, close, num_gpios, open, set_config`.) |
| **A-09** | Enabled SPI requires explicit parent `serial-number` (spec §2, R11) | `spi_nor_id` + overlay with valid `cs-gpios` but `&pdg0` carrying **no** `serial-number` | `EXIT != 0`; log contains the registered `odp,pico-de-gallo-spi parent must define serial-number` substring; log does **not** contain any per-CS diagnostic (isolation: parent assertions precede per-CS assertions, spec §4) |
| **A-10** | Assertion block precedes `#include "pdg_mfd.h"` so a dropped MFD Kconfig does not mask the readable error | A-01 overlay + `/tmp/m4_nomfd.conf` (`CONFIG_MFD_PICO_DE_GALLO=n`); disable `pdg_i2c0`/`pdg_gpio0` so `pdg_spi.c` is the only PDG child | `grep -c '^CONFIG_MFD_PICO_DE_GALLO=y$' <b>/zephyr/.config` = 0 (**prove the fragment took effect**); log contains `require CONFIG_MFD_PICO_DE_GALLO=y`; its byte offset is strictly less than the first `pdg_mfd.h: No such file` offset (absence of the include error also satisfies) |
| **A-11** | Four-sample category preservation | build all four; compare **(undefined-symbol count, set of resolved node paths)** against §0.4 | `i2c_bridge` 0; `spi_nor_id` 0; `spi_bridge` and `combined_i2c_spi_bridge` exactly 1 undefined `__device_dts_ord_*` each, resolving to `/pico-de-gallo/spi/is31fl3743b@0`, `zephyr.elf` clean. **Never compare the literal ordinal** — M4 renumbers them |

Ordinal resolution, per build:

```bash
grep -o '__device_dts_ord_[0-9]*' /tmp/m4_<id>.log | sort -u | while read s; do
  n=${s##*_}
  grep -n "DT_N_.*ORD $n\$" /tmp/m4_test_<id>/zephyr/include/generated/zephyr/devicetree_generated.h
done
```

### 1.2 Class B — source-shape probes

Every probe in this section proves **shape only**. None proves the driver
behaves correctly at runtime.

| ID | Property under test | Mechanism (abridged; full scripts in §4) | Blind to |
| --- | --- | --- | --- |
| **B-01** | `cs-gpio-indices` is gone from all live code | scoped `rg` gate, §4.1 | a renamed equivalent |
| **B-02** | Bottom header is exactly the two declared functions | `[ $(grep -c '^int pdg_spi_bottom_' $BOTH) -eq 2 ]` + exact signature `grep -F`; `grep -Eq 'pdg_spi_bottom_(batch\|open\|close\|num_gpios)'` must be **false** in both `$BOT` and `$BOTH`; no `#include <zephyr/` in either | argument forwarding |
| **B-03** | `struct pdg_spi_data` shape | `spi_context spi_ctx` is member **1**; `void *ctx`; `bool cs_fault`; `int cs_fault_errno`; exactly 4 members; no `k_mutex`, no `cs_*index*` array, no gpio count | field semantics |
| **B-04** | Static init macro order | `off` ordering `SPI_CONTEXT_INIT_LOCK` < `SPI_CONTEXT_INIT_SYNC` < `SPI_CONTEXT_CS_GPIOS_INITIALIZE` | that they initialise anything |
| **B-05** | No readback verification anywhere | `grep -q 'gpio_pin_get_dt\|gpio_pin_get\b' $SRC` must be **false** | — |
| **B-06** | Checked-CS helper exists, returns errno, is the sole edge source | `extent $SRC pdg_spi_cs_control_checked` succeeds; every `gpio_pin_set_dt` occurrence in `$SRC` is inside that extent; each is captured (`ret =` / `return`), never called as a bare statement | that the errno reaches the caller |
| **B-07** | `spi_context_cs_control()` is never called | `grep -q 'spi_context_cs_control' $SRC` must be **false** | — |
| **B-08** | Mandatory comment #1 (spec §5.1) is present, in the helper | helper extent contains `Do not replace this with` + `spi_context_cs_control` + `discards errno` | that anyone reads it |
| **B-09** | Collapsed delay at **both** edges | helper extent contains exactly **2** `k_busy_wait(` calls, both on `->cs.delay`; assert-side one is *after* the `gpio_pin_set_dt` success check, deassert-side one is *before* it | the actual delay value |
| **B-10** | HOLD requires LOCK → `-ENOTSUP` | transceive extent: `SPI_HOLD_ON_CS` and `SPI_LOCK_ON` appear in one condition returning `-ENOTSUP`, at a byte offset **before** `spi_context_lock`, before `set_config`, and before any `gpio_pin_set_dt` | caller-visible errno |
| **B-11** | Latch is checked **after** the lock and **before** any I/O | in the transceive extent: `off spi_context_lock` < `off cs_fault` < `off pdg_spi_bottom_set_config` < `off gpio_pin_set_dt` < `off pdg_spi_bottom_transfer`; the latched branch returns `-EHOSTDOWN` | the race it closes |
| **B-12** | `-EHOSTDOWN` refusal is diagnostic-complete | the latched branch's single `LOG_ERR` names parent serial, CS pin, `cs_fault_errno`, and the literal `spi_release` | that it is emitted |
| **B-13** | Defanged unlock: exactly one private helper, correct shape | `extent $SRC pdg_spi_unlock_defanged` exists; contains `= ctx->config` then `ctx->config = NULL` then `spi_context_unlock_unconditionally` then a **conditional** restore; **every** `spi_context_unlock_unconditionally` call site in `$SRC` is inside that extent | that it never hangs |
| **B-14** | Mandatory comment #2 (spec §5.3) | helper extent contains `defang` + `do not restore the idiomatic live-config call` | — |
| **B-15** | Unlock is unconditional w.r.t. deassert result | in transceive/release extents, no `if` on the deassert errno guards the call to `pdg_spi_unlock_defanged`; it is reachable on every non-early-return path (reviewer-confirmed, plus: number of `return` statements after the deassert equals number of `pdg_spi_unlock_defanged` call sites preceding them) | actual reachability under `goto` |
| **B-16** | Restore-on-failure only | inside the defang helper: `ctx->config = <saved> ` occurs exactly once and is guarded by the deassert-failed condition; success path leaves `NULL` | — |
| **B-17** | Latch set **only** on force-deassert failure, first errno preserved | every `cs_fault = true` assignment in `$SRC` is textually adjacent (±3 lines) to a force-deassert errno test; every `cs_fault_errno =` assignment is guarded by `if (!data->cs_fault)` or equivalent | that the guard works |
| **B-18** | RX commit barrier | `off <unflatten call>` > `off <deassert-success test>` in the transceive extent; the unflatten call is inside the success branch; **no** unflatten occurs on the assert-failure or transfer-failure paths | that the buffer content is right |
| **B-19** | HOLD success may skip the end deassert but still commits | the HOLD branch reaches the unflatten call without an intervening deassert | — |
| **B-20** | Init: indexed configure-all loop reproduces the stock helper | init extent has a `for` over `spi_ctx.cs_gpios` with index; `device_is_ready` on the port **precedes** `gpio_pin_configure_dt`; the configure uses `GPIO_OUTPUT_INACTIVE`; unready returns `-ENODEV`; `spi_context_cs_configure_all` is **not** called | ordering at runtime |
| **B-21** | Init diagnostics carry index, pin, serial, phase, errno; `-EBUSY` special case | init extent `LOG_ERR` set includes all five tokens; a distinct branch on `-EBUSY` names `gallo_system_reset_subscriptions` and includes index and pin | that they fire |
| **B-22** | Init attempts no rollback | init extent contains no force-deassert and no second `gpio_pin_set_dt` after a failure | — |
| **B-23** | Init's terminal unlock issues no edge | the init extent's `pdg_spi_unlock_defanged` (or direct call) occurs while `spi_ctx.config` is provably `NULL` — i.e. no `spi_ctx.config =` assignment precedes it in the extent | — |
| **B-24** | `DT_FOREACH_PROP_ELEM_VARGS` shape exactly as specified | `$SRC` contains `DT_FOREACH_PROP_ELEM_VARGS(DT_DRV_INST(inst), cs_gpios, PDG_SPI_CS_ASSERT, inst)`; the macro body contains `DT_GPIO_CTLR_BY_IDX`, `DT_NODE_HAS_COMPAT`, `DT_NODE_HAS_STATUS_OKAY`, `DT_SAME_NODE(DT_PARENT(`, `DT_INST_PARENT(inst)` — **all five** | that it expands correctly (that is A-02/A-03/A-04) |
| **B-25** | Parent assertion order compatible → status → serial → Kconfig, then per-CS | `off` ordering inside the assertion macro; the per-CS foreach offset is greater than all four; whole block above `#include "pdg_mfd.h"` | emitted diagnostic order (A-10) |
| **B-26** | Preserved assertions | `BUILD_ASSERT(!IS_ENABLED(CONFIG_SPI_ASYNC)` and `CONFIG_SPI_RTIO` survive; `SPI_CS_ACTIVE_HIGH` rejection survives | — |
| **B-27** | Data path: always full-duplex `max(tx,rx)`, scratch both directions, 4096 kept, zero length short-circuits | transceive extent contains a `MAX(`/ternary over the two lengths; two scratch allocations; `4096`; a `len == 0` early `return 0` at an offset **before** `spi_context_lock` | correctness of the fill/discard |
| **B-28** | Binding contract | `$BIND`: `cs-gpios:` with `required: true`; **no** `cs-gpio-indices`; prose names `-EHOSTDOWN`, exclusive ownership, the four-RPC atomicity loss, HOLD+LOCK, and the delay collapse — as **five distinct** tokens no single sentence can satisfy | prose accuracy |
| **B-29** | Kconfig | `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY` exists, default `50`; help text states it must be greater than the GPIO priority | that Kconfig enforces it (it cannot) |
| **B-30** | Samples | each of the three SPI overlays: `#include <zephyr/dt-bindings/gpio/gpio.h>`, `&pdg_gpio0 { status = "okay"; }`, `cs-gpios = <&pdg_gpio0 0 GPIO_ACTIVE_LOW>`, `serial-number = "REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL"`, the R11 comment; `spi_nor_id`'s "GPIO 8" comment rewritten to `firmware user GPIO0 (RP2350 GPIO8, header pin 11)`; **no** `cs-gpio-indices` | — |
| **B-31** | Documentation parity (AGENTS.md §15.1) | `git diff --name-only HEAD` includes `book/src/interfaces/spi.md`, `book/src/interfaces/gpio.md`, `zephyr/README.md`, `zephyr/CHANGELOG.md` | doc accuracy |
| **B-32** | Scope containment | `git diff --name-only HEAD` matches **nothing** under `crates/`, no `Cargo.toml`/`Cargo.lock`, no `[package].version`; `git diff --check` empty; no CRLF in any changed file | — |
| **B-33** | Host batch API untouched | `crates/` diff empty **and** `rg -c 'gallo_spi_batch' crates/pico-de-gallo-ffi/src/lib.rs` unchanged from HEAD; `cargo test --workspace --locked` still 561/0/7 | — |

### 1.3 Class C — deferred to M5, zero assurance in M4

| ID | Behaviour | Why M4 cannot verify it | M5 mechanism |
| --- | --- | --- | --- |
| **C-01** | **CS assert failure aborts with the underlying errno and clocks nothing** | requires inducing a returning `gpio_pin_set_dt` failure | see §3.1 |
| **C-02** | **CS deassert failure is reported, never swallowed; success is never returned with CS asserted; RX is not committed** | as above | see §3.2 |
| **C-03** | **`SPI_HOLD_ON_CS \| SPI_LOCK_ON`: CS stays asserted across the transfer, then checked release drops it** | requires execution + a witness | see §3.3 — **this is M5's whole acceptance mechanism** |
| **C-04** | **HOLD without `spi_release()`: what is stranded, and is it detectable?** | as above | see §3.4 |
| **C-05** | **Latched controller refuses a *different* slave before configuring, asserting or clocking, and never returns 0** | requires a latched state | see §3.5 |
| **C-06** | Only a *successful* checked release clears the latch; a failed release unlocks but leaves it latched | requires two induced failures | §3.5 |
| **C-07** | **Defanged unlock issues no second GPIO edge** | edge count is unobservable without instrumentation | §3.6 |
| **C-08** | **Unlock happens even when the checked deassert failed** (guarded failure mode is a *hang*) | hang detection needs a bounded wait | §3.7 — **weakest design in this suite** |
| **C-09** | HOLD-without-LOCK returns `-ENOTSUP` to the caller | execution | direct `spi_transceive()` call, assert `== -ENOTSUP` |
| **C-10** | Collapsed delay actually elapses at both edges | µs waits between ms RPCs; unmeasurable in practice | logic-analyser only; spec §3.1 already concedes it is meaningless |
| **C-11** | Init residue: earlier entries inactive, failing entry indeterminate, later untouched | requires an induced mid-loop failure | fault injection; see §3.8 |
| **C-12** | `-EBUSY` monitored-pin diagnostic on init | requires an orphaned subscription on a declared CS pin | deliberately create one, then boot |
| **C-13** | Non-returning-RPC boundary (cable pull mid-transfer) | by construction unbounded | §3.9 — **cannot be made to terminate** |
| **C-14** | Read-only becomes zero-filled duplex; write-only discards RX | requires observing MOSI/MISO | logic analyser, or a peripheral that echoes |
| **C-15** | Priority inversion → `-ENODEV` before pin configuration | requires a `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY=44` build **and** execution | build + run + witness that no pin moved |
| **C-16** | Exclusive CS ownership violation by a direct GPIO consumer | requires two concurrent consumers | see §5 defect S-06 — this is an *application* obligation, not a driver property |
| **C-17** | `-EHOSTDOWN` reaches `spi_nor` (which collapses it to `-ENODEV`) | execution | `spi_nor_id` after inducing a latch |
| **C-18** | 4096 limit → `-EMSGSIZE` before any RPC | execution | oversize `spi_transceive()` |

---

## 2. Class A probes actually executed

Two were run. Both are **RED-baseline** runs: they establish, with verbatim
output, that the pre-M4 tree **accepts** the malformed devicetree the spec
requires M4 to reject. That makes A-02 and A-03 non-vacuous by construction —
the current tree *is* the mutant, so no separate mutation control is needed for
these two.

Environment: Zephyr `4.4.99` (`v4.4.0-6123-g26f811ee9d0d`), `native_sim/native/64`,
`ZEPHYR_TOOLCHAIN_VARIANT=host`, sample `zephyr/samples/spi_nor_id`.

### 2.1 A-02 RED baseline — foreign controller (`zephyr,gpio-emul`) is accepted today

`/tmp/m4_foreign_cs.overlay`:

```dts
#include <zephyr/dt-bindings/gpio/gpio.h>

&pdg0 {
	status = "okay";
	serial-number = "REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL";
};

&pdg_gpio0 {
	status = "okay";
};

&pdg_spi0 {
	status = "okay";
	cs-gpios = <&gpio0 0 GPIO_ACTIVE_LOW>;
};
```

`gpio0` is native_sim's `gpio_emul` node
(`~/zephyrproject/zephyr/boards/native/native_sim/native_sim.dts:186`,
`compatible = "zephyr,gpio-emul"`, `status = "okay"`). It is an *enabled, bound,
foreign* GPIO controller — exactly the R9-shaped hazard.

Verbatim tail:

```text
EXIT=0
   Compiling pico-de-gallo-ffi v0.7.1 (/mnt/d/workspace/pico-de-gallo/crates/pico-de-gallo-ffi)
   Compiling pico-de-gallo-lib v0.7.1 (/mnt/d/workspace/pico-de-gallo/crates/pico-de-gallo-lib)
    Finished `release` profile [optimized] target(s) in 16.48s
[1/121] Preparing syscall dependency handling
[3/121] Generating include/generated/zephyr/version.h
-- Zephyr version: 4.4.99 (/home/febalbi/zephyrproject/zephyr), build: v4.4.0-6123-g26f811ee9d0d
[115/121] Linking C executable zephyr/zephyr.elf; Logical command for additional byproducts on target: zephyr_pre0
Generating files from /tmp/m4_test_foreigncs/zephyr/zephyr.elf for board: native_sim/native/64
[120/121] Building native simulator runner, and linking final executable
[121/121] Running utility command for native_runner_executable
```

The phandle really landed — post-M4 the assertion has something to see:

```text
5799:#define DT_N_S_pico_de_gallo_S_spi_P_cs_gpios_IDX_0_EXISTS 1
5800:#define DT_N_S_pico_de_gallo_S_spi_P_cs_gpios_IDX_0_PH DT_N_S_gpio_emul
5801:#define DT_N_S_pico_de_gallo_S_spi_P_cs_gpios_IDX_0_VAL_pin 0
5803:#define DT_N_S_pico_de_gallo_S_spi_P_cs_gpios_IDX_0_VAL_flags 1
```

Non-vacuity evidence from the same build:

```text
$ grep -o 'pdg_[a-z0-9_]*\.c' /tmp/m4_test_foreigncs/compile_commands.json | sort -u
pdg_gpio.c
pdg_mfd.c
pdg_spi.c
```

`pdg_gpio.c` appearing in an SPI sample confirms the A-01 positive-control
mechanism works: enabling `pdg_gpio0` in the overlay does pull M3's driver in.

Bottom-half baseline confirmed for A-08, from the same build:

```text
$ find /tmp/m4_test_foreigncs -name 'pdg_spi_bottom*.o'
/tmp/m4_test_foreigncs/zephyr/NSI/mnt/d/workspace/pico-de-gallo/zephyr/drivers/spi/pdg_spi_bottom.o
$ nm --defined-only .../pdg_spi_bottom.o
0000000000000000 T pdg_spi_bottom_batch
0000000000000000 T pdg_spi_bottom_close
0000000000000000 T pdg_spi_bottom_num_gpios
0000000000000000 T pdg_spi_bottom_open
0000000000000000 T pdg_spi_bottom_set_config
```

**Result: RED confirmed.** `EXIT=0`. A foreign chip-select controller is
accepted by the current tree with no diagnostic whatsoever. Post-M4 this exact
command must produce `EXIT != 0` and the registered compatible diagnostic.

### 2.2 A-03 RED baseline — cross-parent PDG GPIO is accepted today

This is the **discriminating** probe. The CS target here is a genuine
`odp,pico-de-gallo-gpio` controller with `status = "okay"` — so
`DT_NODE_HAS_COMPAT` and `DT_NODE_HAS_STATUS_OKAY` both **pass**. Only
`DT_SAME_NODE(DT_PARENT(ctlr), DT_INST_PARENT(inst))` can catch it. An
implementation that checks compatible and status but forgets the same-parent
clause passes A-02 and **must fail A-03**.

`/tmp/m4_crossparent_cs.overlay`:

```dts
#include <zephyr/dt-bindings/gpio/gpio.h>

/ {
	pdg1: pico-de-gallo-1 {
		compatible = "odp,pico-de-gallo";
		serial-number = "SOME_OTHER_BOARD_SERIAL";
		status = "okay";

		pdg_gpio1: gpio {
			compatible = "odp,pico-de-gallo-gpio";
			gpio-controller;
			#gpio-cells = <2>;
			ngpios = <4>;
			status = "okay";
		};
	};
};

&pdg0 {
	status = "okay";
	serial-number = "REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL";
};

&pdg_gpio0 {
	status = "okay";
};

&pdg_spi0 {
	status = "okay";
	cs-gpios = <&pdg_gpio1 0 GPIO_ACTIVE_LOW>;
};
```

Verbatim tail:

```text
EXIT=0
-- Zephyr version: 4.4.99 (/home/febalbi/zephyrproject/zephyr), build: v4.4.0-6123-g26f811ee9d0d
[115/121] Linking C executable zephyr/zephyr.elf; Logical command for additional byproducts on target: zephyr_pre0
Generating files from /tmp/m4_test_crossparent/zephyr/zephyr.elf for board: native_sim/native/64
[117/121] Copying byproducts `libpico_de_gallo_ffi.a` to /tmp/m4_test_crossparent/modules/pico-de-gallo
[120/121] Building native simulator runner, and linking final executable
[121/121] Running utility command for native_runner_executable
```

The topology is genuinely cross-parent — verified from the generated header,
by symbol not by literal ordinal:

```text
5860:#define DT_N_S_pico_de_gallo_1_S_gpio_PARENT DT_N_S_pico_de_gallo_1
5987:#define DT_N_S_pico_de_gallo_S_spi_PARENT     DT_N_S_pico_de_gallo
6064:#define DT_N_S_pico_de_gallo_S_spi_P_cs_gpios_IDX_0_PH DT_N_S_pico_de_gallo_1_S_gpio
```

`DT_PARENT(ctlr)` = `DT_N_S_pico_de_gallo_1`, `DT_INST_PARENT(inst)` =
`DT_N_S_pico_de_gallo`. Distinct nodes; `DT_SAME_NODE` must be false.

TU set identical to A-02 (`pdg_gpio.c`, `pdg_mfd.c`, `pdg_spi.c`).

**Result: RED confirmed.** `EXIT=0`. The current tree binds a chip-select to a
GPIO controller on a *different physical board* and says nothing. This is the
2026-07-29 §13.17 failure class — an ambiguous board target driving the wrong
pins — expressed in devicetree.

### 2.3 A-05 mechanism, verified from source rather than by a build

The required-property gate and its status scoping were confirmed by reading the
pinned edtlib, not by an extra build:

```python
# ~/zephyrproject/zephyr/scripts/dts/python-devicetree/src/devicetree/edtlib.py:1888
if not prop:
    if required and self.status == "okay":
        _err(
            f"'{name}' is marked as required in 'properties:' in "
            f"'{binding_path}', but does not appear in {node!r}"
        )
```

Two facts follow, and both are load-bearing for the spec:

1. The registered A-05 substring is
   `'cs-gpios' is marked as required in 'properties:' in`, and it is raised via
   `_err` — a hard devicetree error, so `compile_commands.json` is never
   generated. That is the A-05 anti-vacuity criterion.
2. `and self.status == "okay"` is exactly why the **disabled shield node stays
   valid** (spec §2). A-06 is the positive half of the same gate.

The A-05 RED baseline needs no new build: today `spi_nor_id` has **no**
`cs-gpios` at all and is the confirmed-clean 117/117 baseline (§0.4). Today
missing CS builds; post-M4 it must not.

### 2.4 Cleanup

`/tmp/m4_test_foreigncs` and `/tmp/m4_test_crossparent` were removed. No other
agent's directory was touched. `ls -la /tmp | grep -i m4` returns nothing.

---

## 3. M5-deferred test designs

Written so M5 inherits them. Each names its witness mechanism and its known
failure-to-observe.

### 3.0 Fixture preconditions M5 must establish before any of these

- **Power-cycle first.** Plan R7 records an orphaned GPIO subscription on
  firmware pin 2 and pin 3 parked as an output high. A first `set_config` on
  pin 2 returns `GpioPinMonitored` → `-EBUSY`.
- **Explicitly call `gallo_system_reset_subscriptions()` after strict open**,
  check status, log the reset count (spec §8). Keep it out of GPIO callbacks,
  SPI init and ordinary parent init.
- **Establish exclusive ownership** of CS 2 and witness 3 before trusting any
  result.
- **Witness-pin protocol (mandatory, plan R2).** To observe a CS edge on pin 2
  with pin 3 as witness: pre-drive the node **low**, release to pull-down,
  **verify the low baseline**, and only then run the transfer. A pull-down alone
  cannot pull an already-high node low, and a floating pad drifts high within
  seconds — a probe that skips the baseline verification **passes against broken
  code**. Alternatively use a pull-up and invert every expectation.
- **Never read CS back through the same controller.** `port_get_raw` returns 0
  for an output pin by design.

### 3.1 C-01 — CS assert failure aborts and clocks nothing

*Induction.* The only fault injectable without a `crates/` change is transport
loss: unplug the cable between `spi/set-config` and `gpio/put(assert)`. That is
racy and usually produces the *non-returning* case (§3.9) instead. **The
reliable vehicle is a `CONFIG`-gated fault-injection shim in the bottom half**
— a build-time-only `pdg_spi_bottom_transfer`/`gpio` wrapper that fails the Nth
call. M5 must budget for building it; it does not exist.

*Assertions.* (a) `spi_transceive()` returns the assert errno verbatim, not a
substituted one. (b) The transfer RPC was never issued — assert on an
instrumented call counter, **not** on bus observation. (c) A checked
force-deassert was attempted exactly once. (d) With the force-deassert
acknowledged, `cs_fault` is **false** — verified by a follow-up transfer to a
different slave succeeding rather than returning `-EHOSTDOWN`. (d) is the only
part observable through the public API.

### 3.2 C-02 — CS deassert failure is reported and RX is not committed

*Setup.* Pre-fill the caller's RX buffer with a poison pattern (`0xA5` repeated).
Run a transfer whose data would be non-poison, with the deassert forced to fail.

*Assertions.* (a) Return value is the **deassert** errno, not 0 and not the
transfer errno. (b) The caller's RX buffer is **still entirely poison** — this
is the direct, cheap, decisive test of the RX commit barrier and it needs no
instrumentation at all. (c) A subsequent transfer to a *different* slave returns
`-EHOSTDOWN` (proves the latch was set). (d) The single `LOG_ERR` names both the
primary and the cleanup errno.

Assertion (b) is the highest value-per-effort test in the whole M5 set. Record it
as such.

### 3.3 C-03 — `SPI_HOLD_ON_CS | SPI_LOCK_ON`, then `spi_release()`

**This is M5's acceptance mechanism; its design matters more than the rest.**

*Fixture.* CS = firmware index 2, witness = index 3, jumpered. Apply §3.0's
witness protocol: drive node low, release to pull-down, **verify low**.

*Sequence and assertions.*

| Step | Action | Assertion |
| --- | --- | --- |
| 0 | Verify witness baseline LOW | if not LOW, **abort the whole run** — otherwise every later expectation is vacuous |
| 1 | `spi_transceive(cfg)` with `SPI_HOLD_ON_CS \| SPI_LOCK_ON`, `GPIO_ACTIVE_LOW` CS | returns 0; RX **is** committed (spec §5.2 permits it for deliberate HOLD — see defect S-03) |
| 2 | Read witness | CS asserted ⇒ physical LOW ⇒ witness LOW. **Must be read through the witness pin, never through the CS pin.** |
| 3 | Second `spi_transceive` with the **same LOCKed config** | returns 0 (spec §5.2: repeated HOLD with the same LOCKed config remains valid); witness still LOW |
| 4 | `spi_transceive` with a **different** config | must block on the semaphore, not proceed. Detect with a bounded `k_msleep` + a flag, since there is no non-blocking variant. **This is the only way to observe LOCK at all.** |
| 5 | `spi_release(dev, cfg)` | returns 0 |
| 6 | Read witness | HIGH (deasserted) |
| 7 | `spi_release(dev, cfg)` again | must be **rejected** (spec §5.3: successful release leaves config NULL) |
| 8 | `spi_transceive` with a different config | now succeeds |

Step 6 is the actual "checked force-deassert in release drops CS" claim. Steps 7
and 8 are what distinguish a correct release from one that merely returned 0.

*Known weakness.* Step 4 uses a timeout to infer blocking. A sufficiently slow
USB round-trip is indistinguishable from a held lock. Choose the bound at
≥10× the measured p99 transfer latency and record the measurement.

### 3.4 C-04 — HOLD with no `spi_release()`: what is stranded?

*Stranded, per spec §5.2 and the §6 residue table:* (a) the physical CS line,
asserted; (b) `spi_context`'s semaphore and owner — software ownership; (c)
`ctx->config`, still pointing at the caller's config.

*Detectability, honestly:*

- **Within the same process: yes.** Any subsequent `spi_transceive` with a
  different config blocks forever. Detect with the §3.3 step-4 bounded-wait
  probe. That is a *positive* detection of the stranding.
- **Across process death: no.** Nothing in M4 detects it, by design. A new host
  process re-opens the board and finds a CS line already low with no record of
  why. The firmware has no CS ownership concept; `gpio/get` on that pin is not
  state-neutral (§0.3 trap 2), so even *looking* perturbs it.
- **Recommended M5 assertion:** after a deliberate HOLD-and-abandon, terminate
  the process, start a fresh one, and assert that the witness pin is **still
  LOW** — i.e. the stranding is real and survives. Then assert that a fresh
  strict open followed by an explicit deassert recovers it. That converts an
  unobservable hazard into a documented, reproducible recovery procedure, which
  is the most M4's contract actually promises.

### 3.5 C-05 / C-06 — the fault latch

*This is the #104 failure class restated:* a controller that answers a
*different* slave while a previous slave may still be selected.

*Sequence.*

1. Induce a failed force-deassert on slave A (needs the §3.1 shim).
2. `spi_transceive` to **slave B** (a different `cs-gpios` index).
   - **Must return `-EHOSTDOWN`.** Not 0, not the original errno.
   - **Must issue no RPC at all** — assert on the instrumented counters for
     `set_config`, `gpio put`, and `transfer`, all unchanged. This is the
     "before configuring, asserting, or clocking" clause and it is *only*
     checkable with instrumentation; there is no black-box substitute.
   - Witness pin for B must not have moved.
3. `spi_release(dev, <the retained fault config>)` with the deassert still
   failing → returns the edge errno, latch **retained**, but the semaphore is
   released (assert: a subsequent call reaches the latch check rather than
   blocking — a *distinguishing* observation, because a wedged lock and a latched
   controller both fail, but with different errnos and different timings).
4. `spi_release` with the deassert now succeeding → returns 0, latch cleared,
   config NULL.
5. `spi_transceive` to slave B → now succeeds.

*Untestable part.* "Store the **first** originating deassert errno and do not
replace it while latched" (spec §5.4) requires inducing **two failures with
distinct errnos**. Cable-pull yields `-ECOMM`/`-EIO` both times. Without the
fault shim returning a chosen errno, this clause is **not testable even in M5**.
See §5 defect S-05.

### 3.6 C-07 — the defanged unlock issues no second edge

*There is no black-box test for this.* The edge, if issued, is a `gpio/put` to
the same pin with the same logical value the checked deassert already applied —
electrically invisible, and unobservable through the API.

*Only viable mechanism:* an instrumented build that counts
`pdg_gpio_bottom_put` invocations. Assert exactly **one** put per ordinary
transfer end, and exactly **one** per release. A second put is the defect.

*If instrumentation is refused,* B-13's "every `spi_context_unlock_unconditionally`
call site lies inside the defang helper" is the entire assurance, and it is
Class B. Say so; do not report C-07 as covered.

### 3.7 C-08 — unlock happens even when the checked deassert failed

*The guarded failure mode is a hang*, which is intrinsically hard to test.

*Options, in descending order of honesty:*

1. **Bounded wait (usable).** After a failed release, a second thread attempts
   `spi_transceive` with a fresh config and a test-side deadline. If it reaches
   the latch check and returns `-EHOSTDOWN` within the deadline, the semaphore
   was given. If it times out, ownership was wedged. This *does* distinguish the
   two outcomes — the latch is what makes the wedge distinguishable from a
   correct refusal, so the fault latch is load-bearing for its own test.
2. **Instrumented counter.** Count `spi_context_unlock_unconditionally` entries
   and exits. An entry without an exit is the hang.
3. **Watchdog / process timeout.** Detects *a* hang but cannot attribute it; a
   non-returning USB RPC (§3.9) produces an identical symptom.

**Honest answer: not reliably.** Option 1 is the best available and it cannot
distinguish "unlock hung" from "the *preceding* RPC never returned", which is
precisely the case spec §3.1 says is out of scope. Report C-08 as *partially*
testable and name the confound.

### 3.8 C-11 — init residue

Requires failing the Nth `GPIO_OUTPUT_INACTIVE` in a multi-entry `cs-gpios`
array. Declare three CS entries, fail entry 1 (0-based), then assert:

- entry 0's witness is LOW-inactive (acknowledged);
- entry 2's pin was **never** configured — assert on the counter, since its
  physical state is meaningless before configuration;
- init returned nonzero and the device is **not ready**;
- the `LOG_ERR` names index 1, its pin, the parent serial, the phase, the errno.

Entry 1's own state is specified as **indeterminate**, which no observation can
contradict — see §5 defect S-02.

### 3.9 C-13 — the non-returning boundary

**This test cannot be made to terminate**, which is the point. The only
defensible M5 form is a *documentation-conformance* check, not a behavioural one:

1. Start a transfer, pull the cable mid-RPC.
2. Assert the calling thread does **not** return within a generous deadline.
3. Assert the recovery procedure the spec documents actually works: terminate
   the process, start a fresh one, strict-open, reset subscriptions, deassert
   the CS pin, confirm the witness returns HIGH.

Step 3 is the valuable half. Steps 1–2 merely confirm the hazard exists.

---

## 4. Exact commands for the scoped gates

### 4.1 B-01 — `cs-gpio-indices` residue gate

Baseline measured today (`rg -n "cs-gpio-indices|cs_gpio_indices" .`, per file):

| File | Hits today | Post-M4 required |
| --- | --- | --- |
| `zephyr/drivers/spi/pdg_spi.c` | 10 | **0** |
| `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml` | 5 | **0** |
| `zephyr/README.md` | 9 | **0** |
| `zephyr/CHANGELOG.md` | 1 | **0** |
| `book/src/interfaces/spi.md` | 3 | **0** |
| `zephyr/samples/spi_nor_id/app.overlay` | 1 | **0** |
| `zephyr/samples/spi_bridge/app.overlay` | 1 | **0** |
| `zephyr/samples/combined_i2c_spi_bridge/app.overlay` | 1 | **0** |
| `AGENTS.md` | 1 | preserved (§13.17) |
| `docs/superpowers/plans/2026-08-14-spi-chip-select-contract.md` | 4 | preserved |
| `docs/superpowers/specs/2026-08-14-spi-chip-select-contract-design.md` | 4 | preserved |
| `docs/superpowers/plans/2026-08-17-zephyr-mfd-restructure.md` | 4 | preserved |
| `docs/superpowers/specs/2026-08-17-zephyr-mfd-restructure-design.md` | 4 | preserved |
| `docs/superpowers/specs/2026-08-17-zephyr-mfd-m2-nesting.md` | 10 | preserved |
| `docs/superpowers/specs/2026-08-17-zephyr-mfd-m3-gpio.md` | 2 | preserved |
| `docs/superpowers/specs/2026-08-19-zephyr-mfd-m4-cs-gpios.md` | 2 | preserved |
| this document | n | preserved |

Note the baseline count for `zephyr/CHANGELOG.md` is **1**, not the "Unreleased
line 51" singular the spec names — consistent, but confirm the line is deleted
rather than merely moved.

The gate — zero hits **outside** the explicit preserve set, not an impossible
repository-wide zero (spec §9):

```bash
rg -n --no-heading 'cs-gpio-indices|cs_gpio_indices' . \
  | grep -v '^AGENTS\.md:' \
  | grep -v '^docs/superpowers/' \
  | grep -q . && echo "B-01 FAIL: live residue above" && exit 1
echo "B-01 PASS"
```

Companion anti-vacuity conjunct (the gate must be able to see *something*):

```bash
[ "$(rg -c 'cs-gpio-indices' AGENTS.md)" -ge 1 ] || { echo "B-01 gate is blind"; exit 1; }
```

Without that conjunct, a typo in the pattern makes B-01 pass unconditionally —
the plan §9.1 failure mode.

### 4.2 A-08 — bottom-half symbol assertion

```bash
O=$(find /tmp/m4_test_a01 -name 'pdg_spi_bottom*.o' | head -1)
[ -n "$O" ] || { echo "A-08 FAIL: bottom object not built"; exit 1; }
GOT=$(nm --defined-only "$O" | awk '$2=="T"{print $3}' | sort | tr '\n' ' ')
WANT="pdg_spi_bottom_set_config pdg_spi_bottom_transfer "
[ "$GOT" = "$WANT" ] || { echo "A-08 FAIL: got [$GOT] want [$WANT]"; exit 1; }
echo "A-08 PASS"
```

The `find` guard is the anti-vacuity check: an empty symbol set would otherwise
never equal `$WANT` and the probe would report the right answer for the wrong
reason if the object were simply missing.

### 4.3 A-01 non-vacuity — note the digits

```bash
grep -o 'pdg_[a-z0-9_]*\.c' /tmp/m4_test_a01/compile_commands.json | sort -u
# must contain BOTH pdg_spi.c and pdg_gpio.c, plus pdg_mfd.c
```

`pdg_gpio.c` is **new** in the SPI samples after M4 — the overlays now enable
`pdg_gpio0`. That is *positive evidence the migration took effect*, not merely
that nothing broke. Verified today that the mechanism works (§2.1).

---

## 5. Mutation controls

A probe nobody has confirmed can fail is not evidence.

| # | Mutation | Must fail | Must **not** fail (specificity) |
| --- | --- | --- | --- |
| **M1** | *(free — already confirmed)* the pre-M4 tree itself | **A-02**, **A-03**, **A-05** | — |
| **M2** | Drop the `DT_SAME_NODE(DT_PARENT(...))` clause from `PDG_SPI_CS_ASSERT`, keep compatible + status | **A-03** | **A-02**, **A-04** must still fail correctly — proves the three clauses are independently detected |
| **M3** | Drop `required: true` from `cs-gpios` in the binding | **A-05** | A-01, A-07 unaffected |
| **M4** | Call `spi_context_unlock_unconditionally()` directly, with a live config, at the end of transceive | **B-13** (call site outside the defang extent) | B-16 unaffected |
| **M5** | Move the RX unflatten above the deassert-success check | **B-18** | B-27 unaffected |
| **M6** | Delete the latch check from transceive | **B-11** | B-17 unaffected — proves setting and checking are separately probed |
| **M7** | Move the latch check *before* `spi_context_lock` | **B-11** ordering conjunct only | B-12 unaffected |
| **M8** | Accept `SPI_HOLD_ON_CS` without `SPI_LOCK_ON` | **B-10** | B-26 unaffected |
| **M9** | Keep `pdg_spi_bottom_batch` in the bottom half but stop calling it | **A-08**, **B-02** | A-01 must still build — this is the "deleted vs still linked but unused" discriminator |
| **M10** | Restore `ctx->config` unconditionally after the defanged unlock | **B-16** | B-13 unaffected |

M2 and M9 are the two that matter most. **M2** is the only control proving the
three per-CS clauses are not one undifferentiated blob. **M9** is the only
control proving A-08 detects *linkage*, not *call sites* — a `grep` for
`pdg_spi_bottom_batch` in `pdg_spi.c` would pass under M9; `nm` on the object
would not.

Apply each to a scratch copy; never commit; never while another agent holds the
build slot.

---

## 6. Grading

### 6.1 Probe counts

| Class | Count | Share of probes |
| --- | --- | --- |
| **A** | 11 | 17.7% |
| **B** | 33 | 53.2% |
| **C** | 18 | 29.0% |
| **Total** | **62** | 100% |

**Probe count is not contract share.** Class B probes are cheap, so they are
numerous; that inflates their apparent weight. The grading that matters is §6.2.

### 6.2 Honest grade — fraction of the M4 contract genuinely proved

| Contract area | Share of M4 | Assurance |
| --- | --- | --- |
| Devicetree topology and binding contract — `cs-gpios` required, same-parent, compatible, status-okay, disabled-shield exemption, active-high permitted, four-sample category preservation, module non-vacuity, batch symbol removal | **~20%** | **Class A — genuinely proved.** A-02 and A-03 have confirmed RED baselines; A-05's mechanism is verified in pinned edtlib source; A-08 has a measured before-symbol-set. |
| Driver source shape — checked-edge helper, defanged unlock, latch placement, RX barrier, init loop, flag validation, data path, comments, docs, scope containment | **~35%** | **Class B — shape only.** Strong against deletion and rot. **Useless against present-but-wrong.** |
| Runtime behaviour — CS assert/deassert failure handling, HOLD+LOCK lifecycle, latch refuse/clear, no-second-edge, unlock-despite-failure, RX non-commit, init residue, non-returning boundary | **~45%** | **Class C — zero.** |

**Roughly one fifth of M4 is genuinely proved; about one third is source-shape
only; nearly half has no assurance at all in this milestone.**

### 6.3 The spec's most-argued properties, and which class they land in

This is the part that matters most upward. M3 found both of its flagship
properties were merely source-shape and said so. M4 is worse in one respect and
better in another.

**Better:** the two properties the spec argues for in §3 — the same-parent CS
assertion and the required `cs-gpios` — are **genuinely Class A**, and I have
executed the RED baselines for both mechanisms. This is real, and it is the
first time in this restructure that a flagship property has been provable.

**Worse:** the four properties the spec argues *hardest* for — the ones with
their own numbered sections, their own alternatives-rejected entries, and their
own mandatory source comments — are **all Class B or C**:

| Property | Spec weight | Actual class |
| --- | --- | --- |
| **Defanged unlock issues no second edge** (§5.3, two rejected alternatives, a mandatory source comment) | highest | **B** (B-13/B-14) shape; **C-07** behaviour has *no black-box test at all* |
| **Fault latch: refuse a different slave before any I/O** (§5.4, invariant 7, the #104 lesson) | highest | **B** (B-11 byte ordering); **C-05** needs instrumentation to prove "no RPC issued" |
| **RX commit barrier** (§2, §5.2, invariant 10) | high | **B** (B-18); **C-02(b)** is the cheapest real test and belongs to M5 |
| **HOLD requires LOCK** (§5.2, invariant 9) | high | **B** (B-10); trivially **C-09** at M5 |

Anyone reading "the latch is tested" in M4 should read it as "**a byte-offset
ordering between two source constructs is asserted**", nothing more. B-11 will
catch deletion of the latch check and catch moving it before the lock. It will
**not** catch a latch check that reads the wrong field, tests the wrong sense, or
returns 0.

### 6.4 Properties untestable even in M5

1. **"Store the *first* originating deassert errno and do not replace it while
   latched"** (§5.4). Requires two induced deassert failures with **distinct**
   errnos. Every naturally inducible failure (cable pull, transport loss)
   collapses to `-ECOMM`/`-EIO`. *To make it testable:* a `CONFIG`-gated
   fault-injection shim in `pdg_spi_bottom.c` that returns a caller-chosen
   errno on the Nth call. That is new code M4 does not budget for; M5 must.
2. **"The defanged unlock issues no second GPIO edge"** (§5.3, invariant 5). The
   hypothetical second edge is electrically identical to the first and invisible
   through the API. *To make it testable:* an instrumented build counting
   `pdg_gpio_bottom_put` calls. Without that, this property is permanently
   Class B.
3. **"Returns `-EHOSTDOWN` before *any* hardware I/O"** (§5.4). Proving the
   *absence* of an RPC requires the same call counters. A black-box observer
   cannot distinguish "no RPC issued" from "an RPC issued and had no visible
   effect".
4. **"No subsequent code, latch, cleanup, RX commit, or unlock executes"** for a
   non-returning RPC (§6 residue table). By construction the test cannot
   terminate; §3.9 converts it into a recovery-procedure conformance check,
   which is a different claim.
5. **Init residue "indeterminate"** clauses (§4.1, §6 rows 2–3). No observation
   can contradict "indeterminate". These are documentation obligations, not
   testable properties, and should be labelled as such.

### 6.5 Spec defects — cases where I could not construct a test a wrong
implementation would fail

These are **unfalsifiability defects**, reported before code is written.

**S-01 — §5.2's RX-commit rule for HOLD is permissive, so both behaviours
conform.** The text is *"Successful deliberate HOLD **may** unflatten because
remaining selected is requested and protected by LOCK."* `may` means an
implementation that commits RX and one that does not are **both** correct. But
M5's acceptance (§3.3 step 1) needs the data. **I cannot write a test that a
wrong implementation fails, because no implementation is wrong.** *Fix:* change
`may` to `must`. This is the single most consequential wording defect in the
spec, because HOLD+LOCK is M5's acceptance vehicle.

**S-02 — "indeterminate" is unfalsifiable.** Used in five places (§4.1 twice,
§6 rows 2, 3, 6, 7). Nothing can contradict it. These are honest hazard
descriptions and should be typographically marked as *residue documentation*,
not listed alongside testable contract clauses in the same table. A reader —
or a later grading pass — will otherwise count them as covered.

**S-03 — §3.1's exclusive-ownership rule constrains applications, not the
driver.** *"Applications must give SPI exclusive ownership of every declared CS
pin."* There is no M4 implementation that can violate this, so no test can fail.
It is a documentation requirement (correctly covered by B-28's token check) and
should not appear in §10's "adversarial tests must cover" list, where it is
currently listed as `exclusive ownership docs`. Flagged because §10 reads as a
test-coverage obligation and this item is not one.

**S-04 — §5.4's "unacknowledged" is never defined.** It is used interchangeably
with "returns an error" (§5.2: *"latches only if the force-deassert is
unacknowledged"*; §6: *"Cleanup fails: ... latch set"*). A lost ACK on a
*successfully executed* remote operation also returns an error, so the two are
indistinguishable — which is fine, but the spec should say so explicitly.
*Fix:* define once, near §5.1: "unacknowledged ≡ `gpio_pin_set_dt()` returned
nonzero; this cannot be distinguished from non-execution."

**S-05 — §5.4's first-errno preservation has no inducible witness.** See §6.4
item 1. Currently written as a hard requirement with no route to verification.
*Fix:* either commit M5 to the fault-injection shim, or downgrade the clause to
"best-effort diagnostic, structurally asserted only".

**S-06 — §2's "Sample CS index 0 ... avoids known fixture residue" is stated as
a decision but is not a property.** The spec correctly disclaims it (*"it is not
a universal electrical-safety claim"*), so no test is owed. Noted only so the
grading does not count it.

**S-07 — Invariant 11 ("Fail-closed claims cover returning RPCs only") makes the
headline D10 guarantee conditional on an unobservable precondition.** Every
fail-closed assertion in this suite is implicitly "…provided the RPC returned",
and nothing can establish that precondition in advance. This is honest and
correct, but it means **the D10 fail-closed property is not a testable
guarantee** — it is a guarantee about a subset of executions identified only in
hindsight. Worth stating plainly in §12 rather than as a trailing invariant.

**S-08 — §4.1 supersedes §11 step 4 without the plan being updated.** §4.1 says
the local indexed loop "supersedes the original requirement to call the stock
helper"; §11 step 4 still says "RED init tests: ... indexed configure-all
residue". Consistent, but B-20 asserts `spi_context_cs_configure_all` is **not**
called — if the coder reads §11 alone they may call it and pass §11 while
failing B-20. Minor, but it is exactly the kind of drift that produced the
§13.17 2026-06-11 row.

---

## 7. Scope confirmation

- No `gallo_*` MCP tool was invoked.
- No binary was executed. Two builds were performed, sequentially, one at a time.
- Only this document was created. Nothing under `crates/`, `zephyr/drivers/`,
  the bindings, or the sample overlays was touched. No `[package].version`,
  `Cargo.lock`, wire, or firmware change.
- No tree-wide `git checkout`/`restore`/`reset`/`clean`; no formatting run.
- Nothing committed or pushed.
- `/tmp/m4_test_foreigncs` and `/tmp/m4_test_crossparent` were deleted by their
  owner. No other agent's directory was touched.
</content>
</invoke>
