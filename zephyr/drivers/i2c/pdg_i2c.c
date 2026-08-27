/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr I2C controller driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context. Note that "embedded" here just
 * means the embedded part of `native-sim`, not something that actually gets
 * flashed to hardware or anything.  Anyway, this file translates Zephyr I2C API
 * transactions into the small host-context shim declared in pdg_i2c_bottom.h,
 * which forwards them to the Pico de Gallo C FFI.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_i2c

#include <inttypes.h>
#include <string.h>

#include <zephyr/device.h>
#include <zephyr/drivers/i2c.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

#include "pdg_i2c_bottom.h"

/*
 * Structural topology enforcement.
 *
 * This controller borrows its host connection from an odp,pico-de-gallo MFD
 * parent reached through DT_INST_PARENT(). Runtime readiness alone cannot prove
 * that the parent is the *right kind* of device: a child placed under an
 * unrelated but enabled and ready device would pass device_is_ready(), and
 * pdg_mfd_ctx() would then reinterpret that foreign driver's dev->data as
 * struct pdg_mfd_data and hand back an arbitrary pointer that no NULL check can
 * catch. DT_INST_PARENT() on a stale root-level child yields `/`, so asserting
 * status alone is likewise insufficient; the compatible must be checked in its
 * own right.
 *
 * The three assertions are deliberately ordered compatible -> parent status ->
 * Kconfig. Disabling the parent also drops DT_HAS_ODP_PICO_DE_GALLO_ENABLED,
 * which makes CONFIG_MFD_PICO_DE_GALLO `n`, so the third assertion would be
 * true at the same time as the second; emitting the most specific structural
 * diagnostic first keeps the message that names the actual topology error at
 * the top. (_Static_assert is not fatal, so GCC reports every failing assertion
 * in one pass.)
 *
 * They also precede the "pdg_mfd.h" include on purpose: when
 * CONFIG_MFD_PICO_DE_GALLO is `n` the MFD driver subdirectory is not added to
 * the build at all, so pdg_mfd.h is not on the include path. Asserting first
 * guarantees the readable configuration error is emitted before the include
 * failure, instead of an opaque "no such file" or an unresolved
 * __device_dts_ord_N at link time. This follows the same policy as the
 * CONFIG_SPI_ASYNC/CONFIG_SPI_RTIO assertions in pdg_spi.c.
 */
#define PDG_I2C_PARENT_ASSERTS(inst)						\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_INST_PARENT(inst), odp_pico_de_gallo),	\
		"Enabled odp,pico-de-gallo-i2c controllers must be direct "	\
		"children of an odp,pico-de-gallo parent");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(DT_INST_PARENT(inst)),			\
		"Enabled odp,pico-de-gallo-i2c controllers require their "	\
		"odp,pico-de-gallo parent to have status okay");		\
	BUILD_ASSERT(								\
		IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO),				\
		"Enabled Pico de Gallo child controllers require "		\
		"CONFIG_MFD_PICO_DE_GALLO=y");

DT_INST_FOREACH_STATUS_OKAY(PDG_I2C_PARENT_ASSERTS)

#include "pdg_mfd.h"

LOG_MODULE_REGISTER(i2c_pico_de_gallo, CONFIG_I2C_LOG_LEVEL);

/*
 * Per-group transfer ceiling in bytes, applied separately to a group's
 * concatenated write payload and to its terminating read.
 *
 * This is pico_de_gallo_internal::MAX_TRANSFER_SIZE, the firmware's declared
 * single-transfer argument bound. It is NOT a measured end-to-end ceiling for
 * this transport and must not be read as one. The sibling SPI driver carries
 * PDG_SPI_MAX_BUFFER = 1013 precisely because starting from 4096 was wrong
 * twice over: on spi/transfer, 4096 TX-only failed -ECOMM, a reasoned 3072
 * full-duplex guess also failed -ECOMM, and 1015 TX-only wedged the firmware
 * dispatcher device-wide (AGENTS.md 13.17, 2026-08-19). i2c/write carries its
 * whole payload in the request frame exactly as spi/transfer does, so a lower
 * real ceiling plausibly exists here too. Nobody has measured it. Issue #146.
 *
 * 4096 is kept rather than lowered by analogy for two reasons. A bound
 * measured on a differently framed endpoint is a guess, not evidence, and the
 * SPI constant's own comment warns against both raising and lowering it by
 * guesswork. And lowering would reject single-message writes between any new
 * bound and 4096 that this driver accepts today.
 *
 * Concatenating a group's writes does not widen the exposure: the reachable
 * payload range was already [0, 4096] through one i2c_msg, and the running
 * total in validate_group_() keeps it there. What changes is only how easy a
 * large write is to construct.
 */
