#!/usr/bin/env bash
#
# Copyright (c) 2026 Open Device Partnership and Contributors
# SPDX-License-Identifier: MIT
#
# M5 serialized acceptance runner.
#
# Normal sequence:
#   reset -> jumper -> acceptance -> spi_loopback -> recovery_teardown
#
# A phase advances ONLY after zero executable status, its exact success marker,
# and a verified USB release. Logging a failure and returning zero is prohibited,
# which is why every phase checks BOTH the status and the marker.
#
# THE SINGLE MOST LIKELY ROUTE TO A CONFIDENTLY WRONG "PASSED" is `prog |& tee
# log` returning tee's zero status. `set -o pipefail` plus an explicit read of
# PIPESTATUS[0] is what prevents it here. Do not simplify either away.
#
# Timeout is INFRASTRUCTURE, never a test result -- with exactly one carve-out,
# E4, implemented in m5_run_phase().
#
# This script performs no builds. Build the five images first with the commands
# in acceptance spec §9.3, one at a time, into the -d paths below.

set -o pipefail
set -u

M5_TIMEOUT_S="${M5_TIMEOUT_S:-420}"
M5_KILL_AFTER_S="${M5_KILL_AFTER_S:-10}"
M5_USB_ID="045e:067d"

# Documented fixed fallback for CONFIG_SPI_IDEAL_TRANSFER_DURATION_SCALING, used
# ONLY when the timing measurement is not obtainable. It is a FALLBACK, not a
# measurement: whenever it is used, timing is reported NOT_MEASURABLE so nobody
# can read it as a measured multiplier.
M5_FALLBACK_MULTIPLIER=128
#
# Fixture serial: ONE source of truth.
#
# fixture-identity.dtsi is what the images are actually built against, so it is
# authoritative. Parsing it here means the serial cannot drift between the
# devicetree the firmware is selected by and the value this script reports.
# Overridable by the environment for a different bench, but never duplicated.
#
M5_IDENTITY_DTSI="$(dirname "$0")/fixture-identity.dtsi"
if [ -z "${M5_SERIAL:-}" ]; then
	M5_SERIAL=$(sed -n 's/.*serial-number[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
		"$M5_IDENTITY_DTSI" 2>/dev/null | head -n1)
fi
if [ -z "${M5_SERIAL:-}" ]; then
	echo "run-m5.sh: cannot read serial-number from $M5_IDENTITY_DTSI" >&2
	exit 2
fi

M5_BUILD_RESET=/tmp/m5_reset
M5_BUILD_JUMPER=/tmp/m5_jumper
M5_BUILD_ACCEPTANCE=/tmp/m5_acceptance
M5_BUILD_LOOPBACK=/tmp/m5_spi_loopback
M5_BUILD_TEARDOWN=/tmp/m5_teardown

# Aggregate verdict state. Everything starts INCONCLUSIVE: a verdict is only
# ever raised to PASS by evidence, never by the absence of a failure.
m5_fixture_validity=INCONCLUSIVE
m5_cs_lifecycle=INCONCLUSIVE
m5_spi_data_path=INCONCLUSIVE
m5_timing_verdict=INCONCLUSIVE
m5_payload_verdict=INCONCLUSIVE
m5_fault_latch=INCONCLUSIVE
m5_teardown_verdict=INCONCLUSIVE
m5_overall=INCONCLUSIVE
m5_infrastructure=0

m5_before_result=not-run
m5_before_sha=""
m5_after_result=not-run
m5_after_sha=""

m5_slow_hz=0
m5_fast_hz=0
m5_slow_p50=0; m5_slow_p95=0; m5_slow_p99=0; m5_slow_max=0
m5_fast_p50=0; m5_fast_p95=0; m5_fast_p99=0; m5_fast_max=0
m5_multiplier=0

m5_reset_count=0
m5_pin2_mode=unknown
m5_pin2_level=unknown
m5_pin3_mode_pull=unknown
m5_spi_mode=unknown
m5_spi_frequency=0
m5_power_cycle=false
m5_mosi_miso=false
m5_gpio_jumper=false

m5_log() {
	printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$*" >&2
}

m5_fail_infrastructure() {
	m5_log "INFRASTRUCTURE: $*"
	m5_infrastructure=1
	return 1
}

