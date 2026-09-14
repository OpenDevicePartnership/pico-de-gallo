/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Runtime suite for the Pico de Gallo Zephyr UART driver, executed against the
 * recording fake in ../pdg_uart_fake_bottom.c.
 *
 * DELIBERATELY ORDER-DEPENDENT. The driver's transport latch is permanent and
 * cannot be re-armed, so the latch tests own their instances outright and must
 * run after the tests that observe those instances healthy. CONFIG_ZTEST_SHUFFLE
 * must therefore stay off; prj.conf deliberately does not set it.
 */

#include <string.h>

#include <zephyr/ztest.h>
#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/irq_offload.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/util.h>

#include "pdg_fake_bottom.h"
#include "pdg_uart_fake_bottom.h"

/* Neutral PDG_UART_* values, restated as literals: zephyr/drivers/serial is not
 * on the app's include path (see this suite's CMakeLists.txt).
 */
#define EXP_DATA_BITS_5 0U
#define EXP_DATA_BITS_6 1U
#define EXP_DATA_BITS_7 2U
#define EXP_DATA_BITS_8 3U
#define EXP_PARITY_NONE 0U
#define EXP_PARITY_ODD 1U
#define EXP_PARITY_EVEN 2U
#define EXP_PARITY_MARK 3U
#define EXP_PARITY_SPACE 4U
#define EXP_STOP_BITS_1 0U
#define EXP_STOP_BITS_2 1U

/* The refill size the driver must request, and the read timeout it must pass.
 * Both are driver-private policy (PDG_UART_RX_BUFFER_SIZE,
 * PDG_UART_READ_TIMEOUT_MS); changing either must break a test.
 */
#define EXP_REFILL_COUNT 1014U
#define EXP_REFILL_TIMEOUT_MS 0U

/* Number of enabled UART children in fake.overlay. */
#define UART_CHILD_COUNT 5

#define DEV_GENERAL DEVICE_DT_GET(DT_NODELABEL(pdg_uart_general))
#define DEV_BACKOFF DEVICE_DT_GET(DT_NODELABEL(pdg_uart_backoff))
#define DEV_RX_LATCH DEVICE_DT_GET(DT_NODELABEL(pdg_uart_rx_latch))
#define DEV_TX_LATCH DEVICE_DT_GET(DT_NODELABEL(pdg_uart_tx_latch))
#define DEV_CFG_FAIL DEVICE_DT_GET(DT_NODELABEL(pdg_uart_cfg_fail))

#define BAUD_GENERAL 115200U
#define BAUD_BACKOFF 57600U
#define BAUD_RX_LATCH 38400U
#define BAUD_TX_LATCH 19200U
#define BAUD_CFG_FAIL 9600U

/* ---------------------------------------------------------------------------
 * Shared helpers. Non-inline and defined once so later tests reuse them.
 * ------------------------------------------------------------------------ */

/* A test-start recorder snapshot. Every runtime assertion is a delta against
 * one of these rather than an absolute, so a test cannot pass on evidence some
 * earlier test produced.
 */
struct pdg_uart_snap {
	int has_uart;
	int set_config;
	int read;
	int write;
	int close;
};

void pdg_uart_snapshot(struct pdg_uart_snap *s)
{
	s->has_uart = pdg_uart_fake_has_uart_count();
	s->set_config = pdg_uart_fake_set_config_count();
	s->read = pdg_uart_fake_read_count();
	s->write = pdg_uart_fake_write_count();
	s->close = pdg_fake_close_count();
}

/* Assert the four UART recorders and the parent close counter moved by exactly
 * the named deltas since `s`, and that no bounded log overflowed.
 */
void pdg_uart_assert_delta(const struct pdg_uart_snap *s, int d_has_uart, int d_set_config,
			   int d_read, int d_write, const char *what)
{
	zassert_false(pdg_uart_fake_overflowed(),
		      "%s: a bounded recorder overflowed, so no log below can be "
		      "trusted (expected overflowed()==0, got %d)",
		      what, pdg_uart_fake_overflowed());
	zassert_equal(pdg_uart_fake_has_uart_count() - s->has_uart, d_has_uart,
		      "%s: has_uart delta expected %d, got %d", what, d_has_uart,
		      pdg_uart_fake_has_uart_count() - s->has_uart);
	zassert_equal(pdg_uart_fake_set_config_count() - s->set_config, d_set_config,
		      "%s: set_config delta expected %d, got %d", what, d_set_config,
		      pdg_uart_fake_set_config_count() - s->set_config);
	zassert_equal(pdg_uart_fake_read_count() - s->read, d_read,
		      "%s: read delta expected %d, got %d", what, d_read,
		      pdg_uart_fake_read_count() - s->read);
	zassert_equal(pdg_uart_fake_write_count() - s->write, d_write,
		      "%s: write delta expected %d, got %d", what, d_write,
		      pdg_uart_fake_write_count() - s->write);
	zassert_equal(pdg_fake_close_count() - s->close, 0,
		      "%s: a child closed the borrowed parent context; close delta "
		      "expected 0, got %d",
		      what, pdg_fake_close_count() - s->close);
}

/* The 8N1 baseline every child boots with, at `baud`. */
struct uart_config pdg_uart_baseline_config(uint32_t baud)
{
	struct uart_config cfg = {
		.baudrate = baud,
		.parity = UART_CFG_PARITY_NONE,
		.stop_bits = UART_CFG_STOP_BITS_1,
		.data_bits = UART_CFG_DATA_BITS_8,
		.flow_ctrl = UART_CFG_FLOW_CTRL_NONE,
	};

	return cfg;
}

/* Bounded polling helper. Issues at most `cap` uart_poll_in() calls, stopping at
 * the first that does not return 0, and returns how many bytes it collected.
 * `out` must hold `cap` bytes. Never spins: `cap` is a compile-time constant at
 * every call site, so a driver that never returns data fails fast.
 */
int pdg_uart_poll_bounded(const struct device *dev, uint8_t *out, int cap)
{
	int got = 0;

	while (got < cap) {
		unsigned char c = 0U;

		if (uart_poll_in(dev, &c) != 0) {
			break;
		}
		out[got] = (uint8_t)c;
		got++;
	}

	return got;
}

/* ---------------------------------------------------------------------------
 * Suite hooks
 * ------------------------------------------------------------------------ */

static void *uart_suite_setup(void)
{
	/* Runs after every POST_KERNEL device init and before the first test,
	 * which is the only window in which the initialization evidence still
	 * exists in the runtime recorders.
	 */
	zassert_equal(pdg_uart_fake_freeze_boot_snapshot(), 1,
		      "the boot snapshot was already frozen; expected this call to "
		      "perform the capture (expected 1, got 0)");
	zassert_true(pdg_uart_fake_boot_frozen(),
		     "boot snapshot reports not frozen immediately after a "
		     "successful capture");

	return NULL;
}

static void uart_suite_before(void *fixture)
{
	ARG_UNUSED(fixture);

	pdg_uart_fake_reset();
}

static void uart_suite_after(void *fixture)
{
	ARG_UNUSED(fixture);

#if PDG_FAKE_UART_CAPABILITY
	struct uart_config general = pdg_uart_baseline_config(BAUD_GENERAL);
	struct uart_config cfg_fail = pdg_uart_baseline_config(BAUD_CFG_FAIL);

	/* Restoration lives in the hook, not in a test body, so it runs even
	 * when an assertion aborts the body mid-way. A test may have scripted
	 * set-config to fail, so clear the script first.
	 */
	pdg_uart_fake_set_set_config_result(0);
	pdg_uart_fake_set_read_result(0);
	pdg_uart_fake_set_write_result(0);

	(void)uart_configure(DEV_GENERAL, &general);
	(void)uart_configure(DEV_CFG_FAIL, &cfg_fail);
#endif
}

ZTEST_SUITE(pdg_fake_uart, NULL, uart_suite_setup, uart_suite_before, uart_suite_after, NULL);

/* ---------------------------------------------------------------------------
 * Scenario-independent
 * ------------------------------------------------------------------------ */

/* The MFD parent holds the sole registry reference and hands children a
 * borrowed context; zephyr/drivers/mfd/pdg_mfd.h forbids a child closing or
 * freeing it. The counter is latched against pdg_fake_reset(), so this holds
 * regardless of test order and in both capability scenarios.
 */
