# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT
#
# SPIKE -- throwaway. See main.c.

cmake_minimum_required(VERSION 3.28.0)

find_package(Zephyr COMPONENTS unittest REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(pdg_i2c_msgs)

get_filename_component(PDG_MODULE_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../../.." ABSOLUTE)

target_sources(testbinary PRIVATE main.c)

# pdg_i2c.c includes "pdg_i2c_bottom.h" and, after its parent assertions,
# "pdg_mfd.h". Neither is on the default unit_testing include path.
target_include_directories(testbinary PRIVATE
	${PDG_MODULE_ROOT}/zephyr/drivers/i2c
	${PDG_MODULE_ROOT}/zephyr/drivers/mfd
	${PDG_MODULE_ROOT}/zephyr/drivers/common)
