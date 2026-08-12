/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Common helpers and glue for all drivers.
 */

#ifndef PDG_COMMON_H
#define PDG_COMMON_H

#include <stddef.h>
#include <stdint.h>

#include "pico_de_gallo.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Open a Pico de Gallo bridge. serial may be NULL/empty to pick the first one.
 * Returns an opaque context pointer, or NULL if no matching device is reachable
 * or the firmware fails validation.
 */
void *pdg_common_bottom_open(const char *serial);

/* Release a context previously returned by pdg_common_bottom_open(). */
void pdg_common_bottom_close(void *ctx);

/* Converts a pico-de-gallo-ffi status to a Zephyr errno. */
int pdg_common_status_to_errno(Status status);

#ifdef __cplusplus
}
#endif

#endif /* PDG_COMMON_H */
