/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Board-attached exercise of the Pico de Gallo UART driver's RUNTIME
 * CONFIGURATION path: uart_configure() and uart_config_get(), which
 * zephyr/samples/uart_bridge never calls.
 *
 * ---------------------------------------------------------------------------
 * HARDWARE PRECONDITION
 * ---------------------------------------------------------------------------
 *
 * ONE board, with UART TX (GPIO 0) and UART RX (GPIO 1) PHYSICALLY SHORTED
 * together. There is no independent peer and no analyser. Without the jumper
 * every drain reports zero bytes; that is a correct report of missing wiring,
 * not a driver fault.
 *
 * ---------------------------------------------------------------------------
 * WHAT LOOPBACK CAN AND CANNOT PROVE
 * ---------------------------------------------------------------------------
 *
 * Loopback alone CANNOT verify framing. TX and RX are the same PL011, so a
 * stale register round-trips a byte exactly as cleanly as a correctly
 * reprogrammed one. Two discriminators do work, and this program runs both:
 *
 *   1. CONTENT MASKING at 7 data bits. The probe `ff 00 55 aa` returns
 *      `7f 00 55 2a` at 7 bits and unchanged at 8. Note that `0x00` and `0x55`
 *      are FIXED POINTS of the 7-bit mask and prove NOTHING on their own --
 *      the discriminating bytes are the two that change, 0xff -> 0x7f and
 *      0xaa -> 0x2a. They are kept in the probe anyway, as a control: a run
 *      where even the fixed points came back wrong is a wiring or transport
 *      fault, not a framing observation.
 *
 *   2. FRAME-LENGTH TIMING. At a fixed baud, wall-clock per character is
 *      proportional to bits per character: 8N1 = 10, 7N1 = 9, 8N2 = 11,
 *      8E1 = 11. A stale register reports 10 for all four.
 *
 * Odd-vs-even and mark-vs-space parity are UNVERIFIABLE on this rig. One PL011
 * drives both ends, so it validates against whatever it generated, and 8E1,
 * 8O1, 8M1 and 8S1 are all 11-bit frames and therefore indistinguishable by
 * timing too. This program does not attempt to distinguish them and contains no
 * assertion claiming to.
 *
 * ---------------------------------------------------------------------------
 * OUTPUT DISCIPLINE
 * ---------------------------------------------------------------------------
 *
 * Every result line begins with `PDG_BOARD_UART:` so an operator can grep it
 * out of Zephyr log noise:
 *
 *     ./build/zephyr/zephyr.exe 2>&1 | grep '^PDG_BOARD_UART:'
 *
 * Nothing prints inside a tight poll loop.
 *
 * PASS/FAIL lines are REPORTS, not gates. There is no zassert anywhere here:
 * the operator must see the raw bytes whichever way the comparison went, and
 * the program must always reach its final line so that later steps still run
 * and the board is always left restored.
 *
 * NEVER call a side-effecting function inside an assertion or a log-message
 * argument. A prior milestone put a uart_poll_in() inside a zassert message
 * argument and got an extra read out of it, because message varargs are
 * evaluated unconditionally. Every uart_poll_in() below is a statement.
 *
 * ---------------------------------------------------------------------------
 * BOUNDING
 * ---------------------------------------------------------------------------
 *
 * Every drain loop carries BOTH an attempt cap AND an absolute k_uptime_get()
 * deadline, and sleeps only when uart_poll_in() returned -1. Worst-case total
 * runtime is comfortably under 60 s; see the deadline arithmetic at
 * DRAIN_DEADLINE_MS.
 *
 * No burst exceeds TIMING_BLOCK_BYTES (64). The firmware TX path declares a
 * 60 s supervisor budget, and a large write at a very low baud can exceed it
 * and trigger a watchdog-forced device reset. At 9600 baud a 10-bit character
 * is ~1.042 ms, so 64 bytes is ~67 ms -- three orders of magnitude of margin,
 * while still far above USB jitter. Do not raise the block size and lower the
 * baud at the same time, and do not go below 9600.
 */