ZTEST(pdg_fake_uart, test_no_child_closes_the_borrowed_parent_context)
{
	zassert_equal(pdg_fake_close_count(), 0,
		      "a child closed the MFD parent's context: expected close "
		      "count 0, got %d",
		      pdg_fake_close_count());
	zassert_true(pdg_fake_open_count() > 0,
		     "the fake's pdg_common_bottom_open() was never called, so the "
		     "weak override did not take effect (expected > 0, got %d)",
		     pdg_fake_open_count());
}

#if PDG_FAKE_UART_CAPABILITY

/* ---------------------------------------------------------------------------
 * Spec 6.1 test 1 -- initialization evidence, frozen boot snapshot only.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_init_probes_capability_before_configuring_each_child)
{
	static const uint32_t expected_baud[UART_CHILD_COUNT] = {
		BAUD_GENERAL, BAUD_BACKOFF, BAUD_RX_LATCH, BAUD_TX_LATCH, BAUD_CFG_FAIL,
	};
	const struct device *devs[UART_CHILD_COUNT] = {
		DEV_GENERAL, DEV_BACKOFF, DEV_RX_LATCH, DEV_TX_LATCH, DEV_CFG_FAIL,
	};
	int seen[UART_CHILD_COUNT] = { 0 };
	int events;
	int probes = 0;
	int configs = 0;
	int i;
	int j;

	zassert_true(pdg_uart_fake_boot_frozen(), "boot snapshot was never frozen");
	zassert_false(pdg_uart_fake_boot_overflowed(),
		      "a recorder overflowed at or before the freeze, so the boot "
		      "snapshot proves nothing (expected 0, got %d)",
		      pdg_uart_fake_boot_overflowed());

	for (i = 0; i < UART_CHILD_COUNT; i++) {
		zassert_true(device_is_ready(devs[i]),
			     "UART child %d (%s) is not ready under capability=%d",
			     i, devs[i]->name, pdg_uart_fake_capability());
	}

	zassert_equal(pdg_uart_fake_boot_has_uart_count(), UART_CHILD_COUNT,
		      "expected exactly %d capability probes at init, got %d",
		      UART_CHILD_COUNT, pdg_uart_fake_boot_has_uart_count());
	zassert_equal(pdg_uart_fake_boot_set_config_count(), UART_CHILD_COUNT,
		      "expected exactly %d initial set-config calls, got %d",
		      UART_CHILD_COUNT, pdg_uart_fake_boot_set_config_count());
	zassert_equal(pdg_uart_fake_boot_read_count(), 0,
		      "init must issue no reads; expected 0, got %d",
		      pdg_uart_fake_boot_read_count());
	zassert_equal(pdg_uart_fake_boot_write_count(), 0,
		      "init must issue no writes; expected 0, got %d",
		      pdg_uart_fake_boot_write_count());
	zassert_equal(pdg_fake_close_count(), 0,
		      "init closed the parent context; expected 0, got %d",
		      pdg_fake_close_count());

	/* Each child's initial scalars, attributed by baud -- the only
	 * discriminator, since every child shares one borrowed ctx.
	 */
	zassert_equal(pdg_uart_fake_boot_config_log_len(), UART_CHILD_COUNT,
		      "expected %d frozen config-log entries, got %d", UART_CHILD_COUNT,
		      pdg_uart_fake_boot_config_log_len());

	for (i = 0; i < UART_CHILD_COUNT; i++) {
		uint32_t baud = 0U;
		uint8_t data_bits = 0xFFU;
		uint8_t parity = 0xFFU;
		uint8_t stop_bits = 0xFFU;
		int match = -1;

		zassert_equal(pdg_uart_fake_boot_config_log_entry(i, &baud, &data_bits,
								  &parity, &stop_bits),
			      0, "frozen config-log entry %d is unreadable", i);

		zassert_equal(data_bits, EXP_DATA_BITS_8,
			      "frozen config entry %d (baud %u): data bits expected %u, "
			      "got %u",
			      i, (unsigned int)baud, EXP_DATA_BITS_8,
			      (unsigned int)data_bits);
		zassert_equal(parity, EXP_PARITY_NONE,
			      "frozen config entry %d (baud %u): parity expected %u, "
			      "got %u",
			      i, (unsigned int)baud, EXP_PARITY_NONE,
			      (unsigned int)parity);
		zassert_equal(stop_bits, EXP_STOP_BITS_1,
			      "frozen config entry %d (baud %u): stop bits expected %u, "
			      "got %u",
			      i, (unsigned int)baud, EXP_STOP_BITS_1,
			      (unsigned int)stop_bits);

		for (j = 0; j < UART_CHILD_COUNT; j++) {
			if ((baud == expected_baud[j]) && (seen[j] == 0)) {
				match = j;
				break;
			}
		}

		zassert_true(match >= 0,
			     "frozen config entry %d has baud %u, which is not an "
			     "unclaimed devicetree rate (expected one of 115200, "
			     "57600, 38400, 19200, 9600)",
			     i, (unsigned int)baud);
		seen[match] = 1;
	}

	/* Ordering, and exactly how far the evidence reaches.
	 *
	 * PROVEN (aggregate): walking the frozen sequence in order, no
	 * SET_CONFIG is ever preceded by fewer HAS_UART events than
	 * SET_CONFIG events. Counts alone would be satisfied by a driver
	 * that configured everything first and probed afterwards; this
	 * running-prefix invariant rules that out across all children.
	 * Also proven: the exact event counts (UART_CHILD_COUNT of each,
	 * no other kind), and -- from the loop above -- that every child's
	 * devicetree baud appears exactly once in the frozen config log.
	 *
	 * NOT PROVEN (per-child): that child X probed before child X
	 * configured. PDG_UART_FAKE_EV_HAS_UART carries no child
	 * discriminator (see pdg_uart_fake_bottom.h): the shared parent
	 * fake hands every child the same ctx, so the recorder has nothing
	 * to attribute a capability probe to a specific child with. A
	 * sequence such as HAS_UART(A), SET_CONFIG(B), HAS_UART(B),
	 * SET_CONFIG(A) satisfies every assertion below. SET_CONFIG *is*
	 * per-child discriminable, because its aux field carries the baud
	 * (115200 / 57600 / 38400 / 19200 / 9600) and the loop above
	 * exploits exactly that to claim each rate once; HAS_UART has no
	 * equivalent, which is precisely why the claim cannot be tightened
	 * without a fake-side change.
	 *
	 * The ZTEST name is deliberately left as-is (it appears in captured
	 * twister output) even though it reads broader than the evidence.
	 */
	events = pdg_uart_fake_boot_event_count();
	zassert_equal(events, 2 * UART_CHILD_COUNT,
		      "expected exactly %d frozen init events (%d probes + %d "
		      "set-configs), got %d",
		      2 * UART_CHILD_COUNT, UART_CHILD_COUNT, UART_CHILD_COUNT, events);
	zassert_true(events <= PDG_UART_FAKE_EVENT_MAX,
		     "frozen event count %d exceeds the recorder cap %d", events,
		     PDG_UART_FAKE_EVENT_MAX);

	for (i = 0; i < events; i++) {
		uint8_t kind = 0U;
		uint32_t aux = 0U;

		zassert_equal(pdg_uart_fake_boot_event_at(i, &kind, &aux), 0,
			      "frozen event %d is unreadable", i);

		if (kind == PDG_UART_FAKE_EV_HAS_UART) {
			probes++;
		} else if (kind == PDG_UART_FAKE_EV_SET_CONFIG) {
			configs++;
			zassert_true(probes >= configs,
				     "frozen event %d is set-config #%d (baud %u) but "
				     "only %d capability probes preceded it; across "
				     "all children, no configuration may be preceded "
				     "by fewer probes than configurations (aggregate "
				     "ordering; the fake cannot attribute a probe to "
				     "a specific child)",
				     i, configs, (unsigned int)aux, probes);
		} else {
			zassert_unreachable("frozen event %d has kind %u; init must "
					    "only produce HAS_UART (%u) and SET_CONFIG "
					    "(%u)",
					    i, (unsigned int)kind,
					    (unsigned int)PDG_UART_FAKE_EV_HAS_UART,
					    (unsigned int)PDG_UART_FAKE_EV_SET_CONFIG);
		}
	}

	zassert_equal(probes, UART_CHILD_COUNT,
		      "frozen sequence holds %d HAS_UART events, expected %d", probes,
		      UART_CHILD_COUNT);
	zassert_equal(configs, UART_CHILD_COUNT,
		      "frozen sequence holds %d SET_CONFIG events, expected %d", configs,
		      UART_CHILD_COUNT);
}

