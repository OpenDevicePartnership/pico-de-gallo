# Zephyr PWM Driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Zephyr `pwm` driver as a child of the `odp,pico-de-gallo` MFD parent, implementing `pwm_set_cycles()` and `pwm_get_cycles_per_sec()` over the existing `gallo_pwm_*` C FFI.

**Architecture:** Two-context split, exactly like the existing GPIO/UART drivers. `pdg_pwm.c` is the embedded (Zephyr) half and never includes `pico_de_gallo.h`; `pdg_pwm_bottom.c` is the host half, compiled into `native_simulator`, and never includes a Zephyr header. The two communicate through the FFI-free `pdg_pwm_bottom.h`. The bottom functions are `__attribute__((weak))` so the host-executed fake suite can link strong overrides and observe what the driver asked the device to do.

**Tech Stack:** Zephyr 4.4 (`native_sim/native/64`), C11, CMake/Kconfig/devicetree, ztest + twister, cbindgen-generated `pico_de_gallo.h`.

**Design spec:** `docs/superpowers/specs/2026-09-14-zephyr-pwm-driver-design.md`. Read it before starting.

---

## Background you need

Do not skip this. Several of these facts contradict issue #155.

- **PWM is not `hw-rev2`-gated.** `Capabilities::PWM` is set on both revisions. The capability gate below is correct practice but will never fire.
- **`max_duty == top + 1`**, not `top`. Legal compare range is `0..=max_duty`. A compare of 0 is always-low, `max_duty` is always-high.
- **The firmware silently clamps an over-range duty** rather than rejecting it. `PwmError::InvalidDutyCycle` is never returned.
- **`pwm/set-config` rescales both channels' compares with truncating integer division**, so duty ratios drift downward on every reconfiguration. The driver re-asserts instead of trusting this.
- **Channels 0+1 share RP2350 slice 6; channels 2+3 share slice 7.** `pwm/enable` and `pwm/disable` act on the whole slice.
- **Dividers above 255 panic the firmware** (issue #192). The driver's upper period bound exists to keep it out of that range. This is containment, not a fix.
- All nine PWM `Status` codes are **already** mapped in `zephyr/drivers/common/common.c`. **Do not modify that file.**

## Conversion, in one place

All arithmetic in `uint64_t`:

```
frequency_hz = ceil(150e6 / period_cycles)
             = (150e6 + period_cycles - 1) / period_cycles

compare      = (pulse_cycles * max_duty + period_cycles / 2) / period_cycles,
               clamped to max_duty
```

The **ceiling** on frequency is load-bearing: flooring lengthens the period, forcing a larger divider, and at the maximum period it yields 8 Hz, which needs divider 287 and panics. Ceiling yields 9 Hz, which needs 255. The **round-half-up** on compare avoids compounding a downward bias with the firmware's own truncating rescale.

## Verification environment

The Zephyr module only builds on Linux (`ARCH_POSIX`). Use WSL, which already has the toolchain at the **exact revision CI pins** (`26f811ee9d0`).

Define this once and reuse it; every verification step below refers to it as `TWISTER`:

```bash
wsl -d Ubuntu-26.04 -- bash -lc 'cd ~/zephyrproject/zephyr && source ../.venv/bin/activate; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr ZEPHYR_TOOLCHAIN_VARIANT=host; ./scripts/twister -p native_sim/native/64 -T /mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_fake/pwm --inline-logs --jobs 1'
```

`--jobs 1` is not caution: Corrosion runs `cargo install cbindgen`, and concurrent installs race. Keep it.

A known-good baseline (the pre-existing I2C fake suite) runs in ~43 s:

```bash
wsl -d Ubuntu-26.04 -- bash -lc 'cd ~/zephyrproject/zephyr && source ../.venv/bin/activate; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr ZEPHYR_TOOLCHAIN_VARIANT=host; ./scripts/twister -p native_sim/native/64 -T /mnt/d/workspace/pico-de-gallo/zephyr/tests/pdg_fake/i2c --inline-logs --jobs 1'
```

## Line endings

**Every file you create must be LF.** After creating any file, run:

```bash
dos2unix <path>
```

CRLF silently breaks `actionlint` and produces whole-file diffs. This is AGENTS.md §3 and is not optional.

## Commit conventions

Conventional Commits, scope `zephyr`. Every commit must carry these two trailers and **must not** carry `Signed-off-by`:

```
Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

## File Structure

| File | Responsibility |
|---|---|
| `zephyr/drivers/pwm/pdg_pwm_bottom.h` | FFI-free contract between the two halves; the cycles-per-second, channel-count, and period-bound constants. Declares no `disable` function, structurally preventing the sibling-kill. |
| `zephyr/drivers/pwm/pdg_pwm_bottom.c` | Host half. Weak wrappers over `gallo_pwm_*`, each routed through `pdg_common_status_to_errno()`. |
| `zephyr/drivers/pwm/pdg_pwm.c` | Embedded half. Validation, conversion, slice bookkeeping, the Zephyr PWM API. |
| `zephyr/drivers/pwm/Kconfig` | `PWM_PICO_DE_GALLO` + init priority. |
| `zephyr/drivers/pwm/CMakeLists.txt` | The two-context split. |
| `zephyr/dts/bindings/pwm/odp,pico-de-gallo-pwm.yaml` | Binding + the documented constraints. |
| `zephyr/tests/pdg_fake/pwm/` | Host-executed suite. The primary evidence this works. |
| `zephyr/samples/pwm_fade/` | Build-only LED fade demo. |

---

## Task 1: Binding, Kconfig, CMake wiring, and the bottom half

This task produces a driver that builds and initialises but refuses every operation. It exists so later tasks have something to test against.

**Files:**
- Create: `zephyr/dts/bindings/pwm/odp,pico-de-gallo-pwm.yaml`
- Create: `zephyr/drivers/pwm/pdg_pwm_bottom.h`
- Create: `zephyr/drivers/pwm/pdg_pwm_bottom.c`
- Create: `zephyr/drivers/pwm/Kconfig`
- Create: `zephyr/drivers/pwm/CMakeLists.txt`
- Modify: `zephyr/drivers/Kconfig`
- Modify: `zephyr/drivers/CMakeLists.txt`
- Modify: `zephyr/Kconfig`
- Modify: `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay`

- [ ] **Step 1: Create the binding**

Create `zephyr/dts/bindings/pwm/odp,pico-de-gallo-pwm.yaml`:

```yaml
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

description: |
  PWM controller bridged over USB by a Pico de Gallo board.

  Must be a direct child of an odp,pico-de-gallo parent, which owns the host
  connection and the serial-number selector. This node must not carry a
  selector of its own.

  Four channels, 0 to 3, mapping to GPIO12 through GPIO15 on the Pico 2
  header.

  SLICE SHARING. The RP2350 groups channels into slices of two: channels 0
  and 1 share one slice, channels 2 and 3 share another. A slice has a single
  period. Two channels on the same slice therefore CANNOT hold independent
  periods, and this driver refuses the attempt with -EINVAL rather than
  silently changing a period its owner never asked for. Plan for one period
  per slice, or use channels 0 and 2 if two independent periods are required.

  NO DISABLE. The firmware's disable operation acts on a whole slice, so
  disabling one channel would stop its sibling mid-operation. This driver
  therefore enables a slice on first use and never disables it. A pulse width
  of zero already produces a constant-low output, which is what "off" means
  here.

  UNSUPPORTED FLAGS. PWM_POLARITY_INVERTED returns -ENOTSUP. It is not
  emulated by inverting the duty cycle, because that changes the idle level
  rather than the polarity.

  PERIOD BOUNDS. pwm_get_cycles_per_sec() reports the 150 MHz PWM source
  clock, so period_cycles are source-clock ticks. A period below 2 cycles is
  refused with -EINVAL. A period above 16711680 cycles (roughly 9 Hz) is
  refused with -ENOTSUP: a longer period would require a clock divider the
  firmware cannot program, and requesting one panics the firmware. See
  https://github.com/OpenDevicePartnership/pico-de-gallo/issues/192.

  PHASE-CORRECT mode is never used. Zephyr has no corresponding concept, and
  in that mode the firmware's reported full-scale duty no longer corresponds
  to the period.

compatible: "odp,pico-de-gallo-pwm"

include: [pwm-controller.yaml, base.yaml]

properties:
  "#pwm-cells":
    const: 3

pwm-cells:
  - channel
  - period
  - flags
```

- [ ] **Step 2: Create the bottom-half header**

Create `zephyr/drivers/pwm/pdg_pwm_bottom.h`:

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * FFI-free interface between the two halves of the Pico de Gallo PWM driver.
 *
 * Only basic C types appear here so that the embedded side never needs to
 * include the host-only pico_de_gallo.h. All functions return 0 on success or
 * a negative POSIX errno on failure.
 *
 * There is deliberately no open/close pair: this controller is born into MFD
 * ownership and only ever borrows the parent's context.
 *
 * There is ALSO deliberately no disable function. The firmware's disable acts
 * on a whole slice, so disabling one channel stops its sibling. Not declaring
 * it means the top half cannot call it by accident -- the omission is the
 * enforcement, not a comment asking future maintainers to be careful.
 */

#ifndef PDG_PWM_BOTTOM_H
#define PDG_PWM_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * PWM source clock in Hz, reported verbatim by pwm_get_cycles_per_sec().
 *
 * SOURCE OF TRUTH: crates/pico-de-gallo-firmware/src/context.rs, SYS_CLK_HZ.
 * That constant is pub(crate): it is not on the wire and not in
 * pico_de_gallo.h, so this value cannot be tied to it by a _Static_assert and
 * is a genuine duplication. If the firmware's system clock ever changes, this
 * must change with it.
 *
 * This is the SOURCE clock, not the counter rate. The counter advances at
 * 150e6/divider, which changes with every frequency change and differs
 * between the two slices, so it could never serve as the stable per-device
 * constant Zephyr's API requires. Reporting the source clock makes
 * period_cycles independent of the divider -- which matters, because the
 * divider is not on the wire and the driver cannot observe it.
 */
#define PDG_PWM_CYCLES_PER_SEC 150000000U

/*
 * Channels exposed by the firmware. Mirrors NUM_PWM_CHANNELS in
 * pico-de-gallo-internal. Not assertable: unlike GALLO_NUM_GPIOS, the PWM
 * channel count is not exported to C at all.
 */
#define PDG_PWM_NUM_CHANNELS 4U

/* Channels per RP2350 slice. Channels 0+1 share one, 2+3 share another. */
#define PDG_PWM_CHANNELS_PER_SLICE 2U

#define PDG_PWM_NUM_SLICES (PDG_PWM_NUM_CHANNELS / PDG_PWM_CHANNELS_PER_SLICE)

/*
 * Largest clock divider the RP2350 CH_DIV register can hold in its integer
 * part. The firmware searches up to 4095 and panics above this; see issue
 * #192. The period bound below exists solely to keep requests out of that
 * range.
 */
#define PDG_PWM_MAX_DIVIDER 255U

/*
 * Period bounds in source-clock cycles.
 *
 * Lower: the firmware requires top >= 1, so a period of one cycle is
 * unreachable.
 *
 * Upper: 255 * 65536. Conservative by one counter step -- the exact limit is
 * 255 * 65537 -- so that the bound is obviously safe by inspection.
 */
#define PDG_PWM_MIN_PERIOD_CYCLES 2U
#define PDG_PWM_MAX_PERIOD_CYCLES ((uint64_t)PDG_PWM_MAX_DIVIDER * 65536U)

/*
 * Report whether the attached board advertises the PWM capability.
 *
 * Returns 0 and writes *out_has_pwm on success; a negative errno if the query
 * itself failed. The two shapes are distinct and the caller depends on it:
 * "answered, has no PWM" is -ENODEV, while "did not answer" reports its own
 * errno. *out_has_pwm is written only on success.
 */
int pdg_pwm_bottom_has_pwm(void *ctx, bool *out_has_pwm);

/* Set the frequency of the slice owning `channel`. Reconfigures the whole
 * slice, including the sibling channel.
 */
int pdg_pwm_bottom_set_config(void *ctx, uint8_t channel, uint32_t frequency_hz,
			      bool phase_correct);

/* Read the current and full-scale compare values. *out_max_duty is top + 1. */
int pdg_pwm_bottom_get_duty_cycle(void *ctx, uint8_t channel, uint16_t *out_duty,
				  uint16_t *out_max_duty);

/* Set the raw compare value. 0 is always-low; max_duty is always-high. */
int pdg_pwm_bottom_set_duty_cycle(void *ctx, uint8_t channel, uint16_t duty);

/* Enable the slice owning `channel`. There is no counterpart by design. */
int pdg_pwm_bottom_enable(void *ctx, uint8_t channel);

#ifdef __cplusplus
}
#endif

#endif /* PDG_PWM_BOTTOM_H */
```

- [ ] **Step 3: Create the bottom-half implementation**

Create `zephyr/drivers/pwm/pdg_pwm_bottom.c`:

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the Pico de Gallo PWM driver.
 *
 * Compiled into the native simulator runner with the host C library, linking
 * against the Pico de Gallo FFI. It must not include any Zephyr header.
 */

#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "pico_de_gallo.h"
#include "common.h"
#include "pdg_pwm_bottom.h"

/*
 * These are __attribute__((weak)) so the recording fake in
 * zephyr/tests/pdg_fake/pwm can link strong definitions and observe what the
 * driver asked the device to do. Not Zephyr's __weak: this file is
 * host-context and cannot include zephyr/toolchain.h.
 *
 * Every FFI return is routed through pdg_common_status_to_errno(). cbindgen
 * emits "typedef int32_t Status" under C11/C17, so returning a raw Status
 * compiles silently and hands the top half a positive, unmapped number that it
 * would then treat as a negative POSIX errno.
 */

__attribute__((weak)) int pdg_pwm_bottom_has_pwm(void *ctx, bool *out_has_pwm)
{
	struct GalloDeviceInfo info;
	int ret;

	if (out_has_pwm == NULL) {
		return -EINVAL;
	}

	/*
	 * `info` is a bare stack struct and the FFI leaves it untouched when
	 * the query fails, so capabilities is read only after the status has
	 * been confirmed successful.
	 */
	ret = pdg_common_status_to_errno(
		gallo_get_device_info((const struct PicoDeGallo *)ctx, &info));
	if (ret != 0) {
		return ret;
	}

	*out_has_pwm = (info.capabilities & GALLO_CAP_PWM) != 0U;

	return 0;
}

__attribute__((weak)) int pdg_pwm_bottom_set_config(void *ctx, uint8_t channel,
						    uint32_t frequency_hz,
						    bool phase_correct)
{
	return pdg_common_status_to_errno(
		gallo_pwm_set_config((const struct PicoDeGallo *)ctx, channel,
				     frequency_hz, phase_correct));
}

__attribute__((weak)) int pdg_pwm_bottom_get_duty_cycle(void *ctx, uint8_t channel,
							uint16_t *out_duty,
							uint16_t *out_max_duty)
{
	return pdg_common_status_to_errno(
		gallo_pwm_get_duty_cycle((const struct PicoDeGallo *)ctx, channel,
					 out_duty, out_max_duty));
}

__attribute__((weak)) int pdg_pwm_bottom_set_duty_cycle(void *ctx, uint8_t channel,
							uint16_t duty)
{
	return pdg_common_status_to_errno(
		gallo_pwm_set_duty_cycle((const struct PicoDeGallo *)ctx, channel,
					 duty));
}

__attribute__((weak)) int pdg_pwm_bottom_enable(void *ctx, uint8_t channel)
{
	return pdg_common_status_to_errno(
		gallo_pwm_enable((const struct PicoDeGallo *)ctx, channel));
}
```

Note there is no `gallo_pwm_disable` wrapper. That is deliberate; see the header.

- [ ] **Step 4: Create the driver Kconfig**

Create `zephyr/drivers/pwm/Kconfig`:

```kconfig
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

config PWM_PICO_DE_GALLO
	bool "Pico de Gallo PWM controller"
	default y
	depends on DT_HAS_ODP_PICO_DE_GALLO_PWM_ENABLED
	depends on ARCH_POSIX
	select PWM
	help
	  Enable the Pico de Gallo PWM controller driver. The controller is a
	  child of an odp,pico-de-gallo MFD parent and reaches the board over
	  USB, so every operation is a blocking host round trip.

if PWM_PICO_DE_GALLO

config PWM_PICO_DE_GALLO_INIT_PRIORITY
	int "Pico de Gallo PWM controller initialization priority"
	default 45
	help
	  POST_KERNEL initialization priority of the Pico de Gallo PWM
	  controller. It must be greater than
	  CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY (default 40) so the parent has
	  opened its host connection first. PWM has no cross-child dependency,
	  so nothing constrains it from above.

endif # PWM_PICO_DE_GALLO
```

- [ ] **Step 5: Create the driver CMakeLists**

Create `zephyr/drivers/pwm/CMakeLists.txt`:

```cmake
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

# Top half: embedded (Zephyr) context. Sees Zephyr headers only; it must never
# include pico_de_gallo.h.
zephyr_library()
zephyr_library_sources(pdg_pwm.c)
zephyr_library_include_directories(${CMAKE_CURRENT_LIST_DIR})

# Bottom half: native simulator (host) context. Sees the host libc and the
# generated pico_de_gallo.h; it must never include a Zephyr header.
#
# No target_include_directories(native_simulator INTERFACE ...) here.
# pdg_pwm_bottom.c quotes pdg_pwm_bottom.h from its own directory, which the
# preprocessor resolves relative to the including file, so no -I is needed. If
# a cross-directory header is ever required on the native side it must be
# supplied with target_compile_options(native_simulator INTERFACE "-I<dir>"),
# because natsim_config.cmake joins only INTERFACE_COMPILE_OPTIONS into
# NSI_BUILD_OPTIONS and the include-directories form silently vanishes.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/pdg_pwm_bottom.c)
```

- [ ] **Step 6: Wire the driver into the module build**

Modify `zephyr/drivers/Kconfig` — add one line so the list reads:

```kconfig
rsource "mfd/Kconfig"
rsource "gpio/Kconfig"
rsource "i2c/Kconfig"
rsource "pwm/Kconfig"
rsource "serial/Kconfig"
rsource "spi/Kconfig"
```

Modify `zephyr/drivers/CMakeLists.txt`. Add the subdirectory after the I2C line:

```cmake
add_subdirectory_ifdef(CONFIG_PWM_PICO_DE_GALLO pwm)
```

Then add `CONFIG_PWM_PICO_DE_GALLO` to the `if()` guarding `common.c`, so it reads:

```cmake
if(CONFIG_MFD_PICO_DE_GALLO OR CONFIG_GPIO_PICO_DE_GALLO OR
   CONFIG_I2C_PICO_DE_GALLO OR CONFIG_SPI_PICO_DE_GALLO OR
   CONFIG_UART_PICO_DE_GALLO OR CONFIG_PWM_PICO_DE_GALLO)
```

This second edit is mandatory, not cosmetic. That file's own comment explains why: `pdg_pwm_bottom.c` calls `pdg_common_status_to_errno()` directly, so a PWM-only application that disabled every other child would link against a `common.c` that was never compiled.

Modify `zephyr/Kconfig` line 7, appending the new symbol:

```kconfig
    default y if DT_HAS_ODP_PICO_DE_GALLO_ENABLED || DT_HAS_ODP_PICO_DE_GALLO_GPIO_ENABLED || DT_HAS_ODP_PICO_DE_GALLO_I2C_ENABLED || DT_HAS_ODP_PICO_DE_GALLO_PWM_ENABLED || DT_HAS_ODP_PICO_DE_GALLO_SPI_ENABLED || DT_HAS_ODP_PICO_DE_GALLO_UART_ENABLED
```

- [ ] **Step 7: Add the shield node**

Modify `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay`, adding this child inside the `pdg0` node, after `pdg_i2c0` and before `pdg_uart0` to keep the existing alphabetical-ish grouping:

```dts
		pdg_pwm0: pwm {
			compatible = "odp,pico-de-gallo-pwm";
			#pwm-cells = <3>;
			status = "disabled";
		};
```

- [ ] **Step 8: Create the stub top half**

Create `zephyr/drivers/pwm/pdg_pwm.c`. This is the full file structure with the two API entry points stubbed; Task 3 onward fills in `set_cycles`.

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Pico de Gallo PWM controller.
 *
 * Embedded (Zephyr) half. It must never include pico_de_gallo.h; everything
 * that reaches the FFI goes through pdg_pwm_bottom.h.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_pwm

#include <zephyr/device.h>
#include <zephyr/drivers/pwm.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

#include "pdg_pwm_bottom.h"

/*
 * Structural topology enforcement, ordered compatible -> parent status ->
 * parent serial presence -> Kconfig. That source order is the contract: the
 * first failing assertion is the one a misconfigured tree should see.
 *
 * The block precedes the "pdg_mfd.h" include on purpose: when
 * CONFIG_MFD_PICO_DE_GALLO is n the MFD subdirectory is not added to the build
 * at all, so pdg_mfd.h is not on the include path and a tree that got the
 * Kconfig wrong would fail with a confusing missing-header error instead of
 * the assertion written to explain it.
 */
#define PDG_PWM_PARENT_ASSERTS(inst)						\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_INST_PARENT(inst), odp_pico_de_gallo),	\
		"Enabled odp,pico-de-gallo-pwm controllers must be direct "	\
		"children of an odp,pico-de-gallo parent");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(DT_INST_PARENT(inst)),			\
		"Enabled odp,pico-de-gallo-pwm controllers require their "	\
		"odp,pico-de-gallo parent to have status okay");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_PROP(DT_INST_PARENT(inst), serial_number),		\
		"odp,pico-de-gallo-pwm parent must define serial-number");	\
	BUILD_ASSERT(								\
		IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO),				\
		"Enabled Pico de Gallo child controllers require "		\
		"CONFIG_MFD_PICO_DE_GALLO=y");

DT_INST_FOREACH_STATUS_OKAY(PDG_PWM_PARENT_ASSERTS)

#include "pdg_mfd.h"

LOG_MODULE_REGISTER(pwm_pico_de_gallo, CONFIG_PWM_LOG_LEVEL);

struct pdg_pwm_config {
	const struct device *mfd;
	const char *serial_number;
};

/* What a channel was last asked for. Needed to re-assert a sibling's duty
 * after a slice reconfiguration, and to detect a conflicting period.
 */
struct pdg_pwm_channel_state {
	uint32_t period_cycles;
	uint32_t pulse_cycles;
	bool configured;
};

/* Slice-wide state. A slice has one period and one enable. */
struct pdg_pwm_slice_state {
	uint32_t period_cycles;
	bool configured;
	bool enabled;
};

struct pdg_pwm_data {
	void *ctx;
	struct k_mutex lock;
	struct pdg_pwm_channel_state channels[PDG_PWM_NUM_CHANNELS];
	struct pdg_pwm_slice_state slices[PDG_PWM_NUM_SLICES];
};

static inline uint32_t pdg_pwm_slice_of(uint32_t channel)
{
	return channel / PDG_PWM_CHANNELS_PER_SLICE;
}

/* The other channel on the same slice. Valid only for a two-channel slice,
 * which is what PDG_PWM_CHANNELS_PER_SLICE asserts below.
 */
static inline uint32_t pdg_pwm_sibling_of(uint32_t channel)
{
	return channel ^ 1U;
}

BUILD_ASSERT(PDG_PWM_CHANNELS_PER_SLICE == 2U,
	     "pdg_pwm_sibling_of() computes the sibling with XOR 1, which is only "
	     "correct for two channels per slice");

static int pdg_pwm_get_cycles_per_sec(const struct device *dev, uint32_t channel,
				      uint64_t *cycles)
{
	struct pdg_pwm_data *data = dev->data;

	if (data->ctx == NULL) {
		return -ENODEV;
	}

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		LOG_ERR("%s: channel %u is out of range (0 to %u).", dev->name,
			channel, PDG_PWM_NUM_CHANNELS - 1U);
		return -EINVAL;
	}

	if (cycles == NULL) {
		return -EINVAL;
	}

	/*
	 * The SOURCE clock, constant for every channel and every
	 * configuration. See the rationale on PDG_PWM_CYCLES_PER_SEC.
	 */
	*cycles = (uint64_t)PDG_PWM_CYCLES_PER_SEC;

	return 0;
}

static int pdg_pwm_set_cycles(const struct device *dev, uint32_t channel,
			      uint32_t period_cycles, uint32_t pulse_cycles,
			      pwm_flags_t flags)
{
	ARG_UNUSED(dev);
	ARG_UNUSED(channel);
	ARG_UNUSED(period_cycles);
	ARG_UNUSED(pulse_cycles);
	ARG_UNUSED(flags);

	return -ENOSYS;
}

static DEVICE_API(pwm, pdg_pwm_api) = {
	.set_cycles = pdg_pwm_set_cycles,
	.get_cycles_per_sec = pdg_pwm_get_cycles_per_sec,
};

static int pdg_pwm_init(const struct device *dev)
{
	const struct pdg_pwm_config *config = dev->config;
	struct pdg_pwm_data *data = dev->data;
	bool has_pwm = false;
	int ret;

	/*
	 * The mutex and every piece of mutable state are initialized before any
	 * early return, so a device object that exists at all has a usable
	 * lock. Zephyr's PWM wrappers dispatch straight into this API without a
	 * readiness check, so a direct call on a failed device must find an
	 * initialized mutex; the data->ctx == NULL guard at the top of each
	 * callback then turns that call into a safe refusal.
	 */
	k_mutex_init(&data->lock);

	data->ctx = NULL;
	memset(data->channels, 0, sizeof(data->channels));
	memset(data->slices, 0, sizeof(data->slices));

	/*
	 * Mandatory MFD child sequence (pdg_mfd.h): require parent readiness
	 * first, then borrow the context. A NULL context after a passing
	 * readiness check is an ownership invariant failure, not an expected
	 * case, so it is logged distinctly. The context is borrowed: this
	 * driver must never close or free it.
	 */
	if (!device_is_ready(config->mfd)) {
		LOG_ERR("%s: Pico de Gallo parent %s is not ready. Returning -ENODEV.",
			dev->name, config->mfd->name);
		return -ENODEV;
	}

	data->ctx = pdg_mfd_ctx(config->mfd);
	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo parent %s is ready but returned a NULL context; "
			"this is an MFD ownership invariant failure. Returning -ENODEV.",
			dev->name, config->mfd->name);
		return -ENODEV;
	}

	/*
	 * Capability gate.
	 *
	 * Every shipped firmware advertises PWM on both hardware revisions, so
	 * this will not fire today. It is here because the capability bit is
	 * the contract, and a future image that drops PWM should be refused
	 * loudly rather than accepted into silence.
	 *
	 * This is NOT a warm local read. gallo_get_device_info() re-validates
	 * unconditionally, issuing a fresh device/info RPC, so it costs a
	 * metadata round trip on top of the one the parent already paid.
	 *
	 * The two failure shapes stay distinct: a failed query reports its own
	 * mapped errno, because "the board did not answer" is a different
	 * problem from "the board answered and has no PWM".
	 */
	ret = pdg_pwm_bottom_has_pwm(data->ctx, &has_pwm);
	if (ret < 0) {
		LOG_ERR("%s: failed to read the firmware capability bits: errno=%d.",
			dev->name, ret);
		/*
		 * Defensive invalidation of this child's cached borrow -- never
		 * a reference release. The parent holds the sole registry
		 * reference; releasing it here would leave the parent and the
		 * siblings holding a freed pointer. NULL is guardable; a
		 * valid-looking unowned pointer would bypass every NULL check.
		 */
		data->ctx = NULL;
		return ret;
	}

	if (!has_pwm) {
		LOG_ERR("%s: the attached Pico de Gallo does not advertise the PWM "
			"capability. Returning -ENODEV.", dev->name);
		data->ctx = NULL;
		return -ENODEV;
	}

	LOG_INF("%s: ready on Pico de Gallo serial-number \"%s\" with %u channels "
		"at %u Hz.", dev->name, config->serial_number,
		PDG_PWM_NUM_CHANNELS, PDG_PWM_CYCLES_PER_SEC);

	return 0;
}

#define PDG_PWM_INIT(inst)							\
	static struct pdg_pwm_data pdg_pwm_data_##inst;				\
										\
	static const struct pdg_pwm_config pdg_pwm_config_##inst = {		\
		.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst)),			\
		.serial_number = DT_PROP(DT_INST_PARENT(inst), serial_number),	\
	};									\
										\
	DEVICE_DT_INST_DEFINE(inst, pdg_pwm_init, NULL,				\
			      &pdg_pwm_data_##inst,				\
			      &pdg_pwm_config_##inst, POST_KERNEL,		\
			      CONFIG_PWM_PICO_DE_GALLO_INIT_PRIORITY,		\
			      &pdg_pwm_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_PWM_INIT)
```

Add `#include <string.h>` after the Zephyr includes if `memset` is not already reachable; Zephyr's `kernel.h` normally pulls it in, but be explicit rather than relying on it.

- [ ] **Step 9: Normalize line endings**

```bash
dos2unix zephyr/dts/bindings/pwm/odp,pico-de-gallo-pwm.yaml zephyr/drivers/pwm/pdg_pwm.c zephyr/drivers/pwm/pdg_pwm_bottom.c zephyr/drivers/pwm/pdg_pwm_bottom.h zephyr/drivers/pwm/Kconfig zephyr/drivers/pwm/CMakeLists.txt
```

- [ ] **Step 10: Verify it builds**

There is no test yet, so verification is a build of the existing I2C fake suite plus a scratch build with PWM enabled. Run the baseline first to confirm you have not broken the module:

Run the baseline twister command from "Verification environment".
Expected: `1 of 1 executed test configurations passed`.

- [ ] **Step 11: Commit**

```bash
git add zephyr/dts/bindings/pwm zephyr/drivers/pwm zephyr/drivers/Kconfig zephyr/drivers/CMakeLists.txt zephyr/Kconfig zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay
git commit -F - <<'MSG'
feat(zephyr): Add the PWM driver skeleton and bottom half

Adds the odp,pico-de-gallo-pwm binding, the two-context driver split, and
the Kconfig/CMake wiring. pwm_get_cycles_per_sec() is complete and reports
the 150 MHz PWM source clock; pwm_set_cycles() is stubbed and returns
-ENOSYS until the following commits.

The bottom half declares no disable function. The firmware's disable acts
on a whole RP2350 slice, so disabling one channel would stop its sibling;
omitting the declaration means the top half cannot call it by accident.

CONFIG_PWM_PICO_DE_GALLO joins the disjunction in drivers/CMakeLists.txt
that compiles common.c, because pdg_pwm_bottom.c calls
pdg_common_status_to_errno() directly and a PWM-only tree would otherwise
link against a common.c that was never compiled.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## Task 2: The recording fake and the first test

The fake models the firmware's divider search faithfully. A fake returning a fixed `max_duty` would pass while proving nothing about the conversion, because `max_duty` is precisely the quantity the conversion depends on.

**Files:**
- Create: `zephyr/tests/pdg_fake/pwm/pdg_pwm_fake_bottom.h`
- Create: `zephyr/tests/pdg_fake/pwm/pdg_pwm_fake_bottom.c`
- Create: `zephyr/tests/pdg_fake/pwm/CMakeLists.txt`
- Create: `zephyr/tests/pdg_fake/pwm/prj.conf`
- Create: `zephyr/tests/pdg_fake/pwm/fake.overlay`
- Create: `zephyr/tests/pdg_fake/pwm/tests.yaml`
- Create: `zephyr/tests/pdg_fake/pwm/src/main.c`

- [ ] **Step 1: Create the fake header**

Create `zephyr/tests/pdg_fake/pwm/pdg_pwm_fake_bottom.h`:

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Embedded-facing view of the PWM recording fake.
 *
 * native_sim splits into an embedded context (Zephyr, src/main.c) and a host
 * context (the native simulator runner). The fake lives in the host context
 * because that is where the production bottom half lives, and the ztest
 * assertions live in the embedded context. The two share NO globals, so
 * everything a test observes crosses this boundary through an accessor that
 * copies scalars rather than handing back a pointer into host storage.
 *
 * This header is FFI-free -- stdbool/stddef/stdint only.
 */

#ifndef PDG_PWM_FAKE_BOTTOM_H
#define PDG_PWM_FAKE_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PDG_PWM_FAKE_LOG_MAX 32

/* Discard all recorded calls and reset scripted results. Call from setup. */
void pdg_pwm_fake_reset(void);

/* Call counts. */
int pdg_pwm_fake_set_config_count(void);
int pdg_pwm_fake_get_duty_count(void);
int pdg_pwm_fake_set_duty_count(void);
int pdg_pwm_fake_enable_count(void);

/*
 * How many times the production gallo_pwm_disable path was reached.
 *
 * The EXPECTED VALUE IS ZERO AND ALWAYS WILL BE. pdg_pwm_bottom.h declares no
 * disable function, so the top half cannot call one. This counter exists so
 * that a future change which adds a disable wrapper is caught by a failing
 * test rather than silently stopping a sibling channel. It is incremented by
 * a strong definition of pdg_pwm_bottom_disable(), which currently overrides
 * nothing.
 */
int pdg_pwm_fake_disable_count(void);

/* The most recent set_config. Returns 0, or -1 if there was none. */
int pdg_pwm_fake_last_set_config(uint8_t *out_channel, uint32_t *out_frequency_hz,
				 bool *out_phase_correct);

/* Entry `index` of the set_duty log. Returns 0, or -1 if out of range. */
int pdg_pwm_fake_set_duty_log_len(void);
int pdg_pwm_fake_set_duty_log_entry(int index, uint8_t *out_channel,
				    uint16_t *out_duty);

/*
 * The full-scale duty the fake would report for `channel` right now, i.e.
 * top + 1 for the frequency most recently configured on that slice. Lets a
 * test compute the compare value it expects without duplicating the firmware
 * model in the embedded half.
 *
 * Returns 0 and writes *out_max_duty, or -1 if the slice was never
 * configured.
 */
int pdg_pwm_fake_max_duty_for(uint8_t channel, uint16_t *out_max_duty);

/* Script a failure for the next and subsequent calls. 0 restores success. */
void pdg_pwm_fake_set_set_config_result(int result);
void pdg_pwm_fake_set_set_duty_result(int result);
void pdg_pwm_fake_set_enable_result(int result);

/* Non-zero if a log overflowed; a test that overran must fail, not assert
 * against a prefix.
 */
int pdg_pwm_fake_overflowed(void);

#ifdef __cplusplus
}
#endif

#endif /* PDG_PWM_FAKE_BOTTOM_H */
```

- [ ] **Step 2: Create the fake implementation**

Create `zephyr/tests/pdg_fake/pwm/pdg_pwm_fake_bottom.c`:

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Recording fake for the PWM host-context bottom layer.
 *
 * Provides STRONG definitions of the symbols
 * zephyr/drivers/pwm/pdg_pwm_bottom.c defines as weak, so the linker prefers
 * these and no production CMakeLists.txt needs a test-only conditional. It
 * deliberately does NOT link pico_de_gallo.h.
 *
 * WHY EVERY OVERRIDE MUST EXIST, AND WHY NONE MAY TOUCH ctx
 * --------------------------------------------------------
 * The shared parent fake hands back the address of a private int to satisfy
 * pdg_mfd_init()'s NULL check. That token is an OPAQUE NON-POINTER: it is not
 * a PicoDeGallo * and does not point at one. A symbol we fail to override
 * resolves to the real weak production definition, which casts the token and
 * passes it into the Rust FFI, which dereferences it. So every bottom symbol
 * is defined here, unconditionally, and each ignores ctx with an explicit
 * (void) cast.
 *
 * ctx could not be used for attribution in any case: the MFD parent hands
 * every child the same borrowed token.
 *
 * pdg_common_bottom_open()/_close() are NOT defined here -- they belong to the
 * shared parent fake in ../common/pdg_fake_bottom.c, which this suite links.
 * Defining them twice is a duplicate-symbol link error.
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/*
 * The declaration site of the symbols overridden below. Included rather than
 * hand-copied so a signature drift is a compile error instead of undefined
 * behaviour that links cleanly. FFI-free, so it drags nothing in.
 */
#include "pdg_pwm_bottom.h"

#include "pdg_pwm_fake_bottom.h"

/*
 * The firmware's system clock, restated here so the model below can be read
 * as the firmware's own arithmetic. Tied to the driver's constant so the two
 * cannot drift; this is the one translation unit that sees both.
 */
#define FAKE_SYS_CLK_HZ 150000000ULL

_Static_assert(FAKE_SYS_CLK_HZ == (unsigned long long)PDG_PWM_CYCLES_PER_SEC,
	       "the fake's firmware model must use the same system clock the "
	       "driver reports as cycles-per-second");

struct fake_duty_entry {
	uint8_t channel;
	uint16_t duty;
};

/* Per-slice model state: what the firmware would hold after set_config. */
static uint16_t slice_top[PDG_PWM_NUM_SLICES];
static bool slice_configured[PDG_PWM_NUM_SLICES];
static uint16_t slice_compare[PDG_PWM_NUM_CHANNELS];

static int set_config_count;
static int get_duty_count;
static int set_duty_count;
static int enable_count;
static int disable_count;

static int have_last_set_config;
static uint8_t last_cfg_channel;
static uint32_t last_cfg_frequency;
static bool last_cfg_phase_correct;

static struct fake_duty_entry duty_log[PDG_PWM_FAKE_LOG_MAX];
static int duty_log_len;

static int sc_set_config_result;
static int sc_set_duty_result;
static int sc_enable_result;

static int overflowed;

/*
 * Faithful model of compute_pwm_params() in
 * crates/pico-de-gallo-firmware/src/handlers/pwm.rs.
 *
 * Ascending divider search, first acceptable wins, so the smallest divider is
 * chosen and top is maximised. Non-phase-correct only, which is all the
 * driver ever requests.
 *
 * The search bound is 255, NOT the firmware's 4095. The firmware's range is a
 * defect (issue #192): above 255 it writes a divider embassy-rp panics on.
 * Modelling 4095 here would let a test "pass" on a configuration that bricks
 * a real board, so the model stops where the hardware does.
 *
 * Returns 0 and writes *out_top, or -1 if no divider satisfies the request.
 */
static int model_compute_top(uint32_t frequency_hz, uint16_t *out_top)
{
	if (frequency_hz == 0U) {
		return -1;
	}

	for (uint64_t div = 1U; div <= PDG_PWM_MAX_DIVIDER; div++) {
		uint64_t denom = div * (uint64_t)frequency_hz;
		uint64_t raw = FAKE_SYS_CLK_HZ / denom;

		if (raw == 0U) {
			/* Larger dividers only shrink the quotient. */
			return -1;
		}

		uint64_t top = raw - 1U;

		if (top > 0U && top <= 65535U) {
			*out_top = (uint16_t)top;
			return 0;
		}
	}

	return -1;
}

void pdg_pwm_fake_reset(void)
{
	memset(slice_top, 0, sizeof(slice_top));
	memset(slice_configured, 0, sizeof(slice_configured));
	memset(slice_compare, 0, sizeof(slice_compare));

	set_config_count = 0;
	get_duty_count = 0;
	set_duty_count = 0;
	enable_count = 0;
	disable_count = 0;

	have_last_set_config = 0;
	last_cfg_channel = 0U;
	last_cfg_frequency = 0U;
	last_cfg_phase_correct = false;

	duty_log_len = 0;
	memset(duty_log, 0, sizeof(duty_log));

	sc_set_config_result = 0;
	sc_set_duty_result = 0;
	sc_enable_result = 0;

	overflowed = 0;
}

/* --- strong overrides --- */

int pdg_pwm_bottom_has_pwm(void *ctx, bool *out_has_pwm)
{
	(void)ctx; /* opaque non-pointer token; never dereferenced */

	if (out_has_pwm != NULL) {
		*out_has_pwm = true;
	}

	return 0;
}

int pdg_pwm_bottom_set_config(void *ctx, uint8_t channel, uint32_t frequency_hz,
			      bool phase_correct)
{
	uint16_t top = 0U;

	(void)ctx;

	set_config_count++;

	/* Recorded even when scripted to fail: a test proving the driver did
	 * not update its cache must first prove it issued the request.
	 */
	have_last_set_config = 1;
	last_cfg_channel = channel;
	last_cfg_frequency = frequency_hz;
	last_cfg_phase_correct = phase_correct;

	if (sc_set_config_result != 0) {
		return sc_set_config_result;
	}

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		return -EINVAL_FAKE;
	}

	if (model_compute_top(frequency_hz, &top) != 0) {
		/* What the firmware would report as InvalidConfiguration,
		 * mapped by pdg_common_status_to_errno() to -EINVAL.
		 */
		return -EINVAL_FAKE;
	}

	slice_top[pdg_pwm_fake_slice_of(channel)] = top;
	slice_configured[pdg_pwm_fake_slice_of(channel)] = true;

	return 0;
}