#define PDG_I2C_MAX_BUFFER 4096U

struct pdg_i2c_config {
	const struct device *mfd;
	uint32_t clock_frequency;
};

struct pdg_i2c_data {
	void *ctx;
	struct k_mutex lock;
	uint32_t dev_config;
};

/*
 * One STOP-delimited group of Zephyr I2C messages, classified into the shape
 * the FFI can express.
 *
 * `write_count` leading write messages are followed by either nothing or, when
 * `has_read` is set, exactly one terminating read. `write_len` is their
 * combined length, which is what the single FFI call actually sends.
 */
struct pdg_i2c_group {
	uint8_t first;
	uint8_t count;
	uint8_t write_count;
	bool has_read;
	size_t write_len;
};

/*
 * What a validated group actually asks the bus to do, judged by the data bytes
 * it carries rather than by its message shape.
 *
 * Zephyr permits msg.buf == NULL whenever msg.len == 0, and i2c_write(dev,
 * NULL, 0, addr) and i2c_write_read(dev, addr, NULL, 0, rx, n) both produce
 * exactly that (issue #137). A message carrying no bytes contributes nothing
 * to the bus, so a group holding one collapses onto a simpler shape: writes
 * that are all empty plus a real read is just a read, and real writes plus an
 * empty read is just a write. Collapsing matters for more than tidiness -- the
 * FFI rejects a NULL pointer unconditionally, before it looks at any length,
 * because slice::from_raw_parts(NULL, 0) is undefined behaviour in Rust. So a
 * NULL pointer must never be handed to the bottom shim at all.
 *
 * PDG_I2C_OP_PROBE is what is left when a group carries no data bytes in
 * either direction. It is an address-only transaction, which the RP2040/RP2350
 * I2C block cannot emit: the address phase is driven solely by pushing bytes,
 * so START + ADDR + STOP is physically unreachable (rp-rs/rp-hal#678,
 * embassy-rs/embassy#4474). It is handled separately, never forwarded.
 */
enum pdg_i2c_op {
	PDG_I2C_OP_PROBE,
	PDG_I2C_OP_READ,
	PDG_I2C_OP_WRITE,
	PDG_I2C_OP_WRITE_READ,
};

/*
 * Pure over a group validate_group_() has already accepted, and called from
 * both the pre-pass and the execution loop for the same reason validate_group_()
 * is: the classification must be recoverable without an array whose size scales
 * with num_msgs, and it cannot disagree between the two calls because a read
 * writes into the bytes at msgs[i].buf and never into the i2c_msg descriptors.
 *
 * write_len is already the concatenated total of the group's writes, so N
 * empty writes and one empty write are indistinguishable here, which is
 * correct: neither puts a byte on the bus.
 */
static enum pdg_i2c_op classify_group_(const struct i2c_msg *msgs,
				       const struct pdg_i2c_group *group)
{
	bool has_tx = group->write_len != 0U;
	bool has_rx = group->has_read &&
		      (msgs[group->first + group->count - 1U].len != 0U);

	if (has_tx && has_rx) {
		return PDG_I2C_OP_WRITE_READ;
	}
	if (has_tx) {
		return PDG_I2C_OP_WRITE;
	}
	if (has_rx) {
		return PDG_I2C_OP_READ;
	}
	return PDG_I2C_OP_PROBE;
}

/* helper to map a Zephyr I2C speed (see the zephyr I2C_SPEED_... macros) into a
 * pico de gallo speed code (see gallo_i2c_set_config() in pico_de_gallo.h)
 * 
 * `speed` is the Zephyr I2C speed (meaning you will probably pass a
 * I2C_SPEED_... macro into the parameter). The returned value is one of the
 * possible pico-de-gallo speed codes accepted by the FFI
 * `gallo_i2c_set_config()` function via the `frequency` parameter:
 *
 * 0 = Standard (100 kHz), 1 = Fast (400 kHz), 2 = Fast+ (1 MHz).
 */
