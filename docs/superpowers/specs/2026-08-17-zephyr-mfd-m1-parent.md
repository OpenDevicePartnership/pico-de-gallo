# Zephyr MFD restructure M1 — parent specification

Date: 2026-08-17  
Branch baseline: `zephyr` at `970eb3d48b7a`  
Milestone: M1 — additive MFD parent only

## 1. Context, scope, and inventory

M1 adds a Zephyr parent representing one physical USB-attached Pico de Gallo.
It owns one registry reference and exposes its opaque context to later children.
I2C and SPI remain unchanged root siblings and keep opening their own references
until M2. Identical selectors deduplicate to one handle with refcount three.
M1 also rejects ambiguous serial-less multi-parent devicetrees at compile time.

Final inventory:

**Create**

- `zephyr/dts/bindings/mfd/odp,pico-de-gallo.yaml`
- `zephyr/drivers/mfd/pdg_mfd.c`
- `zephyr/drivers/mfd/pdg_mfd.h`
- `zephyr/drivers/mfd/CMakeLists.txt`
- `zephyr/drivers/mfd/Kconfig`
- `zephyr/drivers/common/common_bottom.h`

**Modify**

- `zephyr/drivers/common/common.h`
- `zephyr/drivers/CMakeLists.txt`
- `zephyr/drivers/Kconfig`
- `zephyr/Kconfig`
- `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay`
- `zephyr/CHANGELOG.md`

The common header pair, top-level Kconfig, and changelog amend plan §3; §13
records why. No MFD-specific forwarding C file is needed.

## 2. Parent and child-facing contract

### 2.1 Initialization and diagnostics

`pdg_mfd.c` uses `DT_DRV_COMPAT odp_pico_de_gallo` and
`DEVICE_DT_INST_DEFINE`. Per instance, immutable config stores
`DT_INST_PROP_OR(inst, serial_number, NULL)` and zero-initialized data stores only
`void *ctx`. Registration is `POST_KERNEL` at
`CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY`, with NULL API. There is no parent lock,
per D6, and no metadata cache.

Init calls `pdg_common_bottom_open(config->serial)`. Success stores the borrowed
context and returns zero. Failure leaves `ctx == NULL` and returns `-ENODEV`.
There is no Zephyr teardown; the registry reference lasts for process lifetime,
matching existing static drivers.

Strict open validates firmware metadata and can block for up to **five minutes**.
A disconnected, mismatched, access-denied, or wedged board can make boot appear
hung. The common shim collapses every cause—no device, schema mismatch, selector
conflict, allocation failure, access denial, timeout—into NULL. Cause-preserving
errors are deferred beyond M1.

Registry/FFI diagnostics use host stderr and may not reach Zephyr logs. The
parent's `LOG_ERR` is authoritative and must name `dev->name`, selector mode
(`default` or `explicit`), explicit value when present, and `-ENODEV`. Default
mode must not format a NULL string.

### 2.2 Exact `pdg_mfd.h`

```c
/*
 * Copyright (c) 2026 Open Device Partnership and Contributors
 *
 * SPDX-License-Identifier: MIT
 *
 * Child-facing API for the Pico de Gallo MFD parent.
 */

#ifndef PDG_MFD_H
#define PDG_MFD_H

#include <zephyr/device.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Return the borrowed opaque context owned by a Pico de Gallo parent.
 *
 * This accessor is NULL-safe, but it is not a substitute for checking parent
 * readiness. A child must first require device_is_ready(parent). If readiness
 * is false, the child must log the parent's name and return -ENODEV. Only after
 * that check may it call pdg_mfd_ctx(parent). A NULL result after a successful
 * readiness check is an invariant failure; the child must log it and return
 * -ENODEV. The returned context is borrowed: callers must never close or free
 * it, and it remains valid for the parent's static process lifetime.
 *
 * @param dev Pico de Gallo MFD parent device, or NULL.
 * @return Borrowed opaque context, or NULL when dev is NULL or the parent did
 *         not open successfully.
 */
void *pdg_mfd_ctx(const struct device *dev);

#ifdef __cplusplus
}
#endif

#endif /* PDG_MFD_H */
```

