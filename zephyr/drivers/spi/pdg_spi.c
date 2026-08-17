/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr SPI controller driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context. Note that
 * "embedded" here just means the embedded part of `native-sim`,
 * not something that actually gets flashed to hardware or anything.
 * Anyway, this file translates Zephyr SPI API transactions into the small 
 * host-context shim declared in pdg_spi_bottom.h, which forwards them to the Pico de Gallo C FFI.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_spi

#include <zephyr/device.h>
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
 * option would turn any async use into a jump through a NULL function
 * pointer, which on native_sim is a segfault rather than an error return.
 * CONFIG_SPI_RTIO is also selected transitively by CONFIG_SENSOR_ASYNC_API,
 * so an application can reach it without asking.
 *
 * This is deliberately a BUILD_ASSERT rather than a Kconfig "depends on
 * !SPI_ASYNC". Both SPI_ASYNC and SPI_RTIO depend on SPI, which this driver
 * selects, so expressing the constraint in Kconfig produces a dependency
 * loop and refuses to parse. A BUILD_ASSERT also fails loudly with the
 * reason, where "depends on" would silently drop the driver and surface as
 * an unresolved __device_dts_ord_N at link time.
 */
BUILD_ASSERT(!IS_ENABLED(CONFIG_SPI_ASYNC),
	     "The Pico de Gallo SPI driver does not implement transceive_async(); "
	     "disable CONFIG_SPI_ASYNC.");
BUILD_ASSERT(!IS_ENABLED(CONFIG_SPI_RTIO),
	     "The Pico de Gallo SPI driver does not implement iodev_submit(); "
	     "disable CONFIG_SPI_RTIO (CONFIG_SENSOR_ASYNC_API selects it).");

LOG_MODULE_REGISTER(spi_pico_de_gallo, CONFIG_SPI_LOG_LEVEL);

// Firmware single-transfer limit (pico_de_gallo_internal::MAX_TRANSFER_SIZE).
#define PDG_SPI_MAX_BUFFER 4096U

struct pdg_spi_config {
    const char *serial;
    const uint8_t *cs_indices;
    size_t cs_indices_len;
};

struct pdg_spi_data {
	void *ctx;
	struct k_mutex lock;
	uint8_t num_gpios;
};

// helper to calculate the total byte length of every buffer in a `spi_buf_set`
// used when flattening Zephyr `spi_but_set`s into a normal contiguous buffer to pass into the pico-de-gallo ffi
// (the `direction` parameter isn't part of the actual calculation, it is just for debugging verbosity)
static int bufset_len_(const struct spi_buf_set *bufs, size_t *total_len, const char* direction)
{
    *total_len = 0U;

    if (bufs == NULL) {
        return 0;
    }
    if (bufs->count != 0U && bufs->buffers == NULL) {
        LOG_ERR("SPI %s buffer set has count %zu but no buffer array. Returning -EINVAL.", direction, bufs->count);
        return -EINVAL;
    }

    for (size_t i = 0U; i < bufs->count; ++i) {
        if (bufs->buffers[i].len > PDG_SPI_MAX_BUFFER - *total_len) {
            LOG_WRN("SPI %s buffers exceed maximum transfer size of %u bytes. Returning -EMSGSIZE", direction, PDG_SPI_MAX_BUFFER);
            return -EMSGSIZE;
        }
        *total_len += bufs->buffers[i].len;
    }

    return 0;
}

// helper that flattens a set of `spi_buf_set`s into a single buffer
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

// helper that takes in a normal flat buffer from pico-de-gallo and organizes it into the multiple
// buffer sets provided by zephyr
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

// helper macro for `pdg_spi_transceieve()`'s op flag checks.
#define OP_HAS_FLAG_(flag) (((config)->operation & (flag)) != 0U)

