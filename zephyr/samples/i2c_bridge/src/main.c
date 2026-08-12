/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Small sample/test for the Pico de Gallo I2C controller. Reads the ambient
 * temperature from a TI TMP117 on the bridged I2C bus via the Zephyr sensor
 * API.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/sensor.h>

int main(void)
{
	const struct device *tmp117 = DEVICE_DT_GET(DT_NODELABEL(tmp117));
	struct sensor_value temp;
	int ret;

	if (!device_is_ready(tmp117)) {
		printk("TMP117 not ready (Pico de Gallo bridge connected?)\n");
		return 0;
	}

	while (1) {
		ret = sensor_sample_fetch(tmp117);
		if (ret < 0) {
			printk("sensor_sample_fetch failed: %d\n", ret);
		}

		ret = sensor_channel_get(tmp117, SENSOR_CHAN_AMBIENT_TEMP, &temp);
		if (ret < 0) {
			printk("sensor_channel_get failed: %d\n", ret);
		}

		printk("Temperature: %d.%06d C\n", temp.val1, temp.val2);

		k_sleep(K_SECONDS(1));
	}

	return 0;
}
