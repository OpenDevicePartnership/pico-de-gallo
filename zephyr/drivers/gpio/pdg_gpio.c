/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr GPIO controller driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context, which for this module means
 * the embedded half of native_sim. It translates Zephyr GPIO API calls into
 * the small host-context shim declared in pdg_gpio_bottom.h, which forwards
 * them to the Pico de Gallo C FFI.
 *
 * Every operation that reaches hardware is a blocking USB round trip; calls
 * rejected beforehand (interrupt context, invalid arguments, zero pin masks
 * and toggle) perform no I/O. Levels are queried afresh from
 * the board, while direction and pull are never cached locally; configuration
 * is sent directly to the firmware. This avoids the host/firmware divergence
 * class of defect recorded as issue #104.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_gpio

#include <inttypes.h>

#include <zephyr/device.h>
#include <zephyr/drivers/gpio.h>
#include <zephyr/drivers/gpio/gpio_utils.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

#include "pdg_gpio_bottom.h"

/*
 * Structural topology enforcement.
 *
 * This controller borrows its host connection from an odp,pico-de-gallo MFD
 * parent reached through DT_INST_PARENT(). Runtime readiness alone cannot
 * prove that the parent is the *right kind* of device: a child placed under an
 * unrelated but enabled and ready device would pass device_is_ready(), and
 * pdg_mfd_ctx() would then reinterpret that foreign driver's dev->data as
 * struct pdg_mfd_data and hand back an arbitrary pointer that no NULL check
 * can catch. DT_INST_PARENT() on a stale root-level child yields `/`, so
 * asserting status alone is likewise insufficient; the compatible must be
 * checked in its own right.
 *
 * The assertions are ordered compatible -> parent status -> parent serial
 * presence -> Kconfig, and that *source* order is the normative contract.
 * First prove this is the right enabled hardware node, then prove that GPIO
 * actuation has an explicit board identity, and only then diagnose the
 * software dependency. (_Static_assert is not fatal, so GCC reports every
 * failing assertion in one pass, but C does not specify the emission order;
 * observed diagnostic order is corroborating evidence only.)
 *
 * The serial-presence assertion exists because GPIO actuation drives physical
 * pins. A selector-less strict open cannot report which attached board it
 * selected, so an enabled GPIO child under a selector-less parent would
 * actuate unidentifiable hardware. Presence is not uniqueness: two parents
 * carrying the same explicit serial still alias to one board, as the MFD
 * binding already documents.
 *
 * The fifth assertion bounds ngpios. Binding YAML cannot express an integer
 * range, and gpio_port_pins_t is 32 bits wide on this target, so an
 * out-of-range ngpios would otherwise silently truncate the port_pin_mask that
 * GPIO_COMMON_CONFIG_FROM_DT_INST() derives from it.
 *
 * The whole block precedes the "pdg_mfd.h" include on purpose: when
 * CONFIG_MFD_PICO_DE_GALLO is `n` the MFD driver subdirectory is not added to
 * the build at all, so pdg_mfd.h is not on the include path. Asserting first
 * guarantees the readable configuration error is emitted before the include
 * failure, instead of an opaque "no such file" or an unresolved
 * __device_dts_ord_N at link time.
 */
#define PDG_GPIO_PARENT_ASSERTS(inst)						\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_INST_PARENT(inst), odp_pico_de_gallo),	\
		"Enabled odp,pico-de-gallo-gpio controllers must be direct "	\
		"children of an odp,pico-de-gallo parent");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(DT_INST_PARENT(inst)),			\
		"Enabled odp,pico-de-gallo-gpio controllers require their "	\
		"odp,pico-de-gallo parent to have status okay");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_PROP(DT_INST_PARENT(inst), serial_number),		\
		"odp,pico-de-gallo-gpio parent must define serial-number");	\
	BUILD_ASSERT(								\
		IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO),				\
		"Enabled Pico de Gallo child controllers require "		\
		"CONFIG_MFD_PICO_DE_GALLO=y");					\
	BUILD_ASSERT(								\
		(DT_INST_PROP(inst, ngpios) >= 1) &&				\
		(DT_INST_PROP(inst, ngpios) <= GPIO_MAX_PINS_PER_PORT),		\
		"odp,pico-de-gallo-gpio ngpios must be between 1 and 32");