static int pdg_spi_transceive(const struct device *dev, const struct spi_config *config, const struct spi_buf_set *tx_bufs, const struct spi_buf_set *rx_bufs)
{
    struct pdg_spi_data *data = dev->data;
    const struct pdg_spi_config *dev_config = dev->config;
    struct pdg_spi_batch_op ops[3] = {0};
    struct pdg_spi_batch_op *transfer_op;
    uint8_t *tx_flat = NULL;
    uint8_t *rx_flat = NULL;
    uint8_t cs_index;
    size_t tx_len;
    size_t rx_len;
    size_t clock_len;
    size_t ops_count = 0U;
    size_t out_len = 0U;
    int ret;

    /*
     * Zephyr's spi_transceive() does not check device readiness, so an
     * application that skips device_is_ready() reaches here on a device whose
     * init failed. Guard first: every later diagnostic reads data->num_gpios.
     */
    if (data->ctx == NULL) {
        LOG_ERR("SPI bridge context is NULL before chip-select validation; slave selector, cs-gpio-indices length, mapped GPIO index, and firmware GPIO count are unavailable. Check device readiness and the controller's cs-gpio-indices property. Returning -ENODEV.");
        return -ENODEV;
    }

    if (config == NULL) {
        LOG_ERR("SPI configuration is NULL. Returning -EINVAL.");
        return -EINVAL;
    }

    if(SPI_OP_MODE_GET(config->operation) != SPI_OP_MODE_MASTER) {
        LOG_ERR("The configured SPI peripheral mode is not supported. Only SPI_OP_MODE_MASTER is supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(SPI_WORD_SIZE_GET(config->operation) != 8U) {
        LOG_ERR("The configured SPI word size is not supported. Only 8-bit SPI words are supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_TRANSFER_LSB)) {
        LOG_ERR("This SPI operation (SPI_TRANSFER_LSB) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_MODE_LOOP)) {
        LOG_ERR("This SPI operation (SPI_MODE_LOOP) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_HALF_DUPLEX)) {
        LOG_ERR("This SPI operation (SPI_HALF_DUPLEX) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_HOLD_ON_CS)) {
        LOG_ERR("This SPI operation (SPI_HOLD_ON_CS) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_LOCK_ON)) {
        LOG_ERR("This SPI operation (SPI_LOCK_ON) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_CS_ACTIVE_HIGH)) {
        LOG_ERR("This SPI operation (SPI_CS_ACTIVE_HIGH) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_FRAME_FORMAT_TI)) {
        LOG_ERR("This SPI operation (SPI_FRAME_FORMAT_TI) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    if(OP_HAS_FLAG_(SPI_LINES_MASK)) {
        LOG_ERR("This SPI operation (SPI_LINES_MASK) is not supported. Returning -ENOTSUP.");
        return -ENOTSUP;
    }

    /*
     * The chip-select mapping is validated before the buffers because stacked
     * drivers hide the reason: jedec,spi-nor collapses any transfer failure to
     * -ENODEV (drivers/flash/spi_nor.c), so these LOG_ERR lines are the only
     * authoritative diagnosis of a devicetree error.
     */
    if (spi_cs_is_gpio(config)) {
        LOG_ERR("SPI slave selector %u uses Zephyr GPIO-controlled CS; cs-gpio-indices length is %zu, mapped GPIO index is unavailable, firmware reported %u GPIOs. Remove cs-gpios and configure cs-gpio-indices on the Pico de Gallo SPI controller. Returning -ENOTSUP.",
                (unsigned int)config->slave,
                dev_config->cs_indices_len,
                (unsigned int)data->num_gpios);
        return -ENOTSUP;
    }

    if (dev_config->cs_indices_len == 0U) {
        LOG_ERR("SPI slave selector %u cannot be mapped: cs-gpio-indices length is 0 (property absent), mapped GPIO index is unavailable, firmware reported %u GPIOs. Add cs-gpio-indices to the Pico de Gallo SPI controller. Returning -EINVAL.",
                (unsigned int)config->slave,
                (unsigned int)data->num_gpios);
        return -EINVAL;
    }

    if (config->slave >= dev_config->cs_indices_len) {
        LOG_ERR("SPI slave selector %u is outside cs-gpio-indices length %zu; mapped GPIO index is unavailable, firmware reported %u GPIOs. Extend or correct cs-gpio-indices on the Pico de Gallo SPI controller. Returning -EINVAL.",
                (unsigned int)config->slave,
                dev_config->cs_indices_len,
                (unsigned int)data->num_gpios);
        return -EINVAL;
    }

    cs_index = dev_config->cs_indices[config->slave];

    if (data->num_gpios == 0U) {
        LOG_ERR("SPI slave selector %u maps through cs-gpio-indices length %zu to GPIO index %u, but firmware successfully reported zero GPIOs. Correct the firmware/device pairing or cs-gpio-indices; no chip select is available. Returning -ENODEV.",
                (unsigned int)config->slave,
                dev_config->cs_indices_len,
                (unsigned int)cs_index);
        return -ENODEV;
    }

    if (cs_index >= data->num_gpios) {
        LOG_ERR("SPI slave selector %u maps through cs-gpio-indices length %zu to GPIO index %u, but firmware reported %u GPIOs. Correct cs-gpio-indices on the Pico de Gallo SPI controller. Returning -EINVAL.",
                (unsigned int)config->slave,
                dev_config->cs_indices_len,
                (unsigned int)cs_index,
                (unsigned int)data->num_gpios);
        return -EINVAL;
    }

    ret = bufset_len_(tx_bufs, &tx_len, "TX");
    if (ret != 0) {
        return ret;
    }
    ret = bufset_len_(rx_bufs, &rx_len, "RX");
    if (ret != 0) {
        return ret;
    }

    clock_len = MAX(tx_len, rx_len);
    if (clock_len == 0U) {
        return 0;
    }

    if (tx_len != 0U) {
        tx_flat = k_malloc(clock_len);
        if (tx_flat == NULL) {
            LOG_ERR("Failed to allocate %zu-byte SPI TX buffer. Returning -ENOMEM.", clock_len);
            return -ENOMEM;
        }
        flatten_tx_(tx_bufs, tx_flat, clock_len);
    }
    if (rx_len != 0U) {
        rx_flat = k_malloc(clock_len);
        if (rx_flat == NULL) {
            LOG_ERR("Failed to allocate %zu-byte SPI RX buffer. Returning -ENOMEM.", clock_len);
            k_free(tx_flat);
            return -ENOMEM;
        }
    }

    if (config->cs.setup_ns != 0U) {
        ops[ops_count].tag = PDG_SPI_BATCH_DELAY_NS;
        ops[ops_count].delay_ns = config->cs.setup_ns;
        ops_count++;
    }

    transfer_op = &ops[ops_count++];
    if (tx_len == 0U) {
        transfer_op->tag = PDG_SPI_BATCH_READ;
        transfer_op->read_len = (uint16_t)clock_len;
    } else if (rx_len == 0U) {
        transfer_op->tag = PDG_SPI_BATCH_WRITE;
        transfer_op->data = tx_flat;
        transfer_op->data_len = clock_len;
    } else {
        transfer_op->tag = PDG_SPI_BATCH_TRANSFER;
        transfer_op->data = tx_flat;
        transfer_op->data_len = clock_len;
    }

    if (config->cs.hold_ns != 0U) {
        ops[ops_count].tag = PDG_SPI_BATCH_DELAY_NS;
        ops[ops_count].delay_ns = config->cs.hold_ns;
        ops_count++;
    }

    k_mutex_lock(&data->lock, K_FOREVER);

    ret = pdg_spi_bottom_set_config(data->ctx, config->frequency, (config->operation & SPI_MODE_CPHA) != 0U, (config->operation & SPI_MODE_CPOL) != 0U);

    if (ret != 0) {
        LOG_ERR("Failed to configure SPI bus at %u Hz: Errno=%d", config->frequency, ret);
    } else {
        ret = pdg_spi_bottom_batch(data->ctx, (uint8_t)cs_index, ops, ops_count, rx_flat, rx_len == 0U ? 0U : clock_len, &out_len, NULL);
        if (ret != 0) {
            LOG_ERR("SPI transaction for slave selector %u using firmware GPIO index %u failed: Errno=%d", config->slave, cs_index, ret);
        }
    }

    k_mutex_unlock(&data->lock);

    if (ret == 0 && rx_len != 0U) {
        if (out_len != clock_len) {
            LOG_ERR("SPI response length mismatch: expected %zu bytes, received %zu. Returning -EPROTO.", clock_len, out_len);
            ret = -EPROTO;
        } else {
            unflatten_rx_(rx_bufs, rx_flat);
        }
    }

    k_free(rx_flat);
    k_free(tx_flat);
    return ret;
}

static int pdg_spi_release(const struct device *dev, const struct spi_config *config)
{
    ARG_UNUSED(dev);
    ARG_UNUSED(config);

    return 0;
}

static DEVICE_API(spi, pdg_spi_api) = {
    .transceive = pdg_spi_transceive,
    .release = pdg_spi_release,
};

static int pdg_spi_init(const struct device *dev)
{
	const struct pdg_spi_config *config = dev->config;
	struct pdg_spi_data *data = dev->data;
	int ret;

	k_mutex_init(&data->lock);

    LOG_INF("Opening Pico de Gallo SPI bridge");
	data->ctx = pdg_spi_bottom_open(config->serial);
    if (data->ctx == NULL) {
        if (config->serial != NULL) {
            LOG_ERR("Failed to open Pico de Gallo bridge with serial number %s. Returning -ENODEV.", config->serial);
        } else {
            LOG_ERR("Failed to open a Pico de Gallo bridge. Returning -ENODEV.");
        }

        return -ENODEV;
    }

	/*
	 * pdg_spi_bottom_open() uses gallo_init_strict(), whose successful
	 * validation populates the shared num_gpios cache. This call is therefore
	 * a guaranteed warm-cache read with no USB traffic. The failure branch is
	 * defence-in-depth for an invariant violation, not an expected timeout
	 * path.
	 */
	ret = pdg_spi_bottom_num_gpios(data->ctx, &data->num_gpios);
	if (ret != 0) {
		LOG_ERR("Failed to read the cached firmware GPIO count from the validated Pico de Gallo bridge: Errno=%d. Closing the bridge; the SPI device will remain not ready.",
			ret);
		pdg_spi_bottom_close(data->ctx);
		data->ctx = NULL;
		return ret;
	}

	LOG_INF("Pico de Gallo SPI bridge ready");

	return 0;
}

/*
 * The one-element { 0U } sentinel avoids a non-standard zero-length array when
 * cs-gpio-indices is absent. cs_indices_len stays 0, which is the sole
 * "property absent" signal (min-len: 1 in the binding makes a generated length
 * of 0 unambiguous).
 *
 * Because the sentinel makes cs_indices[0] == 0, the cs_indices_len == 0 guard
 * in pdg_spi_transceive() is load-bearing for *safety*, not merely for its
 * message: removing or reordering it would silently reproduce issue #104 by
 * driving firmware GPIO 0 as a chip select.
 */
#define PDG_SPI_INIT(inst)                                                   \
	static const uint8_t pdg_spi_cs_indices_##inst[] =                    \
		COND_CODE_1(DT_INST_NODE_HAS_PROP(inst, cs_gpio_indices),       \
			    (DT_INST_PROP(inst, cs_gpio_indices)),              \
			    ({ 0U }));                                          \
	                                                                         \
	static struct pdg_spi_data pdg_spi_data_##inst;                        \
	                                                                         \
	static const struct pdg_spi_config pdg_spi_config_##inst = {           \
		.serial = DT_INST_PROP_OR(inst, serial_number, NULL),             \
		.cs_indices = pdg_spi_cs_indices_##inst,                           \
		.cs_indices_len = DT_INST_PROP_LEN_OR(inst, cs_gpio_indices, 0),  \
	};                                                                       \
	                                                                         \
	SPI_DEVICE_DT_INST_DEFINE(inst, pdg_spi_init, NULL,                     \
				  &pdg_spi_data_##inst,                       \
				  &pdg_spi_config_##inst,                     \
				  POST_KERNEL, CONFIG_SPI_INIT_PRIORITY,       \
				  &pdg_spi_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_SPI_INIT)
