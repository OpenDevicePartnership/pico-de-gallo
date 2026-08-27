# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_fake_i2c)

target_sources(app PRIVATE src/main.c)

# The embedded side calls the fake's accessors, so it needs the FFI-free header.
target_include_directories(app PRIVATE ${CMAKE_CURRENT_LIST_DIR}/../common)

# The fake itself runs in the host context, and provides STRONG definitions that
# override the weak ones in drivers/common/common.c and drivers/i2c/
# pdg_i2c_bottom.c. Same mechanism as zephyr/drivers/CMakeLists.txt.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../common/pdg_fake_bottom.c)
target_include_directories(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../common)

# The fake includes the production declaration sites of every symbol it
# overrides, so a signature drift is a compile error rather than undefined
# behaviour that links cleanly. Both headers are FFI-free (stdint/stddef only).
target_include_directories(native_simulator INTERFACE
  ${PDG_MODULE_ROOT}/zephyr/drivers/common
  ${PDG_MODULE_ROOT}/zephyr/drivers/i2c)
