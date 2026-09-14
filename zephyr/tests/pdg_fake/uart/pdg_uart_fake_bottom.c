/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Recording fake for the UART host-context bottom layer.
 *
 * Compiled into native_simulator with the host C library. It provides STRONG
 * definitions of the four symbols zephyr/drivers/serial/pdg_uart_bottom.c
 * defines as weak, so the linker prefers these and no production
 * CMakeLists.txt needs a test-only conditional.
 *
 * It deliberately does NOT link pico_de_gallo.h. Nothing here reaches the FFI,
 * which is the whole point.
 *
 * WHY EVERY OVERRIDE MUST EXIST, AND WHY NONE OF THEM MAY TOUCH ctx
 * ----------------------------------------------------------------
 * The shared parent fake (zephyr/tests/pdg_fake/common/pdg_fake_bottom.c)
 * satisfies pdg_mfd_init()'s NULL check by handing back the address of a
 * private int. That token is an OPAQUE NON-POINTER as far as the production
 * code is concerned: it is not a PicoDeGallo *, and it does not point at one.
 *
 * Consequently:
 *
 *   - A pdg_uart_bottom_* symbol we fail to override strongly resolves to the
 *     real weak production definition, which casts that token to
 *     PicoDeGallo * and passes it into the Rust FFI, which dereferences it.
 *     That is undefined behaviour which may reach real USB code, and it is NOT
 *     guaranteed to be a loud crash -- a silently wrong answer is just as
 *     likely as a segfault. So all four UART symbols below are defined here,
 *     unconditionally, with no #ifdef that could omit one.
 *
 *   - Every definition below takes `ctx` and IGNORES it with an explicit
 *     (void) cast. None of them dereferences it, compares it against a known
 *     value, or uses it to attribute a call to a particular child. It cannot
 *     be used for attribution in any case: the MFD parent hands every child the
 *     same borrowed token, so `ctx` is identical for all five UART children.
 *     Attribution comes from the distinct per-child baud rates recorded in the
 *     set-config log and the global event sequence instead.
 *
 * pdg_common_bottom_open() and pdg_common_bottom_close() are deliberately NOT
 * defined here. They belong to the shared parent fake, which both this suite
 * and the I2C suite link; defining them twice would be a duplicate-symbol link
 * error.
 */

#include <stddef.h>
#include <stdint.h>
#include <string.h>

/*
 * The declaration site of the four symbols overridden below. Included rather
 * than hand-copied so that a signature drift between production and this fake
 * is a compile error instead of undefined behaviour that links cleanly. The
 * header is FFI-free (stdbool/stdint only), so including it here does not drag
 * pico_de_gallo.h in.
 */
#include "pdg_uart_bottom.h"

#include "pdg_uart_fake_bottom.h"

/*
 * The scripted-payload cap and the driver's staging-ring size are two separate
 * literals in two headers that must never see each other's vocabulary. This is
 * the one translation unit that sees both, so it is where the drift is caught.
 */
_Static_assert(PDG_UART_FAKE_RX_MAX == PDG_UART_RX_BUFFER_SIZE,
	       "the fake's scripted-RX cap must equal the driver's staging-ring size");

/* --------------------------------------------------------------------------
 * Storage
 *
 * Three classes, kept in three clearly separated blocks because
 * pdg_uart_fake_reset() must clear exactly one of them. See the recorder model
 * comment in pdg_uart_fake_bottom.h.
 * ----------------------------------------------------------------------- */

struct fake_event {
	uint8_t kind;
	uint32_t aux;
};

struct fake_write_entry {
	uint8_t byte;
	uint16_t len;
};

struct fake_config_entry {
	uint32_t baud;
	uint8_t data_bits;
	uint8_t parity;
	uint8_t stop_bits;
};

/* Class 2: runtime recorders. Cleared by pdg_uart_fake_reset(). */
static int rt_has_uart_count;
static int rt_set_config_count;
static int rt_read_count;
static int rt_write_count;

