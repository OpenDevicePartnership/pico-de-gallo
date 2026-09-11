/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context Zephyr UART driver and the
 * host-context shim that calls the Pico de Gallo C FFI.
 *
 * Only basic C types may appear here so that the embedded side never needs to
 * include the host-only pico_de_gallo.h header. All functions return 0 on
 * success or a negative POSIX errno value on failure.
 *
 * There is deliberately no open/close pair here, and no flush or get-config
 * wrapper. Like the GPIO controller, this UART is born into MFD ownership: the
 * odp,pico-de-gallo parent holds the sole registry reference and the child only
 * ever borrows it, so a close wrapper would make it possible to drop the
 * parent's reference from a child and leave the parent and its GPIO/I2C/SPI
 * siblings holding a freed pointer. The polling UART API has no flush callback,
 * and uart_config_get() is served from the top half's cache of the last
 * acknowledged configuration, so neither gallo_uart_flush() nor
 * gallo_uart_get_config() is reachable from this driver.
 *
 * Every function declared here is defined `__attribute__((weak))` in
 * pdg_uart_bottom.c so that the recording fake planned for Milestone 6 can
 * supply a strong override for each symbol without touching this driver. This
 * header only declares them; it defines nothing.
 *
 * None of these functions logs. The top half latches the exact errno and the
 * failure class into its private atomics instead, because the UART being
 * driven may itself be the console: logging from the output path would recurse
 * per character, self-deadlock while the driver mutex is held, or amplify
 * without bound once it is released.
 */

#ifndef PDG_UART_BOTTOM_H
#define PDG_UART_BOTTOM_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Largest single RX refill this driver will request, in bytes, and therefore
 * the size of the top half's staging ring.
 *
 * A UART read returns its payload inside one postcard-rpc response frame, so
 * the binding limit is what a single response can carry. That is derived
 * rather than guessed -- `pico_de_gallo_internal::MAX_RESPONSE_PAYLOAD`,
 * exported to C as `GALLO_MAX_RESPONSE_PAYLOAD`:
 *
 *     1024   postcard-rpc host inbound transfer buffer
 *      - 7   response header (1 discriminant + 2 key + 4 sequence)
 *      - 1   postcard Result discriminant
 *      - 2   postcard varint length prefix
 *     =1014
 *
 * Declared here as a literal, not as an expression over an FFI constant,
 * because the embedded-context half of the driver includes this header and
 * must never see pico_de_gallo.h. pdg_uart_bottom.c is the one translation
 * unit that sees both vocabularies, and it carries a `_Static_assert` tying
 * this literal to `GALLO_MAX_RESPONSE_PAYLOAD`, so drift is a build failure
 * rather than a silent divergence.
 */
#define PDG_UART_RX_BUFFER_SIZE 1014U

/*
 * Neutral configuration vocabulary shared by both halves of the driver.
 *
 * These eleven values are the only configuration language this header speaks.
 * The top half maps Zephyr's UART_CFG_* ordinals onto them with the
 * constant-expression PDG_UART_MAP_* macros, whose results are pinned by
 * BUILD_ASSERTs; the bottom half maps them onto the FFI's GalloUartDataBits,
 * GalloUartParity and GalloUartStopBits with eleven `_Static_assert`s. Neither
 * half can see the other's vocabulary, so this is the whole contract between
 * them.
 *
 * The assertions are exhaustive for today's 4 + 5 + 2 FFI variants. They
 * cannot detect a future appended FFI variant, because no generated symbol
 * exports a variant count; closing that residual needs a crates/ change and is
 * out of scope here.
 */

/* Word length. */
#define PDG_UART_DATA_BITS_5 0U
#define PDG_UART_DATA_BITS_6 1U
#define PDG_UART_DATA_BITS_7 2U
#define PDG_UART_DATA_BITS_8 3U

/* Parity mode. */
#define PDG_UART_PARITY_NONE  0U
#define PDG_UART_PARITY_ODD   1U
#define PDG_UART_PARITY_EVEN  2U
#define PDG_UART_PARITY_MARK  3U
#define PDG_UART_PARITY_SPACE 4U

/*
 * Stop-bit count. Note that these are NOT Zephyr's ordinals:
 * UART_CFG_STOP_BITS_1 is 1 but maps to PDG_UART_STOP_BITS_1 == 0. Raw ordinal
 * equality between the two vocabularies is therefore wrong, which is why the
 * top half routes every field through a mapping macro rather than forwarding
 * the Zephyr value.
 */
#define PDG_UART_STOP_BITS_1 0U
#define PDG_UART_STOP_BITS_2 1U

