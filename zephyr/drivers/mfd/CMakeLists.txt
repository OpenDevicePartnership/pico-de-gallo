# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

# Normalized so the compile line names drivers/common directly rather than
# drivers/mfd/../common.
get_filename_component(PDG_COMMON_DIR "${CMAKE_CURRENT_LIST_DIR}/../common" ABSOLUTE)

zephyr_library()
zephyr_library_sources(pdg_mfd.c)
# Private: the parent needs the FFI-free common_bottom.h declarations.
zephyr_library_include_directories(${PDG_COMMON_DIR})
# Exported so future child drivers can include pdg_mfd.h.
zephyr_include_directories(${CMAKE_CURRENT_LIST_DIR})