int pdg_pwm_bottom_get_duty_cycle(void *ctx, uint8_t channel, uint16_t *out_duty,
				  uint16_t *out_max_duty)
{
	uint32_t slice;

	(void)ctx;

	get_duty_count++;

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		return -EINVAL_FAKE;
	}

	slice = pdg_pwm_fake_slice_of(channel);

	if (out_duty != NULL) {
		*out_duty = slice_compare[channel];
	}

	if (out_max_duty != NULL) {
		/*
		 * max_duty is top + 1. Before any set_config the firmware's
		 * boot default is top = 0xFFFF, whose +1 saturates to 0xFFFF.
		 * Model that, because the driver may legitimately read it.
		 */
		if (!slice_configured[slice]) {
			*out_max_duty = 65535U;
		} else if (slice_top[slice] == 65535U) {
			*out_max_duty = 65535U;
		} else {
			*out_max_duty = (uint16_t)(slice_top[slice] + 1U);
		}
	}

	return 0;
}

int pdg_pwm_bottom_set_duty_cycle(void *ctx, uint8_t channel, uint16_t duty)
{
	(void)ctx;

	set_duty_count++;

	if (duty_log_len >= PDG_PWM_FAKE_LOG_MAX) {
		overflowed = 1;
	} else {
		duty_log[duty_log_len].channel = channel;
		duty_log[duty_log_len].duty = duty;
		duty_log_len++;
	}

	if (sc_set_duty_result != 0) {
		return sc_set_duty_result;
	}

	if (channel < PDG_PWM_NUM_CHANNELS) {
		slice_compare[channel] = duty;
	}

	return 0;
}