/* Read up to `count` bytes into `buf`, storing the number actually received in
 * *out_len.
 *
 * `timeout_ms` is the firmware-side read allowance. The driver passes zero
 * (PDG_UART_READ_TIMEOUT_MS in pdg_uart.c), which selects the firmware's
 * poll_once() branch: it polls the RX ring exactly once and answers
 * immediately, ~359 us per call, instead of the progress::bounded() branch a
 * non-zero value selects, which waits out the full millisecond at ~1489 us.
 * uart_poll_in() is documented non-blocking and the console drains it in a
 * tight loop, so that 4.15x would be paid on every idle iteration.
 *
 * Zero is NOT the host library's 30-minute bounded_for(0) path. That path
 * exists for gpio/wait-*, where zero means "no caller deadline"; on uart/read
 * zero means the opposite, and pico-de-gallo-lib special-cases it in
 * uart_read_bound() so a zero-timeout read is bounded by the ordinary 5 s call
 * timeout, pinned by uart_read_bound_zero_timeout_is_the_call_bound.
 *
 * Zero still preserves the error-recovery path: both firmware branches call
 * AsyncRead::read(), whose first poll reaches Embassy's try_read(), which
 * consumes the latched RX error and re-enables the RX interrupts.
 *
 * Returns 0 on success, with *out_len set (possibly to zero, meaning no byte
 * was available). Returns a negative POSIX errno on failure, in which case
 * *out_len is not meaningful. Nothing is logged here; the caller latches the
 * exact errno, counts the failure, and -- for the transport classes -ECOMM and
 * -ETIMEDOUT -- marks the link permanently failed.
 */
int pdg_uart_bottom_read(void *ctx, uint8_t *buf, uint16_t count,
		uint32_t timeout_ms, uint16_t *out_len);

/* Write `len` bytes from `buf`.
 *
 * The firmware already owns an ISR-drained TX ring and acknowledges as soon as
 * the bytes are queued, so this driver adds no second ring: each poll_out()
 * byte becomes exactly one call with len == 1.
 *
 * Returns 0 on success or a negative POSIX errno on failure. Nothing is logged
 * here, and there is no retry: a call that returns -ECOMM or -ETIMEDOUT may
 * already have queued its bytes remotely before the response was lost, so
 * delivery is indeterminate rather than known-failed. The caller counts the
 * dropped byte and latches the errno.
 */
int pdg_uart_bottom_write(void *ctx, const uint8_t *buf, uint16_t len);

/* Replace the complete UART configuration.
 *
 * `baud_rate` is forwarded unchanged; every non-zero value is accepted, but
 * firmware clamps the divisor, so a request far below roughly 143 baud is
 * applied as something else and the top half's cache reports what was
 * requested rather than what was achieved. `data_bits`, `parity` and
 * `stop_bits` must each be one of the neutral PDG_UART_* values above.
 *
 * Returns 0 on success or a negative POSIX errno on failure. The operation is
 * not atomic across the four fields, and there is no rollback. Nothing is
 * logged here; the caller latches the errno and leaves its cache unchanged
 * unless the failure was transport-class.
 */
int pdg_uart_bottom_set_config(void *ctx, uint32_t baud_rate, uint8_t data_bits,
		uint8_t parity, uint8_t stop_bits);

/* Report whether the attached device advertises the UART capability bit,
 * storing the answer in *out_has_uart.
 *
 * The two failure shapes are deliberately distinct, and the caller depends on
 * telling them apart: a return of 0 with *out_has_uart false means the device
 * answered and does not have a UART (hw-rev1), which the driver reports as
 * -ENODEV; a negative return means the capability query itself failed, and
 * that mapped errno is reported instead. *out_has_uart is written only when
 * the query succeeds, so it must not be read after a negative return.
 *
 * This is NOT a warm local read. gallo_get_device_info() unconditionally
 * re-validates, which issues a fresh device/info RPC bounded at 300 seconds,
 * so calling this costs a second metadata round trip on top of the one the
 * MFD parent already paid during its strict open. That boot-time cost is an
 * accepted M5 tradeoff -- without the gate, a hw-rev1 board would swallow
 * every transmitted byte silently -- and the escalation to cache the validated
 * device info in pico-de-gallo-lib and expose a cached FFI capability query is
 * recorded in the specification; it needs a crates/ change and is not done
 * here.
 */
int pdg_uart_bottom_has_uart(void *ctx, bool *out_has_uart);

#ifdef __cplusplus
}
#endif

#endif /* PDG_UART_BOTTOM_H */
