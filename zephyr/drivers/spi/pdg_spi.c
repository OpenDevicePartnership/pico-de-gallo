/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr SPI controller driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context. Note that "embedded" here just
 * means the embedded part of `native-sim`, not something that actually gets
 * flashed to hardware. It translates Zephyr SPI API transactions into the small
 * host-context shim declared in pdg_spi_bottom.h, which forwards them to the
 * Pico de Gallo C FFI.
 *
 * Chip select is ordinary Zephyr cs-gpios. Every enabled controller declares at
 * least one entry, each entry targets an enabled odp,pico-de-gallo-gpio sibling
 * under the exact same odp,pico-de-gallo parent, and a child node's reg is a
 * plain index into that array. Selection is therefore no longer atomic with the
 * data phase: an ordinary successful transceive is four USB round trips
 * (set-config, assert, transfer, deassert) instead of one firmware batch. The
 * trade buys standard devicetree composition, SPI_LOCK_ON and a safe
 * SPI_HOLD_ON_CS | SPI_LOCK_ON, at the cost of the batch's chip-select interval
 * guarantee.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_spi

#include <zephyr/device.h>
#include <zephyr/drivers/gpio.h>
#include <zephyr/drivers/spi.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>
#include <zephyr/sys/util.h>

#include <string.h>

#include "pdg_spi_bottom.h"

/*
 * Every operation in this driver is a blocking USB round trip, so the
 * asynchronous and RTIO driver ops are not implemented: .transceive and
 * .release are the only entries in pdg_spi_driver_api.
 *
 * Zephyr's SPI subsystem dispatches transceive_async() and iodev_submit()
 * straight through the driver API with no NULL check -- unlike I2C, which
 * returns -ENOSYS. Compiling this driver into an image that enables either
 * option would turn any async use into a jump through a NULL function pointer,
 * which on native_sim is a segfault rather than an error return.
 * CONFIG_SPI_RTIO is also selected transitively by CONFIG_SENSOR_ASYNC_API, so
 * an application can reach it without asking.
 *
 * This is deliberately a BUILD_ASSERT rather than a Kconfig "depends on
 * !SPI_ASYNC". Both SPI_ASYNC and SPI_RTIO depend on SPI, which this driver
 * selects, so expressing the constraint in Kconfig produces a dependency loop
 * and refuses to parse. A BUILD_ASSERT also fails loudly with the reason, where
 * "depends on" would silently drop the driver and surface as an unresolved
 * __device_dts_ord_N at link time.
 */
BUILD_ASSERT(!IS_ENABLED(CONFIG_SPI_ASYNC),
	     "The Pico de Gallo SPI driver does not implement transceive_async(); "
	     "disable CONFIG_SPI_ASYNC.");
BUILD_ASSERT(!IS_ENABLED(CONFIG_SPI_RTIO),
	     "The Pico de Gallo SPI driver does not implement iodev_submit(); "
	     "disable CONFIG_SPI_RTIO (CONFIG_SENSOR_ASYNC_API selects it).");

/*
 * Per-chip-select topology enforcement.
 *
 * Each cs-gpios entry must satisfy three independent conditions, and all three
 * are load-bearing:
 *
 *   1. compatible -- a foreign controller (native_sim's zephyr,gpio-emul, for
 *      instance) is an enabled, bound, perfectly functional GPIO port that
 *      simply is not on this board at all.
 *
 *   2. status okay -- a disabled sibling has no device object to reach.
 *
 *   3. same parent -- this is the discriminating one. A genuine, enabled
 *      odp,pico-de-gallo-gpio controller under a *different* odp,pico-de-gallo
 *      parent passes both checks above while living on a different physical
 *      board, so chip select would be driven on one board and data clocked on
 *      another. That is the 2026-07-29 ambiguous-target failure class
 *      (AGENTS.md §13.17) expressed in devicetree. Do not drop this clause.
 */
#define PDG_SPI_CS_ASSERT(node_id, prop, idx, inst)				\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_GPIO_CTLR_BY_IDX(node_id, prop, idx),	\
				   odp_pico_de_gallo_gpio),			\
		"odp,pico-de-gallo-spi cs-gpios entry " #idx " must target an "	\
		"odp,pico-de-gallo-gpio controller");				\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(					\
			DT_GPIO_CTLR_BY_IDX(node_id, prop, idx)),		\
		"odp,pico-de-gallo-spi cs-gpios entry " #idx " must target an "	\
		"odp,pico-de-gallo-gpio controller with status okay");		\
	BUILD_ASSERT(								\
		DT_SAME_NODE(							\
			DT_PARENT(DT_GPIO_CTLR_BY_IDX(node_id, prop, idx)),	\
			DT_INST_PARENT(inst)),					\
		"odp,pico-de-gallo-spi cs-gpios entry " #idx " must target an "	\
		"odp,pico-de-gallo-gpio controller under the same "		\
		"odp,pico-de-gallo parent as this controller");

