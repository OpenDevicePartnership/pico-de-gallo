/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Recording fake for the host-context bottom layer.
 *
 * Compiled into native_simulator with the host C library. It provides STRONG
 * definitions of symbols the production bottom files define as weak, so the
 * linker prefers these and no production CMakeLists.txt needs a test-only
 * conditional.
 *
 * It deliberately does NOT link pico_de_gallo.h. Nothing here reaches the FFI,
 * which is the whole point: pdg_mfd.c calls pdg_common_bottom_open() directly,
 * so overriding it means gallo_init_strict() is never entered.
 */

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "pdg_fake_bottom.h"

/*
 * Any non-NULL value. pdg_mfd_init() only checks for NULL, and this file
 * overrides every bottom entry point that is handed the context on the paths
 * this suite exercises: pdg_common_bottom_open/_close, and the four I2C
 * operations (set_config, write, read, write_read). Each override ignores the
 * pointer rather than dereferencing it.
 *
 * That list is exhaustive for the I2C driver only. A future suite that enables
 * the GPIO or SPI children must also override their bottom entry points, or
 * this token reaches the real FFI as a PicoDeGallo * and is dereferenced.
 */
static int fake_ctx_token;

/*
 * Latched. pdg_fake_reset() deliberately does not clear this: the open happens
 * during POST_KERNEL device init, before any ztest setup hook, so clearing it
 * would discard the very event the weak-override gate asserts on. See the
 * invariant documented on pdg_fake_reset() in pdg_fake_bottom.h.
 */
static int open_count_latched;

void pdg_fake_reset(void)
{
	/*
	 * Nothing to clear yet. Task 2 adds the per-call I2C recorders and
	 * resets them here. open_count_latched is never cleared -- see above.
	 */
}

int pdg_fake_open_count(void)
{
	return open_count_latched;
}

int pdg_fake_i2c_write_count(void)
{
	return 0;
}

int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen)
{
	(void)addr;
	(void)buf;
	(void)buflen;
	return -1;
}

/* Strong override of the weak definition in zephyr/drivers/common/common.c. */
void *pdg_common_bottom_open(const char *serial)
{
	(void)serial;
	open_count_latched++;
	return &fake_ctx_token;
}

/* Strong override of the weak definition in zephyr/drivers/common/common.c. */
void pdg_common_bottom_close(void *ctx)
{
	(void)ctx;
}

/*
 * Strong overrides of the four weak definitions in
 * zephyr/drivers/i2c/pdg_i2c_bottom.c.
 *
 * These exist in Task 1 for correctness, not for observation. pdg_i2c_init()
 * calls pdg_i2c_bottom_set_config() unconditionally (pdg_i2c.c:804) and treats
 * a negative return as fatal, nulling data->ctx. Without this override the
 * production shim would pass &fake_ctx_token to gallo_i2c_set_config() as a
 * PicoDeGallo *, and the Rust FFI would dereference an invalid opaque pointer
 * -- so the weak-override gate could fail even when weak/strong resolution
 * works perfectly, which is exactly the confound the gate must not have.
 *
 * They are therefore minimal no-ops returning success. Task 2 replaces these
 * bodies with the recording implementations that back pdg_fake_i2c_*().
 */
int pdg_i2c_bottom_set_config(void *ctx, uint8_t frequency)
{
	(void)ctx;
	(void)frequency;
	return 0;
}

int pdg_i2c_bottom_write(void *ctx, uint16_t addr, const uint8_t *buf, size_t len)
{
	(void)ctx;
	(void)addr;
	(void)buf;
	(void)len;
	return 0;
}

/*
 * buf is filled with 0xA5 rather than left untouched so a later test can tell
 * a completed fake read from uninitialised caller memory. Same for the rx half
 * of write_read below.
 */
int pdg_i2c_bottom_read(void *ctx, uint16_t addr, uint8_t *buf, size_t len)
{
	(void)ctx;
	(void)addr;

	if (buf != NULL && len > 0) {
		memset(buf, 0xA5, len);
	}

	return 0;
}

int pdg_i2c_bottom_write_read(void *ctx, uint16_t addr, const uint8_t *tx, size_t txlen,
			      uint8_t *rx, size_t rxlen)
{
	(void)ctx;
	(void)addr;
	(void)tx;
	(void)txlen;

	if (rx != NULL && rxlen > 0) {
		memset(rx, 0xA5, rxlen);
	}

	return 0;
}
