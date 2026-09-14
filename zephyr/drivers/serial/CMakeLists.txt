# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

# Top half: embedded (Zephyr) context. Sees Zephyr headers only; it must never
# include pico_de_gallo.h.
zephyr_library()
zephyr_library_sources(pdg_uart.c)
zephyr_library_include_directories(${CMAKE_CURRENT_LIST_DIR})

# Bottom half: native simulator (host) context. Sees the host libc and the
# generated pico_de_gallo.h; it must never include a Zephyr header.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/pdg_uart_bottom.c)

# Deliberately NO target_include_directories(native_simulator INTERFACE ...)
# here. pdg_uart_bottom.c quotes pdg_uart_bottom.h from its own directory, which
# the preprocessor resolves relative to the including file, so no -I is needed.
# The sibling i2c/gpio/spi CMakeLists carry such a line; it is ineffective and
# redundant for the same reason, not load-bearing - please do not re-add it here.
#
# If a genuinely cross-directory header is ever needed on the native side, it
# must be supplied with:
#     target_compile_options(native_simulator INTERFACE "-I<dir>")
# Zephyr's boards/native/common/natsim_config.cmake joins only
# INTERFACE_COMPILE_OPTIONS into NSI_BUILD_OPTIONS, so the
# target_include_directories form silently vanishes. The two real cross-directory
# headers are already handled that way: common.h by ../CMakeLists.txt and
# pico_de_gallo.h by zephyr/CMakeLists.txt.
#
# -Werror=switch is applied native-simulator-wide by ../CMakeLists.txt; it is the
# parent's job and is not repeated here.
