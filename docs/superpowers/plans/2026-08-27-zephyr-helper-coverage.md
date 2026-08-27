# Zephyr hardware-free helper coverage — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the six `static` helpers in the Zephyr I2C and SPI drivers real
branch coverage, on a machine with no Pico de Gallo attached.

**Architecture:** A ztest suite on `native_sim/native/64` — which builds a full
Zephyr device model — drives the public driver API (`i2c_transfer()`,
`spi_transceive()`). The host-context bottom layer is replaced by a recording
fake, using `__attribute__((weak))` on the production definitions so the linker
prefers the test's strong ones. No production `CMakeLists.txt` changes.

**Tech Stack:** Zephyr `native_sim`, ztest, twister, CMake, C11.

**Spec:** `docs/superpowers/specs/2026-08-27-zephyr-helper-coverage-design.md`

## Global Constraints

- **LF line endings on every text file.** Run `dos2unix <file>` after creating
  one on Windows. AGENTS.md §3.
- **Conventional Commits with a scope**, `zephyr` for module files, `repo` for
  `.github/` and `docs/`. AGENTS.md §10.
- **AI-assisted commits carry `Assisted-by:` and
  `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`, and
  never `Signed-off-by:`.** AGENTS.md §4 rule 7.
- **Do not bump any `[package].version`.** AGENTS.md §4 rule 12.
- **Every file the embedded side includes from the host side must use only
  `stdint`/`stddef`/`stdbool` types.** The embedded half must never see
  `pico_de_gallo.h`. `zephyr/drivers/i2c/pdg_i2c_bottom.h:9-11`.
- **Use `__attribute__((weak))`, never Zephyr's `__weak`.** The bottom files are
  host-context and cannot include `zephyr/toolchain.h`.
- **Never add a `default:` inside a `switch` over `Status`.** `-Werror=switch`
  in `zephyr/drivers/CMakeLists.txt` is load-bearing. AGENTS.md §15.1.
- **Board is `native_sim/native/64`, shield is `pico_de_gallo`.**

## Verification environment — read before starting

Every task's test step needs a Zephyr workspace at the revision CI pins,
`26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0`. Two situations:

**If you have Linux with a Zephyr workspace:** run the commands as written. A
cycle is seconds.

**If you do not** (the author had Windows, no WSL, no workspace): every
verification is a push to a branch and a wait on `.github/workflows/zephyr.yml`,
roughly 15 minutes per cycle. In that case, do **not** follow the strict
red/green micro-cycle — batch each task's steps into one push and read the
result once. The `- [ ]` steps still define what must be true; only the
observation is batched. Say plainly in the PR body which mode you used.

Setting up the workspace:

```bash
git clone --filter=blob:none https://github.com/zephyrproject-rtos/zephyr zephyrproject/zephyr
git -C zephyrproject/zephyr checkout --detach 26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0
west init -l zephyrproject/zephyr
cd zephyrproject && west update --narrow -o=--filter=blob:none && west packages pip --install
export ZEPHYR_BASE=$PWD/zephyr ZEPHYR_TOOLCHAIN_VARIANT=host
```

---

## File structure

| File | Responsibility |
|---|---|
| `zephyr/drivers/common/common.c` | *modify* — weak-mark `pdg_common_bottom_open`/`_close` only |
| `zephyr/drivers/i2c/pdg_i2c_bottom.c` | *modify* — weak-mark the four transfer entry points |
| `zephyr/drivers/spi/pdg_spi_bottom.c` | *modify* — weak-mark the transfer entry points |
| `zephyr/tests/pdg_fake/common/pdg_fake_bottom.h` | FFI-free accessors the embedded test calls |
| `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c` | host-context strong overrides + recording |
| `zephyr/tests/pdg_fake/i2c/{CMakeLists.txt,prj.conf,fake.overlay,tests.yaml,src/main.c}` | the I2C ztest suite |
| `zephyr/tests/pdg_fake/spi/{...}` | the SPI ztest suite |
| `zephyr/scripts/ci-build.sh` | *modify* — add the new targets to `PDG_TARGETS` |
| `zephyr/README.md`, `zephyr/CHANGELOG.md` | *modify* — document the weak seam |

---

### Task 1: Prove weak override works across the native_simulator link

Everything else depends on this. `native_simulator` is built by a plain Makefile
(`%.c -> %.o`), not by CMake, and weak/strong resolution across that link is
unverified. If this task fails, **stop** and re-open the spec's §4.2 decision —
only the rejected Kconfig seam remains.

**Files:**
- Modify: `zephyr/drivers/common/common.c` (the two `pdg_common_bottom_*` definitions)
- Create: `zephyr/tests/pdg_fake/common/pdg_fake_bottom.h`
- Create: `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c`
- Create: `zephyr/tests/pdg_fake/i2c/CMakeLists.txt`
- Create: `zephyr/tests/pdg_fake/i2c/prj.conf`
- Create: `zephyr/tests/pdg_fake/i2c/fake.overlay`
- Create: `zephyr/tests/pdg_fake/i2c/tests.yaml`
- Create: `zephyr/tests/pdg_fake/i2c/src/main.c`

**Interfaces:**
- Consumes: nothing.
- Produces: `pdg_fake_reset(void)`, `pdg_fake_open_count(void)` — declared in
  `pdg_fake_bottom.h`, used by every later task.

- [ ] **Step 1: Weak-mark the two common bottom functions**

Edit `zephyr/drivers/common/common.c`. Change only these two definitions;
leave `pdg_common_status_to_errno` strong, because the test wants the real
`Status`-to-`errno` mapping.

```c
/* __attribute__((weak)), not Zephyr's __weak: this file is compiled in the
 * host context and must not include zephyr/toolchain.h. A test may link a
 * strong definition to keep gallo_init_strict() from opening a USB device.
 * See docs/superpowers/specs/2026-08-27-zephyr-helper-coverage-design.md 4.3.
 */
__attribute__((weak)) void *pdg_common_bottom_open(const char *serial)
{
    return (void *)pdg_registry_open(serial);
}

__attribute__((weak)) void pdg_common_bottom_close(void *ctx)
{
    pdg_registry_close((const struct PicoDeGallo *)ctx);
}
```

- [ ] **Step 2: Write the fake's embedded-facing header**