DT_INST_FOREACH_STATUS_OKAY(PDG_GPIO_PARENT_ASSERTS)

#include "pdg_mfd.h"

LOG_MODULE_REGISTER(gpio_pico_de_gallo, CONFIG_GPIO_LOG_LEVEL);

/*
 * Positive allow-list of configuration flags this controller understands.
 *
 * Anything outside it is rejected rather than silently ignored, so a newly
 * defined Zephyr flag bit cannot quietly change the meaning of a call until a
 * reviewer deliberately adds and maps it here.
 */
#define PDG_GPIO_ALLOWED_FLAGS							\
	(GPIO_INPUT | GPIO_OUTPUT | GPIO_PULL_UP | GPIO_PULL_DOWN |		\
	 GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH | GPIO_ACTIVE_LOW)

/* Direction and pull byte values accepted by gallo_gpio_set_config(). */
#define PDG_GPIO_DIR_INPUT 0U
#define PDG_GPIO_DIR_OUTPUT 1U
#define PDG_GPIO_PULL_NONE 0U
#define PDG_GPIO_PULL_UP 1U
#define PDG_GPIO_PULL_DOWN 2U

struct pdg_gpio_config {
	struct gpio_driver_config common; /* MUST be first */
	const struct device *mfd;
	const char *serial_number;
	uint8_t ngpios;
};

struct pdg_gpio_data {
	struct gpio_driver_data common; /* MUST be first */
	void *ctx;
	struct k_mutex lock;
};

/*
 * The single locked multi-pin write shared by the masked-write, set-bits and
 * clear-bits callbacks (spec §6.3).
 *
 * There is deliberately exactly one copy of this loop. Ascending pin order,
 * stop-on-first-failure, acknowledged-prefix tracking and the partial-failure
 * diagnostic are a single contract, and three public entry points cannot be
 * kept in lockstep on all four by review alone.
 *
 * Ascending, per-pin, non-atomic writes with no read-modify-write. Not writing
 * an unmasked pin is what preserves it; a pre-read would be both unnecessary
 * and invalid, because the firmware refuses to read a pin recorded as an
 * explicit output.
 *
 * The caller owns the ISR, context and mask checks, owns the zero-mask
 * short-circuit, and must already hold the child mutex. `op` names the public
 * operation for the diagnostic; callers pass mask/value as (mask, value),
 * (pins, pins) and (pins, 0) respectively.
 */
static int pdg_gpio_write_locked(const struct device *port, const char *op,
				 gpio_port_pins_t mask, gpio_port_value_t value)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;
	gpio_port_pins_t acked = 0U;
	uint8_t fail_pin = 0U;
	int ret = 0;

	for (uint8_t pin = 0U; pin < config->ngpios; pin++) {
		if ((mask & BIT(pin)) == 0U) {
			continue;
		}

		fail_pin = pin;
		ret = pdg_gpio_bottom_put(data->ctx, pin, (value & BIT(pin)) != 0U);
		if (ret < 0) {
			break;
		}

		acked |= BIT(pin);
	}

	/*
	 * Deterministic partial-failure residue: the acknowledged prefix
	 * definitely changed, the failed pin is indeterminate because its
	 * request may have executed with only the response lost, and later
	 * selected pins were never issued. There is no rollback.
	 */
	if (ret < 0) {
		LOG_ERR("%s: %s failed at pin %u (mask 0x%08x, value 0x%08x), "
			"acknowledged prefix mask 0x%08x, errno=%d.",
			port->name, op, fail_pin, (uint32_t)mask, (uint32_t)value,
			(uint32_t)acked, ret);
	}

	return ret;
}