/* ---------------------------------------------------------------------------
 * Spec 6.3 test 6 -- one refill serves many polls. Highest-value test.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_one_refill_serves_eight_polls)
{
	static const uint8_t scripted[8] = {
		0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87,
	};
	const struct device *dev = DEV_GENERAL;
	struct pdg_uart_snap snap;
	uint8_t got[ARRAY_SIZE(scripted)];
	uint16_t req_count = 0U;
	uint32_t req_timeout = 0xFFFFFFFFU;
	unsigned char spare = 0xCCU;
	int n;
	int i;
	int ninth;

	pdg_uart_snapshot(&snap);

	zassert_equal(pdg_uart_fake_script_rx(scripted, (uint16_t)ARRAY_SIZE(scripted)), 0,
		      "failed to script %d RX bytes", (int)ARRAY_SIZE(scripted));

	/* Bounded at exactly the scripted length: a driver that never returns
	 * data fails here rather than spinning.
	 */
	n = pdg_uart_poll_bounded(dev, got, (int)ARRAY_SIZE(scripted));
	zassert_equal(n, (int)ARRAY_SIZE(scripted),
		      "expected %d successful polls, got %d", (int)ARRAY_SIZE(scripted), n);

	for (i = 0; i < (int)ARRAY_SIZE(scripted); i++) {
		zassert_equal(got[i], scripted[i],
			      "poll %d returned 0x%02x, expected 0x%02x -- bytes must "
			      "be served from the staging ring in order",
			      i, (unsigned int)got[i], (unsigned int)scripted[i]);
	}

	/* THE claim. The staging ring is what makes eight polls cost one refill. */
	zassert_equal(pdg_uart_fake_read_count() - snap.read, 1,
		      "eight successive polls must record EXACTLY 1 bottom read, "
		      "got %d. Without the driver's staging ring this would be 8 "
		      "(one round trip per byte).",
		      pdg_uart_fake_read_count() - snap.read);

	zassert_equal(pdg_uart_fake_last_read(&req_count, &req_timeout), 0,
		      "no read was recorded, so its arguments cannot be checked");
	zassert_equal(req_count, EXP_REFILL_COUNT,
		      "refill requested %u bytes, expected %u", (unsigned int)req_count,
		      (unsigned int)EXP_REFILL_COUNT);
	zassert_equal(req_timeout, EXP_REFILL_TIMEOUT_MS,
		      "refill requested timeout %u ms, expected %u ms",
		      (unsigned int)req_timeout, (unsigned int)EXP_REFILL_TIMEOUT_MS);

	/* Ninth poll: the ring is drained, so exactly one more refill happens.
	 * The script persists and is not auto-consumed, so re-script it empty.
	 */
	zassert_equal(pdg_uart_fake_script_rx(scripted, 0U), 0,
		      "failed to script an empty RX payload");

	/* Hoisted deliberately: zassert_equal() bottoms out in z_zassert(), a
	 * FUNCTION, so its message varargs are evaluated unconditionally --
	 * pass or fail. No side-effecting call (any uart_* API, any fake
	 * mutator) may ever appear in a zassert message argument; calling
	 * uart_poll_in() here once cost a spurious third bottom read and a
	 * false failure below.
	 */
	ninth = uart_poll_in(dev, &spare);

	zassert_equal(ninth, -1,
		      "the ninth poll met an empty refill and must return exactly -1, "
		      "got %d",
		      ninth);

	zassert_equal(pdg_uart_fake_read_count() - snap.read, 2,
		      "after the ninth poll the bottom read count must be EXACTLY 2, "
		      "got %d. Without the driver's staging ring this would be 9.",
		      pdg_uart_fake_read_count() - snap.read);

	pdg_uart_assert_delta(&snap, 0, 0, 2, 0, "one refill serves eight polls");
}

/* ---------------------------------------------------------------------------
 * Spec 6.3 test 5 -- exact empty result.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_empty_refill_returns_minus_one_and_leaves_output_untouched)
{
	const struct device *dev = DEV_GENERAL;
	struct pdg_uart_snap snap;
	static const uint8_t none[1] = { 0x00 };
	unsigned char c = 0xCCU;
	uint16_t req_count = 0U;
	uint32_t req_timeout = 0xFFFFFFFFU;
	int ret;

	pdg_uart_snapshot(&snap);

	zassert_equal(pdg_uart_fake_script_rx(none, 0U), 0,
		      "failed to script an empty RX payload");

	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, -1, "an empty refill must return exactly -1, got %d", ret);
	zassert_equal(c, 0xCCU,
		      "uart_poll_in() wrote 0x%02x over the caller's seed; an "
		      "unsuccessful poll must leave it at 0xCC",
		      (unsigned int)c);

	zassert_equal(pdg_uart_fake_last_read(&req_count, &req_timeout), 0,
		      "no read was recorded for an empty poll");
	zassert_equal(req_count, EXP_REFILL_COUNT, "refill requested %u bytes, expected %u",
		      (unsigned int)req_count, (unsigned int)EXP_REFILL_COUNT);
	zassert_equal(req_timeout, EXP_REFILL_TIMEOUT_MS,
		      "refill requested timeout %u ms, expected %u ms",
		      (unsigned int)req_timeout, (unsigned int)EXP_REFILL_TIMEOUT_MS);

	pdg_uart_assert_delta(&snap, 0, 0, 1, 0, "empty refill");
}

/* ---------------------------------------------------------------------------
 * Spec 6.4 test 10 -- one write per byte.
 * ------------------------------------------------------------------------ */

/* TX is deliberately asymmetric with RX: no coalescing, no driver-side ring.
 * Three bytes out must be three bottom writes of one byte each.
 */
ZTEST(pdg_fake_uart, test_poll_out_issues_one_write_per_byte)
{
	static const uint8_t emitted[3] = { 'A', 'B', 'C' };
	const struct device *dev = DEV_GENERAL;
	struct pdg_uart_snap snap;
	int i;

	pdg_uart_snapshot(&snap);
	pdg_uart_fake_set_write_result(0);

	for (i = 0; i < (int)ARRAY_SIZE(emitted); i++) {
		uart_poll_out(dev, (unsigned char)emitted[i]);
	}

	pdg_uart_assert_delta(&snap, 0, 0, 0, (int)ARRAY_SIZE(emitted), "one write per byte");

	zassert_equal(pdg_uart_fake_write_log_len(), (int)ARRAY_SIZE(emitted),
		      "write log holds %d entries, expected %d",
		      pdg_uart_fake_write_log_len(), (int)ARRAY_SIZE(emitted));

	for (i = 0; i < (int)ARRAY_SIZE(emitted); i++) {
		uint8_t byte = 0U;
		uint16_t len = 0U;

		zassert_equal(pdg_uart_fake_write_log_entry(i, &byte, &len), 0,
			      "write-log entry %d is unreadable", i);
		zassert_equal(len, 1U,
			      "write %d had length %u, expected 1 -- poll_out must not "
			      "coalesce",
			      i, (unsigned int)len);
		zassert_equal(byte, emitted[i],
			      "write %d carried 0x%02x, expected 0x%02x", i,
			      (unsigned int)byte, (unsigned int)emitted[i]);
	}

	/* Ordering in the global event sequence, not just in the write log. */
	zassert_equal(pdg_uart_fake_event_count(), (int)ARRAY_SIZE(emitted),
		      "expected exactly %d events, got %d", (int)ARRAY_SIZE(emitted),
		      pdg_uart_fake_event_count());

	for (i = 0; i < (int)ARRAY_SIZE(emitted); i++) {
		uint8_t kind = 0U;
		uint32_t aux = 0U;

		zassert_equal(pdg_uart_fake_event_at(i, &kind, &aux), 0,
			      "event %d is unreadable", i);
		zassert_equal(kind, PDG_UART_FAKE_EV_WRITE,
			      "event %d has kind %u, expected WRITE (%u)", i,
			      (unsigned int)kind, (unsigned int)PDG_UART_FAKE_EV_WRITE);
		zassert_equal(aux, (uint32_t)emitted[i],
			      "event %d carried first byte 0x%02x, expected 0x%02x", i,
			      (unsigned int)aux, (unsigned int)emitted[i]);
	}
}

