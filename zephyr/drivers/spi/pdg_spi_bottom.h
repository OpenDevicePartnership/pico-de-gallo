/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Interface between the embedded-context Zephyr SPI driver and the
 * host-context wrapper that calls the Pico de Gallo C FFI.
 *
 * Chip select is no longer part of this interface. The Zephyr driver drives
 * every chip-select edge through the odp,pico-de-gallo-gpio child declared in
 * the controller's cs-gpios property, so the only firmware operations reached
 * from here are bus configuration and one full-duplex transfer.
 */

#ifndef PDG_SPI_BOTTOM_H
#define PDG_SPI_BOTTOM_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Largest full-duplex transfer this driver will attempt, in bytes.
 *
 * Every Zephyr SPI operation reaches the device through one
 * `gallo_spi_transfer()`, which is full duplex: the same `len` is both
 * clocked out and clocked back. The binding limit is therefore what a single
 * response frame can carry, and that is derived rather than guessed --
 * `pico_de_gallo_internal::MAX_RESPONSE_PAYLOAD`, exported to C as
 * `GALLO_MAX_RESPONSE_PAYLOAD`:
 *
 *     1024   postcard-rpc host inbound transfer buffer
 *      - 7   response header (1 discriminant + 2 key + 4 sequence)
 *      - 1   postcard Result discriminant
 *      - 2   postcard varint length prefix
 *     =1014
 *
 * Declared here, as a literal, so both halves of the driver can see it while
 * the embedded-context half stays free of FFI headers. `pdg_spi_bottom.c`
 * carries a `_Static_assert` tying it to `GALLO_MAX_RESPONSE_PAYLOAD`, so
 * drift is a build failure rather than a silent divergence.
 *
 * SUPERSEDES the previous value of 1013, which was measured rather than
 * derived and was off by one against the real edge. That figure came from
 * the M5 acceptance run, which probed 1013 (worked) and 1015 (hung) but
 * never 1014, and which therefore could not narrow the boundary without
 * stepping into the hang. Issues #158 and #179 closed both gaps: 1014
 * returns normally and 1015 fails cleanly, measured directly on
 * `spi/transfer`, `spi/read`, `i2c/read` and `onewire/read` across two
 * boards.
 *
 * Three further claims attached to the old constant are also superseded:
 *
 *   - "1013 is close to 1024, which would be consistent with a ~1 KiB budget
 *     and about 11 bytes of framing. That is SUGGESTIVE ONLY: there is no
 *     evidence for that decomposition." There is now: the decomposition
 *     above is exact, each term pinned by a test in the wire crate. The
 *     count is 10 bytes of framing, not 11.
 *
 *   - "Full duplex succeeded at 512 bytes and failed at 3072 bytes. It was
 *     not tested from 513 through 1013, so duplex at 1013 is UNVERIFIED.
 *     Applications needing a documented-safe duplex size must use 512 bytes
 *     or less." `spi/transfer` *is* the duplex endpoint, and #158 measured
 *     it directly at 1014 (ok) and 1015 (clean error). The 512-byte advice
 *     is no longer needed.
 *
 *   - The 1015-byte dispatcher hang described below was NOT reproducible in
 *     #158 triage on firmware `62dd64e710fd`: the call returned a clean
 *     error in 12 ms and the board stayed responsive. Two changes landed in
 *     between that bear on it -- the #157 watchdog supervisor, which resets
 *     a genuinely wedged dispatcher in ~10.5 s, and #178, which bounds every
 *     host RPC so a lost response is a `Timeout` rather than a permanent
 *     hang. The hang is recorded here as history, not as current behaviour.
 *
 * The FOLLOW-UP the old comment asked for -- "derive the usable ceiling from
 * the worst-case framing, express it as one generated or shared contract
 * rather than a constant duplicated per consumer, and pin limit and limit+1
 * tests against it" -- is what #179 and #158 did. It needed no wire-format
 * or schema change, contrary to the expectation recorded there.
 *
 * Raising this number still requires raising `GALLO_MAX_RESPONSE_PAYLOAD`
 * first, which means changing postcard-rpc's inbound transfer size. That
 * constant is private and non-configurable upstream.
 */
#define PDG_SPI_MAX_BUFFER 1014U

/* Configure the SPI bus frequency, clock phase and clock polarity. */
int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency, bool phase, bool polarity);

/* Clock `len` bytes out of `write_buf` while clocking `len` bytes into
 * `read_buf`. Both pointers must be valid for `len` bytes; the caller supplies
 * zero-filled TX scratch for a read-only transfer and discard RX scratch for a
 * write-only one, because the firmware endpoint is always full duplex.
 *
 * Returns 0, -EINVAL, -EMSGSIZE, -EPROTO, -EIO or -ECOMM.
 */
int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf,
		uint8_t *read_buf, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* PDG_SPI_BOTTOM_H */
