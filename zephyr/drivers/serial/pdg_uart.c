/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Zephyr UART controller driver for the Pico de Gallo USB bridge.
 *
 * This file runs in the embedded/Zephyr context, which for this module means
 * the embedded half of native_sim. It translates Zephyr's polling UART API
 * into the small host-context shim declared in pdg_uart_bottom.h, which
 * forwards it to the Pico de Gallo C FFI. This translation unit never sees
 * pico_de_gallo.h and never names a Gallo* type or a Status value; the neutral
 * PDG_UART_* vocabulary in pdg_uart_bottom.h is the whole contract between the
 * two halves.
 *
 * Three properties of this controller are unusual enough to state up front,
 * because every design decision below follows from them.
 *
 *   1. It is THREAD-CONTEXT-ONLY. Every operation is a blocking USB round trip,
 *      and Zephyr asserts that a k_mutex cannot be locked from an ISR
 *      (kernel/mutex.c). A spinlock is not an escape: holding one across a USB
 *      round trip is strictly worse than refusing. So every public callback
 *      tests k_is_in_isr() and k_is_pre_kernel() BEFORE touching the mutex and
 *      BEFORE any FFI call.
 *
 *   2. poll_out() NEVER LOGS, on any path. This UART may itself be the console
 *      backend, in which case logging a failed write emits characters that each
 *      re-enter poll_out(): logging before the unlock self-deadlocks on the
 *      driver's own mutex, and logging after the unlock amplifies without
 *      bound. Diagnostics go into the private atomics instead.
 *
 *   3. poll_in() returns only 0 or exactly -1. Zephyr's polling contract has no
 *      room for a transport errno, and leaking one would invite callers to
 *      branch on a value whose meaning is a USB fault rather than a UART
 *      condition. The exact errno is latched privately.
 *
 * This device cannot serve as an early console: CONFIG_EARLY_CONSOLE installs
 * the UART console hook at PRE_KERNEL_1 and checks device_is_ready() first,
 * while this driver initializes at POST_KERNEL. A per-instance BUILD_ASSERT
 * below turns that into a readable build error rather than a silent no-console
 * boot.
 */

#define DT_DRV_COMPAT odp_pico_de_gallo_uart

#include <inttypes.h>
#include <stdint.h>

#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>
#include <zephyr/sys/atomic.h>
#include <zephyr/sys/util.h>

#include "pdg_uart_bottom.h"

/*
 * Zephyr-to-neutral configuration mapping, expressed once as constant
 * expressions.
 *
 * These macros are the ONLY place a Zephyr UART_CFG_* ordinal is turned into a
 * neutral PDG_UART_* value. The runtime validator evaluates them, and the
 * BUILD_ASSERTs further down evaluate the very same expressions, so a mapping
 * that is wrong at run time is wrong at build time too. A hand-written switch
 * with separately hand-written assertions could drift apart, which would make
 * the assertions worthless.
 *
 * UINT8_MAX is the rejection sentinel. It is not a neutral value and cannot
 * collide with one: the widest neutral field has five variants, 0 through 4.
 *
 * This matters most for stop bits. UART_CFG_STOP_BITS_1 is ordinal 1 but maps
 * to PDG_UART_STOP_BITS_1 == 0, so asserting raw ordinal equality between the
 * two vocabularies would be simply false. The assertions below pin the MAPPING
 * RESULT for each accepted pair, not an ordinal relationship.
 */
#define PDG_UART_MAP_DATA_BITS(v)						\
	((v) == UART_CFG_DATA_BITS_5 ? PDG_UART_DATA_BITS_5 :			\
	 (v) == UART_CFG_DATA_BITS_6 ? PDG_UART_DATA_BITS_6 :			\
	 (v) == UART_CFG_DATA_BITS_7 ? PDG_UART_DATA_BITS_7 :			\
	 (v) == UART_CFG_DATA_BITS_8 ? PDG_UART_DATA_BITS_8 : UINT8_MAX)

#define PDG_UART_MAP_PARITY(v)							\
	((v) == UART_CFG_PARITY_NONE  ? PDG_UART_PARITY_NONE  :			\
	 (v) == UART_CFG_PARITY_ODD   ? PDG_UART_PARITY_ODD   :			\
	 (v) == UART_CFG_PARITY_EVEN  ? PDG_UART_PARITY_EVEN  :			\
	 (v) == UART_CFG_PARITY_MARK  ? PDG_UART_PARITY_MARK  :			\
	 (v) == UART_CFG_PARITY_SPACE ? PDG_UART_PARITY_SPACE : UINT8_MAX)

#define PDG_UART_MAP_STOP_BITS(v)						\
	((v) == UART_CFG_STOP_BITS_1 ? PDG_UART_STOP_BITS_1 :			\
	 (v) == UART_CFG_STOP_BITS_2 ? PDG_UART_STOP_BITS_2 : UINT8_MAX)

/*
 * The complete set of accepted pairs, asserted through the same expressions the
 * runtime validator uses. Changing an arm of a mapping macro therefore breaks
 * the build rather than silently reframing the wire.
 *
 * These cover mapping direction and every currently accepted value. They cannot
 * detect a future appended FFI variant; that residual is closed by the eleven
 * neutral-to-FFI `_Static_assert`s in pdg_uart_bottom.c, and, for an actual
 * append, only by a crates/ change that exports a variant count.
 */
BUILD_ASSERT(PDG_UART_MAP_DATA_BITS(UART_CFG_DATA_BITS_5) == PDG_UART_DATA_BITS_5,
	     "UART_CFG_DATA_BITS_5 must map to PDG_UART_DATA_BITS_5");
BUILD_ASSERT(PDG_UART_MAP_DATA_BITS(UART_CFG_DATA_BITS_6) == PDG_UART_DATA_BITS_6,
	     "UART_CFG_DATA_BITS_6 must map to PDG_UART_DATA_BITS_6");
BUILD_ASSERT(PDG_UART_MAP_DATA_BITS(UART_CFG_DATA_BITS_7) == PDG_UART_DATA_BITS_7,
	     "UART_CFG_DATA_BITS_7 must map to PDG_UART_DATA_BITS_7");
