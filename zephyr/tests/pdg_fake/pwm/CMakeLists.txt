# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_fake_pwm)

target_sources(app PRIVATE src/main.c)

# The embedded side calls the accessors of both fakes.
target_include_directories(app PRIVATE
  ${CMAKE_CURRENT_LIST_DIR}/../common
  ${CMAKE_CURRENT_LIST_DIR})

# The fakes run in the host context and provide STRONG definitions overriding
# the weak ones in drivers/common/common.c and drivers/pwm/pdg_pwm_bottom.c.
# Both halves are needed: the shared parent fake supplies
# pdg_common_bottom_open()/_close(), keeping pdg_mfd_init() out of
# gallo_init_strict(); the PWM fake supplies the five pdg_pwm_bottom_*
# definitions.
target_sources(native_simulator INTERFACE
  ${CMAKE_CURRENT_LIST_DIR}/../common/pdg_fake_bottom.c
  ${CMAKE_CURRENT_LIST_DIR}/pdg_pwm_fake_bottom.c)

# This MUST be target_compile_options("-I...") and NOT
# target_include_directories(native_simulator INTERFACE ...). The fakes are
# compiled by the native-simulator Makefile, not by CMake, and
# boards/native/common/natsim_config.cmake joins only INTERFACE_COMPILE_OPTIONS
# into NSI_BUILD_OPTIONS -- INTERFACE_INCLUDE_DIRECTORIES is never consumed and
# the flag silently vanishes. Do not "tidy" this back; it has already cost one
# red CI run (33106107101).
#
# drivers/i2c is on the list because the SHARED parent fake includes
# pdg_i2c_bottom.h; this suite enables no I2C child, but the fake is one
# translation unit serving every suite.
target_compile_options(native_simulator INTERFACE
  "-I${CMAKE_CURRENT_LIST_DIR}/../common"
  "-I${CMAKE_CURRENT_LIST_DIR}"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/common"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/i2c"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/pwm")
