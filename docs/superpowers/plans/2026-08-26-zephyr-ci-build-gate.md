# Zephyr CI Build Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a GitHub Actions workflow that compiles and link-checks the Zephyr module against a pinned upstream Zephyr revision, so an FFI change can no longer break `zephyr/` while every existing gate stays green.

**Architecture:** Two jobs in one new workflow. A fast `selftest` job runs the assertion parsers against checked-in fixtures with no Zephyr workspace at all (about ten seconds). A heavy `build` job, gated on `selftest`, sets up a cached west workspace pinned to one Zephyr commit and drives eight `west build` invocations through a single checked-in shell script, asserting per-target outcomes. The gate is build-only: no produced binary is ever executed.

**Tech Stack:** GitHub Actions, Bash, Zephyr `west`, CMake, Corrosion 0.5.2, `native_sim/native/64` with the host C toolchain (no Zephyr SDK).

**Spec:** `docs/superpowers/specs/2026-08-26-zephyr-ci-build-gate-design.md`

**Declared deviation from spec §6.** The spec describes *one* job. This plan uses
two: a `selftest` job that needs no Zephyr workspace, and the `build` job gated
on it. The reason §6 rejected a matrix was cost — eight legs each re-restoring a
multi-gigabyte cache and re-running Corrosion. That reasoning does not apply
here: `selftest` restores no cache, installs nothing, and finishes in seconds.
It buys fast feedback on the assertion parsers, which are the highest-risk part
of the change and the only part that can be tested at all before merge. The
spec's actual constraint — do not multiply the expensive job — is preserved.

## Global Constraints

Copied verbatim from the spec and from AGENTS.md. Every task's requirements implicitly include this section.

- **Pinned Zephyr revision:** `26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0`. This exact 40-character string must appear in both `.github/workflows/zephyr.yml` and `zephyr/README.md`.
- **Board:** `native_sim/native/64`. Never plain `native_sim` — `zephyr/Kconfig` has `depends on 64BIT`.
- **Shield:** `-DSHIELD=pico_de_gallo`, passed explicitly on every build. West resolves `BOARD` before invoking CMake, so the `set(... CACHE ...)` defaults in each sample's `CMakeLists.txt` never take effect.
- **Toolchain:** `ZEPHYR_TOOLCHAIN_VARIANT=host` must be exported, or CMake hard-fails with `Could not find a configuration file for package "Zephyr-sdk"`.
- **BUILD ONLY.** Never `west build -t run`. Never execute `zephyr.exe` or any produced binary. Never invoke `zephyr/tests/pdg_mfd_m5/run-m5.sh`. Rationale: `zephyr/drivers/common/gallo_registry.c:174` calls `gallo_init_strict()`, which opens USB. Linking resolves the symbol; running calls it.
- **Build directories under `/tmp` only.** Repository-root `build/` is NOT gitignored (only `zephyr/build/` is).
- **LF line endings on every new file.** Run `dos2unix` on each. CRLF in a workflow `run:` block breaks `actionlint` with `unexpected character $'\r'`. (AGENTS.md §3)
- **Commit convention:** Conventional Commits, scope `zephyr` and/or `repo`, subject ≤72 chars, capitalized, imperative, no trailing period. (AGENTS.md §10)
- **Required commit trailers** on every commit in this plan, exactly:
  ```
  Assisted-by: OpenCode:claude-opus-5
  Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
  ```
  **Never** add `Signed-off-by:`. (AGENTS.md §4 rule 7)
- **Do not bump any `[package].version`.** (AGENTS.md §4 rule 12)
- **Local linters and shells** (verified present; use these exact paths):
  - `C:\Users\febalbi\AppData\Local\Temp\opencode\tools\actionlint\actionlint.exe`
  - `C:\Users\febalbi\AppData\Local\Temp\opencode\tools\shellcheck\shellcheck.exe`
  - `C:\Program Files\Git\bin\bash.exe` — Git Bash, **bash 5.3.15** with GNU `grep`, `sed`, `sort`, `awk`, `find`, `wc`, `tr`, `cut`, `paste`. The `--self-test` mode **runs locally**; execute it, do not defer it to CI.
  - Invoke as: `& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh --self-test"` (note the POSIX-style path).
- **No local Zephyr environment and no WSL distro.** The eight `west build` targets cannot be run locally under any circumstances; only CI executes those. Git Bash covers `--self-test` and nothing more.
- **Git Bash is MSYS2, not Linux.** A local `--self-test` pass is strong evidence but not proof: utility flags and path handling differ. The parsers read file *contents*, not paths, so the exposure is small — but CI's `selftest` job is still the authoritative run.
- **Working directory:** the worktree at `C:\Users\febalbi\workspace\pico-de-gallo\.worktrees\issue-130-zephyr-ci`, branch `felipebalbi/ci/issue-130-zephyr-build-gate`.

---

## File Structure

| File | Responsibility |
|---|---|
| `zephyr/scripts/ci-build.sh` | Create. Everything: target table, build driver, assertions, self-test, summary. Single file because the assertions and the table that parameterises them change together, and a maintainer needs exactly one command to reproduce CI. |
| `zephyr/scripts/testdata/undefined-ord.log` | Create. Fixture: a realistic native_simulator runner-link failure. |
| `zephyr/scripts/testdata/devicetree_generated.h` | Create. Fixture: the two generated `_ORD` defines, including the `_ORD_STR_SORTABLE` sibling that must not match. |
| `zephyr/scripts/testdata/compile_commands.json` | Create. Fixture: a minimal compile database naming three driver translation units. |
| `.github/workflows/zephyr.yml` | Create. Two jobs: `selftest` and `build`. |
| `zephyr/README.md` | Modify. Record the pinned SHA; document the gate and its build-only limits. |
| `zephyr/CHANGELOG.md` | Modify. Keep a Changelog entry. |
| `AGENTS.md` | Modify, four sites: §5.3 table, §5.4 catalog, §15.1 "Neither gate is automatic", §15.1 checklist item 7. |

---

### Task 1: Script foundation and self-test harness

Creates the script with argument parsing, environment validation, the eight-target table, and a `--self-test` mode with assertion helpers. No building and no build-output assertions yet — those arrive in Tasks 2 and 3. The self-test harness is built first because Task 2 is written test-first against it.

