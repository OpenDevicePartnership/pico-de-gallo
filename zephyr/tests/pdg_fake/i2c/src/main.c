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
 * The open happens during POST_KERNEL device init, before any ztest setup
 * hook runs, so the count it produces cannot be re-established by a test.
 * pdg_fake_open_count() is latched against pdg_fake_reset() for exactly that
 * reason, which is what keeps this assertion independent of test order once
 * later suites start calling reset.
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

/*
 * Regression coverage for #102. i2c_burst_write() emits WRITE followed by
 * WRITE | STOP, and the driver must concatenate the two into ONE bus write.
 * Before the #102 fix the grouping refused that shape with -ENOTSUP.
 *
 * This is the first automated coverage of that regression: the pre-existing
 * zephyr/tests/pdg_i2c_burst suite needs an attached board, so CI only ever
 * built and linked it.
 */
ZTEST(pdg_fake_i2c, test_gather_write_concatenates_into_one_transfer)
{
	const struct device *dev = DEVICE_DT_GET(DT_NODELABEL(pdg_i2c0));
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
	zassert_equal(pdg_fake_i2c_write_read_count(), 0,
		      "the gather path must issue a plain write, not a write-read");

	len = pdg_fake_i2c_last_write(&addr, seen, sizeof(seen));
	zassert_equal(len, 3, "expected a 3-byte payload, saw %d", len);
	zassert_equal(addr, 0x48, "wrong target address");
	zassert_equal(seen[0], 0x02);
	zassert_equal(seen[1], 0x03);
	zassert_equal(seen[2], 0x00);
}