/* ---------------------------------------------------------------------------
 * Spec 6.4 test 12 -- configuration transcription matrix.
 * ------------------------------------------------------------------------ */

struct pdg_uart_cfg_case {
	const char *name;
	uint8_t z_data_bits;
	uint8_t z_parity;
	uint8_t z_stop_bits;
	uint8_t n_data_bits;
	uint8_t n_parity;
	uint8_t n_stop_bits;
};

/* One field at a time, against the 8/none/1 baseline.
 *
 * STOP BITS ARE THE TRAP and are why this is not an ordinal-equality check:
 * UART_CFG_STOP_BITS_1 is ordinal 1 but must arrive as PDG_UART_STOP_BITS_1 == 0.
 * The driver's BUILD_ASSERTs pin the mapping constants against each other; only
 * this runtime table proves pdg_uart_configure() actually routed each field
 * through the right macro.
 */
ZTEST(pdg_fake_uart, test_configuration_transcribes_every_accepted_field)
{
	static const struct pdg_uart_cfg_case cases[] = {
		{ "data 5", UART_CFG_DATA_BITS_5, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_5, EXP_PARITY_NONE, EXP_STOP_BITS_1 },
		{ "data 6", UART_CFG_DATA_BITS_6, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_6, EXP_PARITY_NONE, EXP_STOP_BITS_1 },
		{ "data 7", UART_CFG_DATA_BITS_7, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_7, EXP_PARITY_NONE, EXP_STOP_BITS_1 },
		{ "data 8", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_NONE, EXP_STOP_BITS_1 },
		{ "parity none", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_NONE, EXP_STOP_BITS_1 },
		{ "parity odd", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_ODD,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_ODD, EXP_STOP_BITS_1 },
		{ "parity even", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_EVEN,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_EVEN, EXP_STOP_BITS_1 },
		{ "parity mark", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_MARK,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_MARK, EXP_STOP_BITS_1 },
		{ "parity space", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_SPACE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_SPACE, EXP_STOP_BITS_1 },
		{ "stop 1", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, EXP_DATA_BITS_8, EXP_PARITY_NONE, EXP_STOP_BITS_1 },
		{ "stop 2", UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_2, EXP_DATA_BITS_8, EXP_PARITY_NONE, EXP_STOP_BITS_2 },
	};
	const struct device *dev = DEV_GENERAL;
	int i;

	zassert_true(ARRAY_SIZE(cases) <= PDG_UART_FAKE_CONFIG_LOG_MAX,
		     "the transcription table has %d cases, which exceeds the "
		     "config-log cap %d",
		     (int)ARRAY_SIZE(cases), PDG_UART_FAKE_CONFIG_LOG_MAX);

	for (i = 0; i < (int)ARRAY_SIZE(cases); i++) {
		const struct pdg_uart_cfg_case *c = &cases[i];
		struct uart_config cfg = pdg_uart_baseline_config(BAUD_GENERAL);
		struct pdg_uart_snap snap;
		uint32_t baud = 0U;
		uint8_t data_bits = 0xFFU;
		uint8_t parity = 0xFFU;
		uint8_t stop_bits = 0xFFU;
		int ret;

		/* Per-case reset keeps every assertion an index-0 lookup and
		 * bounds the config log regardless of table growth.
		 */
		pdg_uart_fake_reset();
		pdg_uart_snapshot(&snap);

		cfg.data_bits = c->z_data_bits;
		cfg.parity = c->z_parity;
		cfg.stop_bits = c->z_stop_bits;

		ret = uart_configure(dev, &cfg);
		zassert_equal(ret, 0, "case \"%s\": uart_configure() returned %d, expected 0",
			      c->name, ret);

		pdg_uart_assert_delta(&snap, 0, 1, 0, 0, c->name);

		zassert_equal(pdg_uart_fake_config_log_len(), 1,
			      "case \"%s\": config log holds %d entries, expected 1",
			      c->name, pdg_uart_fake_config_log_len());
		zassert_equal(pdg_uart_fake_config_log_entry(0, &baud, &data_bits, &parity,
							     &stop_bits),
			      0, "case \"%s\": config-log entry 0 is unreadable", c->name);

		zassert_equal(baud, BAUD_GENERAL, "case \"%s\": baud %u, expected %u",
			      c->name, (unsigned int)baud, (unsigned int)BAUD_GENERAL);
		zassert_equal(data_bits, c->n_data_bits,
			      "case \"%s\": data bits transcribed as %u, expected %u",
			      c->name, (unsigned int)data_bits,
			      (unsigned int)c->n_data_bits);
		zassert_equal(parity, c->n_parity,
			      "case \"%s\": parity transcribed as %u, expected %u",
			      c->name, (unsigned int)parity, (unsigned int)c->n_parity);
		zassert_equal(stop_bits, c->n_stop_bits,
			      "case \"%s\": stop bits transcribed as %u, expected %u "
			      "(note: NOT ordinal equality with the Zephyr value)",
			      c->name, (unsigned int)stop_bits,
			      (unsigned int)c->n_stop_bits);
	}
}

/* ---------------------------------------------------------------------------
 * Spec 6.4 test 13 -- rejected configurations are side-effect-free.
 * ------------------------------------------------------------------------ */

struct pdg_uart_reject_case {
	const char *name;
	uint32_t baudrate;
	uint8_t data_bits;
	uint8_t parity;
	uint8_t stop_bits;
	uint8_t flow_ctrl;
	int expected;
};

ZTEST(pdg_fake_uart, test_rejected_configurations_reach_no_bottom_call)
{
	static const struct pdg_uart_reject_case cases[] = {
		{ "data 9", BAUD_GENERAL, UART_CFG_DATA_BITS_9, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, UART_CFG_FLOW_CTRL_NONE, -ENOTSUP },
		{ "stop 0.5", BAUD_GENERAL, UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_0_5, UART_CFG_FLOW_CTRL_NONE, -ENOTSUP },
		{ "stop 1.5", BAUD_GENERAL, UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1_5, UART_CFG_FLOW_CTRL_NONE, -ENOTSUP },
		{ "flow RTS/CTS", BAUD_GENERAL, UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, UART_CFG_FLOW_CTRL_RTS_CTS, -ENOTSUP },
		{ "flow DTR/DSR", BAUD_GENERAL, UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, UART_CFG_FLOW_CTRL_DTR_DSR, -ENOTSUP },
		{ "flow RS485", BAUD_GENERAL, UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, UART_CFG_FLOW_CTRL_RS485, -ENOTSUP },
		{ "baud 0", 0U, UART_CFG_DATA_BITS_8, UART_CFG_PARITY_NONE,
		  UART_CFG_STOP_BITS_1, UART_CFG_FLOW_CTRL_NONE, -EINVAL },
	};
	const struct device *dev = DEV_GENERAL;
	struct pdg_uart_snap snap;
	struct uart_config before;
	struct uart_config after;
	int i;

	pdg_uart_snapshot(&snap);

	zassert_equal(uart_config_get(dev, &before), 0,
		      "uart_config_get() failed before the rejection table ran");

	for (i = 0; i < (int)ARRAY_SIZE(cases); i++) {
		const struct pdg_uart_reject_case *c = &cases[i];
		struct uart_config cfg = {
			.baudrate = c->baudrate,
			.parity = c->parity,
			.stop_bits = c->stop_bits,
			.data_bits = c->data_bits,
			.flow_ctrl = c->flow_ctrl,
		};
		int ret = uart_configure(dev, &cfg);

		zassert_equal(ret, c->expected,
			      "case \"%s\": uart_configure() returned %d, expected %d",
			      c->name, ret, c->expected);

		zassert_equal(uart_config_get(dev, &after), 0,
			      "case \"%s\": uart_config_get() failed", c->name);
		zassert_equal(after.baudrate, before.baudrate,
			      "case \"%s\": cached baud changed to %u, expected %u",
			      c->name, (unsigned int)after.baudrate,
			      (unsigned int)before.baudrate);
		zassert_equal(after.data_bits, before.data_bits,
			      "case \"%s\": cached data bits changed to %u, expected %u",
			      c->name, (unsigned int)after.data_bits,
			      (unsigned int)before.data_bits);
		zassert_equal(after.parity, before.parity,
			      "case \"%s\": cached parity changed to %u, expected %u",
			      c->name, (unsigned int)after.parity,
			      (unsigned int)before.parity);
		zassert_equal(after.stop_bits, before.stop_bits,
			      "case \"%s\": cached stop bits changed to %u, expected %u",
			      c->name, (unsigned int)after.stop_bits,
			      (unsigned int)before.stop_bits);
		zassert_equal(after.flow_ctrl, before.flow_ctrl,
			      "case \"%s\": cached flow control changed to %u, expected %u",
			      c->name, (unsigned int)after.flow_ctrl,
			      (unsigned int)before.flow_ctrl);
	}

	/* Validation happens before the lock and before the FFI call, so a
	 * rejected request must perform no I/O at all.
	 */
	pdg_uart_assert_delta(&snap, 0, 0, 0, 0, "rejected configurations");
	zassert_equal(pdg_uart_fake_config_log_len(), 0,
		      "a rejected configuration was still logged: expected 0 entries, "
		      "got %d",
		      pdg_uart_fake_config_log_len());
}

