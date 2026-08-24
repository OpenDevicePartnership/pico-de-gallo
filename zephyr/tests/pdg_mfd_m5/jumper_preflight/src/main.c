/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * M5 phase 2 -- physical fixture gate (T0, T1a, T1b, T1c).
 *
 * Nothing downstream is trusted until this image prints M5_JUMPER_PASS. A gate
 * failure makes fixture_validity FAIL and every other aggregate verdict
 * INCONCLUSIVE -- not FAIL, because a failure whose fixture was invalid is not
 * evidence of a driver defect either (test design §3).
 *
 * THE RULE THAT GOVERNS EVERY READ HERE (plan R2, CS-contract §8.11):
 * an RP2350 pull-down can *hold* a node that is already low, but it cannot
 * reliably pull an already-high node low, and a floating pad drifts high within
 * seconds. Therefore no read below configures a pull-down and expects LOW
 * without the node having been driven low first. Both prior attempts at this
 * fixture violated that rule. Every step names both pins' full configuration
 * rather than inheriting state, which is also what makes a restart after a
 * failed run sound -- this image performs no rollback (acceptance spec §5.1).
 *
 * Strong versus weak (test design §0.3, §3.1): a read is strong only when one
 * physical situation can produce it. "Pin 2 reads LOW against its own pull-up"
 * is strong, because only an active drive at the far end beats a pull-up. The
 * matching HIGH readings are weak on their own and are asserted only as the far
 * end of a transition.
 *
 * Every failed check exits nonzero immediately, logging pin, flags, expected and
 * observed level.
 */

#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/gpio.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

#include "posix_board_if.h"

#define M5_PIN2 GPIO_DT_SPEC_GET(DT_PATH(zephyr_user), m5_pin2_gpios)
#define M5_PIN3 GPIO_DT_SPEC_GET(DT_PATH(zephyr_user), m5_pin3_gpios)

BUILD_ASSERT(DT_NODE_HAS_STATUS_OKAY(DT_NODELABEL(pdg_gpio0)),
	     "M5 jumper image requires pdg_gpio0 to be okay");
BUILD_ASSERT(!DT_NODE_HAS_STATUS_OKAY(DT_NODELABEL(pdg_spi0)),
	     "M5 jumper image requires pdg_spi0 to be disabled: an enabled SPI "
	     "controller parks pin 2 as an inactive output at init, which would "
	     "invalidate every reading in this phase");

static const struct gpio_dt_spec m5_pin2 = M5_PIN2;
static const struct gpio_dt_spec m5_pin3 = M5_PIN3;

/* Configure one pin, failing the phase loudly on any error. */
static int m5_configure(const struct gpio_dt_spec *spec, const char *who,
			gpio_flags_t flags, const char *flags_text)
{
	int ret = gpio_pin_configure_dt(spec, flags);

	if (ret != 0) {
		printk("M5_JUMPER_FAIL step=configure pin=%s flags=%s errno=%d\n",
		       who, flags_text, ret);
	}

	return ret;
}

