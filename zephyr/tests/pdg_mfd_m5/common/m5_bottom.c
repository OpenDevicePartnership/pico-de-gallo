/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the M5 test applications.
 *
 * This file is compiled into the native simulator runner with the host C
 * library and links against the Pico de Gallo FFI. It must not include any
 * Zephyr headers. It is wired in by each application's own CMakeLists.txt with
 * target_sources(native_simulator INTERFACE ...), the same mechanism
 * zephyr/drivers/CMakeLists.txt already uses for common.c and gallo_registry.c.
 *
 * Every function here is a call-through to an existing gallo_* entry point. No
 * FFI entry point is added, altered or emulated, and no production driver is
 * touched.
 */

#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <time.h>

#include "pico_de_gallo.h"
#include "common.h"
#include "m5_bottom.h"

/*
 * The embedded side names these selectors as plain integers because the FFI
 * header is host-only (see m5_bottom.h). This is the single place where both
 * spellings are visible at once, so it is the only place the correspondence can
 * be checked. AGENTS.md §8 makes these values stable C ABI; a divergence here is
 * a build failure rather than a subscription on the wrong edge or a pin driven
 * in the wrong direction.
 */
_Static_assert(M5_BOTTOM_GPIO_EDGE_RISING == GalloGpioEdge_Rising,
	       "M5_BOTTOM_GPIO_EDGE_RISING must match GalloGpioEdge_Rising");
_Static_assert(M5_BOTTOM_GPIO_EDGE_FALLING == GalloGpioEdge_Falling,
	       "M5_BOTTOM_GPIO_EDGE_FALLING must match GalloGpioEdge_Falling");
_Static_assert(M5_BOTTOM_GPIO_EDGE_ANY == GalloGpioEdge_Any,
	       "M5_BOTTOM_GPIO_EDGE_ANY must match GalloGpioEdge_Any");
_Static_assert(M5_BOTTOM_GPIO_DIR_INPUT == GalloGpioDirection_Input,
	       "M5_BOTTOM_GPIO_DIR_INPUT must match GalloGpioDirection_Input");
_Static_assert(M5_BOTTOM_GPIO_DIR_OUTPUT == GalloGpioDirection_Output,
	       "M5_BOTTOM_GPIO_DIR_OUTPUT must match GalloGpioDirection_Output");
_Static_assert(M5_BOTTOM_GPIO_PULL_NONE == GalloGpioPull_None,
	       "M5_BOTTOM_GPIO_PULL_NONE must match GalloGpioPull_None");
_Static_assert(M5_BOTTOM_GPIO_PULL_UP == GalloGpioPull_Up,
	       "M5_BOTTOM_GPIO_PULL_UP must match GalloGpioPull_Up");
_Static_assert(M5_BOTTOM_GPIO_PULL_DOWN == GalloGpioPull_Down,
	       "M5_BOTTOM_GPIO_PULL_DOWN must match GalloGpioPull_Down");

/*
 * Success is compared against the symbolic enumerator from the generated
 * cbindgen header, as acceptance spec §3 and test design §6.1 require. Not
 * Status_Ok, not GalloStatus_Ok, not a bare 0: cbindgen.toml keeps the Status
 * variants unprefixed precisely because they are stable C ABI, so the spelling
 * is `Ok`.
 *
 * Error mapping delegates to pdg_common_status_to_errno() rather than repeating
 * its switch. That function already carries the AGENTS.md §13.17 (2026-08-17)
 * discipline this shim owes: it switches on (enum Status) with no `default:`
 * inside the switch, so -Werror=switch -- applied native-simulator-wide by
 * zephyr/drivers/CMakeLists.txt -- turns an omitted enumerator into a build
 * failure, and its unknown-value fallback to -EIO sits *after* the switch for
 * a numeric status outside the enum. Duplicating a seventy-case switch here
 * would create exactly the drift that single declaration site exists to
 * prevent.
 */
static int m5_status_to_errno(Status status)
{
	if ((enum Status)status == Ok) {
		return 0;
	}

	return pdg_common_status_to_errno(status);
}

int m5_bottom_reset_subscriptions(void *ctx, uint8_t *out_reset)
{
	return m5_status_to_errno(gallo_system_reset_subscriptions(
		(const struct PicoDeGallo *)ctx, out_reset));
}

int m5_bottom_gpio_subscribe(void *ctx, uint8_t pin, uint8_t edge)
{
	return m5_status_to_errno(gallo_gpio_subscribe(
		(const struct PicoDeGallo *)ctx, pin, edge));
}

int m5_bottom_gpio_unsubscribe(void *ctx, uint8_t pin)
{
	return m5_status_to_errno(gallo_gpio_unsubscribe(
		(const struct PicoDeGallo *)ctx, pin));
}

int m5_bottom_spi_get_config(void *ctx, uint32_t *out_frequency,
			     bool *out_phase, bool *out_polarity)
{
	return m5_status_to_errno(gallo_spi_get_config(
		(const struct PicoDeGallo *)ctx, out_frequency, out_phase,
		out_polarity));
}

/*
 * Host wall clock. See the rationale in m5_bottom.h: Zephyr's simulated clock
 * does not advance while the host thread blocks in a USB call, so it cannot
 * measure a Pico de Gallo transfer at all.
 *
 * CLOCK_MONOTONIC rather than CLOCK_REALTIME: this is only ever used for
 * differences, and it must not jump if the host clock is stepped mid-run.
 */
uint64_t m5_bottom_host_monotonic_us(void)
{
	struct timespec ts;

	if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
		return 0U;
	}

	return ((uint64_t)ts.tv_sec * 1000000U) + ((uint64_t)ts.tv_nsec / 1000U);
}
