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

int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity)
{
	return pdg_common_status_to_errno(gallo_spi_set_config((const struct PicoDeGallo *)ctx, frequency, phase, polarity));
}

int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf,
			    uint8_t *read_buf, size_t len)
{
	return pdg_common_status_to_errno(gallo_spi_transfer(
		(const struct PicoDeGallo *)ctx, write_buf, read_buf, len));
}