static int rt_have_last_read;
static uint16_t rt_last_read_count;
static uint32_t rt_last_read_timeout;

static struct fake_write_entry rt_write_log[PDG_UART_FAKE_WRITE_LOG_MAX];
static int rt_write_log_len;

static struct fake_config_entry rt_config_log[PDG_UART_FAKE_CONFIG_LOG_MAX];
static int rt_config_log_len;

static struct fake_event rt_events[PDG_UART_FAKE_EVENT_MAX];
static int rt_event_count;

static int rt_overflowed;

/* Class 3: scripts. Also cleared by pdg_uart_fake_reset(). */
static int sc_read_result;
static int sc_write_result;
static int sc_set_config_result;
static uint8_t sc_rx_buf[PDG_UART_FAKE_RX_MAX];
static uint16_t sc_rx_len;
static uint16_t sc_rx_reported_len;

/*
 * Class 1: the frozen boot snapshot. NEVER cleared, by reset or anything else.
 * A second freeze attempt is refused so that a stray call from a test body
 * cannot overwrite initialization evidence with runtime noise.
 */
static int bs_frozen;
static int bs_has_uart_count;
static int bs_set_config_count;
static int bs_read_count;
static int bs_write_count;
static int bs_overflowed;
static struct fake_config_entry bs_config_log[PDG_UART_FAKE_CONFIG_LOG_MAX];
static int bs_config_log_len;
static struct fake_event bs_events[PDG_UART_FAKE_EVENT_MAX];
static int bs_event_count;

/* --------------------------------------------------------------------------
 * Recording helpers
 * ----------------------------------------------------------------------- */

/*
 * Append to the global event sequence, or refuse and raise the overflow flag.
 *
 * Refusing rather than wrapping matters: a ring buffer would keep the MOST
 * RECENT events and silently discard the oldest, which is precisely the
 * opposite of what an ordering assertion needs. A test that overran the log
 * must fail, not assert against a suffix.
 */
static void record_event_(uint8_t kind, uint32_t aux)
{
	if (rt_event_count >= PDG_UART_FAKE_EVENT_MAX) {
		rt_overflowed = 1;
		return;
	}

	rt_events[rt_event_count].kind = kind;
	rt_events[rt_event_count].aux = aux;
	rt_event_count++;
}

/* --------------------------------------------------------------------------
 * Strong overrides of the four weak definitions in
 * zephyr/drivers/serial/pdg_uart_bottom.c.
 *
 * Every one of them ignores ctx. See the file header for why that is a safety
 * requirement and not merely tidiness.
 * ----------------------------------------------------------------------- */

int pdg_uart_bottom_has_uart(void *ctx, bool *out_has_uart)
{
	/* Ignored, never dereferenced: it is an opaque non-pointer token. */
	(void)ctx;

	rt_has_uart_count++;
	record_event_(PDG_UART_FAKE_EV_HAS_UART, 0U);

	/*
	 * The production contract says *out_has_uart is written only when the
	 * query succeeds. This fake never fails the query, so it always writes.
	 * A NULL out pointer would be a driver bug; tolerate it rather than
	 * crash, so the resulting test failure is the driver's misuse rather
	 * than a segfault in the fake.
	 */
	if (out_has_uart != NULL) {
		*out_has_uart = (PDG_FAKE_UART_CAPABILITY != 0);
	}

	return 0;
}

