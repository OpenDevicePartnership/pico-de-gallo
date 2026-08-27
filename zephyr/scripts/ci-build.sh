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
PDG_ROOT=$(CDPATH='' cd -- "${SCRIPT_DIR}/../.." && pwd)
TESTDATA_DIR="${SCRIPT_DIR}/testdata"

BOARD=native_sim/native/64
SHIELD=pico_de_gallo

BUILD_ROOT=${PDG_CI_BUILD_ROOT:-/tmp/pdg-ci}
SUMMARY_FILE=${PDG_CI_SUMMARY:-}

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
# drivers/common/common.c is DELIBERATELY ABSENT from every native_objs field.
# Do not add it back. Its object would be matched by name alone, and "common" is
# generic enough that an unrelated object anywhere in a Zephyr build tree could
# satisfy the search — a vacuous pass, which is the one outcome this gate exists
# to prevent. It costs no coverage: zephyr/drivers/CMakeLists.txt adds common.c
# and gallo_registry.c to native_simulator in a single target_sources() call
# under one conditional, so finding the uniquely named gallo_registry object
# proves common.c was compiled too. Every remaining entry is likewise unique.
#
PDG_TARGETS=(
"i2c_bridge|pass|zephyr/samples/i2c_bridge||pdg_mfd.c,pdg_i2c.c|gallo_registry,pdg_i2c_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_I2C_PICO_DE_GALLO"
"spi_nor_id|pass|zephyr/samples/spi_nor_id||pdg_mfd.c,pdg_gpio.c,pdg_spi.c|gallo_registry,pdg_gpio_bottom,pdg_spi_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO,CONFIG_SPI_PICO_DE_GALLO"
"spi_bridge|basefail|zephyr/samples/spi_bridge||||"
"combined_i2c_spi_bridge|basefail|zephyr/samples/combined_i2c_spi_bridge||||"
"m5_reset|pass|zephyr/tests/pdg_mfd_m5/reset_subscriptions|zephyr/tests/pdg_mfd_m5/reset_subscriptions/reset.overlay|pdg_mfd.c|gallo_registry,m5_bottom|CONFIG_MFD_PICO_DE_GALLO"
"m5_jumper|pass|zephyr/tests/pdg_mfd_m5/jumper_preflight|zephyr/tests/pdg_mfd_m5/jumper_preflight/jumper.overlay|pdg_mfd.c,pdg_gpio.c|gallo_registry,pdg_gpio_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO"
"m5_acceptance|pass|zephyr/tests/pdg_mfd_m5/acceptance|zephyr/tests/pdg_mfd_m5/acceptance/acceptance.overlay|pdg_mfd.c,pdg_gpio.c,pdg_spi.c|gallo_registry,pdg_gpio_bottom,pdg_spi_bottom,m5_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO,CONFIG_SPI_PICO_DE_GALLO"
"m5_teardown|pass|zephyr/tests/pdg_mfd_m5/recovery_teardown|zephyr/tests/pdg_mfd_m5/recovery_teardown/recovery.overlay|pdg_mfd.c,pdg_gpio.c,pdg_spi.c|gallo_registry,pdg_gpio_bottom,pdg_spi_bottom,m5_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_GPIO_PICO_DE_GALLO,CONFIG_SPI_PICO_DE_GALLO"
"i2c_burst|pass|zephyr/tests/pdg_i2c_burst|zephyr/tests/pdg_i2c_burst/burst.overlay|pdg_mfd.c,pdg_i2c.c|gallo_registry,pdg_i2c_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_I2C_PICO_DE_GALLO"
)

# All four driver translation units. Assertion 3 is two-sided over exactly this
# set: a target must compile the ones its overlay enables and none of the rest.
PDG_ALL_DRIVER_TUS="pdg_mfd.c pdg_gpio.c pdg_i2c.c pdg_spi.c"

