/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Embedded-facing view of the UART recording fake.
 *
 * WHY THIS FILE EXISTS
 * --------------------
 * native_sim splits into an embedded context (Zephyr, src/main.c) and a host
 * context (the native simulator runner). The fake lives in the host context
 * because that is where the production bottom halves live, and the ztest
 * assertions live in the embedded context. The two halves share NO globals.
 * Everything a test needs to observe therefore crosses this boundary through an
 * accessor declared here, and every accessor copies scalars or bounded arrays
 * rather than handing back a pointer into host storage. Same pattern, and same
 * reason, as zephyr/tests/pdg_fake/common/pdg_fake_bottom.h and
 * zephyr/tests/pdg_mfd_m5/common/m5_bottom.h.
 *
 * This header is FFI-free -- stdbool/stddef/stdint only -- so including it does
 * not drag the host-only pico_de_gallo.h onto the embedded include path.
 *
 * RECORDER MODEL
 * --------------
 * There are three storage classes, and confusing them is the most likely way to
 * write a test that passes for the wrong reason:
 *
 *   1. The FROZEN BOOT SNAPSHOT. Captured once by
 *      pdg_uart_fake_freeze_boot_snapshot(), which the suite `setup` hook calls
 *      before any test case runs. It holds the evidence produced during
 *      POST_KERNEL device initialization, which no ztest hook can re-establish.
 *      Nothing ever clears it.
 *
 *   2. The RUNTIME RECORDERS. Call counts, the write log, the set-config log,
 *      the last-read arguments, and the global event sequence.
 *      pdg_uart_fake_reset() clears exactly these.
 *
 *   3. The SCRIPTS. Programmable return values, the scripted RX payload, and
 *      the independently programmable reported length. pdg_uart_fake_reset()
 *      clears these too, back to "succeed, zero bytes".
 *
 * OVERFLOW IS A FAILURE, NOT A TRUNCATION
 * ---------------------------------------
 * Every bounded log refuses to grow past its cap and raises the sticky flag
 * returned by pdg_uart_fake_overflowed(). A test that drove more calls than the
 * recorder can hold must FAIL rather than silently assert against a prefix, so
 * every test checks that flag.
 */

#ifndef PDG_UART_FAKE_BOTTOM_H
#define PDG_UART_FAKE_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Compile-time capability policy, supplied identically to the embedded app and
 * to the native_simulator half by the suite's CMakeLists.txt.
 *
 * It MUST be compile-time. pdg_uart_bottom_has_uart() is called during
 * POST_KERNEL device initialization, which happens before any ztest setup hook
 * could run a setter, so a runtime switch would always arrive too late and the
 * capability=0 scenario would silently degrade into a second copy of the
 * capability=1 scenario.
 *
 * Defaulting to 1 rather than 0 is deliberate: a build in which the CMake
 * plumbing silently failed to pass the flag then behaves as the healthy suite,
 * whose tests are the ones that would notice. Defaulting to 0 would turn the
 * same plumbing bug into "every child refused", which is exactly what the
 * capability=0 scenario expects to see and would therefore pass.
 */
#ifndef PDG_FAKE_UART_CAPABILITY
#define PDG_FAKE_UART_CAPABILITY 1
#endif

#if (PDG_FAKE_UART_CAPABILITY != 0) && (PDG_FAKE_UART_CAPABILITY != 1)
#error "PDG_FAKE_UART_CAPABILITY must be exactly 0 or 1"
#endif

/*
 * Bounded recorder capacities. Exposed so the embedded side can size its own
 * loop caps from the same numbers rather than from a second hand-written copy.
 */
#define PDG_UART_FAKE_EVENT_MAX      64
#define PDG_UART_FAKE_WRITE_LOG_MAX  32
#define PDG_UART_FAKE_CONFIG_LOG_MAX 32

/* Largest scripted RX payload. Matches PDG_UART_RX_BUFFER_SIZE, which is the
 * largest refill the driver will ever request. Spelled as a literal here for
 * the same reason PDG_UART_RX_BUFFER_SIZE is: this header must stay free of
 * every other vocabulary. pdg_uart_fake_bottom.c includes both and carries a
 * _Static_assert tying them together, so drift is a build failure.
 */
#define PDG_UART_FAKE_RX_MAX 1014