**Files:**
- Create: `zephyr/scripts/ci-build.sh`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `PDG_TARGETS` — array of `|`-delimited records. Field order, fixed for the whole plan: `name|kind|srcdir|overlay|zephyr_tus|native_objs|kconfigs`. `kind` is `pass` or `basefail`. `srcdir` and `overlay` are repo-relative; `overlay` is empty for samples that use `app.overlay`. The three list fields are comma-separated, and empty means "none".
  - `target_field <record> <index>` — echoes field `<index>` (1-based) of a record.
  - `st_check <description> <actual> <expected>` — self-test assertion; increments `ST_PASS` or `ST_FAIL`.
  - `TESTDATA_DIR` — absolute path to `zephyr/scripts/testdata`.
  - `PDG_ROOT` — absolute repository root, derived from the script's own location.
  - `die <message>` — prints to stderr and exits 1.

- [ ] **Step 1: Write the failing self-test**

Create `zephyr/scripts/ci-build.sh` containing only the header, the harness, and one self-test that exercises `target_field` against the real table. Write it in this order so the file is runnable at every later step.

```bash
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
```

- [ ] **Step 2: Verify it lints and runs clean**

```powershell
& 'C:\Users\febalbi\AppData\Local\Temp\opencode\tools\shellcheck\shellcheck.exe' -S warning zephyr/scripts/ci-build.sh
"shellcheck=$LASTEXITCODE"
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh --self-test"
"selftest=$LASTEXITCODE"
```

Expected: shellcheck exit 0 with no output — `run-m5.sh` is clean at this
severity, so the new script must be too. Self-test prints `5 passed, 0 failed`
and exits 0.

Then confirm the argument dispatch rejects the not-yet-implemented path:

```powershell
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh"
"exit=$LASTEXITCODE"
```
Expected: `ci-build: not implemented yet` on stderr, exit 1.

- [ ] **Step 3: Normalise line endings and mark executable**

```powershell
dos2unix zephyr/scripts/ci-build.sh
$b = [IO.File]::ReadAllBytes((Resolve-Path zephyr/scripts/ci-build.sh))
"CR=$(($b | Where-Object {$_ -eq 13}).Count)"
```
Expected: `CR=0`.

```powershell
git add zephyr/scripts/ci-build.sh
git update-index --chmod=+x zephyr/scripts/ci-build.sh
git ls-files -s zephyr/scripts/ci-build.sh
```
Expected mode: `100755`.

- [ ] **Step 4: Commit**

```powershell
git commit -m "ci(zephyr): Add the CI build script foundation and self-test harness" -m "The target table encodes the eight build targets and, for each, the
expected Zephyr-side translation units, native_simulator-side objects
and Kconfig symbols. Those three lists are derived from each target's
overlay, which decides the DT_HAS_* Kconfig defaults, and are what the
non-vacuity assertions in the next task check against.

The self-test harness runs without a Zephyr workspace so the parsing
logic can be gated in CI in seconds rather than behind a multi-gigabyte
west checkout. It also runs under Git Bash on the authoring machine,
which is what lets the assertion logic be tested at all before merge --
no local Zephyr environment exists, so the eight build targets cannot
be exercised outside CI.

Refs: #130" -m "Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 2: Assertion parsers, fixtures, and their self-tests

The highest-risk logic in the change, and the only genuinely unit-testable part. Written test-first against fixtures derived from recorded evidence in the M3 and M4 specs, not from invention.

**Files:**
- Create: `zephyr/scripts/testdata/undefined-ord.log`
- Create: `zephyr/scripts/testdata/devicetree_generated.h`
- Create: `zephyr/scripts/testdata/compile_commands.json`
- Modify: `zephyr/scripts/ci-build.sh`

**Interfaces:**
- Consumes: `st_check`, `TESTDATA_DIR`, `PDG_ALL_DRIVER_TUS` from Task 1.
- Produces:
  - `undefined_ords <logfile>` — echoes the sorted unique ordinal numbers of every undefined `__device_dts_ord_N` in the log, space-separated. Empty output means none.
  - `resolve_ord_defines <dtheader> <ordinal>` — echoes the names of generated defines whose value is that ordinal, one per line. Excludes `_ORD_STR_SORTABLE`.
  - `tu_set <compile_commands.json>` — echoes the sorted unique `pdg_*.c` names found, space-separated.

- [ ] **Step 1: Write the fixtures**

`zephyr/scripts/testdata/devicetree_generated.h`. The `_ORD_STR_SORTABLE` sibling is the entire point of this fixture — a naive grep matches it and yields a wrong answer.

```c
/*
 * Fixture excerpt of a Zephyr-generated devicetree header.
 *
 * Only the shapes the ci-build.sh ordinal resolver depends on. Node paths and
 * the ordinal 49 follow the values recorded for spi_bridge in
 * docs/superpowers/specs/2026-08-19-zephyr-mfd-m4-acceptance.md check A-11.
 *
 * The _ORD_STR_SORTABLE lines are load-bearing: a resolver that matches them
 * returns the wrong define. Do not delete them.
 */

#define DT_N_S_pico_de_gallo_S_spi_ORD 12
#define DT_N_S_pico_de_gallo_S_spi_ORD_STR_SORTABLE "00012"

#define DT_N_S_pico_de_gallo_S_spi_S_is31fl3743b_0_ORD 49
#define DT_N_S_pico_de_gallo_S_spi_S_is31fl3743b_0_ORD_STR_SORTABLE "00049"

#define DT_N_S_pico_de_gallo_S_i2c_S_tmp117_48_ORD 50
#define DT_N_S_pico_de_gallo_S_i2c_S_tmp117_48_ORD_STR_SORTABLE "00050"
```

`zephyr/scripts/testdata/undefined-ord.log`. Shape follows the GNU ld wording the M3 spec greps for (`undefined reference to \`__device_dts_ord_`) and the `.text.main` section M4 A-11 recorded.

