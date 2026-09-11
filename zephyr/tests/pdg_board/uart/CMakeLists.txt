# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_board_uart)

# Deliberately a plain application, NOT a ztest suite.
#
# Every step here needs a physically attached board with TX and RX shorted
# together, so nothing in it can ever be gated by an automated runner. A ztest
# binary invites twister to execute it; a plain main() that tests.yaml marks
# build_only does not. The output is greppable instead of asserted: see the
# PDG_BOARD_UART prefix discipline in src/main.c.
target_sources(app PRIVATE src/main.c)
