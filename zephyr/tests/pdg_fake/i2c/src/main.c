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
 *
 * pdg_fake_reset() is deliberately NOT called here. The open happens during
 * POST_KERNEL device init, long before any ztest setup hook runs, so resetting
 * the counter would discard the very event under test.
 */
ZTEST(pdg_fake_i2c, test_weak_override_replaces_the_bottom_layer)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c0));

	zassert_true(device_is_ready(dev),
		     "I2C child not ready: the real bottom layer probably ran "
		     "and tried to open a USB device");
	zassert_true(pdg_fake_open_count() > 0,
		     "the fake's pdg_common_bottom_open() was never called, so "
		     "the weak override did not take effect");
}
