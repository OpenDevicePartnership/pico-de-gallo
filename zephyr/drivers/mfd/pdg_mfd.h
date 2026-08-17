/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Child-facing API for the Pico de Gallo MFD parent.
 */

#ifndef PDG_MFD_H
#define PDG_MFD_H

#include <zephyr/device.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Return the borrowed opaque context owned by a Pico de Gallo parent.
 *
 * This accessor is NULL-safe, but it is not a substitute for checking parent
 * readiness. A child must first require device_is_ready(parent). If readiness
 * is false, the child must log the parent's name and return -ENODEV. Only after
 * that check may it call pdg_mfd_ctx(parent). A NULL result after a successful
 * readiness check is an invariant failure; the child must log it and return
 * -ENODEV. The returned context is borrowed: callers must never close or free
 * it, and it remains valid for the parent's static process lifetime.
 *
 * @param dev Pico de Gallo MFD parent device, or NULL.
 * @return Borrowed opaque context, or NULL when dev is NULL or the parent did
 *         not open successfully.
 */
void *pdg_mfd_ctx(const struct device *dev);

#ifdef __cplusplus
}
#endif

#endif /* PDG_MFD_H */