BUILD_ASSERT(PDG_UART_MAP_DATA_BITS(UART_CFG_DATA_BITS_8) == PDG_UART_DATA_BITS_8,
	     "UART_CFG_DATA_BITS_8 must map to PDG_UART_DATA_BITS_8");

BUILD_ASSERT(PDG_UART_MAP_PARITY(UART_CFG_PARITY_NONE) == PDG_UART_PARITY_NONE,
	     "UART_CFG_PARITY_NONE must map to PDG_UART_PARITY_NONE");
BUILD_ASSERT(PDG_UART_MAP_PARITY(UART_CFG_PARITY_ODD) == PDG_UART_PARITY_ODD,
	     "UART_CFG_PARITY_ODD must map to PDG_UART_PARITY_ODD");
BUILD_ASSERT(PDG_UART_MAP_PARITY(UART_CFG_PARITY_EVEN) == PDG_UART_PARITY_EVEN,
	     "UART_CFG_PARITY_EVEN must map to PDG_UART_PARITY_EVEN");
BUILD_ASSERT(PDG_UART_MAP_PARITY(UART_CFG_PARITY_MARK) == PDG_UART_PARITY_MARK,
	     "UART_CFG_PARITY_MARK must map to PDG_UART_PARITY_MARK");
BUILD_ASSERT(PDG_UART_MAP_PARITY(UART_CFG_PARITY_SPACE) == PDG_UART_PARITY_SPACE,
	     "UART_CFG_PARITY_SPACE must map to PDG_UART_PARITY_SPACE");

BUILD_ASSERT(PDG_UART_MAP_STOP_BITS(UART_CFG_STOP_BITS_1) == PDG_UART_STOP_BITS_1,
	     "UART_CFG_STOP_BITS_1 must map to PDG_UART_STOP_BITS_1 (note: NOT ordinal "
	     "equality -- UART_CFG_STOP_BITS_1 is 1 and PDG_UART_STOP_BITS_1 is 0)");
BUILD_ASSERT(PDG_UART_MAP_STOP_BITS(UART_CFG_STOP_BITS_2) == PDG_UART_STOP_BITS_2,
	     "UART_CFG_STOP_BITS_2 must map to PDG_UART_STOP_BITS_2");

/*
 * Is this instance the node chosen as zephyr,console?
 *
 * DT_CHOSEN(zephyr_console) expands to nothing when no console is chosen, which
 * would make DT_SAME_NODE() malformed, so the evaluation is guarded by
 * DT_HAS_CHOSEN() and collapses to a plain 0 otherwise.
 */
#if DT_HAS_CHOSEN(zephyr_console)
#define PDG_UART_IS_CHOSEN_CONSOLE(inst)					\
	DT_SAME_NODE(DT_DRV_INST(inst), DT_CHOSEN(zephyr_console))
#else
#define PDG_UART_IS_CHOSEN_CONSOLE(inst) 0
#endif

/*
 * Structural topology enforcement.
 *
 * This controller borrows its host connection from an odp,pico-de-gallo MFD
 * parent reached through DT_INST_PARENT(). Runtime readiness alone cannot prove
 * the parent is the *right kind* of device: a child placed under an unrelated
 * but enabled and ready device would pass device_is_ready(), and pdg_mfd_ctx()
 * would then reinterpret that foreign driver's dev->data as struct pdg_mfd_data
 * and hand back an arbitrary pointer no NULL check can catch. DT_INST_PARENT()
 * on a stale root-level child yields `/`, so asserting status alone is likewise
 * insufficient; the compatible must be checked in its own right.
 *
 * The assertions are ordered compatible -> parent status -> parent serial
 * presence -> Kconfig -> early console, and that *source* order is the
 * normative contract. First prove this is the right enabled hardware node, then
 * prove that driving TX has an explicit board identity, then diagnose the
 * software dependency, and only then the console-role incompatibility.
 * (_Static_assert is not fatal, so GCC reports every failing assertion in one
 * pass, but C does not specify emission order; observed order is corroborating
 * evidence only.)
 *
 * The serial-presence assertion exists because a UART drives TX on a physical
 * pin, exactly as GPIO and SPI chip select do. A selector-less strict open
 * cannot report which attached board it selected, so an enabled UART under a
 * selector-less parent would transmit on unidentifiable hardware. Presence is
 * not uniqueness: two parents carrying the same explicit serial still alias to
 * one board.
 *
 * The fifth assertion is instance-scoped on purpose. CONFIG_EARLY_CONSOLE hooks
 * the console at PRE_KERNEL_1 and requires device_is_ready() (uart_console.c),
 * which a POST_KERNEL device cannot satisfy, so choosing this instance as the
 * console under early console yields a silently console-less boot. It is
 * deliberately NOT a global `!IS_ENABLED(CONFIG_EARLY_CONSOLE)` assertion: an
 * unrelated, genuinely pre-kernel UART may legitimately be the early console in
 * the same image. The runtime pre-kernel guards below remain mandatory
 * regardless, because a direct caller can still dispatch into this API.
 *
 * The whole block precedes the "pdg_mfd.h" include on purpose: when
 * CONFIG_MFD_PICO_DE_GALLO is `n` the MFD driver subdirectory is not added to
 * the build at all, so pdg_mfd.h is not on the include path. Asserting first
 * guarantees the readable configuration error is emitted before the include
 * failure, instead of an opaque "no such file" or an unresolved
 * __device_dts_ord_N at link time.
 */
