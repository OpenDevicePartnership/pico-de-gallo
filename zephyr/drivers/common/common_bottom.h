/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between embedded-context Zephyr drivers and the host-context
 * shim that owns the shared Pico de Gallo connection registry.
 *
 * Only basic C types may appear here so that the embedded side never needs to
 * include the host-only FFI header. This file is the single declaration site
 * for these two symbols; common.h includes it rather than repeating them, so
 * host and embedded callers can never drift apart.
 */

#ifndef PDG_COMMON_BOTTOM_H
#define PDG_COMMON_BOTTOM_H

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

#ifdef __cplusplus
}
#endif

#endif /* PDG_COMMON_BOTTOM_H */
