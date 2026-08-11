/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr I2C controller driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context. Note that
 * "embedded" here just means the embedded part of `native-sim`,
 * not something that actually gets flashed to hardware or anything.
 * Anyway, this file translates Zephyr I2C API transactions into the small 
 * host-context shim declared in pdg_i2c_bottom.h, which forwards them to the Pico de Gallo C FFI.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_i2c

#include <inttypes.h>

#include <zephyr/device.h>
#include <zephyr/drivers/i2c.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

#include "pdg_i2c_bottom.h"

LOG_MODULE_REGISTER(i2c_pico_de_gallo, CONFIG_I2C_LOG_LEVEL);

// Firmware single-transfer limit (pico_de_gallo_internal::MAX_TRANSFER_SIZE).
#define PDG_I2C_MAX_BUFFER 4096U

struct pdg_i2c_config {
	const char *serial;
	uint32_t clock_frequency;
};

struct pdg_i2c_data {
	void *ctx;
	struct k_mutex lock;
	uint32_t dev_config;
};

// helper to map a Zephyr I2C speed (see the zephyr I2C_SPEED_... macros) into a pico de gallo speed code (see gallo_i2c_set_config() in pico_de_gallo.h)
// 
// `speed` is the Zephyr I2C speed (meaning you will probably pass a I2C_SPEED_... macro into the parameter). The returned value is one of the possible pico-de-gallo
// speed codes accepted by the FFI `gallo_i2c_set_config()` function via the `frequency` parameter:
// 0 = Standard (100 kHz), 1 = Fast (400 kHz), 2 = Fast+ (1 MHz).
static int speed_to_code_(uint32_t speed, uint8_t* code)
{
	// the three pico de gallo I2C speed settings
	static const uint8_t Gallo_Standard = 0U; // (100 kHz)
	static const uint8_t Gallo_Fast = 1U; 	  // (400 kHz)
	static const uint8_t Gallo_FastPlus = 2U; // (1 MHz)

	// also here's each of the possible Zephyr I2C speed macros according to their API docs:
	// I2C_SPEED_STANDARD  (100 kHz)
	// I2C_SPEED_FAST 	   (400 kHz)
	// I2C_SPEED_FAST_PLUS (1 MHz)
	// I2C_SPEED_HIGH 	   (3.4 MHz)
	// I2C_SPEED_ULTRA     (5 MHz) 

	switch (speed) {
		case I2C_SPEED_STANDARD:  { *code = Gallo_Standard; return 0; }
		case I2C_SPEED_FAST:      { *code = Gallo_Fast; 	return 0; }
		case I2C_SPEED_FAST_PLUS: { *code = Gallo_FastPlus; return 0; }

		// pico-de-gallo has nothing directly corresponding to I2C_SPEED_HIGH
		case I2C_SPEED_HIGH: {
			LOG_ERR("pico-de-gallo does not support the configured I2C speed (I2C_SPEED_HIGH). Returning -EINVAL. Please use one of the supported variants: I2C_SPEED_STANDARD, I2C_SPEED_FAST, or I2C_SPEED_FAST_PLUS.");
			return -EINVAL; 
		}

		// pico-de-gallo has nothing directly corresponding to I2C_SPEED_ULTRA
		case I2C_SPEED_ULTRA: { 
			LOG_ERR("pico-de-gallo does not support the configured I2C speed (I2C_SPEED_ULTRA). Returning -EINVAL. Please use one of the supported variants: I2C_SPEED_STANDARD, I2C_SPEED_FAST, or I2C_SPEED_FAST_PLUS.");
			return -EINVAL; 
		}


		// unknown
		default: {
			LOG_ERR("pico-de-gallo does not support the configured I2C speed (speed=%" PRIu32 "). Returning -EINVAL. Please use one of the supported variants: I2C_SPEED_STANDARD, I2C_SPEED_FAST, or I2C_SPEED_FAST_PLUS.", speed);
			return -EINVAL; 
		}
	}
}

// helper to map a `clock_frequency` in Hz to a Zephyr I2C speed macro
static int freq_to_speed_(uint32_t clock_frequency, uint32_t* speed)
{
	switch(clock_frequency) {
		case 100000U: 	{ *speed = I2C_SPEED_STANDARD; 	return 0; }
		case 400000U: 	{ *speed = I2C_SPEED_FAST; 	   	return 0; }
		case 1000000U: 	{ *speed = I2C_SPEED_FAST_PLUS; return 0; }
		case 3400000U: 	{ *speed = I2C_SPEED_HIGH; 	 	return 0; }
		case 5000000U: 	{ *speed = I2C_SPEED_ULTRA; 	return 0; }

		default: {
			LOG_ERR("Invalid I2C frequency provided (frequency=% " PRIu32 "). Returning -EINVAL. Try using one of the following: I2C_SPEED_STANDARD (100_000 Hz), I2C_SPEED_FAST (400_000 Hz), I2C_SPEED_FAST_PLUS (1_000_000 Hz), I2C_SPEED_HIGH (3_400_000 Hz), or I2C_SPEED_ULTRA (5_000_000 Hz).");
			return -EINVAL;
		}
	}
}