#define PDG_UART_PARENT_ASSERTS(inst)						\
	BUILD_ASSERT(								\
		DT_NODE_HAS_COMPAT(DT_INST_PARENT(inst), odp_pico_de_gallo),	\
		"Enabled odp,pico-de-gallo-uart controllers must be direct "	\
		"children of an odp,pico-de-gallo parent");			\
	BUILD_ASSERT(								\
		DT_NODE_HAS_STATUS_OKAY(DT_INST_PARENT(inst)),			\
		"Enabled odp,pico-de-gallo-uart controllers require their "	\
		"odp,pico-de-gallo parent to have status okay");		\
	BUILD_ASSERT(								\
		DT_NODE_HAS_PROP(DT_INST_PARENT(inst), serial_number),		\
		"odp,pico-de-gallo-uart parent must define serial-number");	\
	BUILD_ASSERT(								\
		IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO),				\
		"Enabled Pico de Gallo child controllers require "		\
		"CONFIG_MFD_PICO_DE_GALLO=y");					\
	BUILD_ASSERT(								\
		!(IS_ENABLED(CONFIG_EARLY_CONSOLE) &&				\
		  PDG_UART_IS_CHOSEN_CONSOLE(inst)),				\
		"An odp,pico-de-gallo-uart cannot be zephyr,console while "	\
		"CONFIG_EARLY_CONSOLE=y: the console hook is installed at "	\
		"PRE_KERNEL_1 and requires device_is_ready(), which this "	\
		"POST_KERNEL device cannot satisfy. Disable "			\
		"CONFIG_EARLY_CONSOLE or choose a different console UART.");

DT_INST_FOREACH_STATUS_OKAY(PDG_UART_PARENT_ASSERTS)

#include "pdg_mfd.h"

LOG_MODULE_REGISTER(uart_pico_de_gallo, CONFIG_UART_LOG_LEVEL);

/*
 * Firmware-side read allowance, in milliseconds, for every RX refill.
 *
 * One, not zero, and the difference is not cosmetic. A zero timeout selects the
 * host library's `bounded_for(0)` path, which uses the maximum handler timeout
 * -- 30 minutes -- plus call slack, so a lost reply would hold this driver's
 * mutex for half an hour. A timeout of 1 bounds the same lost reply at roughly
 * 5.001 seconds (1 ms firmware allowance plus the default 5 s host call slack).
 *
 * One is also the smallest non-zero value the protocol can express, and it
 * preserves the load-bearing recovery path: both firmware branches call
 * AsyncRead::read(), so a non-zero timeout still reaches Embassy's try_read(),
 * which consumes the latched rx_error and re-enables the RX interrupts. This is
 * invariant RX-RECOVERY; there is deliberately no read_ready() shortcut and no
 * separate readiness probe.
 */
#define PDG_UART_READ_TIMEOUT_MS 1U

/*
 * Backoff, in milliseconds, after a non-transport RX refill error.
 *
 * Endpoint errors such as -EIO are NOT latched permanently, because the next
 * read is precisely what clears the firmware's error state -- latching would
 * make the condition self-perpetuating. But a tight poll_in() loop would then
 * issue an unbounded stream of immediate RPCs, so the next refill is deferred
 * by this interval. Ten milliseconds is larger than the 1 ms firmware poll and
 * short enough to stay interactive. A successful refill clears it.
 */
#define PDG_UART_RX_ERROR_BACKOFF_MS 10

struct pdg_uart_config {
	const struct device *mfd;
	const char *serial_number;
	struct uart_config initial;
};

struct pdg_uart_data {
	/*
	 * Borrowed from the MFD parent, never owned. Set to NULL on every init
	 * failure path after the borrow, because NULL is guardable and a
	 * valid-looking unowned pointer would bypass every check.
	 */
	void *ctx;
	struct k_mutex lock;

	/*
	 * Last successfully requested configuration -- what was asked for, not
	 * necessarily what was achieved. Firmware clamps the baud divisor at
	 * roughly 143 baud, so a request of 1 is cached and reported as 1 while
	 * the line runs at something else entirely.
	 */
	struct uart_config current;

	/* Staging ring: rx_pos <= rx_len <= PDG_UART_RX_BUFFER_SIZE. */
	uint8_t rx_buf[PDG_UART_RX_BUFFER_SIZE];
	uint16_t rx_pos;
	uint16_t rx_len;

	/*
	 * Monotonic uptime deadline before which no refill RPC is issued, or 0
	 * when no backoff is active. Mutex-protected like the ring, so it needs
	 * no atomic.
	 */
	int64_t rx_backoff_until;

	/*
	 * Private diagnostics. These are the ONLY channel poll_out() has, since
	 * it may not log. M5 keeps them driver-private and debugger/test
	 * inspectable; M6 decides whether to surface them through a shell
	 * command or a driver-specific API.
	 */
	atomic_t tx_dropped;
	atomic_t rx_errors;
	atomic_t last_errno;
	atomic_t link_failed;
};

/*
 * Uniform context policy for every public callback (spec section 5.1).
 *
 * Called before the mutex and before any FFI call, without exception. Zephyr
 * asserts !arch_is_in_isr() inside k_mutex_lock(), and a USB round trip is not
 * ISR-safe in any case, so there is no lock type that would make ISR entry
 * workable -- refusing is the only correct answer.
 *
 * Returns 0 when the caller may proceed, -EWOULDBLOCK in an ISR, -EAGAIN before
 * the kernel is up. poll_in() and poll_out() have their own return shapes and
 * translate a non-zero result rather than propagating it.
 */
static int pdg_uart_context_check(void)
{
	if (k_is_in_isr()) {
		return -EWOULDBLOCK;
	}

	if (k_is_pre_kernel()) {
		return -EAGAIN;
	}

	return 0;
}

/*
 * Record a failure in the private atomics, and latch the link when the failure
 * is transport-class.
 *
 * -ECOMM and -ETIMEDOUT mean the USB transport itself failed or its reply was
 * lost. There is no reconnect path anywhere in this module -- the parent opens
 * once, strictly, for the static device lifetime -- so the honest response is
 * permanent fail-fast rather than a retry that can only multiply stalls.
 *
 * An endpoint error such as -EIO is deliberately NOT latched: it can be an
 * ordinary UART line error, and the very next read is what consumes the
 * firmware's error latch. Latching it would make recovery impossible.
 *
 * Callers holding the mutex are responsible for discarding the staging ring
 * when this latches, because a transport failure is a stream-generation
 * boundary and bytes staged before it cannot be attributed to the new one.
 */
static void pdg_uart_latch_error(struct pdg_uart_data *data, int err)
{
	atomic_set(&data->last_errno, (atomic_val_t)err);

	if ((err == -ECOMM) || (err == -ETIMEDOUT)) {
		atomic_set(&data->link_failed, 1);
	}
}