```text
-- west build: building application
[1/9] Generating include/generated/zephyr/devicetree_generated.h
[8/9] Linking C executable zephyr/zephyr.elf
[9/9] Linking C executable zephyr/zephyr.exe
FAILED: zephyr/zephyr.exe
/usr/bin/ld: zephyr/libzephyr.a(main.c.obj): in function `main':
main.c:(.text.main+0x1a): undefined reference to `__device_dts_ord_49'
/usr/bin/ld: main.c:(.text.main+0x2e): undefined reference to `__device_dts_ord_49'
collect2: error: ld returned 1 exit status
ninja: build stopped: subcommand failed.
```

`zephyr/scripts/testdata/compile_commands.json`. Trimmed to the fields the grep touches.

```json
[
  {
    "directory": "/tmp/pdg-ci/spi_nor_id",
    "command": "cc -o modules/pico-de-gallo/drivers/mfd/pdg_mfd.c.obj -c /src/zephyr/drivers/mfd/pdg_mfd.c",
    "file": "/src/zephyr/drivers/mfd/pdg_mfd.c"
  },
  {
    "directory": "/tmp/pdg-ci/spi_nor_id",
    "command": "cc -o modules/pico-de-gallo/drivers/gpio/pdg_gpio.c.obj -c /src/zephyr/drivers/gpio/pdg_gpio.c",
    "file": "/src/zephyr/drivers/gpio/pdg_gpio.c"
  },
  {
    "directory": "/tmp/pdg-ci/spi_nor_id",
    "command": "cc -o modules/pico-de-gallo/drivers/spi/pdg_spi.c.obj -c /src/zephyr/drivers/spi/pdg_spi.c",
    "file": "/src/zephyr/drivers/spi/pdg_spi.c"
  }
]
```

- [ ] **Step 2: Write the failing self-tests**

Insert these `st_check` calls into `self_test()` in `zephyr/scripts/ci-build.sh`, immediately before the final `printf`:

```bash
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
```

- [ ] **Step 3: Run the self-test to verify it fails**

This is the "red" state of the TDD cycle. Run it and record the output:

```powershell
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh --self-test"
"exit=$LASTEXITCODE"
```

Expected: the five Task 1 checks print `ok`, then the new checks fail with
`command not found` for `undefined_ords`, `resolve_ord_defines` and `tu_set`,
and the run exits non-zero. Do **not** proceed to Step 4 until you have seen
that failure — a test that has never failed proves nothing.

- [ ] **Step 4: Write the minimal implementation**

Insert these three functions into `zephyr/scripts/ci-build.sh`, after `target_field` and before `ST_PASS=0`:

```bash
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
```

- [ ] **Step 5: Run the self-test to verify it passes**

```powershell
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh --self-test"
"exit=$LASTEXITCODE"
& 'C:\Users\febalbi\AppData\Local\Temp\opencode\tools\shellcheck\shellcheck.exe' -S warning zephyr/scripts/ci-build.sh
"shellcheck=$LASTEXITCODE"
```

Expected: `12 passed, 0 failed`, exit 0; shellcheck exit 0.

If any check fails, fix the parser — not the fixture. The fixtures encode
outcomes recorded in M3 and M4 against real builds; changing one to make a test
pass discards the only evidence this logic has.

- [ ] **Step 6: Normalise and commit**

```powershell
dos2unix zephyr/scripts/ci-build.sh zephyr/scripts/testdata/undefined-ord.log zephyr/scripts/testdata/devicetree_generated.h zephyr/scripts/testdata/compile_commands.json
git add zephyr/scripts/
git commit -m "ci(zephyr): Add assertion parsers with fixtures and self-tests" -m "Three pure text functions carry the whole risk of this gate, so they
are unit-tested against fixtures rather than only exercised by a full
Zephyr build.

undefined_ords and resolve_ord_defines reuse M3's recorded resolution
loop, which maps ordinal to node rather than the reverse. The trailing
anchor on the _ORD define is load-bearing: Zephyr emits a sibling
_ORD_STR_SORTABLE whose value is a quoted string, and a resolver that
matches it returns the wrong define. A fixture pins that, and a second
test pins that ordinal 4 does not prefix-match the 49 line.

tu_set reuses M4 A-01's compile_commands.json grep, which sidesteps the
.o versus .obj suffix question that object-file globbing would have
introduced.

Fixture values follow the outcomes recorded for spi_bridge in M4 A-11
rather than invented shapes.

Refs: #130" -m "Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 3: Build driver, per-target assertions, and summary

Wires the Task 2 parsers to real build output, adds the two assertion contracts, and emits the results table.

**Files:**
- Modify: `zephyr/scripts/ci-build.sh`

**Interfaces:**
- Consumes: everything from Tasks 1 and 2.
- Produces:
  - `build_target <record>` — runs one `west build`, returns its exit status, writes the log to `${BUILD_ROOT}/<name>.log`.
  - `assert_pass <record> <builddir> <status>` — spec §5.1. Returns 0 on success.
  - `assert_basefail <record> <builddir> <status> <log>` — spec §5.2. Returns 0 on success.
  - Exit status: 0 only if every selected target satisfied its contract.

- [ ] **Step 1: Write the build driver and assertions**

Replace the `main()` stub in `zephyr/scripts/ci-build.sh` and insert the following above it:

```bash
BUILD_ROOT=${PDG_CI_BUILD_ROOT:-/tmp/pdg-ci}
SUMMARY_FILE=${PDG_CI_SUMMARY:-}

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
	if ! find "$builddir" -name 'libpico_de_gallo_ffi.a' \
		| grep -q .; then
		printf '  %s: libpico_de_gallo_ffi.a not found under %s\n' "$name" "$builddir"
		rc=1
	fi

	# 3. Two-sided translation-unit check over the four driver units.
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

	# 4. native_simulator-side objects, tolerant of .o and .obj.
	local obj
	for obj in $(printf '%s' "$expected_objs" | tr ',' ' '); do
		if ! find "$builddir" \( -name "${obj}.c.o" -o -name "${obj}.c.obj" \) \
			| grep -q .; then
			printf '  %s: native_simulator object %s.c.o[bj] not found\n' "$name" "$obj"
			rc=1
		fi
	done

	# 5. Kconfig symbols are actually enabled.
	local sym
	for sym in $(printf '%s' "$expected_kconfigs" | tr ',' ' '); do
		if ! grep -qx "${sym}=y" "${builddir}/zephyr/.config"; then
			printf '  %s: %s is not =y in the build .config\n' "$name" "$sym"
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

	# 3. Exactly one distinct undefined ordinal.
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
```

