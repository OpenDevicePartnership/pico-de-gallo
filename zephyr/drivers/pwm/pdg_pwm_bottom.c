/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the Pico de Gallo PWM driver.
 *
 * Compiled into the native simulator runner with the host C library, linking
 * against the Pico de Gallo FFI. It must not include any Zephyr header.
 */

#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "pico_de_gallo.h"
#include "common.h"
#include "pdg_pwm_bottom.h"

/*
 * These are __attribute__((weak)) so the recording fake in
 * zephyr/tests/pdg_fake/pwm can link strong definitions and observe what the
 * driver asked the device to do. Not Zephyr's __weak: this file is
 * host-context and cannot include zephyr/toolchain.h.
 *
 * Every FFI return is routed through pdg_common_status_to_errno(). cbindgen
 * emits "typedef int32_t Status" under C11/C17, so returning a raw Status
 * compiles silently and hands the top half a positive, unmapped number that it
 * would then treat as a negative POSIX errno.
 */

__attribute__((weak)) int pdg_pwm_bottom_has_pwm(void *ctx, bool *out_has_pwm)
{
	struct GalloDeviceInfo info;
	int ret;

	if (out_has_pwm == NULL) {
		return -EINVAL;
	}

	/*
	 * `info` is a bare stack struct and the FFI leaves it untouched when
	 * the query fails, so capabilities is read only after the status has
	 * been confirmed successful.
	 */
	ret = pdg_common_status_to_errno(
		gallo_get_device_info((const struct PicoDeGallo *)ctx, &info));
	if (ret != 0) {
		return ret;
	}

	*out_has_pwm = (info.capabilities & GALLO_CAP_PWM) != 0U;

	return 0;
}

__attribute__((weak)) int pdg_pwm_bottom_set_config(void *ctx, uint8_t channel,
						    uint32_t frequency_hz,
						    bool phase_correct)
{
	return pdg_common_status_to_errno(
		gallo_pwm_set_config((const struct PicoDeGallo *)ctx, channel,
				     frequency_hz, phase_correct));
}

__attribute__((weak)) int pdg_pwm_bottom_get_duty_cycle(void *ctx, uint8_t channel,
							uint16_t *out_duty,
							uint16_t *out_max_duty)
{
	return pdg_common_status_to_errno(
		gallo_pwm_get_duty_cycle((const struct PicoDeGallo *)ctx, channel,
					 out_duty, out_max_duty));
}

__attribute__((weak)) int pdg_pwm_bottom_set_duty_cycle(void *ctx, uint8_t channel,
							uint16_t duty)
{
	return pdg_common_status_to_errno(
		gallo_pwm_set_duty_cycle((const struct PicoDeGallo *)ctx, channel,
					 duty));
}

__attribute__((weak)) int pdg_pwm_bottom_enable(void *ctx, uint8_t channel)
{
	return pdg_common_status_to_errno(
		gallo_pwm_enable((const struct PicoDeGallo *)ctx, channel));
}