static bool pdg_uart_link_failed(const struct pdg_uart_data *data)
{
	return atomic_get(&((struct pdg_uart_data *)data)->link_failed) != 0;
}

/*
 * Positive allow-list validation of a complete uart_config, plus the neutral
 * mapping the bottom half will be handed.
 *
 * Positive rather than negative for the same reason pdg_gpio_pin_configure() is
 * positive: struct uart_config's fields are plain uint8_t, not enums, so the
 * compiler offers no exhaustiveness help and a value Zephyr appends later must
 * be refused until a reviewer deliberately maps it, not silently reinterpreted.
 * Each catch-all sits last and names the offending value.
 *
 * Baud is the one field forwarded unchanged: every non-zero u32 is accepted,
 * because the firmware -- not this driver -- owns what is achievable, and it
 * clamps rather than refuses. Zero is rejected as -EINVAL since it names no
 * line rate at all.
 *
 * Flow control accepts only NONE. The bridge exposes no modem-control lines, so
 * RTS/CTS, DTR/DSR and RS485 are each named explicitly rather than swept into
 * the catch-all: a caller who asked for hardware flow control deserves to be
 * told that specific thing is missing.
 *
 * On success *data_bits, *parity and *stop_bits carry exactly what the mapping
 * macros produced. This function does not re-derive them in a switch; that
 * would be a second, drift-prone copy of the mapping the BUILD_ASSERTs pin.
 */
static int pdg_uart_validate_config(const struct device *dev,
				    const struct uart_config *cfg,
				    uint8_t *data_bits, uint8_t *parity,
				    uint8_t *stop_bits)
{
	unsigned int mapped;

	if (cfg->baudrate == 0U) {
		LOG_ERR("%s: a baud rate of 0 names no line rate. Returning -EINVAL.",
			dev->name);
		return -EINVAL;
	}

	mapped = PDG_UART_MAP_DATA_BITS(cfg->data_bits);
	if (mapped == UINT8_MAX) {
		if (cfg->data_bits == UART_CFG_DATA_BITS_9) {
			LOG_ERR("%s: 9 data bits are not supported by this bridge; use 5, 6, "
				"7 or 8. Returning -ENOTSUP.", dev->name);
		} else {
			LOG_ERR("%s: unknown data-bits value %u; use 5, 6, 7 or 8. "
				"Returning -ENOTSUP.", dev->name, cfg->data_bits);
		}
		return -ENOTSUP;
	}
	*data_bits = (uint8_t)mapped;

	mapped = PDG_UART_MAP_PARITY(cfg->parity);
	if (mapped == UINT8_MAX) {
		LOG_ERR("%s: unknown parity value %u; use none, odd, even, mark or space. "
			"Returning -ENOTSUP.", dev->name, cfg->parity);
		return -ENOTSUP;
	}
	*parity = (uint8_t)mapped;

	mapped = PDG_UART_MAP_STOP_BITS(cfg->stop_bits);
	if (mapped == UINT8_MAX) {
		if (cfg->stop_bits == UART_CFG_STOP_BITS_0_5) {
			LOG_ERR("%s: 0.5 stop bits are not supported by this bridge; use 1 or "
				"2. Returning -ENOTSUP.", dev->name);
		} else if (cfg->stop_bits == UART_CFG_STOP_BITS_1_5) {
			LOG_ERR("%s: 1.5 stop bits are not supported by this bridge; use 1 or "
				"2. Returning -ENOTSUP.", dev->name);
		} else {
			LOG_ERR("%s: unknown stop-bits value %u; use 1 or 2. "
				"Returning -ENOTSUP.", dev->name, cfg->stop_bits);
		}
		return -ENOTSUP;
	}
	*stop_bits = (uint8_t)mapped;

	switch (cfg->flow_ctrl) {
	case UART_CFG_FLOW_CTRL_NONE:
		break;
	case UART_CFG_FLOW_CTRL_RTS_CTS:
		LOG_ERR("%s: RTS/CTS hardware flow control is unsupported; this bridge "
			"exposes no modem-control lines. Returning -ENOTSUP.", dev->name);
		return -ENOTSUP;
	case UART_CFG_FLOW_CTRL_DTR_DSR:
		LOG_ERR("%s: DTR/DSR hardware flow control is unsupported; this bridge "
			"exposes no modem-control lines. Returning -ENOTSUP.", dev->name);
		return -ENOTSUP;
	case UART_CFG_FLOW_CTRL_RS485:
		LOG_ERR("%s: RS485 flow control is unsupported; this bridge exposes no "
			"driver-enable line. Returning -ENOTSUP.", dev->name);
		return -ENOTSUP;
	default:
		LOG_ERR("%s: unknown flow-control value %u; only none is supported. "
			"Returning -ENOTSUP.", dev->name, cfg->flow_ctrl);
		return -ENOTSUP;
	}

	return 0;
}

/*
 * Polling receive.
 *
 * Returns exactly 0 (one byte, written to *p_char) or exactly -1. Every other
 * outcome -- prohibited context, failed init, a permanently failed link, an
 * empty line, an implausible reported length, an endpoint error, a transport
 * error -- is -1, and *p_char is left untouched. The exact errno lands in
 * last_errno instead, where a debugger or an M6 diagnostics surface can read
 * it without the polling contract having to carry it.
 */