static int speed_to_code_(uint32_t speed, uint8_t* code)
{
	static const uint8_t Gallo_Standard = 0U; /* (100 kHz) */
	static const uint8_t Gallo_Fast = 1U; 	  /* (400 kHz) */
	static const uint8_t Gallo_FastPlus = 2U; /* (1 MHz) */

	switch (speed) {
	case I2C_SPEED_STANDARD:;
		*code = Gallo_Standard;
		return 0;
	case I2C_SPEED_FAST:
		*code = Gallo_Fast;
		return 0;
	case I2C_SPEED_FAST_PLUS:
		*code = Gallo_FastPlus;
		return 0;
	case I2C_SPEED_HIGH:
		LOG_ERR("pico-de-gallo does not support the configured I2C speed (I2C_SPEED_HIGH). Returning -EINVAL. Please use one of the supported variants: I2C_SPEED_STANDARD, I2C_SPEED_FAST, or I2C_SPEED_FAST_PLUS.");
		return -EINVAL; 
	case I2C_SPEED_ULTRA:
		LOG_ERR("pico-de-gallo does not support the configured I2C speed (I2C_SPEED_ULTRA). Returning -EINVAL. Please use one of the supported variants: I2C_SPEED_STANDARD, I2C_SPEED_FAST, or I2C_SPEED_FAST_PLUS.");
		return -EINVAL; 
	default:
		LOG_ERR("pico-de-gallo does not support the configured I2C speed (speed=%" PRIu32 "). Returning -EINVAL. Please use one of the supported variants: I2C_SPEED_STANDARD, I2C_SPEED_FAST, or I2C_SPEED_FAST_PLUS.", speed);
		return -EINVAL; 
	}
}

/* helper to map a `clock_frequency` in Hz to a Zephyr I2C speed macro */
static int freq_to_speed_(uint32_t clock_frequency, uint32_t* speed)
{
	switch(clock_frequency) {
	case 100000U:
		*speed = I2C_SPEED_STANDARD;
		return 0;
	case 400000U:
		*speed = I2C_SPEED_FAST;
		return 0;
	case 1000000U:
		*speed = I2C_SPEED_FAST_PLUS;
		return 0;
	case 3400000U:
		*speed = I2C_SPEED_HIGH;
		return 0;
	case 5000000U:
		*speed = I2C_SPEED_ULTRA;
		return 0;
	default:
		LOG_ERR("Invalid I2C frequency provided (frequency=%" PRIu32 "). "
				"Returning -EINVAL. Try using one of the following: "
				"I2C_SPEED_STANDARD (100_000 Hz), I2C_SPEED_FAST (400_000 Hz), "
				"I2C_SPEED_FAST_PLUS (1_000_000 Hz), "
				"I2C_SPEED_HIGH (3_400_000 Hz), "
				"or I2C_SPEED_ULTRA (5_000_000 Hz).", clock_frequency);
		return -EINVAL;
	}
}

/*
 * Classify one STOP-delimited group of messages and check its size bounds.
 *
 * Accepted shapes, all of which the FFI can express as exactly one call:
 *
 *   - N write messages, N >= 1. Their payloads concatenate into a single
 *     gallo_i2c_write(): one START, one address phase, all the bytes, one
 *     STOP. That is precisely what a real controller puts on the wire for
 *     Zephyr's i2c_burst_write(), which emits I2C_MSG_WRITE followed by
 *     I2C_MSG_WRITE | I2C_MSG_STOP and used to be rejected here. Issue #102.
 *   - A single read message, which becomes gallo_i2c_read().
 *   - N write messages followed by exactly one read, which becomes
 *     gallo_i2c_write_read().
 *
 * Rejected: any group containing a read that is not the final message, and any
 * group containing more than one read. The FFI has no shape for reading and
 * then continuing within one transaction, and splitting such a group into
 * several calls would insert a STOP the caller did not ask for.
 *
 * The terminating read of a multi-message group must carry I2C_MSG_RESTART. A
 * direction change inside a transaction is a repeated START, which is what
 * gallo_i2c_write_read() issues, and every Zephyr helper that produces this
 * shape (i2c_write_read(), i2c_burst_read(), i2c_reg_read_byte()) sets the
 * flag. This requirement is carried over unchanged from the two-message rule
 * this function replaces; a group that was rejected for lacking it before is
 * still rejected for lacking it now.
 *
 * This function is pure over `msgs` and is deliberately called twice per
 * group: once during the pre-pass, and once during execution to recover the
 * same classification without an array whose size would scale with num_msgs.
 * It cannot disagree between the two calls, because a read writes into the
 * bytes at msgs[i].buf and never into the i2c_msg descriptors themselves.
 */
