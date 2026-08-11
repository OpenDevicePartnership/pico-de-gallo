# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

add_subdirectory_ifdef(CONFIG_I2C_PICO_DE_GALLO i2c)
add_subdirectory_ifdef(CONFIG_SPI_PICO_DE_GALLO spi)

if(CONFIG_I2C_PICO_DE_GALLO OR CONFIG_SPI_PICO_DE_GALLO)
	target_sources(native_simulator INTERFACE
		${CMAKE_CURRENT_LIST_DIR}/common/common.c
		${CMAKE_CURRENT_LIST_DIR}/common/gallo_registry.c)
	target_compile_options(native_simulator INTERFACE
		"-I${CMAKE_CURRENT_LIST_DIR}/common")
endif()