# The Kconfig symbol of each of those four drivers, in the same order. Assertion
# 5 is two-sided over exactly this set. The translation-unit check above matches
# substrings in compile_commands.json and could in principle be fooled; a
# Kconfig file is an exact key=value store and cannot be, so the same gap is
# closed a second time by a mechanism that does not depend on string matching.
PDG_ALL_DRIVER_KCONFIGS="CONFIG_MFD_PICO_DE_GALLO CONFIG_GPIO_PICO_DE_GALLO CONFIG_I2C_PICO_DE_GALLO CONFIG_SPI_PICO_DE_GALLO"

target_field() {
	printf '%s' "$1" | cut -d'|' -f"$2"
}

#
# Echo every target name in table order, one per line.
#
# target_field ends in a newline of cut's own making, so nothing is added here.
#
target_names() {
	local record
	for record in "${PDG_TARGETS[@]}"; do
		target_field "$record" 1
	done
}

#
# Echo the names in a comma-separated selection that match no table record,
# space-separated. Empty output means every name is valid.
#
# A typo in --targets used to be silently skipped, so a workflow that asked for
# a target that does not exist ran zero builds and still reported success. That
# is the single worst outcome this gate can produce, so the caller turns any
# output from this function into a hard error.
#
unknown_targets() {
	local selected=$1
	[ -n "$selected" ] || return 0
	local known name out=""
	known=" $(target_names | tr '\n' ' ')"
	for name in $(printf '%s' "$selected" | tr ',' ' '); do
		case "$known" in
		*" ${name} "*) ;;
		*) out="${out}${name} " ;;
		esac
	done
	printf '%s' "$out" | sed 's/ *$//'
}

#
# Echo the target names a selection resolves to, in table order,
# space-separated. An empty selection means all of them.
#
select_targets() {
	local selected=$1
	if [ -z "$selected" ]; then
		target_names | tr '\n' ' ' | sed 's/ *$//'
		return 0
	fi
	local name out=""
	for name in $(target_names); do
		case ",${selected}," in
		*",${name},"*) out="${out}${name} " ;;
		esac
	done
	printf '%s' "$out" | sed 's/ *$//'
}

#
# Extract the distinct ordinals of every undefined __device_dts_ord_N in a
# build log, sorted, space-separated. Empty output means none.
#
# The idiom is M3's, from 2026-08-17-zephyr-mfd-m3-gpio-tests.md.
#
undefined_ords() {
	local log=$1
	[ -f "$log" ] || die "no such log: $log"
	grep -o '__device_dts_ord_[0-9]*' "$log" \
		| sed 's/.*_//' \
		| sort -un \
		| tr '\n' ' ' \
		| sed 's/ *$//'
}

#
# Echo the names of generated defines whose value is exactly <ordinal>.
#
# The trailing anchor is load-bearing. Zephyr emits both
#
#     #define DT_N_..._ORD 49
#     #define DT_N_..._ORD_STR_SORTABLE "00049"
#
# and only the first has the bare ordinal as its value. Anchoring on
# "ORD <n>" at end of line selects it and rejects the sibling, and also
# prevents 4 from matching the 49 line.
#
resolve_ord_defines() {
	local dtheader=$1 ordinal=$2
	[ -f "$dtheader" ] || die "no such devicetree header: $dtheader"
	grep -E "^#define (DT_N_[A-Za-z0-9_]*_ORD) ${ordinal}\$" "$dtheader" \
		| awk '{print $2}'
}