static int pdg_gpio_pin_configure(const struct device *port,
				  gpio_pin_t pin, gpio_flags_t flags)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;
	uint8_t direction;
	uint8_t pull;
	int ret;

	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo GPIO context is NULL; check device readiness. "
			"Returning -ENODEV.", port->name);
		return -ENODEV;
	}

	if (pin >= config->ngpios) { LOG_ERR("%s: pin %u out of range. Returning -EINVAL.", port->name, pin); return -EINVAL; }

	if ((flags & (GPIO_INPUT | GPIO_OUTPUT)) == 0U) { LOG_ERR("%s: GPIO_DISCONNECTED is unsupported. Returning -ENOTSUP.", port->name); return -ENOTSUP; }

	/*
	 * PDG_GPIO_COUPLING_6_2_7 -- coupled with pdg_gpio_port_get_raw().
	 *
	 * That function reports a zero bit for a pin the firmware rejects as an
	 * explicit output (-EACCES) and keeps scanning, exactly as Zephyr's
	 * reference controller does, because port_get_raw is scoped to input
	 * pins. That rule is only honest while a pin cannot be simultaneously
	 * input and output: the rejection immediately below is what guarantees
	 * a reported zero is never a pin whose level a caller was promised.
	 * Relaxing this rejection would make port_get_raw return confident
	 * false levels -- and under GPIO_ACTIVE_LOW a false logical 1, since
	 * z_impl_gpio_port_get() XORs data->invert over our zero. Do not change
	 * either site without the other.
	 */
	if ((flags & (GPIO_INPUT | GPIO_OUTPUT)) == (GPIO_INPUT | GPIO_OUTPUT)) { LOG_ERR("%s: GPIO_INPUT | GPIO_OUTPUT is unsupported. Returning -ENOTSUP.", port->name); return -ENOTSUP; }

	if ((flags & GPIO_SINGLE_ENDED) != 0U) { LOG_ERR("%s: single-ended drive is unsupported. Returning -ENOTSUP.", port->name); return -ENOTSUP; }

	if ((flags & GPIO_LINE_OPEN_DRAIN) != 0U) { LOG_ERR("%s: open-drain is unsupported. Returning -ENOTSUP.", port->name); return -ENOTSUP; }

	if ((flags & (GPIO_PULL_UP | GPIO_PULL_DOWN)) == (GPIO_PULL_UP | GPIO_PULL_DOWN)) { LOG_ERR("%s: both pulls requested. Returning -EINVAL.", port->name); return -EINVAL; }

	if ((flags & (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) == (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) { LOG_ERR("%s: both init levels requested. Returning -EINVAL.", port->name); return -EINVAL; }

	if (((flags & (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) != 0U) &&
	    ((flags & GPIO_OUTPUT) == 0U)) { LOG_ERR("%s: init level without GPIO_OUTPUT. Returning -EINVAL.", port->name); return -EINVAL; }

	/*
	 * z_impl_gpio_pin_configure() asserts these away in CONFIG_ASSERT=y
	 * builds, but they reach a driver dispatched directly or built without
	 * assertions, so they are rejected here in their own right.
	 */
	if ((flags & GPIO_INT_MASK) != 0U) { LOG_ERR("%s: interrupts are unsupported. Returning -ENOTSUP.", port->name); return -ENOTSUP; }

	/* GPIO_INT_WAKEUP is bit 6 and is not a member of GPIO_INT_MASK, so it
	 * reaches the driver in every build, assertions or not.
	 */
	if ((flags & GPIO_INT_WAKEUP) != 0U) { LOG_ERR("%s: interrupt wakeup is unsupported. Returning -ENOTSUP.", port->name); return -ENOTSUP; }

	if ((flags & ~PDG_GPIO_ALLOWED_FLAGS) != 0U) { LOG_ERR("%s: unknown flags 0x%08x. Returning -ENOTSUP.", port->name, (uint32_t)flags); return -ENOTSUP; }

	direction = ((flags & GPIO_OUTPUT) != 0U) ? PDG_GPIO_DIR_OUTPUT : PDG_GPIO_DIR_INPUT;

	if ((flags & GPIO_PULL_UP) != 0U) {
		pull = PDG_GPIO_PULL_UP;
	} else if ((flags & GPIO_PULL_DOWN) != 0U) {
		pull = PDG_GPIO_PULL_DOWN;
	} else {
		pull = PDG_GPIO_PULL_NONE;
	}

	k_mutex_lock(&data->lock, K_FOREVER);

	/*
	 * Configuration must precede the level write: the firmware rejects a
	 * put on a pin recorded as an explicit input, while set-config is what
	 * establishes the explicit output in the first place. The consequence
	 * is that an output is enabled before the requested level arrives, so
	 * the previous or HAL-defined level can briefly appear on the pad.
	 *
	 * Neither failure is rolled back. A failed set-config leaves the
	 * requested direction and pull indeterminate -- the firmware may have
	 * applied them and only the acknowledgement may have been lost -- and a
	 * failed put leaves an explicit output at its previous level. Prior
	 * state is not cached and a rollback RPC can fail in turn.
	 */
	ret = pdg_gpio_bottom_set_config(data->ctx, (uint8_t)pin, direction, pull);
	if (ret < 0) {
		LOG_ERR("%s: set-config failed for pin %u: errno=%d.", port->name, pin, ret);
		k_mutex_unlock(&data->lock);
		return ret;
	}

	if ((flags & (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) != 0U) {
		ret = pdg_gpio_bottom_put(data->ctx, (uint8_t)pin,
					  (flags & GPIO_OUTPUT_INIT_HIGH) != 0U);
		if (ret < 0) {
			LOG_ERR("%s: output init put failed for pin %u: errno=%d.",
				port->name, pin, ret);
		}
	}

	k_mutex_unlock(&data->lock);

	return ret;
}

static int pdg_gpio_port_get_raw(const struct device *port, gpio_port_value_t *value)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;
	gpio_port_value_t tmp = 0U;
	int ret = 0;

	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo GPIO context is NULL; check device readiness. "
			"Returning -ENODEV.", port->name);
		return -ENODEV;
	}

	if (value == NULL) {
		LOG_ERR("%s: NULL value pointer. Returning -EINVAL.", port->name);
		return -EINVAL;
	}

	k_mutex_lock(&data->lock, K_FOREVER);

	for (uint8_t pin = 0U; pin < config->ngpios; pin++) {
		bool level = false;

		ret = pdg_gpio_bottom_get(data->ctx, pin, &level);

		/*
		 * PDG_GPIO_COUPLING_6_2_7 -- coupled with pdg_gpio_pin_configure().
		 *
		 * -EACCES is the firmware's GpioWrongDirection for a pin it has
		 * recorded as an explicit output. Zephyr scopes this operation
		 * to input pins ("Get physical level of all input pins in a
		 * port"), and the reference controller gpio_emul masks
		 * non-input pins to zero and succeeds, so this pin contributes
		 * a zero bit and the scan continues. That is only safe because
		 * pin_configure() rejects GPIO_INPUT | GPIO_OUTPUT with
		 * -ENOTSUP, which makes a reported zero provably not an input
		 * pin. Do not change either site without the other. There is
		 * deliberately no per-pin warning here: this is the hot path of
		 * every gpio_pin_get().
		 */
		if (ret == -EACCES) {
			ret = 0;
			continue;
		}

		if (ret == -EBUSY) {
			LOG_ERR("%s: pin %u is under a firmware event subscription; its input "
				"level is unavailable. Normalizing -EBUSY to -EIO.",
				port->name, pin);
			ret = -EIO;
			break;
		}

		if (ret == -EINVAL) {
			LOG_ERR("%s: firmware rejected devicetree-valid pin %u as invalid "
				"(-EINVAL) after count validation; this is a controller/firmware "
				"inconsistency. Normalizing to -EIO.", port->name, pin);
			ret = -EIO;
			break;
		}

		if (ret == -EIO) {
			/*
			 * GpioGetFailed (GpioError::Other), or a transport
			 * failure already normalized from common -ECOMM by the
			 * bottom half. Both are enumerated §6.2 aborts and
			 * -EIO is already the contract errno, so abort as is.
			 */
			break;
		}

		if (ret != 0) {
			/*
			 * Fail closed. §6.2 is an exhaustive disposition table
			 * and deliberately has no residual catch-all: every
			 * status gallo_gpio_get() can currently produce is
			 * named above. Reaching here therefore means the FFI
			 * or the common status mapping grew a GPIO status this
			 * controller has never been specified against.
			 *
			 * Propagating it verbatim would leak an undocumented
			 * errno through an API whose contract enumerates only
			 * 0, -EIO and -EWOULDBLOCK (gpio.h:1275-1277), and
			 * would silently absorb the change instead of forcing
			 * the specification review §6.2 requires. So it is
			 * logged loudly and normalized to -EIO.
			 */
			LOG_ERR("%s: unspecified firmware status errno=%d while reading pin "
				"%u; this status is absent from the exhaustive specification "
				"table and requires a specification review. Normalizing to "
				"-EIO.", port->name, ret, pin);
			ret = -EIO;
			break;
		}

		if (level) {
			tmp |= BIT(pin);
		}
	}

	k_mutex_unlock(&data->lock);

	/* Commit only on a complete scan; every abort leaves *value untouched. */
	if (ret == 0) {
		*value = tmp;
	}

	return ret;
}

