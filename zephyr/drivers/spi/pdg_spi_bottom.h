/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context Zephyr SPI driver and the
 * host-context wrapper that calls the Pico de Gallo C FFI.
 *
 * Chip select is no longer part of this interface. The Zephyr driver drives
 * every chip-select edge through the odp,pico-de-gallo-gpio child declared in
 * the controller's cs-gpios property, so the only firmware operations reached
 * from here are bus configuration and one full-duplex transfer.
 */

#ifndef PDG_SPI_BOTTOM_H
#define PDG_SPI_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Configure the SPI bus frequency, clock phase and clock polarity. */
int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity);

/* Clock `len` bytes out of `write_buf` while clocking `len` bytes into
 * `read_buf`. Both pointers must be valid for `len` bytes; the caller supplies
 * zero-filled TX scratch for a read-only transfer and discard RX scratch for a
 * write-only one, because the firmware endpoint is always full duplex.
 *
 * Returns 0, -EINVAL, -EMSGSIZE, -EPROTO, -EIO or -ECOMM.
 */
int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf,
			    uint8_t *read_buf, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* PDG_SPI_BOTTOM_H */