#
# Pre-phase probe. Runs before EVERY phase and claims no USB itself.
#
# Enumeration, permission and ownership failures are INFRASTRUCTURE, not test
# failures: they carry no actuation evidence at all. Exactly one board must be
# present -- two boards is the 2026-07-29 ambiguous-target class (AGENTS.md
# §13.17) and is an abort here rather than a test.
#
m5_probe() {
	local phase="$1" line bus dev node owners count

	if ! command -v lsusb >/dev/null 2>&1; then
		m5_fail_infrastructure "$phase: lsusb is not available"
		return 1
	fi

	count=$(lsusb -d "$M5_USB_ID" 2>/dev/null | wc -l)
	if [ "$count" -eq 0 ]; then
		m5_fail_infrastructure "$phase: no $M5_USB_ID enumerated (usbipd attach?)"
		return 1
	fi
	if [ "$count" -ne 1 ]; then
		m5_fail_infrastructure \
			"$phase: $count devices matching $M5_USB_ID; exactly one required. \
Ambiguous target -- detach all but $M5_SERIAL."
		return 1
	fi

	line=$(lsusb -d "$M5_USB_ID" 2>/dev/null | head -n1)
	bus=$(printf '%s' "$line" | awk '{print $2}')
	dev=$(printf '%s' "$line" | awk '{sub(/:/,"",$4); print $4}')
	node="/dev/bus/usb/${bus}/${dev}"

	if [ ! -r "$node" ] || [ ! -w "$node" ]; then
		m5_fail_infrastructure "$phase: $node is not readable and writable by $(id -un)"
		return 1
	fi

	# A previous phase must have released the interface. WinUSB-style
	# exclusive claims are per-interface, and a lingering owner presents as a
	# generic -ENODEV inside Zephyr, indistinguishable from board-absent.
	owners=""
	if command -v fuser >/dev/null 2>&1; then
		owners=$(fuser "$node" 2>/dev/null | tr -d ' ')
	elif command -v lsof >/dev/null 2>&1; then
		owners=$(lsof -t "$node" 2>/dev/null | tr '\n' ' ' | tr -d ' ')
	else
		m5_log "$phase: neither fuser nor lsof available; ownership not verified"
	fi

	if [ -n "$owners" ]; then
		m5_fail_infrastructure "$phase: USB ownership unavailable, $node held by pid(s) $owners"
		return 1
	fi

	m5_log "$phase: probe OK ($node)"
	return 0
}

# Probe with the single bounded retry the spec allows. Blind retry loops are
# prohibited; a second failure aborts.
m5_probe_with_retry() {
	local phase="$1"

	if m5_probe "$phase"; then
		return 0
	fi

	m5_log "$phase: probe failed; waiting 2s for release and retrying ONCE"
	sleep 2
	m5_infrastructure=0

	if m5_probe "$phase"; then
		return 0
	fi

	m5_infrastructure=1
	return 1
}

#
# Bounded phase runner.
#
# Prints M5_PHASE_VERDICT=... and returns:
#   0 PASS
#   1 FAIL (test result)
#   2 INFRASTRUCTURE
#
m5_run_phase() {
	local phase="$1" exe="$2" marker="$3"
	shift 3
	local log="/tmp/m5_${phase}.run.log"
	local rc

	if [ ! -x "$exe" ]; then
		m5_log "$phase: $exe is missing or not executable -- build it first (spec §9.3)"
		echo 'M5_PHASE_VERDICT=INFRASTRUCTURE_MISSING_IMAGE'
		return 2
	fi

	m5_log "$phase: running $exe $* (timeout ${M5_TIMEOUT_S}s)"

	# Merged stdout+stderr: registry and FFI causes are printed on the HOST's
	# stderr while Zephyr may collapse board-absent and board-busy into a
	# single -ENODEV. Losing stderr loses the only discriminator.
	timeout --signal=TERM --kill-after="${M5_KILL_AFTER_S}s" "${M5_TIMEOUT_S}s" \
		"$exe" "$@" 2>&1 | tee "$log"
	rc=${PIPESTATUS[0]}

	if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then
		#
		# E4 carve-out. Plan §11.1: if a caller never releases, a
		# transceive with a different config BLOCKS FOREVER -- no
		# timeout, no watchdog, not detectable across process death.
		# T4 step 4 is exactly that call. Classifying this as
		# infrastructure would hide a genuine controller-lock leak behind
		# a retryable verdict and retry it forever.
		#
		# The distinguisher is in the log: step 3's -EBUSY present, step
		# 4's result line absent. Non-retryable.
		#
		if [ "$phase" = acceptance ] &&
		   grep -qE '^M5_T4_STEP3_RELEASE=-?[0-9]+ sym=EBUSY$' "$log" &&
		   ! grep -q '^M5_T4_STEP4_RESULT=' "$log"; then
			m5_log "$phase: TIMEOUT after T4 step 3 with no step-4 result -- \
possible controller lock leak (plan §11.1). NOT infrastructure, NOT retried."
			echo 'M5_PHASE_VERDICT=FAIL_LOCK_LEAK'
			return 1
		fi

		m5_log "$phase: timed out after ${M5_TIMEOUT_S}s"
		echo 'M5_PHASE_VERDICT=INFRASTRUCTURE_TIMEOUT'
		return 2
	fi

	if [ "$rc" -ne 0 ]; then
		m5_log "$phase: exited $rc"
		echo 'M5_PHASE_VERDICT=FAIL'
		return 1
	fi

	if ! grep -qx "$marker" "$log"; then
		m5_log "$phase: exited 0 but success marker '$marker' is absent"
		echo 'M5_PHASE_VERDICT=FAIL'
		return 1
	fi

	echo 'M5_PHASE_VERDICT=PASS'
	return 0
}

