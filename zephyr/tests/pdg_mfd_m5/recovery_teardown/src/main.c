/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * M5 phase 5 -- recovery, final-state report and teardown (T7).
 *
 * Two roles, one image:
 *
 *   normal teardown  -- runs last in a healthy sequence. The subscription reset
 *                       count MUST be 0; a nonzero count is direct evidence
 *                       that T4's cleanup did not run, and is a T4 DEFECT
 *                       REPORT, not a housekeeping detail (test design §6.4).
 *   recovery         -- run with --allow-nonzero-reset after an ABNORMAL
 *                       acceptance or loopback exit. Here SPI init succeeding
 *                       at all is the test: it proves pin 2 is no longer owned
 *                       by a firmware monitor.
 *
 * There is NO GPIO mode-query FFI. The report therefore distinguishes an
 * ACKNOWLEDGED COMMANDED mode from directly queried state, and prints `unknown`
 * where neither exists. It never guesses (acceptance spec §10).
 *
 * Process-local latch, lock and owner die with the process; physical firmware
 * pin mode and level do not. Never infer recovery from a fresh process merely
 * lacking a latch -- which is why this image requires an independent witness
 * reading rather than trusting its own clean start.
 */

#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/gpio.h>
#include <zephyr/drivers/spi.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

#include "posix_board_if.h"
#include "cmdline.h"
#include "posix_native_task.h"

#include "pdg_mfd.h"
#include "m5_bottom.h"

#define M5_USER DT_PATH(zephyr_user)

static const struct device *const m5_spi = DEVICE_DT_GET(DT_NODELABEL(pdg_spi0));
static const struct gpio_dt_spec m5_witness = GPIO_DT_SPEC_GET(M5_USER, m5_witness_gpios);

static bool m5_allow_nonzero_reset;
static bool m5_power_cycled;
static bool m5_attest_mosi_miso;
static bool m5_attest_gpio_jumper;

static void m5_add_options(void)
{
	static struct args_struct_t m5_options[] = {
		{
			.is_switch = true,
			.option = "allow-nonzero-reset",
			.type = 'b',
			.dest = (void *)&m5_allow_nonzero_reset,
			.descript = "Recovery role: tolerate a nonzero subscription reset "
				    "count. Never pass this in a normal teardown -- a "
				    "nonzero count there is a T4 defect report."
		},
		{
			.is_switch = true,
			.option = "power-cycled",
			.type = 'b',
			.dest = (void *)&m5_power_cycled,
			.descript = "Record that the board was power-cycled during this run "
				    "(aggregate verdict teardown.power_cycle_occurred)."
		},
		{
			.is_switch = true,
			.option = "attest-mosi-miso",
			.type = 'b',
			.dest = (void *)&m5_attest_mosi_miso,
			.descript = "Operator attestation: the MOSI<->MISO jumper is still "
				    "fitted. REQUIRED."
		},
		{
			.is_switch = true,
			.option = "attest-gpio-jumper",
			.type = 'b',
			.dest = (void *)&m5_attest_gpio_jumper,
			.descript = "Operator attestation: the GPIO 2<->3 jumper is still "
				    "fitted. REQUIRED."
		},
		ARG_TABLE_ENDMARKER
	};

	native_add_command_line_opts(m5_options);
}

NATIVE_TASK(m5_add_options, PRE_BOOT_1, 10);

