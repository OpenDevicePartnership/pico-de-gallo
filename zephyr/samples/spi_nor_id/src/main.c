/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Identifies a JEDEC SPI NOR flash attached to a Pico de Gallo bridge, using
 * the Zephyr SPI API from native_sim.
 *
 * This sample is deliberately read-only. It issues only RDID, RDSR, RDSFDP
 * and READ, and never sends a write-enable, program, erase or write-status
 * opcode, so it is safe to point at a part whose contents you care about.
 *
 * It talks to the bus directly rather than through the jedec,spi-nor driver.
 * That driver is a fine thing to use in a real application, but its
 * configuration path issues WREN/WRSR to clear block protection, which is a
 * write. Driving the opcodes here keeps every byte on the wire visible in
 * this file.
 */

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/spi.h>
#include <zephyr/sys/printk.h>

#include <string.h>

/* Read-only JEDEC SPI NOR opcodes. */
#define NOR_CMD_RDID   0x9FU /* JEDEC ID: manufacturer, type, capacity */
#define NOR_CMD_RDSR   0x05U /* Read status register */
#define NOR_CMD_READ   0x03U /* Read data, 3-byte address */
#define NOR_CMD_RDSFDP 0x5AU /* Read SFDP, 3-byte address + 1 dummy byte */

#define NOR_DUMP_LEN 16U

static const struct device *const spi_bus = DEVICE_DT_GET(DT_NODELABEL(pdg_spi0));

/*
 * The Pico de Gallo SPI driver takes chip-select as an index, not a GPIO
 * number: .slave selects one of the four user GPIOs (0 -> GPIO 8,
 * 1 -> GPIO 9, 2 -> GPIO 10, 3 -> GPIO 11). Leaving .cs zeroed is
 * deliberate -- the driver rejects GPIO-controlled chip select with
 * -ENOTSUP.
 */
static const struct spi_config nor_cfg = {
	.frequency = DT_PROP_OR(DT_NODELABEL(pdg_spi0), clock_frequency, 10000000),
	.operation = SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | SPI_TRANSFER_MSB,
	.slave = 0,
};

/*
 * One full-duplex transaction. The bridge clocks MAX(tx,rx) bytes, so the
 * caller sizes a single buffer covering opcode, address, any dummy byte and
 * the reply, then reads the reply back from the same offsets.
 */
static int nor_xfer(uint8_t *buf, size_t len)
{
	const struct spi_buf tx = {.buf = buf, .len = len};
	const struct spi_buf rx = {.buf = buf, .len = len};
	const struct spi_buf_set tx_set = {.buffers = &tx, .count = 1};
	const struct spi_buf_set rx_set = {.buffers = &rx, .count = 1};

	return spi_transceive(spi_bus, &nor_cfg, &tx_set, &rx_set);
}

static void hexdump(const uint8_t *data, size_t len)
{
	for (size_t i = 0; i < len; i++) {
		printk("%02X%s", data[i], (i + 1U == len) ? "" : " ");
	}
}

int main(void)
{
	uint8_t buf[4U + NOR_DUMP_LEN];
	int ret;

	if (!device_is_ready(spi_bus)) {
		printk("SPI bus not ready (Pico de Gallo bridge connected?)\n");
		return 0;
	}

	/* JEDEC ID: 1 opcode byte, then 3 bytes of reply. */
	memset(buf, 0, 4U);
	buf[0] = NOR_CMD_RDID;
	ret = nor_xfer(buf, 4U);
	if (ret < 0) {
		printk("RDID failed: %d\n", ret);
		return 0;
	}

	printk("JEDEC id: mfr=0x%02X type=0x%02X cap=0x%02X", buf[1], buf[2], buf[3]);
	if (buf[3] >= 0x10U && buf[3] <= 0x1FU) {
		printk(" (%u KiB)", (unsigned int)(1U << (buf[3] - 10U)));
	}
	printk("\n");

	/* Status register: 1 opcode byte, then 1 byte of reply. */
	memset(buf, 0, 2U);
	buf[0] = NOR_CMD_RDSR;
	ret = nor_xfer(buf, 2U);
	if (ret < 0) {
		printk("RDSR failed: %d\n", ret);
		return 0;
	}
	printk("status:   0x%02X (WIP=%u WEL=%u)\n", buf[1], buf[1] & 1U, (buf[1] >> 1) & 1U);

	/* SFDP: opcode, 3 address bytes, 1 dummy byte, then the reply. */
	memset(buf, 0, 5U + 4U);
	buf[0] = NOR_CMD_RDSFDP;
	ret = nor_xfer(buf, 5U + 4U);
	if (ret < 0) {
		printk("RDSFDP failed: %d\n", ret);
		return 0;
	}
	printk("SFDP:     ");
	hexdump(&buf[5], 4U);
	if (memcmp(&buf[5], "SFDP", 4) == 0) {
		printk("  (\"SFDP\" signature OK)");
	}
	printk("\n");

	/* Read data: opcode, 3 address bytes, then the reply. */
	memset(buf, 0, sizeof(buf));
	buf[0] = NOR_CMD_READ;
	ret = nor_xfer(buf, 4U + NOR_DUMP_LEN);
	if (ret < 0) {
		printk("READ failed: %d\n", ret);
		return 0;
	}
	printk("@000000:  ");
	hexdump(&buf[4], NOR_DUMP_LEN);
	printk("\n");

	return 0;
}
