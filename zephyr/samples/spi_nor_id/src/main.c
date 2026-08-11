/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Identifies a JEDEC SPI NOR flash attached to a Pico de Gallo bridge, using
 * Zephyr's flash API from native_sim.
 *
 * The point of this sample is that nothing here is Pico de Gallo specific.
 * It uses the stock jedec,spi-nor driver and the generic flash API; the
 * bridge is just another SPI controller as far as they are concerned. The
 * geometry printed below is discovered from the device at runtime via SFDP,
 * so the driver really is talking to the part rather than reading constants
 * back out of the devicetree.
 *
 * This sample only ever calls flash_read(). It does not call flash_write(),
 * flash_erase() or flash_ex_op(), and app.overlay omits every devicetree
 * property that would let the driver issue a write during initialisation.
 * See the comment there for the specific list.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/flash.h>
#include <zephyr/sys/printk.h>

#define NOR_DUMP_LEN 16U

static const struct device *const nor = DEVICE_DT_GET(DT_NODELABEL(nor));

static void hexdump(const uint8_t *data, size_t len)
{
	for (size_t i = 0; i < len; i++) {
		printk("%02X%s", data[i], (i + 1U == len) ? "" : " ");
	}
}

int main(void)
{
	const struct flash_parameters *params;
	uint8_t buf[NOR_DUMP_LEN];
	uint64_t size = 0;
	int ret;

	if (!device_is_ready(nor)) {
		printk("Flash not ready (Pico de Gallo bridge and flash connected?)\n");
		return 0;
	}

	/*
	 * Reaching this point already proves a good deal: the driver read the
	 * JEDEC ID and walked the SFDP tables over the bridge during init, and
	 * refused to become ready if either failed.
	 */
	printk("Flash device ready: %s\n", nor->name);

	ret = flash_get_size(nor, &size);
	if (ret == 0) {
		printk("size:     %llu KiB\n", size / 1024U);
	} else {
		printk("size:     unavailable (%d)\n", ret);
	}

	params = flash_get_parameters(nor);
	if (params != NULL) {
		printk("write:    %zu B block, erased value 0x%02X\n",
		       params->write_block_size, params->erase_value);
	}

#if defined(CONFIG_FLASH_PAGE_LAYOUT)
	struct flash_pages_info page;

	ret = flash_get_page_info_by_offs(nor, 0, &page);
	if (ret == 0) {
		printk("erase:    %zu B pages, %zu total\n", page.size,
		       flash_get_page_count(nor));
	}
#endif

	ret = flash_read(nor, 0, buf, sizeof(buf));
	if (ret < 0) {
		printk("flash_read failed: %d\n", ret);
		return 0;
	}

	printk("@000000:  ");
	hexdump(buf, sizeof(buf));
	printk("\n");

	return 0;
}
