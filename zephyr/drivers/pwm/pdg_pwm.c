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

/* What a channel was last asked for. Needed to re-assert a sibling's duty
 * after a slice reconfiguration, and to detect a conflicting period.
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

static int pdg_pwm_set_cycles(const struct device *dev, uint32_t channel,
			      uint32_t period_cycles, uint32_t pulse_cycles,
			      pwm_flags_t flags)
{
	ARG_UNUSED(dev);
	ARG_UNUSED(channel);
	ARG_UNUSED(period_cycles);
	ARG_UNUSED(pulse_cycles);
	ARG_UNUSED(flags);

	return -ENOSYS;
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
