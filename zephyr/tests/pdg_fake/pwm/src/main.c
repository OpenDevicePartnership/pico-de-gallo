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

/*
 * What compare value the driver should have sent for this ratio, given the
 * full-scale duty the fake is modelling for that channel.
 *
 * Returns through an out-parameter rather than by value because ztest's
 * zassert_* macros expand to a bare `return;` on failure, which is only legal
 * in a void function.
 */
static void expected_compare(uint8_t channel, uint32_t pulse, uint32_t period,
			     uint16_t *out_compare)
{
	uint16_t max_duty = 0U;
	uint64_t scaled;

	zassert_ok(pdg_pwm_fake_max_duty_for(channel, &max_duty),
		   "slice for channel %u was never configured", channel);

	scaled = ((uint64_t)pulse * (uint64_t)max_duty) + ((uint64_t)period / 2U);

	*out_compare = (uint16_t)(scaled / (uint64_t)period);
}

/* The most recent compare value written to `channel`. Out-parameter for the
 * same reason as expected_compare().
 */
static void last_duty_for(uint8_t channel, uint16_t *out_duty)
{
	int len = pdg_pwm_fake_set_duty_log_len();

	for (int i = len - 1; i >= 0; i--) {
		uint8_t ch = 0U;
		uint16_t duty = 0U;

		zassert_ok(pdg_pwm_fake_set_duty_log_entry(i, &ch, &duty));
		if (ch == channel) {
			*out_duty = duty;
			return;
		}
	}

	zassert_unreachable("no set_duty_cycle recorded for channel %u", channel);
}

/*
 * The frequency the driver derives must be the CEILING of the division.
 * Flooring would lengthen the period, forcing a larger divider, and at the
 * maximum period it lands on 8 Hz -- which needs divider 287 and panics.
 * Ceiling lands on 9 Hz, which needs 255.
 *
 * EXP_MAX_PERIOD is reserved to test_accepts_the_maximum_period, which asserts
 * a reconfiguration delta on it. So this test brackets its measurement with a
 * scrub period of its own (1234, used nowhere else): the leading drive
 * guarantees the measured call reconfigures whatever an earlier test left
 * behind, and the trailing one puts slice 0 back on a period that is not
 * EXP_MAX_PERIOD, so this test cannot disarm that one whatever order twister
 * picks.
 */
ZTEST(pdg_fake_pwm, test_frequency_is_rounded_up)
{
	uint32_t freq = 0U;

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1234U, 0U, 0));
	pdg_pwm_fake_reset();

	/* 150e6 / 16711680 = 8.97..., so ceiling is 9 and floor would be 8. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, EXP_MAX_PERIOD, 0U, 0));
	zassert_ok(pdg_pwm_fake_last_set_config(NULL, &freq, NULL));
	zassert_equal(freq, 9U,
		      "expected the ceiling (9 Hz); got %u. A floor here would "
		      "select a divider the firmware panics on.", freq);

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1234U, 0U, 0));
}

/* 150000 cycles is reserved to this test, so the set_config it reads back is
 * guaranteed to have been issued.
 */
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
 *
 * The table's first entry is EXP_MIN_PERIOD and its last is EXP_MAX_PERIOD,
 * both of which other tests reserve. 4569 is this test's own scrub period: the
 * leading drive guarantees the first table entry reconfigures, and the
 * trailing one leaves slice 0 off EXP_MAX_PERIOD.
 */
ZTEST(pdg_fake_pwm, test_frequency_round_trip_never_undershoots)
{
	static const uint32_t periods[] = {
		2U, 3U, 100U, 1500U, 15000U, 150000U, 1500000U,
		15000000U, EXP_MAX_PERIOD,
	};

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 4569U, 0U, 0));

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

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 4569U, 0U, 0));
}

/* Duty accuracy across the range, including both endpoints. 2500 cycles is
 * reserved to this test, so the first iteration is guaranteed to configure the
 * slice and give the fake a full-scale duty to report.
 */
ZTEST(pdg_fake_pwm, test_duty_ratio_is_accurate)
{
	static const uint32_t pulses[] = { 0U, 1U, 250U, 500U, 750U, 999U, 1000U };
	const uint32_t period = 2500U;

	ARRAY_FOR_EACH(pulses, i) {
		uint32_t pulse = pulses[i];
		uint16_t got = 0U;
		uint16_t want = 0U;

		zassert_ok(pwm_set_cycles(PWM_DEV, 0U, period, pulse, 0));

		last_duty_for(0U, &got);
		expected_compare(0U, pulse, period, &want);

		zassert_equal(got, want,
			      "pulse %u of %u produced compare %u, expected %u",
			      pulse, period, got, want);
	}
}

/* A zero pulse must be a compare of 0, which is constant-low. It must NOT be
 * expressed by disabling the slice.
 */
ZTEST(pdg_fake_pwm, test_zero_pulse_is_a_zero_compare)
{
	uint16_t got = 1U;

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 1000U, 0U, 0));
	last_duty_for(0U, &got);
	zassert_equal(got, 0U);
	zassert_equal(pdg_pwm_fake_disable_count(), 0,
		      "a zero pulse must not disable the slice");
}

/* A full pulse must be a compare of exactly max_duty, which is constant-high.
 * 6789 cycles is reserved to this test so the fake has a modelled full-scale
 * duty to report.
 */
ZTEST(pdg_fake_pwm, test_full_pulse_is_full_scale)
{
	uint16_t max_duty = 0U;
	uint16_t got = 0U;

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 6789U, 6789U, 0));
	zassert_ok(pdg_pwm_fake_max_duty_for(0U, &max_duty));
	last_duty_for(0U, &got);
	zassert_equal(got, max_duty);
}

/* Reconfiguration is skipped when the period is unchanged: a repeated duty
 * update must not reconfigure the slice, which would rescale and drift.
 * 5678 cycles is reserved to this test, so the first drive is guaranteed to
 * reconfigure and the second is a genuine no-op rather than an accident of
 * whatever an earlier test left on the slice.
 */
ZTEST(pdg_fake_pwm, test_unchanged_period_does_not_reconfigure)
{
	struct pwm_counts before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 5678U, 250U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 1,
		      "a new period must reconfigure the slice exactly once");

	before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 5678U, 750U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
		      "changing only the pulse width must not reconfigure the slice");
}