Create `zephyr/tests/pdg_fake/common/pdg_fake_bottom.h`. Basic C types only —
the embedded side includes this and must never see `pico_de_gallo.h`.

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Embedded-facing view of the recording fake that replaces the host-context
 * bottom layer. The fake runs in the host context and the ztest assertions run
 * in the embedded context, so they share no globals; everything the test needs
 * to assert on must come through an accessor declared here. Same pattern, and
 * same reason, as zephyr/tests/pdg_mfd_m5/common/m5_bottom.h.
 */

#ifndef PDG_FAKE_BOTTOM_H
#define PDG_FAKE_BOTTOM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Discard all recorded calls. Call from each test's setup. */
void pdg_fake_reset(void);

/* How many times pdg_common_bottom_open() was called. */
int pdg_fake_open_count(void);

/* How many times pdg_i2c_bottom_write() was called. */
int pdg_fake_i2c_write_count(void);

/* Copy the payload of the most recent pdg_i2c_bottom_write() into buf.
 * Returns the number of bytes written, or -1 if there was no such call or
 * the payload does not fit in buflen. Sets *addr to the target address.
 */
int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen);

#ifdef __cplusplus
}
#endif

#endif /* PDG_FAKE_BOTTOM_H */
```

- [ ] **Step 3: Write the fake's host-context implementation**

Create `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c`. Task 1 only needs the
`pdg_common_bottom_open`/`_close` override; the I2C recorders are added in
Task 2, so the counters they serve return zero for now.

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Recording fake for the host-context bottom layer.
 *
 * Compiled into native_simulator with the host C library. It provides STRONG
 * definitions of symbols the production bottom files define as weak, so the
 * linker prefers these and no production CMakeLists.txt needs a test-only
 * conditional.
 *
 * It deliberately does NOT link pico_de_gallo.h. Nothing here reaches the FFI,
 * which is the whole point: pdg_mfd.c calls pdg_common_bottom_open() directly,
 * so overriding it means gallo_init_strict() is never entered.
 */

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "pdg_fake_bottom.h"

/* Any non-NULL value. pdg_mfd_init() only checks for NULL, and no code path in
 * the test ever dereferences it, because every consumer of the context is also
 * overridden here.
 */
static int fake_ctx_token;

static int open_count;

void pdg_fake_reset(void)
{
	open_count = 0;
}

int pdg_fake_open_count(void)
{
	return open_count;
}

int pdg_fake_i2c_write_count(void)
{
	return 0;
}

int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen)
{
	(void)addr;
	(void)buf;
	(void)buflen;
	return -1;
}

/* Strong override of the weak definition in zephyr/drivers/common/common.c. */
void *pdg_common_bottom_open(const char *serial)
{
	(void)serial;
	open_count++;
	return &fake_ctx_token;
}

/* Strong override of the weak definition in zephyr/drivers/common/common.c. */
void pdg_common_bottom_close(void *ctx)
{
	(void)ctx;
}
```

- [ ] **Step 4: Write the devicetree overlay**

Create `zephyr/tests/pdg_fake/i2c/fake.overlay`. Model it on
`zephyr/tests/pdg_i2c_burst/burst.overlay` — read that file and mirror its
parent/child structure. It must enable the `odp,pico-de-gallo` MFD parent and
its `odp,pico-de-gallo-i2c` child, and must **not** enable GPIO or SPI.

Do not invent node names. Copy the shape from `burst.overlay` and drop the
TMP102/TMP117 target node, which this suite does not need because no real bus
traffic occurs.

- [ ] **Step 5: Write the CMakeLists**

Create `zephyr/tests/pdg_fake/i2c/CMakeLists.txt`.

```cmake
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_fake_i2c)

target_sources(app PRIVATE src/main.c)

# The embedded side calls the fake's accessors, so it needs the FFI-free header.
target_include_directories(app PRIVATE ${CMAKE_CURRENT_LIST_DIR}/../common)

# The fake itself runs in the host context, and provides STRONG definitions that
# override the weak ones in drivers/common/common.c and drivers/i2c/
# pdg_i2c_bottom.c. Same mechanism as zephyr/drivers/CMakeLists.txt.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../common/pdg_fake_bottom.c)
target_include_directories(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../common)
```

- [ ] **Step 6: Write prj.conf**

Create `zephyr/tests/pdg_fake/i2c/prj.conf`.

```
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

CONFIG_ZTEST=y
CONFIG_I2C=y
CONFIG_LOG=y
CONFIG_LOG_MODE_IMMEDIATE=y

# Disabled in fake.overlay; stated explicitly so an accidental re-enable is
# visible in review, matching the convention in the other test prj.conf files.
CONFIG_GPIO=n
CONFIG_SPI=n
```

- [ ] **Step 7: Write the failing test**

Create `zephyr/tests/pdg_fake/i2c/src/main.c`.

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 */

#include <zephyr/ztest.h>
#include <zephyr/device.h>
#include <zephyr/drivers/i2c.h>

#include "pdg_fake_bottom.h"

ZTEST_SUITE(pdg_fake_i2c, NULL, NULL, NULL, NULL, NULL);

/*
 * The load-bearing test of the whole design. If the fake's strong
 * pdg_common_bottom_open() did not override the weak one in
 * drivers/common/common.c, the real one runs, reaches gallo_init_strict(),
 * finds no board, and the parent is not ready -- so this fails at the
 * device_is_ready() assertion rather than at the count.
 */
ZTEST(pdg_fake_i2c, test_weak_override_replaces_the_bottom_layer)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));

	zassert_true(device_is_ready(dev),
		     "I2C child not ready: the real bottom layer probably ran "
		     "and tried to open a USB device");
	zassert_true(pdg_fake_open_count() > 0,
		     "the fake's pdg_common_bottom_open() was never called, so "
		     "the weak override did not take effect");
}
```

Replace `DT_NODELABEL(pdg_i2c)` with whatever label `fake.overlay` actually
gives the I2C child in Step 4.

- [ ] **Step 8: Write tests.yaml**

Create `zephyr/tests/pdg_fake/i2c/tests.yaml`.

```yaml
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT
#
# NOT build_only, unlike every other tests.yaml in this module. This suite is
# meant to run: the bottom layer is replaced by a recording fake, so nothing
# reaches gallo_init_strict() and no board is needed.
#
# No depends_on. native_sim/native/64 does not declare i2c in its supported:
# list -- only the 32-bit native_sim does -- so claiming it would filter this
# scenario to nothing, silently. See zephyr/samples/i2c_bridge/tests.yaml.
common:
  tags:
    - drivers
    - i2c
    - pico_de_gallo
