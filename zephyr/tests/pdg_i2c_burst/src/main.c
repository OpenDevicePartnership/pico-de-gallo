/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Regression coverage for issue #102: i2c_burst_write() was rejected with
 * -ENOTSUP.
 *
 * BOARD ATTACHED. This image opens USB through the MFD parent and drives a real
 * I2C target. The zephyr CI gate builds and links it but never runs it, so a
 * green CI run proves only that it still compiles. Behavioural evidence needs
 * the manual procedure in the "Running" section below.
 *
 * WHAT IS BEING TESTED. Before the fix, pdg_i2c.c accepted a STOP-delimited
 * message group only when it held one message, or a write followed by a
 * repeated-start read. Zephyr's i2c_burst_write() emits two *writes*
 * (I2C_MSG_WRITE, then I2C_MSG_WRITE | I2C_MSG_STOP), which matched neither and
 * fell through to -ENOTSUP. The driver now concatenates the writes of a group
 * into one payload and issues a single transaction.
 *
 * WHY THE ASSERTIONS COMPARE AGAINST A REFERENCE READ-BACK RATHER THAN THE
 * BYTES WRITTEN. Peripherals do not necessarily read back what you wrote: a
 * TMP102 in its default 12-bit mode holds its limit registers in bits [15:4]
 * and returns the low nibble as zero, while a TMP117 keeps all sixteen. So
 * every gather case here first establishes what the device returns after the
 * SAME bytes are written through a path that has always worked -- one
 * single-message i2c_write() of {register, byte, byte} -- and then requires the
 * gather path to reproduce exactly that. The test therefore makes no assumption
 * about the target's register width or bit masking.
 *
 * WHY EACH GATHER CASE IS PRECEDED BY WRITING A DIFFERENT VALUE. A read-back
 * that matches is worthless if the register already held the expected value.
 * Every gather case is set up by reference-writing the *other* pattern first,
 * so a driver that silently performed no bus traffic at all would fail.
 *
 * FIXTURE. One Pico de Gallo attached over USB, with an I2C target at
 * PDG_BURST_ADDR exposing an 8-bit register pointer and a writable 16-bit
 * register at PDG_BURST_REG. A TI TMP117 or TMP102 at 0x48 satisfies this, and
 * is the same part samples/i2c_bridge expects. The register is restored to its
 * entry value on every exit path that reaches the end.
 *
 * RUNNING (from the repository root, with a west workspace and ZEPHYR_BASE
 * set):
 *
 *     west build -p always -b native_sim/native/64 zephyr/tests/pdg_i2c_burst \
 *         -- -DSHIELD=pico_de_gallo -DEXTRA_ZEPHYR_MODULES=$PWD \
 *            -DDTC_OVERLAY_FILE=$PWD/zephyr/tests/pdg_i2c_burst/burst.overlay
 *     west build -t run
 *
 * Success is the single line PDG_I2C_BURST_PASS and exit status 0. Every
 * failure prints PDG_I2C_BURST_FAIL with the step, the expected value and the
 * observed value, and exits nonzero immediately.
 */

#include <zephyr/device.h>
#include <zephyr/devicetree.h>
#include <zephyr/drivers/i2c.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include <zephyr/sys/util.h>

#include <errno.h>
#include <stdint.h>
#include <string.h>

#include "posix_board_if.h"

#define PDG_BURST_NODE DT_NODELABEL(pdg_i2c0)

BUILD_ASSERT(DT_NODE_HAS_STATUS_OKAY(PDG_BURST_NODE),
	     "The I2C burst-write regression image requires pdg_i2c0 to be okay");

/*
 * Target address and register.
 *
 * 0x48 is the TMP11x/TMP102 address used by samples/i2c_bridge. Register 0x02
 * is a limit register on both parts: writable, 16-bit, and volatile unless
 * explicitly committed to EEPROM, which this test never does.
 */
#define PDG_BURST_ADDR 0x48U
#define PDG_BURST_REG 0x02U
#define PDG_BURST_REG_LEN 2U

/*
 * Two distinguishable payloads. Their values are arbitrary; only the fact that
 * they differ matters, because every comparison is against a reference
 * read-back rather than against these bytes.
 */