- [ ] **Step 2: Write the main driver**

Replace `main()` entirely:

```bash
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

	require_env
	mkdir -p "$BUILD_ROOT"

	local failures=0 results=""
	local record name kind builddir log status verdict
	for record in "${PDG_TARGETS[@]}"; do
		name=$(target_field "$record" 1)
		kind=$(target_field "$record" 2)

		if [ -n "$selected" ]; then
			case ",${selected}," in
			*",${name},"*) ;;
			*) continue ;;
			esac
		fi

		builddir="${BUILD_ROOT}/${name}"
		log="${BUILD_ROOT}/${name}.log"

		printf '::group::build %s (%s)\n' "$name" "$kind"
		build_target "$record"
		status=$?
		cat "$log"
		printf '::endgroup::\n'

		if [ "$kind" = pass ]; then
			assert_pass "$record" "$builddir" "$status"
		else
			assert_basefail "$record" "$builddir" "$status" "$log"
		fi

		if [ $? -eq 0 ]; then
			verdict=PASS
		else
			verdict=FAIL
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

	[ "$failures" -eq 0 ] || die "${failures} target(s) did not meet their contract"
	printf 'all selected targets met their contract\n'
}
```

- [ ] **Step 3: Verify the static gate and the self-test**

```powershell
& 'C:\Users\febalbi\AppData\Local\Temp\opencode\tools\shellcheck\shellcheck.exe' -S warning zephyr/scripts/ci-build.sh
"shellcheck=$LASTEXITCODE"
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh --self-test"
"selftest=$LASTEXITCODE"
```
Expected: both exit 0; self-test still `12 passed, 0 failed` — this task must not regress Task 2's parsers.

Also confirm `require_env` refuses a bare invocation rather than trying to build:

```powershell
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && unset ZEPHYR_BASE; zephyr/scripts/ci-build.sh --targets i2c_bridge"
"exit=$LASTEXITCODE"
```
Expected: `ci-build: ZEPHYR_BASE is not set` on stderr, exit 1. No build attempted.

If shellcheck flags SC2181 ("check exit code directly") on the `if [ $? -eq 0 ]` after the assert dispatch, restructure that block as:

```bash
		if [ "$kind" = pass ]; then
			assert_pass "$record" "$builddir" "$status" && verdict=PASS || verdict=FAIL
		else
			assert_basefail "$record" "$builddir" "$status" "$log" && verdict=PASS || verdict=FAIL
		fi
		if [ "$verdict" = FAIL ]; then
			failures=$((failures + 1))
		fi
```

Re-run shellcheck until exit 0.

- [ ] **Step 4: Confirm no run path exists**

```powershell
Select-String -Path zephyr/scripts/ci-build.sh -Pattern '-t run|zephyr\.exe|run-m5' | ForEach-Object { "$($_.LineNumber): $($_.Line.Trim())" }
```
Expected: matches ONLY inside comments. Any match on an executable line is a Global Constraints violation — stop and fix.

- [ ] **Step 5: Normalise and commit**

```powershell
dos2unix zephyr/scripts/ci-build.sh
git add zephyr/scripts/ci-build.sh
git commit -m "ci(zephyr): Add the build driver and per-target assertion contracts" -m "Pass targets assert five things: a zero exit, that Corrosion produced
libpico_de_gallo_ffi.a, a two-sided translation-unit check, that the
native_simulator-side objects exist, and that the Kconfig symbols are
actually =y. The last is the one that matters most. A shield overlay
that silently left every CONFIG_*_PICO_DE_GALLO off would compile zero
of this module's code and pass forever, including under mutation, so
it is asserted directly rather than inferred.

Baseline-failure targets treat a SUCCESSFUL build as a failure of the
gate, and say so in the diagnostic: it would mean the IS31FL3743B
driver reached upstream Zephyr and both this table and the README need
revisiting. Attribution resolves the ordinal against the devicetree
header of that same build, never against a literal, because M4 A-11
recorded different ordinals for the two samples.

Refs: #130" -m "Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 4: The workflow

**Files:**
- Create: `.github/workflows/zephyr.yml`
- Modify: `zephyr/README.md` (the pinned-SHA line only — the rest of the README lands in Task 5)

**Interfaces:**
- Consumes: `zephyr/scripts/ci-build.sh` and its `--self-test` mode.
- Produces: two job IDs, `selftest` and `build`; and the literal SHA string in `zephyr/README.md` that this workflow's guard greps for.

- [ ] **Step 1: Write the workflow**

```yaml
on:
  push:
    branches: [main]
    paths:
      - "zephyr/**"
      - "crates/pico-de-gallo-ffi/**"
      - "crates/pico-de-gallo-internal/**"
      - "Cargo.toml"
      - "Cargo.lock"
      - ".github/workflows/zephyr.yml"
  pull_request:
    paths:
      - "zephyr/**"
      - "crates/pico-de-gallo-ffi/**"
      - "crates/pico-de-gallo-internal/**"
      - "Cargo.toml"
      - "Cargo.lock"
      - ".github/workflows/zephyr.yml"

# The Zephyr drivers are a consumer of pico-de-gallo-ffi: they call gallo_*
# through the cbindgen-generated header, linked by Corrosion at Zephyr build
# time. Before this workflow, an FFI change could break them while every other
# gate stayed green -- check.yml builds the FFI crate, which still compiles, and
# cbindgen regenerates the header without complaint, but nothing ever compiled
# the Zephyr translation unit that consumed the removed symbol.
#
# Two compile-time gates already existed in the tree and never fired:
# -Werror=switch over the Status-to-errno mapping in drivers/common/common.c,
# and the _Static_asserts pinning the mirrored FFI enums in
# tests/pdg_mfd_m5/common/m5_bottom.c. This workflow is what makes them fire.
#
# BUILD ONLY. Nothing here executes a produced binary. Runtime coverage needs an
# attached board and remains the manual zephyr/tests/pdg_mfd_m5/run-m5.sh
# procedure. A green run here means the module still compiles and links.
#
# Cargo.lock is in the path filter because zephyr/CMakeLists.txt imports the
# crate with corrosion_import_crate(... LOCKED), so a manifest/lock split breaks
# the Zephyr build too, not just check.yml's lockfile job.
name: zephyr

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.head_ref || github.run_id }}
  cancel-in-progress: true