/*
 * Structural topology enforcement, by the same reasoning as the assertions
 * above: an explicit BUILD_ASSERT that names the problem beats an unresolved
 * __device_dts_ord_N at link time.
 *
 * This controller borrows its host connection from an odp,pico-de-gallo MFD
 * parent reached through DT_INST_PARENT(). Runtime readiness alone cannot prove
 * the parent's *type*: a child under an unrelated but enabled and ready device
 * would pass device_is_ready(), and pdg_mfd_ctx() would then reinterpret that
 * foreign driver's dev->data as struct pdg_mfd_data and return an arbitrary
 * pointer no NULL check can catch. DT_INST_PARENT() on a stale root-level child
 * yields `/`, so status alone is not sufficient either; the compatible must be
 * asserted separately.
 *
 * The order compatible -> parent status -> parent serial -> Kconfig -> per-CS
 * is deliberate and is the normative contract. Disabling the parent also drops
 * DT_HAS_ODP_PICO_DE_GALLO_ENABLED and therefore makes CONFIG_MFD_PICO_DE_GALLO
 * `n`, so the Kconfig assertion would be true simultaneously with the status
 * one; emitting the most specific structural diagnostic first keeps the message
 * naming the actual topology error at the top. (_Static_assert is not fatal, so
 * GCC reports all failing assertions in one pass.)
 *
 * The serial-presence assertion exists because chip select now actuates a
 * physical GPIO, exactly as the GPIO child does. A selector-less strict open
 * cannot report which attached board it selected, so an enabled SPI controller
 * under a selector-less parent would drive an unidentifiable board's pins.
 * Presence is not uniqueness: two parents carrying the same explicit serial
 * still alias to one board.
 *
 * They also precede the "pdg_mfd.h" include on purpose: with
 * CONFIG_MFD_PICO_DE_GALLO=n the MFD driver subdirectory is not added to the
 * build, so pdg_mfd.h is not on the include path. Asserting first guarantees
 * the readable configuration error appears before the include failure.
 */
#define PDG_SPI_PARENT_ASSERTS(inst)						\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_INST_PARENT(inst), odp_pico_de_gallo),	\
		"Enabled odp,pico-de-gallo-spi controllers must be direct "	\
		"children of an odp,pico-de-gallo parent");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(DT_INST_PARENT(inst)),			\
		"Enabled odp,pico-de-gallo-spi controllers require their "	\
		"odp,pico-de-gallo parent to have status okay");		\
	BUILD_ASSERT(								\
		DT_NODE_HAS_PROP(DT_INST_PARENT(inst), serial_number),		\
		"odp,pico-de-gallo-spi parent must define serial-number");	\
	BUILD_ASSERT(								\
		IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO),				\
		"Enabled Pico de Gallo child controllers require "		\
		"CONFIG_MFD_PICO_DE_GALLO=y");					\
	DT_FOREACH_PROP_ELEM_VARGS(DT_DRV_INST(inst), cs_gpios,			\
				   PDG_SPI_CS_ASSERT, inst)

DT_INST_FOREACH_STATUS_OKAY(PDG_SPI_PARENT_ASSERTS)

#include "pdg_mfd.h"

LOG_MODULE_REGISTER(spi_pico_de_gallo, CONFIG_SPI_LOG_LEVEL);

/* spi_context.h uses LOG_ERR in an inline helper, so it must follow the
 * LOG_MODULE_REGISTER above.
 */
#include "spi_context.h"

