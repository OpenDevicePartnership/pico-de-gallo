/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr MFD parent driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context. Note that "embedded" here
 * just means the embedded part of `native-sim`, not something that actually
 * gets flashed to hardware. One enabled node represents one physical
 * USB-attached board: it owns a single reference to the shared host-context
 * connection registry and exposes that borrowed context to child drivers.
 *
 * It must therefore never include the host-only pico_de_gallo.h, nor common.h
 * which pulls it in; the FFI-free common_bottom.h is the only common-layer
 * header it may see.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo

#include <errno.h>

#include <zephyr/device.h>
#include <zephyr/logging/log.h>

#include "common_bottom.h"
#include "pdg_mfd.h"

LOG_MODULE_REGISTER(mfd_pico_de_gallo, CONFIG_MFD_PICO_DE_GALLO_LOG_LEVEL);

/*
 * Two enabled parents that both omit serial-number normalize to the same ""
 * selector, so the second registry lookup silently returns the first board's
 * handle and two logical devices would drive one board. Reject that image at
 * build time. Presence is asserted, not uniqueness: distinct boards still
 * require distinct values by binding contract.
 *
 * The per-instance macro deliberately emits a trailing `&&`; the final `1`
 * completes the constant expression. Zero or one enabled parent needs no
 * serial at all.
 */
#define PDG_MFD_INST_HAS_SERIAL(inst) \
	DT_INST_NODE_HAS_PROP(inst, serial_number) &&

BUILD_ASSERT(
	(DT_NUM_INST_STATUS_OKAY(DT_DRV_COMPAT) <= 1) ||
	(DT_INST_FOREACH_STATUS_OKAY(PDG_MFD_INST_HAS_SERIAL) 1),
	"Multiple enabled odp,pico-de-gallo parents require serial-number on every parent");

struct pdg_mfd_config {
	const char *serial;
};

struct pdg_mfd_data {
	void *ctx;
};

void *pdg_mfd_ctx(const struct device *dev)
{
	const struct pdg_mfd_data *data;

	if (dev == NULL) {
		return NULL;
	}

	data = dev->data;

	return data->ctx;
}

static int pdg_mfd_init(const struct device *dev)
{
	const struct pdg_mfd_config *config = dev->config;
	struct pdg_mfd_data *data = dev->data;

	data->ctx = pdg_common_bottom_open(config->serial);
	if (data->ctx == NULL) {
		if (config->serial != NULL) {
			LOG_ERR("%s: failed to open a Pico de Gallo bridge "
				"(explicit selector \"%s\"). Returning -ENODEV.",
				dev->name, config->serial);
		} else {
			LOG_ERR("%s: failed to open a Pico de Gallo bridge "
				"(default selector). Returning -ENODEV.",
				dev->name);
		}
		return -ENODEV;
	}

	return 0;
}

#define PDG_MFD_INIT(inst)							\
	static struct pdg_mfd_data pdg_mfd_data_##inst;				\
										\
	static const struct pdg_mfd_config pdg_mfd_config_##inst = {		\
		.serial = DT_INST_PROP_OR(inst, serial_number, NULL),		\
	};									\
										\
	DEVICE_DT_INST_DEFINE(inst, pdg_mfd_init, NULL,				\
			      &pdg_mfd_data_##inst,				\
			      &pdg_mfd_config_##inst, POST_KERNEL,		\
			      CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY, NULL);

DT_INST_FOREACH_STATUS_OKAY(PDG_MFD_INIT)