int pdg_pwm_bottom_enable(void *ctx, uint8_t channel)
{
	(void)ctx;
	(void)channel;

	enable_count++;

	return sc_enable_result;
}

/*
 * Strong definition of a symbol the production bottom half deliberately does
 * NOT declare. It overrides nothing today and can only be reached if someone
 * adds a disable wrapper, which is exactly the regression the counter guards.
 */
int pdg_pwm_bottom_disable(void *ctx, uint8_t channel);

int pdg_pwm_bottom_disable(void *ctx, uint8_t channel)
{
	(void)ctx;
	(void)channel;

	disable_count++;

	return 0;
}

/* --- accessors --- */

int pdg_pwm_fake_set_config_count(void) { return set_config_count; }
int pdg_pwm_fake_get_duty_count(void) { return get_duty_count; }
int pdg_pwm_fake_set_duty_count(void) { return set_duty_count; }
int pdg_pwm_fake_enable_count(void) { return enable_count; }
int pdg_pwm_fake_disable_count(void) { return disable_count; }
int pdg_pwm_fake_overflowed(void) { return overflowed; }
int pdg_pwm_fake_set_duty_log_len(void) { return duty_log_len; }

int pdg_pwm_fake_last_set_config(uint8_t *out_channel, uint32_t *out_frequency_hz,
				 bool *out_phase_correct)
{
	if (!have_last_set_config) {
		return -1;
	}

	if (out_channel != NULL) {
		*out_channel = last_cfg_channel;
	}
	if (out_frequency_hz != NULL) {
		*out_frequency_hz = last_cfg_frequency;
	}
	if (out_phase_correct != NULL) {
		*out_phase_correct = last_cfg_phase_correct;
	}

	return 0;
}