tests:
  drivers.pico_de_gallo.i2c.fake:
    platform_allow:
      - native_sim/native/64
    integration_platforms:
      - native_sim/native/64
    extra_dtc_overlay_files:
      - fake.overlay
```

- [ ] **Step 9: Run the test**

```bash
cd zephyrproject
west twister -T "$PDG_ROOT/zephyr/tests/pdg_fake" -p native_sim/native/64 \
    --inline-logs --verbose
```

Expected: **PASS**, `1 of 1 executed test configurations passed`.

If it fails at `device_is_ready()`, the weak override did not take. Check the
link order and confirm `pdg_fake_bottom.c` reached `native_simulator`. If it
still fails, **stop and report** — the spec's §4.2 decision needs revisiting.

- [ ] **Step 10: Verify the fake is actually load-bearing**

A passing test proves nothing if the real bottom would also have passed.
Temporarily rename the fake's `pdg_common_bottom_open` to
`pdg_common_bottom_open_disabled`, re-run Step 9, and confirm the test now
**fails**. Then restore the name.

Record the observed failure message in the commit body. This is the mutation
control; without it, the task is not done.

- [ ] **Step 11: Commit**

```bash
dos2unix zephyr/drivers/common/common.c \
  zephyr/tests/pdg_fake/common/pdg_fake_bottom.h \
  zephyr/tests/pdg_fake/common/pdg_fake_bottom.c \
  zephyr/tests/pdg_fake/i2c/CMakeLists.txt \
  zephyr/tests/pdg_fake/i2c/prj.conf \
  zephyr/tests/pdg_fake/i2c/fake.overlay \
  zephyr/tests/pdg_fake/i2c/tests.yaml \
  zephyr/tests/pdg_fake/i2c/src/main.c
git add zephyr/drivers/common/common.c zephyr/tests/pdg_fake
git commit
```

Commit message subject: `test(zephyr): Replace the bottom layer with a weak-override fake`

The body must state the mutation-control result from Step 10, and must note
that this is the first test in the module that CI actually *runs* rather than
merely builds.

---

### Task 2: Record I2C traffic in the fake

**Files:**
- Modify: `zephyr/drivers/i2c/pdg_i2c_bottom.c`
- Modify: `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c`
- Modify: `zephyr/tests/pdg_fake/i2c/src/main.c`

**Interfaces:**
- Consumes: `pdg_fake_reset()`, `pdg_fake_open_count()` from Task 1.
- Produces: working `pdg_fake_i2c_write_count()` and
  `pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen)`,
  already declared in Task 1's header.

- [ ] **Step 1: Weak-mark the four I2C bottom entry points**

Edit `zephyr/drivers/i2c/pdg_i2c_bottom.c`. Add `__attribute__((weak))` to
`pdg_i2c_bottom_set_config`, `pdg_i2c_bottom_write`, `pdg_i2c_bottom_read` and
`pdg_i2c_bottom_write_read`. Leave `pdg_i2c_bottom_open` and
`pdg_i2c_bottom_close` alone — they delegate to the common pair, which Task 1
already made weak.

Add one comment above the group:

```c
/* These four are __attribute__((weak)) so a test can link strong definitions
 * and observe what the driver asked the bus to do. Not Zephyr's __weak: this
 * file is host-context and cannot include zephyr/toolchain.h. See
 * docs/superpowers/specs/2026-08-27-zephyr-helper-coverage-design.md 4.3.
 */
```

- [ ] **Step 2: Write the failing test**

Add to `zephyr/tests/pdg_fake/i2c/src/main.c`. This is the #102 regression
expressed as a unit test: `i2c_burst_write()` emits `WRITE` then
`WRITE | STOP`, and the driver must concatenate them into **one** bus write.

```c
ZTEST(pdg_fake_i2c, test_gather_write_concatenates_into_one_transfer)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));
	uint8_t reg = 0x02;
	uint8_t val[2] = { 0x03, 0x00 };
	struct i2c_msg msgs[2] = {
		{ .buf = &reg,  .len = 1U, .flags = I2C_MSG_WRITE },
		{ .buf = val,   .len = 2U, .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
	};
	uint8_t seen[8];
	uint16_t addr = 0U;
	int len;

	pdg_fake_reset();
	zassert_ok(i2c_transfer(dev, msgs, 2U, 0x48));

	zassert_equal(pdg_fake_i2c_write_count(), 1,
		      "expected exactly one bus write, saw %d",
		      pdg_fake_i2c_write_count());

	len = pdg_fake_i2c_last_write(&addr, seen, sizeof(seen));
	zassert_equal(len, 3, "expected a 3-byte payload, saw %d", len);
	zassert_equal(addr, 0x48, "wrong target address");
	zassert_equal(seen[0], 0x02);
	zassert_equal(seen[1], 0x03);
	zassert_equal(seen[2], 0x00);
}
```

- [ ] **Step 3: Run it and watch it fail**

```bash
cd zephyrproject
west twister -T "$PDG_ROOT/zephyr/tests/pdg_fake" -p native_sim/native/64 \
    -s drivers.pico_de_gallo.i2c.fake --inline-logs --verbose
```

Expected: FAIL at the write-count assertion, because
`pdg_fake_i2c_write_count()` is still the Task 1 stub returning 0.

- [ ] **Step 4: Implement the recorder**

Replace the three placeholder functions in
`zephyr/tests/pdg_fake/common/pdg_fake_bottom.c`, and add the four strong
overrides.

```c
#define FAKE_MAX_PAYLOAD 4096

static int i2c_write_count;
static uint16_t i2c_last_addr;
static uint8_t i2c_last_buf[FAKE_MAX_PAYLOAD];
static size_t i2c_last_len;
static int i2c_last_overflowed;

int pdg_fake_i2c_write_count(void)
{
	return i2c_write_count;
}

int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen)
{
	if (i2c_write_count == 0 || i2c_last_overflowed || i2c_last_len > buflen) {
		return -1;
	}

	if (addr != NULL) {
		*addr = i2c_last_addr;
	}

	memcpy(buf, i2c_last_buf, i2c_last_len);

	return (int)i2c_last_len;
}

/* Strong overrides of the weak definitions in drivers/i2c/pdg_i2c_bottom.c. */

int pdg_i2c_bottom_set_config(void *ctx, uint8_t frequency)
{
	(void)ctx;
	(void)frequency;
	return 0;
}

