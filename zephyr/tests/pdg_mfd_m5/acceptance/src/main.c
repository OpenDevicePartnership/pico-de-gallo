/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * M5 phase 3 -- behavioural acceptance.
 *
 * Order is NORMATIVE, not stylistic (test design §2, §4.4, §6.3):
 *
 *   T2 (echo) -> T3 (chip-select lifecycle) -> timing -> T5 (payload) -> T4 (fault)
 *
 * T2 first: everything downstream that compares bytes -- including all of T6 --
 * is meaningless if the echo is shifted.
 *
 * T5 before T4: T4 is the only test that can leave the controller latched, and a
 * latched controller returns -EHOSTDOWN from pdg_spi.c:479 to every subsequent
 * transfer, before the payload ceiling at :438 is ever reached. Running T5 after
 * T4 would silently make it vacuous while still "erroring correctly".
 *
 * T4 last: it deliberately re-creates the orphaned pin-2 subscription that the
 * reset image exists to clear, so the set of tests it can contaminate is empty.
 *
 * WITNESS ASYMMETRY (test design §5.4). Pin 3 with a pull-up reads HIGH when
 * pin 2 drives high, when pin 2 is a high-impedance input, when pin 2 is
 * firmware-monitored, and when the jumper has fallen off. A witness LOW is
 * strong; a witness HIGH is weak and is only ever asserted as one end of a
 * transition observed within this same process, or as "unchanged across a call
 * that must issue no edge".
 */

#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/gpio.h>
#include <zephyr/drivers/spi.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/sys/util.h>

#include <string.h>

#include "posix_board_if.h"
#include "cmdline.h"
#include "posix_native_task.h"

#include "pdg_mfd.h"
#include "pdg_gpio_bottom.h"
#include "m5_bottom.h"

#define M5_SPI_NODE DT_NODELABEL(pdg_spi0)
#define M5_USER DT_PATH(zephyr_user)
#define M5_SLOW_DEV DT_NODELABEL(m5_slow_dev)
#define M5_FAST_DEV DT_NODELABEL(m5_fast_dev)

#define M5_CS_PIN 2U
#define M5_SLOW_HZ DT_PROP(M5_SLOW_DEV, spi_max_frequency)
#define M5_FAST_HZ DT_PROP(M5_FAST_DEV, spi_max_frequency)

#define M5_TIMED_ITERATIONS 25
#define M5_TIMED_LEN 54

/*
 * Payload-boundary constants for T5. ONE place to edit when the ceiling moves.
 *
 * M5_SPI_CEILING mirrors PDG_SPI_MAX_BUFFER at zephyr/drivers/spi/pdg_spi.c:226.
 * It is duplicated rather than shared because that constant lives in a .c file
 * with no header, and reaching into driver internals from a test would be
 * worse. Note this is deliberately NOT a self-referential assertion: test
 * design §9.1 deleted "assert PDG_SPI_MAX_BUFFER == N in the test source" as
 * vacuous, because it only proves a macro equals itself. The value actually
 * compiled into the driver is pinned instead by T5b/T5c's -EMSGSIZE behaviour
 * and by the LOG_WRN the driver emits, which names its own constant.
 *
 * 1013 is MEASURED, not derived: the largest length observed to work on
 * hardware, TX-only. See the comment on PDG_SPI_MAX_BUFFER for what is still
 * unknown, in particular that the DUPLEX ceiling has never been measured.
 */
#define M5_SPI_CEILING 1013U
#define M5_SPI_OVER_CEILING (M5_SPI_CEILING + 1U)

/*
 * A duplex length chosen for margin, not coverage: roughly half the measured
 * TX-only ceiling and far from the 1015-byte length that hangs the firmware
 * dispatcher device-wide. Used by T5e (shape check) and as T5d's first
 * fragment. Matches CONFIG_SPI_LARGE_BUFFER_SIZE in ../spi_loopback.conf.
 */
#define M5_SPI_DUPLEX_SAFE 512U

BUILD_ASSERT(M5_SPI_DUPLEX_SAFE < M5_SPI_CEILING,
	     "the duplex shape check must sit below the measured TX-only ceiling");
BUILD_ASSERT(M5_SPI_OVER_CEILING - M5_SPI_DUPLEX_SAFE < M5_SPI_CEILING,
	     "T5d's second fragment must itself be under the ceiling, or the test "
	     "would prove a per-buffer rejection rather than an accumulated one");
BUILD_ASSERT(M5_SPI_OVER_CEILING < 1015U,
	     "no T5 case may reach 1015: a 1015-byte transfer never returns and "
	     "wedges the firmware dispatcher device-wide");

BUILD_ASSERT(DT_NODE_HAS_STATUS_OKAY(M5_SPI_NODE),
	     "M5 acceptance image requires pdg_spi0 to be okay");
BUILD_ASSERT(DT_NODE_HAS_STATUS_OKAY(DT_NODELABEL(pdg_gpio0)),
	     "M5 acceptance image requires pdg_gpio0 to be okay");
BUILD_ASSERT(M5_TIMED_ITERATIONS >= 20,
	     "acceptance spec §7.1 requires at least 20 healthy timed transfers per mode");

/*
 * GUARD 1 of 2 for the chip-select contract of this image.
 *
 * SPI_CS_CONTROL_INIT() sets .cs_is_gpio = DT_SPI_DEV_HAS_CS_GPIOS(node), so if
 * either device node ever loses its bus's cs-gpios, every config below would
 * silently become a native-CS config, spi_cs_is_gpio() would return false, and
 * pdg_spi_cs_control_checked() would return 0 without issuing any GPIO write --
 * a fully successful, perfectly echoing, chip-select-less transfer with no
 * diagnostic anywhere. T3a would then be measuring nothing. Fail the build
 * instead.
 */
BUILD_ASSERT(DT_SPI_DEV_HAS_CS_GPIOS(M5_SLOW_DEV),
	     "m5slow@0 must resolve a GPIO chip select through its bus's cs-gpios; "
	     "without it every spi_config here becomes a native-CS config and the "
	     "driver issues no chip-select edge at all");
BUILD_ASSERT(DT_SPI_DEV_HAS_CS_GPIOS(M5_FAST_DEV),
	     "m5fast@0 must resolve a GPIO chip select through its bus's cs-gpios; "
	     "without it every spi_config here becomes a native-CS config and the "
	     "driver issues no chip-select edge at all");

static const struct device *const m5_spi = DEVICE_DT_GET(M5_SPI_NODE);
static const struct gpio_dt_spec m5_witness = GPIO_DT_SPEC_GET(M5_USER, m5_witness_gpios);

/*
 * Five spi_config objects at FIVE DISTINCT ADDRESSES. spi_context_configured()
 * compares by pointer, so a shared object would make "a different config"
 * indistinguishable from "the same config" and would silently defeat T3d and
 * T4 step 4.
 *
 * Built with the upstream SPI_CONFIG_DT() initializer, NEVER by hand. Hand-
 * building struct spi_config is what produced the T3a failure: the hand-rolled
 * struct spi_cs_control set .gpio and .delay but omitted .cs_is_gpio, which
 * static initialization left false, and spi_cs_is_gpio() reads only that field.
 * Adding one more field by hand would be the same trap with a longer fuse --
 * the macro sets every field the upstream struct has, including any added
 * later. The delay argument is deliberately omitted: passing one triggers
 * SPI_DEPRECATE_DELAY_WARN, and the value is derived from the device node's
 * spi-cs-setup-delay-ns / spi-cs-hold-delay-ns instead.
 */
