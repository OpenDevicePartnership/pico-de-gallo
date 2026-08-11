/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the Pico de Gallo SPI driver.
 *
 * This file is compiled into the native simulator runner with the host C
 * library and links against the Pico de Gallo FFI shared object. It must not
 * include any Zephyr headers.
 */

#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "pico_de_gallo.h"
#include "common.h"
#include "pdg_spi_bottom.h"

void *pdg_spi_bottom_open(const char *serial)
{ 
	return pdg_common_bottom_open(serial);
}

void pdg_spi_bottom_close(void *ctx) 
{
	pdg_common_bottom_close(ctx);
}

int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity)
{
	return pdg_common_status_to_errno(gallo_spi_set_config((const struct PicoDeGallo *)ctx, frequency, phase, polarity));
}

int pdg_spi_bottom_batch(void *ctx, uint8_t cs_pin, const struct pdg_spi_batch_op *ops, size_t ops_count, uint8_t *out_buf, size_t out_capacity, size_t *out_len, uint16_t *out_failed_op)
{
	GalloSpiBatchOp gallo_ops[PDG_SPI_MAX_BATCH_OPS];

	if ((ops == NULL && ops_count != 0U) || ops_count > PDG_SPI_MAX_BATCH_OPS) {
		return -EINVAL;
	}

	for (size_t i = 0U; i < ops_count; ++i) {
		gallo_ops[i].tag = ops[i].tag;
		gallo_ops[i].read_len = ops[i].read_len;
		gallo_ops[i].data = ops[i].data;
		gallo_ops[i].data_len = ops[i].data_len;
		gallo_ops[i].delay_ns = ops[i].delay_ns;
	}

	return pdg_common_status_to_errno(gallo_spi_batch(
		(const struct PicoDeGallo *)ctx, cs_pin,
		ops_count == 0U ? NULL : gallo_ops, ops_count,
		out_buf, out_capacity, out_len, out_failed_op));
}