static uint8_t pattern_a[PDG_BURST_REG_LEN] = { 0x12U, 0x30U };
static uint8_t pattern_b[PDG_BURST_REG_LEN] = { 0x45U, 0x60U };

/*
 * Mirrors PDG_I2C_MAX_WRITE in zephyr/drivers/i2c/pdg_i2c.c.
 *
 * Duplicated rather than shared, because that constant lives in a .c file with
 * no header and reaching into driver internals from a test would be worse. This
 * is deliberately NOT asserted equal to anything here: an assertion that a
 * macro equals itself proves nothing. The value actually compiled into the
 * driver is pinned instead by the behaviour of the oversize case below.
 */
#define PDG_BURST_CEILING 4096U

/* Oversize payloads for the running-total case. In .bss, never on the stack. */
static uint8_t oversize_head[PDG_BURST_CEILING];
static uint8_t oversize_tail[1];

static const struct device *const i2c_dev = DEVICE_DT_GET(PDG_BURST_NODE);

static void fail(const char *step, const char *detail)
{
	printk("PDG_I2C_BURST_FAIL step=%s %s\n", step, detail);
	posix_exit(1);
}

static void fail_errno(const char *step, const char *what, int expected, int actual)
{
	printk("PDG_I2C_BURST_FAIL step=%s call=%s expected=%d observed=%d\n",
	       step, what, expected, actual);
	posix_exit(1);
}

static void dump(const char *step, const char *label, const uint8_t *bytes)
{
	printk("PDG_I2C_BURST_BYTES step=%s %s=0x%02x%02x\n",
	       step, label, bytes[0], bytes[1]);
}

/*
 * Read the register through i2c_burst_read(), which emits a write of the
 * register pointer followed by a repeated-start read. That shape was accepted
 * before this fix and is accepted after it, so it is the stable observation
 * channel every case below reads through.
 */
static void read_reg(const char *step, uint8_t *out)
{
	int ret = i2c_burst_read(i2c_dev, PDG_BURST_ADDR, PDG_BURST_REG, out,
				 PDG_BURST_REG_LEN);

	if (ret != 0) {
		fail_errno(step, "i2c_burst_read", 0, ret);
	}
}

/*
 * Write the register through one single-message i2c_write() of
 * {register, byte, byte}.
 *
 * This is the reference path. A one-message group has always been accepted, so
 * it establishes ground truth independently of anything issue #102 changed.
 */
static void write_reg_reference(const char *step, const uint8_t *value)
{
	uint8_t frame[1U + PDG_BURST_REG_LEN];
	int ret;

	frame[0] = PDG_BURST_REG;
	memcpy(&frame[1], value, PDG_BURST_REG_LEN);

	ret = i2c_write(i2c_dev, frame, sizeof(frame), PDG_BURST_ADDR);
	if (ret != 0) {
		fail_errno(step, "i2c_write", 0, ret);
	}
}

/* Reference-write `value`, read it back, and report what the device returns. */
static void calibrate(const char *step, const uint8_t *value, uint8_t *out)
{
	write_reg_reference(step, value);
	read_reg(step, out);
	dump(step, "reference", out);
}

/* Require the register to read back exactly `expected`. */
static void require_reg(const char *step, const uint8_t *expected)
{
	uint8_t observed[PDG_BURST_REG_LEN];

	read_reg(step, observed);

	if (memcmp(observed, expected, PDG_BURST_REG_LEN) != 0) {
		dump(step, "expected", expected);
		dump(step, "observed", observed);
		fail(step, "reason=readback-mismatch");
	}
}

/* Require an i2c_transfer() of `msgs` to return exactly `expected`. */
static void require_transfer(const char *step, struct i2c_msg *msgs, uint8_t num_msgs,
			     int expected)
{
	int ret = i2c_transfer(i2c_dev, msgs, num_msgs, PDG_BURST_ADDR);

	if (ret != expected) {
		fail_errno(step, "i2c_transfer", expected, ret);
	}
}