/*
 * Measured usable payload ceiling for one SPI transfer.
 *
 * SAFETY FIRST, SIZE SECOND. The strongest argument for this specific number is
 * not capacity, it is containment: a 1015-byte TX-only spi/transfer NEVER
 * RETURNS and wedges the firmware dispatcher device-wide (see below).
 * Rejecting 1014 and above locally, with -EMSGSIZE, before any allocation,
 * lock, set-config, chip-select edge or transport call, puts that hang out of
 * reach through this driver. That containment argument holds regardless of how
 * the size question is eventually resolved.
 *
 * MODEL. This is a packet-buffer budget, NOT the usable payload size. The
 * firmware's per-packet buffer has to hold the payload PLUS the postcard-rpc
 * header, the length varint and the COBS framing, so usable payload must sit
 * strictly below the budget and the difference is not a round number. Two
 * earlier values were set from that model without measuring it, and both were
 * wrong:
 *
 *   4096  was pico_de_gallo_internal::MAX_TRANSFER_SIZE, commented as the
 *         "firmware single-transfer limit". 4096 TX-only reaches the transport
 *         and fails -ECOMM.
 *   3072  was a "conservative" guess reasoned from the firmware's
 *         PacketBuffers<MAX_TRANSFER_SIZE + 1024> headroom. 3072 full duplex
 *         also fails -ECOMM. That reasoning considered ONE direction; the
 *         budget must cover the request frame AND the response frame.
 *
 * MEASURED, NOT DERIVED. 1013 is the largest TX-only length observed to work on
 * hardware on the M5 acceptance fixture board, across two byte-identical
 * consecutive runs. Every observed failure was -ECOMM and never -EMSGSIZE, so
 * the transport was always the limiter and the compiled constant never was.
 *
 * WHAT IS STILL UNKNOWN, stated plainly so nobody mistakes this for a solved
 * problem:
 *
 *   - The TX-only boundary is unresolved between 1013 and 1015. 1014 was never
 *     probed, and 1015 hangs, so the boundary cannot currently be
 *     narrowed by bisection without stepping into the hang.
 *   - Full duplex succeeded at 512 bytes and failed at 3072 bytes. It was not
 *     tested from 513 through 1013, so duplex at 1013 is UNVERIFIED.
 *     Applications needing a documented-safe duplex size must use 512 bytes or
 *     less; do not infer 1013-byte duplex support from this constant.
 *   - A lower constant reduces exposure to the known hang. It does NOT prove
 *     that no other hang window exists below it.
 *   - 1013 is close to 1024, which would be consistent with a ~1 KiB budget and
 *     about 11 bytes of framing. That is SUGGESTIVE ONLY: there is no evidence
 *     for that decomposition and it must not be relied on.
 *
 * KNOWN FIRMWARE HANG (root cause is in crates/, out of scope here). A
 * 1015-byte TX-only spi/transfer never returns and wedges the dispatcher for
 * every subsequent RPC, including from a fresh host process and including
 * system/reset-subscriptions. The 2 s watchdog does not catch it, because the
 * dedicated feeder task keeps feeding while a handler blocks. In the reproduced
 * SPI tests the device resumed after USB re-enumeration (usbipd detach followed
 * by attach on Windows/WSL). This is an observed procedure, not proof that
 * detach directly cancels the blocked handler. On other hosts use cable
 * reconnect or USB unbind/rebind; power-cycle if re-enumeration is unavailable
 * or ineffective. system/reset-subscriptions cannot run while dispatch is
 * blocked.
 *
 * FOLLOW-UP (do not just raise this number, and do not lower it by guesswork
 * either): derive the usable spi/transfer payload ceiling from the worst-case
 * request and response framing, express it as one generated or shared contract
 * rather than a constant duplicated per consumer, and pin limit and limit+1
 * tests against it. That remains the only defensible long-term route, and it
 * needs a wire-crate change with schema and lockstep-release implications,
 * which is out of scope for this module.
 */
#define PDG_SPI_MAX_BUFFER 1013U

struct pdg_spi_config {
	const struct device *mfd;
	const char *serial_number;
};

struct pdg_spi_data {
	struct spi_context spi_ctx; /* MUST be first */
	void *ctx;
	bool cs_fault;
	int cs_fault_errno;
};

/*
 * Diagnostic helper: the chip-select pin currently selected by ctx->config, or
 * 0xFF when no GPIO chip select is in play. Used only in log lines.
 */
static uint8_t pdg_spi_cs_pin(const struct spi_context *ctx)
{
	if ((ctx->config != NULL) && spi_cs_is_gpio(ctx->config)) {
		return (uint8_t)ctx->config->cs.gpio.pin;
	}

	return 0xFFU;
}

