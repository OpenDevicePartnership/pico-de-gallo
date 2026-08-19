/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context M5 test applications and the
 * host-context shim that calls the Pico de Gallo C FFI.
 *
 * WHY THIS FILE EXISTS
 * --------------------
 * native_sim splits into an embedded context (Zephyr, the applications'
 * src/main.c) and a host context (the native simulator runner). pico_de_gallo.h
 * is on the host include path only -- zephyr/CMakeLists.txt adds it via
 * target_compile_options(native_simulator INTERFACE "-I..."). An embedded
 * translation unit therefore cannot call gallo_* directly, which is exactly why
 * every production driver has a pdg_*_bottom.c.
 *
 * The M5 specification requires four FFI entry points for which no production
 * bottom half exists: gallo_system_reset_subscriptions (acceptance spec §3 and
 * §10 step 1), gallo_gpio_subscribe and gallo_gpio_unsubscribe (test design §6.2
 * steps 2, 5 and 9), and gallo_spi_get_config (acceptance spec §10 step 4). This
 * shim supplies them.
 *
 * gallo_gpio_set_config (test design §6.2 step 6) is deliberately NOT wrapped
 * here: the production pdg_gpio_bottom_set_config() already exposes it, and the
 * acceptance applications link it directly.
 *
 * This shim adds no FFI entry point, alters none, and emulates none. Each
 * wrapper is a call-through in precisely the shape of
 * pdg_gpio_bottom_set_config(). Authorized by the coordinator after the
 * host/embedded context split was discovered during M5 implementation.
 *
 * Only basic C types may appear here so that the embedded side never needs to
 * include the host-only pico_de_gallo.h header. All functions return 0 when the
 * FFI returned the symbolic success enumerator read from the generated cbindgen
 * header, or a negative POSIX errno value otherwise.
 */

#ifndef M5_BOTTOM_H
#define M5_BOTTOM_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Edge selector accepted by m5_bottom_gpio_subscribe().
 *
 * Mirrors GalloGpioEdge, whose values are stable C ABI (AGENTS.md §8). Spelled
 * as a plain integer here because this header must stay free of the host-only
 * FFI header; the shim asserts the correspondence against the generated
 * GalloGpioEdge_Any at compile time.
 */
#define M5_BOTTOM_GPIO_EDGE_RISING 0U
#define M5_BOTTOM_GPIO_EDGE_FALLING 1U
#define M5_BOTTOM_GPIO_EDGE_ANY 2U

/*
 * Direction and pull selectors for pdg_gpio_bottom_set_config().
 *
 * That production entry point takes plain uint8_t, and the embedded side cannot
 * name GalloGpioDirection_Output / GalloGpioPull_None because the FFI header is
 * host-only. These spellings exist so the applications do not hardcode bare
 * integers; m5_bottom.c static-asserts each one against the generated cbindgen
 * enumerator, so a divergence is a build failure rather than a wrong pin mode.
 */
#define M5_BOTTOM_GPIO_DIR_INPUT 0U
#define M5_BOTTOM_GPIO_DIR_OUTPUT 1U
#define M5_BOTTOM_GPIO_PULL_NONE 0U
#define M5_BOTTOM_GPIO_PULL_UP 1U
#define M5_BOTTOM_GPIO_PULL_DOWN 2U

/* Tear down every firmware GPIO event subscription on this board.
 *
 * Writes the number of subscriptions that were reset into *out_reset when
 * out_reset is non-NULL. Reset affects subscriptions only: it does not restore
 * pin modes, pulls, output levels, SPI configuration, or any Zephyr-side
 * lock/latch state (acceptance spec §3).
 */
int m5_bottom_reset_subscriptions(void *ctx, uint8_t *out_reset);

/* Subscribe to firmware GPIO edge events on one pin.
 *
 * The firmware monitor task takes ownership of the pin and configures it as an
 * input. This is the zero-driver-change fault route of acceptance spec §6.2: it
 * is what makes a later checked chip-select deassert fail with -EBUSY.
 *
 * edge must be one of the M5_BOTTOM_GPIO_EDGE_* values above.
 */
int m5_bottom_gpio_subscribe(void *ctx, uint8_t pin, uint8_t edge);

/* Release a firmware GPIO event subscription on one pin.
 *
 * Returns -ENOENT when the pin was not subscribed (GpioPinNotMonitored), which
 * test design §6.2 step 9 asserts on deliberately.
 *
 * The monitor leaves the physical pad an input; firmware still tracks whatever
 * mode was recorded before the subscription, so a caller that needs the pad back
 * as an output must reconcile it with pdg_gpio_bottom_set_config().
 */
int m5_bottom_gpio_unsubscribe(void *ctx, uint8_t pin);

/* Read the bus's current SPI configuration.
 *
 * *out_phase is true for capture-on-second-transition (CPHA=1) and *out_polarity
 * is true for idle-high (CPOL=1), matching gallo_spi_get_config().
 */
int m5_bottom_spi_get_config(void *ctx, uint32_t *out_frequency,
			     bool *out_phase, bool *out_polarity);

/* Read the HOST's monotonic clock, in microseconds.
 *
 * Not an FFI call-through -- the one exception in this file, and it is here
 * because this is the only host-context translation unit the M5 applications
 * own.
 *
 * WHY THIS IS NECESSARY. Zephyr's clock on native_sim measures SIMULATED time,
 * which does not advance while the host thread is blocked inside a USB call.
 * Every Pico de Gallo operation is exactly such a blocking call, so
 * k_cycle_get_32() around a transfer reports approximately zero elapsed time
 * however long the transfer really took. Measured during M5 acceptance:
 * p50 = p95 = p99 = max = 0 us across 25 real, correctly-echoing transfers at
 * each of two frequencies.
 *
 * A timing measurement taken from the simulated clock on this target is
 * therefore not merely imprecise, it is vacuous. This returns real elapsed
 * wall-clock microseconds instead, which is what the transfer actually costs.
 *
 * The same limitation applies to upstream spi_loopback's
 * test_spi_complete_multiple_timed, which uses the Zephyr clock and so passes
 * vacuously on native_sim. That is recorded in the aggregate verdict's
 * explicitly_untested list rather than counted as coverage.
 */
uint64_t m5_bottom_host_monotonic_us(void);

#ifdef __cplusplus
}
#endif

#endif /* M5_BOTTOM_H */