int main(void)
{
	const struct device *const parent = DEVICE_DT_GET(DT_NODELABEL(pdg0));
	uint8_t reset_count = 0U;
	uint32_t frequency = 0U;
	bool phase = false;
	bool polarity = false;
	void *ctx;
	int level;
	int ret;

	printk("M5_TEARDOWN_BEGIN\n");

	/*
	 * Attestation is a precondition, not a footnote. Every electrical
	 * conclusion in this milestone assumes both jumpers were fitted for the
	 * whole run, and no software check can observe a jumper pulled out
	 * between phases.
	 */
	if (!m5_attest_mosi_miso || !m5_attest_gpio_jumper) {
		printk("M5_TEARDOWN_FAIL reason=missing-operator-attestation "
		       "mosi_miso=%d gpio_jumper=%d\n",
		       (int)m5_attest_mosi_miso, (int)m5_attest_gpio_jumper);
		posix_exit(1);
	}

	if (!device_is_ready(parent)) {
		printk("M5_TEARDOWN_FAIL reason=parent-not-ready device=%s\n", parent->name);
		posix_exit(1);
	}
	ctx = pdg_mfd_ctx(parent);
	if (ctx == NULL) {
		printk("M5_TEARDOWN_FAIL reason=null-context device=%s\n", parent->name);
		posix_exit(1);
	}

	/* Step 1: explicit reset, count reported verbatim. */
	ret = m5_bottom_reset_subscriptions(ctx, &reset_count);
	if (ret != 0) {
		printk("M5_TEARDOWN_FAIL reason=reset-endpoint errno=%d\n", ret);
		posix_exit(1);
	}
	printk("M5_TEARDOWN_SUBSCRIPTIONS_RESET=%u\n", reset_count);
	if (reset_count != 0U && !m5_allow_nonzero_reset) {
		printk("M5_TEARDOWN_FAIL reason=nonzero-subscription-reset count=%u "
		       "note=T4 cleanup did not run; fault_latch must be reported FAIL\n",
		       reset_count);
		posix_exit(1);
	}

	/*
	 * Step 2. SPI readiness is load-bearing in the recovery role: pdg_spi_init()
	 * configures every cs-gpios pin as an inactive output, so a ready SPI
	 * device proves pin 2 is not monitored AND parks it physically HIGH.
	 */
	if (!device_is_ready(m5_spi)) {
		printk("M5_TEARDOWN_FAIL reason=spi-not-ready device=%s "
		       "note=pin 2 may still be monitored; power-cycle and restart at reset\n",
		       m5_spi->name);
		posix_exit(1);
	}
	if (!device_is_ready(m5_witness.port)) {
		printk("M5_TEARDOWN_FAIL reason=gpio-port-not-ready port=%s\n",
		       m5_witness.port->name);
		posix_exit(1);
	}

	/* Step 3: independent witness. */
	ret = gpio_pin_configure_dt(&m5_witness, GPIO_INPUT | GPIO_PULL_UP);
	if (ret != 0) {
		printk("M5_TEARDOWN_FAIL reason=witness-configure errno=%d\n", ret);
		posix_exit(1);
	}
	level = gpio_pin_get_dt(&m5_witness);
	if (level < 0) {
		printk("M5_TEARDOWN_FAIL reason=witness-read errno=%d\n", level);
		posix_exit(1);
	}
	printk("M5_TEARDOWN_PIN2_LEVEL=%s\n", (level == 1) ? "witnessed HIGH" : "LOW");
	if (level != 1) {
		printk("M5_TEARDOWN_FAIL reason=chip-select-still-asserted\n");
		posix_exit(1);
	}

	/* Step 4: directly queried SPI state. */
	ret = m5_bottom_spi_get_config(ctx, &frequency, &phase, &polarity);
	if (ret != 0) {
		printk("M5_TEARDOWN_SPI_MODE=unknown\n");
		printk("M5_TEARDOWN_SPI_FREQUENCY_HZ=0\n");
		printk("M5_TEARDOWN_FAIL reason=spi-get-config errno=%d\n", ret);
		posix_exit(1);
	}
	printk("M5_TEARDOWN_SPI_MODE=mode%u\n",
	       (unsigned int)((polarity ? 2U : 0U) + (phase ? 1U : 0U)));
	printk("M5_TEARDOWN_SPI_FREQUENCY_HZ=%u\n", frequency);

	/*
	 * Step 5. Both of these are ACKNOWLEDGED COMMANDED modes, not queries.
	 * Pin 2's mode rests on pdg_spi_init() having acknowledged
	 * GPIO_OUTPUT_INACTIVE plus the independent witness HIGH above; pin 3's
	 * rests on this process's own acknowledged configure. There is no GPIO
	 * mode-query FFI, so neither can be read back, and neither is claimed as
	 * a direct observation.
	 */
	printk("M5_TEARDOWN_PIN2_MODE=acknowledged ExplicitOutput\n");
	printk("M5_TEARDOWN_PIN3_MODE_PULL=acknowledged Input/PullUp\n");

	/* Steps 6 and 7. */
	printk("M5_TEARDOWN_POWER_CYCLE_OCCURRED=%s\n", m5_power_cycled ? "true" : "false");
	printk("M5_TEARDOWN_MOSI_MISO_JUMPER_FITTED=true\n");
	printk("M5_TEARDOWN_GPIO2_GPIO3_JUMPER_FITTED=true\n");

	printk("M5_TEARDOWN_PASS\n");

	posix_exit(0);

	return 0;
}
