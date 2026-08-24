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

/* The open/close pair is declared once, in the FFI-free common_bottom.h, so
 * that the embedded side can see it without pulling in pico_de_gallo.h.
 */
#include "common_bottom.h"
#include "pico_de_gallo.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Converts a pico-de-gallo-ffi status to a Zephyr errno. */
int pdg_common_status_to_errno(Status status);

#ifdef __cplusplus
}
#endif

#endif /* PDG_COMMON_H */