static struct spi_config m5_cfg_hold =
	SPI_CONFIG_DT(M5_SLOW_DEV,
		      SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | SPI_HOLD_ON_CS | SPI_LOCK_ON);

static struct spi_config m5_cfg_plain =
	SPI_CONFIG_DT(M5_SLOW_DEV, SPI_WORD_SET(8) | SPI_OP_MODE_MASTER);

/* HOLD without LOCK -- rejected at pdg_spi.c:413 before any I/O (T3e). */
static struct spi_config m5_cfg_hold_nolock =
	SPI_CONFIG_DT(M5_SLOW_DEV,
		      SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | SPI_HOLD_ON_CS);

static struct spi_config m5_cfg_slow =
	SPI_CONFIG_DT(M5_SLOW_DEV, SPI_WORD_SET(8) | SPI_OP_MODE_MASTER);

static struct spi_config m5_cfg_fast =
	SPI_CONFIG_DT(M5_FAST_DEV, SPI_WORD_SET(8) | SPI_OP_MODE_MASTER);

/*
 * Pattern and poison (test design §4.2). The poison is 0x3C, NOT 0xA5: a
 * one-bit right shift of this pattern produces 0xA5 at index 3, so 0xA5 as
 * poison would make "shifted" and "never written" indistinguishable at that
 * byte. 0x3C appears in none of the predicted fault streams.
 */
static const uint8_t m5_echo_tx[5] = { 0x96, 0x2D, 0xE1, 0x4B, 0x73 };
#define M5_POISON 0x3C

static uint8_t m5_echo_rx[5];
static uint8_t m5_timed_tx[M5_TIMED_LEN];
static uint8_t m5_timed_rx[M5_TIMED_LEN];

/* 4096 is the before-control size and the sweep's upper probe bound. */
static uint8_t m5_big_tx[4096];
static uint8_t m5_big_rx[4096];

static bool m5_payload_before_only;
static bool m5_ceiling_sweep;

static void m5_add_options(void)
{
	static struct args_struct_t m5_options[] = {
		{
			.is_switch = true,
			.option = "payload-before-only",
			.type = 'b',
			.dest = (void *)&m5_payload_before_only,
			.descript = "Run only the 4096-byte payload control case, print "
				    "M5_PAYLOAD_BEFORE_RESULT=<errno>, and perform no T2, "
				    "T3, T5 or T4 work (acceptance spec §4.1 step 2)."
		},
		{
			.is_switch = true,
			.option = "ceiling-sweep",
			.type = 'b',
			.dest = (void *)&m5_ceiling_sweep,
			.descript = "Empirically determine the usable SPI payload ceiling by "
				    "binary search, independently for TX-only and full-duplex "
				    "shapes. Runs no other test."
		},
		ARG_TABLE_ENDMARKER
	};

	native_add_command_line_opts(m5_options);
}

NATIVE_TASK(m5_add_options, PRE_BOOT_1, 10);

/*
 * GUARD 2 of 2. The build-time guard proves the devicetree resolves a GPIO chip
 * select; this one proves the constructed objects actually carry it.
 *
 * A config whose cs_is_gpio is false makes pdg_spi_cs_control_checked() return
 * 0 at pdg_spi.c:244 WITHOUT calling gpio_pin_set_dt(), so no value is ever
 * written to pin 2. The transfer still succeeds, still echoes byte-exactly, and
 * still returns 0; the witness reads the init-time inactive HIGH and T3a fails
 * with no indication of why. This check turns that into an immediate, named,
 * pre-test failure. It is exactly what would have caught the defect that got
 * this far.
 */
static int m5_require_gpio_cs(const struct spi_config *cfg, const char *name)
{
	if (!spi_cs_is_gpio(cfg)) {
		printk("M5_ACCEPTANCE_FAIL step=PRECHECK reason=config-has-no-gpio-cs "
		       "config=%s note=spi_cs_is_gpio() is false, so the driver would "
		       "issue no chip-select edge and every transfer would succeed with "
		       "CS never asserted\n", name);
		return -1;
	}

	if (cfg->cs.gpio.port == NULL) {
		printk("M5_ACCEPTANCE_FAIL step=PRECHECK reason=null-cs-port config=%s\n",
		       name);
		return -1;
	}

	if (!device_is_ready(cfg->cs.gpio.port)) {
		printk("M5_ACCEPTANCE_FAIL step=PRECHECK reason=cs-port-not-ready "
		       "config=%s port=%s\n", name, cfg->cs.gpio.port->name);
		return -1;
	}

	if (cfg->cs.gpio.pin != M5_CS_PIN) {
		printk("M5_ACCEPTANCE_FAIL step=PRECHECK reason=unexpected-cs-pin "
		       "config=%s expected=%u observed=%u\n",
		       name, M5_CS_PIN, cfg->cs.gpio.pin);
		return -1;
	}

	printk("M5_PRECHECK config=%s cs_is_gpio=1 pin=%u\n", name, cfg->cs.gpio.pin);

	return 0;
}

static int m5_precheck_configs(void)
{
	if (m5_require_gpio_cs(&m5_cfg_hold, "cfg_hold") != 0 ||
	    m5_require_gpio_cs(&m5_cfg_plain, "cfg_plain") != 0 ||
	    m5_require_gpio_cs(&m5_cfg_hold_nolock, "cfg_hold_nolock") != 0 ||
	    m5_require_gpio_cs(&m5_cfg_slow, "cfg_slow") != 0 ||
	    m5_require_gpio_cs(&m5_cfg_fast, "cfg_fast") != 0) {
		return -1;
	}

	return 0;
}

/*
 * ------------------------------------------------------------------------
 * Empirical payload-ceiling sweep (--ceiling-sweep)
 * ------------------------------------------------------------------------
 *
 * WHY THIS EXISTS. Two failures, neither of which isolates the constraint:
 *
 *   - 4096 TX-only          -> -ECOMM (transport)
 *   - 3072 TX + 3072 RX     -> -ECOMM (transport)
 *
 * The asymmetry nobody accounted for is that the packet budget must cover the
 * REQUEST frame and the RESPONSE frame. Every estimate so far, including the
 * firmware's PacketBuffers<MAX_TRANSFER_SIZE + 1024> reasoning, considered one
 * direction only. TX-only and full duplex may therefore have DIFFERENT
 * ceilings, and which one binds is the entire question. So both shapes are
 * swept independently.
 *
 * Binary search, not a linear crawl, and EVERY probe is logged with its length
 * and exact errno so the transition point is visible in the log rather than
 * inferred from a final number.
 *
 * THE SWEEP DOES NOT DEPEND ON PDG_SPI_MAX_BUFFER having any particular value,
 * and deliberately does not read it. It probes lengths and classifies what
 * comes back. See m5_sweep_report() for the one limitation this creates.
 */
#define M5_SWEEP_MAX_PROBE 4096
#define M5_SWEEP_MAX_ITERATIONS 20

/* Defined below with the rest of the witness helpers. */
static int m5_witness_require(const char *where, int expect);

enum m5_probe_result {
	M5_PROBE_OK,
	M5_PROBE_TRANSPORT,	/* -ECOMM: the framed packet is over the wire limit */
	M5_PROBE_LOCAL,		/* -EMSGSIZE: above the compiled PDG_SPI_MAX_BUFFER */
	M5_PROBE_BAD_DATA,	/* returned 0 but the echo did not match */
	M5_PROBE_UNEXPECTED,	/* premise violated -- stop the sweep */
};