# Extract "KEY=value" from a phase log.
m5_value() {
	local log="$1" key="$2"

	grep -m1 "^${key}=" "$log" 2>/dev/null | cut -d= -f2- || true
}

m5_emit_verdict() {
	cat <<JSON
{
  "milestone": "M5",
  "fixture_validity": "${m5_fixture_validity}",
  "cs_lifecycle": "${m5_cs_lifecycle}",
  "spi_data_path": "${m5_spi_data_path}",
  "timing": {
    "verdict": "${m5_timing_verdict}",
    "slow_p50_us": ${m5_slow_p50:-0},
    "slow_p95_us": ${m5_slow_p95:-0},
    "slow_p99_us": ${m5_slow_p99:-0},
    "slow_max_us": ${m5_slow_max:-0},
    "fast_p50_us": ${m5_fast_p50:-0},
    "fast_p95_us": ${m5_fast_p95:-0},
    "fast_p99_us": ${m5_fast_p99:-0},
    "fast_max_us": ${m5_fast_max:-0},
    "slow_frequency_hz": ${m5_slow_hz:-0},
    "fast_frequency_hz": ${m5_fast_hz:-0},
    "selected_multiplier": ${m5_multiplier:-0}
  },
  "payload_boundary": {
    "verdict": "${m5_payload_verdict}",
    "before_4096": "${m5_before_result}",
    "before_log_sha256": "${m5_before_sha}",
    "after_4096": "${m5_after_result}",
    "after_log_sha256": "${m5_after_sha}"
  },
  "fault_latch": "${m5_fault_latch}",
  "explicitly_untested": [
    "first_errno_preservation_with_distinct_errnos",
    "no_second_gpio_put",
    "ehostdown_before_any_io",
    "non_returning_rpc_residue",
    "cpol_cpha_wire_mapping",
    "driver_source_mutation_controls",
    "hold_lock_different_config_deadlock",
    "upstream_timed_transfer_bounds_on_native_sim",
    "spi_transfer_duration_when_timing_not_measurable",
    "spi_full_duplex_payload_ceiling",
    "payload_ceiling_boundary_between_1013_and_1015",
    "firmware_dispatcher_hang_at_1015_byte_transfer"
  ],
  "teardown": {
    "subscriptions_reset": ${m5_reset_count:-0},
    "pin2_mode": "${m5_pin2_mode}",
    "pin2_level": "${m5_pin2_level}",
    "pin3_mode_pull": "${m5_pin3_mode_pull}",
    "spi_mode": "${m5_spi_mode}",
    "spi_frequency_hz": ${m5_spi_frequency:-0},
    "power_cycle_occurred": ${m5_power_cycle},
    "mosi_miso_jumper_fitted": ${m5_mosi_miso},
    "gpio2_gpio3_jumper_fitted": ${m5_gpio_jumper}
  },
  "overall": "${m5_overall}"
}
JSON
}

