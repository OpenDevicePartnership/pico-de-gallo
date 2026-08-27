/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Embedded-facing view of the recording fake that replaces the host-context
 * bottom layer. The fake runs in the host context and the ztest assertions run
 * in the embedded context, so they share no globals; everything the test needs
 * to assert on must come through an accessor declared here. Same pattern, and
 * same reason, as zephyr/tests/pdg_mfd_m5/common/m5_bottom.h.
 */

#ifndef PDG_FAKE_BOTTOM_H
#define PDG_FAKE_BOTTOM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Discard all recorded calls. Call from each test's setup.
 *
 * INVARIANT: this never clears the open counter. The MFD parent opens during
 * POST_KERNEL device init, long before any ztest setup hook runs, so a reset
 * that cleared it would destroy the only evidence that the weak override took
 * effect -- and would make that assertion depend on test order. Only the
 * per-call recorders are cleared.
 */
void pdg_fake_reset(void);

/* How many times pdg_common_bottom_open() was called. Latched: never cleared
 * by pdg_fake_reset(). See the invariant above.
 */
int pdg_fake_open_count(void);

/* How many times pdg_i2c_bottom_write() was called. Counts plain writes only:
 * a write_read is counted by pdg_fake_i2c_write_read_count() instead, so a
 * test can prove the driver issued a plain write and not a write-read.
 */
int pdg_fake_i2c_write_count(void);

/* How many times pdg_i2c_bottom_write_read() was called. */
int pdg_fake_i2c_write_read_count(void);

/* Copy the payload of the most recent pdg_i2c_bottom_write() into buf.
 * Returns the number of bytes written, or -1 if there was no such call, the
 * payload does not fit in buflen, or buf is NULL with bytes to copy. Sets
 * *addr to the target address; addr may be NULL.
 *
 * The tx half of a write_read is recorded here too, so this reflects the most
 * recent write OR write_read. Use the two counters to tell them apart.
 */
int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen);

#ifdef __cplusplus
}
#endif

#endif /* PDG_FAKE_BOTTOM_H */