static enum m5_probe_result m5_probe(const char *shape, size_t len, bool duplex,
				     int *out_errno)
{
	const struct spi_buf tx = { .buf = m5_big_tx, .len = len };
	const struct spi_buf_set tx_set = { .buffers = &tx, .count = 1 };
	const struct spi_buf rx = { .buf = m5_big_rx, .len = len };
	const struct spi_buf_set rx_set = { .buffers = &rx, .count = 1 };
	enum m5_probe_result result;
	const char *rx_state = "n/a";
	int ret;

	if (duplex) {
		memset(m5_big_rx, M5_POISON, len);
	}

	ret = spi_transceive(m5_spi, &m5_cfg_plain, &tx_set, duplex ? &rx_set : NULL);
	*out_errno = ret;

	if (ret == 0) {
		/*
		 * A transfer that "succeeds" but returns garbage is not a
		 * usable ceiling. On a MOSI<->MISO short this costs nothing to
		 * check, so it is always checked.
		 */
		if (duplex) {
			if (memcmp(m5_big_tx, m5_big_rx, len) == 0) {
				rx_state = "ok";
				result = M5_PROBE_OK;
			} else {
				rx_state = "mismatch";
				result = M5_PROBE_BAD_DATA;
			}
		} else {
			result = M5_PROBE_OK;
		}
	} else if (ret == -ECOMM) {
		result = M5_PROBE_TRANSPORT;
	} else if (ret == -EMSGSIZE) {
		result = M5_PROBE_LOCAL;
	} else {
		result = M5_PROBE_UNEXPECTED;
	}

	printk("M5_SWEEP shape=%s len=%zu errno=%d rx=%s class=%s\n",
	       shape, len, ret, rx_state,
	       (result == M5_PROBE_OK) ? "ok" :
	       (result == M5_PROBE_TRANSPORT) ? "transport" :
	       (result == M5_PROBE_LOCAL) ? "local" :
	       (result == M5_PROBE_BAD_DATA) ? "bad-data" : "unexpected");

	return result;
}

/*
 * Largest length in [1, M5_SWEEP_MAX_PROBE] that transfers cleanly.
 *
 * Returns 0 on success and fills *out_ceiling and *out_first_fail_class; returns
 * -1 if any probe produced an unexpected errno or bad data, because either means
 * the sweep's premise is wrong and a "ceiling" derived from it would be a guess
 * dressed up as a measurement.
 */
static int m5_sweep_shape(const char *shape, bool duplex, size_t *out_ceiling,
			  enum m5_probe_result *out_first_fail_class)
{
	size_t lo = 1U;
	size_t hi = M5_SWEEP_MAX_PROBE;
	size_t best = 0U;
	enum m5_probe_result fail_class = M5_PROBE_UNEXPECTED;
	int iterations = 0;
	int err;

	/* Anchor: the smallest useful transfer must work, or nothing below is
	 * trustworthy and a binary search over a broken bus is meaningless.
	 */
	switch (m5_probe(shape, lo, duplex, &err)) {
	case M5_PROBE_OK:
		best = lo;
		break;
	default:
		printk("M5_SWEEP_FAIL shape=%s reason=anchor-probe-failed len=%zu errno=%d\n",
		       shape, lo, err);
		return -1;
	}

	lo = 2U;
	while (lo <= hi && iterations < M5_SWEEP_MAX_ITERATIONS) {
		size_t mid = lo + ((hi - lo) / 2U);

		iterations++;

		switch (m5_probe(shape, mid, duplex, &err)) {
		case M5_PROBE_OK:
			best = mid;
			lo = mid + 1U;
			break;
		case M5_PROBE_TRANSPORT:
			fail_class = M5_PROBE_TRANSPORT;
			hi = mid - 1U;
			break;
		case M5_PROBE_LOCAL:
			fail_class = M5_PROBE_LOCAL;
			hi = mid - 1U;
			break;
		case M5_PROBE_BAD_DATA:
			printk("M5_SWEEP_FAIL shape=%s reason=echo-mismatch-on-success "
			       "len=%zu\n", shape, mid);
			return -1;
		case M5_PROBE_UNEXPECTED:
		default:
			printk("M5_SWEEP_FAIL shape=%s reason=unexpected-errno len=%zu "
			       "errno=%d note=expected 0, -ECOMM or -EMSGSIZE; the sweep "
			       "premise does not hold\n", shape, mid, err);
			return -1;
		}
	}

	if (iterations >= M5_SWEEP_MAX_ITERATIONS) {
		printk("M5_SWEEP_FAIL shape=%s reason=iteration-bound-exceeded\n", shape);
		return -1;
	}

	*out_ceiling = best;
	*out_first_fail_class = fail_class;

	return 0;
}

static void m5_sweep_report(const char *shape, size_t ceiling,
			    enum m5_probe_result fail_class)
{
	printk("M5_SWEEP_%s_CEILING=%zu\n", shape, ceiling);
	printk("M5_SWEEP_%s_FIRST_FAIL=%zu\n", shape, ceiling + 1U);

	/*
	 * THE ONE LIMITATION, stated in the output rather than left for a reader
	 * to deduce.
	 *
	 * bufset_len_() rejects anything above the compiled PDG_SPI_MAX_BUFFER
	 * with -EMSGSIZE before the transport is ever reached, so the sweep can
	 * only explore AT OR BELOW that constant. Two cases:
	 *
	 *   limited_by=transport         the first failure was -ECOMM, so the
	 *                                true wire ceiling is strictly inside
	 *                                the explored range and this number is
	 *                                the real answer.
	 *
	 *   limited_by=compiled-constant the first failure was -EMSGSIZE, i.e.
	 *                                everything the wire can carry also
	 *                                passed. The sweep CANNOT see past the
	 *                                constant; the true ceiling is at or
	 *                                above it and the constant must be
	 *                                raised before re-sweeping.
	 */
	if (fail_class == M5_PROBE_LOCAL) {
		printk("M5_SWEEP_%s_LIMITED_BY=compiled-constant\n", shape);
		printk("M5_SWEEP_%s_NOTE=the sweep hit the local -EMSGSIZE check before "
		       "any transport failure, so the true ceiling is AT OR ABOVE this "
		       "value and is not visible from here; raise PDG_SPI_MAX_BUFFER and "
		       "re-sweep\n", shape);
	} else if (fail_class == M5_PROBE_TRANSPORT) {
		printk("M5_SWEEP_%s_LIMITED_BY=transport\n", shape);
	} else {
		printk("M5_SWEEP_%s_LIMITED_BY=unknown\n", shape);
	}
}

static int m5_run_ceiling_sweep(void)
{
	size_t tx_ceiling = 0U;
	size_t duplex_ceiling = 0U;
	enum m5_probe_result tx_fail = M5_PROBE_UNEXPECTED;
	enum m5_probe_result duplex_fail = M5_PROBE_UNEXPECTED;

	printk("M5_SWEEP_BEGIN max_probe=%d\n", M5_SWEEP_MAX_PROBE);

	if (m5_sweep_shape("TXONLY", false, &tx_ceiling, &tx_fail) != 0) {
		return -1;
	}
	if (m5_sweep_shape("DUPLEX", true, &duplex_ceiling, &duplex_fail) != 0) {
		return -1;
	}

	m5_sweep_report("TXONLY", tx_ceiling, tx_fail);
	m5_sweep_report("DUPLEX", duplex_ceiling, duplex_fail);

	/* Full duplex is expected to bind, since its packet budget carries both
	 * frames. Report which one actually did rather than assuming.
	 */
	printk("M5_SWEEP_BINDING_SHAPE=%s\n",
	       (duplex_ceiling <= tx_ceiling) ? "DUPLEX" : "TXONLY");
	printk("M5_SWEEP_USABLE_CEILING=%zu\n",
	       (duplex_ceiling <= tx_ceiling) ? duplex_ceiling : tx_ceiling);

	/*
	 * Leave the bench clean. Every probe used m5_cfg_plain -- no HOLD, no
	 * LOCK -- so each transfer deasserted chip select on completion, and the
	 * sweep creates no GPIO subscriptions at all. Confirm the line rather
	 * than assert it.
	 */
	if (m5_witness_require("SWEEP_FINAL_CS_DEASSERTED", 1) != 0) {
		return -1;
	}

	printk("M5_SWEEP_DONE\n");

	return 0;
}