The accessor is out-of-line, preserving private data layout across separate
`zephyr_library()` targets. `void *` is not technically forced: a typed opaque
pointer could use `struct PicoDeGallo;` without the FFI header. It is chosen to
match established `void *ctx` bottom-half APIs and avoid a const/type migration
through every bottom half.

Mandatory child sequence in M2/M3 is: require `device_is_ready(parent)`; on
false, log parent name and return `-ENODEV`; only then call `pdg_mfd_ctx`; treat
subsequent NULL as an invariant failure, log, and return `-ENODEV`; never close
or free the result. The accessor is NULL-safe but not a readiness substitute.

A failed Zephyr device retains static `struct device` and zero-initialized driver
data; its errno is stored in `device_state.init_res`, readiness is false, and
`DEVICE_DT_GET()` still returns the object. Reading failed-parent data safely
yields NULL, not use-after-free. The explicit `dev == NULL` guard remains
mandatory. Future GPIO metadata access can be added without changing this API.

## 3. Selector safety

### 3.1 Temporary sibling rule

Three omitted selectors normalize to `""` and refcount the same entry; three
identical explicit serials do likewise. Mixing omitted and explicit selectors
causes the differing open to return NULL and init to return `-ENODEV`. Current
samples omit selectors everywhere.

The shield parent therefore omits `serial-number`, matching I2C/SPI. Downstream
configuration must either omit it from all three enabled nodes or set the same
explicit value on `pdg0`, `pdg_i2c0`, and `pdg_spi0` until M2. The parent binding
must repeat the existing warning: omitted mode is single-board only; multi-board
setups need a unique explicit serial on every node targeting each board; never
mix omitted and explicit selectors.

### 3.2 Compile-time multi-parent rejection

Two serial-less parents both normalize to `""`; the second registry lookup
returns the first handle before the mixed-selector guard. Two logical devices
would silently target one board. `pdg_mfd.c` must prevent that image:

```c
#define PDG_MFD_INST_HAS_SERIAL(inst) \
	DT_INST_NODE_HAS_PROP(inst, serial_number) &&

BUILD_ASSERT(
	(DT_NUM_INST_STATUS_OKAY(DT_DRV_COMPAT) <= 1) ||
	(DT_INST_FOREACH_STATUS_OKAY(PDG_MFD_INST_HAS_SERIAL) 1),
	"Multiple enabled odp,pico-de-gallo parents require serial-number on every parent");
```

The foreach macro deliberately emits a trailing `&&`; final `1` completes the
constant expression. Zero or one enabled parent remains valid without a serial.
With multiple parents, every enabled instance must carry the property. Presence
is checked, not string uniqueness; distinct boards still require distinct values
by binding contract. Duplicate explicit values remain a residual risk.

## 4. Kconfig, CMake, and init order

`drivers/Kconfig` sources `mfd/Kconfig` before I2C/SPI. It defines:

```text
config MFD_PICO_DE_GALLO
    bool "Pico de Gallo MFD parent"
    default y
    depends on DT_HAS_ODP_PICO_DE_GALLO_ENABLED
    depends on ARCH_POSIX

config MFD_PICO_DE_GALLO_INIT_PRIORITY
    int "Pico de Gallo MFD parent initialization priority"
    default KERNEL_INIT_PRIORITY_DEFAULT
    depends on MFD_PICO_DE_GALLO
```

`zephyr/Kconfig` adds `DT_HAS_ODP_PICO_DE_GALLO_ENABLED` to the existing
`PICO_DE_GALLO` default OR. Normalize only changed line 7 from two-space to the
surrounding four-space indentation. Plain `native_sim` is 32-bit and disables
the module through `depends on 64BIT`; verification uses
`native_sim/native/64`.