static int pdg_gpio_port_set_masked_raw(const struct device *port,
					gpio_port_pins_t mask, gpio_port_value_t value)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;
	int ret;

	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo GPIO context is NULL; check device readiness. "
			"Returning -ENODEV.", port->name);
		return -ENODEV;
	}

	if ((mask & ~config->common.port_pin_mask) != 0U) {
		LOG_ERR("%s: mask outside port_pin_mask. Returning -EINVAL.", port->name);
		return -EINVAL;
	}

	if (mask == 0U) {
		return 0;
	}

	k_mutex_lock(&data->lock, K_FOREVER);
	ret = pdg_gpio_write_locked(port, "masked write", mask, value);
	k_mutex_unlock(&data->lock);

	return ret;
}

static int pdg_gpio_port_set_bits_raw(const struct device *port, gpio_port_pins_t pins)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;
	int ret;

	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo GPIO context is NULL; check device readiness. "
			"Returning -ENODEV.", port->name);
		return -ENODEV;
	}

	if ((pins & ~config->common.port_pin_mask) != 0U) {
		LOG_ERR("%s: pins outside port_pin_mask. Returning -EINVAL.", port->name);
		return -EINVAL;
	}

	if (pins == 0U) {
		return 0;
	}

	k_mutex_lock(&data->lock, K_FOREVER);
	ret = pdg_gpio_write_locked(port, "set-bits", pins, pins);
	k_mutex_unlock(&data->lock);

	return ret;
}

