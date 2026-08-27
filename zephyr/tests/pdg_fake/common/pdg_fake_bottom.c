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

/*
 * The declaration sites of the symbols overridden below. Included rather than
 * hand-copied so a signature drift between production and this fake is a
 * compile error instead of undefined behaviour that links cleanly. Both headers
 * are FFI-free (stdint/stddef only), so including them here does not drag
 * pico_de_gallo.h in.
 */
#include "common_bottom.h"
#include "pdg_i2c_bottom.h"

#include "pdg_fake_bottom.h"

#define FAKE_MAX_PAYLOAD 4096

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

/* Per-call I2C recorders. Unlike open_count_latched, these ARE cleared by
 * pdg_fake_reset().
 */
static int i2c_write_count;
static int i2c_write_read_count;
static int i2c_have_last_write;
static uint16_t i2c_last_addr;
static uint8_t i2c_last_buf[FAKE_MAX_PAYLOAD];
static size_t i2c_last_len;
static int i2c_last_overflowed;

void pdg_fake_reset(void)
{
	/* open_count_latched is deliberately not cleared -- see above. */
	i2c_write_count = 0;
	i2c_write_read_count = 0;
	i2c_have_last_write = 0;
	i2c_last_addr = 0U;
	i2c_last_len = 0U;
	i2c_last_overflowed = 0;
	memset(i2c_last_buf, 0, sizeof(i2c_last_buf));
}

int pdg_fake_open_count(void)
{
	return open_count_latched;
}

int pdg_fake_i2c_write_count(void)
{
	return i2c_write_count;
}

int pdg_fake_i2c_write_read_count(void)
{
	return i2c_write_read_count;
}

int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen)
{
	if (i2c_have_last_write == 0 || i2c_last_overflowed || i2c_last_len > buflen) {
		return -1;
	}

	/* A NULL destination with bytes to hand back is a caller bug, not a
	 * zero-byte result. Refuse it rather than reporting a length nobody
	 * received.
	 */
	if (i2c_last_len > 0U && buf == NULL) {
		return -1;
	}

	if (addr != NULL) {
		*addr = i2c_last_addr;
	}

	if (i2c_last_len > 0U) {
		memcpy(buf, i2c_last_buf, i2c_last_len);
	}

	return (int)i2c_last_len;
}

/*
 * Shared payload capture for both write and write_read. The two call counters
 * stay separate: a test that must prove the driver issued a PLAIN write
 * cannot do so from a counter that a write_read also bumps.
 */
static void record_tx_(uint16_t addr, const uint8_t *buf, size_t len)
{
	i2c_have_last_write = 1;
	i2c_last_addr = addr;

	/*
	 * Record the overflow rather than truncating silently: a test that asks
	 * for a payload we could not store must fail, not pass on a prefix.
	 */
	i2c_last_overflowed = (len > FAKE_MAX_PAYLOAD);
	if (!i2c_last_overflowed) {
		i2c_last_len = len;
		if (len > 0U && buf != NULL) {
			memcpy(i2c_last_buf, buf, len);
		}
	}
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
 * Two jobs. First, correctness: pdg_i2c_init() calls
 * pdg_i2c_bottom_set_config() unconditionally (pdg_i2c.c:804) and treats a
 * negative return as fatal, nulling data->ctx. Without an override the
 * production shim would pass &fake_ctx_token to gallo_i2c_set_config() as a
 * PicoDeGallo *, and the Rust FFI would dereference an invalid opaque pointer.
 * Second, observation: the write path records what the driver asked the bus to
 * do, which is what backs pdg_fake_i2c_*().
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

	i2c_write_count++;
	record_tx_(addr, buf, len);

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

	/*
	 * Captures the tx half into the same last-write buffer as a plain
	 * write, but bumps its OWN counter. Sharing the counter would let a
	 * driver that wrongly issued a write_read satisfy a test written to
	 * prove it issued one plain write.
	 */
	i2c_write_read_count++;
	record_tx_(addr, tx, txlen);

	if (rx != NULL && rxlen > 0) {
		memset(rx, 0xA5, rxlen);
	}

	return 0;
}