Measured values are parent `POST_KERNEL/40` via
`KERNEL_INIT_PRIORITY_DEFAULT`, I2C `POST_KERNEL/50`, SPI `POST_KERNEL/50`, and
libc 35. The symbolic default expresses Zephyr ordering intent, follows the
configured default if it changes, runs after libc needed by malloc/pthreads, and
before children. Upstream `MFD_INIT_PRIORITY=80` is unsuitable. Downstream can
override priorities; M2/M3 readiness guards are the real protection.

`drivers/CMakeLists.txt` adds the MFD subdirectory. Its library compiles
`pdg_mfd.c`, privately includes `../common`, and exports its own directory to
future child libraries. Common host sources change gating to:

```cmake
if(CONFIG_MFD_PICO_DE_GALLO OR CONFIG_I2C_PICO_DE_GALLO OR
   CONFIG_SPI_PICO_DE_GALLO)
```

This preserves common sources, include path, and `-Werror=switch` for MFD-only
configuration. The switch option matters to `common.c`, not the parent.

## 5. Binding and shield topology

The parent binding uses repository headers, `compatible: "odp,pico-de-gallo"`,
and `include: base.yaml`. Its description says this node represents one physical
USB-attached board, owns its host connection, and exposes peripheral controllers
as children. Optional string `serial-number` carries §3's warning.

No `child-binding`, `bus`, address cells, or size cells are needed: M1 children
remain siblings, future compatible children own their bindings, and the parent
is a container rather than an address bus. This does not force a breaking M2
binding change.

The shield adds only:

```dts
pdg0: pico-de-gallo {
	compatible = "odp,pico-de-gallo";
	status = "disabled";
};
```

inside the existing root. `pico-de-gallo` is legal without unit address because
there is no `reg`; `pdg0` is a legal label. Existing `pdg_i2c0` and `pdg_spi0`
remain exactly where they are as root siblings. The permanent parent stays
disabled to avoid redundant runtime opens before M2.

## 6. Compile-only verification

### 6.1 Safety boundary

No build path reaches `gallo_init_strict()`. Configure invokes only
`rustc --version --verbose`; Corrosion compiles and cbindgen generates; linking
merely resolves the call compiled into `gallo_registry.c`. USB opens only when
the native_sim process starts. Builds are safe; produced binaries must never run
(no direct launch, `west build -t run`, test runner, or hardware command).

Measured environment: Zephyr 4.4.99 (`v4.4.0-6123-g26f811ee9d0d`), host toolchain,
Corrosion producing `libpico_de_gallo_ffi.a`. Build directories and temporary
overlays must remain under `/tmp`, because repo-root `build/` is not ignored.

### 6.2 Ordinary-sample regression gate

Run each sample separately:

```bash
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/pdg-base-<NAME> -b native_sim/native/64 zephyr/samples/<NAME> -- -DSHIELD=pico_de_gallo'
```

Required outcomes:

- `i2c_bridge`: success;
- `spi_nor_id`: success;
- `spi_bridge`: `zephyr.elf` succeeds, native_simulator runner link producing
  `zephyr.exe` fails with exactly one undefined device symbol attributable to
  `is31fl3743b@0`;
- `combined_i2c_spi_bridge`: same stage and sole failure, attributable to its
  `is31fl3743b@0`.

Baseline symbols were `__device_dts_ord_43` and `__device_dts_ord_44`, but the
new shield node may renumber ordinals. Compare per sample by the same single
undefined symbol's attribution, never by literal integer or cross-sample
identity. M1 must not remediate either pre-existing failure.

### 6.3 Enabled-parent probe

Create `/tmp/pdg-mfd-m1.overlay`:

```dts
&pdg0 {
	status = "okay";
};
```

Run exactly:

```bash
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/pdg-mfd-m1-build -b native_sim/native/64 zephyr/samples/i2c_bridge -- -DSHIELD=pico_de_gallo -DEXTRA_DTC_OVERLAY_FILE=/tmp/pdg-mfd-m1.overlay'
```

Extra overlay precedence is higher than the sample overlay. The probe must show
an okay parent, MFD Kconfig enabled, priority 40, `pdg_mfd.c` compiled, host
common symbols resolved, and successful link without execution. A local
`FETCHCONTENT_SOURCE_DIR_CORROSION` is optional only if an actual rate limit
occurs.