static int pdg_gpio_port_clear_bits_raw(const struct device *port, gpio_port_pins_t pins)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;
	int ret;

	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo GPIO context is NULL; check device readiness. "
			"Returning -ENODEV.", port->name);
		return -ENODEV;
	}

	if ((pins & ~config->common.port_pin_mask) != 0U) {
		LOG_ERR("%s: pins outside port_pin_mask. Returning -EINVAL.", port->name);
		return -EINVAL;
	}

	if (pins == 0U) {
		return 0;
	}

	k_mutex_lock(&data->lock, K_FOREVER);
	ret = pdg_gpio_write_locked(port, "clear-bits", pins, 0U);
	k_mutex_unlock(&data->lock);

	return ret;
}

/*
 * Toggle dispatch exists so the slot is never NULL, but the capability does
 * not exist. Normal Zephyr output configuration records an explicit output in
 * the firmware, and reading one back returns GpioWrongDirection; temporarily
 * flipping the pin to input would read the pad rather than the output latch,
 * glitch or tri-state the line, and require exactly the cached configuration
 * this driver refuses to keep. Returning -ENOTSUP is the honest answer.
 *
 * Generic toggle consumers -- blinky, the GPIO shell, the TPS382x watchdog and
 * the LS0xx display -- therefore do not work with this controller.
 */
static int pdg_gpio_port_toggle_bits(const struct device *port, gpio_port_pins_t pins)
{
	const struct pdg_gpio_config *config = port->config;
	struct pdg_gpio_data *data = port->data;

	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo GPIO context is NULL; check device readiness. "
			"Returning -ENODEV.", port->name);
		return -ENODEV;
	}

	if ((pins & ~config->common.port_pin_mask) != 0U) {
		LOG_ERR("%s: pins outside port_pin_mask. Returning -EINVAL.", port->name);
		return -EINVAL;
	}

	if (pins == 0U) { return 0; }

	LOG_ERR("%s: toggle is unavailable on this controller. Returning -ENOTSUP.", port->name);

	return -ENOTSUP;
}

