/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Pico de Gallo PWM controller.
 *
 * Embedded (Zephyr) half. It must never include pico_de_gallo.h; everything
 * that reaches the FFI goes through pdg_pwm_bottom.h.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_pwm

#include <zephyr/device.h>
#include <zephyr/drivers/pwm.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

#include <string.h>

#include "pdg_pwm_bottom.h"

/*
 * Structural topology enforcement, ordered compatible -> parent status ->
 * parent serial presence -> Kconfig. That source order is the contract: the
 * first failing assertion is the one a misconfigured tree should see.
 *
 * The block precedes the "pdg_mfd.h" include on purpose: when
 * CONFIG_MFD_PICO_DE_GALLO is n the MFD subdirectory is not added to the build
 * at all, so pdg_mfd.h is not on the include path and a tree that got the
 * Kconfig wrong would fail with a confusing missing-header error instead of
 * the assertion written to explain it.
 */
#define PDG_PWM_PARENT_ASSERTS(inst)						\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_INST_PARENT(inst), odp_pico_de_gallo),	\
		"Enabled odp,pico-de-gallo-pwm controllers must be direct "	\
		"children of an odp,pico-de-gallo parent");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(DT_INST_PARENT(inst)),			\
		"Enabled odp,pico-de-gallo-pwm controllers require their "	\
		"odp,pico-de-gallo parent to have status okay");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_PROP(DT_INST_PARENT(inst), serial_number),		\
		"odp,pico-de-gallo-pwm parent must define serial-number");	\
	BUILD_ASSERT(								\
		IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO),				\
		"Enabled Pico de Gallo child controllers require "		\
		"CONFIG_MFD_PICO_DE_GALLO=y");

DT_INST_FOREACH_STATUS_OKAY(PDG_PWM_PARENT_ASSERTS)

#include "pdg_mfd.h"

LOG_MODULE_REGISTER(pwm_pico_de_gallo, CONFIG_PWM_LOG_LEVEL);

struct pdg_pwm_config {
	const struct device *mfd;
	const char *serial_number;
};

/* What a channel was last asked for, used to detect a period conflicting with
 * the slice sibling's.
 */
struct pdg_pwm_channel_state {
	uint32_t period_cycles;
	uint32_t pulse_cycles;
	bool configured;
};

/* Slice-wide state. A slice has one period and one enable. */
struct pdg_pwm_slice_state {
	uint32_t period_cycles;
	bool configured;
	bool enabled;
};

struct pdg_pwm_data {
	void *ctx;
	struct k_mutex lock;
	struct pdg_pwm_channel_state channels[PDG_PWM_NUM_CHANNELS];
	struct pdg_pwm_slice_state slices[PDG_PWM_NUM_SLICES];
};

static inline uint32_t pdg_pwm_slice_of(uint32_t channel)
{
	return channel / PDG_PWM_CHANNELS_PER_SLICE;
}

/* The other channel on the same slice. Valid only for a two-channel slice,
 * which is what PDG_PWM_CHANNELS_PER_SLICE asserts below.
 */
static inline uint32_t pdg_pwm_sibling_of(uint32_t channel)
{
	return channel ^ 1U;
}

BUILD_ASSERT(PDG_PWM_CHANNELS_PER_SLICE == 2U,
	     "pdg_pwm_sibling_of() computes the sibling with XOR 1, which is only "
	     "correct for two channels per slice");

static int pdg_pwm_get_cycles_per_sec(const struct device *dev, uint32_t channel,
				      uint64_t *cycles)
{
	struct pdg_pwm_data *data = dev->data;

	if (data->ctx == NULL) {
		return -ENODEV;
	}

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		LOG_ERR("%s: channel %u is out of range (0 to %u).", dev->name,
			channel, PDG_PWM_NUM_CHANNELS - 1U);
		return -EINVAL;
	}

	if (cycles == NULL) {
		return -EINVAL;
	}

	/*
	 * The SOURCE clock, constant for every channel and every
	 * configuration. See the rationale on PDG_PWM_CYCLES_PER_SEC.
	 */
	*cycles = (uint64_t)PDG_PWM_CYCLES_PER_SEC;

	return 0;
}