#
# Echo the sorted unique pdg_*.c translation units named in a compile database.
#
# The idiom is M4 A-01's, from 2026-08-19-zephyr-mfd-m4-acceptance.md.
#
tu_set() {
	local ccjson=$1
	[ -f "$ccjson" ] || die "no such compile database: $ccjson"
	grep -o 'pdg_[a-z0-9_]*\.c' "$ccjson" \
		| sort -u \
		| tr '\n' ' ' \
		| sed 's/ *$//'
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

	st_check "table has 9 targets" "${#PDG_TARGETS[@]}" "9"
	st_check "field 1 is the name" \
		"$(target_field "${PDG_TARGETS[0]}" 1)" "i2c_bridge"
	st_check "field 2 is the kind" \
		"$(target_field "${PDG_TARGETS[2]}" 2)" "basefail"
	st_check "empty overlay field yields empty string" \
		"$(target_field "${PDG_TARGETS[0]}" 4)" ""
	st_check "named overlay field is preserved" \
		"$(target_field "${PDG_TARGETS[4]}" 4)" \
		"zephyr/tests/pdg_mfd_m5/reset_subscriptions/reset.overlay"

	# --- undefined_ords ---
	st_check "undefined_ords finds the sole ordinal, deduplicated" \
		"$(undefined_ords "${TESTDATA_DIR}/undefined-ord.log")" "49"
	st_check "undefined_ords is empty for a clean log" \
		"$(undefined_ords "${TESTDATA_DIR}/compile_commands.json")" ""

	# --- resolve_ord_defines ---
	st_check "resolve_ord_defines maps 49 to exactly one define" \
		"$(resolve_ord_defines "${TESTDATA_DIR}/devicetree_generated.h" 49)" \
		"DT_N_S_pico_de_gallo_S_spi_S_is31fl3743b_0_ORD"
	st_check "resolve_ord_defines ignores the _ORD_STR_SORTABLE sibling" \
		"$(resolve_ord_defines "${TESTDATA_DIR}/devicetree_generated.h" 49 | wc -l | tr -d ' ')" \
		"1"
	st_check "resolve_ord_defines does not prefix-match 4 against 49" \
		"$(resolve_ord_defines "${TESTDATA_DIR}/devicetree_generated.h" 4)" ""
	st_check "resolve_ord_defines resolves a different node" \
		"$(resolve_ord_defines "${TESTDATA_DIR}/devicetree_generated.h" 50)" \
		"DT_N_S_pico_de_gallo_S_i2c_S_tmp117_48_ORD"

	# --- tu_set ---
	st_check "tu_set extracts sorted unique driver translation units" \
		"$(tu_set "${TESTDATA_DIR}/compile_commands.json")" \
		"pdg_gpio.c pdg_mfd.c pdg_spi.c"

	# --- selection ---
	st_check "unknown_targets reports a typo" \
		"$(unknown_targets "typo")" "typo"
	st_check "unknown_targets reports the bad half of a mixed list" \
		"$(unknown_targets "i2c_bridge,typo")" "typo"
	st_check "unknown_targets accepts an all-valid list" \
		"$(unknown_targets "i2c_bridge,m5_jumper")" ""
	st_check "select_targets with an empty selection means all nine" \
		"$(select_targets "")" \
		"i2c_bridge spi_nor_id spi_bridge combined_i2c_spi_bridge m5_reset m5_jumper m5_acceptance m5_teardown i2c_burst"
	st_check "select_targets picks exactly the named subset, in table order" \
		"$(select_targets "m5_jumper,i2c_bridge")" "i2c_bridge m5_jumper"

	printf '\n%d passed, %d failed\n' "$ST_PASS" "$ST_FAIL"
	[ "$ST_FAIL" -eq 0 ]
}

require_env() {
	[ -n "${ZEPHYR_BASE:-}" ] || die "ZEPHYR_BASE is not set"
	[ -d "${ZEPHYR_BASE}" ] || die "ZEPHYR_BASE is not a directory: ${ZEPHYR_BASE}"
	[ "${ZEPHYR_TOOLCHAIN_VARIANT:-}" = "host" ] \
		|| die "ZEPHYR_TOOLCHAIN_VARIANT must be 'host', got '${ZEPHYR_TOOLCHAIN_VARIANT:-}'"
	command -v west >/dev/null 2>&1 || die "west is not on PATH"
	command -v cargo >/dev/null 2>&1 || die "cargo is not on PATH"
}