static int validate_group_(const struct i2c_msg *msgs, uint8_t first, uint8_t count,
			   struct pdg_i2c_group *group)
{
	size_t write_len = 0U;
	uint8_t write_count = 0U;
	bool has_read = false;

	for (uint8_t i = 0U; i < count; i++) {
		const struct i2c_msg *msg = &msgs[first + i];

		if ((msg->flags & I2C_MSG_READ) != 0U) {
			if (i != (count - 1U)) {
				LOG_ERR("I2C message %u is a read in the middle of a "
					"STOP-delimited group of %u messages starting at "
					"message %u. A read may only be the last message of a "
					"group. Returning -ENOTSUP.",
					first + i, count, first);
				return -ENOTSUP;
			}

			has_read = true;
			break;
		}

		/*
		 * A running total, not a per-message check. Two 4096-byte
		 * writes each pass individually and concatenate to 8192.
		 * Subtracting from the limit rather than adding to the total
		 * cannot overflow, because write_len never exceeds it.
		 */
		if (msg->len > (PDG_I2C_MAX_BUFFER - write_len)) {
			LOG_ERR("The write messages of the I2C group starting at message %u "
				"exceed the %u-byte transfer limit at message %u (%" PRIu32
				" bytes after %zu). Returning -EMSGSIZE.",
				first, PDG_I2C_MAX_BUFFER, first + i, msg->len, write_len);
			return -EMSGSIZE;
		}

		write_len += msg->len;
		write_count++;
	}

	if (has_read) {
		const struct i2c_msg *read = &msgs[first + count - 1U];

		if ((count > 1U) && ((read->flags & I2C_MSG_RESTART) == 0U)) {
			LOG_ERR("I2C message %u terminates a write-then-read group starting at "
				"message %u but does not set I2C_MSG_RESTART. Changing direction "
				"inside a transaction requires a repeated start. Returning "
				"-ENOTSUP.",
				first + count - 1U, first);
			return -ENOTSUP;
		}

		if (read->len > PDG_I2C_MAX_BUFFER) {
			LOG_ERR("I2C read message %u is %" PRIu32 " bytes, which exceeds the "
				"%u-byte transfer limit. Returning -EMSGSIZE.",
				first + count - 1U, read->len, PDG_I2C_MAX_BUFFER);
			return -EMSGSIZE;
		}
	}

	group->first = first;
	group->count = count;
	group->write_count = write_count;
	group->has_read = has_read;
	group->write_len = write_len;

	return 0;
}

/*
 * Present a group's leading write messages as one contiguous payload.
 *
 * On success *payload points at the bytes to send and *scratch is either NULL
 * or a k_malloc() block the caller must k_free(). Only called with
 * group->write_count >= 1.
 *
 * No copy is made when the group holds fewer than two writes, or when every
 * write in it is empty. In both cases msgs[first].buf together with
 * group->write_len already denotes the exact payload, so the ordinary
 * single-write and write-then-read paths stay allocation-free.
 *
 * The all-empty case is now unreachable from pdg_i2c_transfer(): a group whose
 * writes total zero bytes classifies as PDG_I2C_OP_READ or PDG_I2C_OP_PROBE
 * (issue #137), neither of which calls this function. The guard is kept so
 * this helper remains correct on its own terms rather than only in the context
 * of its one caller -- an empty payload has nothing to merge, and allocating
 * for it would introduce an -ENOMEM failure mode on a call that can otherwise
 * not fail locally. (k_malloc(0) does succeed -- z_alloc_helper() adds a heap
 * reference before allocating -- so this is a deliberate choice, not a
 * workaround for a NULL return.)
 */
