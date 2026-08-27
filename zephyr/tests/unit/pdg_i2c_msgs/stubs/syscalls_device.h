/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * SPIKE -- THROWAWAY. Stand-in for the generated <zephyr/syscalls/device.h>.
 *
 * cmake/modules/unittest.cmake file(TOUCH)es an empty file for each generated
 * header the unit_testing board needs, from a hard-coded list:
 *
 *     devicetree_generated.h  heap_constants.h  offsets.h
 *     syscall_list.h          syscall_macros.h
 *     syscalls/{kernel,kobject,log_core,log_ctrl,log_msg,sys_clock}.h
 *
 * syscalls/device.h is NOT on that list, and <zephyr/device.h> includes it
 * unconditionally at :1438. That is why no suite under tests/unit/ is a driver:
 * the harness was curated for kernel and library code and never extended to the
 * device model.
 *
 * An empty file is not enough. device_is_ready() is declared `__syscall`, so
 * its only declaration is the inline wrapper the real generated header would
 * carry, and pdg_i2c.c:766 calls it. Hence a stub with a body rather than a
 * TOUCH.
 *
 * The body is deliberately the weakest thing that is not a lie. A unit test
 * reaching device_is_ready() has left the pure-helper territory this suite is
 * meant to cover, so this exists to satisfy the linker, not to model readiness.
 */

#ifndef PDG_SPIKE_SYSCALLS_DEVICE_H
#define PDG_SPIKE_SYSCALLS_DEVICE_H

static inline bool device_is_ready(const struct device *dev)
{
	return dev != NULL;
}

#endif /* PDG_SPIKE_SYSCALLS_DEVICE_H */
