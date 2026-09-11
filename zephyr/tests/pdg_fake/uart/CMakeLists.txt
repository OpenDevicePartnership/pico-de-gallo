# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

cmake_minimum_required(VERSION 3.20.0)

set(BOARD native_sim/native/64 CACHE STRING "Default board for Pico de Gallo tests")
set(SHIELD pico_de_gallo CACHE STRING "Default shield for Pico de Gallo tests")
get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)
list(APPEND EXTRA_ZEPHYR_MODULES "${PDG_MODULE_ROOT}")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_fake_uart)

#
# Compile-time capability policy.
#
# PDG_FAKE_UART_CAPABILITY selects what pdg_uart_bottom_has_uart() reports:
# 1 (default) is the healthy runtime suite, 0 is the capability-refusal suite.
#
# It MUST be compile-time. pdg_uart_init() reads the capability during
# POST_KERNEL device initialization, which happens before any ztest setup hook
# runs, so a runtime setter could never influence the gate it is supposed to
# select -- by the time a test could call it, every child has already been
# accepted or refused.
#
set(PDG_FAKE_UART_CAPABILITY 1 CACHE STRING
  "Value pdg_uart_bottom_has_uart() reports from the fake: 1 (has UART) or 0")

if(NOT PDG_FAKE_UART_CAPABILITY STREQUAL "0" AND
   NOT PDG_FAKE_UART_CAPABILITY STREQUAL "1")
  message(FATAL_ERROR
    "PDG_FAKE_UART_CAPABILITY must be exactly 0 or 1, got "
    "'${PDG_FAKE_UART_CAPABILITY}'.")
endif()

target_sources(app PRIVATE src/main.c)

# The embedded side calls the accessors of BOTH fakes, so it needs the shared
# parent fake's FFI-free header and this directory's UART fake header.
target_include_directories(app PRIVATE
  ${CMAKE_CURRENT_LIST_DIR}/../common
  ${CMAKE_CURRENT_LIST_DIR})

# The embedded half must agree with the host half about the policy bit, so the
# test can assert the behaviour the fake was built to produce.
target_compile_definitions(app PRIVATE
  PDG_FAKE_UART_CAPABILITY=${PDG_FAKE_UART_CAPABILITY})

# The fakes themselves run in the host context and provide STRONG definitions
# that override the weak ones in drivers/common/common.c and
# drivers/serial/pdg_uart_bottom.c. Same mechanism as
# zephyr/drivers/CMakeLists.txt.
#
# Both halves are needed: the shared parent fake supplies
# pdg_common_bottom_open()/_close(), which keeps pdg_mfd_init() out of
# gallo_init_strict(); the UART fake supplies the four pdg_uart_bottom_*
# definitions the UART children reach.
target_sources(native_simulator INTERFACE
  ${CMAKE_CURRENT_LIST_DIR}/../common/pdg_fake_bottom.c
  ${CMAKE_CURRENT_LIST_DIR}/pdg_uart_fake_bottom.c)

# Header search path and policy macro for the fakes. They include the
# production declaration sites of every symbol they override -- included rather
# than hand-copied so a signature drift is a compile error rather than
# undefined behaviour that links cleanly. Every one of those headers is FFI-free
# (stdbool/stdint/stddef only), so none of this drags pico_de_gallo.h in.
#
# This MUST be target_compile_options("-I...") and NOT the natural-looking
# target_include_directories(native_simulator INTERFACE ...). The fakes are
# compiled by the native-simulator Makefile, not by CMake, and
# boards/native/common/natsim_config.cmake joins only INTERFACE_COMPILE_OPTIONS
# into NSI_BUILD_OPTIONS -- INTERFACE_INCLUDE_DIRECTORIES is never consumed and
# the flag silently vanishes. Same channel, and same reason, as
# zephyr/drivers/CMakeLists.txt:34-36. Do not "tidy" this back; it has already
# cost one red CI run (33106107101).
#
# The -D travels the same channel for the same reason: a
# target_compile_definitions() on native_simulator would never reach the fake's
# compile line either, and the host half would then silently fall back to
# whatever default the fake source hard-codes.
#
# Two things look like counter-evidence and are not. zephyr/drivers/serial/
# CMakeLists.txt uses the include-directories form, but pdg_uart_bottom.c and
# pdg_uart_bottom.h sit in the same directory, so quoted-include lookup finds
# the header with no -I at all. And common_bottom.h resolves in the drivers
# build only because zephyr/drivers/CMakeLists.txt injects that directory with
# this same -I mechanism.
#
# drivers/i2c is on the list because the SHARED parent fake includes
# pdg_i2c_bottom.h; this suite enables no I2C child, but the fake is one
# translation unit serving both suites.
target_compile_options(native_simulator INTERFACE
  "-I${CMAKE_CURRENT_LIST_DIR}/../common"
  "-I${CMAKE_CURRENT_LIST_DIR}"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/common"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/i2c"
  "-I${PDG_MODULE_ROOT}/zephyr/drivers/serial"
  "-DPDG_FAKE_UART_CAPABILITY=${PDG_FAKE_UART_CAPABILITY}")

# ---------------------------------------------------------------------------
# BEST-EFFORT post-link symbol gate. SELF-CONTAINED AND EASILY REMOVED: delete
# this whole block and verify_overrides.cmake, and nothing else changes.
#
# The fakes hand the driver an opaque NON-POINTER token as ctx. Any bottom
# function that is not STRONGLY overridden silently resolves to the real weak
# production definition, which casts that token to PicoDeGallo * and
# dereferences it. -Wl,--no-undefined cannot catch this, because the weak
# definitions satisfy every reference: the link succeeds and the fake token
# reaches real FFI at run time.
#
# The script derives its required symbol list from the two declaration headers
# rather than hand-maintaining a literal copy, so a bottom function added later
# is covered automatically. It classifies each symbol three ways -- absent is
# safe, a strong `T` is correct, and only a WEAK binding fails the build -- and
# prints a summary line on every run, so a green build is positive evidence the
# gate ran rather than the absence of evidence.
#
# ATTACH POINT. This deliberately does NOT use
# `add_custom_command(TARGET zephyr_final POST_BUILD ...)`, which is what an
# earlier revision did and which silently did nothing. Two independent reasons:
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
      -DPDG_HEADERS=${PDG_MODULE_ROOT}/zephyr/drivers/serial/pdg_uart_bottom.h$<SEMICOLON>${PDG_MODULE_ROOT}/zephyr/drivers/common/common_bottom.h
      -P ${CMAKE_CURRENT_LIST_DIR}/verify_overrides.cmake
    VERBATIM
    USES_TERMINAL
    COMMENT "Verifying Pico de Gallo bottom-layer overrides are not weak")
  add_dependencies(pdg_verify_overrides native_runner_executable)
else()
  message(WARNING
    "pdg_fake_uart: target 'native_runner_executable' does not exist, so the "
    "bottom-layer weak-override gate could not be attached and WILL NOT RUN "
    "on this build. Nothing will verify that the fakes, rather than the weak "
    "production definitions, satisfy the pdg_*_bottom_* references.")
endif()
