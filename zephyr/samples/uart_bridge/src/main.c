/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Small sample for the Pico de Gallo UART controller. Sends a fixed greeting
 * with uart_poll_out(), then makes a bounded number of uart_poll_in()
 * attempts and prints whatever arrived, before returning.
 *
 * The driver implements only the polling API. uart_poll_out() is void and
 * makes one uart/write round trip per byte; uart_poll_in() returns exactly 0
 * or exactly -1, and writes through its output pointer only when it returns 0.
 * -1 exposes no byte. It does not distinguish an idle line from a local
 * refusal or a transport failure -- uart_poll_in()'s contract has no room
 * for an errno, and the driver latches the real one privately. This sample
 * counts the attempt and keeps going.
 *
 * Everything here is bounded at compile time. There is no unconditional loop:
 * the program always reaches `return 0`, which matters because the sample is
 * built for native_sim and a hung binary would wedge whoever ran it.
 */

#include <errno.h>

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>

/* Greeting sent byte-by-byte. sizeof() - 1 drops the NUL terminator. */
static const char greeting[] = "Hello from Pico de Gallo!\r\n";
#define GREETING_LEN (sizeof(greeting) - 1U)

/* Receive attempts before giving up. Fixed at compile time on purpose. */
#define RX_ATTEMPTS 64U

/* Pause between receive attempts, so the cap spans a usable wall-clock span. */
#define RX_INTERVAL_MS 10U

int main(void)
{
	const struct device *uart = DEVICE_DT_GET(DT_NODELABEL(pdg_uart0));
	unsigned char rx[RX_ATTEMPTS];
	size_t received = 0U;

	if (!device_is_ready(uart)) {
		printk("Pico de Gallo UART not ready: check that the board is "
		       "attached and that serial-number in app.overlay names "
		       "it\n");
		return -ENODEV;
	}

	printk("Sending %u bytes\n", (unsigned int)GREETING_LEN);
	for (size_t i = 0U; i < GREETING_LEN; i++) {
		/*
		 * void return: a dropped byte cannot be reported here. The
		 * driver counts it privately and deliberately never logs,
		 * since logging through this same UART would recurse.
		 */
		uart_poll_out(uart, (unsigned char)greeting[i]);
	}

	printk("Polling for input, up to %u attempts\n",
	       (unsigned int)RX_ATTEMPTS);
	for (unsigned int attempt = 0U; attempt < RX_ATTEMPTS; attempt++) {
		unsigned char c;

		if (uart_poll_in(uart, &c) == 0) {
			rx[received++] = c;
			continue;
		}

		/*
		 * -1: no byte exposed. Could be an idle line, a local
		 * refusal, or a transport failure -- the polling contract
		 * cannot say which. Not fatal; retry within the bound.
		 */
		k_sleep(K_MSEC(RX_INTERVAL_MS));
	}

	if (received == 0U) {
		printk("No bytes received in %u attempts\n",
		       (unsigned int)RX_ATTEMPTS);
		return 0;
	}

	printk("Received %u bytes:", (unsigned int)received);
	for (size_t i = 0U; i < received; i++) {
		printk(" %02x", rx[i]);
	}
	printk("\n");

	return 0;
}
