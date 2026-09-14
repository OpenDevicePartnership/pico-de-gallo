/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * FFI-free interface between the two halves of the Pico de Gallo PWM driver.
 *
 * Only basic C types appear here so that the embedded side never needs to
 * include the host-only pico_de_gallo.h. All functions return 0 on success or
 * a negative POSIX errno on failure.
 *
 * There is deliberately no open/close pair: this controller is born into MFD
 * ownership and only ever borrows the parent's context.
 *
 * There is ALSO deliberately no disable function. The firmware's disable acts
 * on a whole slice, so disabling one channel stops its sibling. Not declaring
 * it means the top half cannot call it by accident -- the omission is the
 * enforcement, not a comment asking future maintainers to be careful.
 */

#ifndef PDG_PWM_BOTTOM_H
#define PDG_PWM_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * PWM source clock in Hz, reported verbatim by pwm_get_cycles_per_sec().
 *
 * SOURCE OF TRUTH: crates/pico-de-gallo-firmware/src/context.rs, SYS_CLK_HZ.
 * That constant is pub(crate): it is not on the wire and not in
 * pico_de_gallo.h, so this value cannot be tied to it by a _Static_assert and
 * is a genuine duplication. If the firmware's system clock ever changes, this
 * must change with it.
 *
 * This is the SOURCE clock, not the counter rate. The counter advances at
 * 150e6/divider, which changes with every frequency change and differs
 * between the two slices, so it could never serve as the stable per-device
 * constant Zephyr's API requires. Reporting the source clock makes
 * period_cycles independent of the divider -- which matters, because the
 * divider is not on the wire and the driver cannot observe it.
 */
#define PDG_PWM_CYCLES_PER_SEC 150000000U

/*
 * Channels exposed by the firmware. Mirrors NUM_PWM_CHANNELS in
 * pico-de-gallo-internal. Not assertable: unlike GALLO_NUM_GPIOS, the PWM
 * channel count is not exported to C at all.
 */
#define PDG_PWM_NUM_CHANNELS 4U

/* Channels per RP2350 slice. Channels 0+1 share one, 2+3 share another. */
#define PDG_PWM_CHANNELS_PER_SLICE 2U

#define PDG_PWM_NUM_SLICES (PDG_PWM_NUM_CHANNELS / PDG_PWM_CHANNELS_PER_SLICE)

/*
 * Largest clock divider the RP2350 CH_DIV register can hold in its integer
 * part. The firmware searches up to 4095 and panics above this; see issue
 * #192. The period bound below exists solely to keep requests out of that
 * range.
 */
#define PDG_PWM_MAX_DIVIDER 255U

/*
 * Period bounds in source-clock cycles.
 *
 * Lower: the firmware requires top >= 1, so a period of one cycle is
 * unreachable.
 *
 * Upper: 255 * 65536. Chosen because it is obviously safe by inspection, and
 * it sits comfortably below the true edge rather than at it. With the
 * ceiling conversion the driver uses, frequency_hz = ceil(150e6 / period)
 * stays at 9 Hz -- which needs exactly divider 255 -- for every period up to
 * 18749999. Only at 18750000 does 150e6 / period land on exactly 8, where the
 * ceiling is 8 Hz, which needs divider 287 and panics. So the true safe
 * maximum is 18749999 and this bound is substantially conservative.
 */
#define PDG_PWM_MIN_PERIOD_CYCLES 2U
#define PDG_PWM_MAX_PERIOD_CYCLES ((uint64_t)PDG_PWM_MAX_DIVIDER * 65536U)

/*
 * Report whether the attached board advertises the PWM capability.
 *
 * Returns 0 and writes *out_has_pwm on success; a negative errno if the query
 * itself failed. The two shapes are distinct and the caller depends on it:
 * "answered, has no PWM" is -ENODEV, while "did not answer" reports its own
 * errno. *out_has_pwm is written only on success.
 */
int pdg_pwm_bottom_has_pwm(void *ctx, bool *out_has_pwm);

/* Set the frequency of the slice owning `channel`. Reconfigures the whole
 * slice, including the sibling channel.
 */
int pdg_pwm_bottom_set_config(void *ctx, uint8_t channel, uint32_t frequency_hz,
			      bool phase_correct);

/* Read the current and full-scale compare values. *out_max_duty is top + 1. */
int pdg_pwm_bottom_get_duty_cycle(void *ctx, uint8_t channel, uint16_t *out_duty,
				  uint16_t *out_max_duty);

/* Set the raw compare value. 0 is always-low; max_duty is always-high. */
int pdg_pwm_bottom_set_duty_cycle(void *ctx, uint8_t channel, uint16_t duty);

/* Enable the slice owning `channel`. There is no counterpart by design. */
int pdg_pwm_bottom_enable(void *ctx, uint8_t channel);

#ifdef __cplusplus
}
#endif

#endif /* PDG_PWM_BOTTOM_H */