#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <zephyr/kernel.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>

#define TAG "PDG_BOARD_UART:"

/* ------------------------------------------------------------------------ */
/* Tunables. Compile-time constants so an operator can retune without         */
/* touching any logic below.                                                  */
/* ------------------------------------------------------------------------ */

/* Baseline line rate, matching app.overlay. */
#define BASE_BAUD 115200U

/*
 * Line rate for the frame-length timing sweep. LOW enough that wire time
 * dominates USB round-trip overhead. Do NOT go below 9600: see the bounding
 * note in the file header.
 */
#define TIMING_BAUD 9600U

/* Bytes transmitted per timing measurement. Keep at or below 256. */
#define TIMING_BLOCK_BYTES 64U

/* Filler byte for the timing block. 0x55 is an alternating pattern, so it
 * exercises every bit cell rather than idling the line high. It is also a
 * 7-bit fixed point, which is what we want here: the timing sweep measures
 * duration, and content masking is step 6's job, not this one's.
 */
#define TIMING_FILL 0x55U

/* Per-drain attempt cap. */
#define DRAIN_ATTEMPTS 4096U

/*
 * Per-drain absolute wall-clock deadline, in milliseconds.
 *
 * Worst case for the whole program: three 4-byte probe drains plus four timing
 * drains is seven drains, 7 * 3000 ms = 21 s, plus the transmit time of four
 * 64-byte blocks at 9600 baud (~0.07 s each) and a handful of configure round
 * trips. Comfortably under 60 s.
 */
#define DRAIN_DEADLINE_MS 3000

/* Sleep between failed poll attempts. Only ever reached on a -1 return. */
#define DRAIN_IDLE_MS 2

/*
 * Consecutive empty attempts that end a drain once at least one byte has
 * arrived. Without this a drain always burns its full deadline: the line goes
 * quiet after the last byte and nothing else distinguishes "done" from "slow".
 */
#define DRAIN_QUIET_ATTEMPTS 64U

/* Settling pause after a configure, before the next transmission. */
#define CONFIGURE_SETTLE_MS 50

/* Probe used for the content-masking discriminator, and its two expectations. */
static const uint8_t probe_8bit[] = {0xffU, 0x00U, 0x55U, 0xaaU};
static const uint8_t probe_7bit[] = {0x7fU, 0x00U, 0x55U, 0x2aU};
#define PROBE_LEN ((size_t)ARRAY_SIZE(probe_8bit))

/* Receive scratch. Sized for the largest single drain, the timing block. */
#define RX_CAPACITY TIMING_BLOCK_BYTES

/* ------------------------------------------------------------------------ */
/* Readable renderings of the uart_config enums.                              */
/*                                                                            */
/* Every one of these prints the RAW ORDINAL beside the name. The ordinals are */
/* not interchangeable across this driver's two enum families --               */
/* UART_CFG_STOP_BITS_1 is ordinal 1 while the wire's PDG_UART_STOP_BITS_1 is  */
/* 0 (see the BUILD_ASSERTs in zephyr/drivers/serial/pdg_uart.c) -- so an      */
/* operator comparing this output against a wire capture needs the number, and */
/* a reader skimming the log needs the name.                                   */
/* ------------------------------------------------------------------------ */

static const char *data_bits_name(uint8_t v)
{
	switch (v) {
	case UART_CFG_DATA_BITS_5:
		return "5";
	case UART_CFG_DATA_BITS_6:
		return "6";
	case UART_CFG_DATA_BITS_7:
		return "7";
	case UART_CFG_DATA_BITS_8:
		return "8";
	case UART_CFG_DATA_BITS_9:
		return "9";
	default:
		return "?";
	}
}

static const char *parity_name(uint8_t v)
{
	switch (v) {
	case UART_CFG_PARITY_NONE:
		return "none";
	case UART_CFG_PARITY_ODD:
		return "odd";
	case UART_CFG_PARITY_EVEN:
		return "even";
	case UART_CFG_PARITY_MARK:
		return "mark";
	case UART_CFG_PARITY_SPACE:
		return "space";
	default:
		return "?";
	}
}

