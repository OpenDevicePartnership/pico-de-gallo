# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

zephyr_library()
zephyr_library_sources(pdg_spi.c)
zephyr_library_include_directories(${CMAKE_CURRENT_LIST_DIR})
# pdg_spi.c uses Zephyr's private SPI driver helper, drivers/spi/spi_context.h.
# Upstream in-tree SPI drivers get that directory on the include path from
# zephyr/drivers/spi/CMakeLists.txt; an out-of-tree module has to ask.
zephyr_library_include_directories(${ZEPHYR_BASE}/drivers/spi)

# The bottom file runs in the native simulator (host) context and needs the pico_de_gallo.h from the module's FFI target.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/pdg_spi_bottom.c)
target_include_directories(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR})