int pdg_i2c_bottom_write(void *ctx, uint16_t addr, const uint8_t *buf, size_t len)
{
	(void)ctx;

	i2c_write_count++;
	i2c_last_addr = addr;

	/* Record the overflow rather than truncating silently: a test that asks
	 * for a payload we could not store must fail, not pass on a prefix.
	 */
	i2c_last_overflowed = (len > FAKE_MAX_PAYLOAD);
	if (!i2c_last_overflowed) {
		i2c_last_len = len;
		if (len > 0U && buf != NULL) {
			memcpy(i2c_last_buf, buf, len);
		}
	}

	return 0;
}

int pdg_i2c_bottom_read(void *ctx, uint16_t addr, uint8_t *buf, size_t len)
{
	(void)ctx;
	(void)addr;

	/* Deterministic filler so a test can tell a real read from untouched
	 * memory without depending on what a peripheral would have returned.
	 */
	if (buf != NULL) {
		memset(buf, 0xA5, len);
	}

	return 0;
}

int pdg_i2c_bottom_write_read(void *ctx, uint16_t addr, const uint8_t *tx,
			      size_t txlen, uint8_t *rx, size_t rxlen)
{
	(void)ctx;

	pdg_i2c_bottom_write(ctx, addr, tx, txlen);

	if (rx != NULL) {
		memset(rx, 0xA5, rxlen);
	}

	return 0;
}
```

Extend `pdg_fake_reset()` to clear all of the new state:

```c
void pdg_fake_reset(void)
{
	open_count = 0;
	i2c_write_count = 0;
	i2c_last_addr = 0U;
	i2c_last_len = 0U;
	i2c_last_overflowed = 0;
	memset(i2c_last_buf, 0, sizeof(i2c_last_buf));
}
```

- [ ] **Step 5: Run it and watch it pass**

Same command as Step 3. Expected: PASS, both tests.

- [ ] **Step 6: Commit**

```bash
dos2unix zephyr/drivers/i2c/pdg_i2c_bottom.c \
  zephyr/tests/pdg_fake/common/pdg_fake_bottom.c \
  zephyr/tests/pdg_fake/i2c/src/main.c
git add zephyr/drivers/i2c/pdg_i2c_bottom.c zephyr/tests/pdg_fake
git commit
```

Subject: `test(zephyr): Cover the i2c_burst_write gather path against the fake`

Body must reference #102 and state that this is the first automated coverage of
that regression, which until now had only the board-attached
`zephyr/tests/pdg_i2c_burst`.

---

### Task 3: Cover the `validate_group_` rejection paths

**Files:**
- Modify: `zephyr/tests/pdg_fake/i2c/src/main.c`

**Interfaces:**
- Consumes: everything from Task 2.
- Produces: nothing new.

- [ ] **Step 1: Write the failing tests**

Append to `zephyr/tests/pdg_fake/i2c/src/main.c`. Each maps to one refusal in
`validate_group_`; grep for the symbol in `zephyr/drivers/i2c/pdg_i2c.c` and
confirm each `LOG_ERR` has a case here before writing them.

```c
ZTEST(pdg_fake_i2c, test_mid_group_read_is_rejected)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));
	uint8_t out = 0x00;
	uint8_t in[2];
	struct i2c_msg msgs[2] = {
		{ .buf = in,   .len = 2U, .flags = I2C_MSG_READ },
		{ .buf = &out, .len = 1U, .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
	};

	pdg_fake_reset();
	zassert_equal(i2c_transfer(dev, msgs, 2U, 0x48), -ENOTSUP,
		      "a read before the end of a group must be refused");
	zassert_equal(pdg_fake_i2c_write_count(), 0,
		      "validation must reject before any bus traffic");
}

ZTEST(pdg_fake_i2c, test_write_then_read_without_restart_is_rejected)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));
	uint8_t out = 0x00;
	uint8_t in[2];
	struct i2c_msg msgs[2] = {
		{ .buf = &out, .len = 1U, .flags = I2C_MSG_WRITE },
		{ .buf = in,   .len = 2U, .flags = I2C_MSG_READ | I2C_MSG_STOP },
	};

	pdg_fake_reset();
	zassert_equal(i2c_transfer(dev, msgs, 2U, 0x48), -ENOTSUP,
		      "changing direction inside a transaction requires "
		      "I2C_MSG_RESTART");
	zassert_equal(pdg_fake_i2c_write_count(), 0,
		      "validation must reject before any bus traffic");
}
```

The second assertion in each is the point. `validate_group_` is documented as a
complete pre-pass that runs before the mutex and before any FFI call; asserting
only the return code would not catch a regression that reordered validation
after the first write. That exact failure mode is what the 2026-08-26 row of
AGENTS.md §13.17 describes for `i2c/batch`.

- [ ] **Step 2: Run and confirm they pass**

```bash
cd zephyrproject
west twister -T "$PDG_ROOT/zephyr/tests/pdg_fake" -p native_sim/native/64 \
    -s drivers.pico_de_gallo.i2c.fake --inline-logs --verbose
```

These should pass immediately — the behaviour already exists. That is expected
and is not a TDD violation: these are characterisation tests pinning behaviour
that shipped in #147, not tests driving new code.

If either **fails**, do not adjust the test to match. Stop and report: the
driver disagrees with its own documentation.

- [ ] **Step 3: Commit**

```bash
dos2unix zephyr/tests/pdg_fake/i2c/src/main.c
git add zephyr/tests/pdg_fake/i2c/src/main.c
git commit
```

Subject: `test(zephyr): Pin the validate_group_ rejection paths`

---

### Task 4: Cover the overflow-safe running total

This is the highest-value single test in the plan. It pins the exact defect
called out in the 2026-08-27 row of AGENTS.md §13.17: introducing concatenation
invalidated every per-message bound, so two individually legal
`PDG_I2C_MAX_BUFFER`-sized writes must not merge into one illegal double-sized
one.

**Files:**
- Modify: `zephyr/tests/pdg_fake/i2c/src/main.c`

- [ ] **Step 1: Confirm the limit**

```bash
rg -n 'PDG_I2C_MAX_BUFFER' zephyr/drivers/i2c/pdg_i2c.c
```

Note the value. The test below assumes `4096`; if it differs, use the real one
and do not hard-code a stale number.

- [ ] **Step 2: Write the test**

```c
/* Two writes that are each individually legal must be rejected when their
 * concatenation is not. The buffers are static because two 4096-byte arrays do
 * not belong on the test stack; zephyr/tests/pdg_i2c_burst does the same.
 */
