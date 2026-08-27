# Zephyr hardware-free helper coverage - design

- **Date:** 2026-08-27
- **Issue:** [#109](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/109) - *test(zephyr): Add twister metadata, hardware-free tests, and CI*
- **Related:** #98 (upstreaming tracker), #130 / #149 (the twister metadata and
  CI half, which this document does *not* cover), #101 (zero-length I2C write),
  #102 (`i2c_burst_write()`)
- **Status:** approved in principle, pending implementation

---

## 1. Problem

Six `static` helper functions carry most of the Zephyr drivers' decision logic,
and none has any automated coverage:

| Function | File |
|---|---|
| `speed_to_code_` | `zephyr/drivers/i2c/pdg_i2c.c` |
| `freq_to_speed_` | `zephyr/drivers/i2c/pdg_i2c.c` |
| `validate_group_` | `zephyr/drivers/i2c/pdg_i2c.c` |
| `bufset_len_` | `zephyr/drivers/spi/pdg_spi.c` |
| `flatten_tx_` | `zephyr/drivers/spi/pdg_spi.c` |
| `unflatten_rx_` | `zephyr/drivers/spi/pdg_spi.c` |

Deliberately no line numbers. That table has carried them twice and been wrong
both times - see §4.2 of `docs/superpowers/plans/2026-08-11-zephyr-handoff.md`.

`validate_group_` is the highest-value target by a wide margin. It is the site
of the #102 fix, it holds the overflow-safe running total that stops two legal
4096-byte writes merging into an illegal 8192, and it is the only one of the six
with real branch depth.

The module's existing gates cannot reach any of this. `zephyr/scripts/ci-build.sh`
and the `twister` job added in #149 are **build-only**: running a produced binary
reaches `gallo_init_strict()` in `zephyr/drivers/common/gallo_registry.c` and
needs an attached board. Behavioural claims still rest entirely on the manual
`zephyr/tests/pdg_mfd_m5/run-m5.sh` procedure.

## 2. Constraint