env:
  # Pinned to the revision zephyr/README.md records as the measured baseline.
  # A step below fails the job if the two ever disagree.
  ZEPHYR_REVISION: 26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0

jobs:
  selftest:
    # Exercises the assertion parsers against checked-in fixtures. Needs no
    # Zephyr workspace, so it fails in seconds rather than behind a
    # multi-gigabyte west checkout.
    runs-on: ubuntu-latest

    name: ubuntu / host / zephyr selftest

    steps:
      - uses: actions/checkout@v7
        with:
          submodules: true

      - name: ci-build.sh --self-test
        run: zephyr/scripts/ci-build.sh --self-test

  build:
    # Builds every target in the ci-build.sh table. Gated on selftest so a
    # broken parser does not burn a full west checkout first.
    needs: selftest

    runs-on: ubuntu-latest

    name: ubuntu / host / zephyr build

    env:
      ZEPHYR_TOOLCHAIN_VARIANT: host
      ZEPHYR_BASE: ${{ github.workspace }}/zephyrproject/zephyr
      # Shared so the eight builds do not each pay a full release build of
      # pico-de-gallo-ffi. If Corrosion overrides it, the only cost is time.
      CARGO_TARGET_DIR: /tmp/pdg-cargo-target

    steps:
      - uses: actions/checkout@v7
        with:
          submodules: true

      - name: Assert the pinned revision matches zephyr/README.md
        run: |
          if ! grep -qF "$ZEPHYR_REVISION" zephyr/README.md; then
            echo "zephyr/README.md does not record $ZEPHYR_REVISION" >&2
            echo "The workflow pin and the README must agree." >&2
            exit 1
          fi

      - name: Install build dependencies
        run: |
          sudo apt-get update
          sudo apt-get install --no-install-recommends -y \
            ninja-build gperf device-tree-compiler python3-venv libmagic1

      - name: Install west
        # A venv, not a bare pip install: ubuntu-latest ships a PEP 668
        # externally-managed Python, where `pip install west` hard-fails. This
        # also matches the setup zephyr/README.md documents.
        run: |
          python3 -m venv "$HOME/zephyr-venv"
          "$HOME/zephyr-venv/bin/pip" install --upgrade pip west
          echo "$HOME/zephyr-venv/bin" >> "$GITHUB_PATH"

      - name: Install stable
        uses: dtolnay/rust-toolchain@stable

      - name: Restore the west workspace
        id: west-cache
        uses: actions/cache@v4
        with:
          path: zephyrproject
          key: zephyr-ws-${{ env.ZEPHYR_REVISION }}-v1

      - name: Initialise the west workspace
        if: steps.west-cache.outputs.cache-hit != 'true'
        run: |
          west init -m https://github.com/zephyrproject-rtos/zephyr \
            --mr "$ZEPHYR_REVISION" zephyrproject
          cd zephyrproject
          west update --narrow -o=--filter=blob:none

      - name: Install Zephyr Python dependencies
        working-directory: zephyrproject
        run: west packages pip --install

      - name: Report the Zephyr revision actually checked out
        working-directory: zephyrproject/zephyr
        run: |
          git --no-pager log -1 --format='%H %cI %s'
          test "$(git rev-parse HEAD)" = "$ZEPHYR_REVISION"

      - name: Build the Zephyr module
        working-directory: zephyrproject
        run: |
          "$GITHUB_WORKSPACE/zephyr/scripts/ci-build.sh" \
            --build-root /tmp/pdg-ci \
            --summary "$GITHUB_STEP_SUMMARY"
```

- [ ] **Step 2: Normalise line endings before linting**

CRLF breaks `actionlint` with `unexpected character $'\r'`, so normalise first or the lint result is meaningless.

```powershell
dos2unix .github/workflows/zephyr.yml
$b = [IO.File]::ReadAllBytes((Resolve-Path .github/workflows/zephyr.yml))
"CR=$(($b | Where-Object {$_ -eq 13}).Count)"
```
Expected: `CR=0`.

- [ ] **Step 3: Run actionlint**

```powershell
& 'C:\Users\febalbi\AppData\Local\Temp\opencode\tools\actionlint\actionlint.exe' -color=false
"exit=$LASTEXITCODE"
```
Expected: exit 0, no output. The baseline across the ten pre-existing workflows is clean, so any finding belongs to this file.

- [ ] **Step 4: Record the pinned SHA in the README, so this commit stands alone**

The workflow's agreement guard fails against the current README. AGENTS.md rule
9 requires each commit to build cleanly on its own, so the README line lands
here rather than in Task 5.

`zephyr/README.md:26` currently reads, in the "Zephyr revision" table row:

> The measured build environment was `main` at `v4.4.0-6123-g26f811ee9d0`.

Replace that first sentence with:

> The measured build environment was `main` at `v4.4.0-6123-g26f811ee9d0d`, commit `26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0`. CI pins that exact commit; `.github/workflows/zephyr.yml` fails if this line and its `ZEPHYR_REVISION` disagree.

Leave the rest of the row — the API dependencies and the "verified baseline
rather than an asserted minimum" sentence — unchanged. Note the corrected
`g26f811ee9d0d` spelling: the existing text truncates the `git describe` output
by one character relative to
`docs/superpowers/specs/2026-08-17-zephyr-mfd-m1-parent.md:242`.

Then prove the guard is real rather than vacuous, by running both halves of it:

```powershell
# The guard's own grep, as the workflow runs it.
Select-String -Path zephyr/README.md -Pattern '26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0' -Quiet
# And that the workflow carries the same literal.
Select-String -Path .github/workflows/zephyr.yml -Pattern '26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0' -Quiet
```
Expected: `True` twice. If either is `False` the guard is broken, not satisfied.

- [ ] **Step 5: Commit**

```powershell
dos2unix .github/workflows/zephyr.yml zephyr/README.md
git add .github/workflows/zephyr.yml zephyr/README.md
git commit -m "ci(zephyr): Add the build-only Zephyr module workflow" -m "A fast selftest job runs the assertion parsers against checked-in
fixtures with no Zephyr workspace, and gates the heavy build job so a
broken parser does not burn a multi-gigabyte west checkout first.

