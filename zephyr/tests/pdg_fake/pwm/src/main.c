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

/*
 * Duty accuracy across the range, including both endpoints. 2500 cycles is
 * reserved to this test, so the first iteration is guaranteed to configure the
 * slice and give the fake a full-scale duty to report.
 *
 * This half CANNOT detect a rounding regression, and says so rather than
 * pretending otherwise: at period 2500 the derived frequency is exactly
 * 150e6 / 2500 = 60000 Hz, the fake's divider search lands on top = 2499, and
 * max_duty is therefore exactly 2500. The scaling collapses to
 * (pulse * 2500 + 1250) / 2500, which is `pulse` whether the driver rounds
 * half up or truncates. Its job is coverage of the endpoints and the
 * monotonic middle; test_duty_rounding_is_half_up below is what pins the
 * policy.
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

/*
 * The scaling rounds HALF UP; it must not truncate.
 *
 * WHY THE HARD-CODED EXPECTATIONS.
 * --------------------------------
 * expected_compare() above recomputes the driver's own formula. If both it and
 * the driver truncated, they would agree and the test would pass -- which is
 * precisely how a rounding regression could ship unnoticed. So the primary
 * assertion here is against values worked out by hand, and expected_compare()
 * is kept only as a corroborating cross-check.
 *
 * WHY THESE PERIODS.
 * ------------------
 * A case only distinguishes the two policies when max_duty != period AND the
 * quotient is fractional. 7000 and 9000 are both used nowhere else in this
 * file, and both give a fractional half:
 *
 *   period 7000: frequency = ceil(150e6 / 7000) = 21429 Hz. The fake's
 *   divider search takes div = 1, raw = 150e6 / 21429 = 6999 (integer
 *   division, since 21429 * 7000 = 150003000 overshoots), top = 6998, so
 *   max_duty = 6999.
 *     pulse 3500 -> round: (3500*6999 + 3500) / 7000 = 24500000 / 7000 = 3500
 *                   trunc: 24496500 / 7000 = 3499
 *     pulse 1750 -> round: (1750*6999 + 3500) / 7000 = 12251750 / 7000 = 1750
 *                   trunc: 12248250 / 7000 = 1749
 *
 *   period 9000: frequency = ceil(150e6 / 9000) = 16667 Hz, raw = 8999,
 *   top = 8998, max_duty = 8999.
 *     pulse 4500 -> round: 40500000 / 9000 = 4500; trunc: 40495500 / 9000 = 4499
 *     pulse 2250 -> round: 20252250 / 9000 = 2250; trunc: 20247750 / 9000 = 2249
 *
 * Two independent periods on purpose, so the policy is not pinned by a single
 * data point.
 */
