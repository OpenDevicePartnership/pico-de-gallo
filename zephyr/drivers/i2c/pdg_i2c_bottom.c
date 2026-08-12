/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the Pico de Gallo I2C driver.
 *
 * This file is compiled into the native simulator runner with the host C
 * library and links against the Pico de Gallo FFI shared object. It must not
 * include any Zephyr headers.
 */

#include <errno.h>
#include <stddef.h>
#include <stdint.h>

#include "pico_de_gallo.h"
#include "common.h"
#include "pdg_i2c_bottom.h"

void *pdg_i2c_bottom_open(const char *serial)
{
	return pdg_common_bottom_open(serial);
}

void pdg_i2c_bottom_close(void *ctx)
{
	pdg_common_bottom_close(ctx);
}

int pdg_i2c_bottom_set_config(void *ctx, uint8_t frequency)
{
	return pdg_common_status_to_errno(gallo_i2c_set_config((const struct PicoDeGallo *)ctx, frequency));
}

int pdg_i2c_bottom_write(void *ctx, uint16_t addr, const uint8_t *buf, size_t len)
{
	return pdg_common_status_to_errno(gallo_i2c_write((const struct PicoDeGallo *)ctx, (uint8_t)addr, buf, len));
}

int pdg_i2c_bottom_read(void *ctx, uint16_t addr, uint8_t *buf, size_t len)
{
	return pdg_common_status_to_errno(gallo_i2c_read((const struct PicoDeGallo *)ctx, (uint8_t)addr, buf, len));
}

int pdg_i2c_bottom_write_read(void *ctx, uint16_t addr, const uint8_t *tx, size_t txlen, uint8_t *rx, size_t rxlen)
{
	return pdg_common_status_to_errno(gallo_i2c_write_read((const struct PicoDeGallo *)ctx, (uint8_t)addr, tx, txlen, rx, rxlen));
}