int pdg_uart_bottom_set_config(void *ctx, uint32_t baud_rate, uint8_t data_bits,
			       uint8_t parity, uint8_t stop_bits)
{
	/* Ignored, never dereferenced. */
	(void)ctx;

	rt_set_config_count++;
	record_event_(PDG_UART_FAKE_EV_SET_CONFIG, baud_rate);

	/*
	 * Recorded even when the call is scripted to fail. A failed-config test
	 * needs to prove the driver ISSUED the request before it can prove the
	 * cache did not follow it, and a log that only held successes could not
	 * distinguish "issued and refused" from "never issued".
	 */
	if (rt_config_log_len >= PDG_UART_FAKE_CONFIG_LOG_MAX) {
		rt_overflowed = 1;
	} else {
		rt_config_log[rt_config_log_len].baud = baud_rate;
		rt_config_log[rt_config_log_len].data_bits = data_bits;
		rt_config_log[rt_config_log_len].parity = parity;
		rt_config_log[rt_config_log_len].stop_bits = stop_bits;
		rt_config_log_len++;
	}

	return sc_set_config_result;
}

int pdg_uart_bottom_read(void *ctx, uint8_t *buf, uint16_t count, uint32_t timeout_ms,
			 uint16_t *out_len)
{
	uint16_t copy;

	/* Ignored, never dereferenced. */
	(void)ctx;

	rt_read_count++;
	record_event_(PDG_UART_FAKE_EV_READ, (uint32_t)count);

	/*
	 * The requested count and timeout are captured BEFORE the failure
	 * branch, so a test can prove what the driver asked for even on a call
	 * that was scripted to fail.
	 */
	rt_have_last_read = 1;
	rt_last_read_count = count;
	rt_last_read_timeout = timeout_ms;

	if (sc_read_result != 0) {
		/*
		 * The production contract says *out_len is not meaningful after
		 * a negative return, and the caller must not read it. Leave it
		 * strictly untouched so that a driver which read it anyway is
		 * looking at its own stale value rather than something this
		 * fake helpfully zeroed -- that would mask the bug.
		 */
		return sc_read_result;
	}

	/*
	 * Copy at most what the caller asked for, independently of what we are
	 * about to REPORT. This is what makes the implausible-length test safe:
	 * reporting 1015 while copying at most `count` bytes exercises the
	 * driver's defence without smashing the driver's stack, which would
	 * turn a behavioural test into undefined behaviour.
	 */
	copy = sc_rx_len;
	if (copy > count) {
		copy = count;
	}

	if (buf != NULL && copy > 0U) {
		memcpy(buf, sc_rx_buf, copy);
	}

	if (out_len != NULL) {
		*out_len = sc_rx_reported_len;
	}

	return 0;
}

int pdg_uart_bottom_write(void *ctx, const uint8_t *buf, uint16_t len)
{
	uint32_t aux = PDG_UART_FAKE_NO_BYTE;

	/* Ignored, never dereferenced. */
	(void)ctx;

	rt_write_count++;

	if (buf != NULL && len > 0U) {
		aux = (uint32_t)buf[0];
	}

	record_event_(PDG_UART_FAKE_EV_WRITE, aux);

	if (rt_write_log_len >= PDG_UART_FAKE_WRITE_LOG_MAX) {
		rt_overflowed = 1;
	} else {
		rt_write_log[rt_write_log_len].byte =
			(buf != NULL && len > 0U) ? buf[0] : 0U;
		rt_write_log[rt_write_log_len].len = len;
		rt_write_log_len++;
	}

	return sc_write_result;
}

/* --------------------------------------------------------------------------
 * Lifecycle accessors
 * ----------------------------------------------------------------------- */

