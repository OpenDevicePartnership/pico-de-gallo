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

/* Discard all recorded calls. Call from each test's setup. */
void pdg_fake_reset(void);

/* How many times pdg_common_bottom_open() was called. */
int pdg_fake_open_count(void);

/* How many times pdg_i2c_bottom_write() was called. */
int pdg_fake_i2c_write_count(void);

/* Copy the payload of the most recent pdg_i2c_bottom_write() into buf.
 * Returns the number of bytes written, or -1 if there was no such call or
 * the payload does not fit in buflen. Sets *addr to the target address.
 */
int pdg_fake_i2c_last_write(uint16_t *addr, uint8_t *buf, size_t buflen);

#ifdef __cplusplus
}
#endif

#endif /* PDG_FAKE_BOTTOM_H */