int pdg_pwm_fake_set_duty_log_entry(int index, uint8_t *out_channel, uint16_t *out_duty)
{
	if (index < 0 || index >= duty_log_len) {
		return -1;
	}

	if (out_channel != NULL) {
		*out_channel = duty_log[index].channel;
	}
	if (out_duty != NULL) {
		*out_duty = duty_log[index].duty;
	}

	return 0;
}

int pdg_pwm_fake_max_duty_for(uint8_t channel, uint16_t *out_max_duty)
{
	uint32_t slice;

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		return -1;
	}

	slice = pdg_pwm_fake_slice_of(channel);

	if (!slice_configured[slice]) {
		return -1;
	}

	if (out_max_duty != NULL) {
		*out_max_duty = (slice_top[slice] == 65535U)
			? 65535U
			: (uint16_t)(slice_top[slice] + 1U);
	}

	return 0;
}

void pdg_pwm_fake_set_set_config_result(int result) { sc_set_config_result = result; }
void pdg_pwm_fake_set_set_duty_result(int result) { sc_set_duty_result = result; }
void pdg_pwm_fake_set_enable_result(int result) { sc_enable_result = result; }
```

**Two loose ends you must close while writing this file.** The draft above uses `EINVAL_FAKE` and `pdg_pwm_fake_slice_of()`, neither of which is defined. Resolve them as follows rather than inventing something else:

1. Add `#include <errno.h>` and use plain `-EINVAL`. The host context has errno.h; `pdg_gpio_bottom.c` and `pdg_uart_bottom.c` both include it. Replace every `-EINVAL_FAKE` with `-EINVAL`.
2. Add a file-local helper next to `model_compute_top`, and use it everywhere `pdg_pwm_fake_slice_of` appears:

```c
static inline uint32_t pdg_pwm_fake_slice_of(uint8_t channel)
{
	return (uint32_t)channel / PDG_PWM_CHANNELS_PER_SLICE;
}
```

It must be declared before first use.

- [ ] **Step 3: Create the suite build files**

Create `zephyr/tests/pdg_fake/pwm/CMakeLists.txt`:

```cmake
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_fake_pwm)

target_sources(app PRIVATE src/main.c)

# The embedded side calls the accessors of both fakes.
target_include_directories(app PRIVATE
  ${CMAKE_CURRENT_LIST_DIR}/../common
  ${CMAKE_CURRENT_LIST_DIR})

# The fakes run in the host context and provide STRONG definitions overriding
# the weak ones in drivers/common/common.c and drivers/pwm/pdg_pwm_bottom.c.
# Both halves are needed: the shared parent fake supplies
# pdg_common_bottom_open()/_close(), keeping pdg_mfd_init() out of
# gallo_init_strict(); the PWM fake supplies the five pdg_pwm_bottom_*
# definitions.
target_sources(native_simulator INTERFACE
  ${CMAKE_CURRENT_LIST_DIR}/../common/pdg_fake_bottom.c
  ${CMAKE_CURRENT_LIST_DIR}/pdg_pwm_fake_bottom.c)

# This MUST be target_compile_options("-I...") and NOT
# target_include_directories(native_simulator INTERFACE ...). The fakes are
# compiled by the native-simulator Makefile, not by CMake, and
# boards/native/common/natsim_config.cmake joins only INTERFACE_COMPILE_OPTIONS
# into NSI_BUILD_OPTIONS -- INTERFACE_INCLUDE_DIRECTORIES is never consumed and
# the flag silently vanishes. Do not "tidy" this back; it has already cost one
# red CI run (33106107101).
#
# drivers/i2c is on the list because the SHARED parent fake includes
# pdg_i2c_bottom.h; this suite enables no I2C child, but the fake is one
# translation unit serving every suite.
target_compile_options(native_simulator INTERFACE
  "-I${CMAKE_CURRENT_LIST_DIR}/../common"
  "-I${CMAKE_CURRENT_LIST_DIR}"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/common"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/i2c"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/pwm")
```

Create `zephyr/tests/pdg_fake/pwm/prj.conf`:

```conf
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

CONFIG_ZTEST=y
CONFIG_PWM=y
CONFIG_LOG=y
CONFIG_LOG_MODE_IMMEDIATE=y

# Disabled in fake.overlay; stated explicitly so an accidental re-enable is
# visible in review, matching the convention in the other test prj.conf files.
CONFIG_GPIO=n
CONFIG_I2C=n
CONFIG_SPI=n
```

Create `zephyr/tests/pdg_fake/pwm/fake.overlay`:

```dts
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Topology for the PWM recording-fake suite: parent and PWM okay, nothing
 * else.
 *
 * No FFI call occurs, and the reason is specific rather than general: every
 * bottom entry point this topology touches has a strong override. That is
 * pdg_common_bottom_open()/_close() in ../common/pdg_fake_bottom.c, which
 * keeps pdg_mfd_init() out of gallo_init_strict(), plus the five
 * pdg_pwm_bottom_* definitions in pdg_pwm_fake_bottom.c -- including
 * pdg_pwm_bottom_has_pwm(), which pdg_pwm_init() calls unconditionally and
 * which would otherwise hand the fake's opaque token to the real
 * gallo_get_device_info().
 *
 * That guarantee is scoped to this overlay. Enabling another child would add
 * bottom entry points that are NOT overridden.
 *
 * serial-number IS declared. pdg_pwm.c's PDG_PWM_PARENT_ASSERTS carries a
 * DT_NODE_HAS_PROP assertion on it, as the GPIO, SPI and UART drivers do, so
 * omitting it is a build failure rather than a runtime surprise.
 */

&pdg0 {
	status = "okay";
	serial-number = "PDGFAKEPWM00000";
};

&pdg_pwm0 {
	status = "okay";
};
```

Create `zephyr/tests/pdg_fake/pwm/tests.yaml`:

```yaml
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT
#
# NOT build_only. This suite is meant to run: the bottom layer is replaced by
# recording fakes, so nothing reaches gallo_init_strict() and no board is
# needed. It is the primary evidence the PWM driver works, because PWM cannot
# be verified on hardware in this project's current setup.
#
# No depends_on. native_sim/native/64 does not declare pwm in its supported:
# list, so claiming it would filter this scenario to nothing, silently. See
# zephyr/samples/i2c_bridge/tests.yaml.
common:
  tags:
    - drivers
    - pwm
    - pico_de_gallo
tests:
  drivers.pico_de_gallo.pwm.fake:
    platform_allow:
      - native_sim/native/64
    integration_platforms:
      - native_sim/native/64
    extra_dtc_overlay_files:
      - fake.overlay
```

- [ ] **Step 4: Write the first failing test**

Create `zephyr/tests/pdg_fake/pwm/src/main.c`:

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 */

#include <zephyr/ztest.h>
#include <zephyr/device.h>
#include <zephyr/drivers/pwm.h>

#include "pdg_fake_bottom.h"
#include "pdg_pwm_fake_bottom.h"

#define PWM_DEV DEVICE_DT_GET(DT_NODELABEL(pdg_pwm0))

/* Restated rather than included: the embedded half deliberately does not get
 * zephyr/drivers/pwm on its include path, so it cannot see pdg_pwm_bottom.h.
 * These must match that header.
 */
#define EXP_CYCLES_PER_SEC 150000000ULL
#define EXP_MAX_PERIOD     16711680U
#define EXP_MIN_PERIOD     2U

static void pwm_before(void *fixture)
{
	ARG_UNUSED(fixture);
	pdg_pwm_fake_reset();
}

ZTEST_SUITE(pdg_fake_pwm, NULL, NULL, pwm_before, NULL, NULL);

/*
 * The load-bearing test of the whole design. If the fake's strong
 * pdg_common_bottom_open() did not override the weak one, the real one runs,
 * reaches gallo_init_strict(), finds no board, and the parent is not ready --
 * so this fails at device_is_ready() rather than at the count.
 */
ZTEST(pdg_fake_pwm, test_weak_override_replaces_the_bottom_layer)
{
	zassert_true(device_is_ready(PWM_DEV),
		     "PWM child not ready: the real bottom layer probably ran "
		     "and tried to open a USB device");
	zassert_true(pdg_fake_open_count() > 0,
		     "the fake's pdg_common_bottom_open() was never called, so "
		     "the weak override did not take effect");
}

/*
 * cycles_per_sec must be the SOURCE clock: constant across channels and
 * independent of any configuration. A driver reporting the counter rate would
 * pass for channel 0 before configuration and then drift.
 */