void pdg_uart_fake_reset(void)
{
	/* Class 2: runtime recorders. */
	rt_has_uart_count = 0;
	rt_set_config_count = 0;
	rt_read_count = 0;
	rt_write_count = 0;

	rt_have_last_read = 0;
	rt_last_read_count = 0U;
	rt_last_read_timeout = 0U;

	rt_write_log_len = 0;
	memset(rt_write_log, 0, sizeof(rt_write_log));

	rt_config_log_len = 0;
	memset(rt_config_log, 0, sizeof(rt_config_log));

	rt_event_count = 0;
	memset(rt_events, 0, sizeof(rt_events));

	rt_overflowed = 0;

	/*
	 * Class 3: scripts, back to the quietest possible behaviour -- succeed,
	 * hand back zero bytes. A leftover error injection or a leftover
	 * payload is the single most likely way for one test to make another
	 * pass or fail for reasons that have nothing to do with it.
	 */
	sc_read_result = 0;
	sc_write_result = 0;
	sc_set_config_result = 0;
	sc_rx_len = 0U;
	sc_rx_reported_len = 0U;
	memset(sc_rx_buf, 0, sizeof(sc_rx_buf));

	/* Class 1 is deliberately untouched. See the header's INVARIANT. */
}

int pdg_uart_fake_freeze_boot_snapshot(void)
{
	if (bs_frozen) {
		return 0;
	}

	bs_has_uart_count = rt_has_uart_count;
	bs_set_config_count = rt_set_config_count;
	bs_read_count = rt_read_count;
	bs_write_count = rt_write_count;
	bs_overflowed = rt_overflowed;

	bs_config_log_len = rt_config_log_len;
	memcpy(bs_config_log, rt_config_log, sizeof(bs_config_log));

	bs_event_count = rt_event_count;
	memcpy(bs_events, rt_events, sizeof(bs_events));

	bs_frozen = 1;

	return 1;
}

int pdg_uart_fake_overflowed(void)
{
	return rt_overflowed;
}

int pdg_uart_fake_capability(void)
{
	return PDG_FAKE_UART_CAPABILITY;
}

/* --------------------------------------------------------------------------
 * Runtime call counts
 * ----------------------------------------------------------------------- */

int pdg_uart_fake_has_uart_count(void)
{
	return rt_has_uart_count;
}

int pdg_uart_fake_set_config_count(void)
{
	return rt_set_config_count;
}

int pdg_uart_fake_read_count(void)
{
	return rt_read_count;
}

int pdg_uart_fake_write_count(void)
{
	return rt_write_count;
}

/* --------------------------------------------------------------------------
 * Runtime detail records
 * ----------------------------------------------------------------------- */

int pdg_uart_fake_last_read(uint16_t *out_count, uint32_t *out_timeout_ms)
{
	if (!rt_have_last_read) {
		return -1;
	}

	if (out_count != NULL) {
		*out_count = rt_last_read_count;
	}

	if (out_timeout_ms != NULL) {
		*out_timeout_ms = rt_last_read_timeout;
	}

	return 0;
}

int pdg_uart_fake_write_log_len(void)
{
	return rt_write_log_len;
}

int pdg_uart_fake_write_log_entry(int index, uint8_t *out_byte, uint16_t *out_len)
{
	if (index < 0 || index >= rt_write_log_len) {
		return -1;
	}

	if (out_byte != NULL) {
		*out_byte = rt_write_log[index].byte;
	}

	if (out_len != NULL) {
		*out_len = rt_write_log[index].len;
	}

	return 0;
}

int pdg_uart_fake_config_log_len(void)
{
	return rt_config_log_len;
}

int pdg_uart_fake_config_log_entry(int index, uint32_t *out_baud, uint8_t *out_data_bits,
				   uint8_t *out_parity, uint8_t *out_stop_bits)
{
	if (index < 0 || index >= rt_config_log_len) {
		return -1;
	}

	if (out_baud != NULL) {
		*out_baud = rt_config_log[index].baud;
	}

	if (out_data_bits != NULL) {
		*out_data_bits = rt_config_log[index].data_bits;
	}

	if (out_parity != NULL) {
		*out_parity = rt_config_log[index].parity;
	}

	if (out_stop_bits != NULL) {
		*out_stop_bits = rt_config_log[index].stop_bits;
	}

	return 0;
}

int pdg_uart_fake_event_count(void)
{
	return rt_event_count;
}