#
# Run one west build. Never passes a run target: see the header.
#
build_target() {
	local record=$1
	local name srcdir overlay builddir log
	name=$(target_field "$record" 1)
	srcdir=$(target_field "$record" 3)
	overlay=$(target_field "$record" 4)
	builddir="${BUILD_ROOT}/${name}"
	log="${BUILD_ROOT}/${name}.log"

	local args
	args=(-p always -d "$builddir" -b "$BOARD" "${PDG_ROOT}/${srcdir}"
	      -- "-DSHIELD=${SHIELD}" "-DEXTRA_ZEPHYR_MODULES=${PDG_ROOT}")
	if [ -n "$overlay" ]; then
		args+=("-DDTC_OVERLAY_FILE=${PDG_ROOT}/${overlay}")
	fi

	west build "${args[@]}" >"$log" 2>&1
}

#
# Spec section 5.1.
#
assert_pass() {
	local record=$1 builddir=$2 status=$3
	local name expected_tus expected_objs expected_kconfigs
	name=$(target_field "$record" 1)
	expected_tus=$(target_field "$record" 5)
	expected_objs=$(target_field "$record" 6)
	expected_kconfigs=$(target_field "$record" 7)
	local rc=0

	if [ "$status" -ne 0 ]; then
		printf '  %s: west build exited %d, expected 0\n' "$name" "$status"
		return 1
	fi

	# 2. Corrosion produced the static library.
	#
	# -print -quit rather than a pipe into grep -q: an early-closing consumer
	# can SIGPIPE find once more than one artefact matches, and under
	# set -o pipefail that turns a good build into an intermittent, timing
	# dependent false failure.
	#
	# Output wins over status. GNU find still reports the status it would
	# otherwise have returned, so an unreadable subtree traversed BEFORE the
	# match yields status 1 with the artefact correctly printed; failing on
	# that would be a false failure on a good tree. A non-zero status only
	# means anything when nothing was printed, and then it is a different
	# fault from a genuinely absent artefact, so it gets its own diagnostic:
	# the CI log is the only debugging surface this gate has.
	local libhit libst
	libhit=$(find "$builddir" -name 'libpico_de_gallo_ffi.a' -print -quit)
	libst=$?
	if [ -n "$libhit" ]; then
		: # found; traversal errors elsewhere in the tree do not matter
	elif [ "$libst" -ne 0 ]; then
		printf '  %s: search for libpico_de_gallo_ffi.a under %s failed (find exited %d); the build tree was not fully inspected\n' \
			"$name" "$builddir" "$libst"
		rc=1
	else
		printf '  %s: libpico_de_gallo_ffi.a not found under %s\n' "$name" "$builddir"
		rc=1
	fi

	# 3. Two-sided translation-unit check over the four driver units.
	#
	# tu_set is called inside a command substitution deliberately: it calls
	# die on a missing compile database, and inside $(...) only the subshell
	# exits, so a missing artefact degrades this one target to FAIL instead
	# of aborting the whole gate.
	local actual_tus
	actual_tus=$(tu_set "${builddir}/compile_commands.json")
	local tu
	for tu in $(printf '%s' "$expected_tus" | tr ',' ' '); do
		case " ${actual_tus} " in
		*" ${tu} "*) ;;
		*)
			printf '  %s: expected translation unit %s absent (got: %s)\n' \
				"$name" "$tu" "$actual_tus"
			rc=1
			;;
		esac
	done
	for tu in $PDG_ALL_DRIVER_TUS; do
		case ",${expected_tus}," in
		*",${tu},"*) continue ;;
		esac
		case " ${actual_tus} " in
		*" ${tu} "*)
			printf '  %s: unexpected translation unit %s compiled\n' "$name" "$tu"
			rc=1
			;;
		esac
	done

	# 4. native_simulator-side objects. Same -print -quit shape and same
	# output-wins-over-status rule as assertion 2.
	#
	# Three name shapes are accepted. The native_simulator runner is built by
	# a plain Makefile whose rule is %.c -> %.o, so the real artefact is
	# <name>.o, not CMake's <name>.c.o; the '<name>*.o' form is the one
	# A-08 of 2026-08-19-zephyr-mfd-m4-acceptance.md used to locate these
	# very objects, and its '*' absorbs any suffix decoration. The two CMake
	# shapes are kept because they cost nothing and would cover a future
	# build-system change.
	#
	# On a miss, list what IS there under that base name (capped): the first
	# run of this gate cost a full CI round precisely because "not found"
	# reported the absence without reporting the truth beside it.
	local obj objhit objst cands
	for obj in $(printf '%s' "$expected_objs" | tr ',' ' '); do
		objhit=$(find "$builddir" \
			\( -name "${obj}*.o" -o -name "${obj}.c.o" \
			   -o -name "${obj}.c.obj" \) -print -quit)
		objst=$?
		if [ -n "$objhit" ]; then
			continue
		fi
		cands=$(find "$builddir" -name "*${obj}*" -type f 2>/dev/null \
			| sed -n '1,5p')
		if [ "$objst" -ne 0 ]; then
			printf '  %s: search for %s object under %s failed (find exited %d); the build tree was not fully inspected\n' \
				"$name" "$obj" "$builddir" "$objst"
		else
			printf '  %s: native_simulator object %s.o/.c.o[bj] not found\n' "$name" "$obj"
		fi
		if [ -n "$cands" ]; then
			printf '  %s: candidates matching *%s* (first 5):\n' "$name" "$obj"
			printf '%s\n' "$cands" | sed 's/^/    /'
		else
			printf '  %s: no file under %s has %s in its name\n' \
				"$name" "$builddir" "$obj"
		fi
		rc=1
	done

	# 5. Two-sided Kconfig check: every expected symbol is =y, and every
	# driver symbol this target did NOT ask for is not =y. The negative half
	# is the one a substring match cannot fake.
	#
	# A missing .config is reported once. Letting the loops run would emit a
	# filesystem error per grep and then claim each symbol individually was
	# not enabled, which is true but describes the wrong fault.
	local config="${builddir}/zephyr/.config"
	if [ ! -f "$config" ]; then
		printf '  %s: %s does not exist, so no Kconfig symbol could be checked\n' \
			"$name" "$config"
		return 1
	fi
	local sym
	for sym in $(printf '%s' "$expected_kconfigs" | tr ',' ' '); do
		if ! grep -qx "${sym}=y" "$config"; then
			printf '  %s: %s is not =y in the build .config\n' "$name" "$sym"
			rc=1
		fi
	done
	for sym in $PDG_ALL_DRIVER_KCONFIGS; do
		case ",${expected_kconfigs}," in
		*",${sym},"*) continue ;;
		esac
		if grep -qx "${sym}=y" "$config"; then
			printf '  %s: %s is =y but was not expected for this target\n' \
				"$name" "$sym"
			rc=1
		fi
	done

	return $rc
}