ZTEST(pdg_fake_pwm, test_cycles_per_sec_is_the_source_clock)
{
	uint64_t cycles = 0U;

	for (uint32_t ch = 0U; ch < 4U; ch++) {
		zassert_ok(pwm_get_cycles_per_sec(PWM_DEV, ch, &cycles),
			   "channel %u rejected", ch);
		zassert_equal(cycles, EXP_CYCLES_PER_SEC,
			      "channel %u reported %llu, expected %llu", ch,
			      (unsigned long long)cycles,
			      (unsigned long long)EXP_CYCLES_PER_SEC);
	}
}

ZTEST(pdg_fake_pwm, test_cycles_per_sec_rejects_an_out_of_range_channel)
{
	uint64_t cycles = 0U;

	zassert_equal(pwm_get_cycles_per_sec(PWM_DEV, 4U, &cycles), -EINVAL);
}
```

- [ ] **Step 5: Run the suite and watch it pass on cycles_per_sec, fail on nothing yet**

Run the `TWISTER` command.
Expected: `3 of 3 executed test cases passed`. `set_cycles` is not exercised yet, so the `-ENOSYS` stub is not reached.

If the parent is not ready, the fake did not override — check the `-I` lines in `CMakeLists.txt` and that `pdg_pwm_fake_bottom.c` is in `target_sources(native_simulator ...)`.

- [ ] **Step 6: Normalize line endings and commit**

```bash
dos2unix zephyr/tests/pdg_fake/pwm/CMakeLists.txt zephyr/tests/pdg_fake/pwm/prj.conf zephyr/tests/pdg_fake/pwm/fake.overlay zephyr/tests/pdg_fake/pwm/tests.yaml zephyr/tests/pdg_fake/pwm/src/main.c zephyr/tests/pdg_fake/pwm/pdg_pwm_fake_bottom.c zephyr/tests/pdg_fake/pwm/pdg_pwm_fake_bottom.h
git add zephyr/tests/pdg_fake/pwm
git commit -F - <<'MSG'
test(zephyr): Add the PWM recording-fake suite

Host-executed ztest suite for the PWM driver, following the pdg_fake/i2c
and pdg_fake/uart pattern: strong symbol definitions override the weak
production bottom layer, so nothing reaches gallo_init_strict() and no
board is needed.

The fake models the firmware's compute_pwm_params() rather than returning
a fixed full-scale duty. max_duty is exactly the quantity the driver's
cycles-to-compare conversion depends on, so a fake that invented it would
pass while proving nothing.

The model searches dividers to 255, not the firmware's 4095. The firmware
range is a defect (#192) -- above 255 it programs a divider embassy-rp
panics on -- and modelling it would let a test pass on a configuration
that takes down a real board.

Covers cycles-per-second reporting so far. set_cycles is still stubbed.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## CRITICAL: test isolation hazard — read before Task 3

`pdg_pwm_fake_reset()` clears the **fake**. It does not, and cannot, clear the **driver's** state.

`struct pdg_pwm_data` is a static, per-device object created by `DEVICE_DT_INST_DEFINE`. It lives for the whole ztest binary. So `data->slices[].configured`, `data->slices[].period_cycles`, `data->slices[].enabled`, and every `data->channels[]` entry **persist across tests**, in whatever order twister happens to run them.

This is not hypothetical. It breaks assertions in the obvious way:

- A test asserting `set_config_count == 1` fails if an earlier test already left that slice configured at the same period, because the driver correctly skips the reconfiguration.
- A test asserting `enable_count == 1` fails if an earlier test already enabled that slice, because the driver correctly enables once.
- A test asserting a conflicting sibling period is refused can fail *for the wrong reason* if the sibling was left configured by an earlier test.

Do not solve this by adding a reset hook to the driver. A test-only entry point in production code is worse than the problem.

**Three rules. Follow all of them in every test you write from here on.**

1. **Assert deltas, never absolute counts.** Snapshot the counter, act, then assert the difference. Add these helpers to `src/main.c` now:

   ```c
   struct pwm_counts {
   	int set_config;
   	int set_duty;
   	int enable;
   	int disable;
   };

   static struct pwm_counts snapshot_counts(void)
   {
   	return (struct pwm_counts){
   		.set_config = pdg_pwm_fake_set_config_count(),
   		.set_duty = pdg_pwm_fake_set_duty_count(),
   		.enable = pdg_pwm_fake_enable_count(),
   		.disable = pdg_pwm_fake_disable_count(),
   	};
   }
   ```

   Because `pdg_pwm_fake_reset()` runs in the `before` hook, the counters do start at zero each test — so a delta and an absolute count coincide *within* a test. The reason to still write deltas is that the **driver** state behind them does not reset, so what the driver chooses to do varies with history. Deltas make that explicit instead of accidental.

2. **Use a period unique to each test** whenever the test depends on a reconfiguration happening. A period no other test uses is guaranteed to differ from whatever the slice currently holds, so `set_config` is guaranteed to be issued. Reserve a distinct value per test and say so in a comment. Where a test does not care, any period is fine.

3. **Never assume a slice starts unconfigured or disabled.** If a test needs a known starting point, establish it in the test body by driving the channel explicitly first, then snapshot.

Applying rule 2 to the tests below: `test_accepts_the_maximum_period` and `test_accepts_the_minimum_period` already use `EXP_MAX_PERIOD` and `EXP_MIN_PERIOD`, which no other test uses, so they are safe. `test_frequency_is_rounded_up` also uses `EXP_MAX_PERIOD` — that collides with `test_accepts_the_maximum_period`, so **change one of them**: have `test_frequency_is_rounded_up` first drive channel 0 to some other period (say `1234U`), then to `EXP_MAX_PERIOD`, guaranteeing the reconfiguration it needs to observe.

Applying rule 1: every `zassert_equal(pdg_pwm_fake_set_config_count(), N)` in Tasks 3 through 5 should become a delta assertion against a snapshot taken immediately before the call under test. The one exception is `assert_nothing_reached_the_device()`, which is a genuine absolute-zero assertion: it runs after a `before` hook that zeroed the counters and after a call that must have made none.

Finally, `test_reconfiguration_reasserts_the_sibling_duty` in Task 5 is written misleadingly and you should restructure it. Its second half calls `pdg_pwm_fake_reset()` and then re-drives channel 1, expecting a reconfiguration — but the driver may skip it, since only the fake was reset. Rewrite it as: drive channel 1 at period `P1` and 50%, snapshot, then drive channel 0 at a *different* period `P2` — which the conflict check will refuse. So instead drive **both** channels to `P1`, then change **both** to `P2` by driving channel 0 first; that is itself refused by the conflict rule. The honest test is therefore: configure only channel 1 at `P1`, then drive channel 0 at `P2` with the sibling *unconfigured*, confirming no re-assertion; and separately configure both at `P1`, then re-drive channel 0 at `P1` and confirm no reconfiguration and no sibling write. Getting a genuine reconfiguration with a configured sibling requires the sibling to share the new period, which the conflict rule guarantees — so the re-assertion path is reached only on the *first* configuration of a slice where the sibling was already tracked at that same period. Write the test to match the code's actual reachable behaviour, and if you conclude the re-assertion branch is unreachable, **say so and remove it rather than keeping dead code with a test that pretends to cover it.**

---

## Task 3: Argument validation

TDD. All of these must be refused **before any call reaches the fake**, which is what the `count == 0` assertions prove.

**Files:**
- Modify: `zephyr/tests/pdg_fake/pwm/src/main.c`
- Modify: `zephyr/drivers/pwm/pdg_pwm.c`

- [ ] **Step 1: Write the failing tests**

Append to `zephyr/tests/pdg_fake/pwm/src/main.c`:

```c
/* A refusal must be local. If any of these reach the device, the driver has
 * spent a USB round trip to be told what it already knew -- and for the
 * over-long period, it would have panicked the firmware (#192).
 */
static void assert_nothing_reached_the_device(void)
{
	zassert_equal(pdg_pwm_fake_set_config_count(), 0,
		      "a refused request still issued set_config");
	zassert_equal(pdg_pwm_fake_set_duty_count(), 0,
		      "a refused request still issued set_duty_cycle");
	zassert_equal(pdg_pwm_fake_enable_count(), 0,
		      "a refused request still issued enable");
}

ZTEST(pdg_fake_pwm, test_rejects_an_out_of_range_channel)
{
	zassert_equal(pwm_set_cycles(PWM_DEV, 4U, 1500U, 750U, 0), -EINVAL);
	assert_nothing_reached_the_device();
}

ZTEST(pdg_fake_pwm, test_rejects_inverted_polarity)
{
	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, 1500U, 750U,
				     PWM_POLARITY_INVERTED), -ENOTSUP);
	assert_nothing_reached_the_device();
}

ZTEST(pdg_fake_pwm, test_rejects_a_period_below_the_minimum)
{
	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, 0U, 0U, 0), -EINVAL);
	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, 1U, 0U, 0), -EINVAL);
	assert_nothing_reached_the_device();
}

/*
 * The #192 panic guard. EXP_MAX_PERIOD is the largest period whose derived
 * frequency the firmware can reach with a divider of 255 or less. One cycle
 * more would floor to a frequency needing divider 287, which panics.
 */
ZTEST(pdg_fake_pwm, test_rejects_a_period_above_the_divider_limit)
{
	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, EXP_MAX_PERIOD + 1U, 0U, 0),
		      -ENOTSUP);
	assert_nothing_reached_the_device();
}

/* The boundary itself must be ACCEPTED. A guard that also refused the last
 * legal period would be untestably conservative.
 */
ZTEST(pdg_fake_pwm, test_accepts_the_maximum_period)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, EXP_MAX_PERIOD, 0U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 1);
}

ZTEST(pdg_fake_pwm, test_accepts_the_minimum_period)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, EXP_MIN_PERIOD, 1U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 1);
}

ZTEST(pdg_fake_pwm, test_rejects_a_pulse_longer_than_the_period)
{
	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, 1500U, 1501U, 0), -EINVAL);
	assert_nothing_reached_the_device();
}
```

- [ ] **Step 2: Run and verify they fail**

Run the `TWISTER` command.
Expected: FAIL. The stub returns `-ENOSYS` for every call, so each assertion reports `-ENOSYS` where a specific errno was expected, and the two "accepts" tests fail outright.

- [ ] **Step 3: Implement the validation**

In `zephyr/drivers/pwm/pdg_pwm.c`, replace the stubbed `pdg_pwm_set_cycles` with:

```c
static int pdg_pwm_set_cycles(const struct device *dev, uint32_t channel,
			      uint32_t period_cycles, uint32_t pulse_cycles,
			      pwm_flags_t flags)
{
	struct pdg_pwm_data *data = dev->data;

	if (data->ctx == NULL) {
		return -ENODEV;
	}

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		LOG_ERR("%s: channel %u is out of range (0 to %u).", dev->name,
			channel, PDG_PWM_NUM_CHANNELS - 1U);
		return -EINVAL;
	}

	/*
	 * Not emulated by inverting the duty cycle. That would change the idle
	 * level rather than the polarity, which is a different signal and a
	 * worse failure than a clean refusal.
	 */
	if ((flags & PWM_POLARITY_INVERTED) != 0U) {
		LOG_ERR("%s: channel %u requested inverted polarity, which the "
			"firmware does not support. Returning -ENOTSUP.",
			dev->name, channel);
		return -ENOTSUP;
	}

	if (period_cycles < PDG_PWM_MIN_PERIOD_CYCLES) {
		LOG_ERR("%s: channel %u requested a period of %u cycles; the "
			"minimum is %u. Returning -EINVAL.", dev->name, channel,
			period_cycles, PDG_PWM_MIN_PERIOD_CYCLES);
		return -EINVAL;
	}

	/*
	 * Panic guard, not a capability statement. A longer period derives a
	 * frequency the firmware would try to reach with a clock divider above
	 * 255, which embassy-rp refuses with a panic that takes the whole
	 * device down. See issue #192; this contains the defect for Zephyr
	 * consumers without fixing it, and every other host surface remains
	 * able to reach it.
	 */
	if ((uint64_t)period_cycles > PDG_PWM_MAX_PERIOD_CYCLES) {
		LOG_ERR("%s: channel %u requested a period of %u cycles; the "
			"maximum is %llu (about %u Hz), because a longer period "
			"needs a clock divider the firmware cannot program. "
			"Returning -ENOTSUP.", dev->name, channel, period_cycles,
			(unsigned long long)PDG_PWM_MAX_PERIOD_CYCLES,
			(unsigned int)(PDG_PWM_CYCLES_PER_SEC /
				       PDG_PWM_MAX_PERIOD_CYCLES));
		return -ENOTSUP;
	}

	if (pulse_cycles > period_cycles) {
		LOG_ERR("%s: channel %u requested a pulse of %u cycles inside a "
			"period of %u. Returning -EINVAL.", dev->name, channel,
			pulse_cycles, period_cycles);
		return -EINVAL;
	}

	return pdg_pwm_apply(dev, channel, period_cycles, pulse_cycles);
}
```

Add a placeholder for the applier directly above it, so the file still compiles:

```c
static int pdg_pwm_apply(const struct device *dev, uint32_t channel,
			 uint32_t period_cycles, uint32_t pulse_cycles)
{
	ARG_UNUSED(dev);
	ARG_UNUSED(channel);
	ARG_UNUSED(period_cycles);
	ARG_UNUSED(pulse_cycles);

	return -ENOSYS;
}
```

- [ ] **Step 4: Run and verify the refusals pass**

Run the `TWISTER` command.
Expected: every `test_rejects_*` test passes. `test_accepts_the_maximum_period` and `test_accepts_the_minimum_period` still FAIL with `-ENOSYS` — that is correct, Task 4 implements them.

- [ ] **Step 5: Commit**

```bash
git add zephyr/drivers/pwm/pdg_pwm.c zephyr/tests/pdg_fake/pwm/src/main.c
git commit -F - <<'MSG'
feat(zephyr): Validate PWM arguments before reaching the device

Refuses an out-of-range channel, inverted polarity, a pulse longer than
its period, and a period outside the supported range -- all locally, so a
bad request costs no USB round trip.

The upper period bound is a panic guard rather than a capability
statement. A longer period derives a frequency the firmware would try to
reach with a clock divider above 255, and embassy-rp panics on that,
taking the whole device down. This contains the defect for Zephyr
consumers; every other host surface can still reach it. See #192.

The tests assert that a refused request issues no set_config, no
set_duty_cycle and no enable, which is what proves the refusal is local
rather than merely correct.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## Task 4: The conversion and the device sequence

**Files:**
- Modify: `zephyr/tests/pdg_fake/pwm/src/main.c`
- Modify: `zephyr/drivers/pwm/pdg_pwm.c`

- [ ] **Step 1: Write the failing tests**

Append to `zephyr/tests/pdg_fake/pwm/src/main.c`:

```c
/* Helper: what compare value the driver should have sent for this ratio,
 * given the full-scale duty the fake is modelling for that channel.
 */
static uint16_t expected_compare(uint8_t channel, uint32_t pulse, uint32_t period)
{
	uint16_t max_duty = 0U;
	uint64_t scaled;

	zassert_ok(pdg_pwm_fake_max_duty_for(channel, &max_duty),
		   "slice for channel %u was never configured", channel);

	scaled = ((uint64_t)pulse * (uint64_t)max_duty) + ((uint64_t)period / 2U);

	return (uint16_t)(scaled / (uint64_t)period);
}

static uint16_t last_duty_for(uint8_t channel)
{
	int len = pdg_pwm_fake_set_duty_log_len();

	for (int i = len - 1; i >= 0; i--) {
		uint8_t ch = 0U;
		uint16_t duty = 0U;

		zassert_ok(pdg_pwm_fake_set_duty_log_entry(i, &ch, &duty));
		if (ch == channel) {
			return duty;
		}
	}

	zassert_unreachable("no set_duty_cycle recorded for channel %u", channel);
	return 0U;
}

/*
 * The frequency the driver derives must be the CEILING of the division.
 * Flooring would lengthen the period, forcing a larger divider, and at the
 * maximum period it lands on 8 Hz -- which needs divider 287 and panics.
 * Ceiling lands on 9 Hz, which needs 255.
 */
ZTEST(pdg_fake_pwm, test_frequency_is_rounded_up)
{
	uint32_t freq = 0U;

	/* 150e6 / 16711680 = 8.97..., so ceiling is 9 and floor would be 8. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, EXP_MAX_PERIOD, 0U, 0));
	zassert_ok(pdg_pwm_fake_last_set_config(NULL, &freq, NULL));
	zassert_equal(freq, 9U,
		      "expected the ceiling (9 Hz); got %u. A floor here would "
		      "select a divider the firmware panics on.", freq);
}

ZTEST(pdg_fake_pwm, test_phase_correct_is_never_requested)
{
	bool phase_correct = true;

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 150000U, 75000U, 0));
	zassert_ok(pdg_pwm_fake_last_set_config(NULL, NULL, &phase_correct));
	zassert_false(phase_correct,
		      "phase-correct mode breaks the max_duty-to-period "
		      "relation and must never be requested");
}

/*
 * Round-trip a table of periods. The achieved frequency must be within
 * tolerance AND must never be BELOW the requested one -- the firmware floors
 * top, so it always overshoots, and the driver's ceiling preserves that.
 */
ZTEST(pdg_fake_pwm, test_frequency_round_trip_never_undershoots)
{
	static const uint32_t periods[] = {
		2U, 3U, 100U, 1500U, 15000U, 150000U, 1500000U,
		15000000U, EXP_MAX_PERIOD,
	};

	ARRAY_FOR_EACH(periods, i) {
		uint32_t period = periods[i];
		uint32_t requested_hz;
		uint32_t freq = 0U;

		pdg_pwm_fake_reset();
		zassert_ok(pwm_set_cycles(PWM_DEV, 0U, period, 0U, 0),
			   "period %u rejected", period);
		zassert_ok(pdg_pwm_fake_last_set_config(NULL, &freq, NULL));

		requested_hz = (uint32_t)(EXP_CYCLES_PER_SEC / period);

		zassert_true(freq >= requested_hz,
			     "period %u: derived %u Hz, which undershoots the "
			     "requested %u Hz", period, freq, requested_hz);
		zassert_true(freq - requested_hz <= 1U,
			     "period %u: derived %u Hz, more than 1 Hz above "
			     "the requested %u Hz", period, freq, requested_hz);
	}
}

/* Duty accuracy across the range, including both endpoints. */
ZTEST(pdg_fake_pwm, test_duty_ratio_is_accurate)
{
	static const uint32_t pulses[] = { 0U, 1U, 250U, 500U, 750U, 999U, 1000U };
	const uint32_t period = 1000U;

	ARRAY_FOR_EACH(pulses, i) {
		uint32_t pulse = pulses[i];

		pdg_pwm_fake_reset();
		zassert_ok(pwm_set_cycles(PWM_DEV, 0U, period, pulse, 0));
		zassert_equal(last_duty_for(0U), expected_compare(0U, pulse, period),
			      "pulse %u of %u produced the wrong compare value",
			      pulse, period);
	}
}

/* A zero pulse must be a compare of 0, which is constant-low. It must NOT be
 * expressed by disabling the slice.
 */
ZTEST(pdg_fake_pwm, test_zero_pulse_is_a_zero_compare)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 0U, 0));
	zassert_equal(last_duty_for(0U), 0U);
	zassert_equal(pdg_pwm_fake_disable_count(), 0,
		      "a zero pulse must not disable the slice");
}