ZTEST(pdg_fake_pwm, test_duty_rounding_is_half_up)
{
	static const struct {
		uint32_t period;
		uint32_t pulse;
		uint16_t expected_max_duty;
		uint16_t expected_compare;
		uint16_t truncating_compare;
	} cases[] = {
		{ 7000U, 3500U, 6999U, 3500U, 3499U },
		{ 7000U, 1750U, 6999U, 1750U, 1749U },
		{ 9000U, 4500U, 8999U, 4500U, 4499U },
		{ 9000U, 2250U, 8999U, 2250U, 2249U },
	};

	ARRAY_FOR_EACH(cases, i) {
		uint32_t period = cases[i].period;
		uint32_t pulse = cases[i].pulse;
		uint16_t max_duty = 0U;
		uint16_t got = 0U;
		uint16_t want = 0U;

		zassert_ok(pwm_set_cycles(PWM_DEV, 0U, period, pulse, 0));

		/* The hand arithmetic above rests on this, so assert it rather
		 * than assume it: a change to the fake's divider model must
		 * fail here and not silently move the expected compares.
		 */
		zassert_ok(pdg_pwm_fake_max_duty_for(0U, &max_duty));
		zassert_equal(max_duty, cases[i].expected_max_duty,
			      "period %u modelled max_duty %u, expected %u; the "
			      "hand-computed compares below no longer apply",
			      period, max_duty, cases[i].expected_max_duty);

		last_duty_for(0U, &got);

		zassert_equal(got, cases[i].expected_compare,
			      "pulse %u of %u produced compare %u, expected %u. "
			      "A truncating scale would have produced %u.",
			      pulse, period, got, cases[i].expected_compare,
			      cases[i].truncating_compare);

		/* Corroboration only. This recomputes the driver's formula, so
		 * it cannot catch a shared truncation -- that is what the
		 * hard-coded value above is for.
		 */
		expected_compare(0U, pulse, period, &want);
		zassert_equal(got, want,
			      "pulse %u of %u: compare %u disagrees with the "
			      "recomputed %u", pulse, period, got, want);
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

/*
 * SLICE-SHARING TESTS -- WHY THEY ALL RUN ON SLICE 1 (CHANNELS 2 AND 3)
 * ---------------------------------------------------------------------
 * Every test above this point drives channel 0 and ONLY channel 0, so channel
 * 1 -- channel 0's slice sibling -- is never configured.
 *
 * That is load-bearing, not incidental. The driver's channel state is never
 * cleared (see the TEST ISOLATION note at the top of this file), so a channel
 * configured once stays configured for the rest of the binary. Configuring
 * channel 1 at any period P would therefore pin channel 0 to P permanently,
 * and every test above that drives channel 0 at some other period would start
 * failing with -EINVAL from the very conflict check these tests exist to
 * verify -- in whatever order twister happens to run them.
 *
 * So the sibling-pair tests use slice 1, which nothing else touches, and
 * NOTHING in this file ever configures channel 1.
 *
 * Slice 1 carries the same hazard internally: once channel 3 is configured at
 * SIB_PERIOD, channel 2 can only ever use SIB_PERIOD. All of these tests
 * therefore share one period and assert deltas, so each one holds whether it
 * runs first or last.
 *
 * SIB_PERIOD (4096) and SIB_OTHER_PERIOD (8192, 4097) are used nowhere else in
 * this file; verified by grep before they were chosen.
 */
#define SIB_PERIOD       4096U
#define SIB_OTHER_PERIOD 8192U

/*
 * Two channels on one slice cannot hold independent periods. Refusing is
 * better than reconfiguring: silently changing channel 2's period because
 * channel 3 asked for a different one is a defect its owner cannot see.
 *
 * 4097 is this test's own conflicting period, used nowhere else.
 */
ZTEST(pdg_fake_pwm, test_rejects_a_conflicting_sibling_period)
{
	struct pwm_counts before;

	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 500U, 0));

	before = snapshot_counts();

	zassert_equal(pwm_set_cycles(PWM_DEV, 3U, 4097U, 500U, 0), -EINVAL,
		      "channel 3 must not be allowed a period differing from "
		      "its slice sibling channel 2's");
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
		      "the refused request still reconfigured the slice");
	zassert_equal(pdg_pwm_fake_set_duty_count() - before.set_duty, 0,
		      "the refused request still wrote a duty cycle");
	zassert_equal(pdg_pwm_fake_enable_count() - before.enable, 0,
		      "the refused request still enabled the slice");
}

/* The same period on a sibling is fine, and must not reconfigure. */
ZTEST(pdg_fake_pwm, test_accepts_a_matching_sibling_period)
{
	struct pwm_counts before;

	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 250U, 0));

	before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 3U, SIB_PERIOD, 750U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
		      "a sibling with a matching period must not reconfigure");
}

/*
 * Channels on DIFFERENT slices are independent: configuring slice 1 must not
 * disturb slice 0, and the two must be able to hold different periods at the
 * same time.
 *
 * 3201 is reserved to this test, so the first drive is guaranteed to
 * reconfigure slice 0 and the third is a genuine no-op rather than an accident
 * of what an earlier test left behind.
 */
ZTEST(pdg_fake_pwm, test_different_slices_hold_independent_periods)
{
	struct pwm_counts before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 3201U, 500U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 1,
		      "a period unique to this test must reconfigure slice 0");

	/* A different period on the other slice, which must be accepted. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 500U, 0));

	before = snapshot_counts();

	/* Slice 0 must still be on 3201, untouched by the slice 1 work. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 3201U, 250U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
		      "configuring slice 1 disturbed slice 0's period");
}

/*
 * The invariant that makes a sibling duty re-assertion unnecessary.
 *
 * pwm/set-config rescales BOTH channels' compares with truncating integer
 * division, which would drift a sibling's duty downward. This driver never
 * has to compensate, because a slice is not reconfigured while a sibling
 * is in use: the conflicting-period refusal guarantees any surviving
 * request already carries the slice's current period.
 *
 * If the conflict rule is ever relaxed, this test fails -- and the drift
 * becomes real, so a re-assertion would then be required.
 *
 * The invariant holds unconditionally, not merely along success paths. It
 * rests on slices[].period_cycles agreeing with channels[].period_cycles, and
 * the driver keeps those two in agreement by forgetting BOTH whenever a
 * sequence fails after set_config has already succeeded.
 * test_a_failed_sequence_does_not_leave_stale_slice_state covers that failure
 * path; this test covers the success paths reachable through the public API.
 */
