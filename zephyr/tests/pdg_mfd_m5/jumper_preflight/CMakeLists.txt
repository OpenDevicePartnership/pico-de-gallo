# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo M5 tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo M5 tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_m5_jumper_preflight)

target_sources(app PRIVATE src/main.c)

# This image uses only the Zephyr GPIO API. It needs no host-context shim, so
# m5_bottom.c is deliberately not linked here.
