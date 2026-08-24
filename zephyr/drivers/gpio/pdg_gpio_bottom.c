/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the Pico de Gallo GPIO driver.
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
#include "pdg_gpio_bottom.h"

/*
 * GPIO-local ecomm normalisation: map -ECOMM to -EIO.
 *
 * Zephyr's GPIO API contract for an off-SoC controller enumerates -EIO for a
 * transport failure; -ECOMM is not one of its documented returns. The shared
 * mapper cannot make this conversion for us, and must not be changed to do so,
 * because it is also used by the I2C and SPI drivers.
 *
 * The premise this normalization rests on, written down here because it is a
 * human-review obligation that no grep in this directory can observe:
 *
 *   1. pdg_common_status_to_errno() maps BOTH CommsFailed AND OneWireNoPresence
 *      to -ECOMM (see common.c:31-32). Once collapsed, the two are
 *      indistinguishable here.
 *   2. The GPIO endpoints reachable through this file surface only the statuses
 *      produced by gpio_error_to_status() plus the transport CommsFailed. The
 *      1-Wire OneWireNoPresence status is not reachable through
 *      gallo_gpio_get/put/set_config or gallo_num_gpios, so every -ECOMM that
 *      can arrive here today originated from CommsFailed.
 *
 * If either gpio_error_to_status()'s status set or the common.c:31-32 collapse
 * changes, premise 2 must be re-established before this normalization stays
 * valid. Everything other than -ECOMM is passed through unchanged.
 */
static int pdg_gpio_normalize_ecomm(int ret)
{
	if (ret == -ECOMM) {
		return -EIO;
	}

	return ret;
}

int pdg_gpio_bottom_get(void *ctx, uint8_t pin, bool *state)
{
	return pdg_gpio_normalize_ecomm(pdg_common_status_to_errno(
		gallo_gpio_get((const struct PicoDeGallo *)ctx, pin, state)));
}

int pdg_gpio_bottom_put(void *ctx, uint8_t pin, bool state)
{
	return pdg_gpio_normalize_ecomm(pdg_common_status_to_errno(
		gallo_gpio_put((const struct PicoDeGallo *)ctx, pin, state)));
}

int pdg_gpio_bottom_set_config(void *ctx, uint8_t pin,
			       uint8_t direction, uint8_t pull)
{
	return pdg_gpio_normalize_ecomm(pdg_common_status_to_errno(
		gallo_gpio_set_config((const struct PicoDeGallo *)ctx, pin,
				      direction, pull)));
}

int pdg_gpio_bottom_num_gpios(void *ctx, uint8_t *out_num_gpios)
{
	return pdg_gpio_normalize_ecomm(pdg_common_status_to_errno(
		gallo_num_gpios((const struct PicoDeGallo *)ctx, out_num_gpios)));
}