static uint8_t half_a[PDG_I2C_MAX_BUFFER];
static uint8_t half_b[PDG_I2C_MAX_BUFFER];

ZTEST(pdg_fake_i2c, test_concatenated_writes_respect_the_limit)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));
	struct i2c_msg msgs[2] = {
		{ .buf = half_a, .len = sizeof(half_a), .flags = I2C_MSG_WRITE },
		{ .buf = half_b, .len = sizeof(half_b),
		  .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
	};

	pdg_fake_reset();
	zassert_equal(i2c_transfer(dev, msgs, 2U, 0x48), -EMSGSIZE,
		      "two PDG_I2C_MAX_BUFFER writes concatenate to twice the "
		      "limit and must be refused");
	zassert_equal(pdg_fake_i2c_write_count(), 0,
		      "validation must reject before any bus traffic");
}

ZTEST(pdg_fake_i2c, test_a_single_max_sized_write_is_accepted)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));
	struct i2c_msg msgs[1] = {
		{ .buf = half_a, .len = sizeof(half_a),
		  .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
	};

	pdg_fake_reset();
	zassert_ok(i2c_transfer(dev, msgs, 1U, 0x48),
		   "one write at exactly the limit must still be accepted");
	zassert_equal(pdg_fake_i2c_write_count(), 1);
}
```

`PDG_I2C_MAX_BUFFER` is defined inside `pdg_i2c.c` and is not visible to the
test. If it is not exported, define a local constant with a comment naming the
source of truth, and add a `BUILD_ASSERT` if any header does expose it. Do not
silently duplicate the number without a pointer back.

The second test is not padding. Without it, a regression that rejected *all*
large writes would leave the first test green.

- [ ] **Step 3: Run**

Expected: both PASS.

- [ ] **Step 4: Verify the first test is load-bearing**

Temporarily change `half_b`'s length to `1U`, re-run, and confirm
`test_concatenated_writes_respect_the_limit` now **fails** with `0` instead of
`-EMSGSIZE`. Restore.

Record the result in the commit body.

- [ ] **Step 5: Commit**

Subject: `test(zephyr): Pin the overflow-safe I2C write total`

Body references the 2026-08-27 row of AGENTS.md §13.17 and #102.

---

### Task 5: Cover `speed_to_code_` and `freq_to_speed_`

**Files:**
- Modify: `zephyr/tests/pdg_fake/common/pdg_fake_bottom.{c,h}`
- Modify: `zephyr/tests/pdg_fake/i2c/src/main.c`

**Interfaces:**
- Produces: `int pdg_fake_i2c_last_frequency(void)` — returns the `frequency`
  byte of the most recent `pdg_i2c_bottom_set_config()` call, or `-1` if there
  was none.

- [ ] **Step 1: Add the accessor to the header**

In `pdg_fake_bottom.h`:

```c
/* The frequency byte of the most recent pdg_i2c_bottom_set_config() call:
 * 0 = Standard, 1 = Fast, 2 = Fast+. Returns -1 if there was no such call.
 */
int pdg_fake_i2c_last_frequency(void);
```

- [ ] **Step 2: Implement it**

In `pdg_fake_bottom.c`, add `static int i2c_last_frequency = -1;`, clear it to
`-1` in `pdg_fake_reset()`, set it in `pdg_i2c_bottom_set_config()`, and return
it from the accessor.

- [ ] **Step 3: Write the test**

```c
ZTEST(pdg_fake_i2c, test_bitrate_maps_to_the_wire_frequency_byte)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));

	pdg_fake_reset();
	zassert_ok(i2c_configure(dev, I2C_SPEED_SET(I2C_SPEED_STANDARD) | I2C_MODE_CONTROLLER));
	zassert_equal(pdg_fake_i2c_last_frequency(), 0, "Standard must map to 0");

	pdg_fake_reset();
	zassert_ok(i2c_configure(dev, I2C_SPEED_SET(I2C_SPEED_FAST) | I2C_MODE_CONTROLLER));
	zassert_equal(pdg_fake_i2c_last_frequency(), 1, "Fast must map to 1");

	pdg_fake_reset();
	zassert_ok(i2c_configure(dev, I2C_SPEED_SET(I2C_SPEED_FAST_PLUS) | I2C_MODE_CONTROLLER));
	zassert_equal(pdg_fake_i2c_last_frequency(), 2, "Fast+ must map to 2");
}

ZTEST(pdg_fake_i2c, test_unsupported_bitrate_is_refused)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c));

	pdg_fake_reset();
	zassert_equal(i2c_configure(dev, I2C_SPEED_SET(I2C_SPEED_HIGH) | I2C_MODE_CONTROLLER),
		      -EINVAL, "I2C_SPEED_HIGH is not supported by the bridge");
	zassert_equal(pdg_fake_i2c_last_frequency(), -1,
		      "a refused configure must not reach the bus");
}
```

Those three constants are stable C ABI and mirror the `I2cFrequency` wire enum,
whose variant order is itself ABI. AGENTS.md §8 and §6.1.

- [ ] **Step 4: Run**

Expected: PASS.

- [ ] **Step 5: Cover `freq_to_speed_` with per-scenario overlays**

`speed_to_code_` is now covered directly, but `freq_to_speed_` is not. It is
called only from init — `zephyr/drivers/i2c/pdg_i2c.c` calls
`freq_to_speed_(config->clock_frequency, &speed)` and then feeds the result to
`speed_to_code_` — so its branches are selected by a **devicetree** value, not
by a runtime call. One overlay can only reach one branch.

Twister covers this with multiple scenarios over one source directory.

First, latch the init-time value in the fake so `pdg_fake_reset()` cannot erase
it. In `pdg_fake_bottom.h`:

```c
/* The frequency byte of the FIRST pdg_i2c_bottom_set_config() call, which the
 * driver makes during init from the devicetree clock-frequency. Unlike
 * pdg_fake_i2c_last_frequency() this is never cleared by pdg_fake_reset(),
 * because the call happens before any test body runs. Returns -1 if init never
 * configured the bus.
 */