/* A full pulse must be a compare of exactly max_duty, which is constant-high. */
ZTEST(pdg_fake_pwm, test_full_pulse_is_full_scale)
{
	uint16_t max_duty = 0U;

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 1000U, 0));
	zassert_ok(pdg_pwm_fake_max_duty_for(0U, &max_duty));
	zassert_equal(last_duty_for(0U), max_duty);
}

/* Reconfiguration is skipped when the period is unchanged: a repeated duty
 * update must not reconfigure the slice, which would rescale and drift.
 */
ZTEST(pdg_fake_pwm, test_unchanged_period_does_not_reconfigure)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 250U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 1);

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 750U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 1,
		      "changing only the pulse width must not reconfigure the slice");
}
```

- [ ] **Step 2: Run and verify they fail**

Run the `TWISTER` command.
Expected: FAIL, every new test reporting `-ENOSYS` from the `pdg_pwm_apply` placeholder.

- [ ] **Step 3: Implement the conversion and the applier**

In `zephyr/drivers/pwm/pdg_pwm.c`, add the two conversion helpers above `pdg_pwm_apply`:

```c
/*
 * CEILING, not floor, and this is load-bearing.
 *
 * Flooring lengthens the period, which forces the firmware to choose a larger
 * clock divider. At the maximum supported period it yields 8 Hz, which needs
 * a divider of 287 -- above the 255 the hardware can hold, and embassy-rp
 * panics rather than refusing. Ceiling yields 9 Hz, which needs exactly 255.
 *
 * It also matches the firmware's own behaviour: because it floors `top`, its
 * achieved frequency is always greater than or equal to the requested one.
 *
 * period_cycles is bounded to at least PDG_PWM_MIN_PERIOD_CYCLES by the
 * caller, so there is no division by zero here.
 */
static uint32_t pdg_pwm_frequency_for(uint32_t period_cycles)
{
	uint64_t numerator = (uint64_t)PDG_PWM_CYCLES_PER_SEC +
			     (uint64_t)period_cycles - 1U;

	return (uint32_t)(numerator / (uint64_t)period_cycles);
}

/*
 * Scale a pulse width into the firmware's raw compare domain.
 *
 * Round half up rather than truncating. The firmware already truncates when it
 * rescales compares across a reconfiguration, and truncating here too would
 * compound a downward bias on every duty cycle.
 *
 * The clamp is belt and braces: pulse_cycles <= period_cycles is enforced by
 * the caller, so the quotient cannot exceed max_duty. The firmware would
 * silently clamp an over-range value anyway, which is exactly why the driver
 * must not rely on it to catch a mistake.
 */
static uint16_t pdg_pwm_compare_for(uint32_t pulse_cycles, uint32_t period_cycles,
				    uint16_t max_duty)
{
	uint64_t scaled = ((uint64_t)pulse_cycles * (uint64_t)max_duty) +
			  ((uint64_t)period_cycles / 2U);
	uint64_t compare = scaled / (uint64_t)period_cycles;

	if (compare > (uint64_t)max_duty) {
		compare = (uint64_t)max_duty;
	}

	return (uint16_t)compare;
}
```

Then replace the `pdg_pwm_apply` placeholder with the real implementation:

```c
static int pdg_pwm_apply(const struct device *dev, uint32_t channel,
			 uint32_t period_cycles, uint32_t pulse_cycles)
{
	struct pdg_pwm_data *data = dev->data;
	uint32_t slice = pdg_pwm_slice_of(channel);
	uint16_t max_duty = 0U;
	uint16_t current_duty = 0U;
	bool reconfigured = false;
	int ret;

	k_mutex_lock(&data->lock, K_FOREVER);

	if (!data->slices[slice].configured ||
	    data->slices[slice].period_cycles != period_cycles) {
		ret = pdg_pwm_bottom_set_config(data->ctx, (uint8_t)channel,
						pdg_pwm_frequency_for(period_cycles),
						false);
		if (ret < 0) {
			LOG_ERR("%s: channel %u failed to configure the slice: "
				"errno=%d.", dev->name, channel, ret);
			goto out;
		}

		data->slices[slice].configured = true;
		data->slices[slice].period_cycles = period_cycles;
		reconfigured = true;
	}

	/*
	 * max_duty is read back rather than computed. It is top + 1, and `top`
	 * is whatever the firmware's divider search settled on -- which the
	 * driver cannot predict without duplicating that search, and must not,
	 * because a duplicate would drift from it silently.
	 */
	ret = pdg_pwm_bottom_get_duty_cycle(data->ctx, (uint8_t)channel,
					    &current_duty, &max_duty);
	if (ret < 0) {
		LOG_ERR("%s: channel %u failed to read the full-scale duty: "
			"errno=%d.", dev->name, channel, ret);
		goto out;
	}

	if (max_duty == 0U) {
		LOG_ERR("%s: channel %u: the firmware reported a full-scale duty "
			"of zero, which would make every duty cycle a division "
			"by zero. Returning -EIO.", dev->name, channel);
		ret = -EIO;
		goto out;
	}

	ret = pdg_pwm_bottom_set_duty_cycle(data->ctx, (uint8_t)channel,
					    pdg_pwm_compare_for(pulse_cycles,
								period_cycles,
								max_duty));
	if (ret < 0) {
		LOG_ERR("%s: channel %u failed to set the duty cycle: errno=%d.",
			dev->name, channel, ret);
		goto out;
	}

	data->channels[channel].configured = true;
	data->channels[channel].period_cycles = period_cycles;
	data->channels[channel].pulse_cycles = pulse_cycles;

	ret = pdg_pwm_reassert_sibling(dev, channel, max_duty, reconfigured);
	if (ret < 0) {
		goto out;
	}

	if (!data->slices[slice].enabled) {
		ret = pdg_pwm_bottom_enable(data->ctx, (uint8_t)channel);
		if (ret < 0) {
			LOG_ERR("%s: channel %u failed to enable the slice: "
				"errno=%d.", dev->name, channel, ret);
			goto out;
		}

		data->slices[slice].enabled = true;
	}

	ret = 0;
out:
	k_mutex_unlock(&data->lock);

	return ret;
}
```

Add a placeholder for the sibling re-assertion above `pdg_pwm_apply`; Task 5 implements it:

```c
static int pdg_pwm_reassert_sibling(const struct device *dev, uint32_t channel,
				    uint16_t max_duty, bool reconfigured)
{
	ARG_UNUSED(dev);
	ARG_UNUSED(channel);
	ARG_UNUSED(max_duty);
	ARG_UNUSED(reconfigured);

	return 0;
}
```

- [ ] **Step 4: Run and verify**

Run the `TWISTER` command.
Expected: all tests pass, including the two "accepts" tests from Task 3.

If `test_frequency_is_rounded_up` reports 8 rather than 9, you implemented a floor. Re-read the helper.

- [ ] **Step 5: Commit**

```bash
git add zephyr/drivers/pwm/pdg_pwm.c zephyr/tests/pdg_fake/pwm/src/main.c
git commit -F - <<'MSG'
feat(zephyr): Implement the PWM cycles-to-frequency conversion

Derives the frequency from period_cycles with a CEILING and scales the
pulse width into the firmware's raw compare domain with round-half-up.

The ceiling is load-bearing. Flooring lengthens the period, forcing a
larger clock divider; at the maximum supported period it yields 8 Hz,
which needs divider 287 and panics inside embassy-rp. Ceiling yields 9 Hz,
which needs exactly 255. It also matches the firmware, which floors top
and therefore always achieves a frequency at or above the request.

