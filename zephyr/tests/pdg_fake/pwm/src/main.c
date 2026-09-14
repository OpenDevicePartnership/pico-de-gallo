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