/*
 * Checked chip-select edge.
 *
 * Do not replace this with `spi_context_cs_control()`. PDG CS is a fallible,
 * potentially non-returning USB GPIO operation; Zephyr's void helper discards
 * errno. This helper preserves upstream delay/HOLD rules while making returning
 * failures observable.
 *
 * Behaviour mirrors _spi_context_cs_control() exactly, including the collapsed
 * DIV_ROUND_UP(MAX(setup_ns, hold_ns), 1000) microsecond wait that Zephyr
 * stores in config->cs.delay and applies at *both* edges: after a successful
 * assert, and before the deassert write.
 *
 * The level is never verified by reading the pin back. The GPIO child masks an
 * explicit output to zero in port_get_raw(), and a legacy-mode read mutates the
 * pin's direction, so a readback would either lie or corrupt the line.
 */
static int pdg_spi_cs_control_checked(struct spi_context *ctx, bool on,
		bool force_off)
{
	const struct spi_config *config = ctx->config;
	int ret;

	if ((config == NULL) || !spi_cs_is_gpio(config)) {
		return 0;
	}

	if (on) {
		ret = gpio_pin_set_dt(&config->cs.gpio, 1);
		if (ret < 0) {
			return ret;
		}

		k_busy_wait(config->cs.delay);

		return 0;
	}

	if (!force_off && ((config->operation & SPI_HOLD_ON_CS) != 0U)) {
		return 0;
	}

	k_busy_wait(config->cs.delay);

	ret = gpio_pin_set_dt(&config->cs.gpio, 0);
	if (ret < 0) {
		return ret;
	}

	return 0;
}

/*
 * Defanged unconditional unlock.
 *
 * spi_context_unlock_unconditionally() begins with a *void* forced CS-off edge
 * and only then clears the owner and gives the semaphore. On this controller
 * that edge is a second USB GPIO write which duplicates the checked deassert we
 * already performed, discards its errno, and -- worst of all -- can fail to
 * return at all, wedging software ownership before the semaphore is ever given.
 *
 * Clearing ctx->config first makes the stock edge guard false, so the helper
 * degenerates to exactly the owner/semaphore bookkeeping we want and issues no
 * GPIO traffic. This is deliberate defanging: do not restore the idiomatic
 * live-config call. Verified against the pinned upstream source --
 * _spi_context_cs_control() does nothing when ctx->config == NULL, and the
 * remainder reads only lock and owner.
 *
 * The saved pointer is restored only when the preceding checked deassert
 * failed, so a latched controller keeps the exact recovery target a later
 * spi_release(dev, saved) must match. A successful release leaves it NULL,
 * which is what makes a second release rejectable.
 */
static void pdg_spi_unlock_defanged(struct spi_context *ctx, bool deassert_failed)
{
	const struct spi_config *saved = ctx->config;

	ctx->config = NULL;
	spi_context_unlock_unconditionally(ctx);

	if (deassert_failed) {
		ctx->config = saved;
	}
}

/* helper to calculate the total byte length of every buffer in a `spi_buf_set`
 * used when flattening Zephyr `spi_buf_set`s into a normal contiguous buffer to
 * pass into the pico-de-gallo ffi (the `direction` parameter isn't part of the
 * actual calculation, it is just for debugging verbosity)
 */
static int bufset_len_(const struct spi_buf_set *bufs, size_t *total_len, const char *direction)
{
	*total_len = 0U;

	if (bufs == NULL) {
		return 0;
	}
	if (bufs->count != 0U && bufs->buffers == NULL) {
		LOG_ERR("SPI %s buffer set has count %zu but no buffer array. Returning -EINVAL.",
				direction, bufs->count);
		return -EINVAL;
	}

	for (size_t i = 0U; i < bufs->count; ++i) {
		if (bufs->buffers[i].len > PDG_SPI_MAX_BUFFER - *total_len) {
			LOG_WRN("SPI %s buffers exceed maximum transfer size of %u bytes. Returning -EMSGSIZE",
					direction, PDG_SPI_MAX_BUFFER);
			return -EMSGSIZE;
		}
		*total_len += bufs->buffers[i].len;
	}

	return 0;
}

/* helper that flattens a set of `spi_buf_set`s into a single buffer */
static void flatten_tx_(const struct spi_buf_set *tx_bufs, uint8_t *flat, size_t flat_len)
{
	size_t offset = 0U;

	memset(flat, 0, flat_len);
	if (tx_bufs == NULL) {
		return;
	}

	for (size_t i = 0U; i < tx_bufs->count; ++i) {
		const struct spi_buf *buf = &tx_bufs->buffers[i];

		if (buf->buf != NULL) {
			memcpy(flat + offset, buf->buf, buf->len);
		}
		offset += buf->len;
	}
}

