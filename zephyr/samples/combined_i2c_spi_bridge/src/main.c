/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Combined sample/test for the Pico de Gallo I2C and SPI controllers. Reads
 * a TI TMP117 through the Zephyr sensor API while driving an IS31FL3743B LED
 * matrix through the Zephyr LED API. Both controllers share one bridge.
 */

#include <string.h>

#include <zephyr/device.h>
#include <zephyr/drivers/led.h>
#include <zephyr/drivers/sensor.h>
#include <zephyr/kernel.h>

/* IS31FL3743B matrix is 11x18 */
#define NUM_LEDS (11 * 18)
#define CHANNELS_PER_PIXEL 3
#define WORKER_STACK_SIZE 2048
#define WORKER_PRIORITY 5

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

static void i2c_worker(void *unused1, void *unused2, void *unused3)
{
	const struct device *tmp117 = DEVICE_DT_GET(DT_NODELABEL(tmp117));
	struct sensor_value temp;
	int ret;

	ARG_UNUSED(unused1);
	ARG_UNUSED(unused2);
	ARG_UNUSED(unused3);

	if (!device_is_ready(tmp117)) {
		printk("TMP117 not ready (Pico de Gallo bridge connected?)\n");
		return;
	}

	while (1) {
		ret = sensor_sample_fetch(tmp117);
		if (ret < 0) {
			printk("sensor_sample_fetch failed: %d\n", ret);
		} else {
			ret = sensor_channel_get(tmp117, SENSOR_CHAN_AMBIENT_TEMP, &temp);
			if (ret < 0) {
				printk("sensor_channel_get failed: %d\n", ret);
			} else {
				printk("Temperature: %d.%06d C\n", temp.val1, temp.val2);
			}
		}

		k_sleep(K_SECONDS(1));
	}
}

static void spi_worker(void *unused1, void *unused2, void *unused3)
{
	const struct device *led_matrix = DEVICE_DT_GET(DT_NODELABEL(led_matrix));
	uint8_t pwm[NUM_LEDS];
	int ret;

	ARG_UNUSED(unused1);
	ARG_UNUSED(unused2);
	ARG_UNUSED(unused3);

	if (!device_is_ready(led_matrix)) {
		printk("IS31FL3743B not ready (Pico de Gallo bridge connected?)\n");
		return;
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
}

K_THREAD_DEFINE(i2c_worker_id, WORKER_STACK_SIZE, i2c_worker, NULL, NULL, NULL,
		WORKER_PRIORITY, 0, 0);
K_THREAD_DEFINE(spi_worker_id, WORKER_STACK_SIZE, spi_worker, NULL, NULL, NULL,
		WORKER_PRIORITY, 0, 0);

int main(void)
{
	printk("Pico de Gallo combined I2C/SPI bridge sample started\n");
	return 0;
}
