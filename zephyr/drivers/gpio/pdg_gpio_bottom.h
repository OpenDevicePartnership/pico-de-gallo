/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context Zephyr GPIO driver and the
 * host-context shim that calls the Pico de Gallo C FFI.
 *
 * Only basic C types may appear here so that the embedded side never needs to
 * include the host-only pico_de_gallo.h header. All functions return 0 on
 * success or a negative POSIX errno value on failure.
 *
 * There is deliberately no open/close pair here. Unlike the pre-MFD drivers,
 * this controller is born into MFD ownership: the odp,pico-de-gallo parent
 * holds the sole registry reference and the child only ever borrows it. A
 * close wrapper would make it possible to drop the parent's reference from a
 * child, leaving the parent and its I2C/SPI siblings holding a freed pointer.
 */

#ifndef PDG_GPIO_BOTTOM_H
#define PDG_GPIO_BOTTOM_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Read the physical level of one pin into *state.
 *
 * Firmware rejects a read of a pin it has recorded as an explicit output with
 * GpioWrongDirection, which surfaces here as -EACCES.
 */
int pdg_gpio_bottom_get(void *ctx, uint8_t pin, bool *state);

/* Drive one pin high or low.
 *
 * Firmware rejects a write to a pin it has recorded as an explicit input with
 * GpioWrongDirection, which surfaces here as -EACCES.
 */
int pdg_gpio_bottom_put(void *ctx, uint8_t pin, bool state);

/* Configure one pin's direction (0 = input, 1 = output) and pull resistor
 * (0 = none, 1 = up, 2 = down).
 */
int pdg_gpio_bottom_set_config(void *ctx, uint8_t pin,
			       uint8_t direction, uint8_t pull);

/* Read the firmware-reported GPIO count into *out_num_gpios.
 *
 * The parent's strict open already validated the device and populated the
 * handle-shared device-info cache, so this is a warm local read: it issues no
 * USB traffic and carries no device/info timeout exposure.
 */
int pdg_gpio_bottom_num_gpios(void *ctx, uint8_t *out_num_gpios);

#ifdef __cplusplus
}
#endif

#endif /* PDG_GPIO_BOTTOM_H */
