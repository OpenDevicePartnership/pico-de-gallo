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

# Header search path for the fake. It needs its own directory, plus the
# production declaration sites of every symbol it overrides -- included rather
# than hand-copied so a signature drift is a compile error rather than
# undefined behaviour that links cleanly. All three headers are FFI-free
# (stdint/stddef only), so none of this drags pico_de_gallo.h in.
#
# This MUST be target_compile_options("-I...") and NOT the natural-looking
# target_include_directories(native_simulator INTERFACE ...). The fake is
# compiled by the native-simulator Makefile, not by CMake, and
# boards/native/common/natsim_config.cmake joins only INTERFACE_COMPILE_OPTIONS
# into NSI_BUILD_OPTIONS -- INTERFACE_INCLUDE_DIRECTORIES is never consumed and
# the flag silently vanishes. Same channel, and same reason, as
# zephyr/drivers/CMakeLists.txt:32-34. Do not "tidy" this back; it has already
# cost one red CI run (33106107101).
#
# Two things look like counter-evidence and are not. zephyr/drivers/i2c/
# CMakeLists.txt uses the include-directories form, but pdg_i2c_bottom.c and
# pdg_i2c_bottom.h sit in the same directory, so quoted-include lookup finds
# the header with no -I at all. And common_bottom.h resolves in the drivers
# build only because zephyr/drivers/CMakeLists.txt injects that directory with
# this same -I mechanism.
target_compile_options(native_simulator INTERFACE
  "-I${CMAKE_CURRENT_LIST_DIR}/../common"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/common"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/i2c")