No Zephyr SDK is installed. ZEPHYR_TOOLCHAIN_VARIANT=host builds
native_sim with the host compiler, and native/64 avoids gcc-multilib,
which removes the most expensive part of a conventional Zephyr CI
setup and is why a plain runner suffices.

west is installed into a venv rather than with a bare pip install,
because ubuntu-latest ships a PEP 668 externally-managed Python where
the latter hard-fails. This also matches zephyr/README.md.

The pinned revision is asserted against zephyr/README.md by a step that
fails the job when they disagree, so issue #130's second acceptance
criterion is a test rather than a promise. A later step additionally
checks that the revision actually checked out matches the pin, so a
stale cache entry cannot silently serve a different tree.

Refs: #130" -m "Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 5: Documentation

Makes every affected document true. Per AGENTS.md §15.1's carve-out, `zephyr/` has no book chapter, so `zephyr/README.md` plus `zephyr/CHANGELOG.md` satisfies the parity rule and **no `book/src/**` change is required**.

**Files:**
- Modify: `zephyr/README.md` (add the CI section; the SHA line already landed in Task 4)
- Modify: `zephyr/CHANGELOG.md`
- Modify: `AGENTS.md` (four sites)

**Interfaces:**
- Consumes: the SHA line Task 4 wrote to `zephyr/README.md`.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Confirm the pinned SHA is already recorded**

Task 4 added the SHA line to `zephyr/README.md:26` so that its own commit would
stand alone. Verify it survived and do not add it twice:

```powershell
Select-String -Path zephyr/README.md -Pattern '26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0' | ForEach-Object { "$($_.LineNumber): $($_.Line.Trim())" }
```
Expected: exactly one match. If there are zero, Task 4 regressed — restore it
before continuing. If there are two, remove the duplicate.

- [ ] **Step 2: Add a CI section to the README**

Add a new section after the build instructions. It must state the build-only limit explicitly, because a green check is exactly the thing a reader will be tempted to over-read.

```markdown
## Continuous integration

`.github/workflows/zephyr.yml` builds this module on every pull request that
touches `zephyr/`, `crates/pico-de-gallo-ffi/`, `crates/pico-de-gallo-internal/`
or either root Cargo file. It pins Zephyr to the commit recorded above and
drives `zephyr/scripts/ci-build.sh`, which builds eight targets: the two viable
samples, the two IS31 samples (asserted to fail exactly as they do at baseline),
and the four M5 test applications.

To reproduce a CI failure locally, with a Zephyr workspace already set up:

```bash
export ZEPHYR_BASE=~/zephyrproject/zephyr
export ZEPHYR_TOOLCHAIN_VARIANT=host
zephyr/scripts/ci-build.sh --targets i2c_bridge
```

`--self-test` runs the assertion parsers against checked-in fixtures and needs
no Zephyr workspace at all.

**This gate is build-only.** It never runs a produced binary, because doing so
reaches `gallo_init_strict()` and needs an attached board. A green run means the
module still compiles and links — it says nothing about whether it still works
against hardware. That remains `tests/pdg_mfd_m5/run-m5.sh`, run by hand with a
board and the physical jumpers in place.
```

- [ ] **Step 3: Add the CHANGELOG entry**

Under the `Unreleased` heading in `zephyr/CHANGELOG.md`, in an `### Added` subsection (create it if absent), following the file's existing Keep a Changelog style:

```markdown
- CI gate (`.github/workflows/zephyr.yml`) building the module against a pinned
  Zephyr revision on `native_sim/native/64`, driven by
  `zephyr/scripts/ci-build.sh`. Covers the two viable samples, baseline-failure
  assertions for the two IS31 samples, and the four M5 test applications.
  Build-only: no produced binary is executed, so this adds no runtime coverage.
  ([#130](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/130))
```

- [ ] **Step 4: Update AGENTS.md §5.3**

Append one row to the table that currently ends at `nostd.yml` (line 231):

```markdown
| `zephyr.yml`       | Builds the Zephyr module on `native_sim/native/64` against a pinned Zephyr revision. Build-only.|
```

- [ ] **Step 5: Update AGENTS.md §5.4**

Insert after the `nostd.yml` row (line 238):

```markdown
| `zephyr.yml`              | Push to `main`, PRs (path-filtered)| Builds `zephyr/` for `native_sim/native/64` at a pinned Zephyr revision: 2 samples pass, 2 IS31 samples asserted to fail at baseline, 4 M5 apps. **Build-only** |
```

- [ ] **Step 6: Rewrite AGENTS.md §15.1 lines 907-913**

Replace the whole "Neither gate is automatic." paragraph with:

```markdown
**Both gates now run in CI, within limits.** `.github/workflows/zephyr.yml`
builds the module on every PR touching `zephyr/`, `crates/pico-de-gallo-ffi/`,
`crates/pico-de-gallo-internal/` or either root Cargo file, so
`-Werror=switch` and those `_Static_assert`s fire automatically. The
`_Static_assert`s only compile in the M5 targets, which the gate builds
for exactly that reason.

Two limits still bind. The workflow is **path-filtered**, so a change
outside those paths does not run it. And it is **build-only** — it never
executes a produced binary, because that reaches `gallo_init_strict()`
and needs an attached board. So a green run is evidence that `zephyr/`
still *compiles and links*; it is not evidence that it still *works*.
Behavioural claims still require the manual, board-attached
`zephyr/tests/pdg_mfd_m5/run-m5.sh` procedure.
```

Do not overcorrect. The distinction the original paragraph protected — compiling is not working — remains true and is the reason for the second paragraph.

- [ ] **Step 7: Rewrite AGENTS.md §15.1 checklist item 7 (lines 957-960)**