/* ---------------------------------------------------------------------------
 * Spec 6.4 test 14 -- the cache follows acknowledged success only.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_cache_and_staged_ring_survive_a_failed_set_config)
{
	static const uint8_t staged[2] = { 0x31, 0x32 };
	const struct device *dev = DEV_CFG_FAIL;
	struct pdg_uart_snap snap;
	struct uart_config cfg_a = pdg_uart_baseline_config(BAUD_CFG_FAIL);
	struct uart_config cfg_b = pdg_uart_baseline_config(BAUD_CFG_FAIL);
	struct uart_config got;
	unsigned char c = 0xCCU;
	int ret;

	cfg_a.parity = UART_CFG_PARITY_EVEN;
	cfg_a.stop_bits = UART_CFG_STOP_BITS_2;

	cfg_b.parity = UART_CFG_PARITY_ODD;
	cfg_b.stop_bits = UART_CFG_STOP_BITS_1;
	cfg_b.data_bits = UART_CFG_DATA_BITS_7;

	pdg_uart_snapshot(&snap);

	/* A succeeds and is cached. */
	ret = uart_configure(dev, &cfg_a);
	zassert_equal(ret, 0, "configuring A returned %d, expected 0", ret);
	zassert_equal(uart_config_get(dev, &got), 0, "uart_config_get() failed after A");
	zassert_equal(got.parity, cfg_a.parity, "cached parity %u, expected %u",
		      (unsigned int)got.parity, (unsigned int)cfg_a.parity);
	zassert_equal(got.stop_bits, cfg_a.stop_bits, "cached stop bits %u, expected %u",
		      (unsigned int)got.stop_bits, (unsigned int)cfg_a.stop_bits);

	/* Stage two bytes and consume one. A succeeded set cleared the ring, so
	 * this refill happens after it.
	 */
	zassert_equal(pdg_uart_fake_script_rx(staged, (uint16_t)ARRAY_SIZE(staged)), 0,
		      "failed to script %d RX bytes", (int)ARRAY_SIZE(staged));
	zassert_equal(uart_poll_in(dev, &c), 0, "the first staged poll failed");
	zassert_equal(c, staged[0], "first staged byte 0x%02x, expected 0x%02x",
		      (unsigned int)c, (unsigned int)staged[0]);
	pdg_uart_assert_delta(&snap, 0, 1, 1, 0, "after A and one staged poll");

	/* B fails with a non-transport error. */
	pdg_uart_fake_set_set_config_result(-EIO);
	ret = uart_configure(dev, &cfg_b);
	zassert_equal(ret, -EIO, "configuring B returned %d, expected %d", ret, -EIO);

	/* The request DID reach the bottom -- that is what makes the remote state
	 * indeterminate -- but the cache must not follow it.
	 */
	pdg_uart_assert_delta(&snap, 0, 2, 1, 0, "after the failed B");

	zassert_equal(uart_config_get(dev, &got), 0, "uart_config_get() failed after B");
	zassert_equal(got.parity, cfg_a.parity,
		      "cache followed a FAILED set: parity %u, expected A's %u",
		      (unsigned int)got.parity, (unsigned int)cfg_a.parity);
	zassert_equal(got.stop_bits, cfg_a.stop_bits,
		      "cache followed a FAILED set: stop bits %u, expected A's %u",
		      (unsigned int)got.stop_bits, (unsigned int)cfg_a.stop_bits);
	zassert_equal(got.data_bits, cfg_a.data_bits,
		      "cache followed a FAILED set: data bits %u, expected A's %u",
		      (unsigned int)got.data_bits, (unsigned int)cfg_a.data_bits);

	/* config_get is local: it must cause no bottom-call delta at all. */
	pdg_uart_assert_delta(&snap, 0, 2, 1, 0, "config_get causes no bottom call");

	/* A non-transport failure is not a stream-generation boundary, so the
	 * remaining staged byte must still be served without a new refill.
	 */
	c = 0xCCU;
	zassert_equal(uart_poll_in(dev, &c), 0,
		      "the remaining staged byte was discarded by a non-transport "
		      "set-config failure");
	zassert_equal(c, staged[1], "second staged byte 0x%02x, expected 0x%02x",
		      (unsigned int)c, (unsigned int)staged[1]);
	pdg_uart_assert_delta(&snap, 0, 2, 1, 0,
			      "the staged byte was served without a new read");
}

/* ---------------------------------------------------------------------------
 * Spec 6.4 test 15 -- async and wide stubs.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_async_and_wide_stubs_refuse_without_touching_the_bottom)
{
	const struct device *dev = DEV_GENERAL;
	struct pdg_uart_snap snap;
	uint8_t buf8[2] = { 0U, 0U };
	uint16_t buf16[2] = { 0U, 0U };
	uint16_t in16 = 0U;
	int ret;

	pdg_uart_snapshot(&snap);

	ret = uart_tx(dev, buf8, sizeof(buf8), SYS_FOREVER_US);
	zassert_equal(ret, -ENOTSUP, "uart_tx() returned %d, expected %d", ret, -ENOTSUP);

	ret = uart_tx_abort(dev);
	zassert_equal(ret, -ENOTSUP, "uart_tx_abort() returned %d, expected %d", ret,
		      -ENOTSUP);

	ret = uart_rx_enable(dev, buf8, sizeof(buf8), SYS_FOREVER_US);
	zassert_equal(ret, -ENOTSUP, "uart_rx_enable() returned %d, expected %d", ret,
		      -ENOTSUP);

	ret = uart_rx_buf_rsp(dev, buf8, sizeof(buf8));
	zassert_equal(ret, -ENOTSUP, "uart_rx_buf_rsp() returned %d, expected %d", ret,
		      -ENOTSUP);

	ret = uart_rx_disable(dev);
	zassert_equal(ret, -ENOTSUP, "uart_rx_disable() returned %d, expected %d", ret,
		      -ENOTSUP);

	ret = uart_tx_u16(dev, buf16, ARRAY_SIZE(buf16), SYS_FOREVER_US);
	zassert_equal(ret, -ENOTSUP, "uart_tx_u16() returned %d, expected %d", ret,
		      -ENOTSUP);

	ret = uart_rx_enable_u16(dev, buf16, ARRAY_SIZE(buf16), SYS_FOREVER_US);
	zassert_equal(ret, -ENOTSUP, "uart_rx_enable_u16() returned %d, expected %d", ret,
		      -ENOTSUP);

	ret = uart_rx_buf_rsp_u16(dev, buf16, ARRAY_SIZE(buf16));
	zassert_equal(ret, -ENOTSUP, "uart_rx_buf_rsp_u16() returned %d, expected %d", ret,
		      -ENOTSUP);

	/* Void, must return safely. A refused wide write is proven by the
	 * absence of a recorded write, asserted in the delta below.
	 */
	uart_poll_out_u16(dev, 0x01FFU);

	/* Deliberately NULL API slots: the wrappers NULL-check and answer. */
	ret = uart_callback_set(dev, NULL, NULL);
	zassert_equal(ret, -ENOSYS, "uart_callback_set() returned %d, expected %d", ret,
		      -ENOSYS);

	ret = uart_poll_in_u16(dev, &in16);
	zassert_equal(ret, -ENOSYS, "uart_poll_in_u16() returned %d, expected %d", ret,
		      -ENOSYS);

	pdg_uart_assert_delta(&snap, 0, 0, 0, 0, "async and wide stubs");
	zassert_equal(pdg_uart_fake_event_count(), 0,
		      "a stub reached the bottom layer: expected 0 events, got %d",
		      pdg_uart_fake_event_count());
}