static int pdg_uart_poll_in(const struct device *dev, unsigned char *p_char)
{
	struct pdg_uart_data *data = dev->data;
	uint16_t got = 0U;
	int ret;

	/* Before the mutex and before any FFI call, without exception. */
	if (pdg_uart_context_check() != 0) {
		return -1;
	}

	/*
	 * Zephyr's UART wrappers dispatch without checking device readiness, so
	 * this guard is primary protection for a failed-init device, not
	 * defence in depth: without it a direct caller would lock an
	 * uninitialized mutex and issue an RPC through a stale borrow.
	 */
	if (data->ctx == NULL) {
		return -1;
	}

	if (p_char == NULL) {
		return -1;
	}

	/*
	 * Pre-lock latch check. The post-lock check below is not redundant with
	 * it: a caller already queued on the mutex when the link died passed
	 * here while it was still healthy, and must not then issue an RPC.
	 */
	if (pdg_uart_link_failed(data)) {
		return -1;
	}

	k_mutex_lock(&data->lock, K_FOREVER);

	if (pdg_uart_link_failed(data)) {
		k_mutex_unlock(&data->lock);
		return -1;
	}

	/* A staged byte costs no round trip. */
	if (data->rx_pos < data->rx_len) {
		*p_char = data->rx_buf[data->rx_pos];
		data->rx_pos++;
		k_mutex_unlock(&data->lock);
		return 0;
	}

	/*
	 * Backoff from a previous non-transport refill error. k_uptime_get() is
	 * only reached here, after the context guards, so it is never called
	 * from an ISR or before the kernel is up.
	 */
	if ((data->rx_backoff_until != 0) &&
	    (k_uptime_get() < data->rx_backoff_until)) {
		k_mutex_unlock(&data->lock);
		return -1;
	}

	data->rx_pos = 0U;
	data->rx_len = 0U;

	ret = pdg_uart_bottom_read(data->ctx, data->rx_buf, PDG_UART_RX_BUFFER_SIZE,
				   PDG_UART_READ_TIMEOUT_MS, &got);
	if (ret < 0) {
		atomic_inc(&data->rx_errors);
		pdg_uart_latch_error(data, ret);

		/*
		 * The ring is already empty. A transport failure is now latched
		 * permanently and needs no backoff, because no further RPC will
		 * be issued at all; an endpoint error gets the bounded backoff
		 * instead of a latch, so that the read which clears the
		 * firmware's error state can still happen.
		 */
		if (!pdg_uart_link_failed(data)) {
			data->rx_backoff_until =
				k_uptime_get() + PDG_UART_RX_ERROR_BACKOFF_MS;
		}

		k_mutex_unlock(&data->lock);
		return -1;
	}

	if (got > PDG_UART_RX_BUFFER_SIZE) {
		/*
		 * The device reported more bytes than it was asked for. Nothing
		 * in the protocol can produce this, so the staged buffer cannot
		 * be trusted at all -- serving from it would hand out whatever
		 * the previous refill left behind.
		 */
		atomic_inc(&data->rx_errors);
		pdg_uart_latch_error(data, -EIO);
		data->rx_backoff_until = k_uptime_get() + PDG_UART_RX_ERROR_BACKOFF_MS;
		k_mutex_unlock(&data->lock);
		return -1;
	}

	/* A completed refill, empty or not, means the endpoint is answering. */
	data->rx_backoff_until = 0;

	if (got == 0U) {
		k_mutex_unlock(&data->lock);
		return -1;
	}

	data->rx_len = got;
	*p_char = data->rx_buf[0];
	data->rx_pos = 1U;

	k_mutex_unlock(&data->lock);

	return 0;
}

/*
 * Polling transmit. One byte, one RPC, no buffering, no retry, and NO LOGGING
 * ON ANY PATH.
 *
 * The no-logging rule is the load-bearing one. If this UART is the console
 * backend, a LOG_ERR here emits characters that each re-enter this function:
 * before the unlock that self-deadlocks on data->lock, and after the unlock it
 * amplifies without bound. So every failure is recorded in tx_dropped and
 * last_errno and nowhere else.
 *
 * No second ring is added: the firmware already owns an ISR-drained TX ring and
 * acknowledges once the bytes are queued, and the polling API has no flush
 * callback to drain a driver-side ring with.
 *
 * A write that fails with a transport error may already have queued its byte
 * remotely before the reply was lost, so delivery is indeterminate rather than
 * known-failed. It is counted as dropped and never retried.
 */
static void pdg_uart_poll_out(const struct device *dev, unsigned char out_char)
{
	struct pdg_uart_data *data = dev->data;
	uint8_t byte = (uint8_t)out_char;
	int ret;

	/* Before the mutex and before any FFI call, without exception. */
	if (pdg_uart_context_check() != 0) {
		atomic_inc(&data->tx_dropped);
		return;
	}

	if (data->ctx == NULL) {
		atomic_inc(&data->tx_dropped);
		return;
	}

	/* Pre-lock latch check; see pdg_uart_poll_in() for why both exist. */
	if (pdg_uart_link_failed(data)) {
		atomic_inc(&data->tx_dropped);
		return;
	}

	k_mutex_lock(&data->lock, K_FOREVER);

	if (pdg_uart_link_failed(data)) {
		k_mutex_unlock(&data->lock);
		atomic_inc(&data->tx_dropped);
		return;
	}

	ret = pdg_uart_bottom_write(data->ctx, &byte, 1U);
	if (ret < 0) {
		pdg_uart_latch_error(data, ret);

		/*
		 * A transport failure is a stream-generation boundary: bytes
		 * staged for receive were decoded on the far side of it and
		 * cannot be attributed to the new generation, so they go.
		 */
		if (pdg_uart_link_failed(data)) {
			data->rx_pos = 0U;
			data->rx_len = 0U;
		}
	}

	k_mutex_unlock(&data->lock);

	if (ret < 0) {
		atomic_inc(&data->tx_dropped);
	}
}

#ifdef CONFIG_UART_WIDE_DATA
/*
 * Wide output. The wrapper z_impl_uart_poll_out_u16() dispatches with NO NULL
 * check, so leaving this slot NULL would be a jump through a null pointer
 * rather than a graceful refusal. CONFIG_UART_WIDE_DATA is a global symbol that
 * an unrelated driver can enable, so this stub must exist whenever it is set.
 *
 * It drops and counts rather than transmitting: the bridge has no 9-bit path,
 * and the void signature cannot report that. Like poll_out(), it never logs.
 */
static void pdg_uart_poll_out_u16(const struct device *dev, uint16_t out_u16)
{
	struct pdg_uart_data *data = dev->data;

	ARG_UNUSED(out_u16);

	/* Before any other work, without exception; see pdg_uart_poll_out(). */
	if (pdg_uart_context_check() != 0) {
		atomic_inc(&data->tx_dropped);
		return;
	}

	atomic_inc(&data->tx_dropped);
}
#endif /* CONFIG_UART_WIDE_DATA */