/*
 * CEILING, not floor, and this is load-bearing.
 *
 * Flooring lengthens the period, which forces the firmware to choose a larger
 * clock divider. At the maximum supported period it yields 8 Hz, which needs
 * a divider of 287 -- above the 255 the hardware can hold, and embassy-rp
 * panics rather than refusing. Ceiling yields 9 Hz, which needs exactly 255.
 *
 * It also matches the firmware's own behaviour: because it floors `top`, its
 * achieved frequency is always greater than or equal to the requested one.
 *
 * period_cycles is bounded to at least PDG_PWM_MIN_PERIOD_CYCLES by the
 * caller, so there is no division by zero here.
 */
static uint32_t pdg_pwm_frequency_for(uint32_t period_cycles)
{
	uint64_t numerator = (uint64_t)PDG_PWM_CYCLES_PER_SEC +
			     (uint64_t)period_cycles - 1U;

	return (uint32_t)(numerator / (uint64_t)period_cycles);
}

/*
 * Scale a pulse width into the firmware's raw compare domain.
 *
 * Round half up rather than truncating. The firmware already truncates when it
 * rescales compares across a reconfiguration, and truncating here too would
 * compound a downward bias on every duty cycle.
 *
 * The clamp is belt and braces: pulse_cycles <= period_cycles is enforced by
 * the caller, so the quotient cannot exceed max_duty. The firmware would
 * silently clamp an over-range value anyway, which is exactly why the driver
 * must not rely on it to catch a mistake.
 */
static uint16_t pdg_pwm_compare_for(uint32_t pulse_cycles, uint32_t period_cycles,
				    uint16_t max_duty)
{
	uint64_t scaled = ((uint64_t)pulse_cycles * (uint64_t)max_duty) +
			  ((uint64_t)period_cycles / 2U);
	uint64_t compare = scaled / (uint64_t)period_cycles;

	if (compare > (uint64_t)max_duty) {
		compare = (uint64_t)max_duty;
	}

	return (uint16_t)compare;
}

/*
 * Drop everything the driver believes about a slice and both its channels.
 *
 * Called on any failure that occurs AFTER set_config has already succeeded.
 * At that point the device state is indeterminate: the reconfiguration was
 * applied but the duty was not, or could not be read back. Continuing to
 * claim knowledge of it is what desynchronises slices[] from channels[].
 *
 * The alternative -- rolling slices[s].period_cycles back to its previous
 * value -- is deliberately NOT taken. It would make the shadow lie about
 * hardware that really was reconfigured, so a later request for the old period
 * would skip the reconfiguration and silently run at the wrong frequency.
 * Forgetting is strictly safer than remembering something false.
 *
 * Forgetting costs nothing: pwm_set_cycles() always supplies both period and
 * pulse, so the next call on either channel re-establishes everything. That is
 * also why this is applied unconditionally on those failure paths rather than
 * only when the set_config branch actually ran: distinguishing the two would
 * need another flag to buy back one redundant reconfiguration.
 *
 * `enabled` is NOT cleared. The slice really is enabled, that is a fact about
 * the device rather than a cached period, and nothing in this driver can
 * disable it. Clearing it would only buy a redundant enable.
 *
 * The caller holds data->lock.
 */
static void pdg_pwm_forget_slice(struct pdg_pwm_data *data, uint32_t slice)
{
	uint32_t base = slice * PDG_PWM_CHANNELS_PER_SLICE;

	data->slices[slice].configured = false;

	for (uint32_t i = 0U; i < PDG_PWM_CHANNELS_PER_SLICE; i++) {
		data->channels[base + i].configured = false;
	}
}

/*
 * Configure the slice if its period changed, scale the pulse against the
 * full-scale duty the firmware reports, then enable the slice on first use.
 *
 * There is deliberately no sibling duty re-assertion here. pwm/set-config
 * rescales BOTH channels' compares with truncating integer division, which
 * would drift a sibling's duty downward -- but this driver never has to
 * compensate, because a slice is not reconfigured while a sibling is in use.
 * The conflicting-period refusal guarantees a request that reaches this
 * function already carries the slice's current period whenever the sibling is
 * configured, so the reconfiguration branch below is skipped in exactly the
 * cases a re-assertion would have been needed.
 *
 * That guarantee rests on an invariant the failure paths must maintain:
 * whenever channels[sibling].configured is true, slices[s].configured is true
 * and slices[s].period_cycles == channels[sibling].period_cycles. The two
 * fields are committed at DIFFERENT points -- the slice's right after a
 * successful set_config, the channel's only after the set_duty_cycle that
 * follows it -- so every failure between those two points calls
 * pdg_pwm_forget_slice() to drop both, rather than leaving them disagreeing.
 * A failure of set_config ITSELF needs no invalidation, because it returns
 * before either field is written and nothing was committed. See issue #155.
 */
