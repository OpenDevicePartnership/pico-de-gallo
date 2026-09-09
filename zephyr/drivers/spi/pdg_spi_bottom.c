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

/*
 * PDG_SPI_MAX_BUFFER is declared as a literal in pdg_spi_bottom.h so the
 * embedded-context half of the driver can see it without pulling in any FFI
 * header. This is the one translation unit where both are visible, so it is
 * where the two are tied together: drift becomes a build failure rather than
 * a driver that silently refuses the wrong sizes.
 *
 * The same technique pins the mirrored wire-enum discriminants in
 * tests/pdg_mfd_m5/common/m5_bottom.c. Issue #158.
 */
_Static_assert(PDG_SPI_MAX_BUFFER == GALLO_MAX_RESPONSE_PAYLOAD,
	       "PDG_SPI_MAX_BUFFER must track GALLO_MAX_RESPONSE_PAYLOAD: every Zephyr "
	       "SPI operation is one full-duplex gallo_spi_transfer, so the response "
	       "frame is the binding limit");

int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase,
		bool polarity)
{
	const struct PicoDeGallo *pdg = ctx;
	int ret;

	ret = gallo_spi_set_config(pdg, frequency, phase, polarity);

	return pdg_common_status_to_errno(ret);
}

int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf,
			    uint8_t *read_buf, size_t len)
{
	const struct PicoDeGallo *pdg = ctx;
	int ret;

	ret = gallo_spi_transfer(pdg, write_buf, read_buf, len);

	return pdg_common_status_to_errno(ret);
}
