/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Small sample/test for the Pico de Gallo SPI controller. Runs a
 * IS31FL3743B LED matrix on the bridged SPI bus via the Zephyr LED API.
 */

#include <string.h>

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/led.h>

/* IS31FL3743B matrix is 11x18 */
#define NUM_LEDS (11 * 18)
#define CHANNELS_PER_PIXEL 3

static const uint8_t colors[][CHANNELS_PER_PIXEL] = {
	{ 255, 0, 0 },
	{ 0, 255, 0 },
	{ 0, 0, 255 },
	{ 255, 255, 0 },
	{ 0, 255, 255 },
	{ 255, 0, 255 },
};

static void fill_color(uint8_t *pwm, const uint8_t color[CHANNELS_PER_PIXEL], uint8_t level)
{
	for (size_t channel = 0; channel < NUM_LEDS; channel++) {
		pwm[channel] = (color[channel % CHANNELS_PER_PIXEL] * level) / 255;
	}
}

int main(void)
{
	const struct device *led_matrix = DEVICE_DT_GET(DT_NODELABEL(led_matrix));
	uint8_t pwm[NUM_LEDS];
	int ret;

	if (!device_is_ready(led_matrix)) {
		printk("IS31FL3743B not ready (Pico de Gallo bridge connected?)\n");
		return 0;
	}

	while (1) {
		for (size_t color = 0; color < ARRAY_SIZE(colors); color++) {
			/* Breathe each color up to full brightness and back down. */
			for (int level = 0; level <= 510; level++) {
				uint8_t brightness = level <= 255 ? level : 510 - level;

				fill_color(pwm, colors[color], brightness);
				ret = led_write_channels(led_matrix, 0, NUM_LEDS, pwm);
				if (ret < 0) {
					printk("led_write_channels failed: %d\n", ret);
				}

				k_sleep(K_MSEC(5));
			}
		}
	}

	return 0;
}