Round-half-up on the compare avoids compounding a downward bias with the
firmware's own truncating rescale.

max_duty is read back rather than computed, because it is top + 1 and top
depends on the divider search the driver cannot observe and must not
duplicate.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## Task 5: Slice sharing — conflict refusal, sibling re-assertion, enable-once

**Files:**
- Modify: `zephyr/tests/pdg_fake/pwm/src/main.c`
- Modify: `zephyr/drivers/pwm/pdg_pwm.c`

- [ ] **Step 1: Write the failing tests**

Append to `zephyr/tests/pdg_fake/pwm/src/main.c`:

```c
/*
 * Two channels on one slice cannot hold independent periods. Refusing is
 * better than reconfiguring: silently changing channel 0's period because
 * channel 1 asked for a different one is a defect its owner cannot see.
 */
ZTEST(pdg_fake_pwm, test_rejects_a_conflicting_sibling_period)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 500U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 1);

	zassert_equal(pwm_set_cycles(PWM_DEV, 1U, 2000U, 500U, 0), -EINVAL,
		      "channel 1 must not be allowed a period differing from "
		      "its slice sibling's");
	zassert_equal(pdg_pwm_fake_set_config_count(), 1,
		      "the refused request still reconfigured the slice");
}

/* The same period on a sibling is fine, and must not reconfigure. */
ZTEST(pdg_fake_pwm, test_accepts_a_matching_sibling_period)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 250U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 1000U, 750U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 1,
		      "a sibling with a matching period must not reconfigure");
}

/* Channels on DIFFERENT slices are independent. */
ZTEST(pdg_fake_pwm, test_different_slices_hold_independent_periods)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 500U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, 2000U, 500U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count(), 2);
}

/*
 * After a reconfiguration the firmware rescales both compares with truncating
 * integer division, so the sibling's duty drifts downward. The driver must
 * re-assert it from its own tracked ratio.
 */
ZTEST(pdg_fake_pwm, test_reconfiguration_reasserts_the_sibling_duty)
{
	uint16_t max_duty = 0U;
	int len_before;

	/* Channel 1 at 50%, on a slice configured for period 1000. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 1000U, 500U, 0));
	len_before = pdg_pwm_fake_set_duty_log_len();

	/*
	 * Channel 0 keeps the same period, so no reconfiguration happens and
	 * the sibling must NOT be touched.
	 */
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 250U, 0));
	zassert_equal(pdg_pwm_fake_set_duty_log_len(), len_before + 1,
		      "without a reconfiguration the sibling must not be "
		      "re-asserted");

	/*
	 * Now force a reconfiguration by clearing state and driving both
	 * channels at a new period.
	 */
	pdg_pwm_fake_reset();
	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 1000U, 500U, 0));
	zassert_ok(pdg_pwm_fake_max_duty_for(1U, &max_duty));
	zassert_equal(last_duty_for(1U), expected_compare(1U, 500U, 1000U),
		      "channel 1 did not land on its own requested ratio");
}

/* The slice is enabled exactly once, however many updates follow. */
ZTEST(pdg_fake_pwm, test_slice_is_enabled_once)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 250U, 0));
	zassert_equal(pdg_pwm_fake_enable_count(), 1);

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 500U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 1000U, 500U, 0));
	zassert_equal(pdg_pwm_fake_enable_count(), 1,
		      "the slice must be enabled once, not on every update");
}

/*
 * Nothing in this driver may ever disable a slice. The bottom header declares
 * no disable function, so this is structurally impossible today; the test
 * exists so that adding one is caught here rather than in the field, where it
 * would stop an unrelated channel.
 */
ZTEST(pdg_fake_pwm, test_never_disables_a_slice)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 500U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 0U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 1000U, 0U, 0));
	zassert_equal(pdg_pwm_fake_disable_count(), 0,
		      "the driver disabled a slice, which stops the sibling "
		      "channel");
}

/* No log may have overflowed during any of the above. */
ZTEST(pdg_fake_pwm, test_no_recorder_overflowed)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 500U, 0));
	zassert_equal(pdg_pwm_fake_overflowed(), 0);
}
```

- [ ] **Step 2: Run and verify they fail**

Run the `TWISTER` command.
Expected: `test_rejects_a_conflicting_sibling_period` FAILS (it currently returns 0 and reconfigures). The others may pass incidentally; that is fine, they are regression protection.

- [ ] **Step 3: Add the conflict check**

In `pdg_pwm_set_cycles`, after the `pulse_cycles > period_cycles` check and before the call to `pdg_pwm_apply`, insert:

```c
	/*
	 * Slice-sharing conflict.
	 *
	 * Channels 0+1 and 2+3 each share an RP2350 slice, and a slice has one
	 * period. Reconfiguring would silently change the sibling's period
	 * behind its owner's back, and additionally drift its duty through the
	 * firmware's truncating rescale. Refusing surfaces the constraint at
	 * the call that violates it.
	 *
	 * Read under the lock, because a concurrent call on the sibling could
	 * otherwise be observed half-applied.
	 */
	{
		struct pdg_pwm_data *locked = dev->data;
		uint32_t sibling = pdg_pwm_sibling_of(channel);
		bool conflict;
		uint32_t sibling_period;

		k_mutex_lock(&locked->lock, K_FOREVER);
		conflict = locked->channels[sibling].configured &&
			   locked->channels[sibling].period_cycles != period_cycles;
		sibling_period = locked->channels[sibling].period_cycles;
		k_mutex_unlock(&locked->lock);

		if (conflict) {
			LOG_ERR("%s: channel %u requested a period of %u cycles, but "
				"its slice sibling channel %u is using %u. Channels "
				"%u and %u share one PWM slice and cannot hold "
				"independent periods. Use channels 0 and 2 for two "
				"independent periods. Returning -EINVAL.",
				dev->name, channel, period_cycles, sibling,
				sibling_period, channel, sibling);
			return -EINVAL;
		}
	}
```

- [ ] **Step 4: Implement the sibling re-assertion**

Replace the `pdg_pwm_reassert_sibling` placeholder:

```c
/*
 * Re-assert the sibling channel's duty after a slice reconfiguration.
 *
 * pwm/set-config rescales both channels' compares proportionally using
 * truncating integer division, so the sibling's duty ratio drifts downward
 * every time the slice is reconfigured. Recomputing it from the ratio this
 * driver tracked is exact, where trusting the firmware's rescale is not.
 *
 * Called with the lock held. max_duty is the slice's, so it is the same value
 * for both channels and does not need re-reading.
 */
static int pdg_pwm_reassert_sibling(const struct device *dev, uint32_t channel,
				    uint16_t max_duty, bool reconfigured)
{
	struct pdg_pwm_data *data = dev->data;
	uint32_t sibling = pdg_pwm_sibling_of(channel);
	int ret;

	if (!reconfigured || !data->channels[sibling].configured) {
		return 0;
	}

	ret = pdg_pwm_bottom_set_duty_cycle(
		data->ctx, (uint8_t)sibling,
		pdg_pwm_compare_for(data->channels[sibling].pulse_cycles,
				    data->channels[sibling].period_cycles,
				    max_duty));
	if (ret < 0) {
		LOG_ERR("%s: channel %u reconfigured the slice but failed to "
			"re-assert sibling channel %u's duty cycle: errno=%d. "
			"The sibling's duty is now whatever the firmware's "
			"rescale produced.", dev->name, channel, sibling, ret);
		return ret;
	}

	return 0;
}
```

Note the conflict check guarantees `data->channels[sibling].period_cycles == period_cycles` whenever the sibling is configured, so the recomputed ratio uses a period consistent with the new configuration.

- [ ] **Step 5: Run and verify**

Run the `TWISTER` command.
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add zephyr/drivers/pwm/pdg_pwm.c zephyr/tests/pdg_fake/pwm/src/main.c
git commit -F - <<'MSG'
feat(zephyr): Handle PWM slice sharing explicitly

RP2350 slices own two channels each, and the firmware's configure and
enable operations act on the whole slice. Three consequences are now
handled rather than left to be discovered at runtime.

A channel requesting a period that differs from its slice sibling's is
refused with -EINVAL, naming both channels and both periods. Reconfiguring
instead would change the sibling's period behind its owner's back.

After a reconfiguration the sibling's duty is re-asserted from the ratio
this driver tracked. The firmware rescales compares with truncating
integer division, so trusting it drifts the duty downward every time.

The slice is enabled on first use and never disabled. A zero pulse is
already a constant-low output, so disable is not needed to express "off",
and calling it would stop the sibling mid-operation. The bottom header
declares no disable function at all, so this is structurally enforced; a
test guards against someone adding one.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## Task 6: The sample

**Files:**
- Create: `zephyr/samples/pwm_fade/CMakeLists.txt`
- Create: `zephyr/samples/pwm_fade/prj.conf`
- Create: `zephyr/samples/pwm_fade/app.overlay`
- Create: `zephyr/samples/pwm_fade/src/main.c`
- Create: `zephyr/samples/pwm_fade/tests.yaml`

- [ ] **Step 1: Create the sample**

`zephyr/samples/pwm_fade/CMakeLists.txt`:

```cmake
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo samples")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo samples")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_pwm_fade)

target_sources(app PRIVATE src/main.c)
```

`zephyr/samples/pwm_fade/prj.conf`:

```conf
CONFIG_PWM=y
CONFIG_LOG=y
```

`zephyr/samples/pwm_fade/app.overlay`:

```dts
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 */

&pdg0 {
	status = "okay";
	serial-number = "PDGPWMFADE00000";
};

&pdg_pwm0 {
	status = "okay";
};
```

`zephyr/samples/pwm_fade/src/main.c`:

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Fades an LED on PWM channel 0 (GPIO12) of a USB-attached Pico de Gallo.
 *
 * The period is expressed in cycles against pwm_get_cycles_per_sec(), which
 * this driver reports as the 150 MHz PWM source clock. Asking the driver
 * rather than hardcoding 150000000 is the point: a consumer should never need
 * to know the firmware's system clock.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/pwm.h>

#define PWM_CHANNEL 0U
#define FADE_STEPS  50U
#define TARGET_HZ   1000U

int main(void)
{
	const struct device *pwm = DEVICE_DT_GET(DT_NODELABEL(pdg_pwm0));
	uint64_t cycles_per_sec;
	uint32_t period;
	int ret;

	if (!device_is_ready(pwm)) {
		printk("PWM not ready (Pico de Gallo bridge connected?)\n");
		return 0;
	}

	ret = pwm_get_cycles_per_sec(pwm, PWM_CHANNEL, &cycles_per_sec);
	if (ret < 0) {
		printk("pwm_get_cycles_per_sec failed: %d\n", ret);
		return 0;
	}

	period = (uint32_t)(cycles_per_sec / TARGET_HZ);
	printk("Fading channel %u at %u Hz (%u cycles of %llu per second)\n",
	       PWM_CHANNEL, TARGET_HZ, period,
	       (unsigned long long)cycles_per_sec);

	while (1) {
		for (uint32_t step = 0U; step <= FADE_STEPS; step++) {
			uint32_t pulse = (uint32_t)(((uint64_t)period * step) /
						    FADE_STEPS);

			ret = pwm_set_cycles(pwm, PWM_CHANNEL, period, pulse, 0);
			if (ret < 0) {
				printk("pwm_set_cycles failed: %d\n", ret);
				return 0;
			}

			k_sleep(K_MSEC(20));
		}
	}

	return 0;
}
```

`zephyr/samples/pwm_fade/tests.yaml`:

```yaml
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT
#
# BUILD ONLY, and not negotiable. Booting this image initialises the MFD
# parent, which reaches gallo_init_strict() and opens a USB device. GitHub
# runners have no board attached, and twister treats native_sim as
# `type: native`, so it would otherwise RUN the binary.
#
# No depends_on: native_sim/native/64 does not name pwm in its supported:
# list, so claiming it would silently filter this scenario to nothing.
sample:
  name: Pico de Gallo PWM fade
  description: Fade an LED on a USB-attached Pico de Gallo PWM channel
common:
  build_only: true
  tags:
    - pwm
    - shield
    - pico_de_gallo
tests:
  sample.pico_de_gallo.pwm_fade:
    platform_allow:
      - native_sim/native/64
    integration_platforms:
      - native_sim/native/64
```

- [ ] **Step 2: Verify it builds**

```bash
wsl -d Ubuntu-26.04 -- bash -lc 'cd ~/zephyrproject/zephyr && source ../.venv/bin/activate; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr ZEPHYR_TOOLCHAIN_VARIANT=host; ./scripts/twister -p native_sim/native/64 -T /mnt/d/workspace/pico-de-gallo/zephyr/samples/pwm_fade --inline-logs --jobs 1'
```

Expected: `1 of 1 executed test configurations passed`, reported as built but not run.

- [ ] **Step 3: Normalize and commit**

```bash
dos2unix zephyr/samples/pwm_fade/CMakeLists.txt zephyr/samples/pwm_fade/prj.conf zephyr/samples/pwm_fade/app.overlay zephyr/samples/pwm_fade/src/main.c zephyr/samples/pwm_fade/tests.yaml
git add zephyr/samples/pwm_fade
git commit -F - <<'MSG'
feat(zephyr): Add the PWM fade sample

Fades an LED on PWM channel 0 of a USB-attached Pico de Gallo.

