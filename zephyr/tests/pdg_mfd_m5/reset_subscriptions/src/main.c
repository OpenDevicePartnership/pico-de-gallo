/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * M5 phase 1 -- explicit firmware GPIO subscription reset.
 *
 * Acceptance spec §3. The parent's init performs the strict open; main() then
 * calls the reset endpoint exactly once. Both children are disabled in
 * reset.overlay, so nothing has touched chip-select pin 2 by the time the reset
 * runs -- which is the entire point of giving this its own image.
 *
 * "Idempotent" is conditional: reset is safe only while the firmware dispatcher
 * can service the endpoint. It cannot preempt an outstanding serial,
 * zero-timeout gpio/wait-*, which wedges dispatch device-wide. Reset affects
 * subscriptions ONLY -- not pin modes, pulls, output levels, SPI configuration,
 * or any Zephyr-side lock/latch state.
 *
 * Every failed check exits nonzero. M5_RESET_PASS is printed only after every
 * check has passed.
 */

#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

#include "posix_board_if.h"

#include "pdg_mfd.h"
#include "m5_bottom.h"

/*
 * Topology, asserted at build time as well as in the generated devicetree.
 * A child quietly re-enabled here would reintroduce exactly the init-order
 * conflict this image exists to avoid, and would do it silently.
 */
BUILD_ASSERT(DT_NODE_HAS_STATUS_OKAY(DT_NODELABEL(pdg0)),
	     "M5 reset image requires the odp,pico-de-gallo parent to be okay");
BUILD_ASSERT(!DT_NODE_HAS_STATUS_OKAY(DT_NODELABEL(pdg_gpio0)),
	     "M5 reset image requires pdg_gpio0 to be disabled: an enabled GPIO child "
	     "could touch chip-select pin 2 before the subscription reset runs");
BUILD_ASSERT(!DT_NODE_HAS_STATUS_OKAY(DT_NODELABEL(pdg_spi0)),
	     "M5 reset image requires pdg_spi0 to be disabled: pdg_spi_init() configures "
	     "every cs-gpios pin as an inactive output and would fail -EBUSY on a "
	     "monitored pin before the subscription reset runs");

int main(void)
{
	const struct device *const parent = DEVICE_DT_GET(DT_NODELABEL(pdg0));
	uint8_t reset_count = 0U;
	void *ctx;
	int ret;

	printk("M5_RESET_BEGIN\n");

	if (!device_is_ready(parent)) {
		printk("M5_RESET_FAIL reason=parent-not-ready device=%s\n", parent->name);
		posix_exit(1);
	}

	/*
	 * pdg_mfd.h contract: require readiness first, then borrow. A NULL
	 * context after a passing readiness check is an ownership invariant
	 * failure, not an expected case. The context is borrowed -- never close
	 * or free it.
	 */
	ctx = pdg_mfd_ctx(parent);
	if (ctx == NULL) {
		printk("M5_RESET_FAIL reason=null-context device=%s\n", parent->name);
		posix_exit(1);
	}

	ret = m5_bottom_reset_subscriptions(ctx, &reset_count);
	if (ret != 0) {
		printk("M5_RESET_FAIL reason=reset-endpoint errno=%d\n", ret);
		posix_exit(1);
	}

	/*
	 * The count is evidence, not decoration. Test design §6.4 part 3: a
	 * nonzero count after a supposedly-normal acceptance phase is direct
	 * evidence that T4's cleanup did not run. The runner carries this value
	 * into teardown.subscriptions_reset in the aggregate verdict.
	 */
	printk("M5_RESET_COUNT=%u\n", reset_count);
	printk("M5_RESET_PASS\n");

	posix_exit(0);

	return 0;
}