/*
 * Line-error report.
 *
 * Reports 0 only for a healthy, initialized thread-context call, and never
 * invents UART_ERROR_* bits. The bridge's read endpoint does not surface
 * framing, parity, overrun or break status separately, so any non-zero bitmask
 * here would be fabricated. A latched transport fault is instead reported as
 * the negative errno -EIO, which is a legitimate uart_err_check() return and is
 * not a line-error bitmask.
 */
static int pdg_uart_err_check(const struct device *dev)
{
	struct pdg_uart_data *data = dev->data;
	int ret;

	ret = pdg_uart_context_check();
	if (ret != 0) {
		return ret;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo UART context is NULL; check device readiness. "
			"Returning -ENODEV.", dev->name);
		return -ENODEV;
	}

	/*
	 * A latched transport fault means every subsequent call fails fast, so
	 * reporting 0 ("healthy") here would be actively misleading. -EIO is
	 * returned for consistency with pdg_uart_configure() in the same state.
	 * It is a negative errno, not a fabricated UART_ERROR_* bitmask.
	 *
	 * Deliberately not logged: err_check() is routinely polled in a loop, so
	 * a per-call LOG_ERR would be unbounded repetition, and if this UART is
	 * the console it would also route back through pdg_uart_poll_out(). The
	 * latch was already logged once at the point it was set.
	 */
	if (pdg_uart_link_failed(data)) {
		return -EIO;
	}

	return 0;
}

#ifdef CONFIG_UART_USE_RUNTIME_CONFIGURE
/*
 * Replace the complete configuration.
 *
 * Validated before the lock so a rejected request performs no I/O at all. On
 * acknowledged success the cache is updated and the local staging ring is
 * cleared, because bytes already staged were decoded under the old framing and
 * nothing distinguishes them from bytes decoded under the new one.
 *
 * That clearing is necessary but NOT sufficient, and the limitation is real:
 * the firmware's own ring may still hold old-framing bytes this driver has
 * never seen and cannot identify. Callers must quiesce and drain both
 * directions before reconfiguring.
 *
 * The operation is not atomic across the four fields and there is no rollback.
 * A failed set leaves the cache and the ring untouched -- the far side may have
 * applied part or all of the change with only the acknowledgement lost -- unless
 * the failure was transport-class, which establishes a dead generation and
 * therefore does discard the ring.
 */
static int pdg_uart_configure(const struct device *dev, const struct uart_config *cfg)
{
	struct pdg_uart_data *data = dev->data;
	uint8_t data_bits;
	uint8_t parity;
	uint8_t stop_bits;
	int ret;

	ret = pdg_uart_context_check();
	if (ret != 0) {
		return ret;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo UART context is NULL; check device readiness. "
			"Returning -ENODEV.", dev->name);
		return -ENODEV;
	}

	if (cfg == NULL) {
		LOG_ERR("%s: NULL uart_config pointer. Returning -EINVAL.", dev->name);
		return -EINVAL;
	}

	if (pdg_uart_link_failed(data)) {
		LOG_ERR("%s: the Pico de Gallo transport has failed permanently; no further "
			"requests are issued. Returning -EIO.", dev->name);
		return -EIO;
	}

	ret = pdg_uart_validate_config(dev, cfg, &data_bits, &parity, &stop_bits);
	if (ret < 0) {
		return ret;
	}

	k_mutex_lock(&data->lock, K_FOREVER);

	if (pdg_uart_link_failed(data)) {
		k_mutex_unlock(&data->lock);
		LOG_ERR("%s: the Pico de Gallo transport has failed permanently; no further "
			"requests are issued. Returning -EIO.", dev->name);
		return -EIO;
	}

	ret = pdg_uart_bottom_set_config(data->ctx, cfg->baudrate, data_bits, parity,
					 stop_bits);
	if (ret == 0) {
		data->current = *cfg;
		data->rx_pos = 0U;
		data->rx_len = 0U;
		data->rx_backoff_until = 0;
	} else {
		pdg_uart_latch_error(data, ret);

		if (pdg_uart_link_failed(data)) {
			data->rx_pos = 0U;
			data->rx_len = 0U;
		}
	}

	k_mutex_unlock(&data->lock);

	if (ret < 0) {
		LOG_ERR("%s: failed to apply the UART configuration (baud %" PRIu32
			", %u data bits, parity %u, stop bits %u): errno=%d. The remote "
			"state is indeterminate and there is no rollback.",
			dev->name, cfg->baudrate, cfg->data_bits, cfg->parity,
			cfg->stop_bits, ret);
	}

	return ret;
}

/*
 * Report the last acknowledged configuration.
 *
 * This is the requested shadow, not a measurement. The firmware clamps the baud
 * divisor at roughly 143 baud, so a request far below that is applied as
 * something else while this still reports what was asked for.
 */
static int pdg_uart_config_get(const struct device *dev, struct uart_config *cfg)
{
	struct pdg_uart_data *data = dev->data;
	int ret;

	ret = pdg_uart_context_check();
	if (ret != 0) {
		return ret;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo UART context is NULL; check device readiness. "
			"Returning -ENODEV.", dev->name);
		return -ENODEV;
	}

	if (cfg == NULL) {
		LOG_ERR("%s: NULL uart_config pointer. Returning -EINVAL.", dev->name);
		return -EINVAL;
	}

	k_mutex_lock(&data->lock, K_FOREVER);
	*cfg = data->current;
	k_mutex_unlock(&data->lock);

	return 0;
}
#endif /* CONFIG_UART_USE_RUNTIME_CONFIGURE */

#ifdef CONFIG_UART_ASYNC_API
/*
 * Asynchronous operation stubs.
 *
 * These exist ONLY because the pinned wrappers dispatch them without a NULL
 * check (z_impl_uart_tx, z_impl_uart_tx_abort, z_impl_uart_rx_enable,
 * uart_rx_buf_rsp, z_impl_uart_rx_disable). A NULL slot there is a jump through
 * a null function pointer, which on native_sim is a segfault rather than an
 * error return. CONFIG_UART_ASYNC_API is a global symbol that an unrelated
 * driver can enable, so this driver cannot assume it is off.
 *
 * .callback_set is deliberately left NULL by contrast: its wrapper DOES
 * NULL-check and returns -ENOSYS, which is the correct answer and needs no stub.
 *
 * Each stub is side-effect-free: no mutex, no FFI, no RPC, and no logging on
 * the -ENOTSUP path. The last matters because this UART can be the console: a
 * LOG_ERR there would route its own text back through pdg_uart_poll_out() and
 * turn a capability query into real UART write RPCs. The context checks still
 * come first so that the driver has exactly one uniform public-callback policy,
 * and a failed-init device is reported as -ENODEV ahead of the capability
 * answer, because "this device never came up" is more actionable than "this
 * operation does not exist".
 */