#
# Spec section 5.2.
#
assert_basefail() {
	local record=$1 builddir=$2 status=$3 log=$4
	local name
	name=$(target_field "$record" 1)
	local rc=0

	# 1. It must fail. Success means the IS31 driver landed upstream.
	if [ "$status" -eq 0 ]; then
		printf '  %s: west build SUCCEEDED, expected the baseline failure.\n' "$name"
		printf '  %s: if issi,is31fl3743b reached upstream Zephyr, move this\n' "$name"
		printf '  %s: target to kind=pass and update zephyr/README.md.\n' "$name"
		return 1
	fi

	# 2. The ELF link succeeded; only the runner link failed.
	if [ ! -f "${builddir}/zephyr/zephyr.elf" ]; then
		printf '  %s: zephyr.elf absent, so the build failed earlier than the runner link\n' "$name"
		rc=1
	fi

	# 3. Exactly one distinct undefined ordinal. Command substitution keeps a
	# missing log local to this target: see the note in assert_pass.
	local ords count
	ords=$(undefined_ords "$log")
	count=$(printf '%s' "$ords" | wc -w | tr -d ' ')
	if [ "$count" -ne 1 ]; then
		printf '  %s: expected exactly 1 undefined __device_dts_ord_N, got %s (%s)\n' \
			"$name" "$count" "$ords"
		return 1
	fi

	# 4. That ordinal resolves to the is31fl3743b node in THIS build.
	local dtheader defines
	dtheader="${builddir}/zephyr/include/generated/zephyr/devicetree_generated.h"
	defines=$(resolve_ord_defines "$dtheader" "$ords")
	if [ -z "$defines" ]; then
		printf '  %s: ordinal %s resolves to no define in %s\n' "$name" "$ords" "$dtheader"
		return 1
	fi
	case "$defines" in
	*is31fl3743b*) ;;
	*)
		printf '  %s: ordinal %s resolves to %s, which is not an is31fl3743b node\n' \
			"$name" "$ords" "$defines"
		rc=1
		;;
	esac

	return $rc
}

