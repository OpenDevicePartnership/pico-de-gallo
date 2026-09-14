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

# ---------------------------------------------------------------------------
# BEST-EFFORT post-link symbol gate, ported verbatim in mechanism from
# zephyr/tests/pdg_fake/uart. SELF-CONTAINED AND EASILY REMOVED: delete this
# whole block and verify_overrides.cmake, and nothing else changes.
#
# The fakes hand the driver an opaque NON-POINTER token as ctx. Any bottom
# function that is not STRONGLY overridden silently resolves to the real weak
# production definition, which casts that token to PicoDeGallo * and
# dereferences it. -Wl,--no-undefined cannot catch this, because the weak
# definitions satisfy every reference: the link succeeds and the fake token
# reaches real FFI at run time.
#
# pdg_pwm_bottom.h declares five functions and this fake overrides all five, so
# the gate has nothing to catch TODAY. It is here for the sixth: the script
# derives its required symbol list from the declaration headers rather than
# hand-maintaining a literal copy, so a bottom function added later is covered
# without anyone remembering to update the gate. It classifies each symbol three
# ways -- absent is safe, a strong `T` is correct, and only a WEAK binding fails
# the build -- and prints a summary line on every run, so a green build is
# positive evidence the gate ran rather than the absence of evidence.
#
# ATTACH POINT. This deliberately does NOT use
# `add_custom_command(TARGET zephyr_final POST_BUILD ...)`, which is what an
# earlier revision of the UART gate did and which silently did nothing. Two
# independent reasons:
#
#  1. `zephyr_final` is created in ${ZEPHYR_BASE}'s directory scope (pulled in
#     by find_package(Zephyr) -> cmake/modules/kernel.cmake:200
#     `add_subdirectory(${ZEPHYR_BASE} ${__build_dir})`), not in this one.
#     add_custom_command(TARGET) is only supported for a target created in the
#     CURRENT directory; attaching cross-directory is not guaranteed to be
#     honoured and is dropped without diagnostic by some generators.
#  2. Even if honoured, `zephyr_final` links zephyr.ELF. The final zephyr.EXE
#     is produced later, by the native-simulator Makefile driven by the
#     `native_runner_executable` custom target
#     (zephyr/CMakeLists.txt:2103-2112, `DEPENDS ${logical_target_for_zephyr_elf}`,
#     `BYPRODUCTS ${KERNEL_EXE_NAME}`). A zephyr_final POST_BUILD therefore
#     fires BEFORE the artefact it is meant to inspect exists.
#
# So: declare our own ALL target in THIS directory and order it after
# `native_runner_executable` with add_dependencies(), which is documented to
# work across directory scopes. That target exists by now because
# find_package(Zephyr) above has already processed ${ZEPHYR_BASE}.
#
# BYPRODUCT_KERNEL_EXE_NAME is the cache variable Zephyr sets to the absolute
# path of the linked executable, right beside the same target
# (zephyr/CMakeLists.txt:2114), under the identical CONFIG_BUILD_OUTPUT_EXE
# guard -- so if the target exists, so does the variable.
# ---------------------------------------------------------------------------
if(TARGET native_runner_executable)
  if(BYPRODUCT_KERNEL_EXE_NAME)
    set(pdg_verify_exe "${BYPRODUCT_KERNEL_EXE_NAME}")
  else()
    set(pdg_verify_exe "${PROJECT_BINARY_DIR}/zephyr/zephyr.exe")
  endif()

  add_custom_target(pdg_verify_overrides ALL
    COMMAND ${CMAKE_COMMAND}
      -DPDG_NM=$<IF:$<BOOL:${CMAKE_NM}>,${CMAKE_NM},nm>
      -DPDG_EXECUTABLE=${pdg_verify_exe}
      -DPDG_HEADERS=${PDG_MODULE_ROOT}/zephyr/drivers/pwm/pdg_pwm_bottom.h$<SEMICOLON>${PDG_MODULE_ROOT}/zephyr/drivers/common/common_bottom.h
      -P ${CMAKE_CURRENT_LIST_DIR}/verify_overrides.cmake
    VERBATIM
    USES_TERMINAL
    COMMENT "Verifying Pico de Gallo bottom-layer overrides are not weak")
  add_dependencies(pdg_verify_overrides native_runner_executable)
else()
  message(WARNING
    "pdg_fake_pwm: target 'native_runner_executable' does not exist, so the "
    "bottom-layer weak-override gate could not be attached and WILL NOT RUN "
    "on this build. Nothing will verify that the fakes, rather than the weak "
    "production definitions, satisfy the pdg_*_bottom_* references.")
endif()