Replace with:

```markdown
7. FFI or wire-protocol changes name the `zephyr/` consumer they
   affect, or state in the PR body that none is affected. A green
   `zephyr.yml` run is acceptable evidence that the consumer still
   compiles and links. It is **not** evidence that behaviour is
   unchanged, and it does not run at all if the PR touches none of
   that workflow's filtered paths — check that it actually ran.
```

- [ ] **Step 8: Verify the docs**

```powershell
dos2unix zephyr/README.md zephyr/CHANGELOG.md AGENTS.md

# The workflow's agreement guard must now pass.
Select-String -Path zephyr/README.md -Pattern '26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0' -Quiet

# No stale claim survives anywhere.
Select-String -Path AGENTS.md -Pattern 'No CI job builds|Neither gate is automatic|Until #130 lands'
```
Expected: `True`, then **no matches**.

- [ ] **Step 9: Commit**

```powershell
git add zephyr/README.md zephyr/CHANGELOG.md AGENTS.md
git commit -m "docs(repo,zephyr): Document the Zephyr CI gate and its limits" -m "AGENTS.md section 15.1 asserted that no CI job builds the Zephyr module
and that a green CI run is not evidence the module still builds. Both
become false, and the reviewer checklist told reviewers to reject 'CI
is green' outright.

The rewrite deliberately does not overcorrect. Two limits still bind
and are now stated instead: the workflow is path-filtered, so it does
not run on every PR, and it is build-only, so a green run is evidence
the module compiles and links and is not evidence it works. The
distinction the original paragraph protected survives; only the
'nothing builds it at all' claim goes.

The README gains a section documenting how to reproduce a CI failure
locally and restating the build-only limit, since a green check is
exactly what a reader will be tempted to over-read.

No book change: AGENTS.md section 15.1's carve-out makes
zephyr/README.md and zephyr/CHANGELOG.md authoritative for
Zephyr-only changes, and nothing here alters what the book describes.

Refs: #130" -m "Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 6: Push, verify, and run the mutation check

The only task that produces evidence rather than code. Nothing here is merged except the branch as it stands at the end.

**Files:** none created or modified permanently.

**Interfaces:**
- Consumes: everything.
- Produces: a green run URL, a red mutation run URL, and a draft PR.

- [ ] **Step 1: Final local gate**

```powershell
& 'C:\Users\febalbi\AppData\Local\Temp\opencode\tools\actionlint\actionlint.exe' -color=false; "actionlint=$LASTEXITCODE"
& 'C:\Users\febalbi\AppData\Local\Temp\opencode\tools\shellcheck\shellcheck.exe' -S warning zephyr/scripts/ci-build.sh; "shellcheck=$LASTEXITCODE"
& 'C:\Program Files\Git\bin\bash.exe' -lc "cd /c/Users/febalbi/workspace/pico-de-gallo/.worktrees/issue-130-zephyr-ci && zephyr/scripts/ci-build.sh --self-test"; "selftest=$LASTEXITCODE"
git status --short
git log --oneline main..HEAD
```
Expected: all three exit 0; clean tree; the commits from Tasks 1-5 plus the four planning commits.

Also confirm no CRLF slipped into any touched file:
```powershell
git ls-files -z | ForEach-Object { $_ } | Where-Object { $_ } | ForEach-Object {
  $p = $_; if (Test-Path -LiteralPath $p -PathType Leaf) {
    $b = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $p))
    if ($b -contains 13) { "CRLF: $p" } } }
```
Expected: no output for any file this branch touched.

- [ ] **Step 2: Push and open a draft PR**

AGENTS.md §11 wants a draft PR first, with CI allowed to run before review is requested.

Write the body to `C:\Users\febalbi\AppData\Local\Temp\opencode\pr-130-body.md`:

```markdown
Closes #130.

Nothing in CI built, compiled or lints `zephyr/`. Because the Zephyr drivers
call `gallo_*` through the cbindgen-generated header, an FFI change could break
them while every gate stayed green: `check.yml` builds the FFI crate, which
still compiles, cbindgen regenerates the header, and the Zephyr translation unit
that consumed the removed symbol is never compiled by anyone.

Two compile-time gates already existed in the tree and never fired:
`-Werror=switch` over the `Status`-to-`errno` mapping in
`zephyr/drivers/common/common.c`, and the eight `_Static_assert`s pinning the
mirrored FFI enums in `zephyr/tests/pdg_mfd_m5/common/m5_bottom.c:37-51`.
AGENTS.md §15.1 said of both that they "only fire when a human runs a Zephyr
build by hand". This makes them fire.

## What it does

A `selftest` job runs the assertion parsers against checked-in fixtures with no
Zephyr workspace (seconds). A `build` job, gated on it, pins Zephyr to
`26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0` and builds eight targets:

| Target | Expected |
|---|---|
| `i2c_bridge`, `spi_nor_id` | build clean |
| `spi_bridge`, `combined_i2c_spi_bridge` | fail *exactly* at the native_simulator runner link, attributable to `is31fl3743b@0` |
| the four `pdg_mfd_m5` applications | build clean |

Attribution resolves the ordinal against that build's own
`devicetree_generated.h`, never a literal — M4 A-11 recorded ordinals 49 and 50
for the two samples, so cross-sample identity must not be asserted.

## BUILD ONLY

No produced binary is ever executed: no `west build -t run`, no `zephyr.exe`, no
`run-m5.sh`. Running a `native_sim` image reaches `gallo_init_strict()` at
`zephyr/drivers/common/gallo_registry.c:174`, which opens USB and needs a board.
Linking merely resolves that symbol.

**A green run means `zephyr/` still compiles and links. It does not mean it
still works.** Behavioural regressions — the AGENTS.md §13.17 rows dated
2026-08-17 and 2026-08-19 — were found on hardware and would still need
hardware. I have deliberately *not* adopted issue #130's claim that "compile
coverage alone would have caught both prior regressions"; as far as I can tell
that is overstated, and the spec says so.

## Two limits worth knowing before this becomes a required check

1. **Path-filtered**, so it reports *skipped* on unrelated PRs. If it is made a
   required status check, GitHub blocks merges on skipped required checks unless
   a companion no-op job is added. That configuration choice is deliberately
   left to the maintainer rather than made here.