/* ---------------------------------------------------------------------------
 * Spec 6.2 test 4 -- ISR refusal matrix.
 * ------------------------------------------------------------------------ */

struct pdg_uart_isr_results {
	int poll_in;
	int err_check;
	int configure;
	int config_get;
	int tx;
	int tx_abort;
	int rx_enable;
	int rx_buf_rsp;
	int rx_disable;
	int tx_u16;
	int rx_enable_u16;
	int rx_buf_rsp_u16;
	int reached_end;
};

static void pdg_uart_isr_probe(const void *param)
{
	struct pdg_uart_isr_results *r = (struct pdg_uart_isr_results *)param;
	const struct device *dev = DEV_GENERAL;
	struct uart_config cfg = pdg_uart_baseline_config(BAUD_GENERAL);
	struct uart_config got;
	unsigned char c = 0xCCU;
	uint8_t buf8[2] = { 0U, 0U };
	uint16_t buf16[2] = { 0U, 0U };

	r->poll_in = uart_poll_in(dev, &c);
	r->err_check = uart_err_check(dev);
	r->configure = uart_configure(dev, &cfg);
	r->config_get = uart_config_get(dev, &got);

	r->tx = uart_tx(dev, buf8, sizeof(buf8), SYS_FOREVER_US);
	r->tx_abort = uart_tx_abort(dev);
	r->rx_enable = uart_rx_enable(dev, buf8, sizeof(buf8), SYS_FOREVER_US);
	r->rx_buf_rsp = uart_rx_buf_rsp(dev, buf8, sizeof(buf8));
	r->rx_disable = uart_rx_disable(dev);

	r->tx_u16 = uart_tx_u16(dev, buf16, ARRAY_SIZE(buf16), SYS_FOREVER_US);
	r->rx_enable_u16 = uart_rx_enable_u16(dev, buf16, ARRAY_SIZE(buf16), SYS_FOREVER_US);
	r->rx_buf_rsp_u16 = uart_rx_buf_rsp_u16(dev, buf16, ARRAY_SIZE(buf16));

	/* Void shapes: the observable is that control returns at all. */
	uart_poll_out(dev, 'Z');
	uart_poll_out_u16(dev, 0x01FFU);

	r->reached_end = 1;
}

/*
 * LIMITATION, stated deliberately: this test proves ONLY that an ISR-context
 * call returns the documented value and makes no bottom call. It does NOT prove
 * that the context guard precedes the mutex. A guard moved to just after an
 * UNCONTENDED k_mutex_lock() would satisfy every assertion below unchanged, and
 * deliberately contending the lock risks deadlocking single-threaded ztest.
 * Guard-before-mutex remains source-review evidence only.
 */
ZTEST(pdg_fake_uart, test_isr_context_is_refused_without_reaching_the_bottom)
{
	static struct pdg_uart_isr_results results;
	struct pdg_uart_snap snap;

	pdg_uart_snapshot(&snap);
	memset(&results, 0, sizeof(results));

	/* Exactly one offload; the routine itself is straight-line and bounded. */
	irq_offload(pdg_uart_isr_probe, (const void *)&results);

	zassert_equal(results.reached_end, 1,
		      "the ISR probe did not run to completion (expected 1, got %d); "
		      "a callback blocked or faulted in ISR context",
		      results.reached_end);

	zassert_equal(results.poll_in, -1,
		      "uart_poll_in() in ISR returned %d, expected exactly -1",
		      results.poll_in);

	zassert_equal(results.err_check, -EWOULDBLOCK,
		      "uart_err_check() in ISR returned %d, expected %d",
		      results.err_check, -EWOULDBLOCK);
	zassert_equal(results.configure, -EWOULDBLOCK,
		      "uart_configure() in ISR returned %d, expected %d",
		      results.configure, -EWOULDBLOCK);
	zassert_equal(results.config_get, -EWOULDBLOCK,
		      "uart_config_get() in ISR returned %d, expected %d",
		      results.config_get, -EWOULDBLOCK);
	zassert_equal(results.tx, -EWOULDBLOCK, "uart_tx() in ISR returned %d, expected %d",
		      results.tx, -EWOULDBLOCK);
	zassert_equal(results.tx_abort, -EWOULDBLOCK,
		      "uart_tx_abort() in ISR returned %d, expected %d", results.tx_abort,
		      -EWOULDBLOCK);
	zassert_equal(results.rx_enable, -EWOULDBLOCK,
		      "uart_rx_enable() in ISR returned %d, expected %d",
		      results.rx_enable, -EWOULDBLOCK);
	zassert_equal(results.rx_buf_rsp, -EWOULDBLOCK,
		      "uart_rx_buf_rsp() in ISR returned %d, expected %d",
		      results.rx_buf_rsp, -EWOULDBLOCK);
	zassert_equal(results.rx_disable, -EWOULDBLOCK,
		      "uart_rx_disable() in ISR returned %d, expected %d",
		      results.rx_disable, -EWOULDBLOCK);
	zassert_equal(results.tx_u16, -EWOULDBLOCK,
		      "uart_tx_u16() in ISR returned %d, expected %d", results.tx_u16,
		      -EWOULDBLOCK);
	zassert_equal(results.rx_enable_u16, -EWOULDBLOCK,
		      "uart_rx_enable_u16() in ISR returned %d, expected %d",
		      results.rx_enable_u16, -EWOULDBLOCK);
	zassert_equal(results.rx_buf_rsp_u16, -EWOULDBLOCK,
		      "uart_rx_buf_rsp_u16() in ISR returned %d, expected %d",
		      results.rx_buf_rsp_u16, -EWOULDBLOCK);

	pdg_uart_assert_delta(&snap, 0, 0, 0, 0, "ISR refusal matrix");
	zassert_equal(pdg_uart_fake_event_count(), 0,
		      "an ISR-context call reached the bottom layer: expected 0 "
		      "events, got %d",
		      pdg_uart_fake_event_count());
}

/* ---------------------------------------------------------------------------
 * Spec 6.3 test 7 -- implausible reported length.
 * ------------------------------------------------------------------------ */

/* No real firmware can report more bytes than it was asked for; this is a
 * defensive path only a fake can reach. The fake reports 1015 while still
 * writing at most `count` bytes, so the driver is exercised without a smashed
 * stack.
 */