static int gather_writes_(const struct i2c_msg *msgs, const struct pdg_i2c_group *group,
			  const uint8_t **payload, uint8_t **scratch)
{
	uint8_t *flat;
	size_t offset = 0U;

	*scratch = NULL;

	if ((group->write_count < 2U) || (group->write_len == 0U)) {
		*payload = msgs[group->first].buf;
		return 0;
	}

	flat = k_malloc(group->write_len);
	if (flat == NULL) {
		LOG_ERR("Failed to allocate a %zu-byte buffer to merge I2C messages %u-%u "
			"into one write. Returning -ENOMEM.",
			group->write_len, group->first, group->first + group->write_count - 1U);
		return -ENOMEM;
	}

	for (uint8_t i = 0U; i < group->write_count; i++) {
		const struct i2c_msg *msg = &msgs[group->first + i];

		/*
		 * The NULL guard is load-bearing since issue #137 relaxed the
		 * NULL-buffer rejection to fire only when len is non-zero: a
		 * group may now legitimately mix an empty message carrying a
		 * NULL buf with non-empty ones, and memcpy(dst, NULL, 0) is
		 * undefined behaviour in C even at n == 0.
		 */
		if (msg->buf != NULL) {
			memcpy(flat + offset, msg->buf, msg->len);
		}

		offset += msg->len;
	}

	*payload = flat;
	*scratch = flat;

	return 0;
}

