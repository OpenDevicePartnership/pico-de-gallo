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

#include <errno.h>
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

/* Channels 0+1 live on slice 0, channels 2+3 on slice 1. */
static inline uint32_t pdg_pwm_fake_slice_of(uint8_t channel)
{
	return (uint32_t)channel / PDG_PWM_CHANNELS_PER_SLICE;
}

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
		return -EINVAL;
	}

	if (model_compute_top(frequency_hz, &top) != 0) {
		/* What the firmware would report as InvalidConfiguration,
		 * mapped by pdg_common_status_to_errno() to -EINVAL.
		 */
		return -EINVAL;
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
		return -EINVAL;
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