static int pdg_uart_async_unsupported(const struct device *dev)
{
	struct pdg_uart_data *data = dev->data;
	int ret;

	ret = pdg_uart_context_check();
	if (ret != 0) {
		return ret;
	}

	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo UART context is NULL; check device readiness. "
			"Returning -ENODEV.", dev->name);
		return -ENODEV;
	}

	return -ENOTSUP;
}

static int pdg_uart_tx(const struct device *dev, const uint8_t *buf, size_t len,
		       int32_t timeout)
{
	ARG_UNUSED(buf);
	ARG_UNUSED(len);
	ARG_UNUSED(timeout);

	return pdg_uart_async_unsupported(dev);
}

static int pdg_uart_tx_abort(const struct device *dev)
{
	return pdg_uart_async_unsupported(dev);
}

static int pdg_uart_rx_enable(const struct device *dev, uint8_t *buf, size_t len,
			      int32_t timeout)
{
	ARG_UNUSED(buf);
	ARG_UNUSED(len);
	ARG_UNUSED(timeout);

	return pdg_uart_async_unsupported(dev);
}

static int pdg_uart_rx_buf_rsp(const struct device *dev, uint8_t *buf, size_t len)
{
	ARG_UNUSED(buf);
	ARG_UNUSED(len);

	return pdg_uart_async_unsupported(dev);
}

static int pdg_uart_rx_disable(const struct device *dev)
{
	return pdg_uart_async_unsupported(dev);
}

#ifdef CONFIG_UART_WIDE_DATA
/*
 * The three wide asynchronous slots are guarded by BOTH symbols, exactly as
 * their wrappers are (`#if defined(CONFIG_UART_ASYNC_API) &&
 * defined(CONFIG_UART_WIDE_DATA)`). Guarding them by only one would either
 * leave a reachable NULL slot or fail to compile, depending on which one.
 */
static int pdg_uart_tx_u16(const struct device *dev, const uint16_t *buf, size_t len,
			   int32_t timeout)
{
	ARG_UNUSED(buf);
	ARG_UNUSED(len);
	ARG_UNUSED(timeout);

	return pdg_uart_async_unsupported(dev);
}

static int pdg_uart_rx_enable_u16(const struct device *dev, uint16_t *buf, size_t len,
				  int32_t timeout)
{
	ARG_UNUSED(buf);
	ARG_UNUSED(len);
	ARG_UNUSED(timeout);

	return pdg_uart_async_unsupported(dev);
}

static int pdg_uart_rx_buf_rsp_u16(const struct device *dev, uint16_t *buf, size_t len)
{
	ARG_UNUSED(buf);
	ARG_UNUSED(len);

	return pdg_uart_async_unsupported(dev);
}
#endif /* CONFIG_UART_WIDE_DATA */
#endif /* CONFIG_UART_ASYNC_API */

/*
 * Driver API object.
 *
 * Designated initializers only. The struct is conditionally compiled -- its
 * member set changes with CONFIG_UART_ASYNC_API, CONFIG_UART_WIDE_DATA,
 * CONFIG_UART_USE_RUNTIME_CONFIGURE, CONFIG_UART_INTERRUPT_DRIVEN,
 * CONFIG_UART_LINE_CTRL and CONFIG_UART_DRV_CMD -- so positional initialization
 * would silently assign functions to the wrong slots.
 *
 * Populated:
 *   .poll_in, .poll_out, .err_check           always
 *   .configure, .config_get                   CONFIG_UART_USE_RUNTIME_CONFIGURE
 *   .tx, .tx_abort, .rx_enable,
 *   .rx_buf_rsp, .rx_disable                  CONFIG_UART_ASYNC_API
 *   .tx_u16, .rx_enable_u16, .rx_buf_rsp_u16  ASYNC_API && WIDE_DATA
 *   .poll_out_u16                             CONFIG_UART_WIDE_DATA
 *
 * Deliberately NULL, because every one of these wrappers NULL-checks and
 * returns -ENOSYS (or -ENOTSUP when the global API is off), or, for the void
 * IRQ wrappers, silently no-ops:
 *   .callback_set, .poll_in_u16, every fifo_*, every irq_*, .line_ctrl_set,
 *   .line_ctrl_get, .drv_cmd
 */
static DEVICE_API(uart, pdg_uart_api) = {
	.poll_in = pdg_uart_poll_in,
	.poll_out = pdg_uart_poll_out,
	.err_check = pdg_uart_err_check,
#ifdef CONFIG_UART_WIDE_DATA
	.poll_out_u16 = pdg_uart_poll_out_u16,
#endif
#ifdef CONFIG_UART_USE_RUNTIME_CONFIGURE
	.configure = pdg_uart_configure,
	.config_get = pdg_uart_config_get,
#endif
#ifdef CONFIG_UART_ASYNC_API
	.tx = pdg_uart_tx,
	.tx_abort = pdg_uart_tx_abort,
	.rx_enable = pdg_uart_rx_enable,
	.rx_buf_rsp = pdg_uart_rx_buf_rsp,
	.rx_disable = pdg_uart_rx_disable,
#ifdef CONFIG_UART_WIDE_DATA
	.tx_u16 = pdg_uart_tx_u16,
	.rx_enable_u16 = pdg_uart_rx_enable_u16,
	.rx_buf_rsp_u16 = pdg_uart_rx_buf_rsp_u16,
#endif
#endif
};

