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
	FetchContent_Declare(
		Corrosion
		URL https://github.com/corrosion-rs/corrosion/archive/refs/tags/v0.5.2.tar.gz
		URL_HASH SHA256=6bc02411e29183a896aa60c58db6819ec6cf57c08997481d0b0da9029356b529
		${_pdg_fetchcontent_timestamp_args}
	)
	FetchContent_MakeAvailable(Corrosion)

	FetchContent_Declare(
		pico_de_gallo_ffi_src
		URL https://static.crates.io/crates/pico-de-gallo-ffi/pico-de-gallo-ffi-0.7.1.crate
		URL_HASH SHA256=600148491a19fb14caad56a144ffb182ee24c0719850273aacaf77eda25cff64
		${_pdg_fetchcontent_timestamp_args}
	)
	FetchContent_MakeAvailable(pico_de_gallo_ffi_src)

	# this keeps Cargo from treating this generated dependency as a member of a
	# parent workspace when the Zephyr build directory is inside this repository.
	set(_pdg_ffi_manifest "${pico_de_gallo_ffi_src_SOURCE_DIR}/Cargo.toml")
	file(READ "${_pdg_ffi_manifest}" _pdg_ffi_manifest_contents)
	if(NOT _pdg_ffi_manifest_contents MATCHES "(^|[\r\n])[ \t]*\\[workspace\\][ \t]*([\r\n]|$)")
		file(APPEND "${_pdg_ffi_manifest}" "\n[workspace]\n")
	endif()

	get_property(_pdg_shared_libs_supported GLOBAL PROPERTY TARGET_SUPPORTS_SHARED_LIBS)
	set_property(GLOBAL PROPERTY TARGET_SUPPORTS_SHARED_LIBS TRUE)
	corrosion_import_crate(
		MANIFEST_PATH "${_pdg_ffi_manifest}"
		CRATES pico-de-gallo-ffi
		CRATE_TYPES cdylib
		PROFILE release
		LOCKED
	)
	set_property(GLOBAL PROPERTY TARGET_SUPPORTS_SHARED_LIBS "${_pdg_shared_libs_supported}")
	corrosion_set_hostbuild(pico_de_gallo_ffi)
	corrosion_experimental_cbindgen(
		TARGET pico_de_gallo_ffi
		HEADER_NAME pico_de_gallo.h
	)

	set(_pdg_ffi_output_dir "${CMAKE_CURRENT_BINARY_DIR}")
	set(_pdg_ffi_include_dir
		"${_pdg_ffi_output_dir}/corrosion_generated/cbindgen/pico_de_gallo_ffi/include")
	set(_pdg_ffi_library "${_pdg_ffi_output_dir}/libpico_de_gallo_ffi.so")
	target_compile_options(native_simulator INTERFACE "-I${_pdg_ffi_include_dir}")
	target_link_options(native_simulator INTERFACE "-Wl,-rpath,${_pdg_ffi_output_dir}")
	set_property(TARGET native_simulator APPEND PROPERTY RUNNER_LINK_LIBRARIES
		"${_pdg_ffi_library}")

	cmake_language(DEFER DIRECTORY "${CMAKE_SOURCE_DIR}" CALL add_dependencies
		native_runner_executable
		cargo-build_pico_de_gallo_ffi
		_corrosion_cbindgen_pico_de_gallo_ffi_bindings
	)
endif()

add_subdirectory(drivers)