### 6.4 Assertion probes

Using external throwaway overlays and §6.3's environment:

1. Enable serial-less `pdg0` and add a second enabled serial-less parent: build
   must fail with the `BUILD_ASSERT` message.
2. Enable two parents carrying distinct explicit serials: compile and link must
   succeed, without execution.
3. The single serial-less parent probe must still succeed.

Delete `/tmp` artifacts afterward. A clean tester checkout must end with empty
`git status --short`; in a shared checkout, pre-existing entries must remain
exactly unchanged.

### 6.5 Assurance boundary

M1 proves devicetree generation, Kconfig activation/priorities, compilation,
runner linking and symbol resolution, disabled-node non-activation, and build-
time rejection of ambiguous serial-less parents.

It does not prove USB open, schema validation, failure logging, refcount
transitions, runtime multi-board selection, timeout behavior, explicit-serial
uniqueness, or interface release.

## 7. Host/embedded declaration ownership

Create FFI-free `drivers/common/common_bottom.h` with house copyright/SPDX,
`PDG_COMMON_BOTTOM_H`, `extern "C"`, documentation, and exactly:

```c
void *pdg_common_bottom_open(const char *serial);
void pdg_common_bottom_close(void *ctx);
```

`common.h` includes it and removes its duplicate declarations. Thus existing
host includers and `common.c` see them transitively while there is one declaration
site. `pdg_mfd.c` includes only `common_bottom.h`, never host-only `common.h`.

Architecture review explicitly required this added file, satisfying plan §3's
justification rule. Independent host/embedded redeclarations can drift into ABI
undefined behavior without diagnostic; the shared header follows the existing
`pdg_i2c_bottom.h` pattern.

No forwarding `pdg_mfd_bottom.c` is needed. Existing embedded `pdg_i2c.c`
already calls symbols implemented in host-target `pdg_i2c_bottom.c`, proving
direct embedded-to-host linkage. The issue is declaration ownership, not linkage.

## 8. Invariants and failure modes

Invariants:

1. One enabled parent owns one registry reference.
2. The accessor borrows ownership; children never close/free it.
3. Children check readiness before context and reject NULL afterward.
4. Embedded parent/children never include `pico_de_gallo.h`.
5. Common open/close have one declaration site.
6. At most one enabled parent may omit `serial-number`.
7. Multiple parents carry explicit serial properties; distinct boards require
   distinct values.
8. During M1, parent and legacy siblings targeting one board use identical
   selector treatment.
9. No parent lock exists.
10. Parent 40 follows libc 35 and precedes children 50.
11. Disabled parent creates no instance or USB open.

Failure modes:

- Open failures collapse to NULL; parent logs context and returns `-ENODEV`.
- Failure can take five minutes.
- Mixed parent/legacy-child selectors fail the differing init at runtime.
- Multiple serial-less parents fail at build time.
- Duplicate explicit parent serials pass presence assertion and would alias at
  runtime; binding forbids this for distinct boards.
- Priority overrides can violate order; later child readiness guards catch it.

## 9. Alternatives considered

- Typed opaque pointer: viable, but rejected for established `void *ctx`
  consistency and avoiding cross-bottom-half type migration.
- Static inline accessor: rejected because it exposes parent data layout.
- Errno/out-param accessor: unnecessary because readiness carries init state.
- Private common prototypes: rejected due silent ABI drift; use shared header.
- MFD forwarding bottom half: unnecessary; linkage already works directly.
- Upstream priority 80: after children, so rejected.
- Literal 40: rejected for symbolic `KERNEL_INIT_PRIORITY_DEFAULT`, which
  expresses intent and resolves to 40 today.
- Permanently enabled parent: rejected during additive staging.
- Documentation-only or runtime multi-parent guard: inadequate/unverifiable;
  compile-time rejection prevents unsafe images.
- Parent child-binding/address cells: premature and semantically unnecessary.

## 10. Acceptance criteria

