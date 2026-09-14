# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

# Top half: embedded (Zephyr) context. Sees Zephyr headers only; it must never
# include pico_de_gallo.h.
zephyr_library()
zephyr_library_sources(pdg_pwm.c)
zephyr_library_include_directories(${CMAKE_CURRENT_LIST_DIR})

# Bottom half: native simulator (host) context. Sees the host libc and the
# generated pico_de_gallo.h; it must never include a Zephyr header.
#
# No target_include_directories(native_simulator INTERFACE ...) here.
# pdg_pwm_bottom.c quotes pdg_pwm_bottom.h from its own directory, which the
# preprocessor resolves relative to the including file, so no -I is needed. If
# a cross-directory header is ever required on the native side it must be
# supplied with target_compile_options(native_simulator INTERFACE "-I<dir>"),
# because natsim_config.cmake joins only INTERFACE_COMPILE_OPTIONS into
# NSI_BUILD_OPTIONS and the include-directories form silently vanishes.
target_sources(native_simulator INTERFACE ${CMAKE_CURRENT_LIST_DIR}/pdg_pwm_bottom.c)