static int pdg_uart_init(const struct device *dev)
{
	const struct pdg_uart_config *config = dev->config;
	struct pdg_uart_data *data = dev->data;
	uint8_t data_bits;
	uint8_t parity;
	uint8_t stop_bits;
	bool has_uart = false;
	int ret;

	/*
	 * The mutex and every piece of mutable state are initialized before any
	 * early return, so that a device object which exists at all has a usable
	 * lock. Zephyr's UART wrappers dispatch straight into this API without a
	 * readiness check, so a direct call on a failed device must find an
	 * initialized mutex; the data->ctx == NULL guard at the top of each
	 * callback is then what turns that call into a safe refusal.
	 */
	k_mutex_init(&data->lock);

	data->ctx = NULL;
	data->rx_pos = 0U;
	data->rx_len = 0U;
	data->rx_backoff_until = 0;
	atomic_set(&data->tx_dropped, 0);
	atomic_set(&data->rx_errors, 0);
	atomic_set(&data->last_errno, 0);
	atomic_set(&data->link_failed, 0);
	data->current = config->initial;

	/*
	 * Mandatory MFD child sequence (pdg_mfd.h): require parent readiness
	 * first, then borrow the context. A NULL context *after* a passing
	 * readiness check is an ownership invariant failure, not an expected
	 * case, so it is logged distinctly. The context is borrowed: this driver
	 * must never close or free it.
	 */
	if (!device_is_ready(config->mfd)) {
		LOG_ERR("%s: Pico de Gallo parent %s is not ready. Returning -ENODEV.",
			dev->name, config->mfd->name);
		return -ENODEV;
	}

	data->ctx = pdg_mfd_ctx(config->mfd);
	if (data->ctx == NULL) {
		LOG_ERR("%s: Pico de Gallo parent %s is ready but returned a NULL context; "
			"this is an MFD ownership invariant failure. Returning -ENODEV.",
			dev->name, config->mfd->name);
		return -ENODEV;
	}

	/*
	 * Capability gate. Without it a hw-rev1 board, which has no UART at all,
	 * would swallow every transmitted byte silently.
	 *
	 * This is NOT a warm local read, and it must not be described as one.
	 * gallo_get_device_info() re-validates unconditionally, which issues a
	 * fresh device/info RPC bounded at 300 seconds, so this costs a second
	 * metadata round trip on top of the one the parent already paid during
	 * its strict open. That boot-time cost is an accepted M5 tradeoff; the
	 * escalation to cache the validated device info in pico-de-gallo-lib
	 * needs a crates/ change and is not done here.
	 *
	 * The two failure shapes are kept distinct on purpose. A failed query is
	 * reported with its own mapped errno, because "the board did not answer"
	 * is a different problem from "the board answered and has no UART", which
	 * is -ENODEV.
	 */
	ret = pdg_uart_bottom_has_uart(data->ctx, &has_uart);
	if (ret < 0) {
		LOG_ERR("%s: failed to read the firmware capability bits: errno=%d.",
			dev->name, ret);
		/*
		 * Defensive invalidation of this child's cached borrow -- never
		 * a reference release. The parent holds the sole registry
		 * reference; releasing it here would leave the parent and the
		 * GPIO/I2C/SPI siblings holding a freed pointer. NULL is
		 * guardable; a valid-looking unowned pointer would bypass every
		 * NULL check.
		 */
		data->ctx = NULL;
		return ret;
	}

	if (!has_uart) {
		LOG_ERR("%s: the attached Pico de Gallo does not advertise the UART "
			"capability (hardware revision 1 has no UART). Returning -ENODEV.",
			dev->name);
		data->ctx = NULL;
		return -ENODEV;
	}

	ret = pdg_uart_validate_config(dev, &config->initial, &data_bits, &parity,
				       &stop_bits);
	if (ret < 0) {
		data->ctx = NULL;
		return ret;
	}

	ret = pdg_uart_bottom_set_config(data->ctx, config->initial.baudrate, data_bits,
					 parity, stop_bits);
	if (ret < 0) {
		LOG_ERR("%s: failed to apply the initial UART configuration: errno=%d. The "
			"remote state is indeterminate -- the device may have applied part "
			"or all of it with only the acknowledgement lost -- and there is no "
			"rollback.", dev->name, ret);
		data->ctx = NULL;
		return ret;
	}

	data->current = config->initial;

	LOG_INF("%s: ready on Pico de Gallo serial-number \"%s\" at %" PRIu32 " baud "
		"(requested, not measured).",
		dev->name, config->serial_number, config->initial.baudrate);

	return 0;
}

/*
 * Only `parity` carries a binding-level default in uart-controller.yaml, so
 * current-speed, stop-bits and data-bits all need the _OR forms; using
 * DT_INST_PROP() for them would fail to build on a node that omits any one.
 * hw-flow-control is a boolean and is therefore always defined.
 */
#define PDG_UART_INIT(inst)							\
	static struct pdg_uart_data pdg_uart_data_##inst;			\
										\
	static const struct pdg_uart_config pdg_uart_config_##inst = {		\
		.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst)),			\
		.serial_number = DT_PROP(DT_INST_PARENT(inst), serial_number),	\
		.initial = {							\
			.baudrate = DT_INST_PROP_OR(inst, current_speed, 115200),\
			.parity = DT_INST_ENUM_IDX_OR(inst, parity,		\
						      UART_CFG_PARITY_NONE),	\
			.stop_bits = DT_INST_ENUM_IDX_OR(inst, stop_bits,	\
							 UART_CFG_STOP_BITS_1),	\
			.data_bits = DT_INST_ENUM_IDX_OR(inst, data_bits,	\
							 UART_CFG_DATA_BITS_8),	\
			.flow_ctrl = DT_INST_PROP(inst, hw_flow_control)	\
				     ? UART_CFG_FLOW_CTRL_RTS_CTS		\
				     : UART_CFG_FLOW_CTRL_NONE,			\
		},								\
	};									\
										\
	DEVICE_DT_INST_DEFINE(inst, pdg_uart_init, NULL,			\
			      &pdg_uart_data_##inst,				\
			      &pdg_uart_config_##inst, POST_KERNEL,		\
			      CONFIG_UART_PICO_DE_GALLO_INIT_PRIORITY,		\
			      &pdg_uart_api);

DT_INST_FOREACH_STATUS_OKAY(PDG_UART_INIT)
