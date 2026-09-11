/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Host-context shim for the Pico de Gallo UART driver.
 *
 * This file is compiled into the native simulator runner with the host C
 * library and links against the Pico de Gallo FFI shared object. It must not
 * include any Zephyr headers.
 *
 * Nothing here logs. The UART being driven may itself be the console, so a
 * diagnostic emitted from the output path would recurse per character. The top
 * half latches the exact errno and the failure class instead.
 */

#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "pico_de_gallo.h"
#include "common.h"
#include "pdg_uart_bottom.h"

/*
 * This translation unit is the only one that sees both the neutral
 * PDG_UART_* vocabulary from pdg_uart_bottom.h and the FFI's GalloUart*
 * enumerators from pico_de_gallo.h, so it is the only place the correspondence
 * can be checked at all.
 *
 * The FFI enums mirror the pico-de-gallo-internal wire enums, whose variant
 * order is itself ABI (AGENTS.md 6.1). These assertions are what turn a
 * reordered Rust variant into a build failure here rather than a silently wrong
 * byte on the wire -- a UART configured for the wrong parity or word length
 * does not fail, it corrupts every character.
 *
 * Each assertion is independently meaningful: nothing is checked in bulk, so
 * any single mismatched discriminant names itself in the diagnostic.
 *
 * The set is exhaustive for today's 4 + 5 + 2 FFI variants. It cannot detect a
 * future variant appended to one of those enums, because no generated symbol
 * exports a variant count; closing that residual needs a crates/ change and is
 * out of scope here.
 *
 * The enumerators are usable as integer constant expressions under C11/C17
 * because cbindgen emits the enum bodies outside the C23 `: uint8_t` typedef
 * conditional; only the `typedef` itself is conditional. The same technique
 * pins PDG_SPI_MAX_BUFFER in drivers/spi/pdg_spi_bottom.c and the mirrored GPIO
 * discriminants in tests/pdg_mfd_m5/common/m5_bottom.c.
 */

/* Word length: PDG_UART_DATA_BITS_* vs GalloUartDataBits. */
_Static_assert(PDG_UART_DATA_BITS_5 == GalloUartDataBits_Five,
	       "PDG_UART_DATA_BITS_5 must match GalloUartDataBits_Five");
_Static_assert(PDG_UART_DATA_BITS_6 == GalloUartDataBits_Six,
	       "PDG_UART_DATA_BITS_6 must match GalloUartDataBits_Six");
_Static_assert(PDG_UART_DATA_BITS_7 == GalloUartDataBits_Seven,
	       "PDG_UART_DATA_BITS_7 must match GalloUartDataBits_Seven");
_Static_assert(PDG_UART_DATA_BITS_8 == GalloUartDataBits_Eight,
	       "PDG_UART_DATA_BITS_8 must match GalloUartDataBits_Eight");

/* Parity mode: PDG_UART_PARITY_* vs GalloUartParity. */
_Static_assert(PDG_UART_PARITY_NONE == GalloUartParity_None,
	       "PDG_UART_PARITY_NONE must match GalloUartParity_None");
_Static_assert(PDG_UART_PARITY_ODD == GalloUartParity_Odd,
	       "PDG_UART_PARITY_ODD must match GalloUartParity_Odd");
_Static_assert(PDG_UART_PARITY_EVEN == GalloUartParity_Even,
	       "PDG_UART_PARITY_EVEN must match GalloUartParity_Even");
_Static_assert(PDG_UART_PARITY_MARK == GalloUartParity_Mark,
	       "PDG_UART_PARITY_MARK must match GalloUartParity_Mark");
_Static_assert(PDG_UART_PARITY_SPACE == GalloUartParity_Space,
	       "PDG_UART_PARITY_SPACE must match GalloUartParity_Space");

/*
 * Stop-bit count: PDG_UART_STOP_BITS_* vs GalloUartStopBits. These are the
 * assertions most worth having, because the neutral values are deliberately
 * NOT Zephyr's ordinals -- UART_CFG_STOP_BITS_1 is 1 while
 * PDG_UART_STOP_BITS_1 is 0 -- so an off-by-one here would look plausible on
 * both sides.
 */