int pdg_fake_i2c_init_frequency(void);
```

In `pdg_fake_bottom.c`, add `static int i2c_init_frequency = -1;`, set it in
`pdg_i2c_bottom_set_config()` only when it is still `-1`, return it from the
accessor, and **do not** clear it in `pdg_fake_reset()`.

Create two more overlays alongside `fake.overlay`, identical except for the I2C
child's `clock-frequency`: `fake_fast.overlay` at `<400000>` and
`fake_fastplus.overlay` at `<1000000>`. Keep `fake.overlay` at `<100000>`.

Add the assertion to `src/main.c`:

```c
/* Which branch of freq_to_speed_ this exercises is chosen by the overlay, so
 * the expected value comes from the build. See tests.yaml.
 */
#ifndef EXPECTED_INIT_FREQUENCY
#define EXPECTED_INIT_FREQUENCY 0
#endif

ZTEST(pdg_fake_i2c, test_devicetree_clock_frequency_maps_to_the_wire_byte)
{
	zassert_equal(pdg_fake_i2c_init_frequency(), EXPECTED_INIT_FREQUENCY,
		      "devicetree clock-frequency mapped to %d, expected %d",
		      pdg_fake_i2c_init_frequency(), EXPECTED_INIT_FREQUENCY);
}
```

Then fan the scenario out in `zephyr/tests/pdg_fake/i2c/tests.yaml`:

```yaml
  drivers.pico_de_gallo.i2c.fake.fast:
    platform_allow:
      - native_sim/native/64
    extra_dtc_overlay_files:
      - fake_fast.overlay
    extra_configs:
      - CONFIG_COMPILER_OPT="-DEXPECTED_INIT_FREQUENCY=1"
  drivers.pico_de_gallo.i2c.fake.fastplus:
    platform_allow:
      - native_sim/native/64
    extra_dtc_overlay_files:
      - fake_fastplus.overlay
    extra_configs:
      - CONFIG_COMPILER_OPT="-DEXPECTED_INIT_FREQUENCY=2"
```

If `CONFIG_COMPILER_OPT` does not thread the define through, fall back to
deriving the expectation from devicetree in the test itself:

```c
	const uint32_t hz = DT_PROP(DT_NODELABEL(pdg_i2c), clock_frequency);
	const int expected = (hz == 100000U) ? 0 : (hz == 400000U) ? 1 : 2;
```

That fallback is preferable if it works, because it removes the risk of a
scenario asserting a constant that no longer matches its own overlay.

- [ ] **Step 6: Run all three scenarios**

```bash
cd zephyrproject
west twister -T "$PDG_ROOT/zephyr/tests/pdg_fake" -p native_sim/native/64 \
    --inline-logs --verbose
```

Expected: three I2C scenarios, all PASS. Confirm from the output that all three
actually ran — a scenario that silently filtered would leave a branch uncovered
while looking green.

- [ ] **Step 7: Commit**

Subject: `test(zephyr): Cover the I2C bitrate mapping against the fake`

Body notes that `freq_to_speed_` is covered by devicetree fan-out rather than by
a runtime call, because its input is a build-time property.

---

### Task 6: The SPI half

**Files:**
- Modify: `zephyr/drivers/spi/pdg_spi_bottom.c`
- Modify: `zephyr/tests/pdg_fake/common/pdg_fake_bottom.h`
- Modify: `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c`
- Create: `zephyr/tests/pdg_fake/spi/CMakeLists.txt`
- Create: `zephyr/tests/pdg_fake/spi/prj.conf`
- Create: `zephyr/tests/pdg_fake/spi/fake.overlay`
- Create: `zephyr/tests/pdg_fake/spi/tests.yaml`
- Create: `zephyr/tests/pdg_fake/spi/src/main.c`

**Interfaces:**
- Consumes: `pdg_fake_reset()` from Task 1.
- Produces: `pdg_fake_spi_transfer_count()`,
  `pdg_fake_spi_last_tx(uint8_t *buf, size_t buflen)`.

The SPI bottom is only two functions, and unlike I2C the transfer is **always
full duplex with a single length** — the driver supplies zero-filled TX scratch
for a read-only transfer and discard RX scratch for a write-only one:

```c
int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity);
int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf, uint8_t *read_buf, size_t len);
```

- [ ] **Step 1: Weak-mark both SPI bottom entry points**

Edit `zephyr/drivers/spi/pdg_spi_bottom.c`, adding `__attribute__((weak))` to
`pdg_spi_bottom_set_config` and `pdg_spi_bottom_transfer`, with the same
explanatory comment used in Task 2 Step 1.

- [ ] **Step 2: Add the SPI accessors to the fake's header**

In `zephyr/tests/pdg_fake/common/pdg_fake_bottom.h`:

```c
/* How many times pdg_spi_bottom_transfer() was called. */
int pdg_fake_spi_transfer_count(void);

/* Copy the write_buf of the most recent pdg_spi_bottom_transfer() into buf.
 * Returns the number of bytes copied, or -1 if there was no such call or the
 * payload does not fit in buflen.
 */
int pdg_fake_spi_last_tx(uint8_t *buf, size_t buflen);
```

- [ ] **Step 3: Implement them**

In `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c`:

```c
static int spi_transfer_count;
static uint8_t spi_last_tx[FAKE_MAX_PAYLOAD];
static size_t spi_last_len;
static int spi_last_overflowed;

int pdg_fake_spi_transfer_count(void)
{
	return spi_transfer_count;
}

int pdg_fake_spi_last_tx(uint8_t *buf, size_t buflen)
{
	if (spi_transfer_count == 0 || spi_last_overflowed || spi_last_len > buflen) {
		return -1;
	}

	memcpy(buf, spi_last_tx, spi_last_len);

	return (int)spi_last_len;
}

/* Strong overrides of the weak definitions in drivers/spi/pdg_spi_bottom.c. */

int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity)
{
	(void)ctx;
	(void)frequency;
	(void)phase;
	(void)polarity;
	return 0;
}

int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf, uint8_t *read_buf, size_t len)
{
	(void)ctx;

	spi_transfer_count++;

	spi_last_overflowed = (len > FAKE_MAX_PAYLOAD);
	if (!spi_last_overflowed) {
		spi_last_len = len;
		if (len > 0U && write_buf != NULL) {
			memcpy(spi_last_tx, write_buf, len);
		}
	}

	/* Deterministic filler, as for I2C: lets a test distinguish a real read
	 * from untouched memory.
	 */
	if (read_buf != NULL) {
		memset(read_buf, 0x5A, len);
	}

	return 0;
}
```

Add `#include <stdbool.h>` to the file, and extend `pdg_fake_reset()`:

