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

/*
 * TEST ISOLATION.
 *
 * pdg_pwm_fake_reset() clears the FAKE. It cannot clear the DRIVER.
 * struct pdg_pwm_data is a static per-device object created by
 * DEVICE_DT_INST_DEFINE and lives for the whole ztest binary, so
 * slices[].configured, slices[].period_cycles, slices[].enabled and every
 * channels[] entry persist across tests in whatever order twister runs them.
 *
 * The counters do start at zero in each test, because the before hook resets
 * the fake. But what the driver CHOOSES to do depends on state the reset did
 * not touch -- it skips a reconfiguration whose period is unchanged, and
 * enables a slice only once. So assert deltas, and where a test needs a
 * reconfiguration to happen, use a period no other test uses.
 */
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

/* A refusal must be local. If any of these reach the device, the driver has
 * spent a USB round trip to be told what it already knew -- and for the
 * over-long period, it would have panicked the firmware (#192).
 *
 * These are absolute-zero assertions on purpose, and they are the one place
 * the delta rule above does not apply: the before hook zeroed the counters,
 * and a refused request must have issued nothing at all.
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

/*
 * The boundary itself must be ACCEPTED. A guard that also refused the last
 * legal period would be untestably conservative.
 *
 * EXP_MAX_PERIOD is reserved to this test: it depends on the slice actually
 * being reconfigured, so no other test may drive channel 0 at this period or
 * the driver would correctly skip the set_config this asserts.
 */
ZTEST(pdg_fake_pwm, test_accepts_the_maximum_period)
{
	struct pwm_counts before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, EXP_MAX_PERIOD, 0U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 1,
		      "the maximum period must reconfigure the slice exactly once");
}

/* EXP_MIN_PERIOD is likewise reserved to this test; see above. */
ZTEST(pdg_fake_pwm, test_accepts_the_minimum_period)
{
	struct pwm_counts before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, EXP_MIN_PERIOD, 1U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 1,
		      "the minimum period must reconfigure the slice exactly once");
}

/*
 * Zephyr's pwm_set_cycles() wrapper screens pulse > period itself, before the
 * driver is called (include/zephyr/drivers/pwm.h). A test that went through
 * the wrapper would therefore pass without our driver checking anything.
 *
 * So this asserts both layers. First that the public path refuses it, which
 * is what a caller sees; then that the driver's own slot refuses it too, by
 * invoking the API directly and bypassing the wrapper. The second half is
 * what keeps our defence-in-depth check honest.
 */
ZTEST(pdg_fake_pwm, test_rejects_a_pulse_longer_than_the_period)
{
	const struct pwm_driver_api *api = DEVICE_API_GET(pwm, PWM_DEV);

	zassert_equal(pwm_set_cycles(PWM_DEV, 0U, 1500U, 1501U, 0), -EINVAL,
		      "the public wrapper must refuse a pulse longer than its period");

	zassert_equal(api->set_cycles(PWM_DEV, 0U, 1500U, 1501U, 0), -EINVAL,
		      "the driver's own slot must refuse it too, independently of "
		      "the wrapper");

	assert_nothing_reached_the_device();
}
