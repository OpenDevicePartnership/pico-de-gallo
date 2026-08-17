/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context Zephyr SPI driver and the
 * host-context wrapper that calls the Pico de Gallo C FFI.
 */

#ifndef PDG_SPI_BOTTOM_H
#define PDG_SPI_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define PDG_SPI_MAX_BATCH_OPS 64U

enum pdg_spi_batch_op_tag {
	PDG_SPI_BATCH_READ = 0,
	PDG_SPI_BATCH_WRITE = 1,
	PDG_SPI_BATCH_TRANSFER = 2,
	PDG_SPI_BATCH_DELAY_NS = 3,
};

struct pdg_spi_batch_op {
	uint8_t tag;
	uint16_t read_len;
	const uint8_t *data;
	size_t data_len;
	uint32_t delay_ns;
};

#ifdef __cplusplus
extern "C" {
#endif

/* Open a Pico de Gallo bridge. serial may be NULL/empty to pick the first one.
 * Returns an opaque context pointer, or NULL if no matching device is reachable
 * or the firmware fails validation.
 */
void *pdg_spi_bottom_open(const char *serial);

/* Release a context previously returned by pdg_spi_bottom_open(). */
void pdg_spi_bottom_close(void *ctx);

/* Read how many GPIOs the firmware reports, into *out_num_gpios.
 *
 * This is a warm-cache read: pdg_spi_bottom_open() validates the device, and a
 * successful validation populates the shared count cache, so no USB traffic is
 * generated here. *out_num_gpios is written only on success.
 */
int pdg_spi_bottom_num_gpios(void *ctx, uint8_t *out_num_gpios);

int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity);

int pdg_spi_bottom_batch(void *ctx, uint8_t cs_pin,
			 const struct pdg_spi_batch_op *ops, size_t ops_count,
			 uint8_t *out_buf, size_t out_capacity, size_t *out_len,
			 uint16_t *out_failed_op);

#ifdef __cplusplus
}
#endif

#endif /* PDG_SPI_BOTTOM_H */