ZTEST(pdg_fake_uart, test_implausible_reported_length_is_refused_then_recovers)
{
	static const uint8_t one[1] = { 0x5A };
	const struct device *dev = DEV_BACKOFF;
	struct pdg_uart_snap snap;
	unsigned char c = 0xCCU;
	int64_t t_error;
	int i;
	int ret;

	pdg_uart_snapshot(&snap);

	zassert_equal(pdg_uart_fake_script_rx(one, 1U), 0, "failed to script one RX byte");
	pdg_uart_fake_set_reported_len((uint16_t)(EXP_REFILL_COUNT + 1U));

	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, -1,
		      "a refill reporting %u bytes must be refused with exactly -1, "
		      "got %d",
		      (unsigned int)(EXP_REFILL_COUNT + 1U), ret);
	zassert_equal(c, 0xCCU,
		      "the caller's seed was overwritten with 0x%02x; an implausible "
		      "length must leave it at 0xCC",
		      (unsigned int)c);
	pdg_uart_assert_delta(&snap, 0, 0, 1, 0, "implausible length refused");

	/* Backoff engaged: immediate retries must not reach the fake. Bounded at
	 * a compile-time 4.
	 */
	t_error = k_uptime_get();
	for (i = 0; i < 4; i++) {
		c = 0xCCU;
		ret = uart_poll_in(dev, &c);
		zassert_equal(ret, -1, "suppressed retry %d returned %d, expected -1", i,
			      ret);
		zassert_equal(c, 0xCCU, "suppressed retry %d wrote 0x%02x over the seed",
			      i, (unsigned int)c);
	}
	zassert_true(k_uptime_get() == t_error,
		     "simulated uptime advanced during four immediate polls "
		     "(expected %lld, got %lld); the suppression below would then "
		     "prove nothing",
		     (long long)t_error, (long long)k_uptime_get());
	pdg_uart_assert_delta(&snap, 0, 0, 1, 0, "immediate retries are suppressed");

	/* REQUIRED recovery arm: without it a permanent latch would be
	 * indistinguishable from a bounded backoff. Ticks are 10 ms, so 11 ms
	 * rounds up to 2 ticks (~20 ms), past the driver's +10 ms deadline.
	 */
	k_sleep(K_MSEC(11));
	zassert_true((k_uptime_get() - t_error) >= 10,
		     "uptime advanced only %lld ms across the sleep, expected >= 10",
		     (long long)(k_uptime_get() - t_error));

	zassert_equal(pdg_uart_fake_script_rx(one, 1U), 0, "failed to re-script one byte");

	c = 0xCCU;
	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, 0,
		      "the post-deadline retry returned %d, expected 0: the backoff "
		      "must be bounded, not a permanent latch",
		      ret);
	zassert_equal(c, one[0], "post-deadline byte 0x%02x, expected 0x%02x",
		      (unsigned int)c, (unsigned int)one[0]);
	pdg_uart_assert_delta(&snap, 0, 0, 2, 0, "post-deadline retry reaches the fake");
}

/* ---------------------------------------------------------------------------
 * Spec 6.3 test 8 -- non-transport backoff and recovery.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_endpoint_error_backs_off_then_retries_after_the_deadline)
{
	static const uint8_t one[1] = { 0xA5 };
	const struct device *dev = DEV_BACKOFF;
	struct pdg_uart_snap snap;
	unsigned char c = 0xCCU;
	int64_t t_error;
	int i;
	int ret;

	pdg_uart_snapshot(&snap);

	pdg_uart_fake_set_read_result(-EIO);

	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, -1, "a -EIO refill must return exactly -1, got %d", ret);
	zassert_equal(c, 0xCCU, "a failed refill wrote 0x%02x over the caller's seed",
		      (unsigned int)c);
	pdg_uart_assert_delta(&snap, 0, 0, 1, 0, "the -EIO refill");

	/* native_sim charges zero simulated time for CPU work, so these four
	 * polls cannot cross the deadline; asserting the clock did not move is
	 * what makes the suppression non-vacuous. Bounded at a fixed 4.
	 */
	t_error = k_uptime_get();
	for (i = 0; i < 4; i++) {
		c = 0xCCU;
		ret = uart_poll_in(dev, &c);
		zassert_equal(ret, -1, "suppressed poll %d returned %d, expected -1", i,
			      ret);
		zassert_equal(c, 0xCCU, "suppressed poll %d wrote 0x%02x over the seed", i,
			      (unsigned int)c);
		zassert_equal(pdg_uart_fake_read_count() - snap.read, 1,
			      "suppressed poll %d issued a refill: read delta expected "
			      "1, got %d",
			      i, pdg_uart_fake_read_count() - snap.read);
	}
	zassert_true(k_uptime_get() == t_error,
		     "simulated uptime advanced across four immediate polls "
		     "(expected %lld, got %lld)",
		     (long long)t_error, (long long)k_uptime_get());

	/* 11 ms rounds up to 2 ticks (~20 ms) at 100 ticks/s. */
	k_sleep(K_MSEC(11));
	zassert_true((k_uptime_get() - t_error) >= 10,
		     "uptime advanced only %lld ms across the sleep, expected >= 10",
		     (long long)(k_uptime_get() - t_error));

	pdg_uart_fake_set_read_result(0);
	zassert_equal(pdg_uart_fake_script_rx(one, 1U), 0, "failed to script one RX byte");

	c = 0xCCU;
	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, 0, "the post-deadline poll returned %d, expected 0", ret);
	zassert_equal(c, one[0], "post-deadline byte 0x%02x, expected 0x%02x",
		      (unsigned int)c, (unsigned int)one[0]);
	pdg_uart_assert_delta(&snap, 0, 0, 2, 0, "exactly one retry after the deadline");
}

/* ---------------------------------------------------------------------------
 * Spec 6.3 test 9 -- RX transport latch. PERMANENT: pdg_uart_rx_latch is used
 * by no other test, because nothing can re-arm it.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_rx_transport_error_latches_permanently)
{
	const struct device *dev = DEV_RX_LATCH;
	struct pdg_uart_snap snap;
	struct uart_config cfg = pdg_uart_baseline_config(BAUD_RX_LATCH);
	unsigned char c = 0xCCU;
	int i;
	int ret;

	pdg_uart_snapshot(&snap);

	pdg_uart_fake_set_read_result(-ECOMM);

	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, -1, "a -ECOMM refill must return exactly -1, got %d", ret);
	zassert_equal(c, 0xCCU, "a failed refill wrote 0x%02x over the caller's seed",
		      (unsigned int)c);
	pdg_uart_assert_delta(&snap, 0, 0, 1, 0, "the -ECOMM refill");

	/* The latch is proven behaviourally: -EIO out of both reporting
	 * callbacks, and nothing further reaching the fake. The driver's private
	 * link_failed flag is deliberately not inspected.
	 */
	ret = uart_err_check(dev);
	zassert_equal(ret, -EIO, "uart_err_check() after the latch returned %d, expected %d",
		      ret, -EIO);

	ret = uart_configure(dev, &cfg);
	zassert_equal(ret, -EIO, "uart_configure() after the latch returned %d, expected %d",
		      ret, -EIO);

	/* Unlike the backoff, this must NEVER recover, so there is no sleep. Any
	 * scripted success must stay unreachable. Bounded at a fixed 4.
	 */
	pdg_uart_fake_set_read_result(0);
	pdg_uart_fake_set_write_result(0);

	for (i = 0; i < 4; i++) {
		c = 0xCCU;
		zassert_equal(uart_poll_in(dev, &c), -1,
			      "post-latch poll %d returned success; the latch must be "
			      "permanent",
			      i);
		uart_poll_out(dev, 'X');
		zassert_equal(uart_configure(dev, &cfg), -EIO,
			      "post-latch configure %d did not return %d", i, -EIO);
	}

	zassert_equal(uart_err_check(dev), -EIO,
		      "uart_err_check() stopped reporting %d after later calls", -EIO);

	pdg_uart_assert_delta(&snap, 0, 0, 1, 0,
			      "no call after the latch reaches the bottom layer");
}