static const char *stop_bits_name(uint8_t v)
{
	switch (v) {
	case UART_CFG_STOP_BITS_0_5:
		return "0.5";
	case UART_CFG_STOP_BITS_1:
		return "1";
	case UART_CFG_STOP_BITS_1_5:
		return "1.5";
	case UART_CFG_STOP_BITS_2:
		return "2";
	default:
		return "?";
	}
}

static const char *flow_ctrl_name(uint8_t v)
{
	switch (v) {
	case UART_CFG_FLOW_CTRL_NONE:
		return "none";
	case UART_CFG_FLOW_CTRL_RTS_CTS:
		return "rts-cts";
	case UART_CFG_FLOW_CTRL_DTR_DSR:
		return "dtr-dsr";
	case UART_CFG_FLOW_CTRL_RS485:
		return "rs485";
	default:
		return "?";
	}
}

/*
 * Print a uart_config, labelled.
 *
 * `label` names the step, so a grep for the prefix yields a readable
 * chronology rather than four indistinguishable config dumps.
 */
static void report_config(const char *label, const struct uart_config *cfg)
{
	printk("%s CONFIG %s baudrate=%u data_bits=%u(%s) parity=%u(%s) "
	       "stop_bits=%u(%s) flow_ctrl=%u(%s)\n",
	       TAG, label, (unsigned int)cfg->baudrate,
	       (unsigned int)cfg->data_bits, data_bits_name(cfg->data_bits),
	       (unsigned int)cfg->parity, parity_name(cfg->parity),
	       (unsigned int)cfg->stop_bits, stop_bits_name(cfg->stop_bits),
	       (unsigned int)cfg->flow_ctrl, flow_ctrl_name(cfg->flow_ctrl));
}

/*
 * uart_config_get() and report, in one place.
 *
 * Returns the driver's return code. On failure the config is NOT printed,
 * because its contents are then undefined: printing them would manufacture a
 * plausible-looking baseline out of stack garbage, which is worse than
 * printing nothing.
 */
static int show_config(const struct device *uart, const char *label)
{
	struct uart_config cfg;
	int rc;

	rc = uart_config_get(uart, &cfg);
	printk("%s CONFIG_GET %s rc=%d\n", TAG, label, rc);
	if (rc == 0) {
		report_config(label, &cfg);
	}

	return rc;
}

/* ------------------------------------------------------------------------ */
/* Transmit and drain primitives.                                             */
/* ------------------------------------------------------------------------ */

/*
 * Write `len` bytes with uart_poll_out().
 *
 * uart_poll_out() is void: a dropped byte cannot be reported here, and the
 * driver counts it privately. `len` is bounded by every caller; see the
 * bounding note in the file header.
 */
static void tx_block(const struct device *uart, const uint8_t *buf, size_t len)
{
	for (size_t i = 0U; i < len; i++) {
		uart_poll_out(uart, (unsigned char)buf[i]);
	}
}

/*
 * Drain up to `capacity` bytes into `out`.
 *
 * Bounded THREE ways, all of which must hold for the loop to continue: an
 * attempt cap, an absolute wall-clock deadline, and -- once at least one byte
 * has arrived -- a quiet-period cutoff. Sleeps only when uart_poll_in()
 * returned -1.
 *
 * uart_poll_in() returns exactly 0 or exactly -1, and writes through its
 * output pointer only on 0. -1 exposes no byte and does not distinguish an
 * idle line from a local refusal or a transport failure; the polling contract
 * has no room for an errno. This counts the attempt and keeps going.
 *
 * Returns the number of bytes received. Prints nothing: the caller reports.
 */
