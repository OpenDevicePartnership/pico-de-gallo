/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context Zephyr I2C driver and the
 * host-context shim that calls the Pico de Gallo C FFI.
 *
 * Only basic C types may appear here so that the embedded side never needs to
 * include the host-only pico_de_gallo.h header. All functions return 0 on
 * success or a negative POSIX errno value on failure.
 */

#ifndef PDG_I2C_BOTTOM_H
#define PDG_I2C_BOTTOM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Open a Pico de Gallo bridge. serial may be NULL/empty to pick the first one.
 * Returns an opaque context pointer, or NULL if no matching device is reachable
 * or the firmware fails validation.
 */
void *pdg_i2c_bottom_open(const char *serial);

/* Release a context previously returned by pdg_i2c_bottom_open(). */
void pdg_i2c_bottom_close(void *ctx);

/* Set the bus frequency: 0 = Standard, 1 = Fast, 2 = Fast+. */
int pdg_i2c_bottom_set_config(void *ctx, uint8_t frequency);

int pdg_i2c_bottom_write(void *ctx, uint16_t addr, const uint8_t *buf,
		size_t len);

int pdg_i2c_bottom_read(void *ctx, uint16_t addr, uint8_t *buf, size_t len);

int pdg_i2c_bottom_write_read(void *ctx, uint16_t addr, const uint8_t *tx,
		size_t txlen, uint8_t *rx, size_t rxlen);

#ifdef __cplusplus
}
#endif

#endif /* PDG_I2C_BOTTOM_H */