static int pdg_pwm_apply(const struct device *dev, uint32_t channel,
			 uint32_t period_cycles, uint32_t pulse_cycles)
{
	struct pdg_pwm_data *data = dev->data;
	uint32_t slice = pdg_pwm_slice_of(channel);
	uint16_t max_duty = 0U;
	uint16_t current_duty = 0U;
	int ret;

	k_mutex_lock(&data->lock, K_FOREVER);

	if (!data->slices[slice].configured ||
	    data->slices[slice].period_cycles != period_cycles) {
		ret = pdg_pwm_bottom_set_config(data->ctx, (uint8_t)channel,
						pdg_pwm_frequency_for(period_cycles),
						false);
		if (ret < 0) {
			LOG_ERR("%s: channel %u failed to configure the slice: "
				"errno=%d.", dev->name, channel, ret);
			goto out;
		}

		data->slices[slice].configured = true;
		data->slices[slice].period_cycles = period_cycles;
	}

	/*
	 * max_duty is read back rather than computed. It is top + 1, and `top`
	 * is whatever the firmware's divider search settled on -- which the
	 * driver cannot predict without duplicating that search, and must not,
	 * because a duplicate would drift from it silently.
	 */
	ret = pdg_pwm_bottom_get_duty_cycle(data->ctx, (uint8_t)channel,
					    &current_duty, &max_duty);
	if (ret < 0) {
		LOG_ERR("%s: channel %u failed to read the full-scale duty: "
			"errno=%d.", dev->name, channel, ret);
		pdg_pwm_forget_slice(data, slice);
		goto out;
	}

	if (max_duty == 0U) {
		LOG_ERR("%s: channel %u: the firmware reported a full-scale duty "
			"of zero, which would make every duty cycle a division "
			"by zero. Returning -EIO.", dev->name, channel);
		ret = -EIO;
		pdg_pwm_forget_slice(data, slice);
		goto out;
	}

	ret = pdg_pwm_bottom_set_duty_cycle(data->ctx, (uint8_t)channel,
					    pdg_pwm_compare_for(pulse_cycles,
								period_cycles,
								max_duty));
	if (ret < 0) {
		LOG_ERR("%s: channel %u failed to set the duty cycle: errno=%d.",
			dev->name, channel, ret);
		pdg_pwm_forget_slice(data, slice);
		goto out;
	}

	data->channels[channel].configured = true;
	data->channels[channel].period_cycles = period_cycles;
	data->channels[channel].pulse_cycles = pulse_cycles;

	if (!data->slices[slice].enabled) {
		ret = pdg_pwm_bottom_enable(data->ctx, (uint8_t)channel);
		if (ret < 0) {
			LOG_ERR("%s: channel %u failed to enable the slice: "
				"errno=%d.", dev->name, channel, ret);
			goto out;
		}

		data->slices[slice].enabled = true;
	}

	ret = 0;
out:
	k_mutex_unlock(&data->lock);

	return ret;
}