/* helper that takes in a normal flat buffer from pico-de-gallo and organizes it
 * into the multiple buffer sets provided by zephyr
 */
static void unflatten_rx_(const struct spi_buf_set *rx_bufs, const uint8_t *flat)
{
	size_t offset = 0U;

	if (rx_bufs == NULL) {
		return;
	}

	for (size_t i = 0U; i < rx_bufs->count; ++i) {
		const struct spi_buf *buf = &rx_bufs->buffers[i];

		if (buf->buf != NULL) {
			memcpy(buf->buf, flat + offset, buf->len);
		}
		offset += buf->len;
	}
}

/* helper macro for `pdg_spi_transceive()`'s op flag checks. */
#define OP_HAS_FLAG_(flag) (((config)->operation & (flag)) != 0U)

static int pdg_spi_transceive(const struct device *dev,
		const struct spi_config *config,
		const struct spi_buf_set *tx_bufs,
		const struct spi_buf_set *rx_bufs)
{
	struct pdg_spi_data *data = dev->data;
	const struct pdg_spi_config *dev_config = dev->config;
	struct spi_context *ctx = &data->spi_ctx;
	uint8_t *tx_flat = NULL;
	uint8_t *rx_flat = NULL;
	size_t tx_len;
	size_t rx_len;
	size_t clock_len;
	bool commit_rx = false;
	bool deassert_failed = false;
	bool retain_lock = false;
	int cleanup;
	int ret;

	/*
	 * Zephyr's spi_transceive() does not check device readiness, so an
	 * application that skips device_is_ready() reaches here on a device
	 * whose init failed. Guard first: everything below dereferences the
	 * borrowed context or the statically initialized spi_context.
	 */
	if (data->ctx == NULL) {
		LOG_ERR("SPI bridge context is NULL; the controller's chip-select GPIOs were "
			"never configured. Check device readiness and the controller's "
			"cs-gpios property. Returning -ENODEV.");
		return -ENODEV;
	}

	if (config == NULL) {
		LOG_ERR("SPI configuration is NULL. Returning -EINVAL.");
		return -EINVAL;
	}

	if (SPI_OP_MODE_GET(config->operation) != SPI_OP_MODE_MASTER) {
		LOG_ERR("The configured SPI peripheral mode is not supported. Only SPI_OP_MODE_MASTER is supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (SPI_WORD_SIZE_GET(config->operation) != 8U) {
		LOG_ERR("The configured SPI word size is not supported. Only 8-bit SPI words are supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (OP_HAS_FLAG_(SPI_TRANSFER_LSB)) {
		LOG_ERR("This SPI operation (SPI_TRANSFER_LSB) is not supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (OP_HAS_FLAG_(SPI_MODE_LOOP)) {
		LOG_ERR("This SPI operation (SPI_MODE_LOOP) is not supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (OP_HAS_FLAG_(SPI_HALF_DUPLEX)) {
		LOG_ERR("This SPI operation (SPI_HALF_DUPLEX) is not supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	/*
	 * SPI_HOLD_ON_CS leaves a slave selected after this call returns. On a
	 * bridge whose chip select is a separate, fallible USB operation that
	 * is only safe while nothing else can start a transfer to a *different*
	 * slave -- which is precisely what SPI_LOCK_ON guarantees. Holding
	 * without locking would let the next caller select a second slave while
	 * the first is still asserted.
	 */
	if (OP_HAS_FLAG_(SPI_HOLD_ON_CS) && !OP_HAS_FLAG_(SPI_LOCK_ON)) {
		LOG_ERR("SPI_HOLD_ON_CS requires SPI_LOCK_ON on this controller: holding chip "
			"select without owning the bus would let another configuration select "
			"a second slave while the first remains asserted. Add SPI_LOCK_ON. "
			"Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (OP_HAS_FLAG_(SPI_CS_ACTIVE_HIGH)) {
		LOG_ERR("This SPI operation (SPI_CS_ACTIVE_HIGH) is not supported; express "
			"chip-select polarity with GPIO_ACTIVE_HIGH in cs-gpios instead. "
			"Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (OP_HAS_FLAG_(SPI_FRAME_FORMAT_TI)) {
		LOG_ERR("This SPI operation (SPI_FRAME_FORMAT_TI) is not supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	if (OP_HAS_FLAG_(SPI_LINES_MASK)) {
		LOG_ERR("This SPI operation (SPI_LINES_MASK) is not supported. Returning -ENOTSUP.");
		return -ENOTSUP;
	}

	ret = bufset_len_(tx_bufs, &tx_len, "TX");
	if (ret != 0) {
		return ret;
	}
	ret = bufset_len_(rx_bufs, &rx_len, "RX");
	if (ret != 0) {
		return ret;
	}

	/*
	 * The firmware endpoint is always full duplex, so a read-only transfer
	 * clocks zero-filled TX and a write-only transfer discards the returned
	 * RX. Both scratch buffers are therefore allocated in every direction.
	 */
	clock_len = MAX(tx_len, rx_len);
	if (clock_len == 0U) {
		return 0;
	}

	tx_flat = k_malloc(clock_len);
	if (tx_flat == NULL) {
		LOG_ERR("Failed to allocate %zu-byte SPI TX buffer. Returning -ENOMEM.", clock_len);
		return -ENOMEM;
	}
	flatten_tx_(tx_bufs, tx_flat, clock_len);

	rx_flat = k_malloc(clock_len);
	if (rx_flat == NULL) {
		LOG_ERR("Failed to allocate %zu-byte SPI RX buffer. Returning -ENOMEM.", clock_len);
		k_free(tx_flat);
		return -ENOMEM;
	}

	spi_context_lock(ctx, false, NULL, NULL, config);

	/*
	 * The latch is checked *after* acquiring the controller lock and before
	 * any set-config, chip-select edge or clocking. Checking it before the
	 * lock would race a preceding transfer that faults while this caller is
	 * still waiting on the semaphore.
	 */
	if (data->cs_fault) {
		LOG_ERR("Pico de Gallo SPI controller on serial-number \"%s\" is latched after an "
			"unacknowledged chip-select deassert on GPIO pin %u (originating "
			"errno=%d); no configuration, chip-select edge or clocking is issued. "
			"Call spi_release() with the retained configuration and have it "
			"deassert successfully to recover. Returning -EHOSTDOWN.",
			dev_config->serial_number, pdg_spi_cs_pin(ctx), data->cs_fault_errno);
		pdg_spi_unlock_defanged(ctx, true);
		k_free(rx_flat);
		k_free(tx_flat);
		return -EHOSTDOWN;
	}

	ctx->config = config;

	ret = pdg_spi_bottom_set_config(data->ctx, config->frequency,
					(config->operation & SPI_MODE_CPHA) != 0U,
					(config->operation & SPI_MODE_CPOL) != 0U);
	if (ret != 0) {
		LOG_ERR("Failed to configure the SPI bus at %u Hz on serial-number \"%s\" "
			"(CS GPIO pin %u): errno=%d, cleanup not attempted, fault latch not "
			"entered.", config->frequency, dev_config->serial_number,
			pdg_spi_cs_pin(ctx), ret);
		goto out;
	}

	ret = pdg_spi_cs_control_checked(ctx, true, false);
	if (ret != 0) {
		cleanup = pdg_spi_cs_control_checked(ctx, false, true);
		if (cleanup != 0) {
			if (!data->cs_fault) {
				data->cs_fault = true;
				data->cs_fault_errno = cleanup;
			}
			deassert_failed = true;
		}
		LOG_ERR("Chip-select assert failed on serial-number \"%s\" GPIO pin %u: "
			"errno=%d, cleanup errno=%d, fault latch %s. No clocks were issued.",
			dev_config->serial_number, pdg_spi_cs_pin(ctx), ret, cleanup,
			deassert_failed ? "entered" : "not entered");
		goto out;
	}

	ret = pdg_spi_bottom_transfer(data->ctx, tx_flat, rx_flat, clock_len);
	if (ret != 0) {
		cleanup = pdg_spi_cs_control_checked(ctx, false, true);
		if (cleanup != 0) {
			if (!data->cs_fault) {
				data->cs_fault = true;
				data->cs_fault_errno = cleanup;
			}
			deassert_failed = true;
		}
		LOG_ERR("SPI transfer of %zu bytes failed on serial-number \"%s\" GPIO pin %u: "
			"errno=%d, cleanup errno=%d, fault latch %s. RX is not committed.",
			clock_len, dev_config->serial_number, pdg_spi_cs_pin(ctx), ret, cleanup,
			deassert_failed ? "entered" : "not entered");
		if (deassert_failed) {
			ret = cleanup;
		}
		goto out;
	}

	if (OP_HAS_FLAG_(SPI_HOLD_ON_CS)) {
		/*
		 * A deliberate hold intentionally leaves the slave selected, so
		 * there is no deassert to serve as the commit barrier. The data
		 * is valid and the caller asked to stay selected under
		 * SPI_LOCK_ON, so RX must be committed here.
		 */
		commit_rx = true;
		goto out;
	}

	cleanup = pdg_spi_cs_control_checked(ctx, false, false);
	if (cleanup != 0) {
		if (!data->cs_fault) {
			data->cs_fault = true;
			data->cs_fault_errno = cleanup;
		}
		deassert_failed = true;
		ret = cleanup;
		LOG_ERR("SPI transfer of %zu bytes succeeded but the chip-select deassert on "
			"serial-number \"%s\" GPIO pin %u was unacknowledged: primary errno=0, "
			"cleanup errno=%d, fault latch entered. RX is not committed; the "
			"peripheral may remain selected.",
			clock_len, dev_config->serial_number, pdg_spi_cs_pin(ctx), cleanup);
		goto out;
	}

	commit_rx = true;

out:
	if (commit_rx) {
		unflatten_rx_(rx_bufs, rx_flat);
	}

	/*
	 * SPI_LOCK_ON retains bus ownership across a successful call, exactly as
	 * spi_context_release() does upstream; every failure releases it so an
	 * error can never strand the software lock.
	 */
	retain_lock = (ret == 0) && OP_HAS_FLAG_(SPI_LOCK_ON);
	if (!retain_lock) {
		pdg_spi_unlock_defanged(ctx, deassert_failed);
	}

	k_free(rx_flat);
	k_free(tx_flat);

	return ret;
}

static int pdg_spi_release(const struct device *dev, const struct spi_config *config)
{
	struct pdg_spi_data *data = dev->data;
	const struct pdg_spi_config *dev_config = dev->config;
	struct spi_context *ctx = &data->spi_ctx;
	int ret;

	if (data->ctx == NULL) {
		LOG_ERR("SPI bridge context is NULL; check device readiness. Returning -ENODEV.");
		return -ENODEV;
	}

	if (config == NULL) {
		LOG_ERR("SPI configuration is NULL. Returning -EINVAL.");
		return -EINVAL;
	}

	if (!spi_context_configured(ctx, config)) {
		LOG_ERR("spi_release() was called with a configuration this controller does not "
			"currently retain; a successful release leaves no retained "
			"configuration, so a second release is rejected. Returning -EINVAL.");
		return -EINVAL;
	}

	spi_context_lock(ctx, false, NULL, NULL, config);

	/* Recheck after acquire: the precheck above raced whoever held the lock. */
	if (!spi_context_configured(ctx, config)) {
		LOG_ERR("spi_release() lost the retained configuration while waiting for the "
			"controller lock. Returning -EINVAL.");
		pdg_spi_unlock_defanged(ctx, false);
		return -EINVAL;
	}

	ret = pdg_spi_cs_control_checked(ctx, false, true);
	if (ret != 0) {
		if (!data->cs_fault) {
			data->cs_fault = true;
			data->cs_fault_errno = ret;
		}
		LOG_ERR("spi_release() chip-select deassert on serial-number \"%s\" GPIO pin %u "
			"was unacknowledged: errno=%d, fault latch entered (originating "
			"errno=%d). Software ownership is released, but transfers remain "
			"blocked; retry spi_release() with this exact configuration, or "
			"reinitialize/power-cycle.",
			dev_config->serial_number, pdg_spi_cs_pin(ctx), ret,
			data->cs_fault_errno);
	} else {
		data->cs_fault = false;
		data->cs_fault_errno = 0;
	}

	pdg_spi_unlock_defanged(ctx, ret != 0);

	return ret;
}

static DEVICE_API(spi, pdg_spi_api) = {
	.transceive = pdg_spi_transceive,
	.release = pdg_spi_release,
};

static int pdg_spi_init(const struct device *dev)
{
	const struct pdg_spi_config *config = dev->config;
	struct pdg_spi_data *data = dev->data;
	struct spi_context *ctx = &data->spi_ctx;
	int ret;

	/*
	 * spi_context's lock/sync semaphores and its cs_gpios array are
	 * statically initialized by the SPI_CONTEXT_* macros below, and the
	 * fault latch is zero-initialized, so every device object that exists
	 * at all is usable before this function runs. Zephyr's spi_transceive()
	 * dispatches into the driver without checking readiness; the data->ctx
	 * == NULL guard at the top of pdg_spi_transceive() turns such a call on
	 * a failed device into -ENODEV.
	 */

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

	/*
	 * Local indexed equivalent of spi_context_cs_configure_all().
	 *
	 * Do not call the stock helper here. It reproduces exactly this loop --
	 * readiness check, then gpio_pin_configure_dt(GPIO_OUTPUT_INACTIVE), in
	 * ascending array order -- but returns only an errno and discards the
	 * failing iterator, so it cannot name the cs-gpios array index or the
	 * GPIO pin this driver's diagnostics are specified to report. Calling
	 * it and then re-probing to recover the index would issue duplicate,
	 * state-changing, unbounded USB round trips. The behaviour and ordering
	 * are identical; only the diagnostics differ.
	 *
	 * Readiness is checked before any configuration, so a priority
	 * inversion (this controller running before the GPIO child) fails
	 * loudly with -ENODEV having actuated no pin.
	 *
	 * There is deliberately no rollback on failure: init has no trustworthy
	 * record of the prior configuration, and another unbounded RPC could
	 * hang boot. Residue is documented in the module README.
	 */
	for (size_t idx = 0U; idx < ctx->num_cs_gpios; idx++) {
		const struct gpio_dt_spec *cs = &ctx->cs_gpios[idx];

		if (!device_is_ready(cs->port)) {
			LOG_ERR("%s: cs-gpios index %zu (GPIO pin %u on port %s, Pico de Gallo "
				"serial-number \"%s\") is not ready during phase "
				"\"readiness check\"; no chip-select pin was configured. This "
				"is an initialization priority inversion: "
				"CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY must be greater than "
				"CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY. Returning -ENODEV.",
				dev->name, idx, cs->pin, cs->port->name,
				config->serial_number);
			data->ctx = NULL;
			return -ENODEV;
		}

		ret = gpio_pin_configure_dt(cs, GPIO_OUTPUT_INACTIVE);
		if (ret < 0) {
			if (ret == -EBUSY) {
				LOG_ERR("%s: cs-gpios index %zu (GPIO pin %u, Pico de Gallo "
					"serial-number \"%s\") failed phase \"configure "
					"inactive output\" with errno=%d: a firmware GPIO "
					"event subscription owns this pin. Reset it explicitly "
					"with gallo_system_reset_subscriptions() after a strict "
					"open, then reinitialize; or power-cycle the board.",
					dev->name, idx, cs->pin, config->serial_number, ret);
			} else {
				LOG_ERR("%s: cs-gpios index %zu (GPIO pin %u, Pico de Gallo "
					"serial-number \"%s\") failed phase \"configure "
					"inactive output\" with errno=%d. Earlier entries are "
					"acknowledged inactive; this entry is indeterminate and "
					"later entries were never issued. No rollback is "
					"attempted.",
					dev->name, idx, cs->pin, config->serial_number, ret);
			}
			data->ctx = NULL;
			return ret;
		}
	}

	/*
	 * Give the lock semaphore its initial count. ctx->config is still NULL
	 * here -- nothing above assigns it -- so the defanged helper's stock
	 * call issues no chip-select edge, which is exactly what init wants.
	 */
	pdg_spi_unlock_defanged(ctx, false);

	LOG_INF("%s: ready on Pico de Gallo serial-number \"%s\" with %zu chip select(s).",
		dev->name, config->serial_number, ctx->num_cs_gpios);

	return 0;
}

#define PDG_SPI_INIT(inst)							\
	static struct pdg_spi_data pdg_spi_data_##inst = {			\
		SPI_CONTEXT_INIT_LOCK(pdg_spi_data_##inst, spi_ctx),		\
		SPI_CONTEXT_INIT_SYNC(pdg_spi_data_##inst, spi_ctx),		\
		SPI_CONTEXT_CS_GPIOS_INITIALIZE(DT_DRV_INST(inst), spi_ctx)	\
	};									\
										\
	static const struct pdg_spi_config pdg_spi_config_##inst = {		\
		.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst)),			\
		.serial_number = DT_PROP(DT_INST_PARENT(inst), serial_number),	\
	};									\
										\
	SPI_DEVICE_DT_INST_DEFINE(inst, pdg_spi_init, NULL,			\
				  &pdg_spi_data_##inst,				\
				  &pdg_spi_config_##inst,			\
				  POST_KERNEL,					\
				  CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY,	\
				  &pdg_spi_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_SPI_INIT)
