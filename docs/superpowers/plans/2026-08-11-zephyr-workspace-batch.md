# Zephyr Workspace Batch (#103, #107, #108) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up a Zephyr workspace, then close the three Zephyr-module issues that cannot be verified without one: a NULL-function-pointer crash on the SPI async/RTIO paths (#103), the module building against a crates.io tarball instead of the in-tree FFI crate (#107), and the build assuming a Linux host (#108).

**Architecture:** Nothing here changes driver logic. #103 is a Kconfig guard that makes an unimplemented driver op a config-time refusal instead of a runtime jump through NULL. #107 and #108 both rewrite the same block of `zephyr/CMakeLists.txt` — #107 redirects Corrosion at the in-tree manifest, #108 removes the ELF-only assumptions layered on top. They are sequenced so each lands independently and the tree builds after every commit.

**Tech Stack:** Zephyr `main` (see §Constraints — no released Zephyr works), `native_sim/native/64`, CMake ≥3.20, Corrosion 0.5.2, Rust 1.90+, host GCC.

**Issues:** [#103](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/103), [#107](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/107), [#108](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/108)

**Branch:** `zephyr`, currently 4 commits ahead of `upstream/main` (`f92dd100`). Nothing pushed.

---

## Where things stand

The `zephyr/` module is **not upstream**. It exists only on the local `zephyr`
branch, rebased from `bjackson312006/pico-de-gallo`. Do not look for it on
`main`.

```
20055ed7  feat(ffi): Export transfer limits and config enums     <- closes most of #106
a4d5a3ae  fix(zephyr): Correct two I2C log format strings        <- closes #105
e89f6f96  feat(zephyr): Add I2C and SPI bridge drivers           <- the module itself
a0a45059  docs(repo): Allow the `zephyr` commit scope
────────  upstream/main (f92dd100)
```

Recovery refs, delete once you are confident: `zephyr-prerebase` (Blake's
original), `zephyr-14commits` (rebased, pre-squash).

Ten issues came out of the review, #101–#110. This plan covers **only**
#103, #107, #108 — the three that are blocked on having a Zephyr build.
The others (#101, #102, #104 I2C/SPI correctness; #109 tests/CI; #110 docs
governance) are out of scope here.

---

## Ground rules

Project policy from `AGENTS.md`. Several of these differ from Rust/Zephyr
defaults and have bitten this repo before.

1. **Never bump `[package].version`** (§4 rule 12). Version bumps are a
   separate, deliberate `chore(release):` commit made by a maintainer. This
   applies even though Task 3 edits `crates/pico-de-gallo-ffi/Cargo.toml`.
2. **AI attribution trailers on every commit** (§4 rule 7), and **never**
   `Signed-off-by:` — only humans certify the DCO:
   ```text
   Assisted-by: OpenCode:claude-opus-5
   Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
   ```
3. **Commit subject ≤50 chars**, capitalized, imperative, no trailing period
   (§10). Scope is one of `internal lib hal ffi application mcp pyco firmware
   repo zephyr` — `zephyr` was added by `a0a45059`.
4. **LF line endings** on every file touched (§3). Check with
   `grep -cU $'\r' <file>` — expect 0.
5. **Each commit builds cleanly on its own** (§4 rule 9). No squash-merge.
6. **Book parity is a hard rule** (§15.1). Task 3 touches the FFI crate's
   build surface; if you change anything user-visible about how the library
   is produced, `book/src/crates/ffi.md` §"Building and Linking" changes in
   the same commit.
7. **Do not push or force-push** without asking (§4 rule 8).
8. **Commit `Cargo.toml` and `Cargo.lock` together** (§4 rule 3). Task 3
   edits a `Cargo.toml`; run `cargo check --locked` at the repo root and stage
   the lock if it moves.

---

## Constraints discovered during review

These are established facts, verified. Do not re-litigate them without new
evidence.

- **No released Zephyr works.** `pdg_spi.c:220,240` read `config->cs.setup_ns`
  and `config->cs.hold_ns`. Checked against `v4.0.0`, `v4.1.0`, `v4.2.0` and
  `main`: those fields exist **only on `main`**. (`spi_cs_is_gpio()` at
  `pdg_spi.c:184` *does* exist in all releases — an earlier review claim that
  it did not was wrong.) Verify for your checkout with:
  ```bash
  curl -sf https://raw.githubusercontent.com/zephyrproject-rtos/zephyr/main/include/zephyr/drivers/spi.h | grep -c setup_ns
  ```
- **Only one of three samples is buildable.** `spi_bridge` and
  `combined_i2c_spi_bridge` need an IS31FL3743B LED driver that is not
  upstream (`zephyr/README.md:50-53`). **`samples/i2c_bridge` is the only
  sample you can build.** Use it for every build check in this plan.
- **`west` on this machine is broken.** It is installed system-wide against
  `/usr/bin/python` (3.14.6) and cannot import `pykwalify`:
  ```
  ModuleNotFoundError: No module named 'pykwalify'
  ```
  Task 0 sidesteps this with a virtualenv rather than repairing the system
  install. Python 3.14 is very new; if Zephyr's tooling breaks on it, fall
  back to an older interpreter in the venv.
- **The module needs a 64-bit target.** `zephyr/Kconfig:5-6` has
  `depends on ARCH_POSIX` + `depends on 64BIT`, because
  `corrosion_set_hostbuild()` forces the rustc host triple. Always build
  `native_sim/native/64`, never bare `native_sim` (which is 32-bit).
- **cbindgen fails silently in two ways** — now documented in `AGENTS.md` §8.
  Types unreferenced by an exported signature are pruned unless listed in
  `[export] include`; const initializers that are not literals emit nothing.
  Relevant if Task 3 tempts you to touch header generation.

---

## File structure

| File | Change | Responsibility |
|---|---|---|
| `zephyr/drivers/spi/Kconfig` | Modify: after `:9` | Task 1 — refuse to build under async/RTIO |
| `zephyr/CMakeLists.txt` | Modify: delete `:53-67`, rewrite `:71-77` | Task 2 — source the FFI in-tree |
| `zephyr/CMakeLists.txt` | Modify: `:69-78`, `:85-92` | Task 3 — remove ELF-only assumptions |
| `crates/pico-de-gallo-ffi/Cargo.toml` | Modify: `:21-22` | Task 3 — add `staticlib`, **only if** Task 3a succeeds |
| `book/src/crates/ffi.md` | Modify: §"Building and Linking" | Task 3 — only if the produced artifacts change |

No new files.

---

### Task 0: Stand up a Zephyr workspace and get a baseline build

Nothing else in this plan can be verified without this. Do it first and do
not skip the baseline — you need to know the module builds *before* you
change it, or you cannot attribute a later failure.

**Files:** none (environment only; the workspace lives outside the repo)

- [ ] **Step 1: Create a virtualenv and install west**

The system `west` is broken (see Constraints). Do not try to fix it.

```bash
python3 -m venv ~/zephyr-venv
~/zephyr-venv/bin/pip install --upgrade pip
~/zephyr-venv/bin/pip install west
```

- [ ] **Step 2: Initialise the workspace on Zephyr `main`**

```bash
source ~/zephyr-venv/bin/activate
west init ~/zephyrproject
cd ~/zephyrproject
west update
west zephyr-export
```

Expected: `~/zephyrproject/zephyr/` exists. This takes a while and pulls
several GB.

If `west init` defaults to a release tag rather than `main`, force it:
`west init -m https://github.com/zephyrproject-rtos/zephyr --mr main ~/zephyrproject`

- [ ] **Step 3: Install Python dependencies**

```bash
cd ~/zephyrproject
west packages pip --install
```

If that subcommand does not exist on your west version, use the older form:

```bash
pip install -r ~/zephyrproject/zephyr/scripts/requirements.txt
```

- [ ] **Step 4: Confirm `setup_ns` is present**

This is the constraint that forces `main`. If it is missing, the module
cannot compile and everything downstream will look broken for the wrong
reason.

```bash
grep -c setup_ns ~/zephyrproject/zephyr/include/zephyr/drivers/spi.h
```

Expected: `1` or more. If `0`, your checkout is not on `main` — go back to
Step 2.

- [ ] **Step 5: Build the baseline**

`native_sim` uses the host toolchain, so no Zephyr SDK is needed for this
board.

```bash
source ~/zephyr-venv/bin/activate
export ZEPHYR_BASE=~/zephyrproject/zephyr
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
```

Expected: build succeeds and produces `build/zephyr/zephyr.exe`.

> The sample's `CMakeLists.txt` sets `BOARD`/`SHIELD` via `set(... CACHE ...)`,
> but west resolves `BOARD` *before* invoking CMake, so those are inert. Pass
> `-b` and `-DSHIELD=` explicitly, as above. (This is the root of the
> README's broken `west build -t run` instruction; fixing the README belongs
> to #110, not here.)

- [ ] **Step 6: Record the baseline**

Save the output so later failures are attributable:

```bash
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo 2>&1 \
  | tee /tmp/zephyr-baseline.log
grep -E "^\[|error|warning" /tmp/zephyr-baseline.log | tail -20
```

Do not commit anything in this task.

---

### Task 1: Refuse to build the SPI driver under async/RTIO (#103)

`zephyr/drivers/spi/pdg_spi.c:283-286` sets only `.transceive` and
`.release`. Unlike I2C, Zephyr's SPI subsystem does **not** NULL-check the
optional ops:

```c
/* zephyr/include/zephyr/drivers/spi.h:1206 — no NULL check */
return DEVICE_API_GET(spi, dev)->transceive_async(dev, config, tx_bufs, rx_bufs, callback, userdata);
/* zephyr/include/zephyr/drivers/spi.h:1348 — no NULL check */
DEVICE_API_GET(spi, dev)->iodev_submit(dev, iodev_sqe);
/* contrast — zephyr/include/zephyr/drivers/i2c.h:1048 */
if (api->transfer_cb == NULL) { return -ENOSYS; }
```

So `CONFIG_SPI_ASYNC` or `CONFIG_SPI_RTIO` turns any async use into a jump
through NULL. `CONFIG_SPI_RTIO` arrives transitively via
`CONFIG_SENSOR_ASYNC_API`, so an application can enable it without asking.

The driver is inherently blocking (every call is a synchronous USB round
trip), so refusing at config time is the correct fix, not implementing the
ops.

**Files:**
- Modify: `zephyr/drivers/spi/Kconfig`

- [ ] **Step 1: Reproduce the crash**

Confirm the failure is real before fixing it. Add RTIO to the sample and
build:

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- \
  -DSHIELD=pico_de_gallo -DCONFIG_SPI=y -DCONFIG_SPI_RTIO=y
```

Expected *before* the fix: the build **succeeds**. That is the bug — nothing
stops the combination. Note it and move on. (Triggering the actual segfault
needs an application that calls an RTIO API; a build that silently permits
the combination is sufficient evidence.)

- [ ] **Step 2: Add the guard**

Edit `zephyr/drivers/spi/Kconfig`, appending to the `SPI_PICO_DE_GALLO`
block after `select SPI`:

```
config SPI_PICO_DE_GALLO
	bool "Pico de Gallo SPI controller"
	default y
	depends on DT_HAS_ODP_PICO_DE_GALLO_SPI_ENABLED
	depends on ARCH_POSIX
	# Every operation is a blocking USB round trip, so the async and RTIO
	# driver ops are not implemented. Zephyr's SPI subsystem calls
	# transceive_async() and iodev_submit() without a NULL check (unlike
	# I2C, which returns -ENOSYS), so leaving them unset would mean a jump
	# through a NULL function pointer rather than a graceful error.
	depends on !SPI_ASYNC
	depends on !SPI_RTIO
	select SPI
	help
	  Enable the Pico de Gallo SPI controller driver. This driver forwards
	  Zephyr SPI API calls to a Pico de Gallo USB bridge attached to the host
	  running native_sim via the Pico de Gallo C FFI.
```

- [ ] **Step 3: Verify the guard fires**

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- \
  -DSHIELD=pico_de_gallo -DCONFIG_SPI=y -DCONFIG_SPI_RTIO=y
```

Expected *after* the fix: `SPI_PICO_DE_GALLO` is no longer selectable, so the
SPI driver is not compiled. Confirm:

```bash
grep SPI_PICO_DE_GALLO build/zephyr/.config
```

Expected: `# CONFIG_SPI_PICO_DE_GALLO is not set`.

- [ ] **Step 4: Verify the normal build still works**

```bash
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
grep SPI_PICO_DE_GALLO build/zephyr/.config
```

Expected: build succeeds. (`i2c_bridge` does not enable SPI, so the symbol
may be unset here too; the point is that the build is unaffected.)

- [ ] **Step 5: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add zephyr/drivers/spi/Kconfig
git commit -F - <<'EOF'
fix(zephyr): Guard SPI driver against async and RTIO

pdg_spi.c implements only .transceive and .release. Zephyr's SPI
subsystem calls transceive_async() and iodev_submit() through the
driver API without a NULL check -- unlike I2C, which returns -ENOSYS
-- so enabling CONFIG_SPI_ASYNC or CONFIG_SPI_RTIO turned any async
use into a jump through a NULL function pointer. On native_sim that
is a segfault, not an error return.

CONFIG_SPI_RTIO is selected transitively by CONFIG_SENSOR_ASYNC_API
and by several in-tree SPI sensor drivers, so an application could
reach this without asking for it.

Every operation in this driver is a blocking USB round trip, so the
async ops are not merely unimplemented -- they are not meaningful.
Refuse the combination at config time rather than implementing
synchronous stand-ins.

Fixes #103

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

### Task 2: Build against the in-tree FFI crate (#107)

`zephyr/CMakeLists.txt:53-59` downloads `pico-de-gallo-ffi` from crates.io
even though the crate lives in this repository. Consequences:

- Editing `crates/pico-de-gallo-ffi/` has **no effect** on the Zephyr build.
  The constants and enums added by `20055ed7` for #106 are invisible to the
  module until a release is published.
- The pin is invisible to `cargo`, `cargo-deny`, `cargo-semver-checks`, and
  the `lockfile` CI job.
- `:63-67` appends `[workspace]` to the downloaded `Cargo.toml` **after**
  `URL_HASH` verified it, so the integrity check is worthless. It also writes
  into the FetchContent source tree, breaking read-only caches.
- Configure requires network access.

At the time of writing the pinned tarball happens to be byte-identical to the
in-tree source, because the version numbers coincidentally line up at 0.7.1.
That is luck, not a property.

**Files:**
- Modify: `zephyr/CMakeLists.txt`

- [ ] **Step 1: Prove the drift is real**

Before changing anything, demonstrate that in-tree edits do not reach the
build. Check whether the #106 constants are present in the header the Zephyr
build actually uses:

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
find build -name pico_de_gallo.h -exec grep -c GALLO_MAX_TRANSFER_SIZE {} +
```

Expected *before* the fix: `0`, or the file is generated from the downloaded
0.7.1 tarball rather than the working tree. Record which.

- [ ] **Step 2: Delete the download and the manifest mutation**

Remove lines 53–67 of `zephyr/CMakeLists.txt` entirely — the
`FetchContent_Declare(pico_de_gallo_ffi_src ...)`, its
`FetchContent_MakeAvailable`, and the `file(READ ...)` / `file(APPEND ...)`
block that injects `[workspace]`. The in-tree crate is already a workspace
member, so nothing needs injecting.

- [ ] **Step 3: Point Corrosion at the in-tree manifest**

Replace the `corrosion_import_crate` call (currently `:71-77`) with:

```cmake
	# Build the FFI from this repository rather than crates.io, so that
	# edits to crates/pico-de-gallo-ffi/ are picked up by the Zephyr build
	# and so the pin stays visible to cargo-deny, cargo-semver-checks and
	# CI's lockfile job. Also makes offline configure possible.
	corrosion_import_crate(
		MANIFEST_PATH "${ZEPHYR_PICO_DE_GALLO_MODULE_DIR}/crates/pico-de-gallo-ffi/Cargo.toml"
		CRATES pico-de-gallo-ffi
		CRATE_TYPES cdylib
		PROFILE release
		LOCKED
	)
```

`ZEPHYR_PICO_DE_GALLO_MODULE_DIR` is provided by Zephyr's module system and
already used at `zephyr/Kconfig:17`, so it is known-good in this build.

- [ ] **Step 4: Allow offline Corrosion**

Corrosion itself is still fetched from GitHub. Leave the `FetchContent` in
place but document the escape hatch, adding above the
`FetchContent_Declare(Corrosion ...)` block:

```cmake
	# For offline or air-gapped builds, point FetchContent at a local
	# Corrosion checkout instead of the GitHub tarball:
	#   cmake -DFETCHCONTENT_SOURCE_DIR_CORROSION=/path/to/corrosion ...
```

- [ ] **Step 5: Rebuild and prove the in-tree source is used**

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
find build -name pico_de_gallo.h -exec grep -c GALLO_MAX_TRANSFER_SIZE {} +
```

Expected *after* the fix: at least `1`. The #106 constants now reach the
module, which is the whole point.

- [ ] **Step 6: Prove edits propagate (mutation check)**

This is the actual regression guard. Temporarily change the in-tree constant
and confirm the Zephyr build sees it:

```bash
cd /home/balbi/workspace/pico-de-gallo
sed -i 's/^pub const GALLO_MAX_BATCH_OPS: usize = 64;/pub const GALLO_MAX_BATCH_OPS: usize = 65;/' \
    crates/pico-de-gallo-ffi/src/lib.rs
```

Expected: the build now **fails**, because `20055ed7` guards that constant
with `const _: () = assert!(GALLO_MAX_BATCH_OPS == lib::MAX_BATCH_OPS);`.
A build failure proves the in-tree source is being compiled. Restore:

```bash
git checkout crates/pico-de-gallo-ffi/src/lib.rs
```

- [ ] **Step 7: Confirm no network is needed**

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
```

with Corrosion already populated in the build tree. If configure still
reaches crates.io, something was missed in Step 2.

- [ ] **Step 8: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add zephyr/CMakeLists.txt
git commit -F - <<'EOF'
fix(zephyr): Build the FFI from the tree, not crates.io

The module downloaded pico-de-gallo-ffi from static.crates.io at
configure time, despite living in the same repository as that crate.
Edits under crates/pico-de-gallo-ffi/ therefore had no effect on the
Zephyr build, and the pin was invisible to cargo, cargo-deny,
cargo-semver-checks and CI's lockfile job.

The SHA256 on the download was also defeated by the code immediately
after it, which appended a [workspace] table to the fetched
Cargo.toml once the hash had already been verified -- and wrote into
the FetchContent source tree while doing so. The in-tree crate is
already a workspace member, so no injection is needed.

Point Corrosion at the in-tree manifest and drop both the download
and the mutation. Configure no longer needs network access for the
FFI; note the FETCHCONTENT_SOURCE_DIR_CORROSION escape hatch for
Corrosion itself.

Fixes #107

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

### Task 3: Remove the Linux-only assumptions (#108)

`zephyr/CMakeLists.txt` hardcodes an ELF shared object and an ELF linker
flag:

```cmake
# :88
set(_pdg_ffi_library "${_pdg_ffi_output_dir}/libpico_de_gallo_ffi.so")
# :90
target_link_options(native_simulator INTERFACE "-Wl,-rpath,${_pdg_ffi_output_dir}")
```

macOS is a first-class `native_sim` host and produces `.dylib`, so line 88
names a file that never exists and the link fails with no hint as to why.

This task has two stages. **Do Stage A first and commit it.** Stage B is a
larger change with a real risk of not working; if it fails, Stage A has
already fixed the reported bug.

**Files:**
- Modify: `zephyr/CMakeLists.txt`
- Modify (Stage B only): `crates/pico-de-gallo-ffi/Cargo.toml`
- Modify (Stage B only, if artifacts change): `book/src/crates/ffi.md`

#### Stage A — stop hardcoding the filename

- [ ] **Step 1: Find Corrosion's target name**

Corrosion creates an imported target for the crate. Confirm its name before
relying on it:

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
grep -rn "pico_de_gallo_ffi" build/CMakeFiles/*/  2>/dev/null | head
cmake --build build --target help 2>/dev/null | grep -i pico
```

Expected: a target such as `pico_de_gallo_ffi` and/or
`cargo-build_pico_de_gallo_ffi`. Record the exact name — the next step
depends on it.

- [ ] **Step 2: Replace the hardcoded path with a generator expression**

Substitute the target name you found in Step 1:

```cmake
	set(_pdg_ffi_include_dir
		"${CMAKE_CURRENT_BINARY_DIR}/corrosion_generated/cbindgen/pico_de_gallo_ffi/include")
	target_compile_options(native_simulator INTERFACE "-I${_pdg_ffi_include_dir}")
	set_property(TARGET native_simulator APPEND PROPERTY RUNNER_LINK_LIBRARIES
		"$<TARGET_FILE:pico_de_gallo_ffi>")
	if(NOT CMAKE_SYSTEM_NAME STREQUAL "Windows")
		target_link_options(native_simulator INTERFACE
			"-Wl,-rpath,$<TARGET_FILE_DIR:pico_de_gallo_ffi>")
	endif()
```

`$<TARGET_FILE:...>` resolves to `.so`, `.dylib` or `.dll` per platform, so
the extension is no longer assumed.

- [ ] **Step 3: Rebuild and confirm it still links on Linux**

```bash
cd /home/balbi/workspace/pico-de-gallo/zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
ldd build/zephyr/zephyr.exe | grep pico_de_gallo
```

Expected: build succeeds, and `ldd` shows `libpico_de_gallo_ffi.so` resolving
to the build directory (proving the rpath still works).

- [ ] **Step 4: Commit Stage A**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add zephyr/CMakeLists.txt
git commit -F - <<'EOF'
fix(zephyr): Stop assuming an ELF host for the FFI

The build named libpico_de_gallo_ffi.so literally and appended an
ELF-only -Wl,-rpath. macOS is a supported native_sim host and builds
a .dylib, so the hardcoded path referred to a file that never exists
and the link failed with no indication of the cause. On Windows the
rpath flag is meaningless.

Use $<TARGET_FILE:...> from the Corrosion-imported target so the
extension follows the platform, and guard the rpath.

Fixes #108

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

#### Stage B — evaluate a static link (optional, higher risk)

A `staticlib` would additionally remove the rpath entirely and the
`TARGET_SUPPORTS_SHARED_LIBS` global save/set/restore at `:69-78`, and give
the native_sim runner a self-contained binary. **It may not work**: a Rust
`staticlib` does not carry its transitive system-library requirements, and
this crate pulls in tokio and nusb, so the link needs `-lpthread -ldl -lm`
and likely `-ludev`. Corrosion's handling of `native-static-libs` varies by
version.

- [ ] **Step 1: Find out what the static link would require**

```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-ffi
cargo rustc --release --crate-type staticlib -- --print native-static-libs 2>&1 \
  | grep -A2 "native-static-libs"
```

Record the list. If it is long or includes anything unusual (`udev`,
`systemd`), **stop and keep Stage A**. Note the finding on #108 and close it
on Stage A alone.

- [ ] **Step 2: If the list is short, try it**

Add `staticlib` to `crates/pico-de-gallo-ffi/Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib", "lib", "staticlib"]
```

This is additive. `release-ffi.yml` uploads explicitly-named files
(`libpico_de_gallo_ffi.so`, `pico_de_gallo_ffi.dll`,
`pico_de_gallo_ffi.dll.lib`), so an extra `.a` does not disturb it —
re-verify by reading `.github/workflows/release-ffi.yml:58-112` before
committing.

Then in `zephyr/CMakeLists.txt`: change `CRATE_TYPES cdylib` to
`CRATE_TYPES staticlib`, delete the `TARGET_SUPPORTS_SHARED_LIBS`
save/set/restore, and delete the `target_link_options(... -rpath ...)` line.

- [ ] **Step 3: Verify, including the lockfile rule**

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo check --workspace --locked          # AGENTS.md §4 rule 3
cd zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
ldd build/zephyr/zephyr.exe | grep -c pico_de_gallo
```

Expected: build succeeds, and `ldd` now reports **0** matches — the FFI is
linked in statically.

If `cargo check --locked` reports the lock is stale, regenerate it and stage
`Cargo.toml` and `Cargo.lock` together. **Do not bump any version.**

- [ ] **Step 4: Update the book if artifacts changed**

Adding a static library changes what the crate produces, so
`book/src/crates/ffi.md` §"Building and Linking" needs a line about it
(§15.1 is a hard rule). If Stage B was abandoned, no book change is needed.

- [ ] **Step 5: Commit Stage B**

```bash
git add crates/pico-de-gallo-ffi/Cargo.toml zephyr/CMakeLists.txt book/src/crates/ffi.md
# add Cargo.lock too if it moved
git commit -F - <<'EOF'
build(ffi,zephyr): Link the FFI statically into native_sim

Producing a staticlib lets the native_sim runner link the FFI
directly, which removes the -Wl,-rpath and the global
TARGET_SUPPORTS_SHARED_LIBS save/set/restore that existed only to
let Zephyr's toolchain files emit a shared object. A self-contained
runner binary is also the right shape for a test executable.

The cdylib is retained, since release-ffi.yml publishes it.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Definition of done

- [ ] `samples/i2c_bridge` builds clean on `native_sim/native/64`.
- [ ] `CONFIG_SPI_RTIO=y` no longer permits `SPI_PICO_DE_GALLO` (#103).
- [ ] The Zephyr build compiles the in-tree FFI — proven by the mutation
      check in Task 2 Step 6, not by inspection (#107).
- [ ] No `.so` literal and no unguarded `-Wl,-rpath` in
      `zephyr/CMakeLists.txt` (#108).
- [ ] `cargo check --workspace --locked` passes at the repo root.
- [ ] Every new commit: subject ≤50 chars, valid scope, `Assisted-by:` +
      `Co-authored-by:`, no `Signed-off-by:`, LF endings.
- [ ] Nothing pushed without asking.

## Out of scope

#101, #102, #104 (I2C/SPI correctness — need real hardware), #109
(tests/CI), #110 (book and AGENTS.md governance). #106 is closed except for
`GALLO_NUM_GPIOS`, which needs `NUM_GPIOS` hoisted from the firmware crate
into `pico-de-gallo-internal` and is tracked separately on that issue.