/* Read the witness. Returns 0/1, or negative on a read error. */
static int m5_witness_read(const char *where)
{
	int level = gpio_pin_get_dt(&m5_witness);

	if (level < 0) {
		printk("M5_ACCEPTANCE_FAIL step=%s reason=witness-read errno=%d\n",
		       where, level);
		return level;
	}

	printk("M5_WITNESS step=%s level=%d\n", where, level);

	return level;
}

static int m5_witness_require(const char *where, int expect)
{
	int level = m5_witness_read(where);

	if (level < 0) {
		return -1;
	}

	if (level != expect) {
		printk("M5_ACCEPTANCE_FAIL step=%s reason=witness-level expected=%d observed=%d\n",
		       where, expect, level);
		return -1;
	}

	return 0;
}

/*
 * Errno -> symbolic name, for every value any M5 assertion depends on.
 *
 * WHY THIS EXISTS. The runner used to grep for numeric literals (-108, -122).
 * Those are Zephyr MINIMAL-LIBC values, but native_sim links the HOST glibc,
 * where EHOSTDOWN is 112 and EMSGSIZE is 90. The literals therefore never
 * matched, and the runner would have reported fault_latch=INCONCLUSIVE and
 * payload_verdict=FAIL on a run where BOTH ACTUALLY PASSED -- silently
 * discrediting the two highest-value results in the milestone.
 *
 * Emitting the symbol alongside the number fixes the class, not the instance:
 * the C side resolves <errno.h> for whatever libc it was built against, and the
 * runner matches `sym=NAME`, so a libc change cannot silently move a verdict
 * again. The numeric value is still printed, for the human reading the log.
 */
static const char *m5_errno_sym(int err)
{
	switch (err) {
	case 0:
		return "OK";
	case -EBUSY:
		return "EBUSY";
	case -EHOSTDOWN:
		return "EHOSTDOWN";
	case -EMSGSIZE:
		return "EMSGSIZE";
	case -ECOMM:
		return "ECOMM";
	case -EINVAL:
		return "EINVAL";
	case -ENOTSUP:
		return "ENOTSUP";
	case -ENOENT:
		return "ENOENT";
	case -ENODEV:
		return "ENODEV";
	case -ENOMEM:
		return "ENOMEM";
	case -EACCES:
		return "EACCES";
	default:
		return "UNKNOWN";
	}
}

static int m5_require_errno(const char *step, int observed, int expect)
{
	printk("M5_%s_RESULT=%d sym=%s\n", step, observed, m5_errno_sym(observed));

	if (observed != expect) {
		printk("M5_ACCEPTANCE_FAIL step=%s reason=errno expected=%d observed=%d\n",
		       step, expect, observed);
		return -1;
	}

	return 0;
}

/*
 * T2 classifier (test design §4.3). Reports the NAMED corruption, never a bare
 * "mismatch": a deterministic shift is a mode/fixture limitation and makes M5
 * INCONCLUSIVE, while bit reversal, stuck-at and poison-intact are outright
 * failures with different causes.
 */
enum m5_echo_verdict {
	M5_ECHO_EXACT,
	M5_ECHO_LAG,
	M5_ECHO_SHIFT_LEFT,
	M5_ECHO_SHIFT_RIGHT,
	M5_ECHO_BIT_REVERSED,
	M5_ECHO_STUCK_0,
	M5_ECHO_STUCK_1,
	M5_ECHO_POISON_INTACT,
	M5_ECHO_UNCLASSIFIED,
};

static bool m5_all(const uint8_t *rx, uint8_t v)
{
	for (size_t i = 0U; i < sizeof(m5_echo_tx); i++) {
		if (rx[i] != v) {
			return false;
		}
	}

	return true;
}

static bool m5_eq(const uint8_t *rx, const uint8_t *expect)
{
	return memcmp(rx, expect, sizeof(m5_echo_tx)) == 0;
}

static enum m5_echo_verdict m5_classify(const uint8_t *rx)
{
	static const uint8_t lag[5] = { M5_POISON, 0x96, 0x2D, 0xE1, 0x4B };
	static const uint8_t shl_a[5] = { 0x2C, 0x5B, 0xC2, 0x96, 0xE6 };
	static const uint8_t shl_b[5] = { 0x2C, 0x5B, 0xC2, 0x96, 0xE7 };
	static const uint8_t shr_a[5] = { 0x4B, 0x16, 0xF0, 0xA5, 0xB9 };
	static const uint8_t shr_b[5] = { 0xCB, 0x16, 0xF0, 0xA5, 0xB9 };
	static const uint8_t rev[5] = { 0x69, 0xB4, 0x87, 0xD2, 0xCE };

	if (m5_eq(rx, m5_echo_tx)) {
		return M5_ECHO_EXACT;
	}
	if (m5_all(rx, M5_POISON)) {
		return M5_ECHO_POISON_INTACT;
	}
	if (m5_all(rx, 0x00)) {
		return M5_ECHO_STUCK_0;
	}
	if (m5_all(rx, 0xFF)) {
		return M5_ECHO_STUCK_1;
	}
	if (m5_eq(rx, lag)) {
		return M5_ECHO_LAG;
	}
	if (m5_eq(rx, shl_a) || m5_eq(rx, shl_b)) {
		return M5_ECHO_SHIFT_LEFT;
	}
	if (m5_eq(rx, shr_a) || m5_eq(rx, shr_b)) {
		return M5_ECHO_SHIFT_RIGHT;
	}
	if (m5_eq(rx, rev)) {
		return M5_ECHO_BIT_REVERSED;
	}

	return M5_ECHO_UNCLASSIFIED;
}

static const char *m5_echo_name(enum m5_echo_verdict v)
{
	switch (v) {
	case M5_ECHO_EXACT:
		return "EXACT_ECHO";
	case M5_ECHO_LAG:
		return "WHOLE_BYTE_LAG";
	case M5_ECHO_SHIFT_LEFT:
		return "ONE_BIT_LEFT_SHIFT";
	case M5_ECHO_SHIFT_RIGHT:
		return "ONE_BIT_RIGHT_SHIFT";
	case M5_ECHO_BIT_REVERSED:
		return "PER_BYTE_BIT_REVERSAL";
	case M5_ECHO_STUCK_0:
		return "STUCK_AT_0";
	case M5_ECHO_STUCK_1:
		return "STUCK_AT_1";
	case M5_ECHO_POISON_INTACT:
		return "RX_NEVER_WRITTEN";
	case M5_ECHO_UNCLASSIFIED:
		return "UNCLASSIFIED_MISMATCH";
	}

	return "UNKNOWN";
}