2. **Pin rot.** Pinning to the only revision this module has ever been measured
   against means upstream API drift is invisible until someone bumps the pin. A
   weekly non-blocking `main` tracker was considered and deferred; #130's
   acceptance criteria say nothing about upstream drift.

## Mutation check

MUTATION_EVIDENCE_PLACEHOLDER

## Docs

`zephyr/README.md`, `zephyr/CHANGELOG.md`, and four sites in AGENTS.md (§5.3
table, §5.4 catalog, §15.1 "Neither gate is automatic", §15.1 checklist item 7)
— all four asserted that nothing builds this module.

No `book/src/**` change: AGENTS.md §15.1's carve-out makes `zephyr/README.md`
and `zephyr/CHANGELOG.md` authoritative for Zephyr-only changes, and nothing
here alters what the book describes.

Spec: `docs/superpowers/specs/2026-08-26-zephyr-ci-build-gate-design.md`
Plan: `docs/superpowers/plans/2026-08-26-zephyr-ci-build-gate.md`
```

Then:

```powershell
git push -u origin felipebalbi/ci/issue-130-zephyr-build-gate
gh pr create --repo OpenDevicePartnership/pico-de-gallo --draft `
  --title "ci(zephyr): Add a build-only CI gate for the Zephyr module" `
  --body-file 'C:\Users\febalbi\AppData\Local\Temp\opencode\pr-130-body.md'
```

- [ ] **Step 3: Wait for the first run and fix forward**

```powershell
gh run watch --repo OpenDevicePartnership/pico-de-gallo
```

This is the first execution of both new files. Expect iteration. Fix forward with ordinary commits; do not amend. Per spec §7.3 the likely failures are, in order: a missing apt package, west/`ZEPHYR_BASE` resolution, the native_simulator object suffix in `assert_pass`, and cache behaviour.

Do not proceed to Step 4 until the run is green.

- [ ] **Step 4: Run the mutation check**

Record the green SHA first:
```powershell
$clean = git rev-parse HEAD; $clean
```

Rename the symbol that only `zephyr/tests/pdg_mfd_m5/common/m5_bottom.c:94` calls. Verified: `gallo_gpio_unsubscribe` has exactly one call site in `zephyr/`, so a green result under this mutation would also prove the M5 targets vacuous.

In `crates/pico-de-gallo-ffi/src/lib.rs`, rename the exported `gallo_gpio_unsubscribe` to `gallo_gpio_unsubscribe_MUTANT`. Commit and push:

```powershell
git commit -am "MUTATION - DO NOT MERGE: rename gallo_gpio_unsubscribe"
git push
gh run watch --repo OpenDevicePartnership/pico-de-gallo
```

Expected: the `build` job **fails**, at the native_simulator runner link, on an undefined reference to `gallo_gpio_unsubscribe`, in the `m5_reset`, `m5_acceptance` and `m5_teardown` targets. Record the run URL.

If it passes, the gate is vacuous. Stop, and treat that as a defect in `assert_pass` rather than a curiosity.

- [ ] **Step 5: Restore and record**

```powershell
git reset --hard $clean
git push --force-with-lease
git log --oneline -1
```
Expected: HEAD back at the green SHA, no mutation commit on the branch.

Add the mutation run URL to the PR body, replacing `MUTATION_EVIDENCE_PLACEHOLDER` with the recorded evidence:

```markdown
Per acceptance criterion 3. Renamed `gallo_gpio_unsubscribe` in
`pico-de-gallo-ffi` — chosen because it has **exactly one** call site in
`zephyr/`, at `tests/pdg_mfd_m5/common/m5_bottom.c:94`, and none in any sample.
A green result under that mutation would therefore have proven the four M5
targets vacuous, so this is simultaneously a check on the non-vacuity
assertions.

- Clean run (green): <URL>
- Mutated run (red): <URL> — failed in `m5_reset`, `m5_acceptance`,
  `m5_teardown` on an undefined reference to `gallo_gpio_unsubscribe` at the
  native_simulator runner link.

The mutation was reverted with `git reset --hard` + `git push
--force-with-lease` and is not present on this branch.
```

Verify the placeholder is gone:
```powershell
gh pr view --repo OpenDevicePartnership/pico-de-gallo --json body --jq '.body' |
  Select-String -Pattern 'MUTATION_EVIDENCE_PLACEHOLDER' -Quiet
```
Expected: `False`.

- [ ] **Step 6: Confirm the branch is clean and mark ready**

```powershell
git log --oneline main..HEAD
gh pr ready --repo OpenDevicePartnership/pico-de-gallo
```
Expected: no commit mentioning MUTATION; the final run green.

---

## Acceptance criteria (from issue #130)

| Criterion | Satisfied by |
|---|---|
| A workflow builds at least the two viable samples for `native_sim/native/64` on PRs touching `zephyr/`, `crates/pico-de-gallo-ffi/`, `crates/pico-de-gallo-internal/` | Task 4. Exceeded: eight targets, and `Cargo.toml`/`Cargo.lock` added to the filter because Corrosion imports with `LOCKED` |
| The pinned Zephyr revision is recorded in the workflow and in `zephyr/README.md`, and the two agree | Task 4 Steps 1 and 4 — the workflow `env:`, the README line, and the guard that fails the job when they disagree, all in one commit. Enforced mechanically, not by convention |
| A deliberate FFI symbol rename fails the job (do not merge the mutation) | Task 6 Steps 4-5 |
| `actionlint` passes; the workflow file is LF-only per AGENTS.md §3 | Task 4 Steps 2-3, re-checked in Task 6 Step 1 |
| Item 2 of Suggested scope: assert the IS31 samples fail identically to baseline | Task 3 `assert_basefail`, per M1 §6.2 and M6 §3.5 |
| Item 3: add `zephyr/tests/pdg_mfd_m5` only if it runs without hardware | Included as build-only. They need a board at run time, which this gate never does |

## Out of scope

Per spec §8, and not to be added by this plan: the weekly non-blocking Zephyr `main` tracker; twister metadata and hardware-free unit tests (#109); any hardware run.