```c
	spi_transfer_count = 0;
	spi_last_len = 0U;
	spi_last_overflowed = 0;
	memset(spi_last_tx, 0, sizeof(spi_last_tx));
```

- [ ] **Step 4: Create the SPI suite scaffolding**

Copy `zephyr/tests/pdg_fake/i2c/{CMakeLists.txt,tests.yaml}`, changing
`project(pdg_fake_i2c)` to `project(pdg_fake_spi)`, the scenario name to
`drivers.pico_de_gallo.spi.fake`, and the tags to `spi`.

`prj.conf`:

```
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

CONFIG_ZTEST=y
CONFIG_SPI=y
CONFIG_GPIO=y
CONFIG_LOG=y
CONFIG_LOG_MODE_IMMEDIATE=y

# I2C is disabled in fake.overlay; stated explicitly so an accidental re-enable
# is visible in review.
CONFIG_I2C=n
```

`CONFIG_GPIO=y` is **not optional**. `pdg_spi_bottom.h` records that chip select
left this interface: the driver drives every CS edge through the
`odp,pico-de-gallo-gpio` child named in the controller's `cs-gpios`. A SPI
overlay without a GPIO controller will not bind.

For `fake.overlay`, read
`zephyr/tests/pdg_mfd_m5/acceptance/acceptance.overlay` and mirror its
parent / GPIO / SPI structure. Do not invent node names or `cs-gpios` phandles.

- [ ] **Step 5: Write the failing tests**

Create `zephyr/tests/pdg_fake/spi/src/main.c`. Replace `DT_NODELABEL(pdg_spi)`
with the label the overlay actually uses.

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 */

#include <zephyr/ztest.h>
#include <zephyr/device.h>
#include <zephyr/drivers/spi.h>

#include "pdg_fake_bottom.h"

/* Mirrors PDG_SPI_MAX_BUFFER in zephyr/drivers/spi/pdg_spi.c, which is not
 * exported. That value is a containment limit for the 1015-byte device-wide
 * dispatcher wedge, not a duplex-capacity guarantee -- see the 2026-08-19 row
 * of AGENTS.md 13.17. Re-derive it from the driver before trusting this.
 */
#define FAKE_SPI_MAX_BUFFER 1013

static const struct spi_config cfg = {
	.frequency = 1000000U,
	.operation = SPI_WORD_SET(8) | SPI_OP_MODE_MASTER,
	.slave = 0U,
};

ZTEST_SUITE(pdg_fake_spi, NULL, NULL, NULL, NULL, NULL);

/*
 * flatten_tx_ skips a NULL buf->buf but still advances the offset, so a NULL
 * entry becomes a run of buf->len zero bytes rather than being elided. A test
 * that only checked the total length would miss an off-by-one in that advance,
 * which is why this asserts the bytes either side land at exact offsets.
 */
ZTEST(pdg_fake_spi, test_null_tx_buffer_becomes_a_zero_gap)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_spi));
	uint8_t head[2] = { 0xAA, 0xBB };
	uint8_t tail[2] = { 0xCC, 0xDD };
	struct spi_buf tx[3] = {
		{ .buf = head, .len = 2U },
		{ .buf = NULL, .len = 3U },
		{ .buf = tail, .len = 2U },
	};
	struct spi_buf_set tx_set = { .buffers = tx, .count = 3U };
	uint8_t seen[16];
	int len;

	pdg_fake_reset();
	zassert_ok(spi_write(dev, &cfg, &tx_set));
	zassert_equal(pdg_fake_spi_transfer_count(), 1);

	len = pdg_fake_spi_last_tx(seen, sizeof(seen));
	zassert_equal(len, 7, "2 + 3 + 2 bytes must be clocked, saw %d", len);

	zassert_equal(seen[0], 0xAA);
	zassert_equal(seen[1], 0xBB);
	zassert_equal(seen[2], 0x00, "the NULL buffer must clock zeros");
	zassert_equal(seen[3], 0x00);
	zassert_equal(seen[4], 0x00);
	zassert_equal(seen[5], 0xCC, "the offset must advance past the gap");
	zassert_equal(seen[6], 0xDD);
}

/* bufset_len_ refuses a set that claims buffers it does not have. */
ZTEST(pdg_fake_spi, test_nonzero_count_with_null_buffers_is_rejected)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_spi));
	struct spi_buf_set tx_set = { .buffers = NULL, .count = 1U };

	pdg_fake_reset();
	zassert_equal(spi_write(dev, &cfg, &tx_set), -EINVAL);
	zassert_equal(pdg_fake_spi_transfer_count(), 0,
		      "validation must reject before any transfer");
}

/* Accumulation, not a per-buffer bound: two halves that are each legal must be
 * refused when their total is not.
 */
static uint8_t spi_half_a[FAKE_SPI_MAX_BUFFER];
static uint8_t spi_half_b[FAKE_SPI_MAX_BUFFER];

ZTEST(pdg_fake_spi, test_accumulated_length_respects_the_limit)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_spi));
	struct spi_buf tx[2] = {
		{ .buf = spi_half_a, .len = sizeof(spi_half_a) },
		{ .buf = spi_half_b, .len = sizeof(spi_half_b) },
	};
	struct spi_buf_set tx_set = { .buffers = tx, .count = 2U };

	pdg_fake_reset();
	zassert_equal(spi_write(dev, &cfg, &tx_set), -EMSGSIZE);
	zassert_equal(pdg_fake_spi_transfer_count(), 0);
}

/* Without this, a regression rejecting ALL large transfers leaves the test
 * above green. bufset_len_ compares with '>', so exactly the limit is legal.
 */
ZTEST(pdg_fake_spi, test_a_transfer_at_exactly_the_limit_is_accepted)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_spi));
	struct spi_buf tx[1] = {
		{ .buf = spi_half_a, .len = sizeof(spi_half_a) },
	};
	struct spi_buf_set tx_set = { .buffers = tx, .count = 1U };

	pdg_fake_reset();
	zassert_ok(spi_write(dev, &cfg, &tx_set));
	zassert_equal(pdg_fake_spi_transfer_count(), 1);
}

/* unflatten_rx_ mirrors flatten_tx_: a NULL buf->buf is skipped, but the
 * offset still advances, so the following buffer receives the correct slice.
 */
