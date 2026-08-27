/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * SPIKE -- THROWAWAY. Do not build on this.
 *
 * This exists to make a compiler answer one question that no amount of reading
 * can settle: does `#include`ing a Pico de Gallo driver translation unit
 * survive a `type: unit` build?
 *
 * Issue #109 wants hardware-free coverage of six static helpers. Upstream
 * Zephyr's answer to "I need to test a static function" is tests/unit/, whose
 * suites include the .c under test directly -- tests/unit/rbtree/main.c has
 * `#include "../../../lib/utils/rb.c"`, and base64, hex, winstream, crc,
 * net_timeout and cbprintf all do the same. The unit_testing board builds no
 * kernel, no scheduler, no device model and no devicetree, so the file under
 * test is compiled straight into this translation unit and its statics are
 * simply in scope.
 *
 * Whether *this* driver survives that treatment is a different question,
 * because pdg_i2c.c is not a leaf utility. At file scope it carries:
 *
 *   pdg_i2c.c:15  #define DT_DRV_COMPAT odp_pico_de_gallo_i2c
 *   pdg_i2c.c:20  #include <zephyr/device.h>
 *   pdg_i2c.c:22  #include <zephyr/kernel.h>
 *   pdg_i2c.c:70  DT_INST_FOREACH_STATUS_OKAY(PDG_I2C_PARENT_ASSERTS)
 *   pdg_i2c.c:72  #include "pdg_mfd.h"
 *   pdg_i2c.c:74  LOG_MODULE_REGISTER(i2c_pico_de_gallo, CONFIG_I2C_LOG_LEVEL)
 *   pdg_i2c.c:829 DT_INST_FOREACH_STATUS_OKAY(PDG_I2C_INIT)
 *
 * Predicted outcomes, in the order I expect them to bite:
 *
 *  1. The two DT_INST_FOREACH_STATUS_OKAY() should expand to nothing.
 *     cmake/modules/unittest.cmake file(TOUCH)es an EMPTY devicetree_generated.h,
 *     so there are zero instances and neither the parent assertions nor
 *     I2C_DEVICE_DT_INST_DEFINE is ever emitted. If this is wrong, the build
 *     fails on an undefined DT macro and the whole route is in doubt.
 *  2. LOG_MODULE_REGISTER / LOG_ERR are the most likely failure. With
 *     CONFIG_LOG=n the logging header degrades to the minimal backend, which
 *     still wants printk. Upstream's fix is to add
 *     ${ZEPHYR_BASE}/subsys/logging/log_minimal.c to testbinary, exactly as
 *     tests/bluetooth/host/id/bt_id_add/CMakeLists.txt does. Deliberately NOT
 *     pre-emptively added, so the error message tells us what is really needed
 *     rather than what I guessed.
 *  3. CONFIG_I2C_LOG_LEVEL is undefined without the I2C subsystem's Kconfig.
 *  4. <zephyr/kernel.h> may not compile standalone outside a kernel build.
 *
 * Any of 2-4 is a fixable stub; only 1 would send us to route (A), extracting
 * the helpers into a pdg_i2c-priv.h/.c pair with external linkage on the model
 * of drivers/i2c/i2c-priv.h.
 *
 * The single assertion below is intentionally trivial. Coverage is not the
 * point; reaching the linker is.
 */

#include <zephyr/ztest.h>

#include "../../../drivers/i2c/pdg_i2c.c"

ZTEST_SUITE(pdg_i2c_msgs, NULL, NULL, NULL, NULL, NULL);

/*
 * speed_to_code_() maps a Zephyr I2C speed onto the wire's frequency byte:
 * 0 = Standard, 1 = Fast, 2 = Fast+. Those values are stable C ABI (AGENTS.md
 * section 8) and mirror the I2cFrequency wire enum, whose variant order is
 * itself ABI (section 6.1).
 */
ZTEST(pdg_i2c_msgs, test_speed_to_code_maps_standard)
{
	uint8_t code = 0xFFU;

	zassert_ok(speed_to_code_(I2C_SPEED_STANDARD, &code));
	zassert_equal(code, 0U, "I2C_SPEED_STANDARD must map to 0, got %u", code);
}