static size_t drain(const struct device *uart, uint8_t *out, size_t capacity)
{
	const int64_t deadline = k_uptime_get() + DRAIN_DEADLINE_MS;
	size_t received = 0U;
	unsigned int quiet = 0U;

	for (unsigned int attempt = 0U; attempt < DRAIN_ATTEMPTS; attempt++) {
		unsigned char c;

		if (received >= capacity) {
			break;
		}
		if (k_uptime_get() >= deadline) {
			break;
		}

		/*
		 * A STATEMENT, never an argument. See the header note on
		 * side effects inside log-message varargs.
		 */
		if (uart_poll_in(uart, &c) == 0) {
			out[received++] = (uint8_t)c;
			quiet = 0U;
			continue;
		}

		if (received > 0U) {
			quiet++;
			if (quiet >= DRAIN_QUIET_ATTEMPTS) {
				break;
			}
		}

		k_sleep(K_MSEC(DRAIN_IDLE_MS));
	}

	return received;
}

/* Print a received buffer as space-separated hex, on one line. */
static void report_bytes(const char *label, const uint8_t *buf, size_t len)
{
	printk("%s RX %s count=%u bytes:", TAG, label, (unsigned int)len);
	for (size_t i = 0U; i < len; i++) {
		printk(" %02x", buf[i]);
	}
	printk("\n");
}

/*
 * Discard anything still on the line, so one step's leftovers cannot be
 * counted as the next step's reply. Reports how much it threw away: a non-zero
 * count is itself a finding.
 */
static void flush_stale(const struct device *uart, const char *label)
{
	uint8_t scratch[RX_CAPACITY];
	size_t n;

	n = drain(uart, scratch, sizeof(scratch));
	if (n != 0U) {
		printk("%s FLUSH %s discarded=%u stale byte(s)\n", TAG, label,
		       (unsigned int)n);
	}
}

/*
 * Send the 4-byte probe and report what came back, comparing against
 * `expect`.
 *
 * A REPORT, not a gate: it prints PASS or FAIL and returns, so the operator
 * sees the raw bytes either way and every later step still runs.
 */
static void probe_and_report(const struct device *uart, const char *label,
			     const uint8_t *expect)
{
	uint8_t rx[RX_CAPACITY];
	size_t n;
	bool match;

	flush_stale(uart, label);

	printk("%s PROBE %s tx: %02x %02x %02x %02x\n", TAG, label,
	       probe_8bit[0], probe_8bit[1], probe_8bit[2], probe_8bit[3]);
	tx_block(uart, probe_8bit, PROBE_LEN);

	n = drain(uart, rx, sizeof(rx));
	report_bytes(label, rx, n);

	match = (n == PROBE_LEN);
	for (size_t i = 0U; match && i < PROBE_LEN; i++) {
		if (rx[i] != expect[i]) {
			match = false;
		}
	}

	printk("%s RESULT %s %s expected: %02x %02x %02x %02x\n", TAG, label,
	       match ? "PASS" : "FAIL", expect[0], expect[1], expect[2],
	       expect[3]);
}

/* ------------------------------------------------------------------------ */
/* Configuration helper.                                                      */
/* ------------------------------------------------------------------------ */

/*
 * Apply one configuration and report the return code.
 *
 * Flow control is always UART_CFG_FLOW_CTRL_NONE: the bridge exposes no modem
 * control lines and the driver refuses anything else.
 */
static int apply_config(const struct device *uart, const char *label,
			uint32_t baud, uint8_t data_bits, uint8_t parity,
			uint8_t stop_bits)
{
	struct uart_config cfg = {
		.baudrate = baud,
		.data_bits = data_bits,
		.parity = parity,
		.stop_bits = stop_bits,
		.flow_ctrl = UART_CFG_FLOW_CTRL_NONE,
	};
	int rc;

	rc = uart_configure(uart, &cfg);
	printk("%s CONFIGURE %s rc=%d requested: baudrate=%u data_bits=%u(%s) "
	       "parity=%u(%s) stop_bits=%u(%s)\n",
	       TAG, label, rc, (unsigned int)baud, (unsigned int)data_bits,
	       data_bits_name(data_bits), (unsigned int)parity,
	       parity_name(parity), (unsigned int)stop_bits,
	       stop_bits_name(stop_bits));

	if (rc == 0) {
		k_sleep(K_MSEC(CONFIGURE_SETTLE_MS));
	}

	return rc;
}