int pdg_uart_fake_event_at(int index, uint8_t *out_kind, uint32_t *out_aux)
{
	if (index < 0 || index >= rt_event_count) {
		return -1;
	}

	if (out_kind != NULL) {
		*out_kind = rt_events[index].kind;
	}

	if (out_aux != NULL) {
		*out_aux = rt_events[index].aux;
	}

	return 0;
}

/* --------------------------------------------------------------------------
 * Frozen boot snapshot accessors
 * ----------------------------------------------------------------------- */

int pdg_uart_fake_boot_frozen(void)
{
	return bs_frozen;
}

int pdg_uart_fake_boot_has_uart_count(void)
{
	return bs_has_uart_count;
}

int pdg_uart_fake_boot_set_config_count(void)
{
	return bs_set_config_count;
}

int pdg_uart_fake_boot_read_count(void)
{
	return bs_read_count;
}

int pdg_uart_fake_boot_write_count(void)
{
	return bs_write_count;
}

int pdg_uart_fake_boot_overflowed(void)
{
	return bs_overflowed;
}

int pdg_uart_fake_boot_config_log_len(void)
{
	return bs_config_log_len;
}

int pdg_uart_fake_boot_config_log_entry(int index, uint32_t *out_baud,
					uint8_t *out_data_bits, uint8_t *out_parity,
					uint8_t *out_stop_bits)
{
	if (index < 0 || index >= bs_config_log_len) {
		return -1;
	}

	if (out_baud != NULL) {
		*out_baud = bs_config_log[index].baud;
	}

	if (out_data_bits != NULL) {
		*out_data_bits = bs_config_log[index].data_bits;
	}

	if (out_parity != NULL) {
		*out_parity = bs_config_log[index].parity;
	}

	if (out_stop_bits != NULL) {
		*out_stop_bits = bs_config_log[index].stop_bits;
	}

	return 0;
}

int pdg_uart_fake_boot_event_count(void)
{
	return bs_event_count;
}

int pdg_uart_fake_boot_event_at(int index, uint8_t *out_kind, uint32_t *out_aux)
{
	if (index < 0 || index >= bs_event_count) {
		return -1;
	}

	if (out_kind != NULL) {
		*out_kind = bs_events[index].kind;
	}

	if (out_aux != NULL) {
		*out_aux = bs_events[index].aux;
	}

	return 0;
}

/* --------------------------------------------------------------------------
 * Scripting
 * ----------------------------------------------------------------------- */

void pdg_uart_fake_set_read_result(int result)
{
	sc_read_result = result;
}

void pdg_uart_fake_set_write_result(int result)
{
	sc_write_result = result;
}

void pdg_uart_fake_set_set_config_result(int result)
{
	sc_set_config_result = result;
}

int pdg_uart_fake_script_rx(const uint8_t *bytes, uint16_t len)
{
	if (len > PDG_UART_FAKE_RX_MAX) {
		/*
		 * Refused and recorded, never truncated. A test that asked for
		 * a payload we cannot store must fail rather than silently
		 * assert against a prefix.
		 */
		rt_overflowed = 1;
		return -1;
	}

	if (len > 0U && bytes == NULL) {
		rt_overflowed = 1;
		return -1;
	}

	memset(sc_rx_buf, 0, sizeof(sc_rx_buf));
	if (len > 0U) {
		memcpy(sc_rx_buf, bytes, len);
	}

	sc_rx_len = len;
	sc_rx_reported_len = len;

	return 0;
}

void pdg_uart_fake_set_reported_len(uint16_t len)
{
	/*
	 * Deliberately NOT clamped to PDG_UART_FAKE_RX_MAX. The whole purpose of
	 * this setter is to report a length the protocol cannot legitimately
	 * produce, so clamping it would make the driver's defence untestable.
	 * The number of bytes actually copied is bounded independently, by the
	 * caller's own `count`, inside pdg_uart_bottom_read().
	 */
	sc_rx_reported_len = len;
}