The design must survive upstreaming into `zephyrproject-rtos/zephyr` (#98).
This is the constraint that eliminates approaches, and it is why §3 is mostly a
record of things that do not work.

## 3. What was ruled out, and on what evidence

### 3.1 The "no `#include` of a `.c`" constraint was lifted

`docs/superpowers/plans/2026-08-11-zephyr-handoff.md` recorded this as a hard
maintainer ruling, and concluded it "rules out the whitebox approach, which is
otherwise the cheapest way in."

It cannot stand *on the grounds that upstream would reject it*, because upstream
is where the idiom comes from. Seven of the fourteen suites under `tests/unit/`
include the `.c` under test directly:

```c
tests/unit/rbtree/main.c       #include "../../../lib/utils/rb.c"
tests/unit/base64/main.c       #include "../../../lib/utils/base64.c"
tests/unit/winstream/main.c    #include "../../../lib/utils/winstream.c"
tests/unit/hex/main.c          #include "../../../lib/utils/hex.c"
tests/unit/net_timeout/main.c  #include "../../../subsys/net/ip/net_timeout.c"
tests/unit/cbprintf/main.c     #include "../../../lib/os/cbprintf.c"
tests/unit/crc/main.c          #include "../../../subsys/crc/crc8_sw.c"  (x8)
```

There is a dedicated pseudo-board (`subsys/testsuite/boards/unit_testing`,
identifier `unit_testing`, `arch: unit`), a dedicated CMake component
(`find_package(Zephyr COMPONENTS unittest)`), a dedicated twister key
(`type: unit`), and a section in `doc/develop/test/ztest.rst`. The constraint is
marked superseded, not deleted: it was a real ruling and the record should show
both that it was made and why it was reversed.

### 3.2 Route C - `tests/unit/` host compilation - is REFUTED

Lifting the constraint made route C look like the obvious answer. It is not.
A throwaway spike (PR #149, commits `d89d3b1` and `74d2fa0`, retired in
`9e45272`) put it in front of a compiler three times:

| Run | Failure | What it settled |
|---|---|---|
| 1 | `bits/wordsize.h: No such file or directory` | Nothing about the `#include`. `unittest.cmake` defaults to `-m32` and `ubuntu-latest` has no 32-bit glibc headers; Zephyr's own `ztest_defaults.c` failed first and identically. Cleared with `gcc-multilib`. |
| 2 | `zephyr/syscalls/device.h: No such file or directory`, from `<zephyr/device.h>:1438` | `unittest.cmake` `file(TOUCH)`es a **hard-coded list** of empty generated headers, and `syscalls/device.h` is not on it. |
| 3 | `devicetree.h:3296: return type defaults to 'int'`; `sys/bitarray.h:34: empty declaration`; `sys/mem_blocks.h:51: storage class specified for parameter` | Supplying that one header **moved** the failure rather than removing it. `syscall_macros.h` is TOUCHed empty too, so `__syscall` expands to nothing and `devicetree.h`, `ffs.h`, `bitarray.h` and `mem_blocks.h` collapse together. |

The TOUCH list in full:

```
devicetree_generated.h  heap_constants.h  offsets.h
syscall_list.h          syscall_macros.h
syscalls/{kernel,kobject,log_core,log_ctrl,log_msg,sys_clock}.h
```

**Conclusion: the `unit_testing` board cannot compile a driver translation
unit.** The empty-generated-header scheme only holds for code that barely
touches Zephyr. This is a property of the harness, not of this module.

That reframes an observation made earlier and dismissed as incidental: *no
`tests/unit/` suite is a driver.* All fourteen come from `lib/utils`,
`subsys/crc` and `subsys/net/ip`. That absence is a consequence, not a
coincidence, and weighing it before recommending the route would have saved
three CI runs. Recorded here because the reasoning error - counting precedents
without checking what they had in common - is more transferable than the result.

### 3.3 Route A - extraction - is refuted by the same wall

Extracting the helpers into a `pdg_i2c-priv.{c,h}` pair with external linkage,
on the model of `drivers/i2c/i2c-priv.h` (which holds `i2c_map_dt_bitrate()`,
essentially our `freq_to_speed_`), does not help. `validate_group_` takes a
`const struct i2c_msg *`, and `<zephyr/drivers/i2c.h>` pulls `<zephyr/device.h>`
because the whole I2C API is expressed over `const struct device *`. There is no
way to reach the struct without the device model.

Redefining a local `struct i2c_msg` to dodge the header would mean testing a
type that production does not use. Rejected.

`i2c-priv.h`'s motivation is also worth noting: it exists for code sharing
between controllers, not testability, and `"i2c-priv.h" path:tests` returns
**0**. The structure is idiomatic; the motivation would have been novel anyway.

## 4. Chosen design - route B

A ztest suite on `native_sim`, which builds a **real** device model, driving the
public driver API against a substituted host-context bottom layer.

### 4.1 Why this is better than what it replaces

The testing *shape* has direct upstream precedent. `tests/drivers/i2c/i2c_emul/`
is a `native_sim` suite that instantiates hardware-free controllers via
devicetree and drives them through `i2c_transfer()`, asserting on FFF fakes at
the far end. It never reaches into `i2c_emul.c`'s statics.

It also yields *more* coverage than unit-testing the six statics would have.
Every branch of `validate_group_` is reachable by crafting a `struct i2c_msg[]`
and calling `i2c_transfer()`: N-write concatenation, mid-group read rejection,
double-read rejection, the missing `I2C_MSG_RESTART` case, and the overflow-safe
running total. Likewise `bufset_len_`, `flatten_tx_` and `unflatten_rx_` through
`spi_transceive()`.

And the fake is independently valuable. `docs/superpowers/plans/2026-08-11-zephyr-handoff.md`
at §4.2 item 3 argues it is what makes #101's real question - *what does a
zero-length write actually put on the wire?* - answerable at all, without a
bench peripheral and a logic analyser.

### 4.2 The substitution mechanism

The bottom layer is not linked into the Zephyr image. Each driver's
`CMakeLists.txt` adds it to the host context instead:

```cmake
# zephyr/drivers/i2c/CMakeLists.txt
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/pdg_i2c_bottom.c)
```

The embedded side reaches it only through `pdg_i2c_bottom.h`, which is
deliberately restricted to `stdint`/`stddef` types so that the embedded half
never includes the host-only `pico_de_gallo.h` (`pdg_i2c_bottom.h:9-11`). That
seam already exists and is already documented; nothing needs inventing.

**Decision: mark the production definitions `__attribute__((weak))` and let the
test link strong overrides.**

The rejected alternative was a Kconfig guard around the `target_sources` call:

```cmake
if(NOT CONFIG_PDG_STUB_BOTTOM)      # rejected
        target_sources(native_simulator INTERFACE pdg_i2c_bottom.c)
endif()
```

That puts a test-only conditional in shipping build files, which is exactly what
an upstream reviewer objects to and for which there is nothing to cite. The weak
approach leaves every production `CMakeLists.txt` untouched: the same files are
compiled, and the linker simply prefers the test's strong definitions.

It must be spelled `__attribute__((weak))`, **not** Zephyr's `__weak`. The
bottom files are compiled in the host context by the host compiler and must not
include Zephyr headers, so `zephyr/toolchain.h` is unavailable. Both GCC and
Clang support the attribute directly.

Honest costs: weak symbols inhibit some inlining, and an accidental duplicate
definition would silently win rather than producing a multiple-definition error.
Both are acceptable for six shim functions that exist only to cross a context
boundary.

### 4.3 Scope of the substitution

Only `zephyr/drivers/*/pdg_*_bottom.c` gain the attribute. In particular
`zephyr/drivers/common/gallo_registry.c` - which owns `gallo_init_strict()` - is
**not** in scope for this design. A suite that stubs the per-driver bottoms but
still boots the real registry would try to open a USB device. How the test
prevents the registry from running is an open question; see §6.

## 5. Verification obligations

`native_sim` is `type: native`, so twister *runs* the binary. A suite that
reaches the real FFI fails in CI, where no board is attached. Therefore:

- The suite must be a genuine ztest suite (not `build_only`), and must
  demonstrably never reach `gallo_*`.
- The stub must record what it was asked to do, so assertions are about observed
  calls rather than about return codes alone.
- The `twister` job added in #149 already runs `native_sim/native/64` and will
  pick the suite up. `-p unit_testing` was removed in `9e45272` and must not be
  restored without a `type: unit` suite that has been shown to build.

## 6. Open questions

1. **Does weak override work across the `native_simulator` link?** That context
   is built by a plain Makefile whose rule is `%.c -> %.o`, not by CMake
   (`zephyr/scripts/ci-build.sh` assertion 4 documents this). Weak/strong
   resolution should behave normally, but this is unverified and is the first
   thing to probe.
2. **How is `gallo_registry.c` prevented from opening a device?** Options
   include giving `gallo_init_strict()` the same weak treatment, or arranging
   the devicetree so no MFD parent is instantiated. Unresolved.
3. **Is a fixture gate viable for the board-attached suites?** #149 chose
   `build_only: true` on the assumption that a fixture-filtered instance is
   skipped rather than built, forfeiting build coverage. That assumption was
   never tested.
4. **Should `unittest.cmake` be patched upstream?** Adding `syscalls/device.h`
   to its TOUCH list is a two-line change, but run 3 shows it is not sufficient
   on its own - `syscall_macros.h` being empty is the deeper problem. A patch
   that genuinely enables driver unit tests is a larger piece of work and should
   not be promised on the strength of this investigation.

## 7. What this design does not do

- It does not unit-test the six helpers in isolation. It reaches their branches
  through the public API. If a branch turns out to be unreachable that way, that
  is a finding about the driver, not a gap in the test.
- It does not remove the need for `run-m5.sh`. A stubbed bottom proves the
  driver's logic, never the wire behaviour of real hardware.
- It does not address `checkpatch`, a devicetree-only tier, or the nightly
  self-hosted hardware tier. Those remain open on #109.

## 8. Documentation obligations

Per AGENTS.md §15.1 and its `zephyr/` carve-out: implementation updates
`zephyr/README.md` and `zephyr/CHANGELOG.md`. No `book/src/**` change is
expected, as nothing here touches an interface, status code or transfer limit
the book describes. If `__attribute__((weak))` lands on production bottom files,
`zephyr/README.md` must explain why, so the next reader does not delete it as
noise.