The sample derives its period from pwm_get_cycles_per_sec() rather than
hardcoding 150000000, which is the point: a consumer should never need to
know the firmware's system clock.

Build-only. Booting it initialises the MFD parent, which opens a USB
device, and CI runners have no board attached.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## Task 7: CI registration and documentation

**Files:**
- Modify: `zephyr/scripts/ci-build.sh`
- Modify: `zephyr/README.md`
- Modify: `zephyr/CHANGELOG.md`

- [ ] **Step 1: Register the new build targets**

`zephyr/scripts/ci-build.sh` carries a `PDG_TARGETS` table and a self-test asserting its length. Add two rows after the `uart_fake` row (around line 92), following the existing pipe-separated field order `name|kind|path|overlay|sources|objects|configs`:

```bash
# The PWM recording-fake suite. BUILT HERE, NOT RUN HERE: twister executes it
# (its tests.yaml omits build_only), while this script only ever builds. The
# two are complementary -- twister proves the suite passes, this table proves
# it still links against the production driver with the module's own
# -DEXTRA_ZEPHYR_MODULES wiring.
"pwm_fake|pass|zephyr/tests/pdg_fake/pwm|zephyr/tests/pdg_fake/pwm/fake.overlay|pdg_mfd.c,pdg_pwm.c|gallo_registry,pdg_pwm_bottom,pdg_pwm_fake_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_PWM_PICO_DE_GALLO"
"pwm_fade|pass|zephyr/samples/pwm_fade||pdg_mfd.c,pdg_pwm.c|gallo_registry,pdg_pwm_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_PWM_PICO_DE_GALLO"
```

Then update the self-test count. Find:

```bash
		st_check "table has 13 targets" "${#PDG_TARGETS[@]}" "13"
```

and change both the label and the value to `15`:

```bash
		st_check "table has 15 targets" "${#PDG_TARGETS[@]}" "15"
```

Verify the self-test still passes:

```bash
wsl -d Ubuntu-26.04 -- bash -lc 'cd /mnt/d/workspace/pico-de-gallo && bash zephyr/scripts/ci-build.sh --self-test'
```

Expected: the self-test reports all checks passing. If it reports a count mismatch you missed the `st_check` edit.

- [ ] **Step 2: Update the README**

`zephyr/README.md` is the authoritative documentation for this module — there is no book chapter, by the AGENTS.md §15.1 carve-out. Add PWM to:

1. The driver list / supported-features table, alongside GPIO, I2C, SPI and UART.
2. A new PWM section, modelled on the UART section, covering: the four channels and their GPIO mapping; that `pwm_get_cycles_per_sec()` reports the 150 MHz source clock; the period bounds and what each refusal means; the slice-sharing constraint and the `-EINVAL` on a conflicting sibling period; that the driver never disables a slice; and that phase-correct mode is unreachable.
3. An **unsupported-flag table** for the PWM driver, which issue #155 asks for explicitly:

```markdown
| Flag / feature           | Status    | Reason                                                                 |
|--------------------------|-----------|------------------------------------------------------------------------|
| `PWM_POLARITY_INVERTED`  | `-ENOTSUP`| Not emulated by inverting the duty cycle, which would change the idle level rather than the polarity. |
| `pwm_capture_*`          | `-ENOSYS` | The firmware exposes no capture endpoint; the API slots are left NULL. |
| Phase-correct mode       | unreachable | Zephyr has no corresponding concept, and in that mode the firmware's reported full-scale duty no longer tracks the period. |
| Disabling a channel      | not implemented | Disable acts on a whole slice and would stop the sibling channel. A zero pulse gives a constant-low output instead. |
| Independent periods on channels 0+1 or 2+3 | `-EINVAL` | Those pairs share one RP2350 slice, which has a single period. |
| Period above 16711680 cycles | `-ENOTSUP` | Would need a clock divider above 255, which panics the firmware. See #192. |
```

- [ ] **Step 3: Update the CHANGELOG**

Add to the `### Added` section under `## [Unreleased]` in `zephyr/CHANGELOG.md`, creating that subsection if it does not exist (the current top section is `### Changed`):

```markdown
### Added

- A `pwm` driver, `drivers/pwm/pdg_pwm.c`, as a child of the
  `odp,pico-de-gallo` MFD parent, with the binding
  `dts/bindings/pwm/odp,pico-de-gallo-pwm.yaml`. Refs #155.

  `pwm_get_cycles_per_sec()` reports the 150 MHz PWM **source** clock, not
  the counter rate. The counter advances at `150e6/divider`, which changes
  with every frequency change and differs between the two slices, so it could
  not serve as the stable per-device constant Zephyr's API requires.
  Reporting the source clock makes `period_cycles` independent of the
  divider — which matters, because the divider is not on the wire and the
  driver cannot observe it.

  The frequency is derived with a **ceiling**, not a floor. Flooring
  lengthens the period and forces a larger divider; at the maximum supported
  period it yields 8 Hz, which needs divider 287 and panics inside
  embassy-rp. Ceiling yields 9 Hz, which needs exactly 255. The driver
  therefore refuses any period above `255 * 65536 = 16711680` cycles with
  `-ENOTSUP`. That is **containment of #192, not a fix**: the underlying
  firmware defect remains reachable from the CLI, `pico-de-gallo-lib`, the C
  FFI, Python and MCP.

  Channels 0+1 and 2+3 each share an RP2350 slice, which has a single period
  and a single enable. A channel requesting a period differing from its
  sibling's is refused with `-EINVAL` rather than silently changing the
  sibling's period. After any reconfiguration the sibling's duty is
  re-asserted from the driver's own tracked ratio, because the firmware
  rescales compares with truncating integer division and drifts them
  downward. The driver enables a slice on first use and **never disables
  one** — a zero pulse already gives a constant-low output, and disabling
  would stop the sibling channel. `pdg_pwm_bottom.h` declares no disable
  function at all, so that is structurally enforced rather than merely
  documented.

  `PWM_POLARITY_INVERTED` returns `-ENOTSUP` and is deliberately not
  emulated by inverting the duty cycle, which would change the idle level
  rather than the polarity. Phase-correct mode is never requested.

  Coverage is `tests/pdg_fake/pwm`, executed by twister with no board
  attached. PWM cannot currently be verified on hardware in this project, so
  this suite is the primary evidence rather than a supplement to a hardware
  demo. Its fake models the firmware's `compute_pwm_params()` — including
  stopping the divider search at 255 rather than the firmware's 4095 — so
  the full-scale duty it reports is realistic and a test cannot pass on a
  configuration that would take down a real board.

- A `pwm_fade` sample under `samples/`, build-only.
```

- [ ] **Step 4: Normalize and run everything**

```bash
dos2unix zephyr/scripts/ci-build.sh zephyr/README.md zephyr/CHANGELOG.md
```

Run the full module suite to confirm nothing regressed:

```bash
wsl -d Ubuntu-26.04 -- bash -lc 'cd ~/zephyrproject/zephyr && source ../.venv/bin/activate; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr ZEPHYR_TOOLCHAIN_VARIANT=host; ./scripts/twister -p native_sim/native/64 -T /mnt/d/workspace/pico-de-gallo/zephyr/samples -T /mnt/d/workspace/pico-de-gallo/zephyr/tests --inline-logs --jobs 1'
```

Expected: every scenario passes or is built-not-run. Note that `spi_bridge` and `combined_i2c_spi_bridge` have no `tests.yaml` and are not picked up by twister at all, so their absence is correct.

- [ ] **Step 5: Commit**

```bash
git add zephyr/scripts/ci-build.sh zephyr/README.md zephyr/CHANGELOG.md
git commit -F - <<'MSG'
docs(zephyr): Document the PWM driver and register its CI targets

Adds the pwm_fake and pwm_fade rows to the ci-build.sh target table, and
bumps the self-test's expected target count to match.

Documents the driver in zephyr/README.md, which is authoritative for this
module -- there is no book chapter, per the AGENTS.md 15.1 carve-out --
including the unsupported-flag table issue #155 asks for.

The CHANGELOG entry records the two decisions most likely to be
second-guessed later: reporting the PWM source clock rather than the
counter rate as cycles-per-second, and deriving the frequency with a
ceiling. It also records that the period ceiling is containment of the
#192 firmware panic rather than a fix, since every other host surface can
still reach it.

Refs #155

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
MSG
```

---

## Done criteria

- [ ] `TWISTER` on `zephyr/tests/pdg_fake/pwm` passes every test case.
- [ ] `twister` across `zephyr/samples` and `zephyr/tests` shows no regression.
- [ ] `bash zephyr/scripts/ci-build.sh --self-test` passes.
- [ ] Every new file is LF (`file zephyr/drivers/pwm/*` reports no CRLF).
- [ ] No commit carries `Signed-off-by`; every commit carries both AI trailers.
- [ ] `zephyr/drivers/common/common.c` is **unmodified**.
- [ ] `git log --oneline main..issue-155` shows one logical change per commit.

## Deliberately not done

- **Fixing #192.** Firmware scope. The driver's period ceiling contains it for Zephyr consumers only.
- **Fixing the book's PWM drift.** `book/src/interfaces/pwm.md` shows C examples using `GalloPwmDutyCycleInfo` and `GalloPwmConfigurationInfo`, which do not exist, and documents `max_duty` as a constant 65535 when it is `top + 1`. Pre-existing, unrelated to this change; worth its own issue.
- **Hardware verification.** Not possible for PWM in this setup. The fake suite is the evidence.

## RESOLVED during plan self-review: the sibling re-assertion is deleted

**Decision: do not implement `pdg_pwm_reassert_sibling`. It is unreachable.** Task 4 and Task 5 below still describe it; ignore those parts and follow this section instead. Task 7's CHANGELOG wording is corrected here too.

The argument:

- `slices[s].period_cycles` is assigned only on a successful `set_config`.
- `channels[c].period_cycles` is assigned only after a successful `set_duty_cycle`, which follows that `set_config`.
- Therefore, whenever `channels[sibling].configured` is true, `slices[s].period_cycles == channels[sibling].period_cycles`.
- The conflict check refuses any request whose period differs from a configured sibling's.
- So a request that survives the conflict check always carries a period equal to `slices[s].period_cycles`, and `pdg_pwm_apply` skips the reconfiguration.
- Hence `reconfigured == true` and `channels[sibling].configured == true` cannot hold simultaneously, and the re-assertion never fires.

The error paths do not create an exception: a mismatched period is refused before `apply` is entered, so a partially applied reconfiguration cannot leave the slice on a period the sibling disagrees with.

**What this means concretely:**

1. In **Task 4**, do not add the `pdg_pwm_reassert_sibling` placeholder, and delete this line from `pdg_pwm_apply`:

   ```c
   	ret = pdg_pwm_reassert_sibling(dev, channel, max_duty, reconfigured);
   	if (ret < 0) {
   		goto out;
   	}
   ```

   The `reconfigured` local then has no remaining reader, so delete it too — both the declaration and the `reconfigured = true;` assignment. Leaving an unused variable would fail the build on `-Werror=unused-but-set-variable`.

2. In **Task 5**, skip the "Implement the sibling re-assertion" step and do not write `test_reconfiguration_reasserts_the_sibling_duty`. Keep everything else: the conflict refusal, `test_accepts_a_matching_sibling_period`, `test_different_slices_hold_independent_periods`, `test_slice_is_enabled_once`, `test_never_disables_a_slice`, `test_no_recorder_overflowed`.

3. Add this test in its place, which pins the invariant that makes the re-assertion unnecessary. If someone later relaxes the conflict rule, this fails and points at the consequence:

   ```c
   /*
    * The invariant that makes a sibling duty re-assertion unnecessary.
    *
    * pwm/set-config rescales BOTH channels' compares with truncating integer
    * division, which would drift a sibling's duty downward. This driver never
    * has to compensate, because a slice is never reconfigured while a sibling
    * is in use: the conflicting-period refusal guarantees any surviving
    * request already carries the slice's current period.
    *
    * If the conflict rule is ever relaxed, this test fails -- and the drift
    * becomes real, so a re-assertion would then be required.
    */
   ZTEST(pdg_fake_pwm, test_a_slice_is_never_reconfigured_while_a_sibling_is_in_use)
   {
   	struct pwm_counts before;

   	/* Both channels of slice 0 in use at the same period. */
   	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 4096U, 1024U, 0));
   	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 4096U, 2048U, 0));

   	before = snapshot_counts();

   	/* Any further request on either channel must either keep the period,
    	 * and so not reconfigure, or differ and be refused outright.
    	 */
   	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 4096U, 3072U, 0));
   	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
   		      "the slice was reconfigured while channel 1 was in use");

   	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, 8192U, 1024U, 0), -EINVAL);
   	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
   		      "a refused request still reconfigured the slice");
   }
   ```

4. In **Task 7**, replace the sentence in the CHANGELOG entry that reads *"After any reconfiguration the sibling's duty is re-asserted from the driver's own tracked ratio, because the firmware rescales compares with truncating integer division and drifts them downward"* with:

   > The firmware rescales both compares with truncating integer division on
   > every reconfiguration, which would drift a sibling's duty downward. This
   > driver never has to compensate: the conflicting-period refusal means a
   > slice is never reconfigured while its sibling is in use, so the drift is
   > unreachable rather than mitigated.

   Make the same correction in the README's PWM section.

---