/* One 5-byte echo transfer with the RX buffer poisoned first. */
static int m5_echo_once(struct spi_config *cfg, const char *step)
{
	const struct spi_buf tx = { .buf = (void *)m5_echo_tx, .len = sizeof(m5_echo_tx) };
	const struct spi_buf_set tx_set = { .buffers = &tx, .count = 1 };
	const struct spi_buf rx = { .buf = m5_echo_rx, .len = sizeof(m5_echo_rx) };
	const struct spi_buf_set rx_set = { .buffers = &rx, .count = 1 };
	enum m5_echo_verdict verdict;
	int ret;

	memset(m5_echo_rx, M5_POISON, sizeof(m5_echo_rx));

	ret = spi_transceive(m5_spi, cfg, &tx_set, &rx_set);
	if (ret != 0) {
		printk("M5_ACCEPTANCE_FAIL step=%s reason=transceive errno=%d\n", step, ret);
		return -1;
	}

	verdict = m5_classify(m5_echo_rx);
	printk("M5_ECHO step=%s classification=%s rx=%02x,%02x,%02x,%02x,%02x\n",
	       step, m5_echo_name(verdict), m5_echo_rx[0], m5_echo_rx[1],
	       m5_echo_rx[2], m5_echo_rx[3], m5_echo_rx[4]);

	if (verdict == M5_ECHO_EXACT) {
		return 0;
	}

	/*
	 * A deterministic shift or lag is a mode/fixture limitation, so the
	 * milestone is INCONCLUSIVE rather than FAIL, and the runner must stop
	 * before loopback either way: 26 identical byte-comparison failures
	 * against a known shift say nothing.
	 */
	if (verdict == M5_ECHO_LAG || verdict == M5_ECHO_SHIFT_LEFT ||
	    verdict == M5_ECHO_SHIFT_RIGHT) {
		printk("M5_ACCEPTANCE_INCONCLUSIVE step=%s classification=%s\n",
		       step, m5_echo_name(verdict));
	} else {
		printk("M5_ACCEPTANCE_FAIL step=%s reason=echo classification=%s\n",
		       step, m5_echo_name(verdict));
	}

	return -1;
}

/*
 * T2 -- mode sweep 0..3.
 *
 * HONEST LIMITATION, recorded here and in explicitly_untested: a MOSI<->MISO
 * short is MODE-BLIND. The same clock drives shift-out and shift-in, so the
 * short echoes byte-exactly for any consistent CPOL/CPHA. This proves the four
 * modes are ACCEPTED and the path stays byte-exact. It does NOT prove CPOL/CPHA
 * are mapped correctly onto the wire, and no test on this fixture can.
 */
static int m5_run_t2(void)
{
	static struct spi_config mode_cfg[4];
	static const uint32_t mode_flags[4] = {
		0,
		SPI_MODE_CPHA,
		SPI_MODE_CPOL,
		SPI_MODE_CPOL | SPI_MODE_CPHA,
	};
	static const char *const mode_step[4] = { "T2_MODE0", "T2_MODE1", "T2_MODE2", "T2_MODE3" };

	for (int i = 0; i < 4; i++) {
		mode_cfg[i] = m5_cfg_plain;
		mode_cfg[i].operation = SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | mode_flags[i];

		if (m5_echo_once(&mode_cfg[i], mode_step[i]) != 0) {
			return -1;
		}
	}

	printk("M5_T2_PASS\n");

	return 0;
}

/* T3 -- chip-select lifecycle under SPI_HOLD_ON_CS | SPI_LOCK_ON. */
static int m5_run_t3(void)
{
	int ret;

	/* Step 1: pre-call baseline. Weak on its own; the far end of T3b. */
	if (m5_witness_require("T3_STEP1_BASELINE", 1) != 0) {
		return -1;
	}

	/* Step 2/3: transfer under HOLD+LOCK, then the load-bearing LOW. */
	if (m5_echo_once(&m5_cfg_hold, "T3_STEP2_HOLD_TRANSFER") != 0) {
		return -1;
	}

	/*
	 * THE load-bearing measurement of the milestone. Pin 3 holds the shared
	 * node up through its own pull-up; the only thing that can bring it low
	 * is pin 2 actively driving. This is also the negative control: delete
	 * pdg_spi_cs_control_checked() from pdg_spi_transceive() and this is the
	 * ONLY assertion in the entire M5 suite that fails -- a loopback passes
	 * regardless of what chip select does.
	 */
	if (m5_witness_require("T3A_CS_ASSERTED", 0) != 0) {
		printk("M5_ACCEPTANCE_FAIL step=T3A reason=cs-not-asserted\n");
		return -1;
	}
	printk("M5_T3A_PASS\n");

	/* Step 4/5: release, and require the LOW->HIGH transition. */
	ret = spi_release(m5_spi, &m5_cfg_hold);
	if (m5_require_errno("T3B_RELEASE", ret, 0) != 0) {
		return -1;
	}
	if (m5_witness_require("T3B_CS_DEASSERTED", 1) != 0) {
		return -1;
	}
	printk("M5_T3B_TRANSITION=LOW_TO_HIGH\n");

	/* Step 6: a successful release cleared ctx->config, so replay is -EINVAL. */
	ret = spi_release(m5_spi, &m5_cfg_hold);
	if (m5_require_errno("T3C_SECOND_RELEASE", ret, -EINVAL) != 0) {
		return -1;
	}

	/* Step 7: the lock was genuinely given back -- a DIFFERENT config works. */
	if (m5_echo_once(&m5_cfg_plain, "T3D_DIFFERENT_CONFIG") != 0) {
		return -1;
	}
	if (m5_witness_require("T3D_AFTER", 1) != 0) {
		return -1;
	}

	/*
	 * Step 9: HOLD without LOCK. The witness clause is not decoration:
	 * -ENOTSUP alone is consistent with a driver that asserted CS, noticed
	 * the flag problem, and returned without deasserting. HIGH *unchanged*
	 * across the call is what proves the rejection at pdg_spi.c:413 happens
	 * before pdg_spi_cs_control_checked() at :505.
	 */
	{
		const struct spi_buf tx = { .buf = (void *)m5_echo_tx,
					    .len = sizeof(m5_echo_tx) };
		const struct spi_buf_set tx_set = { .buffers = &tx, .count = 1 };

		ret = spi_transceive(m5_spi, &m5_cfg_hold_nolock, &tx_set, NULL);
		if (m5_require_errno("T3E_HOLD_WITHOUT_LOCK", ret, -ENOTSUP) != 0) {
			return -1;
		}
		if (m5_witness_require("T3E_UNCHANGED", 1) != 0) {
			return -1;
		}
	}

	printk("M5_T3_PASS\n");

	return 0;
}

static void m5_sort(uint32_t *a, size_t n)
{
	for (size_t i = 1U; i < n; i++) {
		uint32_t key = a[i];
		size_t j = i;

		while (j > 0U && a[j - 1U] > key) {
			a[j] = a[j - 1U];
			j--;
		}
		a[j] = key;
	}
}

/* Nearest-rank percentile over a sorted ascending array. */
static uint32_t m5_pct(const uint32_t *sorted, size_t n, unsigned int pct)
{
	size_t rank = (size_t)((pct * n + 99U) / 100U);

	if (rank == 0U) {
		rank = 1U;
	}

	return sorted[rank - 1U];
}