/*
 * Only the six unconditionally dispatched slots are populated. The
 * interrupt-configure, callback-management and pending-interrupt slots stay
 * NULL on purpose: the API layer NULL-checks them and returns -ENOSYS, which
 * is the correct answer for a controller with no interrupt support. The
 * optional pin_get_config and port_get_direction slots are likewise NULL and
 * likewise yield -ENOSYS.
 */
static DEVICE_API(gpio, pdg_gpio_api) = {
	.pin_configure = pdg_gpio_pin_configure,
	.port_get_raw = pdg_gpio_port_get_raw,
	.port_set_masked_raw = pdg_gpio_port_set_masked_raw,
	.port_set_bits_raw = pdg_gpio_port_set_bits_raw,
	.port_clear_bits_raw = pdg_gpio_port_clear_bits_raw,
	.port_toggle_bits = pdg_gpio_port_toggle_bits,
};

static int pdg_gpio_init(const struct device *dev)
{
	const struct pdg_gpio_config *config = dev->config;
	struct pdg_gpio_data *data = dev->data;
	uint8_t num_gpios = 0U;
	int ret;

	k_mutex_init(&data->lock);

	/*
	 * The mutex is initialized above, before any early exit, so that every
	 * device object that exists at all has a usable lock. Zephyr's GPIO API
	 * dispatches straight into the driver without a readiness check, so a
	 * direct call on a failed device must find an initialized mutex; the
	 * NULL-context guard at the top of each callback then turns that call
	 * into -ENODEV.
	 */

	/*
	 * Mandatory MFD child sequence (pdg_mfd.h): require parent readiness
	 * first, then borrow the context. A NULL context *after* a passing
	 * readiness check is an ownership invariant failure, not an expected
	 * case, so it is logged distinctly. The context is borrowed: this
	 * driver must never release it.
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
	 * The parent's strict open already validated the device and filled the
	 * handle-shared device-info cache, so this is a warm local read: no USB
	 * round trip and no device/info timeout exposure on this child path.
	 */
	ret = pdg_gpio_bottom_num_gpios(data->ctx, &num_gpios);
	if (ret < 0) {
		LOG_ERR("%s: failed to read the firmware GPIO count: errno=%d.",
			dev->name, ret);
		/*
		 * Defensive invalidation of this child's cached borrow -- never
		 * a reference release. The parent holds the sole registry
		 * reference; releasing it here would leave the parent and the
		 * I2C/SPI siblings holding a freed pointer.
		 */
		data->ctx = NULL;
		return ret;
	}

	if (num_gpios != config->ngpios) {
		LOG_ERR("%s: devicetree ngpios (%u) does not match the firmware-reported "
			"GPIO count (%u). This is a local devicetree/firmware configuration "
			"error. Returning -EINVAL.", dev->name, config->ngpios, num_gpios);
		data->ctx = NULL;
		return -EINVAL;
	}

	LOG_INF("%s: ready on Pico de Gallo serial-number \"%s\" with %u GPIOs.",
		dev->name, config->serial_number, config->ngpios);

	return 0;
}

#define PDG_GPIO_INIT(inst)							\
	static struct pdg_gpio_data pdg_gpio_data_##inst;			\
										\
	static const struct pdg_gpio_config pdg_gpio_config_##inst = {		\
		.common = GPIO_COMMON_CONFIG_FROM_DT_INST(inst),			\
		.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst)),			\
		.serial_number = DT_PROP(DT_INST_PARENT(inst), serial_number),	\
		.ngpios = DT_INST_PROP(inst, ngpios),				\
	};									\
										\
	DEVICE_DT_INST_DEFINE(inst, pdg_gpio_init, NULL,			\
			      &pdg_gpio_data_##inst,				\
			      &pdg_gpio_config_##inst, POST_KERNEL,		\
			      CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY,		\
			      &pdg_gpio_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_GPIO_INIT)