/* ------------------------------------------------------------------------ */
/* Frame-length timing sweep.                                                 */
/* ------------------------------------------------------------------------ */

struct timing_case {
	const char *name;
	uint8_t data_bits;
	uint8_t parity;
	uint8_t stop_bits;
	unsigned int bits_per_char; /* start + data + parity + stop */
};

/*
 * Four configurations spanning three distinct frame lengths at one baud.
 *
 * A character is one start bit, N data bits, an optional parity bit, and the
 * stop bits:
 *
 *   8N1 -> 1 + 8 + 0 + 1 = 10
 *   7N1 -> 1 + 7 + 0 + 1 =  9
 *   8N2 -> 1 + 8 + 0 + 2 = 11
 *   8E1 -> 1 + 8 + 1 + 1 = 11
 *
 * A device whose registers never changed reports 10 for all four. 8N2 and 8E1
 * are both 11 and are NOT expected to differ from each other -- they are two
 * independent routes to the same frame length, which is the point: agreeing
 * with each other while differing from 8N1 is stronger evidence than either
 * alone.
 */
static const struct timing_case timing_cases[] = {
	{"8N1", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE, UART_CFG_STOP_BITS_1, 10U},
	{"7N1", UART_CFG_DATA_BITS_7, UART_CFG_PARITY_NONE, UART_CFG_STOP_BITS_1, 9U},
	{"8N2", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE, UART_CFG_STOP_BITS_2, 11U},
	{"8E1", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_EVEN, UART_CFG_STOP_BITS_1, 11U},
};

#define TIMING_CASES ((size_t)ARRAY_SIZE(timing_cases))

struct timing_result {
	int configure_rc;
	size_t received;
	int64_t tx_ms;    /* transmit burst only */
	int64_t total_ms; /* transmit plus loopback drain */
};

static struct timing_result timing_results[TIMING_CASES];

/*
 * Measure one configuration.
 *
 * TWO clocks are reported and neither alone is trustworthy. `tx_ms` covers the
 * uart_poll_out() burst, which is one USB round trip PER BYTE and therefore
 * carries a large constant that has nothing to do with the wire. `total_ms`
 * additionally covers the loopback return, so it contains the wire time twice
 * over -- but also the poll_in overhead. Frame-length differences show up as a
 * RATIO between configurations, where the common overhead largely cancels;
 * neither absolute figure means much on its own.
 *
 * Nothing is asserted. Ratio interpretation is the operator's job.
 */
static void measure(const struct device *uart, size_t idx)
{
	static uint8_t block[TIMING_BLOCK_BYTES];
	const struct timing_case *tc = &timing_cases[idx];
	struct timing_result *out = &timing_results[idx];
	uint8_t rx[RX_CAPACITY];
	int64_t t0, t1, t2;

	for (size_t i = 0U; i < TIMING_BLOCK_BYTES; i++) {
		block[i] = TIMING_FILL;
	}

	out->configure_rc = apply_config(uart, tc->name, TIMING_BAUD,
					 tc->data_bits, tc->parity,
					 tc->stop_bits);
	if (out->configure_rc != 0) {
		out->received = 0U;
		out->tx_ms = 0;
		out->total_ms = 0;
		return;
	}

	flush_stale(uart, tc->name);

	t0 = k_uptime_get();
	tx_block(uart, block, TIMING_BLOCK_BYTES);
	t1 = k_uptime_get();
	out->received = drain(uart, rx, TIMING_BLOCK_BYTES);
	t2 = k_uptime_get();

	out->tx_ms = t1 - t0;
	out->total_ms = t2 - t0;

	printk("%s TIMING %s bits_per_char=%u bytes=%u received=%u tx_ms=%lld "
	       "total_ms=%lld tx_us_per_byte=%lld total_us_per_byte=%lld\n",
	       TAG, tc->name, tc->bits_per_char,
	       (unsigned int)TIMING_BLOCK_BYTES, (unsigned int)out->received,
	       (long long)out->tx_ms, (long long)out->total_ms,
	       (long long)((out->tx_ms * 1000) / (int64_t)TIMING_BLOCK_BYTES),
	       (long long)((out->total_ms * 1000) / (int64_t)TIMING_BLOCK_BYTES));
}