static int pdg_pwm_set_cycles(const struct device *dev, uint32_t channel,
			      uint32_t period_cycles, uint32_t pulse_cycles,
			      pwm_flags_t flags)
{
	struct pdg_pwm_data *data = dev->data;

	if (data->ctx == NULL) {
		return -ENODEV;
	}

	if (channel >= PDG_PWM_NUM_CHANNELS) {
		LOG_ERR("%s: channel %u is out of range (0 to %u).", dev->name,
			channel, PDG_PWM_NUM_CHANNELS - 1U);
		return -EINVAL;
	}

	/*
	 * Not emulated by inverting the duty cycle. That would change the idle
	 * level rather than the polarity, which is a different signal and a
	 * worse failure than a clean refusal.
	 */
	if ((flags & PWM_POLARITY_INVERTED) != 0U) {
		LOG_ERR("%s: channel %u requested inverted polarity, which the "
			"firmware does not support. Returning -ENOTSUP.",
			dev->name, channel);
		return -ENOTSUP;
	}

	if (period_cycles < PDG_PWM_MIN_PERIOD_CYCLES) {
		LOG_ERR("%s: channel %u requested a period of %u cycles; the "
			"minimum is %u. Returning -EINVAL.", dev->name, channel,
			period_cycles, PDG_PWM_MIN_PERIOD_CYCLES);
		return -EINVAL;
	}

	/*
	 * Panic guard, not a capability statement. A longer period derives a
	 * frequency the firmware would try to reach with a clock divider above
	 * 255, which embassy-rp refuses with a panic that takes the whole
	 * device down. See issue #192; this contains the defect for Zephyr
	 * consumers without fixing it, and every other host surface remains
	 * able to reach it.
	 *
	 * The "about N Hz" parenthetical is derived through
	 * pdg_pwm_frequency_for() rather than by dividing directly, because a
	 * direct integer division floors to 8 Hz -- which is precisely the
	 * frequency this bound exists to keep the firmware away from. Printing
	 * it would send anyone debugging #192 after the wrong number.
	 */
	if ((uint64_t)period_cycles > PDG_PWM_MAX_PERIOD_CYCLES) {
		LOG_ERR("%s: channel %u requested a period of %u cycles; the "
			"maximum is %llu (about %u Hz), because a longer period "
			"needs a clock divider the firmware cannot program. "
			"Returning -ENOTSUP.", dev->name, channel, period_cycles,
			(unsigned long long)PDG_PWM_MAX_PERIOD_CYCLES,
			(unsigned int)pdg_pwm_frequency_for(
				(uint32_t)PDG_PWM_MAX_PERIOD_CYCLES));
		return -ENOTSUP;
	}

	/*
	 * Defence in depth, and deliberately kept despite looking unreachable.
	 * Zephyr's z_impl_pwm_set_cycles() screens pulse > period before it
	 * dispatches here (include/zephyr/drivers/pwm.h), so a caller on the
	 * public path never gets this far. But the API slot is reachable
	 * directly through DEVICE_API_GET(pwm, dev)->set_cycles(), which
	 * bypasses that wrapper entirely, and the fake suite invokes it that
	 * way precisely to keep this check honest. Do not delete it as dead.
	 */
	if (pulse_cycles > period_cycles) {
		LOG_ERR("%s: channel %u requested a pulse of %u cycles inside a "
			"period of %u. Returning -EINVAL.", dev->name, channel,
			pulse_cycles, period_cycles);
		return -EINVAL;
	}

	/*
	 * Slice-sharing conflict.
	 *
	 * Channels 0+1 and 2+3 each share an RP2350 slice, and a slice has one
	 * period. Reconfiguring would silently change the sibling's period
	 * behind its owner's back, and additionally drift its duty through the
	 * firmware's truncating rescale. Refusing surfaces the constraint at
	 * the call that violates it.
	 *
	 * Read under the lock, because a concurrent call on the sibling could
	 * otherwise be observed half-applied. The lock is released before
	 * pdg_pwm_apply() takes it again rather than held across the call:
	 * k_mutex is recursive for the same thread, but relying on that would
	 * make the ownership of every field inside apply() ambiguous.
	 */
	{
		uint32_t sibling = pdg_pwm_sibling_of(channel);
		uint32_t sibling_period;
		bool conflict;

		k_mutex_lock(&data->lock, K_FOREVER);
		conflict = data->channels[sibling].configured &&
			   data->channels[sibling].period_cycles != period_cycles;
		sibling_period = data->channels[sibling].period_cycles;
		k_mutex_unlock(&data->lock);

		if (conflict) {
			LOG_ERR("%s: channel %u requested a period of %u cycles, but "
				"its slice sibling channel %u is using %u. Channels "
				"%u and %u share one PWM slice and cannot hold "
				"independent periods. Use channels 0 and 2 for two "
				"independent periods. Returning -EINVAL.",
				dev->name, channel, period_cycles, sibling,
				sibling_period, channel, sibling);
			return -EINVAL;
		}
	}

	return pdg_pwm_apply(dev, channel, period_cycles, pulse_cycles);
}