/* ---------------------------------------------------------------------------
 * Spec 6.4 test 11 -- TX transport latch and staged-ring discard.
 * pdg_uart_tx_latch is likewise exclusively owned: the latch is permanent.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_tx_transport_error_latches_and_discards_the_staged_ring)
{
	static const uint8_t staged[2] = { 0x11, 0x22 };
	const struct device *dev = DEV_TX_LATCH;
	struct pdg_uart_snap snap;
	struct uart_config cfg = pdg_uart_baseline_config(BAUD_TX_LATCH);
	unsigned char c = 0xCCU;
	int i;
	int ret;

	pdg_uart_snapshot(&snap);

	/* Stage two, consume one: one byte remains in the driver's ring. */
	zassert_equal(pdg_uart_fake_script_rx(staged, (uint16_t)ARRAY_SIZE(staged)), 0,
		      "failed to script %d RX bytes", (int)ARRAY_SIZE(staged));
	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, 0, "the first staged poll returned %d, expected 0", ret);
	zassert_equal(c, staged[0], "first staged byte 0x%02x, expected 0x%02x",
		      (unsigned int)c, (unsigned int)staged[0]);
	pdg_uart_assert_delta(&snap, 0, 0, 1, 0, "one refill staged two bytes");

	/* A lost write acknowledgement is a stream-generation boundary. */
	pdg_uart_fake_set_write_result(-ETIMEDOUT);
	uart_poll_out(dev, 'Q');
	pdg_uart_assert_delta(&snap, 0, 0, 1, 1, "the failing write reached the fake");

	/* The remaining staged byte must NOT be served: it was decoded on the far
	 * side of the boundary. And no new refill may be issued, because the
	 * link is latched.
	 */
	c = 0xCCU;
	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, -1,
		      "poll after the TX latch returned %d, expected -1; the staged "
		      "byte 0x%02x must have been discarded",
		      ret, (unsigned int)staged[1]);
	zassert_equal(c, 0xCCU,
		      "poll after the TX latch handed back 0x%02x; expected the seed "
		      "0xCC to be untouched",
		      (unsigned int)c);
	pdg_uart_assert_delta(&snap, 0, 0, 1, 1, "no refill after the TX latch");

	ret = uart_err_check(dev);
	zassert_equal(ret, -EIO, "uart_err_check() after the TX latch returned %d, "
				"expected %d",
		      ret, -EIO);
	ret = uart_configure(dev, &cfg);
	zassert_equal(ret, -EIO, "uart_configure() after the TX latch returned %d, "
				"expected %d",
		      ret, -EIO);

	/* Scripted success must stay unreachable. Bounded at a fixed 4. */
	pdg_uart_fake_set_write_result(0);
	pdg_uart_fake_set_read_result(0);

	for (i = 0; i < 4; i++) {
		c = 0xCCU;
		zassert_equal(uart_poll_in(dev, &c), -1,
			      "post-latch poll %d returned success", i);
		uart_poll_out(dev, 'Y');
		zassert_equal(uart_configure(dev, &cfg), -EIO,
			      "post-latch configure %d did not return %d", i, -EIO);
	}

	pdg_uart_assert_delta(&snap, 0, 0, 1, 1,
			      "no call after the TX latch reaches the bottom layer");
}

#else /* PDG_FAKE_UART_CAPABILITY == 0 */

/* ---------------------------------------------------------------------------
 * Spec 6.1 test 2 -- capability refusal.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_capability_refusal_stops_every_child_at_init)
{
	const struct device *devs[UART_CHILD_COUNT] = {
		DEV_GENERAL, DEV_BACKOFF, DEV_RX_LATCH, DEV_TX_LATCH, DEV_CFG_FAIL,
	};
	struct pdg_uart_snap snap;
	int i;

	pdg_uart_snapshot(&snap);

	zassert_equal(pdg_uart_fake_capability(), 0,
		      "this test only compiles under capability=0 but the fake "
		      "reports %d",
		      pdg_uart_fake_capability());
	zassert_true(pdg_uart_fake_boot_frozen(), "boot snapshot was never frozen");
	zassert_false(pdg_uart_fake_boot_overflowed(),
		      "a recorder overflowed at or before the freeze (expected 0, "
		      "got %d)",
		      pdg_uart_fake_boot_overflowed());

	for (i = 0; i < UART_CHILD_COUNT; i++) {
		zassert_false(device_is_ready(devs[i]),
			      "UART child %d (%s) reports ready even though the fake "
			      "advertises no UART capability",
			      i, devs[i]->name);
	}

	zassert_equal(pdg_uart_fake_boot_has_uart_count(), UART_CHILD_COUNT,
		      "expected exactly %d capability probes (one per enabled UART "
		      "child), got %d",
		      UART_CHILD_COUNT, pdg_uart_fake_boot_has_uart_count());
	zassert_equal(pdg_uart_fake_boot_set_config_count(), 0,
		      "a refused child still issued set-config; expected 0, got %d",
		      pdg_uart_fake_boot_set_config_count());
	zassert_equal(pdg_uart_fake_boot_read_count(), 0,
		      "a refused child still issued a read; expected 0, got %d",
		      pdg_uart_fake_boot_read_count());
	zassert_equal(pdg_uart_fake_boot_write_count(), 0,
		      "a refused child still issued a write; expected 0, got %d",
		      pdg_uart_fake_boot_write_count());

	pdg_uart_assert_delta(&snap, 0, 0, 0, 0, "capability refusal");
}

/* ---------------------------------------------------------------------------
 * Spec 6.1 test 3 -- NULL-context direct dispatch.
 *
 * Zephyr's UART wrappers dispatch into the driver API WITHOUT checking device
 * readiness, so calling a failed-init device is a real reachable path. Every
 * callback must refuse on its own data->ctx == NULL guard.
 * ------------------------------------------------------------------------ */

ZTEST(pdg_fake_uart, test_failed_init_dispatch_refuses_without_dereferencing_context)
{
	const struct device *dev = DEV_GENERAL;
	struct pdg_uart_snap snap;
	struct uart_config cfg = pdg_uart_baseline_config(BAUD_GENERAL);
	struct uart_config got;
	unsigned char c = 0xCCU;
	uint8_t buf8[2] = { 0U, 0U };
	uint16_t buf16[2] = { 0U, 0U };
	int ret;

	pdg_uart_snapshot(&snap);

	zassert_false(device_is_ready(dev),
		      "%s reports ready under capability=0; this test requires a "
		      "failed-init device",
		      dev->name);

	ret = uart_poll_in(dev, &c);
	zassert_equal(ret, -1, "uart_poll_in() on a failed-init device returned %d, "
			       "expected exactly -1",
		      ret);
	zassert_equal(c, 0xCCU, "uart_poll_in() wrote 0x%02x over the caller's seed",
		      (unsigned int)c);

	/* Void shapes: the observable is that control returns and no write is
	 * recorded (asserted in the delta below).
	 */
	uart_poll_out(dev, 'A');
	uart_poll_out_u16(dev, 0x01FFU);

	ret = uart_err_check(dev);
	zassert_equal(ret, -ENODEV, "uart_err_check() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_configure(dev, &cfg);
	zassert_equal(ret, -ENODEV, "uart_configure() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_config_get(dev, &got);
	zassert_equal(ret, -ENODEV, "uart_config_get() returned %d, expected %d", ret,
		      -ENODEV);

	/* pdg_uart_async_unsupported() reports failed init AHEAD of the
	 * capability answer, so these are -ENODEV and not -ENOTSUP.
	 */
	ret = uart_tx(dev, buf8, sizeof(buf8), SYS_FOREVER_US);
	zassert_equal(ret, -ENODEV, "uart_tx() returned %d, expected %d", ret, -ENODEV);

	ret = uart_tx_abort(dev);
	zassert_equal(ret, -ENODEV, "uart_tx_abort() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_rx_enable(dev, buf8, sizeof(buf8), SYS_FOREVER_US);
	zassert_equal(ret, -ENODEV, "uart_rx_enable() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_rx_buf_rsp(dev, buf8, sizeof(buf8));
	zassert_equal(ret, -ENODEV, "uart_rx_buf_rsp() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_rx_disable(dev);
	zassert_equal(ret, -ENODEV, "uart_rx_disable() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_tx_u16(dev, buf16, ARRAY_SIZE(buf16), SYS_FOREVER_US);
	zassert_equal(ret, -ENODEV, "uart_tx_u16() returned %d, expected %d", ret, -ENODEV);

	ret = uart_rx_enable_u16(dev, buf16, ARRAY_SIZE(buf16), SYS_FOREVER_US);
	zassert_equal(ret, -ENODEV, "uart_rx_enable_u16() returned %d, expected %d", ret,
		      -ENODEV);

	ret = uart_rx_buf_rsp_u16(dev, buf16, ARRAY_SIZE(buf16));
	zassert_equal(ret, -ENODEV, "uart_rx_buf_rsp_u16() returned %d, expected %d", ret,
		      -ENODEV);

	pdg_uart_assert_delta(&snap, 0, 0, 0, 0, "failed-init direct dispatch");
	zassert_equal(pdg_uart_fake_event_count(), 0,
		      "a failed-init callback reached the bottom layer: expected 0 "
		      "events, got %d",
		      pdg_uart_fake_event_count());
}

#endif /* PDG_FAKE_UART_CAPABILITY */