/*
 * Event kinds in the global monotonic sequence.
 *
 * The sequence exists because call COUNTS cannot express ordering, and test 1's
 * central claim -- that every child probes the capability bit BEFORE it applies
 * its initial configuration -- is an ordering claim. Counts alone would be
 * satisfied by a driver that configured first and probed afterwards.
 *
 * LIMITATION, stated here because it bounds what the suite can prove: this
 * sequence covers the four UART bottom symbols only. The MFD parent's open and
 * close live in the shared fake (zephyr/tests/pdg_fake/common/pdg_fake_bottom.c)
 * and contribute counters, not events, so "open precedes the first capability
 * probe" is NOT provable here. It is ordered by Zephyr's init-level machinery
 * (the parent initializes at a lower priority than its children) and is
 * static-review evidence only.
 */
#define PDG_UART_FAKE_EV_HAS_UART   1U
#define PDG_UART_FAKE_EV_SET_CONFIG 2U
#define PDG_UART_FAKE_EV_READ       3U
#define PDG_UART_FAKE_EV_WRITE      4U

/*
 * Sentinel stored in an event's auxiliary word for a write whose payload was
 * empty or whose buffer was NULL. It cannot collide with a real byte, which is
 * always 0..255.
 */
#define PDG_UART_FAKE_NO_BYTE 0x1000U

/* ---------------------------------------------------------------------------
 * Lifecycle
 * ------------------------------------------------------------------------ */

/* Clear the runtime recorders and the scripts.
 *
 * INVARIANT: this never touches the frozen boot snapshot, and it never touches
 * the shared fake's latched parent-open counter. Both record events that
 * happened during POST_KERNEL device initialization, before any ztest hook
 * existed to observe them; clearing either would destroy evidence that cannot
 * be re-established and would make test 1 depend on execution order.
 *
 * After a reset the fake is scripted to succeed with zero received bytes, which
 * is the quietest possible behaviour: a driver that issues an unexpected refill
 * then gets an empty answer rather than stale bytes from a previous test.
 */
void pdg_uart_fake_reset(void);

/* Copy the current runtime recorders into the immutable boot snapshot.
 *
 * Call exactly once, from the ztest suite `setup` hook, which runs after every
 * POST_KERNEL device init and before the first test case. Calling it a second
 * time is refused (the first capture wins) so that a stray call from a test
 * body cannot overwrite the initialization evidence with runtime noise.
 *
 * Returns 1 if this call performed the capture, 0 if a capture already existed.
 */
int pdg_uart_fake_freeze_boot_snapshot(void);

/* Non-zero once a bounded log refused an entry because it was full, or a
 * scripted payload exceeded PDG_UART_FAKE_RX_MAX. Sticky across
 * pdg_uart_fake_reset() is NOT wanted here -- it is cleared by reset, because
 * it describes the current test's recording -- but it is never cleared by a
 * recording call. Every test asserts it is clear before trusting a log.
 */
int pdg_uart_fake_overflowed(void);

/* The compile-time capability policy this binary was built with, 0 or 1.
 * Exposed so a test can report the scenario it is running under in a failure
 * message rather than the reader having to guess from the test name.
 */
int pdg_uart_fake_capability(void);

/* ---------------------------------------------------------------------------
 * Runtime call counts
 * ------------------------------------------------------------------------ */

int pdg_uart_fake_has_uart_count(void);
int pdg_uart_fake_set_config_count(void);
int pdg_uart_fake_read_count(void);
int pdg_uart_fake_write_count(void);

/* ---------------------------------------------------------------------------
 * Runtime detail records
 * ------------------------------------------------------------------------ */

/* Arguments of the most recent pdg_uart_bottom_read(), exactly as the driver
 * passed them. Returns 0 on success, -1 if no read has been recorded since the
 * last reset. Either out pointer may be NULL.
 *
 * The point of recording the REQUESTED count and timeout, rather than assuming
 * them, is that PDG_UART_RX_BUFFER_SIZE and PDG_UART_READ_TIMEOUT_MS are
 * driver-private policy. A change to either is a change to how much firmware
 * time every idle poll costs, and it should break a test.
 */
int pdg_uart_fake_last_read(uint16_t *out_count, uint32_t *out_timeout_ms);

/* Number of entries in the ordered write log. */
int pdg_uart_fake_write_log_len(void);

/* Copy write-log entry `index` (0-based, oldest first). Returns 0 on success,
 * -1 if the index is out of range. Either out pointer may be NULL.
 *
 * *out_byte is the first byte of the payload, or an unspecified value when
 * *out_len is 0. *out_len is the length the driver asked to write. The driver's
 * documented contract is one call per byte with len == 1, so a test proves that
 * by reading both fields rather than only the count.
 */
int pdg_uart_fake_write_log_entry(int index, uint8_t *out_byte, uint16_t *out_len);

/* Number of entries in the set-config log. */
int pdg_uart_fake_config_log_len(void);