static DEVICE_API(pwm, pdg_pwm_api) = {
	.set_cycles = pdg_pwm_set_cycles,
	.get_cycles_per_sec = pdg_pwm_get_cycles_per_sec,
};

static int pdg_pwm_init(const struct device *dev)
{
	const struct pdg_pwm_config *config = dev->config;
	struct pdg_pwm_data *data = dev->data;
	bool has_pwm = false;
	int ret;

	/*
	 * The mutex and every piece of mutable state are initialized before any
	 * early return, so a device object that exists at all has a usable
	 * lock. Zephyr's PWM wrappers dispatch straight into this API without a
	 * readiness check, so a direct call on a failed device must find an
	 * initialized mutex; the data->ctx == NULL guard at the top of each
	 * callback then turns that call into a safe refusal.
	 */
	k_mutex_init(&data->lock);

	data->ctx = NULL;
	memset(data->channels, 0, sizeof(data->channels));
	memset(data->slices, 0, sizeof(data->slices));

	/*
	 * Mandatory MFD child sequence (pdg_mfd.h): require parent readiness
	 * first, then borrow the context. A NULL context after a passing
	 * readiness check is an ownership invariant failure, not an expected
	 * case, so it is logged distinctly. The context is borrowed: this
	 * driver must never close or free it.
	 */
	if (!device_is_ready(config->mfd)) {
		LOG_ERR("%s: Pico de Gallo parent %s is not ready. Returning -ENODEV.",
			dev->name, config->mfd->name);
		return -ENODEV;
	}

	data->ctx = pdg_mfd_ctx(config->mfd);
	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo parent %s is ready but returned a NULL context; "
			"this is an MFD ownership invariant failure. Returning -ENODEV.",
			dev->name, config->mfd->name);
		return -ENODEV;
	}

	/*
	 * Capability gate.
	 *
	 * Every shipped firmware advertises PWM on both hardware revisions, so
	 * this will not fire today. It is here because the capability bit is
	 * the contract, and a future image that drops PWM should be refused
	 * loudly rather than accepted into silence.
	 *
	 * This is NOT a warm local read. gallo_get_device_info() re-validates
	 * unconditionally, issuing a fresh device/info RPC, so it costs a
	 * metadata round trip on top of the one the parent already paid.
	 *
	 * The two failure shapes stay distinct: a failed query reports its own
	 * mapped errno, because "the board did not answer" is a different
	 * problem from "the board answered and has no PWM".
	 */
	ret = pdg_pwm_bottom_has_pwm(data->ctx, &has_pwm);
	if (ret < 0) {
		LOG_ERR("%s: failed to read the firmware capability bits: errno=%d.",
			dev->name, ret);
		/*
		 * Defensive invalidation of this child's cached borrow -- never
		 * a reference release. The parent holds the sole registry
		 * reference; releasing it here would leave the parent and the
		 * siblings holding a freed pointer. NULL is guardable; a
		 * valid-looking unowned pointer would bypass every NULL check.
		 */
		data->ctx = NULL;
		return ret;
	}

	if (!has_pwm) {
		LOG_ERR("%s: the attached Pico de Gallo does not advertise the PWM "
			"capability. Returning -ENODEV.", dev->name);
		data->ctx = NULL;
		return -ENODEV;
	}

	LOG_INF("%s: ready on Pico de Gallo serial-number \"%s\" with %u channels "
		"at %u Hz.", dev->name, config->serial_number,
		PDG_PWM_NUM_CHANNELS, PDG_PWM_CYCLES_PER_SEC);

	return 0;
}

#define PDG_PWM_INIT(inst)							\
	static struct pdg_pwm_data pdg_pwm_data_##inst;				\
										\
	static const struct pdg_pwm_config pdg_pwm_config_##inst = {		\
		.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst)),			\
		.serial_number = DT_PROP(DT_INST_PARENT(inst), serial_number),	\
	};									\
										\
	DEVICE_DT_INST_DEFINE(inst, pdg_pwm_init, NULL,				\
			      &pdg_pwm_data_##inst,				\
			      &pdg_pwm_config_##inst, POST_KERNEL,		\
			      CONFIG_PWM_PICO_DE_GALLO_INIT_PRIORITY,		\
			      &pdg_pwm_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_PWM_INIT)