#
# Acceptance spec §4.1 step 2 -- the pre-fix control.
#
# Runs ONLY the 4096-byte case against a tree that still has
# PDG_SPI_MAX_BUFFER == 4096U. Expected result is -ECOMM (-70). ANY other
# result stops M5 for re-analysis: the sanctioned root-cause premise has not
# reproduced, and the fix must not be applied on the strength of an unexplained
# reading.
#
m5_payload_before_only() {
	local log=/tmp/m5-payload-before.log
	local rc

	if ! m5_probe_with_retry payload-before; then
		m5_overall=INFRASTRUCTURE
		m5_emit_verdict
		return 2
	fi

	m5_run_phase payload-before \
		"${M5_BUILD_ACCEPTANCE}/zephyr/zephyr.exe" \
		'M5_PAYLOAD_BEFORE_DONE' --payload-before-only
	rc=$?

	cp -f /tmp/m5_payload-before.run.log "$log" 2>/dev/null || true
	m5_before_sha=$(sha256sum "$log" 2>/dev/null | cut -d' ' -f1)

	if [ "$rc" -eq 2 ]; then
		m5_overall=INFRASTRUCTURE
		m5_emit_verdict
		return 2
	fi
	if [ "$rc" -ne 0 ]; then
		m5_payload_verdict=FAIL
		m5_overall=FAIL
		m5_emit_verdict
		return 1
	fi

	m5_before_result=$(m5_value "$log" M5_PAYLOAD_BEFORE_RESULT)

	# Symbolic, not numeric -- see the note in m5_full_sequence().
	if printf '%s' "$m5_before_result" | grep -q 'sym=ECOMM'; then
		m5_log "before-control reproduced the -ECOMM regression; the fix may proceed"
		m5_before_result="-ECOMM|-70"
	else
		m5_log "STOP: before-control returned '${m5_before_result}', not -ECOMM (-70). \
The sanctioned root-cause premise has NOT reproduced. Do not apply the fix and \
do not explain this away -- re-analyse."
		m5_payload_verdict=FAIL
		m5_overall=FAIL
	fi

	m5_emit_verdict
	[ "$m5_payload_verdict" = FAIL ] && return 1
	return 0
}

