#!/usr/bin/env bash
#
# Build the Pico de Gallo Zephyr module and assert per-target outcomes.
#
# BUILD ONLY. This script never runs a produced binary. It never calls
# `west build -t run`, never launches zephyr.exe, and never invokes
# tests/pdg_mfd_m5/run-m5.sh. Running a native_sim image reaches
# gallo_init_strict() in drivers/common/gallo_registry.c, which opens USB and
# needs an attached board. Linking merely resolves that symbol.
#
# A green run of this script is evidence that the module still COMPILES AND
# LINKS. It is not evidence that it still works against hardware; that remains
# the manual run-m5.sh procedure.
#
# Usage:
#   ci-build.sh [--targets a,b,c] [--build-root DIR] [--summary FILE]
#   ci-build.sh --self-test
#
# Requires ZEPHYR_BASE and ZEPHYR_TOOLCHAIN_VARIANT=host in the environment,
# except under --self-test, which touches no Zephyr workspace.
#
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT

set -u
set -o pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck disable=SC2034  # consumed by the build path added in a later commit
PDG_ROOT=$(CDPATH='' cd -- "${SCRIPT_DIR}/../.." && pwd)
# shellcheck disable=SC2034  # consumed by the build path added in a later commit
TESTDATA_DIR="${SCRIPT_DIR}/testdata"

# shellcheck disable=SC2034  # consumed by the build path added in a later commit
BOARD=native_sim/native/64
# shellcheck disable=SC2034  # consumed by the build path added in a later commit
SHIELD=pico_de_gallo

die() {
	printf 'ci-build: %s\n' "$*" >&2
	exit 1
}

#
# Target table.
#
# Fields: name|kind|srcdir|overlay|zephyr_tus|native_objs|kconfigs
#
# kind      - "pass" (must build) or "basefail" (must fail exactly at the
#             native_simulator runner link, attributable to is31fl3743b@0)
# srcdir    - repo-relative application source directory
# overlay   - repo-relative named overlay, or empty to let west pick app.overlay
# zephyr_tus  - driver translation units that MUST appear in compile_commands.json
# native_objs - native_simulator-side objects that MUST exist as build artefacts
# kconfigs  - Kconfig symbols that MUST be =y in <build>/zephyr/.config
#
# Derived from each target's overlay; see spec section 4.1. The M5 command forms
# follow docs/superpowers/specs/2026-08-19-zephyr-mfd-m5-acceptance.md:562-566.
#
PDG_TARGETS=(
"i2c_bridge|pass|zephyr/samples/i2c_bridge||pdg_mfd.c,pdg_i2c.c|common,gallo_registry,pdg_i2c_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_I2C_PICO_DE_GALLO"
"spi_nor_id|pass|zephyr/samples/spi_nor_id||pdg_mfd.c,pdg_gpio.c,pdg_spi.c|common,gallo_registry,pdg_gpio_bottom,pdg_spi_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO,CONFIG_SPI_PICO_DE_GALLO"
"spi_bridge|basefail|zephyr/samples/spi_bridge||||"
"combined_i2c_spi_bridge|basefail|zephyr/samples/combined_i2c_spi_bridge||||"
"m5_reset|pass|zephyr/tests/pdg_mfd_m5/reset_subscriptions|zephyr/tests/pdg_mfd_m5/reset_subscriptions/reset.overlay|pdg_mfd.c|common,gallo_registry,m5_bottom|CONFIG_MFD_PICO_DE_GALLO"
"m5_jumper|pass|zephyr/tests/pdg_mfd_m5/jumper_preflight|zephyr/tests/pdg_mfd_m5/jumper_preflight/jumper.overlay|pdg_mfd.c,pdg_gpio.c|common,gallo_registry,pdg_gpio_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO"
"m5_acceptance|pass|zephyr/tests/pdg_mfd_m5/acceptance|zephyr/tests/pdg_mfd_m5/acceptance/acceptance.overlay|pdg_mfd.c,pdg_gpio.c,pdg_spi.c|common,gallo_registry,pdg_gpio_bottom,pdg_spi_bottom,m5_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO,CONFIG_SPI_PICO_DE_GALLO"
"m5_teardown|pass|zephyr/tests/pdg_mfd_m5/recovery_teardown|zephyr/tests/pdg_mfd_m5/recovery_teardown/recovery.overlay|pdg_mfd.c,pdg_gpio.c,pdg_spi.c|common,gallo_registry,pdg_gpio_bottom,pdg_spi_bottom,m5_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO,CONFIG_SPI_PICO_DE_GALLO"
)

# All four driver translation units. Assertion 3 is two-sided over exactly this
# set: a target must compile the ones its overlay enables and none of the rest.
# shellcheck disable=SC2034  # consumed by the build path added in a later commit
PDG_ALL_DRIVER_TUS="pdg_mfd.c pdg_gpio.c pdg_i2c.c pdg_spi.c"

target_field() {
	printf '%s' "$1" | cut -d'|' -f"$2"
}

ST_PASS=0
ST_FAIL=0

st_check() {
	local desc=$1 actual=$2 expected=$3
	if [ "$actual" = "$expected" ]; then
		ST_PASS=$((ST_PASS + 1))
		printf '  ok   %s\n' "$desc"
	else
		ST_FAIL=$((ST_FAIL + 1))
		printf '  FAIL %s\n     expected: %s\n     actual:   %s\n' \
			"$desc" "$expected" "$actual"
	fi
}

self_test() {
	printf 'ci-build self-test\n'

	st_check "table has 8 targets" "${#PDG_TARGETS[@]}" "8"
	st_check "field 1 is the name" \
		"$(target_field "${PDG_TARGETS[0]}" 1)" "i2c_bridge"
	st_check "field 2 is the kind" \
		"$(target_field "${PDG_TARGETS[2]}" 2)" "basefail"
	st_check "empty overlay field yields empty string" \
		"$(target_field "${PDG_TARGETS[0]}" 4)" ""
	st_check "named overlay field is preserved" \
		"$(target_field "${PDG_TARGETS[4]}" 4)" \
		"zephyr/tests/pdg_mfd_m5/reset_subscriptions/reset.overlay"

	printf '\n%d passed, %d failed\n' "$ST_PASS" "$ST_FAIL"
	[ "$ST_FAIL" -eq 0 ]
}

main() {
	case "${1:-}" in
	--self-test)
		self_test
		;;
	*)
		die "not implemented yet"
		;;
	esac
}

main "$@"