/* ------------------------------------------------------------------------ */
/* Steps.                                                                     */
/* ------------------------------------------------------------------------ */

/*
 * Step 9: local validation refusals.
 *
 * Both are SAFE: pdg_uart_validate_config() rejects them before any RPC is
 * issued, so neither reaches the bus and neither can wedge the board.
 *
 * The two expected codes DIFFER, and the driver's own text says why. A
 * baudrate of 0 "names no line rate" and yields -EINVAL. An unsupported
 * data-bits value is a capability refusal -- the bridge supports only 7 and 8
 * -- and yields -ENOTSUP. Treating both as -EINVAL would be wrong; the lines
 * below print the code and the expectation separately so a drift in either is
 * visible without re-reading this comment.
 */
static void check_invalid_configs(const struct device *uart)
{
	struct uart_config cfg;
	int rc;

	cfg = (struct uart_config){
		.baudrate = 0U,
		.data_bits = UART_CFG_DATA_BITS_8,
		.parity = UART_CFG_PARITY_NONE,
		.stop_bits = UART_CFG_STOP_BITS_1,
		.flow_ctrl = UART_CFG_FLOW_CTRL_NONE,
	};
	rc = uart_configure(uart, &cfg);
	printk("%s INVALID baudrate_zero rc=%d expected=%d %s\n", TAG, rc,
	       -EINVAL, (rc == -EINVAL) ? "PASS" : "FAIL");

	/*
	 * 9 data bits is a real Zephyr enumerator the bridge cannot do, so it
	 * exercises the driver's capability branch rather than its
	 * unknown-value catch-all.
	 */
	cfg = (struct uart_config){
		.baudrate = BASE_BAUD,
		.data_bits = UART_CFG_DATA_BITS_9,
		.parity = UART_CFG_PARITY_NONE,
		.stop_bits = UART_CFG_STOP_BITS_1,
		.flow_ctrl = UART_CFG_FLOW_CTRL_NONE,
	};
	rc = uart_configure(uart, &cfg);
	printk("%s INVALID data_bits_9 rc=%d expected=%d %s\n", TAG, rc,
	       -ENOTSUP, (rc == -ENOTSUP) ? "PASS" : "FAIL");

	/* And a value that is not a valid enumerator at all. */
	cfg = (struct uart_config){
		.baudrate = BASE_BAUD,
		.data_bits = 0x7fU,
		.parity = UART_CFG_PARITY_NONE,
		.stop_bits = UART_CFG_STOP_BITS_1,
		.flow_ctrl = UART_CFG_FLOW_CTRL_NONE,
	};
	rc = uart_configure(uart, &cfg);
	printk("%s INVALID data_bits_out_of_range rc=%d expected=%d %s\n", TAG,
	       rc, -ENOTSUP, (rc == -ENOTSUP) ? "PASS" : "FAIL");
}

static void print_summary(void)
{
	printk("%s SUMMARY frame-length sweep at %u baud, %u bytes per block\n",
	       TAG, (unsigned int)TIMING_BAUD, (unsigned int)TIMING_BLOCK_BYTES);
	printk("%s SUMMARY %-5s %-14s %-9s %-9s %-9s %s\n", TAG, "config",
	       "bits_per_char", "rc", "received", "tx_ms", "total_ms");
	for (size_t i = 0U; i < TIMING_CASES; i++) {
		printk("%s SUMMARY %-5s %-14u %-9d %-9u %-9lld %lld\n", TAG,
		       timing_cases[i].name, timing_cases[i].bits_per_char,
		       timing_results[i].configure_rc,
		       (unsigned int)timing_results[i].received,
		       (long long)timing_results[i].tx_ms,
		       (long long)timing_results[i].total_ms);
	}
	printk("%s SUMMARY interpret the RATIOS, not the absolute values; "
	       "8N2 and 8E1 are both 11-bit frames and should agree with each "
	       "other while exceeding 8N1\n", TAG);
	printk("%s SUMMARY odd-vs-even and mark-vs-space parity are NOT "
	       "distinguishable on a single-PL011 loopback and are not "
	       "attempted\n", TAG);
}