/*
 * Timing measurement at the EXACT spi-max-frequency literals declared for
 * m5slow@0 and m5fast@0. Measuring at any other operating point derives the
 * multiplier from the wrong ratio, and the error is silent (test design §8.5).
 *
 * TIME COMES FROM THE HOST CLOCK, NOT THE ZEPHYR CLOCK. On native_sim the
 * Zephyr clock measures SIMULATED time, which does not advance while the host
 * thread is blocked inside a USB call -- and every operation here is exactly
 * such a call. Measured with k_cycle_get_32(): p50 = p95 = p99 = max = 0 us
 * across 25 real, correctly-echoing transfers at each of two frequencies, which
 * drove the derived multiplier to ceil(1.25 * 0 / t) = 0. That is not an
 * imprecise measurement, it is a vacuous one. m5_bottom_host_monotonic_us()
 * returns real elapsed wall-clock microseconds instead.
 *
 * The same limitation applies to upstream spi_loopback's
 * test_spi_complete_multiple_timed, which uses the Zephyr clock and therefore
 * passes vacuously on this target. That is recorded in explicitly_untested, not
 * counted as coverage.
 *
 * The ratio is USB-latency dominated: wall time is roughly four USB round trips
 * regardless of clock rate, while theoretical time shrinks as frequency rises.
 * FAST is what binds. The executor computes
 * ceil((1.25 * observed_max_us) / theoretical_minimum_us) and takes the maximum
 * over both modes. If FAST demands more than 256, the remedy is a LOWER fast@0
 * frequency, never a larger multiplier -- a large multiplier recreates a
 * vacuous timing test.
 */
static int m5_run_timing(struct spi_config *cfg, const char *tag, uint32_t hz)
{
	static uint32_t samples[M5_TIMED_ITERATIONS];
	const struct spi_buf tx = { .buf = m5_timed_tx, .len = M5_TIMED_LEN };
	const struct spi_buf_set tx_set = { .buffers = &tx, .count = 1 };
	const struct spi_buf rx = { .buf = m5_timed_rx, .len = M5_TIMED_LEN };
	const struct spi_buf_set rx_set = { .buffers = &rx, .count = 1 };

	for (int i = 0; i < M5_TIMED_ITERATIONS; i++) {
		uint64_t start = m5_bottom_host_monotonic_us();
		int ret = spi_transceive(m5_spi, cfg, &tx_set, &rx_set);
		uint64_t elapsed = m5_bottom_host_monotonic_us() - start;

		if (ret != 0) {
			printk("M5_ACCEPTANCE_FAIL step=TIMING_%s reason=transceive "
			       "iteration=%d errno=%d\n", tag, i, ret);
			return -1;
		}

		/* Only healthy transfers are measured, so the bytes must match. */
		if (memcmp(m5_timed_tx, m5_timed_rx, M5_TIMED_LEN) != 0) {
			printk("M5_ACCEPTANCE_FAIL step=TIMING_%s reason=echo-mismatch "
			       "iteration=%d\n", tag, i);
			return -1;
		}

		samples[i] = (elapsed > UINT32_MAX) ? UINT32_MAX : (uint32_t)elapsed;
	}

	m5_sort(samples, M5_TIMED_ITERATIONS);

	printk("M5_TIMING_%s_FREQUENCY_HZ=%u\n", tag, hz);
	printk("M5_TIMING_%s_P50_US=%u\n", tag, m5_pct(samples, M5_TIMED_ITERATIONS, 50));
	printk("M5_TIMING_%s_P95_US=%u\n", tag, m5_pct(samples, M5_TIMED_ITERATIONS, 95));
	printk("M5_TIMING_%s_P99_US=%u\n", tag, m5_pct(samples, M5_TIMED_ITERATIONS, 99));
	printk("M5_TIMING_%s_MAX_US=%u\n", tag, samples[M5_TIMED_ITERATIONS - 1]);
	printk("M5_TIMING_%s_SAMPLES=%d\n", tag, M5_TIMED_ITERATIONS);

	return 0;
}

/* One oversized-payload probe with the witness sampled before and after. */
static int m5_payload_reject(const char *step, size_t len_a, size_t len_b, int expect)
{
	struct spi_buf tx[2];
	struct spi_buf_set tx_set = { .buffers = tx, .count = (len_b != 0U) ? 2U : 1U };
	int ret;

	tx[0].buf = m5_big_tx;
	tx[0].len = len_a;
	tx[1].buf = m5_big_tx + len_a;
	tx[1].len = len_b;

	if (m5_witness_require(step, 1) != 0) {
		return -1;
	}

	ret = spi_transceive(m5_spi, &m5_cfg_plain, &tx_set, NULL);
	if (m5_require_errno(step, ret, expect) != 0) {
		return -1;
	}

	/*
	 * The witness clause is what makes "rejected LOCALLY" observable.
	 * bufset_len_() runs at pdg_spi.c:438/:442, before k_malloc (:457),
	 * before spi_context_lock (:471), before set-config (:494) and before
	 * pdg_spi_cs_control_checked (:505). A local rejection therefore issues
	 * no chip-select edge. An errno alone could be produced by a driver that
	 * rejects AFTER asserting.
	 */
	if (m5_witness_require(step, 1) != 0) {
		printk("M5_ACCEPTANCE_FAIL step=%s reason=cs-edge-during-local-rejection\n",
		       step);
		return -1;
	}

	return 0;
}

/*
 * T5 -- payload boundary.
 *
 * GOVERNING PRINCIPLE: every case here is either (a) a length MEASURED to work,
 * or (b) a length expected to be rejected LOCALLY with -EMSGSIZE before any bus
 * traffic. No T5 case puts an unmeasured length on the wire. That makes the
 * 1015-byte firmware hang window unreachable by construction rather than by
 * care, and it keeps unmeasured assumptions out of the boundary tests -- which
 * is what two earlier rounds of this milestone got wrong.
 *
 * Consequently there is deliberately NO case at 1015, 1016 or 3072.
 */