ZTEST(pdg_fake_pwm, test_a_slice_is_never_reconfigured_while_a_sibling_is_in_use)
{
	struct pwm_counts before;

	/* Both channels of slice 1 in use at the same period. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 1024U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 3U, SIB_PERIOD, 2048U, 0));

	before = snapshot_counts();

	/* Any further request on either channel must either keep the period,
	 * and so not reconfigure, or differ and be refused outright.
	 */
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 3072U, 0));
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
		      "the slice was reconfigured while channel 3 was in use");

	zassert_equal(pwm_set_cycles(PWM_DEV, 2U, SIB_OTHER_PERIOD, 1024U, 0),
		      -EINVAL);
	zassert_equal(pdg_pwm_fake_set_config_count() - before.set_config, 0,
		      "a refused request still reconfigured the slice");
}

/*
 * A sequence that fails AFTER set_config succeeded must not leave the driver
 * claiming knowledge it no longer has.
 *
 * slices[].period_cycles is committed right after a successful set_config;
 * channels[].period_cycles only after the set_duty_cycle that follows it. A
 * failure in between used to leave the two disagreeing, and because the
 * conflict check reads the CHANNEL while the reconfiguration test reads the
 * SLICE, the disagreement was directly observable: a request for the period
 * the slice is genuinely on was refused as "conflicting" with a sibling
 * record that no longer described anything.
 *
 * The driver now invalidates the whole slice on any such failure, so this
 * asserts the observable consequence of that: after the failure, driving the
 * sibling at the slice's real period is accepted rather than refused.
 *
 * PERIODS. 7001 and 7002 are used nowhere else in this file.
 *
 * CHANNEL 1. This is the ONLY test that touches channel 1, and it must leave
 * it unconfigured or every channel-0 test above would be pinned to 7002 for
 * the rest of the binary. The trailing scrub is what guarantees that: it
 * forces one more post-set_config failure on slice 0, whose invalidation
 * clears BOTH channels. Do not delete it.
 */
ZTEST(pdg_fake_pwm, test_a_failed_sequence_does_not_leave_stale_slice_state)
{
	int ret;

	/* 1. A clean drive on channel 0, establishing 7001 on slice 0. */
	zassert_ok(pwm_set_cycles(PWM_DEV, 0U, 7001U, 100U, 0));

	/* 2. A re-drive at 7002 whose set_duty_cycle fails. set_config has
	 * already succeeded at this point, so the slice really is on 7002.
	 */
	pdg_pwm_fake_set_set_duty_result(-EIO);
	ret = pwm_set_cycles(PWM_DEV, 0U, 7002U, 100U, 0);
	pdg_pwm_fake_set_set_duty_result(0);
	zassert_equal(ret, -EIO,
		      "the scripted set_duty_cycle failure did not reach the "
		      "caller");

	/* 3. The sibling now asks for 7002 -- the period the slice is actually
	 * on. Stale channel-0 state would make this look like a conflict.
	 */
	zassert_ok(pwm_set_cycles(PWM_DEV, 1U, 7002U, 500U, 0),
		   "channel 1 was refused the period its slice is genuinely "
		   "on, because the failed sequence left channel 0 claiming a "
		   "period the slice no longer has");

	/* Scrub: invalidate slice 0 so channel 1 stops pinning channel 0.
	 *
	 * It must carry 7002, the period channel 1 now holds. Any other period
	 * would be turned away by the conflict check before it ever reached
	 * the invalidating failure path, which is the whole point of that
	 * check and was observed happening.
	 */
	pdg_pwm_fake_set_set_duty_result(-EIO);
	(void)pwm_set_cycles(PWM_DEV, 0U, 7002U, 0U, 0);
	pdg_pwm_fake_set_set_duty_result(0);
}

