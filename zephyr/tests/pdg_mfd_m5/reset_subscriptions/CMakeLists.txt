# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo M5 tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo M5 tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_m5_reset_subscriptions)

target_sources(app PRIVATE src/main.c)

# pdg_mfd.h is the child-facing accessor for the borrowed context; m5_bottom.h
# is this suite's host-context shim interface. Both are basic-C-types headers.
target_include_directories(app PRIVATE
	${PDG_MODULE_ROOT}/zephyr/drivers/mfd
	${CMAKE_CURRENT_LIST_DIR}/../common)

# The shim runs in the native simulator (host) context, where the generated
# pico_de_gallo.h lives. Same mechanism as zephyr/drivers/CMakeLists.txt.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../common/m5_bottom.c)
target_include_directories(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../common)