_Static_assert(PDG_UART_STOP_BITS_1 == GalloUartStopBits_One,
	       "PDG_UART_STOP_BITS_1 must match GalloUartStopBits_One");
_Static_assert(PDG_UART_STOP_BITS_2 == GalloUartStopBits_Two,
	       "PDG_UART_STOP_BITS_2 must match GalloUartStopBits_Two");

/*
 * Staging-ring size. A UART read returns its payload inside one postcard-rpc
 * response frame, so the response payload ceiling is the binding limit on a
 * single refill. pdg_uart_bottom.h declares it as a literal because the
 * embedded half includes that header and must never see pico_de_gallo.h; this
 * is where the literal is tied back to its source of truth.
 */
_Static_assert(PDG_UART_RX_BUFFER_SIZE == GALLO_MAX_RESPONSE_PAYLOAD,
	       "PDG_UART_RX_BUFFER_SIZE must track GALLO_MAX_RESPONSE_PAYLOAD: a UART "
	       "read returns its payload in one response frame, so the response "
	       "ceiling is the binding limit on a single refill");

/*
 * These four are __attribute__((weak)) so the recording fake planned for
 * Milestone 6 can link strong definitions and observe what the driver asked
 * the link to do. Not Zephyr's __weak: this file is host-context and cannot
 * include zephyr/toolchain.h.
 *
 * Every FFI return is routed through pdg_common_status_to_errno() rather than
 * propagated as-is. cbindgen emits `typedef int32_t Status` under C11/C17, so
 * returning a raw Status compiles silently and hands the top half a positive,
 * unmapped number that it would then treat as a negative POSIX errno.
 */

__attribute__((weak)) int pdg_uart_bottom_read(void *ctx, uint8_t *buf,
					       uint16_t count,
					       uint32_t timeout_ms,
					       uint16_t *out_len)
{
	return pdg_common_status_to_errno(
		gallo_uart_read((const struct PicoDeGallo *)ctx, buf, count,
				timeout_ms, out_len));
}

__attribute__((weak)) int pdg_uart_bottom_write(void *ctx, const uint8_t *buf,
						uint16_t len)
{
	return pdg_common_status_to_errno(
		gallo_uart_write((const struct PicoDeGallo *)ctx, buf, len));
}

__attribute__((weak)) int pdg_uart_bottom_set_config(void *ctx,
						     uint32_t baud_rate,
						     uint8_t data_bits,
						     uint8_t parity,
						     uint8_t stop_bits)
{
	return pdg_common_status_to_errno(
		gallo_uart_set_config((const struct PicoDeGallo *)ctx,
				      baud_rate, data_bits, parity, stop_bits));
}

/*
 * Capability gate.
 *
 * This is not a warm or cached read. gallo_get_device_info() re-validates
 * unconditionally, which issues a fresh device/info RPC bounded at 300
 * seconds, so each call costs a full metadata round trip on top of the one the
 * MFD parent already paid during its strict open. The driver pays it once at
 * init; without the gate a hw-rev1 board would swallow every transmitted byte
 * silently.
 *
 * `info` is a bare stack struct, and the FFI leaves it untouched when the
 * query fails, so `capabilities` is read only after the status has been
 * confirmed successful. Reading it first would test a capability bit against
 * whatever the stack happened to hold.
 *
 * The two failure shapes stay distinct, and the caller depends on telling them
 * apart: 0 with *out_has_uart false means the device answered and has no UART,
 * which the top half reports as -ENODEV; a negative return means the query
 * itself failed and that errno is reported instead. *out_has_uart is written
 * only on success.
 */
__attribute__((weak)) int pdg_uart_bottom_has_uart(void *ctx,
						   bool *out_has_uart)
{
	struct GalloDeviceInfo info;
	int ret;

	if (out_has_uart == NULL) {
		return -EINVAL;
	}

	ret = pdg_common_status_to_errno(
		gallo_get_device_info((const struct PicoDeGallo *)ctx, &info));
	if (ret != 0) {
		return ret;
	}

	*out_has_uart = (info.capabilities & GALLO_CAP_UART) != 0U;

	return 0;
}