/*
 * The slice is enabled on first use, and at most once thereafter.
 *
 * WHY THIS IS NOT A DELTA ASSERTION ON THE FIRST DRIVE.
 * -----------------------------------------------------
 * It used to be, and it was vacuous: slices[].enabled survives across tests,
 * so an earlier test may already have enabled slice 1, which forced the first
 * assertion down to "no more than one". Deleting the production call to
 * pdg_pwm_bottom_enable() entirely made every delta 0, and the test still
 * passed. It proved nothing about enabling.
 *
 * It is now driven from a KNOWN not-yet-enabled state, and proves the enable
 * happens by its RETURN VALUE rather than by a counter. Getting there needs
 * two steps:
 *
 *   1. Force a post-set_config failure so pdg_pwm_forget_slice() runs. That
 *      clears slices[].enabled along with everything else, which is exactly
 *      what makes this testable -- nothing else in the driver ever clears it.
 *   2. Script enable itself to fail with a distinctive errno. If the driver
 *      calls enable, the caller sees that errno; if it does not, the call
 *      returns 0 and this test fails. There is no way to pass without the
 *      call.
 *
 * The "once, not every time" half then follows from the same known state: the
 * first successful drive must issue exactly one enable, and the updates after
 * it exactly zero.
 *
 * Every scripted result is restored before each assertion that could return
 * early, so an alphabetically later test cannot inherit a failure.
 */
ZTEST(pdg_fake_pwm, test_slice_is_enabled_once)
{
	struct pwm_counts before;
	int ret;

	/* 1. Put slice 1 back into a not-yet-enabled state. A set_duty failure
	 * is a post-set_config failure, so the driver forgets the slice --
	 * including its enabled flag.
	 */
	pdg_pwm_fake_set_set_duty_result(-EIO);
	ret = pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 250U, 0);
	pdg_pwm_fake_set_set_duty_result(0);
	zassert_equal(ret, -EIO,
		      "the scripted set_duty_cycle failure did not reach the "
		      "caller, so the slice was never invalidated");

	/* 2. THE non-vacuous assertion: a scripted enable failure must reach
	 * the caller. Only possible if the driver actually calls enable.
	 */
	pdg_pwm_fake_set_enable_result(-EPIPE);
	ret = pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 250U, 0);
	pdg_pwm_fake_set_enable_result(0);
	zassert_equal(ret, -EPIPE,
		      "a not-yet-enabled slice was driven and the scripted "
		      "enable failure did not reach the caller, so the driver "
		      "never enabled the slice at all");

	/* 3. The same drive with enable unscripted must succeed, and must
	 * issue exactly one enable -- the slice is still not enabled, because
	 * step 2's attempt failed.
	 */
	before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 250U, 0));
	zassert_equal(pdg_pwm_fake_enable_count() - before.enable, 1,
		      "the first successful drive on a not-yet-enabled slice "
		      "must enable it exactly once");

	/* 4. And every update after that must issue none. */
	before = snapshot_counts();

	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 500U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 3U, SIB_PERIOD, 500U, 0));
	zassert_equal(pdg_pwm_fake_enable_count() - before.enable, 0,
		      "the slice must be enabled once, not on every update");
}

/*
 * Nothing in this driver may ever disable a slice. The bottom header declares
 * no disable function, so this is structurally impossible today; the test
 * exists so that adding one is caught here rather than in the field, where it
 * would stop an unrelated channel.
 *
 * Absolute zero rather than a delta, and legitimately so: the count is not
 * merely zero at the start of this test, it is zero for the entire lifetime of
 * the binary.
 */
ZTEST(pdg_fake_pwm, test_never_disables_a_slice)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 500U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 0U, 0));
	zassert_ok(pwm_set_cycles(PWM_DEV, 3U, SIB_PERIOD, 0U, 0));
	zassert_equal(pdg_pwm_fake_disable_count(), 0,
		      "the driver disabled a slice, which stops the sibling "
		      "channel");
}

/* No log may have overflowed during any of the above. */
ZTEST(pdg_fake_pwm, test_no_recorder_overflowed)
{
	zassert_ok(pwm_set_cycles(PWM_DEV, 2U, SIB_PERIOD, 500U, 0));
	zassert_equal(pdg_pwm_fake_overflowed(), 0);
}