/* Copy set-config-log entry `index` (0-based, oldest first). Returns 0 on
 * success, -1 if the index is out of range. Any out pointer may be NULL.
 *
 * The scalars are recorded EXACTLY as received, in the neutral PDG_UART_*
 * vocabulary. That is what pins the Zephyr->neutral transcription at run time;
 * the driver's BUILD_ASSERTs only pin the constant expressions against each
 * other and cannot show that pdg_uart_configure() actually routed a field
 * through the right macro.
 *
 * An entry is recorded even when the call was scripted to fail, because the
 * fact that the driver ISSUED the request is itself the observation a
 * failed-configuration test needs.
 */
int pdg_uart_fake_config_log_entry(int index, uint32_t *out_baud, uint8_t *out_data_bits,
				   uint8_t *out_parity, uint8_t *out_stop_bits);

/* Number of entries in the global monotonic event sequence. */
int pdg_uart_fake_event_count(void);

/* Copy event `index` (0-based, oldest first). Returns 0 on success, -1 if the
 * index is out of range. Either out pointer may be NULL.
 *
 * *out_kind is one of PDG_UART_FAKE_EV_*. *out_aux carries the one scalar that
 * makes the event attributable:
 *
 *   HAS_UART    0 -- nothing distinguishes one child's probe from another's.
 *   SET_CONFIG  the requested baud rate. The suite's devicetree gives every
 *               child a DISTINCT baud, so this is the ONLY discriminator by
 *               which an initialization event can be attributed to a specific
 *               child: the shared MFD parent hands every child the same
 *               borrowed context token, so ctx cannot tell them apart.
 *   READ        the requested count.
 *   WRITE       the first payload byte, or PDG_UART_FAKE_NO_BYTE.
 */
int pdg_uart_fake_event_at(int index, uint8_t *out_kind, uint32_t *out_aux);

/* ---------------------------------------------------------------------------
 * Frozen boot snapshot
 * ------------------------------------------------------------------------ */

/* Non-zero once pdg_uart_fake_freeze_boot_snapshot() has captured. */
int pdg_uart_fake_boot_frozen(void);

int pdg_uart_fake_boot_has_uart_count(void);
int pdg_uart_fake_boot_set_config_count(void);
int pdg_uart_fake_boot_read_count(void);
int pdg_uart_fake_boot_write_count(void);

/* Non-zero if a recorder overflowed at or before the freeze. A boot snapshot
 * taken from an overflowed recorder proves nothing and must fail test 1.
 */
int pdg_uart_fake_boot_overflowed(void);

int pdg_uart_fake_boot_config_log_len(void);
int pdg_uart_fake_boot_config_log_entry(int index, uint32_t *out_baud,
					uint8_t *out_data_bits, uint8_t *out_parity,
					uint8_t *out_stop_bits);

int pdg_uart_fake_boot_event_count(void);
int pdg_uart_fake_boot_event_at(int index, uint8_t *out_kind, uint32_t *out_aux);

/* ---------------------------------------------------------------------------
 * Scripting
 * ------------------------------------------------------------------------ */

/* Program the value the NEXT and every subsequent pdg_uart_bottom_read() will
 * return until changed. 0 means success. A negative errno is returned without
 * touching the caller's buffer or *out_len.
 *
 * The three programmable returns are independent on purpose: test 14 must make
 * set-config fail while read continues to succeed, and test 11 must make write
 * fail while read continues to succeed.
 */
void pdg_uart_fake_set_read_result(int result);
void pdg_uart_fake_set_write_result(int result);
void pdg_uart_fake_set_set_config_result(int result);

/* Script the bytes a successful refill hands back, and set the reported length
 * to `len`. Returns 0 on success, -1 if len exceeds PDG_UART_FAKE_RX_MAX, in
 * which case the overflow flag is raised and the script is left unchanged.
 *
 * The script PERSISTS across reads; it is not consumed. Test 6 depends on that
 * being explicit: it re-scripts an empty payload before the ninth poll, and if
 * the script were auto-consumed that call would be a no-op and the test would
 * pass for the wrong reason.
 */
int pdg_uart_fake_script_rx(const uint8_t *bytes, uint16_t len);

/* Override the length a successful refill REPORTS, without changing the bytes
 * it copies.
 *
 * This is what makes test 7 possible without undefined behaviour. The driver
 * defends against a device that claims more bytes than it was asked for, and
 * the only honest way to exercise that defence is to report an implausible
 * length while still writing no more than `count` bytes into the caller's
 * buffer. A fake that actually wrote 1015 bytes into a 1014-byte buffer would
 * be testing the driver with a smashed stack.
 */
void pdg_uart_fake_set_reported_len(uint16_t len);

#ifdef __cplusplus
}
#endif

#endif /* PDG_UART_FAKE_BOTTOM_H */
