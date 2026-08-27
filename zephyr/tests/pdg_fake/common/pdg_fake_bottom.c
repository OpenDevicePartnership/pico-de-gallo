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

/* Any non-NULL value. pdg_mfd_init() only checks for NULL, and no code path in
 * the test ever dereferences it, because every consumer of the context is also
 * overridden here.
 */
static int fake_ctx_token;

static int open_count;

void pdg_fake_reset(void)
{
	open_count = 0;
}

int pdg_fake_open_count(void)
{
	return open_count;
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
	open_count++;
	return &fake_ctx_token;
}

/* Strong override of the weak definition in zephyr/drivers/common/common.c. */
void pdg_common_bottom_close(void *ctx)
{
	(void)ctx;
}