// helper for pdg_i2c_transfer() to validate a group of messages and make sure it is in a format that is supported by the current pico-de-gallo ffi API
static int validate_group_(const struct i2c_msg *msgs, uint8_t first, uint8_t count)
{
	// The current FFI supports one read, one write, or one write
	// followed by a repeated-start read within a STOP-delimited group.

	if (count == 1U) {
		return 0;
	}

	if ((count == 2U) &&
	    ((msgs[first].flags & I2C_MSG_READ) == 0U) &&
	    ((msgs[first + 1U].flags & I2C_MSG_READ) != 0U) &&
	    ((msgs[first + 1U].flags & I2C_MSG_RESTART) != 0U)) {
		return 0;
	}

	LOG_ERR("Unsupported I2C message group starting at message %u with %u messages. Returning -ENOTSUP.", first, count);
	return -ENOTSUP;
}

static int pdg_i2c_configure(const struct device *dev, uint32_t dev_config)
{
	struct pdg_i2c_data *data = dev->data;
	int ret;

	if ((dev_config & I2C_ADDR_10_BITS) != 0U) {
		LOG_ERR("10-bit I2C addressing (I2C_ADDR_10_BITS) is not supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if ((dev_config & I2C_MODE_CONTROLLER) == 0U) {
		LOG_ERR("The configured I2C peripheral mode is not supported. I2C_MODE_CONTROLLER is required. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	uint8_t code = 0;
	ret = speed_to_code_(I2C_SPEED_GET(dev_config), &code);
	if (ret < 0) { return ret; }

	k_mutex_lock(&data->lock, K_FOREVER);
	ret = pdg_i2c_bottom_set_config(data->ctx, code);
	if (ret == 0) {
		data->dev_config = dev_config;
	} else {
		LOG_ERR("Failed to set I2C config: errno=%d", ret);
	}
	k_mutex_unlock(&data->lock);

	return ret;
}

static int pdg_i2c_get_config(const struct device *dev, uint32_t *dev_config)
{
	struct pdg_i2c_data *data = dev->data;

	k_mutex_lock(&data->lock, K_FOREVER);
	*dev_config = data->dev_config;
	k_mutex_unlock(&data->lock);

	return 0;
}

static int pdg_i2c_transfer(const struct device *dev, struct i2c_msg *msgs, uint8_t num_msgs, uint16_t addr)
{
	struct pdg_i2c_data *data = dev->data;
	uint8_t group_start = 0U;
	int ret;

	if (addr > 0x7fU) {
		LOG_ERR("I2C address 0x%04x exceeds the 7-bit address range. Returning -EINVAL.", addr);
		return -EINVAL;
	}

	// validate the provided messages
	for (uint8_t i = 0U; i < num_msgs; i++) {

		// make sure msgs isn't null
		// (the Zephyr API should already enforce this via the i2c_transfer() docs but still going to check here)
		if ((msgs[i].buf == NULL)) {
			LOG_ERR("NULL buffer provided for I2C message %u. Returning -EINVAL.", i, msgs[i].len);
			return -EINVAL;
		}

		// make sure I2C_MSG_ADDR_10_BITS isn't requested since it isn't supported
		if ((msgs[i].flags & I2C_MSG_ADDR_10_BITS) != 0U) {
			LOG_ERR("I2C message %u is requesting 10-bit addressing (I2C_MSG_ADDR_10_BITS), but this addressing is unsupported. Returning -ENOTSUP.", i);
			return -ENOTSUP;
		}

		// make sure the message size doesn't exceed pico-de-gallo's max buffer size
		if (msgs[i].len > PDG_I2C_MAX_BUFFER) {
			LOG_ERR("I2C message %u is %u bytes, which exceeds the %u-byte transfer limit. Returning -EMSGSIZE.", i, msgs[i].len, PDG_I2C_MAX_BUFFER);
			return -EMSGSIZE;
		}

		if ((msgs[i].flags & I2C_MSG_STOP) != 0U) {
            ret = validate_group_(msgs, group_start, i - group_start + 1U);
            if (ret < 0) {
                return ret;
            }

            group_start = i + 1U;
        }
	}

	// pico-de-gallo-ffi's I2C currently always generates STOP. The Zephyr should API conform to this by default but
	// it is still possible to manually attempt low-level I2C transactions that omit STOP, so we gotta check for that:
	if (group_start != num_msgs) {
        LOG_ERR("A final I2C transaction without STOP is unsupported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

	// okay at this point we know all the groups are known to be supported so we can start actually transferring
	k_mutex_lock(&data->lock, K_FOREVER);
	group_start = 0U;
	ret = 0;

	// loop through every message and send one FFI call per "message group"
	// a "message group" contains all messages since the previous STOP
	// a "message group" can either be one or two messages, as enforced by validate_group_().
	// the purpose of sending messages in  a "message group" is because pico-de-gallo-ffi uses STOP for each individual operation
	for (uint8_t i = 0U; i < num_msgs; i++) {
		struct i2c_msg *first;
		uint8_t group_count;

		// if this message isn't a STOP it isn't the end to a message group, so we can skip
		if ((msgs[i].flags & I2C_MSG_STOP) == 0U) {
			continue;
		}

		first = &msgs[group_start];
		group_count = i - group_start + 1U;

		if (group_count == 1U) {
			// group is just a single READ message
			if ((first->flags & I2C_MSG_READ) != 0U) {
				ret = pdg_i2c_bottom_read(data->ctx, addr, first->buf, first->len);
				if (ret < 0) {
					LOG_ERR("I2C read message %u from address 0x%02x failed (%u bytes): errno=%d.", group_start, addr, first->len, ret);
				}
			// group is just a single WRITE message
			} else {
				ret = pdg_i2c_bottom_write(data->ctx, addr, first->buf, first->len);
				if (ret < 0) {
					LOG_ERR("I2C write message %u to address 0x%02x failed (%u bytes): errno=%d.", group_start, addr, first->len, ret);
				}
			}
		// group is a two-message WRITE READ operation
		} else {
			struct i2c_msg *second = &msgs[group_start + 1U];

			ret = pdg_i2c_bottom_write_read(data->ctx, addr, first->buf, first->len, second->buf, second->len);
			if (ret < 0) {
				LOG_ERR("I2C write-read messages %u-%u at address 0x%02x failed (TX=%u bytes, RX=%u bytes): errno=%d.", group_start, group_start + 1U, addr, first->len, second->len, ret);
			}
		}

		if (ret < 0) {
			break;
		}

		group_start = i + 1U;
	}

	k_mutex_unlock(&data->lock);

	return ret;
}

static DEVICE_API(i2c, pdg_i2c_api) = {
	.configure = pdg_i2c_configure,
	.get_config = pdg_i2c_get_config,
	.transfer = pdg_i2c_transfer,
};

static int pdg_i2c_init(const struct device *dev)
{
	int ret = 0;
	const struct pdg_i2c_config *config = dev->config;
	struct pdg_i2c_data *data = dev->data;

	uint32_t speed = 0;
	ret = freq_to_speed_(config->clock_frequency, &speed);
	if(ret < 0) {
		return ret;
	}

	k_mutex_init(&data->lock);

	data->ctx = pdg_i2c_bottom_open(config->serial);
	if (data->ctx == NULL) {
		if (config->serial != NULL) {
    		LOG_ERR("Failed to open Pico de Gallo bridge with serial number %s. Returning -ENODEV.", config->serial);
		} else {
    		LOG_ERR("Failed to open a Pico de Gallo bridge. Returning -ENODEV.");
		}
		return -ENODEV;
	}

	uint32_t dev_config_ = I2C_MODE_CONTROLLER | I2C_SPEED_SET(speed);

	uint8_t code = 0;
	ret = speed_to_code_(speed, &code);
	if(ret < 0) {
		pdg_i2c_bottom_close(data->ctx); 
		data->ctx = NULL;
		return ret; 
	}

	ret = pdg_i2c_bottom_set_config(data->ctx, code);
	if (ret < 0) {
		LOG_ERR("Failed to set I2C config: errno=%d", ret);
		pdg_i2c_bottom_close(data->ctx);
		data->ctx = NULL;
		return ret;
	}

	data->dev_config = dev_config_;
	return ret;
}

#define PDG_I2C_INIT(inst)							\
	static struct pdg_i2c_data pdg_i2c_data_##inst;				\
										\
	static const struct pdg_i2c_config pdg_i2c_config_##inst = {		\
		.serial = DT_INST_PROP_OR(inst, serial_number, NULL),		\
		.clock_frequency = DT_INST_PROP_OR(inst, clock_frequency,	\
						   I2C_BITRATE_STANDARD),	\
	};									\
										\
	I2C_DEVICE_DT_INST_DEFINE(inst, pdg_i2c_init, NULL,			\
				  &pdg_i2c_data_##inst,				\
				  &pdg_i2c_config_##inst, POST_KERNEL,		\
				  CONFIG_I2C_INIT_PRIORITY, &pdg_i2c_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_I2C_INIT)