ZTEST(pdg_fake_spi, test_null_rx_buffer_does_not_shift_later_buffers)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_spi));
	uint8_t tail[2] = { 0x00, 0x00 };
	struct spi_buf rx[2] = {
		{ .buf = NULL, .len = 3U },
		{ .buf = tail, .len = 2U },
	};
	struct spi_buf_set rx_set = { .buffers = rx, .count = 2U };

	pdg_fake_reset();
	zassert_ok(spi_read(dev, &cfg, &rx_set));

	/* The fake fills read_buf with 0x5A, so a correctly advanced offset
	 * leaves both tail bytes written. A dropped advance would still write
	 * them, so this asserts the transfer length too.
	 */
	zassert_equal(tail[0], 0x5A);
	zassert_equal(tail[1], 0x5A);
	zassert_equal(pdg_fake_spi_transfer_count(), 1);
}
```

- [ ] **Step 6: Run**

```bash
cd zephyrproject
west twister -T "$PDG_ROOT/zephyr/tests/pdg_fake" -p native_sim/native/64 \
    -s drivers.pico_de_gallo.spi.fake --inline-logs --verbose
```

Expected: PASS. If `test_a_transfer_at_exactly_the_limit_is_accepted` fails,
re-derive `PDG_SPI_MAX_BUFFER` from `zephyr/drivers/spi/pdg_spi.c` — the
mirrored constant has drifted.

- [ ] **Step 7: Verify the gap test is load-bearing**

Temporarily change `flatten_tx_` in `zephyr/drivers/spi/pdg_spi.c` so the
`offset += buf->len;` sits *inside* the `if (buf->buf != NULL)` block, re-run,
and confirm `test_null_tx_buffer_becomes_a_zero_gap` now fails on `seen[5]`.
Restore the driver.

Record the observed failure in the commit body. This is the only test in the
plan that distinguishes the two plausible readings of that loop.

- [ ] **Step 8: Commit**

```bash
dos2unix zephyr/drivers/spi/pdg_spi_bottom.c zephyr/tests/pdg_fake/common/pdg_fake_bottom.h \
  zephyr/tests/pdg_fake/common/pdg_fake_bottom.c zephyr/tests/pdg_fake/spi/CMakeLists.txt \
  zephyr/tests/pdg_fake/spi/prj.conf zephyr/tests/pdg_fake/spi/fake.overlay \
  zephyr/tests/pdg_fake/spi/tests.yaml zephyr/tests/pdg_fake/spi/src/main.c
git add zephyr/drivers/spi/pdg_spi_bottom.c zephyr/tests/pdg_fake
git commit
```

Subject: `test(zephyr): Cover the SPI buffer-set flatten and unflatten paths`

---

### Task 7: Wire the suites into ci-build.sh and document the seam

**Files:**
- Modify: `zephyr/scripts/ci-build.sh`
- Modify: `zephyr/README.md`
- Modify: `zephyr/CHANGELOG.md`

- [ ] **Step 1: Add both suites to the target table**

Edit `PDG_TARGETS` in `zephyr/scripts/ci-build.sh` (around line 68). Fields are
`name|kind|srcdir|overlay|zephyr_tus|native_objs|kconfigs`. Read the existing
`i2c_burst` row and mirror it.

Add a `pdg_fake_bottom` entry to the `native_objs` field of each new row, so
assertion 4 proves the fake actually reached the `native_simulator` link. That
assertion is the only mechanical guard that the weak override is still in play;
without it, a silent regression to the real bottom would only show up as a
mysterious hardware timeout.

- [ ] **Step 2: Check the translation-unit assertion**

`PDG_ALL_DRIVER_TUS` at `ci-build.sh:82` is `"pdg_mfd.c pdg_gpio.c pdg_i2c.c
pdg_spi.c"`. Assertion 3 is two-sided over **exactly** that set, so a file not
listed there escapes both halves silently.

This plan adds no new `pdg_*.c` driver translation unit, so no change is needed.
Confirm that is still true before ticking this step; if Task 6 ended up
extracting anything, add it here in the same commit.

- [ ] **Step 3: Run the whole gate**

```bash
zephyr/scripts/ci-build.sh --self-test
cd zephyrproject && "$PDG_ROOT/zephyr/scripts/ci-build.sh" --build-root /tmp/pdg-ci
```

Expected: `--self-test` passes with a target count of 11 rather than 9 — the
first self-test assertion checks the table length and **will fail** until you
update its expected value. That is intentional; it is the table's own guard.

- [ ] **Step 4: Document the weak seam**

`zephyr/README.md` needs a subsection under "Continuous integration" explaining
that some bottom-layer functions are `__attribute__((weak))` **on purpose**, why
it is not Zephyr's `__weak`, and that removing it silently disables the only
tests CI actually runs. Without this, the attribute reads as noise and someone
will delete it.

- [ ] **Step 5: Update the changelog**

Add an `### Added` entry to `zephyr/CHANGELOG.md` under `## [Unreleased]`.
State that these are the first tests in the module that CI executes rather than
merely builds, and that `#102`'s regression now has automated coverage.

- [ ] **Step 6: Commit**

Subject: `test(zephyr,repo): Run the fake-backed suites in CI`

---

## Done when

- `west twister -T zephyr/tests/pdg_fake -p native_sim/native/64` reports all
  scenarios **passed**, not merely built.
- `zephyr/scripts/ci-build.sh --self-test` passes.
- `.github/workflows/zephyr.yml`'s `twister` job is green on a PR.
- Every `zassert` that claims to be load-bearing has had its mutation control
  run and recorded in a commit body.

## Explicitly out of scope

- `checkpatch`, a devicetree-only tier, and the nightly self-hosted hardware
  tier. All remain open on #109.
- Any change to `zephyr/drivers/common/gallo_registry.c`. §4.3 of the spec
  explains why none is needed.
- Retiring `zephyr/tests/pdg_mfd_m5/run-m5.sh`. A stubbed bottom proves the
  driver's logic and never the wire behaviour of real hardware.
- Revisiting `build_only: true` on the seven pre-existing suites. Spec §6 item 3
  notes the fixture-gate assumption behind it was never tested; it is unrelated
  to this plan's goal and should be settled on its own.
- Patching `unittest.cmake` upstream. Spec §6 item 4 — run 3 of the spike showed
  that adding `syscalls/device.h` alone is not sufficient, so a patch that
  genuinely enables driver unit tests is a larger piece of work than this plan
  should promise.