usage() {
	sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
}

main() {
	local selected=""
	while [ $# -gt 0 ]; do
		case $1 in
		--self-test) self_test; return $? ;;
		--targets) selected=$2; shift 2 ;;
		--build-root) BUILD_ROOT=$2; shift 2 ;;
		--summary) SUMMARY_FILE=$2; shift 2 ;;
		-h|--help) usage; return 0 ;;
		*) die "unknown argument: $1" ;;
		esac
	done

	# Validate the selection BEFORE require_env, so a typo is reported as a
	# typo rather than hidden behind a missing-toolchain complaint.
	local unknown selection
	unknown=$(unknown_targets "$selected")
	if [ -n "$unknown" ]; then
		die "unknown target(s): ${unknown} (valid: $(select_targets ''))"
	fi
	selection=$(select_targets "$selected")
	[ -n "$selection" ] || die "no targets selected"

	require_env
	mkdir -p "$BUILD_ROOT"

	local ran=0 failures=0 results=""
	local record name kind builddir log status verdict
	for record in "${PDG_TARGETS[@]}"; do
		name=$(target_field "$record" 1)
		kind=$(target_field "$record" 2)

		case " ${selection} " in
		*" ${name} "*) ;;
		*) continue ;;
		esac
		ran=$((ran + 1))

		builddir="${BUILD_ROOT}/${name}"
		log="${BUILD_ROOT}/${name}.log"

		printf '::group::build %s (%s)\n' "$name" "$kind"
		build_target "$record"
		status=$?
		cat "$log"
		printf '::endgroup::\n'

		if [ "$kind" = pass ]; then
			assert_pass "$record" "$builddir" "$status" && verdict=PASS || verdict=FAIL
		else
			assert_basefail "$record" "$builddir" "$status" "$log" && verdict=PASS || verdict=FAIL
		fi
		if [ "$verdict" = FAIL ]; then
			failures=$((failures + 1))
		fi
		printf '%s %s (%s)\n' "$verdict" "$name" "$kind"
		results="${results}| \`${name}\` | ${kind} | ${verdict} |"$'\n'
	done

	if [ -n "$SUMMARY_FILE" ]; then
		{
			printf '## Zephyr build gate\n\n'
			printf 'Build-only. No produced binary was executed.\n\n'
			printf '| Target | Expected | Result |\n|---|---|---|\n'
			printf '%s' "$results"
		} >>"$SUMMARY_FILE"
	fi

	# Belt and braces: the selection was non-empty, so this cannot fire unless
	# the loop above grows a new skip path. Success must never be printed for
	# a run that built nothing.
	[ "$ran" -gt 0 ] || die "no targets ran"
	[ "$failures" -eq 0 ] || die "${failures} target(s) did not meet their contract"
	printf 'all %d selected target(s) met their contract\n' "$ran"
}

main "$@"