static int pdg_i2c_configure(const struct device *dev, uint32_t dev_config)
{
	struct pdg_i2c_data *data = dev->data;
	int ret;

	/*
	 * Zephyr's z_impl_i2c_transfer(), and the configure/get-config entry
	 * points, dispatch straight into the driver without checking device
	 * readiness -- exactly as spi_transceive() does, which is why
	 * pdg_spi_transceive() already carries this guard. An application that
	 * skips device_is_ready() therefore reaches here on a device whose init
	 * failed. Guarding before the lock or any cached state is read keeps a
	 * failed child from locking an uninitialized mutex, issuing an RPC
	 * through a stale borrow, or returning zero-initialized configuration
	 * as a false success.
	 *
	 * This hole predates the MFD migration. It is closed here because the
	 * migration's cache-invalidation property (a failed child clears its
	 * borrowed pointer, so direct calls fail safely) is not actually true
	 * without it, and because the GPIO child added in a later milestone
	 * copies this child-driver pattern.
	 */
	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo I2C bridge context is NULL; check device readiness. Returning -ENODEV.",
			dev->name);
		return -ENODEV;
	}

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

	/* See pdg_i2c_configure(): direct API dispatch does not check readiness. */
	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo I2C bridge context is NULL; check device readiness. Returning -ENODEV.",
			dev->name);
		return -ENODEV;
	}

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

	/* See pdg_i2c_configure(): direct API dispatch does not check readiness. */
	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo I2C bridge context is NULL; check device readiness. Returning -ENODEV.",
				dev->name);
		return -ENODEV;
	}

	if (addr > 0x7fU) {
		LOG_ERR("I2C address 0x%04x exceeds the 7-bit address range. Returning -EINVAL.", addr);
		return -EINVAL;
	}

	// validate the provided messages
	for (uint8_t i = 0U; i < num_msgs; i++) {
		/*
		 * A NULL buffer is only an error when the message claims to
		 * carry bytes. Zephyr explicitly permits buf == NULL when
		 * len == 0, which is what i2c_write(dev, NULL, 0, addr) and
		 * the write half of i2c_write_read(dev, addr, NULL, 0, ...)
		 * produce (issue #137). A message carrying no bytes is never
		 * forwarded: classify_group_() reduces the group to the
		 * operation that does carry data, and gather_writes_() skips
		 * an empty message rather than memcpy()ing from it. So the
		 * NULL pointer never reaches the FFI -- which rejects NULL
		 * unconditionally, ahead of any length check, because
		 * slice::from_raw_parts(NULL, 0) is undefined behaviour in
		 * Rust.
		 */
		if ((msgs[i].buf == NULL) && (msgs[i].len != 0U)) {
			LOG_ERR("NULL buffer provided for I2C message %u (len=%" PRIu32 "). "
					"Returning -EINVAL.", i, msgs[i].len);
			return -EINVAL;
		}

		/* make sure I2C_MSG_ADDR_10_BITS isn't requested since it isn't supported */
		if ((msgs[i].flags & I2C_MSG_ADDR_10_BITS) != 0U) {
			LOG_ERR("I2C message %u is requesting 10-bit addressing (I2C_MSG_ADDR_10_BITS), but this addressing is unsupported. Returning -ENOTSUP.", i);
			return -ENOTSUP;
		}

		if ((msgs[i].flags & I2C_MSG_STOP) != 0U) {
			/*
			 * Size limits are checked per group rather than per
			 * message, because the writes of a group are sent as
			 * one payload. See validate_group_().
			 */
			struct pdg_i2c_group group;

			ret = validate_group_(msgs, group_start, i - group_start + 1U, &group);
			if (ret < 0) {
				return ret;
			}

			/*
			 * Refuse a degenerate group here rather than in the
			 * execution loop, so that a multi-group transfer
			 * containing one is rejected before the mutex is taken
			 * and before any earlier group has reached the bus.
			 * That keeps validation a complete pre-pass, which the
			 * rest of this function already relies on.
			 */
			if ((classify_group_(msgs, &group) == PDG_I2C_OP_PROBE) &&
			    !IS_ENABLED(CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ)) {
				LOG_ERR("The I2C group starting at message %u carries no data "
					"bytes in either direction. The RP2040/RP2350 I2C block "
					"cannot emit an address-only transaction; enable "
					"CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ to substitute a "
					"1-byte read. Returning -ENOTSUP.", group_start);
				return -ENOTSUP;
			}

			group_start = i + 1U;
		}
	}

	/* pico-de-gallo-ffi's I2C currently always generates STOP. The Zephyr
	 * should API conform to this by default but it is still possible to
	 * manually attempt low-level I2C transactions that omit STOP, so we
	 * gotta check for that:
	 */
	if (group_start != num_msgs) {
		LOG_ERR("A final I2C transaction without STOP is unsupported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	/* okay at this point we know all the groups are known to be supported
	 * so we can start actually transferring
	 */
	k_mutex_lock(&data->lock, K_FOREVER);
	group_start = 0U;
	ret = 0;

	/* loop through every message and send one FFI call per "message group"
	 *
	 * - a "message group" contains all messages since the previous STOP
	 * - the shapes a group may take are enumerated by validate_group_(),
	 *   which the pre-pass above has already run over every one of them
	 *
	 * the purpose of sending messages in a "message group" is because
	 * pico-de-gallo-ffi uses STOP for each individual operation
	 */
	for (uint8_t i = 0U; i < num_msgs; i++) {
		struct pdg_i2c_group group;
		enum pdg_i2c_op op;

		// if this message isn't a STOP it isn't the end to a message group, so we can skip
		if ((msgs[i].flags & I2C_MSG_STOP) == 0U) {
			continue;
		}

		/*
		 * Re-derives what the pre-pass already accepted. It cannot
		 * fail here and cannot disagree: validate_group_() is pure
		 * over msgs, and a completed read writes into the bytes at
		 * msgs[].buf, never into the descriptors. Checked anyway so a
		 * future edit that breaks that property fails loudly instead
		 * of transferring against a stale classification.
		 */
		ret = validate_group_(msgs, group_start, i - group_start + 1U, &group);
		if (ret < 0) {
			LOG_ERR("I2C group starting at message %u passed validation but was "
				"rejected during execution: errno=%d. This is a driver bug.",
				group_start, ret);
			break;
		}

		op = classify_group_(msgs, &group);

		switch (op) {
		case PDG_I2C_OP_READ: {
			/*
			 * A lone read, or a group whose writes are all empty
			 * and so put nothing on the bus. Either way the read is
			 * the last message of the group.
			 */
			const struct i2c_msg *read = &msgs[group.first + group.count - 1U];

			ret = pdg_i2c_bottom_read(data->ctx, addr, read->buf, read->len);
			if (ret < 0) {
				LOG_ERR("I2C read message %u from address 0x%02x failed (%" PRIu32
						" bytes): errno=%d.",
						group.first + group.count - 1U, addr, read->len, ret);
			}
			break;
		}
		case PDG_I2C_OP_WRITE:
		case PDG_I2C_OP_WRITE_READ: {
			// one or more WRITE messages, merged into a single payload
			const uint8_t *payload;
			uint8_t *scratch;

			/*
			 * On failure gather_writes_() has allocated nothing and
			 * left *scratch NULL, so there is nothing to free; the
			 * shared check below then breaks the loop.
			 */
			ret = gather_writes_(msgs, &group, &payload, &scratch);
			if (ret == 0) {
				if (op == PDG_I2C_OP_WRITE_READ) {
					// group is a WRITE-then-READ operation
					const struct i2c_msg *read =
						&msgs[group.first + group.write_count];

					ret = pdg_i2c_bottom_write_read(data->ctx, addr, payload,
							group.write_len, read->buf, read->len);
					if (ret < 0) {
						LOG_ERR("I2C write-read messages %u-%u at address "
								"0x%02x failed (TX=%zu bytes, RX=%" PRIu32
								" bytes): errno=%d.",
								group.first,
								group.first + group.count - 1U,
								addr, group.write_len, read->len, ret);
					}
				} else {
					/*
					 * Only writes put bytes on the bus. A
					 * terminating read of zero bytes, if the
					 * group had one, adds nothing and is
					 * dropped rather than turned into an
					 * empty read phase.
					 */
					ret = pdg_i2c_bottom_write(data->ctx, addr, payload,
							group.write_len);
					if (ret < 0) {
						LOG_ERR("I2C write messages %u-%u to address 0x%02x "
								"failed (%zu bytes): errno=%d.",
								group.first,
								group.first + group.count - 1U,
								addr, group.write_len, ret);
					}
				}

				/* NULL whenever no copy was made; k_free() ignores it. */
				k_free(scratch);
			}
			break;
		}
		case PDG_I2C_OP_PROBE: {
			/*
			 * The pre-pass already returned -ENOTSUP for this shape
			 * unless the substitution was opted into, so reaching
			 * here means it was. The probe byte is a loop local,
			 * not a file static: one driver instance per device,
			 * each under its own mutex.
			 */
			uint8_t probe = 0U;

			if (IS_ENABLED(CONFIG_I2C_PICO_DE_GALLO_PROBE_WITH_READ)) {
				ret = pdg_i2c_bottom_read(data->ctx, addr, &probe, 1U);
				if (ret < 0) {
					LOG_ERR("I2C probe of address 0x%02x for the group starting "
							"at message %u failed (1-byte read substituted "
							"for an address-only transaction): errno=%d.",
							addr, group.first, ret);
				}
			} else {
				ret = -ENOTSUP;
			}
			break;
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

	/*
	 * The mutex is initialized before any early return so that every device
	 * object that exists at all has a usable lock. Zephyr's I2C API
	 * dispatches directly into the driver without a readiness check, so a
	 * direct call on a failed device must find an initialized mutex; the
	 * data->ctx == NULL guards at the top of each callback then turn that
	 * call into -ENODEV.
	 */
	k_mutex_init(&data->lock);

	/*
	 * Mandatory MFD child sequence (pdg_mfd.h): require parent readiness
	 * first, then borrow the context. A NULL context *after* a passing
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

	uint32_t speed = 0;
	ret = freq_to_speed_(config->clock_frequency, &speed);
	if (ret < 0) {
		/*
		 * Defensive invalidation of this child's cached borrow -- never
		 * a reference release. The parent holds the sole registry
		 * reference; closing here would drop it and leave the parent
		 * and the SPI sibling holding a freed pointer. NULL is
		 * guardable and becomes -ENODEV; a valid-looking unowned
		 * pointer would bypass every NULL check.
		 */
		data->ctx = NULL;
		return ret;
	}

	uint32_t dev_config_ = I2C_MODE_CONTROLLER | I2C_SPEED_SET(speed);

	uint8_t code = 0;
	ret = speed_to_code_(speed, &code);
	if(ret < 0) {
		data->ctx = NULL;
		return ret; 
	}

	ret = pdg_i2c_bottom_set_config(data->ctx, code);
	if (ret < 0) {
		LOG_ERR("Failed to set I2C config: errno=%d", ret);
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
		.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst)),			\
		.clock_frequency = DT_INST_PROP_OR(inst, clock_frequency,	\
						   I2C_BITRATE_STANDARD),	\
	};									\
										\
	I2C_DEVICE_DT_INST_DEFINE(inst, pdg_i2c_init, NULL,			\
				  &pdg_i2c_data_##inst,				\
				  &pdg_i2c_config_##inst, POST_KERNEL,		\
				  CONFIG_I2C_INIT_PRIORITY, &pdg_i2c_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_I2C_INIT)
