/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Embedded-facing view of the PWM recording fake.
 *
 * native_sim splits into an embedded context (Zephyr, src/main.c) and a host
 * context (the native simulator runner). The fake lives in the host context
 * because that is where the production bottom half lives, and the ztest
 * assertions live in the embedded context. The two share NO globals, so
 * everything a test observes crosses this boundary through an accessor that
 * copies scalars rather than handing back a pointer into host storage.
 *
 * This header is FFI-free -- stdbool/stddef/stdint only.
 */

#ifndef PDG_PWM_FAKE_BOTTOM_H
#define PDG_PWM_FAKE_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PDG_PWM_FAKE_LOG_MAX 32

/* Discard all recorded calls and reset scripted results. Call from setup. */
void pdg_pwm_fake_reset(void);

/* Call counts. */
int pdg_pwm_fake_set_config_count(void);
int pdg_pwm_fake_get_duty_count(void);
int pdg_pwm_fake_set_duty_count(void);
int pdg_pwm_fake_enable_count(void);

/*
 * How many times the production gallo_pwm_disable path was reached.
 *
 * The EXPECTED VALUE IS ZERO AND ALWAYS WILL BE. pdg_pwm_bottom.h declares no
 * disable function, so the top half cannot call one. This counter exists so
 * that a future change which adds a disable wrapper is caught by a failing
 * test rather than silently stopping a sibling channel. It is incremented by
 * a strong definition of pdg_pwm_bottom_disable(), which currently overrides
 * nothing.
 */
int pdg_pwm_fake_disable_count(void);

/* The most recent set_config. Returns 0, or -1 if there was none. */
int pdg_pwm_fake_last_set_config(uint8_t *out_channel, uint32_t *out_frequency_hz,
				 bool *out_phase_correct);

/* Entry `index` of the set_duty log. Returns 0, or -1 if out of range. */
int pdg_pwm_fake_set_duty_log_len(void);
int pdg_pwm_fake_set_duty_log_entry(int index, uint8_t *out_channel,
				    uint16_t *out_duty);

/*
 * The full-scale duty the fake would report for `channel` right now, i.e.
 * top + 1 for the frequency most recently configured on that slice. Lets a
 * test compute the compare value it expects without duplicating the firmware
 * model in the embedded half.
 *
 * Returns 0 and writes *out_max_duty, or -1 if the slice was never
 * configured.
 */
int pdg_pwm_fake_max_duty_for(uint8_t channel, uint16_t *out_max_duty);

/* Script a failure for the next and subsequent calls. 0 restores success. */
void pdg_pwm_fake_set_set_config_result(int result);
void pdg_pwm_fake_set_set_duty_result(int result);
void pdg_pwm_fake_set_enable_result(int result);

/* Non-zero if a log overflowed; a test that overran must fail, not assert
 * against a prefix.
 */
int pdg_pwm_fake_overflowed(void);

#ifdef __cplusplus
}
#endif

#endif /* PDG_PWM_FAKE_BOTTOM_H */