int main(void)
{
	const struct device *uart = DEVICE_DT_GET(DT_NODELABEL(pdg_uart0));
	int rc;

	printk("%s START base_baud=%u timing_baud=%u block_bytes=%u\n", TAG,
	       (unsigned int)BASE_BAUD, (unsigned int)TIMING_BAUD,
	       (unsigned int)TIMING_BLOCK_BYTES);
	printk("%s NOTE requires UART TX and RX physically shorted (loopback)\n",
	       TAG);

	/* Step 1: readiness. */
	if (!device_is_ready(uart)) {
		printk("%s FATAL device not ready: check that the board is "
		       "attached and that serial-number names it (see "
		       "app.overlay)\n", TAG);
		printk("%s DONE rc=%d\n", TAG, -ENODEV);
		return -ENODEV;
	}
	printk("%s READY device=%s\n", TAG, uart->name);

	/* Step 2: baseline configuration. */
	rc = show_config(uart, "baseline");
	if (rc != 0) {
		/*
		 * Not fatal on purpose. -ENOSYS here means
		 * CONFIG_UART_USE_RUNTIME_CONFIGURE is off, which is the single
		 * most likely misconfiguration of this application, and the
		 * operator is better served by the remaining steps failing
		 * loudly and consistently than by an early exit.
		 */
		printk("%s WARN baseline uart_config_get failed; if rc=%d the "
		       "build is missing CONFIG_UART_USE_RUNTIME_CONFIGURE\n",
		       TAG, -ENOSYS);
	}

	/* Step 3: loopback at the baseline. */
	probe_and_report(uart, "baseline_8N1", probe_8bit);

	/* Steps 4 and 5: switch to 7N1 and re-read. */
	rc = apply_config(uart, "7N1", BASE_BAUD, UART_CFG_DATA_BITS_7,
			  UART_CFG_PARITY_NONE, UART_CFG_STOP_BITS_1);
	(void)show_config(uart, "after_7N1");

	/*
	 * Step 6: content-masking discriminator.
	 *
	 * Only meaningful if the configure succeeded. If it did not, run the
	 * probe anyway and say so: the bytes still tell the operator whether
	 * the line is alive, which is the more useful diagnostic at that
	 * point.
	 */
	if (rc != 0) {
		printk("%s WARN 7N1 configure failed rc=%d; the masking probe "
		       "below tests an UNKNOWN configuration\n", TAG, rc);
	}
	probe_and_report(uart, "masked_7N1", probe_7bit);

	/* Step 7: back to 8N1 and confirm the mask is gone. */
	rc = apply_config(uart, "restore_8N1", BASE_BAUD, UART_CFG_DATA_BITS_8,
			  UART_CFG_PARITY_NONE, UART_CFG_STOP_BITS_1);
	(void)show_config(uart, "after_restore_8N1");
	if (rc != 0) {
		printk("%s WARN restore to 8N1 failed rc=%d; the probe below "
		       "tests an UNKNOWN configuration\n", TAG, rc);
	}
	probe_and_report(uart, "restored_8N1", probe_8bit);

	/* Step 8: frame-length timing sweep. */
	for (size_t i = 0U; i < TIMING_CASES; i++) {
		measure(uart, i);
	}
	print_summary();

	/* Step 9: local validation refusals. */
	check_invalid_configs(uart);

	/* Step 10: restore the baseline and report it. */
	rc = apply_config(uart, "final_115200_8N1", BASE_BAUD,
			  UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
			  UART_CFG_STOP_BITS_1);
	flush_stale(uart, "final");
	(void)show_config(uart, "final");

	printk("%s DONE rc=%d\n", TAG, rc);
	return rc;
}