static int m5_run_t5(void)
{
	const struct spi_buf tx_ceiling = { .buf = m5_big_tx, .len = M5_SPI_CEILING };
	const struct spi_buf_set tx_ceiling_set = { .buffers = &tx_ceiling, .count = 1 };
	const struct spi_buf tx_duplex = { .buf = m5_big_tx, .len = M5_SPI_DUPLEX_SAFE };
	const struct spi_buf_set tx_duplex_set = { .buffers = &tx_duplex, .count = 1 };
	const struct spi_buf rx_duplex = { .buf = m5_big_rx, .len = M5_SPI_DUPLEX_SAFE };
	const struct spi_buf_set rx_duplex_set = { .buffers = &rx_duplex, .count = 1 };
	int ret;

	/*
	 * T5a -- the ceiling case, TX-ONLY BY DELIBERATE CHOICE.
	 *
	 * DO NOT "improve" this into a full-duplex transfer. TX-only is the
	 * shape that was actually measured on hardware; the duplex ceiling has
	 * never been measured at any working length, and the only duplex data
	 * point (3072) fails -ECOMM. Making this duplex would put an unmeasured
	 * length on the wire, which is exactly what this test set exists to
	 * avoid. T5e covers the duplex SHAPE separately, at a safe length.
	 *
	 * Load-bearing in the opposite direction from T5b-T5d: a driver that
	 * rejects EVERYTHING with -EMSGSIZE passes b, c and d and fails here.
	 * bufset_len_ uses '>' at pdg_spi.c:293, so an off-by-one to '>=' also
	 * fails here.
	 *
	 * No RX comparison is possible for a TX-only transfer; the assertion is
	 * that the transfer completes.
	 */
	ret = spi_transceive(m5_spi, &m5_cfg_plain, &tx_ceiling_set, NULL);
	if (m5_require_errno("T5A_CEILING_TXONLY", ret, 0) != 0) {
		printk("M5_ACCEPTANCE_FAIL step=T5A reason=measured-ceiling-rejected "
		       "len=%u note=%u is the largest length measured to work on "
		       "hardware; a failure here means the ceiling moved\n",
		       M5_SPI_CEILING, M5_SPI_CEILING);
		return -1;
	}
	printk("M5_T5A_LENGTH=%u shape=tx-only\n", M5_SPI_CEILING);

	/* T5b -- first length over the line. Rejected locally, no bus traffic. */
	if (m5_payload_reject("T5B_OVER_CEILING", M5_SPI_OVER_CEILING, 0, -EMSGSIZE) != 0) {
		return -1;
	}
	printk("M5_T5B_LENGTH=%u\n", M5_SPI_OVER_CEILING);

	/*
	 * T5c -- the AFTER arm of the controlled 4096 experiment.
	 *
	 * The BEFORE arm, run against a tree still compiling 4096U, measured
	 * errno=-70 (-ECOMM) from pdg_spi_bottom_transfer(): the call reached
	 * the transport. The AFTER arm must be a LOCAL rejection instead. The
	 * errno is the discriminator, and m5_payload_reject() additionally
	 * requires the witness to read HIGH immediately before and immediately
	 * after the call -- direct electrical evidence that no chip-select edge
	 * was issued and therefore that no bus transaction began.
	 */
	if (m5_payload_reject("T5C_4096", 4096, 0, -EMSGSIZE) != 0) {
		return -1;
	}
	printk("M5_T5C_AFTER_ARM len=4096 result=-EMSGSIZE origin=local "
	       "before_arm=-ECOMM(-70) origin=transport verdict=regression-closed\n");

	/*
	 * T5d -- accumulation, not per-buffer. bufset_len_'s check is
	 * `buf->len > PDG_SPI_MAX_BUFFER - *total_len`. BOTH fragments are
	 * individually under the ceiling and only their sum is over it, so a
	 * plausible refactor to a per-buffer check passes T5a-T5c and fails
	 * only here.
	 */
	if (m5_payload_reject("T5D_ACCUMULATED", M5_SPI_DUPLEX_SAFE,
			      M5_SPI_OVER_CEILING - M5_SPI_DUPLEX_SAFE, -EMSGSIZE) != 0) {
		return -1;
	}
	printk("M5_T5D_FRAGMENTS=%u+%u total=%u\n", M5_SPI_DUPLEX_SAFE,
	       M5_SPI_OVER_CEILING - M5_SPI_DUPLEX_SAFE, M5_SPI_OVER_CEILING);

	/*
	 * T5e -- duplex SHAPE check, NOT a ceiling check.
	 *
	 * Full duplex has never been shown to work at any length: the only
	 * duplex data point on record is 3072, which fails -ECOMM. This
	 * establishes that the shape works at all, at a deliberately safe
	 * length well below the measured TX-only ceiling. It says NOTHING about
	 * where the duplex ceiling is, and must not be read as if it did.
	 */
	memset(m5_big_rx, M5_POISON, M5_SPI_DUPLEX_SAFE);
	ret = spi_transceive(m5_spi, &m5_cfg_plain, &tx_duplex_set, &rx_duplex_set);
	if (m5_require_errno("T5E_DUPLEX_SHAPE", ret, 0) != 0) {
		return -1;
	}
	if (memcmp(m5_big_tx, m5_big_rx, M5_SPI_DUPLEX_SAFE) != 0) {
		printk("M5_ACCEPTANCE_FAIL step=T5E_DUPLEX_SHAPE reason=echo-mismatch\n");
		return -1;
	}
	printk("M5_T5E_LENGTH=%u shape=full-duplex note=shape-check-only, the duplex "
	       "ceiling remains unmeasured\n", M5_SPI_DUPLEX_SAFE);

	printk("M5_T5_PASS\n");

	return 0;
}

/*
 * T4 -- fault injection: latch entry, -EHOSTDOWN, recovery.
 *
 * THE RE-CREATED SUBSCRIPTION HAZARD (test design §6.4). Between step 2 and
 * step 5 the fixture is in exactly the state the reset image exists to clean:
 * pin 2 owned by a firmware monitor task. Process death in that window -- crash,
 * timeout kill, Ctrl-C -- leaves the board in the entry-state hazard and the
 * next SPI init fails -EBUSY.
 *
 * Design response: the window is three calls with NO intervening logic, no
 * sleeps, no retries and no other I/O. Nothing may be inserted there "for
 * diagnostics". Every assertion failure inside the window exits through the
 * single fault_cleanup label.
 *
 * THE WITNESS IS DELIBERATELY NOT READ between steps 2 and 6. Subscribing sets
 * pin 2 to input, so the shared node is held solely by pin 3's pull-up and sits
 * HIGH. Asserting HIGH there would be a test that passes against a driver that
 * never deasserts -- precisely the vacuity class this suite exists to eliminate.
 *
 * Pin 3 must NOT be reconfigured or driven anywhere between step 2 and step 5.
 * That is what makes the Any-edge subscription quiescent: the node is static, so
 * the unconsumed gpio/event topic sees zero or one settling edge.
 */
