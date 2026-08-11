# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

if(CONFIG_PICO_DE_GALLO)
	if(POLICY CMP0116)
		cmake_policy(SET CMP0116 NEW)

		# added this because it seems like Corrosion needs it. without it CMake gives a warning when you build
		set(CMAKE_POLICY_DEFAULT_CMP0116 NEW)
	endif()

	find_program(PDG_CARGO_EXECUTABLE NAMES cargo)
	find_program(PDG_RUSTC_EXECUTABLE NAMES rustc)
	mark_as_advanced(PDG_CARGO_EXECUTABLE PDG_RUSTC_EXECUTABLE)

	if(NOT PDG_CARGO_EXECUTABLE OR NOT PDG_RUSTC_EXECUTABLE)
		message(FATAL_ERROR
			"Pico de Gallo host drivers require Cargo and Rust 1.90 or newer. "
			"Install a Rust toolchain from https://rustup.rs/ and re-run CMake.")
	endif()

	if(NOT Rust_CARGO_TARGET)
		execute_process(
			COMMAND "${PDG_RUSTC_EXECUTABLE}" --version --verbose
			OUTPUT_VARIABLE _pdg_rustc_version
			RESULT_VARIABLE _pdg_rustc_result
			OUTPUT_STRIP_TRAILING_WHITESPACE
		)
		string(REGEX MATCH "host: ([^\r\n]+)" _pdg_rustc_host "${_pdg_rustc_version}")
		if(NOT _pdg_rustc_result EQUAL 0 OR NOT CMAKE_MATCH_1)
			message(FATAL_ERROR "Unable to determine the Rust host target from rustc.")
		endif()
		set(Rust_CARGO_TARGET "${CMAKE_MATCH_1}" CACHE STRING
			"Rust target for Pico de Gallo native simulator host libraries")
	endif()

	include(FetchContent)
	set(_pdg_fetchcontent_timestamp_args)
	if(CMAKE_VERSION VERSION_GREATER_EQUAL 3.24)
		list(APPEND _pdg_fetchcontent_timestamp_args DOWNLOAD_EXTRACT_TIMESTAMP TRUE)
	endif()

	# using Corrosion 0.5 because it supports CMake 3.20 (which is Zephyr's baseline CMake version). Corrosion 0.6 requires 3.22 so
	# it can't be used here
	#
	# For offline or air-gapped builds, point FetchContent at a local
	# Corrosion checkout instead of the GitHub tarball:
	#   cmake -DFETCHCONTENT_SOURCE_DIR_CORROSION=/path/to/corrosion ...
	FetchContent_Declare(
		Corrosion
		URL https://github.com/corrosion-rs/corrosion/archive/refs/tags/v0.5.2.tar.gz
		URL_HASH SHA256=6bc02411e29183a896aa60c58db6819ec6cf57c08997481d0b0da9029356b529
		${_pdg_fetchcontent_timestamp_args}
	)
	FetchContent_MakeAvailable(Corrosion)

	# Build the FFI from this repository rather than crates.io, so that
	# edits to crates/pico-de-gallo-ffi/ are picked up by the Zephyr build
	# and so the pin stays visible to cargo-deny, cargo-semver-checks and
	# CI's lockfile job. Also makes offline configure possible.
	#
	# staticlib rather than cdylib: the native_simulator runner then carries
	# the FFI inside the executable, so there is no -Wl,-rpath to get right,
	# no shared object to keep alongside zephyr.exe, and no need to force
	# TARGET_SUPPORTS_SHARED_LIBS on for Zephyr's toolchain files.
	corrosion_import_crate(
		MANIFEST_PATH "${ZEPHYR_PICO_DE_GALLO_MODULE_DIR}/crates/pico-de-gallo-ffi/Cargo.toml"
		CRATES pico-de-gallo-ffi
		CRATE_TYPES staticlib
		PROFILE release
		LOCKED
	)
	corrosion_set_hostbuild(pico_de_gallo_ffi)
	corrosion_experimental_cbindgen(
		TARGET pico_de_gallo_ffi
		HEADER_NAME pico_de_gallo.h
	)

	set(_pdg_ffi_output_dir "${CMAKE_CURRENT_BINARY_DIR}")
	set(_pdg_ffi_include_dir
		"${_pdg_ffi_output_dir}/corrosion_generated/cbindgen/pico_de_gallo_ffi/include")
	# Derive the artifact name from CMake's platform conventions instead of
	# hardcoding an ELF archive: libpico_de_gallo_ffi.a on Linux and macOS,
	# pico_de_gallo_ffi.lib on Windows. macOS is a supported native_sim host,
	# and the previous hardcoded libpico_de_gallo_ffi.so named a file that
	# never exists there.
	#
	# $<TARGET_FILE:pico_de_gallo_ffi> would be the obvious spelling but does
	# not work here, for two independent reasons: Corrosion's target is not an
	# executable or library target that TARGET_FILE accepts, and generator
	# expressions are not recursive, so the $<TARGET_PROPERTY:> read of
	# RUNNER_LINK_LIBRARIES in boards/native/common/natsim_config.cmake would
	# emit the genex verbatim rather than evaluating it.
	set(_pdg_ffi_library
		"${_pdg_ffi_output_dir}/${CMAKE_STATIC_LIBRARY_PREFIX}pico_de_gallo_ffi${CMAKE_STATIC_LIBRARY_SUFFIX}")
	target_compile_options(native_simulator INTERFACE "-I${_pdg_ffi_include_dir}")
	set_property(TARGET native_simulator APPEND PROPERTY RUNNER_LINK_LIBRARIES
		"${_pdg_ffi_library}")

	cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL add_dependencies
		native_runner_executable
		cargo-build_pico_de_gallo_ffi
		_corrosion_cbindgen_pico_de_gallo_ffi_bindings
	)
endif()

add_subdirectory(drivers)