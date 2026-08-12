# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

zephyr_library()
zephyr_library_sources(pdg_spi.c)
zephyr_library_include_directories(${CMAKE_CURRENT_LIST_DIR})

# The bottom file runs in the native simulator (host) context and needs the pico_de_gallo.h from the module's FFI target.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/pdg_spi_bottom.c)
target_include_directories(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR})