static int m5_run_t4(void *ctx)
{
	const struct spi_buf tx = { .buf = (void *)m5_echo_tx, .len = sizeof(m5_echo_tx) };
	const struct spi_buf_set tx_set = { .buffers = &tx, .count = 1 };
	int ret;

	/* Step 1: assert and hold. The strong LOW whose transition step 7 closes. */
	if (m5_echo_once(&m5_cfg_hold, "T4_STEP1_HOLD") != 0) {
		return -1;
	}
	if (m5_witness_require("T4_STEP1_CS_ASSERTED", 0) != 0) {
		return -1;
	}

	/* ---- subscription window opens ---- */
	ret = m5_bottom_gpio_subscribe(ctx, M5_CS_PIN, M5_BOTTOM_GPIO_EDGE_ANY);
	printk("M5_T4_STEP2_SUBSCRIBE=%d\n", ret);
	if (ret != 0) {
		printk("M5_ACCEPTANCE_FAIL step=T4_STEP2 reason=subscribe errno=%d\n", ret);
		goto fault_cleanup;
	}

	/*
	 * Step 3. The monitor owns pin 2 and made it an input, so the checked
	 * deassert's gpio_pin_set_dt() returns -EBUSY (PinMonitored). The driver
	 * must return -EBUSY, latch, release software ownership, and RETAIN
	 * ctx->config via pdg_spi_unlock_defanged(ctx, true).
	 *
	 * This marker is flushed immediately: the bounded runner keys the E4
	 * carve-out off exactly this line being present with no step-4 result.
	 */
	ret = spi_release(m5_spi, &m5_cfg_hold);
	printk("M5_T4_STEP3_RELEASE=%d sym=%s\n", ret, m5_errno_sym(ret));
	if (ret != -EBUSY) {
		printk("M5_ACCEPTANCE_FAIL step=T4_STEP3 reason=errno expected=%d observed=%d\n",
		       -EBUSY, ret);
		goto fault_cleanup;
	}

	/*
	 * Step 4. -EHOSTDOWN is returned from exactly one place in the driver
	 * (pdg_spi.c:489, the latch branch); it appears nowhere else in
	 * pdg_spi.c or pdg_gpio.c, so there is no ambiguity about which path
	 * produced it. A 0 here would be the crash-class defect the latch exists
	 * to prevent. A block-forever here is the controller-lock leak, which
	 * the runner's E4 carve-out classifies as FAIL, not infrastructure.
	 */
	ret = spi_transceive(m5_spi, &m5_cfg_plain, &tx_set, NULL);
	printk("M5_T4_STEP4_RESULT=%d sym=%s\n", ret, m5_errno_sym(ret));
	if (ret != -EHOSTDOWN) {
		printk("M5_ACCEPTANCE_FAIL step=T4_STEP4 reason=errno expected=%d observed=%d\n",
		       -EHOSTDOWN, ret);
		goto fault_cleanup;
	}

	ret = m5_bottom_gpio_unsubscribe(ctx, M5_CS_PIN);
	printk("M5_T4_STEP5_UNSUBSCRIBE=%d\n", ret);
	if (ret != 0) {
		printk("M5_ACCEPTANCE_FAIL step=T4_STEP5 reason=unsubscribe errno=%d\n", ret);
		goto fault_cleanup;
	}
	/* ---- subscription window closes ---- */

	/*
	 * Step 6. The monitor left the physical pad an input while firmware
	 * still tracks ExplicitOutput. Reconcile the two before asking the
	 * driver to deassert again.
	 */
	ret = pdg_gpio_bottom_set_config(ctx, M5_CS_PIN, M5_BOTTOM_GPIO_DIR_OUTPUT,
					 M5_BOTTOM_GPIO_PULL_NONE);
	printk("M5_T4_STEP6_SET_CONFIG=%d\n", ret);
	if (ret != 0) {
		printk("M5_ACCEPTANCE_FAIL step=T4_STEP6 reason=set-config errno=%d\n", ret);
		return -1;
	}

	/* Step 7: retry the release with the RETAINED config; latch clears. */
	ret = spi_release(m5_spi, &m5_cfg_hold);
	if (m5_require_errno("T4_STEP7_RETRY_RELEASE", ret, 0) != 0) {
		return -1;
	}
	if (m5_witness_require("T4_STEP7_CS_DEASSERTED", 1) != 0) {
		return -1;
	}
	printk("M5_T4_STEP7_TRANSITION=LOW_TO_HIGH\n");

	/*
	 * Step 8 is NOT redundant with step 7. Step 7 proves the release path
	 * reported success; only step 8 proves data->cs_fault was actually
	 * cleared rather than the release having taken a different branch.
	 */
	if (m5_echo_once(&m5_cfg_plain, "T4_STEP8_AFTER_RECOVERY") != 0) {
		return -1;
	}

	/* Step 9: belt and braces -- proves step 5 took effect and the run
	 * leaves no subscription behind. GpioPinNotMonitored maps to -ENOENT.
	 */
	ret = m5_bottom_gpio_unsubscribe(ctx, M5_CS_PIN);
	if (m5_require_errno("T4_STEP9_UNSUBSCRIBE", ret, -ENOENT) != 0) {
		return -1;
	}

	printk("M5_T4_PASS\n");

	return 0;

fault_cleanup:
	/*
	 * Covers every assertion failure inside the window. It cannot help
	 * against process death -- that is what the reset image and the teardown
	 * subscription count are for. Return values are deliberately ignored:
	 * this is best-effort cleanup on an already-failing path.
	 */
	(void)m5_bottom_gpio_unsubscribe(ctx, M5_CS_PIN);
	(void)pdg_gpio_bottom_set_config(ctx, M5_CS_PIN, M5_BOTTOM_GPIO_DIR_OUTPUT,
					 M5_BOTTOM_GPIO_PULL_NONE);

	return -1;
}

/*
 * Acceptance spec §4.1 step 2: the pre-fix control. Runs ONLY the 4096-byte
 * case and performs no T2/T3/T5/T4 work. With the tree still at
 * PDG_SPI_MAX_BUFFER == 4096U this call passes the local check, reaches the
 * transport and is expected to fail -ECOMM (-70). Any other result stops M5 for
 * re-analysis: the sanctioned root-cause premise has not reproduced, and the fix
 * must not be applied on the strength of an unexplained reading.
 */
static int m5_run_payload_before(void)
{
	const struct spi_buf tx = { .buf = m5_big_tx, .len = 4096 };
	const struct spi_buf_set tx_set = { .buffers = &tx, .count = 1 };
	int ret = spi_transceive(m5_spi, &m5_cfg_plain, &tx_set, NULL);

	printk("M5_PAYLOAD_BEFORE_RESULT=%d sym=%s\n", ret, m5_errno_sym(ret));
	printk("M5_PAYLOAD_BEFORE_DONE\n");

	return 0;
}

int main(void)
{
	void *ctx;
	const struct device *parent;

	printk("M5_ACCEPTANCE_BEGIN\n");

	for (size_t i = 0U; i < sizeof(m5_big_tx); i++) {
		m5_big_tx[i] = (uint8_t)((i * 7U) + 3U);
	}
	for (size_t i = 0U; i < sizeof(m5_timed_tx); i++) {
		m5_timed_tx[i] = (uint8_t)((i * 5U) + 17U);
	}

	if (!device_is_ready(m5_spi)) {
		printk("M5_ACCEPTANCE_FAIL reason=spi-not-ready device=%s\n", m5_spi->name);
		posix_exit(1);
	}

	/*
	 * Precondition, not a test: run before ANY transfer in either mode. A
	 * config without a GPIO chip select produces successful, byte-exact,
	 * chip-select-less transfers, so every result below would be measuring
	 * something other than what it claims.
	 */
	if (m5_precheck_configs() != 0) {
		posix_exit(1);
	}

	if (m5_payload_before_only) {
		printk("M5_MODE=payload-before-only\n");
		if (m5_run_payload_before() != 0) {
			posix_exit(1);
		}
		posix_exit(0);
	}

	if (!device_is_ready(m5_witness.port)) {
		printk("M5_ACCEPTANCE_FAIL reason=gpio-port-not-ready port=%s\n",
		       m5_witness.port->name);
		posix_exit(1);
	}
	parent = DEVICE_DT_GET(DT_NODELABEL(pdg0));
	if (!device_is_ready(parent)) {
		printk("M5_ACCEPTANCE_FAIL reason=parent-not-ready device=%s\n", parent->name);
		posix_exit(1);
	}
	ctx = pdg_mfd_ctx(parent);
	if (ctx == NULL) {
		printk("M5_ACCEPTANCE_FAIL reason=null-context device=%s\n", parent->name);
		posix_exit(1);
	}

	/*
	 * Configure the WITNESS only. Pin 2 is deliberately untouched: SPI init
	 * already parked it GPIO_OUTPUT_INACTIVE (physically HIGH under
	 * GPIO_ACTIVE_LOW), and reconfiguring it here would destroy the baseline
	 * every reading below is measured against.
	 */
	if (gpio_pin_configure_dt(&m5_witness, GPIO_INPUT | GPIO_PULL_UP) != 0) {
		printk("M5_ACCEPTANCE_FAIL reason=witness-configure\n");
		posix_exit(1);
	}

	/*
	 * The sweep is a MEASUREMENT, not a test: it runs no T2/T3/T5/T4 work
	 * and asserts no ceiling. It needs the witness configured only for its
	 * closing bench-clean check.
	 */
	if (m5_ceiling_sweep) {
		printk("M5_MODE=ceiling-sweep\n");
		if (m5_run_ceiling_sweep() != 0) {
			posix_exit(1);
		}
		posix_exit(0);
	}

	if (m5_run_t2() != 0) {
		posix_exit(1);
	}
	if (m5_run_t3() != 0) {
		posix_exit(1);
	}
	if (m5_run_timing(&m5_cfg_slow, "SLOW", M5_SLOW_HZ) != 0) {
		posix_exit(1);
	}
	if (m5_run_timing(&m5_cfg_fast, "FAST", M5_FAST_HZ) != 0) {
		posix_exit(1);
	}
	if (m5_run_t5() != 0) {
		posix_exit(1);
	}
	if (m5_run_t4(ctx) != 0) {
		posix_exit(1);
	}

	printk("M5_ACCEPTANCE_PASS\n");

	posix_exit(0);

	return 0;
}