int main(void)
{
	uint8_t entry[PDG_BURST_REG_LEN];
	uint8_t ref_a[PDG_BURST_REG_LEN];
	uint8_t ref_b[PDG_BURST_REG_LEN];

	printk("PDG_I2C_BURST_BEGIN\n");

	if (!device_is_ready(i2c_dev)) {
		fail("T0", "reason=i2c-controller-not-ready");
	}

	/*
	 * T0 -- capture the entry value so the register can be put back, and
	 * prove the observation channel works before anything depends on it.
	 */
	read_reg("T0", entry);
	dump("T0", "entry", entry);

	/*
	 * T1 -- calibrate. Establish what the device returns for each pattern
	 * when written through the reference path, and require the two to
	 * differ. If they do not, this fixture cannot distinguish a working
	 * gather from a no-op, and every later comparison would be vacuous.
	 */
	calibrate("T1a", pattern_a, ref_a);
	calibrate("T1b", pattern_b, ref_b);

	if (memcmp(ref_a, ref_b, PDG_BURST_REG_LEN) == 0) {
		dump("T1", "ref_a", ref_a);
		dump("T1", "ref_b", ref_b);
		fail("T1", "reason=patterns-indistinguishable-on-this-target");
	}

	/*
	 * T2 -- THE REGRESSION. i2c_burst_write() emits I2C_MSG_WRITE followed
	 * by I2C_MSG_WRITE | I2C_MSG_STOP. Before the fix this returned
	 * -ENOTSUP. The register currently holds ref_b, so a driver that
	 * returned success without issuing any traffic fails the read-back.
	 */
	{
		int ret = i2c_burst_write(i2c_dev, PDG_BURST_ADDR, PDG_BURST_REG,
					  pattern_a, PDG_BURST_REG_LEN);

		if (ret != 0) {
			fail_errno("T2", "i2c_burst_write", 0, ret);
		}
	}
	require_reg("T2", ref_a);
	printk("PDG_I2C_BURST_T2_BURST_WRITE=OK\n");

	/*
	 * T3 -- three-message hand-rolled gather. i2c_burst_write() only ever
	 * produces two messages, so this is the case that proves concatenation
	 * generalises past N == 2, which is where the bulk of the affected
	 * driver population actually is.
	 */
	write_reg_reference("T3", pattern_b);
	{
		uint8_t reg = PDG_BURST_REG;
		struct i2c_msg msgs[3] = {
			{ .buf = &reg, .len = 1U, .flags = I2C_MSG_WRITE },
			{ .buf = &pattern_a[0], .len = 1U, .flags = I2C_MSG_WRITE },
			{ .buf = &pattern_a[1], .len = 1U,
			  .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
		};

		require_transfer("T3", msgs, ARRAY_SIZE(msgs), 0);
	}
	require_reg("T3", ref_a);
	printk("PDG_I2C_BURST_T3_THREE_MESSAGE_GATHER=OK\n");

	/*
	 * T4 -- gathered writes followed by a repeated-start read, which routes
	 * through gallo_i2c_write_read() rather than gallo_i2c_write().
	 *
	 * The empty second write is synthetic: this target's register pointer is
	 * a single byte, so there is no natural way to split the write side of a
	 * write-then-read across two messages. It is nonetheless the shape that
	 * matters, because two write messages take the merge path while one does
	 * not, and the assertion is real -- the bytes returned must equal what
	 * the plain read channel returns for the same register.
	 */
	{
		uint8_t reg = PDG_BURST_REG;
		uint8_t observed[PDG_BURST_REG_LEN];
		struct i2c_msg msgs[3] = {
			{ .buf = &reg, .len = 1U, .flags = I2C_MSG_WRITE },
			{ .buf = &reg, .len = 0U, .flags = I2C_MSG_WRITE },
			{ .buf = observed, .len = PDG_BURST_REG_LEN,
			  .flags = I2C_MSG_READ | I2C_MSG_RESTART | I2C_MSG_STOP },
		};

		require_transfer("T4", msgs, ARRAY_SIZE(msgs), 0);

		if (memcmp(observed, ref_a, PDG_BURST_REG_LEN) != 0) {
			dump("T4", "expected", ref_a);
			dump("T4", "observed", observed);
			fail("T4", "reason=gathered-write-read-mismatch");
		}
	}
	printk("PDG_I2C_BURST_T4_GATHERED_WRITE_READ=OK\n");

	/*
	 * T5 -- a read that is not the last message of its group stays
	 * -ENOTSUP. The FFI has no shape for reading and then continuing within
	 * one transaction, and splitting the group would insert a STOP the
	 * caller never asked for.
	 */
	{
		uint8_t reg = PDG_BURST_REG;
		uint8_t scratch[PDG_BURST_REG_LEN];
		struct i2c_msg msgs[2] = {
			{ .buf = scratch, .len = PDG_BURST_REG_LEN,
			  .flags = I2C_MSG_READ },
			{ .buf = &reg, .len = 1U, .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
		};

		require_transfer("T5", msgs, ARRAY_SIZE(msgs), -ENOTSUP);
	}

	/* T6 -- two reads in one group stays -ENOTSUP for the same reason. */
	{
		uint8_t reg = PDG_BURST_REG;
		uint8_t scratch[PDG_BURST_REG_LEN];
		struct i2c_msg msgs[3] = {
			{ .buf = &reg, .len = 1U, .flags = I2C_MSG_WRITE },
			{ .buf = &scratch[0], .len = 1U,
			  .flags = I2C_MSG_READ | I2C_MSG_RESTART },
			{ .buf = &scratch[1], .len = 1U,
			  .flags = I2C_MSG_READ | I2C_MSG_STOP },
		};

		require_transfer("T6", msgs, ARRAY_SIZE(msgs), -ENOTSUP);
	}

	/*
	 * T7 -- a write-then-read group whose read omits I2C_MSG_RESTART stays
	 * -ENOTSUP. Unchanged from the two-message rule this replaces, and
	 * pinned here so that relaxing it later is a deliberate act with a
	 * failing test attached rather than an accident.
	 */
	{
		uint8_t reg = PDG_BURST_REG;
		uint8_t scratch[PDG_BURST_REG_LEN];
		struct i2c_msg msgs[2] = {
			{ .buf = &reg, .len = 1U, .flags = I2C_MSG_WRITE },
			{ .buf = scratch, .len = PDG_BURST_REG_LEN,
			  .flags = I2C_MSG_READ | I2C_MSG_STOP },
		};

		require_transfer("T7", msgs, ARRAY_SIZE(msgs), -ENOTSUP);
	}
	printk("PDG_I2C_BURST_T5_T7_REJECTIONS=OK\n");

	/*
	 * T8 -- the running total. Two writes that each pass a per-message check
	 * against the ceiling but exceed it in aggregate must be rejected with
	 * -EMSGSIZE, not concatenated into an oversized payload.
	 *
	 * -EMSGSIZE specifically, not merely "some error": before the fix this
	 * same group returned -ENOTSUP, from the accept-shape check, without the
	 * size ever being considered in aggregate.
	 *
	 * The accept side is deliberately not probed. A successful 4096-byte
	 * write would put a payload of unmeasured safety on the wire; the
	 * sibling SPI endpoint wedges the firmware dispatcher device-wide well
	 * below its own nominal 4096 (issue #146).
	 */
	{
		struct i2c_msg msgs[2] = {
			{ .buf = oversize_head, .len = PDG_BURST_CEILING,
			  .flags = I2C_MSG_WRITE },
			{ .buf = oversize_tail, .len = sizeof(oversize_tail),
			  .flags = I2C_MSG_WRITE | I2C_MSG_STOP },
		};

		require_transfer("T8", msgs, ARRAY_SIZE(msgs), -EMSGSIZE);
	}

	/*
	 * The rejection above happens in a complete pre-pass, before the
	 * controller mutex is taken and before any FFI call. The register must
	 * therefore be untouched -- no partial bus traffic, no leading write
	 * that reached the device before the offending message was noticed.
	 */
	require_reg("T8", ref_a);
	printk("PDG_I2C_BURST_T8_RUNNING_TOTAL=OK\n");

	/* Restore the register so repeated runs start from the same state. */
	write_reg_reference("T9", entry);
	require_reg("T9", entry);

	printk("PDG_I2C_BURST_PASS\n");

	posix_exit(0);

	return 0;
}
