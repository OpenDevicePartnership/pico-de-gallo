/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Fades an LED on PWM channel 0 (GPIO12) of a USB-attached Pico de Gallo.
 *
 * The period is expressed in cycles against pwm_get_cycles_per_sec(), which
 * this driver reports as the 150 MHz PWM source clock. Asking the driver
 * rather than hardcoding 150000000 is the point: a consumer should never need
 * to know the firmware's system clock.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/pwm.h>

#define PWM_CHANNEL 0U
#define FADE_STEPS  50U
#define TARGET_HZ   1000U

int main(void)
{
	const struct device *pwm = DEVICE_DT_GET(DT_NODELABEL(pdg_pwm0));
	uint64_t cycles_per_sec;
	uint32_t period;
	int ret;

	if (!device_is_ready(pwm)) {
		printk("PWM not ready (Pico de Gallo bridge connected?)\n");
		return 0;
	}

	ret = pwm_get_cycles_per_sec(pwm, PWM_CHANNEL, &cycles_per_sec);
	if (ret < 0) {
		printk("pwm_get_cycles_per_sec failed: %d\n", ret);
		return 0;
	}

	period = (uint32_t)(cycles_per_sec / TARGET_HZ);
	printk("Fading channel %u at %u Hz (%u cycles of %llu per second)\n",
	       PWM_CHANNEL, TARGET_HZ, period,
	       (unsigned long long)cycles_per_sec);

	while (1) {
		for (uint32_t step = 0U; step <= FADE_STEPS; step++) {
			uint32_t pulse = (uint32_t)(((uint64_t)period * step) /
						    FADE_STEPS);

			ret = pwm_set_cycles(pwm, PWM_CHANNEL, period, pulse, 0);
			if (ret < 0) {
				printk("pwm_set_cycles failed: %d\n", ret);
				return 0;
			}

			k_sleep(K_MSEC(20));
		}
	}

	return 0;
}
