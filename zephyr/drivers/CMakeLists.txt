# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

add_subdirectory_ifdef(CONFIG_I2C_PICO_DE_GALLO i2c)
add_subdirectory_ifdef(CONFIG_SPI_PICO_DE_GALLO spi)

# common.c is compiled by the native-simulator Makefile, not by CMake, so it
# never appears in compile_commands.json and set_source_files_properties() does
# not reach it. The only channel into that compile line is native_simulator's
# INTERFACE_COMPILE_OPTIONS, which boards/native/common/natsim_config.cmake
# joins into NSI_BUILD_OPTIONS.
#
# -Werror=switch turns a Status enum member omitted from the switch in common.c
# into a build failure instead of a silent -EIO. It is load-bearing together
# with the (enum Status) cast in that switch: cbindgen emits
# "typedef int32_t Status" under C11/C17, so an uncast switch gets no -Wswitch
# coverage at all.
#
# Note this applies native-simulator-wide, not per file (verified clean today).
if(CONFIG_I2C_PICO_DE_GALLO OR CONFIG_SPI_PICO_DE_GALLO)
	target_sources(native_simulator INTERFACE
		${CMAKE_CURRENT_LIST_DIR}/common/common.c
		${CMAKE_CURRENT_LIST_DIR}/common/gallo_registry.c)
	target_compile_options(native_simulator INTERFACE
		"-I${CMAKE_CURRENT_LIST_DIR}/common"
		"-Werror=switch")
endif()