m5_full_sequence() {
	local rc log

	# ---------------- phase 1: reset ----------------
	if ! m5_probe_with_retry reset; then
		m5_overall=INFRASTRUCTURE; m5_emit_verdict; return 2
	fi
	m5_run_phase reset "${M5_BUILD_RESET}/zephyr/zephyr.exe" 'M5_RESET_PASS'
	rc=$?
	if [ "$rc" -ne 0 ]; then
		m5_overall=$([ "$rc" -eq 2 ] && echo INFRASTRUCTURE || echo FAIL)
		m5_emit_verdict; return "$rc"
	fi

	# The phase-1 count is RECORDED, never gated on. Zero is a valid and
	# healthy entry state: the orphaned pin-2 subscription that plan R7
	# describes is conditional on prior board state, not guaranteed. This
	# phase is still mandatory precisely because that state cannot be
	# assumed -- but a clean board must not be reported as a failure.
	#
	# A NONZERO count here is not a failure either; it means the PREVIOUS
	# run left residue, and it is worth seeing in the log.
	m5_log "reset: entry-state subscription count = \
$(m5_value /tmp/m5_reset.run.log M5_RESET_COUNT)"

	# ---------------- phase 2: jumper ----------------
	# The fixture GATE. A failure here voids every later measurement, so the
	# sequence aborts and the other verdicts stay INCONCLUSIVE rather than
	# FAIL: a failure whose fixture was invalid is not evidence of a driver
	# defect either.
	if ! m5_probe_with_retry jumper; then
		m5_overall=INFRASTRUCTURE; m5_emit_verdict; return 2
	fi
	m5_run_phase jumper "${M5_BUILD_JUMPER}/zephyr/zephyr.exe" 'M5_JUMPER_PASS'
	rc=$?
	if [ "$rc" -ne 0 ]; then
		m5_fixture_validity=$([ "$rc" -eq 2 ] && echo INFRASTRUCTURE || echo FAIL)
		m5_overall=$([ "$rc" -eq 2 ] && echo INFRASTRUCTURE || echo FAIL)
		m5_emit_verdict; return "$rc"
	fi
	m5_fixture_validity=PASS

	# ---------------- phase 3: acceptance ----------------
	if ! m5_probe_with_retry acceptance; then
		m5_overall=INFRASTRUCTURE; m5_emit_verdict; return 2
	fi
	m5_run_phase acceptance "${M5_BUILD_ACCEPTANCE}/zephyr/zephyr.exe" 'M5_ACCEPTANCE_PASS'
	rc=$?
	log=/tmp/m5_acceptance.run.log
	cp -f "$log" /tmp/m5-payload-after.log 2>/dev/null || true
	m5_after_sha=$(sha256sum /tmp/m5-payload-after.log 2>/dev/null | cut -d' ' -f1)

	if [ "$rc" -ne 0 ]; then
		# A named deterministic shift or lag is a mode/fixture
		# limitation, so the data path is INCONCLUSIVE, not FAIL.
		if grep -q '^M5_ACCEPTANCE_INCONCLUSIVE' "$log" 2>/dev/null; then
			m5_spi_data_path=INCONCLUSIVE
			m5_overall=INCONCLUSIVE
		elif [ "$rc" -eq 2 ]; then
			m5_overall=INFRASTRUCTURE
		else
			m5_overall=FAIL
			m5_fault_latch=FAIL
		fi
		m5_emit_verdict; return "$rc"
	fi

	m5_spi_data_path=PASS

	# CS evidence is decisive and is NEVER inferred. Both transitions must be
	# present as transitions observed within the acceptance process; a
	# standalone HIGH is necessary but never sufficient, because high-Z, a
	# firmware monitor and a missing jumper all read HIGH too.
	if grep -qx 'M5_T3A_PASS' "$log" &&
	   grep -qx 'M5_T3B_TRANSITION=LOW_TO_HIGH' "$log" &&
	   grep -qx 'M5_T3_PASS' "$log"; then
		m5_cs_lifecycle=PASS
	else
		m5_log "CS witness evidence missing or inconclusive -- cs_lifecycle stays INCONCLUSIVE"
		m5_cs_lifecycle=INCONCLUSIVE
	fi

	#
	# ERRNO MATCHING IS SYMBOLIC, NEVER NUMERIC.
	#
	# This block used to grep for -108 and -122. Those are Zephyr
	# MINIMAL-LIBC values; native_sim links the HOST glibc, where EHOSTDOWN
	# is 112 and EMSGSIZE is 90. The literals never matched, so this would
	# have reported fault_latch=INCONCLUSIVE and payload_verdict=FAIL on a
	# run where both PASSED -- discrediting the two highest-value results in
	# M5 with a test-infrastructure bug. The apps now print `sym=NAME`
	# resolved from their own <errno.h>, and everything below matches the
	# symbol. Do not reintroduce a numeric errno literal in this file.
	#
	if grep -qx 'M5_T4_PASS' "$log" &&
	   grep -qE '^M5_T4_STEP3_RELEASE=-?[0-9]+ sym=EBUSY$' "$log" &&
	   grep -qE '^M5_T4_STEP4_RESULT=-?[0-9]+ sym=EHOSTDOWN$' "$log" &&
	   grep -qx 'M5_T4_STEP7_TRANSITION=LOW_TO_HIGH' "$log"; then
		m5_fault_latch=PASS
	else
		m5_fault_latch=INCONCLUSIVE
	fi

	#
	# T5c is the "after" half of the before/after pair. It must be a LOCAL
	# rejection, and the driver's own warning must name the constant actually
	# compiled into the translation unit.
	#
	# The ceiling is NOT a literal here either: it is read back from the
	# app's own M5_T5A_LENGTH line and used to build the pattern, so the
	# value the test exercised and the value the driver compiled are checked
	# against each other with no third copy in this script to drift.
	#
	m5_ceiling=$(m5_value "$log" M5_T5A_LENGTH | awk '{print $1}')
	if [ -z "${m5_ceiling:-}" ]; then
		m5_log "T5a did not report its length; cannot verify the compiled ceiling"
		m5_after_result="other"
		m5_payload_verdict=FAIL
	elif grep -qx 'M5_T5_PASS' "$log" &&
	     grep -qE '^M5_T5C_4096_RESULT=-?[0-9]+ sym=EMSGSIZE$' "$log" &&
	     grep -q "maximum transfer size of ${m5_ceiling} bytes" "$log"; then
		m5_after_result="-EMSGSIZE"
		m5_payload_verdict=PASS
	else
		m5_after_result="other"
		m5_payload_verdict=FAIL
	fi

	m5_slow_hz=$(m5_value "$log" M5_TIMING_SLOW_FREQUENCY_HZ)
	m5_fast_hz=$(m5_value "$log" M5_TIMING_FAST_FREQUENCY_HZ)
	m5_slow_p50=$(m5_value "$log" M5_TIMING_SLOW_P50_US)
	m5_slow_p95=$(m5_value "$log" M5_TIMING_SLOW_P95_US)
	m5_slow_p99=$(m5_value "$log" M5_TIMING_SLOW_P99_US)
	m5_slow_max=$(m5_value "$log" M5_TIMING_SLOW_MAX_US)
	m5_fast_p50=$(m5_value "$log" M5_TIMING_FAST_P50_US)
	m5_fast_p95=$(m5_value "$log" M5_TIMING_FAST_P95_US)
	m5_fast_p99=$(m5_value "$log" M5_TIMING_FAST_P99_US)
	m5_fast_max=$(m5_value "$log" M5_TIMING_FAST_MAX_US)

	# ceil((1.25 * observed_max_us) / theoretical_minimum_us) for a 54-byte
	# transfer, taking the maximum over both modes. Never raise this to make
	# a failed run green -- if the measured value and the run disagree, the
	# measurement is stale, so remeasure.
	m5_multiplier=$(awk -v sm="${m5_slow_max:-0}" -v sf="${m5_slow_hz:-1}" \
			    -v fm="${m5_fast_max:-0}" -v ff="${m5_fast_hz:-1}" '
		BEGIN {
			bits = 54 * 8;
			ts = bits * 1000000.0 / sf;
			tf = bits * 1000000.0 / ff;
			ms = (ts > 0) ? int((1.25 * sm) / ts) + ((1.25 * sm) % ts > 0) : 0;
			mf = (tf > 0) ? int((1.25 * fm) / tf) + ((1.25 * fm) % tf > 0) : 0;
			m = (ms > mf) ? ms : mf;
			print m;
		}')

	if [ "${m5_multiplier:-0}" -gt 0 ] && [ "${m5_multiplier:-0}" -le 256 ]; then
		m5_timing_verdict=PASS
		printf 'CONFIG_SPI_IDEAL_TRANSFER_DURATION_SCALING=%s\n' "$m5_multiplier" \
			> /tmp/m5-measured.conf
	else
		#
		# TIMING IS NON-GATING. It used to abort the sequence here, which
		# meant a target where the measurement is not obtainable could
		# never reach loopback at all.
		#
		# The acceptance app now takes elapsed time from the HOST clock
		# (m5_bottom_host_monotonic_us), because Zephyr's simulated clock
		# on native_sim does not advance while the host thread blocks in
		# a USB call and reported 0 us for every real transfer. If the
		# multiplier is still unusable, the honest answer is
		# NOT_MEASURABLE plus a documented fixed fallback -- never a
		# large multiplier presented as if it had been measured, which
		# would recreate a vacuous timing test with a veneer of rigour.
		#
		# NOT_MEASURABLE is not a failure and does not stop the run; it
		# makes the milestone INCONCLUSIVE at worst, and the reason is
		# carried in explicitly_untested.
		#
		m5_log "timing multiplier ${m5_multiplier} is outside 1..256 -- recording \
NOT_MEASURABLE and continuing with the documented fallback of \
${M5_FALLBACK_MULTIPLIER}. This is NOT a measured value and must not be \
reported as one."
		m5_timing_verdict=NOT_MEASURABLE
		m5_multiplier=0
		printf 'CONFIG_SPI_IDEAL_TRANSFER_DURATION_SCALING=%s\n' \
			"$M5_FALLBACK_MULTIPLIER" > /tmp/m5-measured.conf
	fi

	# ---------------- phase 4: upstream spi_loopback ----------------
	# Never after an abnormal acceptance exit: process-local latch, lock and
	# owner die with the process, but physical firmware pin state does not,
	# so a killed HOLD can leave CS 2 an output driving LOW.
	if ! m5_probe_with_retry loopback; then
		m5_overall=INFRASTRUCTURE; m5_emit_verdict; return 2
	fi
	#
	# The upstream suite exits NONZERO on this target because of a known,
	# expected, environmental failure (see the expected-failure block below),
	# so its executable status alone cannot be the verdict. The marker
	# required here is only that the suite reached its second spec; the
	# disposition is then decided by the ledger and expected-failure checks.
	# An INFRASTRUCTURE result (timeout, missing image) still aborts.
	#
	m5_run_phase loopback "${M5_BUILD_LOOPBACK}/zephyr/zephyr.exe" \
		'Testing loopback spec: FAST'
	rc=$?
	log=/tmp/m5_loopback.run.log
	if [ "$rc" -eq 2 ]; then
		m5_overall=INFRASTRUCTURE
		m5_emit_verdict; return 2
	fi

	# The suite has no custom marker, so require BOTH spec markers. A
	# malformed spec array -- one child, or two children resolving to the
	# same node -- runs half the cases and still reports all-green.
	if ! grep -q 'Testing loopback spec: SLOW' "$log" ||
	   ! grep -q 'Testing loopback spec: FAST' "$log"; then
		m5_log "loopback: one or both spec markers absent -- the run was vacuous"
		m5_overall=FAIL
		m5_emit_verdict; return 1
	fi

	#
	# ---------------- observed ledger ----------------
	#
	# OBSERVATION-BACKED, not source-derived. The previous 26/12/2 ledger was
	# read off spi.c and was wrong: it counted distinct test FUNCTIONS in the
	# source rather than the RESULTS ztest reports, missed that ztest
	# re-attempts a failing test, and mis-attributed the split between the
	# two suites. Observed on hardware: 41 PASS / 12 SKIP / 1 FAIL /
	# 2 NOT BUILT.
	#
	# THE SKIP COUNT IS THE ANTI-VACUITY CHECK AND IS EXACT. A pass in an
	# expected-SKIP row is the disposition most likely to hide a defect --
	# most sharply for the five word sizes, where passing would mean
	# pdg_spi.c:385 failed to reject a non-8-bit word and the bytes only
	# compared equal because a short echoes anything. 12 on the nose:
	# 5 word sizes + test_spi_deinit + test_spi_hold_on_cs, x2 iterations.
	#
	m5_skips=$(grep -c ' SKIP - ' "$log" || true)
	if [ "${m5_skips:-0}" -ne 12 ]; then
		m5_log "loopback: expected exactly 12 SKIP results, observed ${m5_skips}. \
A pass in an expected-SKIP row is a driver defect; an extra skip in an \
expected-PASS row is too."
		m5_overall=FAIL
		m5_emit_verdict; return 1
	fi
	m5_log "loopback: SKIP count 12 as expected (anti-vacuity check held)"

	#
	# ---------------- expected failures ----------------
	#
	# test_spi_complete_multiple_timed is KNOWN-UNRUNNABLE on this target and
	# is NOT a driver defect. It remains VISIBLE: it is reported on every
	# run, by name, with its reasoning. An expected failure that stops being
	# printed is how a real regression hides.
	#
	# Why the classification is certain, not convenient:
	#
	#   1. spi.c:406 asserts time_spent_us >= minimum_transfer_time_us -- a
	#      LOWER bound. CONFIG_SPI_IDEAL_TRANSFER_DURATION_SCALING bounds
	#      only the UPPER limit, so NO value of the multiplier can affect
	#      this assertion. Raising it would be useless and a softening.
	#   2. It fails on SLOW and passes on FAST purely because SLOW's
	#      theoretical minimum (432 us) is larger. That is structural, not
	#      flaky -- ztest's FLAKY label is misleading here.
	#
	# Root cause: upstream spi.c measures with the Zephyr clock, which does
	# not advance on native_sim while the host thread blocks in a USB call.
	# Our acceptance app moved to clock_gettime(CLOCK_MONOTONIC); patching
	# upstream is out of scope.
	#
	m5_fails=$(grep -c ' FAIL - ' "$log" || true)
	m5_expected_fails=$(grep -c ' FAIL - test_spi_complete_multiple_timed' "$log" || true)

	if [ "${m5_expected_fails:-0}" -gt 0 ]; then
		m5_log "loopback: EXPECTED FAILURE present and reported -- \
test_spi_complete_multiple_timed x${m5_expected_fails} (simulated-clock lower-bound \
assert at spi.c:406; unaffected by any multiplier value; upstream, out of scope)"
	fi

	if [ "${m5_fails:-0}" -ne "${m5_expected_fails:-0}" ]; then
		m5_log "loopback: ${m5_fails} failure(s) observed but only \
${m5_expected_fails} are on the expected-failure list -- treat the remainder as a \
driver defect"
		m5_overall=FAIL
		m5_emit_verdict; return 1
	fi

	m5_log "M5_LOOPBACK_PASS"

	# ---------------- phase 5: teardown ----------------
	if ! m5_probe_with_retry teardown; then
		m5_overall=INFRASTRUCTURE; m5_emit_verdict; return 2
	fi
	m5_run_phase teardown "${M5_BUILD_TEARDOWN}/zephyr/zephyr.exe" 'M5_TEARDOWN_PASS' \
		--attest-mosi-miso --attest-gpio-jumper
	rc=$?
	log=/tmp/m5_teardown.run.log
	if [ "$rc" -ne 0 ]; then
		m5_teardown_verdict=$([ "$rc" -eq 2 ] && echo INFRASTRUCTURE || echo FAIL)
		m5_overall=$([ "$rc" -eq 2 ] && echo INFRASTRUCTURE || echo FAIL)
		m5_emit_verdict; return "$rc"
	fi
	m5_teardown_verdict=PASS

	m5_reset_count=$(m5_value "$log" M5_TEARDOWN_SUBSCRIPTIONS_RESET)
	m5_pin2_mode=$(m5_value "$log" M5_TEARDOWN_PIN2_MODE)
	m5_pin2_level=$(m5_value "$log" M5_TEARDOWN_PIN2_LEVEL)
	m5_pin3_mode_pull=$(m5_value "$log" M5_TEARDOWN_PIN3_MODE_PULL)
	m5_spi_mode=$(m5_value "$log" M5_TEARDOWN_SPI_MODE)
	m5_spi_frequency=$(m5_value "$log" M5_TEARDOWN_SPI_FREQUENCY_HZ)
	m5_power_cycle=$(m5_value "$log" M5_TEARDOWN_POWER_CYCLE_OCCURRED)
	m5_mosi_miso=$(m5_value "$log" M5_TEARDOWN_MOSI_MISO_JUMPER_FITTED)
	m5_gpio_jumper=$(m5_value "$log" M5_TEARDOWN_GPIO2_GPIO3_JUMPER_FITTED)

	# A nonzero count after a supposedly-normal acceptance is direct evidence
	# that T4's cleanup did not run. That is a T4 DEFECT REPORT, not a
	# housekeeping detail, and it makes fault_latch FAIL even if every T4
	# assertion passed.
	#
	# NOTE ON WHAT THIS COUNT CAN AND CANNOT DISTINGUISH. Zero here is
	# produced by BOTH "T4 ran and cleaned up" and "T4 never ran at all", so
	# it is not by itself evidence that T4 executed. That distinction is
	# carried by POSITIVE evidence in the acceptance log instead: fault_latch
	# is only raised to PASS when M5_T4_PASS, the step-3 -EBUSY, the step-4
	# -EHOSTDOWN and the step-7 transition are all present, and an acceptance
	# run that never reached T4 would not have printed M5_ACCEPTANCE_PASS and
	# so would already have failed its phase. This count is the NEGATIVE
	# check -- residue detection -- and the two are deliberately independent.
	if [ "${m5_reset_count:-0}" -ne 0 ]; then
		m5_log "teardown reset ${m5_reset_count} subscription(s) after a normal \
acceptance -- T4 cleanup did not run"
		m5_fault_latch=FAIL
	fi

	# ---------------- overall ----------------
	if [ "$m5_infrastructure" -ne 0 ]; then
		m5_overall=INFRASTRUCTURE
	elif [ "$m5_fixture_validity" = PASS ] &&
	     [ "$m5_cs_lifecycle" = PASS ] &&
	     [ "$m5_spi_data_path" = PASS ] &&
	     [ "$m5_timing_verdict" = PASS ] &&
	     [ "$m5_payload_verdict" = PASS ] &&
	     [ "$m5_fault_latch" = PASS ] &&
	     [ "$m5_teardown_verdict" = PASS ]; then
		m5_overall=PASS
	elif [ "$m5_cs_lifecycle" != PASS ]; then
		# Missing, shifted, contradictory or inconclusive CS witness
		# evidence makes the milestone INCONCLUSIVE even if every
		# loopback byte matched: a loopback passes regardless of what
		# chip select does.
		m5_overall=INCONCLUSIVE
	elif [ "$m5_fixture_validity" = FAIL ] ||
	     [ "$m5_spi_data_path" = FAIL ] ||
	     [ "$m5_timing_verdict" = FAIL ] ||
	     [ "$m5_payload_verdict" = FAIL ] ||
	     [ "$m5_fault_latch" = FAIL ] ||
	     [ "$m5_teardown_verdict" = FAIL ]; then
		m5_overall=FAIL
	else
		# Reached when nothing FAILed but something is not PASS -- in
		# particular timing = NOT_MEASURABLE. That is deliberately
		# INCONCLUSIVE rather than FAIL: an unobtainable measurement is
		# not evidence of a defect, and it must not abort the run.
		m5_overall=INCONCLUSIVE
	fi

	m5_emit_verdict
	[ "$m5_overall" = PASS ]
}

m5_usage() {
	cat <<'USAGE'
Usage: run-m5.sh [--payload-before-only]

  (no argument)          Run the full serialized sequence:
                         reset -> jumper -> acceptance -> loopback -> teardown

  --payload-before-only  Acceptance spec §4.1 step 2. Run ONLY the 4096-byte
                         payload control against a tree that still has
                         PDG_SPI_MAX_BUFFER == 4096U. Expected: -ECOMM (-70).

Build the five images first, one at a time, per acceptance spec §9.3.
USAGE
}

case "${1:-}" in
--payload-before-only)
	m5_payload_before_only
	;;
"")
	m5_full_sequence
	;;
-h|--help)
	m5_usage
	;;
*)
	m5_usage
	exit 64
	;;
esac