#define M5_CONFIGURE(spec, who, flags)					\
	m5_configure((spec), (who), (flags), #flags)

/*
 * GPIO_OUTPUT_LOW, never a bare GPIO_OUTPUT_INIT_LOW.
 *
 * GPIO_OUTPUT_INIT_LOW is BIT(18) -- the init-LEVEL bit alone. With neither
 * GPIO_INPUT (BIT(16)) nor GPIO_OUTPUT (BIT(17)) set, the request IS
 * GPIO_DISCONNECTED, and pdg_gpio.c:210 correctly rejects it with -ENOTSUP.
 * GPIO_OUTPUT_LOW is (GPIO_OUTPUT | GPIO_OUTPUT_INIT_LOW), which is what
 * "drive this pin low" actually spells.
 *
 * Observed on hardware during M5 phase 2: every one of the three drive steps
 * below is a STRONG reading, so the bare-init-bit spelling reduced the whole
 * gate to two released-node reads that prove nothing about the jumper. The
 * assertions below make the distinction a build failure rather than a runtime
 * -ENOTSUP.
 */
BUILD_ASSERT((GPIO_OUTPUT_LOW & GPIO_OUTPUT) == GPIO_OUTPUT,
	     "GPIO_OUTPUT_LOW must carry the GPIO_OUTPUT direction bit");
BUILD_ASSERT((GPIO_OUTPUT_INIT_LOW & (GPIO_INPUT | GPIO_OUTPUT)) == 0,
	     "GPIO_OUTPUT_INIT_LOW is an init-level bit only and is never a "
	     "valid pin_configure argument on its own; use GPIO_OUTPUT_LOW");

/* Read one pin and require an exact level. The pin must be an input: the GPIO
 * child masks an explicit output to zero in port_get_raw(), so reading an
 * output would report a confident falsehood rather than an error.
 */
static int m5_require_level(const struct gpio_dt_spec *spec, const char *who,
			    int expect, const char *step, const char *driven)
{
	int level = gpio_pin_get_dt(spec);

	if (level < 0) {
		printk("M5_JUMPER_FAIL step=%s pin=%s reason=read-error errno=%d\n",
		       step, who, level);
		return -1;
	}

	printk("M5_JUMPER_READ step=%s pin=%s driven=%s expected=%d observed=%d\n",
	       step, who, driven, expect, level);

	if (level != expect) {
		printk("M5_JUMPER_FAIL step=%s pin=%s driven=%s expected=%d observed=%d\n",
		       step, who, driven, expect, level);
		return -1;
	}

	return 0;
}

int main(void)
{
	int ret;

	printk("M5_JUMPER_BEGIN\n");

	if (!device_is_ready(m5_pin2.port)) {
		printk("M5_JUMPER_FAIL reason=gpio-port-not-ready port=%s\n",
		       m5_pin2.port->name);
		posix_exit(1);
	}

	/*
	 * T0 -- released-node baseline. Both pins input with their own pull-ups,
	 * nothing driven. Pull-UPs work normally on RP2350; only pull-downs are
	 * limited. A read path stubbed to 0 fails here; one stubbed to 1 fails
	 * T1a.1 and T1b.1. The pair is what makes both directions non-vacuous.
	 *
	 * Entry state matters here: pin 3 arrives from #104 acceptance as an
	 * output parked high (plan R7) and pin 2 may arrive monitored. This
	 * configure is the first act of the phase, so the parked state is
	 * overwritten before any read; and if the reset image did not run, the
	 * configure on pin 2 returns -EBUSY and this phase fails loudly, which
	 * is a useful cross-check that reset actually did something.
	 */
	ret = M5_CONFIGURE(&m5_pin2, "2", GPIO_INPUT | GPIO_PULL_UP);
	if (ret != 0) {
		posix_exit(1);
	}
	ret = M5_CONFIGURE(&m5_pin3, "3", GPIO_INPUT | GPIO_PULL_UP);
	if (ret != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin2, "2", 1, "T0", "nothing (both released, pulled up)") != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin3, "3", 1, "T0", "nothing (both released, pulled up)") != 0) {
		posix_exit(1);
	}

	/*
	 * T1a -- jumper proof, driven from pin 3. Pin 2 stays input/pull-up.
	 * T1a.1 is STRONG: only an active drive at pin 3 beats pin 2's own
	 * pull-up. A no-op write leaves the node high and fails.
	 */
	ret = M5_CONFIGURE(&m5_pin3, "3", GPIO_OUTPUT_LOW);
	if (ret != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin2, "2", 0, "T1a.1", "pin 3 drives LOW") != 0) {
		posix_exit(1);
	}

	ret = gpio_pin_set_dt(&m5_pin3, 1);
	if (ret != 0) {
		printk("M5_JUMPER_FAIL step=T1a.2 pin=3 reason=set-high errno=%d\n", ret);
		posix_exit(1);
	}
	/* Weak on its own; asserted as the far end of the 0 -> 1 transition. */
	if (m5_require_level(&m5_pin2, "2", 1, "T1a.2", "pin 3 drives HIGH") != 0) {
		posix_exit(1);
	}
	printk("M5_JUMPER_T1A_TRANSITION=LOW_TO_HIGH\n");

	/*
	 * T1b -- the same proof driven from the opposite end (design spec §7.6).
	 * Defeats a driver that only works for one pin index, or whose per-pin
	 * mask is wrong. Release pin 3 to an input first so that the node is not
	 * contended when pin 2 becomes an output.
	 */
	ret = M5_CONFIGURE(&m5_pin3, "3", GPIO_INPUT | GPIO_PULL_UP);
	if (ret != 0) {
		posix_exit(1);
	}
	ret = M5_CONFIGURE(&m5_pin2, "2", GPIO_OUTPUT_LOW);
	if (ret != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin3, "3", 0, "T1b.1", "pin 2 drives LOW") != 0) {
		posix_exit(1);
	}

	ret = gpio_pin_set_dt(&m5_pin2, 1);
	if (ret != 0) {
		printk("M5_JUMPER_FAIL step=T1b.2 pin=2 reason=set-high errno=%d\n", ret);
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin3, "3", 1, "T1b.2", "pin 2 drives HIGH") != 0) {
		posix_exit(1);
	}
	printk("M5_JUMPER_T1B_TRANSITION=LOW_TO_HIGH\n");

	/*
	 * T1c -- pull-down hold baseline. This is a FIXTURE assertion about the
	 * silicon, not driver coverage, and test design §9.2 explicitly refuses
	 * to count it toward the driver. It establishes that the RP2350
	 * pre-charge/hold model holds on this board today, which is what makes
	 * the later electrical reasoning sound.
	 *
	 * Order is the whole point: drive the node LOW first, and only then
	 * apply the pull-down. The pull-down is never asked to pull a high node
	 * down -- only to hold a low one.
	 */
	ret = M5_CONFIGURE(&m5_pin2, "2", GPIO_OUTPUT_LOW);
	if (ret != 0) {
		posix_exit(1);
	}
	ret = M5_CONFIGURE(&m5_pin3, "3", GPIO_INPUT | GPIO_PULL_DOWN);
	if (ret != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin3, "3", 0, "T1c.1",
			     "pin 2 drives LOW (pre-charge established)") != 0) {
		posix_exit(1);
	}

	/* Release pin 2 from its driven LOW into a pull-down. Both pull-downs
	 * now merely hold a node that is already low.
	 */
	ret = M5_CONFIGURE(&m5_pin2, "2", GPIO_INPUT | GPIO_PULL_DOWN);
	if (ret != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin2, "2", 0, "T1c.2",
			     "released from a driven LOW into a pull-down") != 0) {
		posix_exit(1);
	}
	if (m5_require_level(&m5_pin3, "3", 0, "T1c.2",
			     "released from a driven LOW into a pull-down") != 0) {
		posix_exit(1);
	}

	/*
	 * No rollback (acceptance spec §5.1). Pin modes and pulls are left
	 * mutated deliberately; the next jumper attempt establishes its own
	 * initial modes explicitly, and the acceptance image's SPI init parks
	 * pin 2 as an inactive output before anything reads it.
	 */
	printk("M5_JUMPER_PASS\n");

	posix_exit(0);

	return 0;
}