1. Enabled `odp,pico-de-gallo` with optional string `serial-number` activates
   the parent.
2. Shield exposes disabled serial-less `pdg0: pico-de-gallo`; existing I2C/SPI
   remain unchanged root siblings.
3. `pdg_mfd.h` matches §2.2 verbatim, including readiness-first semantics.
4. `common_bottom.h` solely declares common open/close; `common.h` includes it;
   embedded parent includes neither `common.h` nor `pico_de_gallo.h`.
5. Enabled-parent probe generates okay node, enables MFD, resolves priority 40,
   compiles parent, and links runner without execution.
6. Two serial-less parents fail with the specified assertion message.
7. Two distinct explicit parents and one serial-less parent each compile/link.
8. Two baseline-success samples remain successful; two known failures retain
   only their per-sample `is31fl3743b@0` runner-link failure.
9. Defaults are parent 40, I2C/SPI 50, all POST_KERNEL, after libc 35.
10. MFD-only config receives common sources/include/options.
11. `zephyr/CHANGELOG.md` records the compatible under Unreleased/Added.
12. No repository verification artifacts remain and no image executes.

## 11. Residual risks

The conductor must report:

- runtime open and schema validation are untested;
- boot can appear hung for five minutes;
- failures collapse to NULL and stderr visibility is untested;
- refcount transitions and cleanup are untested;
- runtime multi-board selection is untested;
- explicit serial presence is asserted, but uniqueness is not;
- interface release is untested; and
- downstream priority overrides remain possible until child guards land.

## 12. Non-goals and documentation parity

M1 does not modify I2C/SPI source, bottom halves, or bindings; nest children;
add GPIO or validate `ngpios`; alter chip-select semantics; add a parent lock;
remediate the two broken samples; run native_sim/hardware; improve error
propagation; change wire/firmware/Rust/version/locks; or add other peripheral
children.

`zephyr/CHANGELOG.md` **must** add the new public compatible under existing
`[Unreleased]` → `Added`, because this is a devicetree-contract addition.

No book change is required: the parent stays disabled, no child consumes it, and
no active CLI/endpoint/FFI/Python/hardware/Zephyr peripheral behavior changes.
The PR must include that one-line AGENTS.md §15.1 justification. M6 handles book
updates when topology and GPIO/SPI behavior become active; that does not defer
the Zephyr changelog.

## 13. Amendments and contradictions against parent plan/design

1. Plan §1's “four samples build; parent initialises” gate is wrong: two samples
   have measured pre-existing runner-link failures, ordinary samples disable the
   parent, and runtime is forbidden. Replace it with per-sample baseline plus
   enabled-parent and assertion compile-only probes.
2. Plan §4's command is defective here: it must source venv and Zephyr env,
   export host toolchain, use 64-bit native_sim, explicitly select shield, and
   build outside the repository.
3. Plan §3 omits `zephyr/Kconfig`; MFD-only activation requires it. Normalize
   only substantively changed line 7's indentation.
4. Plan §3 omits `common_bottom.h` and `common.h`; review required one shared
   FFI-free declaration owner. This reviewer request satisfies added-file
   justification.
5. Plan §3 omits `zephyr/CHANGELOG.md`; a public compatible requires an
   Unreleased/Added entry.
6. Design §4.1 underspecifies the accessor; §2.2 fixes exact API and child
   sequence.
7. Design §4.3 says direct registry open; use common shim through shared FFI-free
   header instead, preserving ownership.
8. Design §4.6 is too generic: use module-local symbolic default resolving to
   40, after libc 35 and before children 50; upstream 80 is invalid.
9. Design's serial is illustrative, not shipped M1 content; all current nodes
   omit it unless downstream consistently supplies it.
10. Parent plan/design omit ambiguous multi-parent rejection; M1 adds the
    compile-time property-presence assertion.
11. No forwarding MFD C file is needed; the new common header addresses
    declaration ownership while existing linkage precedent remains valid.

## 14. Open questions

No implementation-blocking question remains. Runtime properties in §11 cannot
be determined under the no-execution constraint and are not claimed.
