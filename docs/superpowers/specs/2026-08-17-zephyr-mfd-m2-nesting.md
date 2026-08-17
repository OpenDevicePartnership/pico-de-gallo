# Zephyr MFD restructure M2 — nesting and handle ownership specification

Date: 2026-08-17  
Branch baseline: `zephyr` at `dd3a7834f3de`  
Milestone: M2 — happy-path bus refactor; breaking devicetree and initialization migration

## 1. Context, scope, and final inventory

M2 makes the existing `odp,pico-de-gallo` parent the sole owner of a board's
USB handle. I2C and SPI become direct devicetree children and borrow that
handle.

Happy-path I2C and SPI transfer behaviour is unchanged. Initialization
ownership, failure coupling, failure location, and worst-case boot latency do
change: one parent validation at `POST_KERNEL/40` now gates both children at
`POST_KERNEL/50`. Physical USB opens were already deduplicated to one; M2
reduces registry calls and references from three to one. A parent failure now
fails both children coherently and quickly after the single parent attempt,
rather than allowing independent child outcomes or up to three independent
five-minute strict opens. This fail-closed identity coupling is a net reliability
improvement, at a small availability cost when one child could previously have
initialized independently.

M2 leaves `cs-gpio-indices`, SPI batching, and
`pdg_spi_bottom_num_gpios()` intact. GPIO is M3 and standard `cs-gpios` is M4.
There is no parent lock. Nothing under `crates/`, no wire/firmware/version/
lockfile change.

### 1.1 Final file inventory

**Create**

- `docs/superpowers/specs/2026-08-17-zephyr-mfd-m2-nesting.md`

**Modify**

- `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay`
- `zephyr/drivers/i2c/pdg_i2c.c`
- `zephyr/drivers/spi/pdg_spi.c`
- `zephyr/dts/bindings/mfd/odp,pico-de-gallo.yaml`
- `zephyr/dts/bindings/i2c/odp,pico-de-gallo-i2c.yaml`
- `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml`
- `zephyr/samples/i2c_bridge/app.overlay`
- `zephyr/samples/spi_bridge/app.overlay`
- `zephyr/samples/spi_nor_id/app.overlay`
- `zephyr/samples/combined_i2c_spi_bridge/app.overlay`
- `zephyr/README.md`
- `book/src/interfaces/spi.md`
- `zephyr/CHANGELOG.md`

Plan §3 omits the parent binding, README, SPI book page, changelog, and
this specification. Section 15 records every divergence. No bottom-half file is
modified in M2.

## 2. Nested devicetree topology

The shield overlay must land exactly as:

```dts
/ {
	pdg0: pico-de-gallo {
		compatible = "odp,pico-de-gallo";
		status = "disabled";

		pdg_i2c0: i2c {
			compatible = "odp,pico-de-gallo-i2c";
			clock-frequency = <400000>;
			#address-cells = <1>;
			#size-cells = <0>;
			status = "disabled";
		};

		pdg_spi0: spi {
			compatible = "odp,pico-de-gallo-spi";
			#address-cells = <1>;
			#size-cells = <0>;
			status = "disabled";
		};
	};
};
```

Use design §3's `i2c` and `spi` node names, not transitional `pdg-i2c` and
`pdg-spi`. Absolute paths, generated identifiers, and dependency ordinals
change. Labels remain stable, so `&pdg_i2c0` and `&pdg_spi0` consumers do not.
Repository search found no `DT_PATH`, `DT_ALIAS`, `DEVICE_DT_GET_ANY`,
`DT_CHOSEN`, aliases/chosen nodes, or hard-coded Zephyr path strings. All four
sample applications use node labels only. The absolute-path consequence is
nevertheless public and must be in the changelog.

The parent needs no `#address-cells` or `#size-cells`. It is an ownership
container; its direct children have neither `reg` nor unit addresses and are not
addressed in a parent bus space. The child controllers' own cells describe their
peripheral grandchildren. No `avoid_default_addr_size`, `reg_format`, or other
new `dtc` warning is expected. This is a build-time observation the gate must
confirm, not an assumed fact.

## 3. Structural parent enforcement

Runtime readiness is insufficient to prove type. A child under a ready unrelated
device would pass `device_is_ready()`, after which `pdg_mfd_ctx()` would
reinterpret foreign driver data as `struct pdg_mfd_data` and return an arbitrary
pointer. NULL guards cannot detect that pointer. This is an R8-class
wrong-target/crash hazard.

Each enabled I2C and SPI instance must therefore emit three per-instance
`BUILD_ASSERT`s before any device definition, **in this fixed order**:

1. `DT_INST_PARENT(inst)` has compatible `odp,pico-de-gallo`;
2. that parent has status `okay`;
3. `CONFIG_MFD_PICO_DE_GALLO` is enabled.

The order is load-bearing and is part of the contract, not an implementation
detail. Disabling the parent also drops `DT_HAS_ODP_PICO_DE_GALLO_ENABLED`,
which makes `CONFIG_MFD_PICO_DE_GALLO` `n`, so assertion 3 is true at exactly
the same time as assertion 2 in the disabled-parent case. Fixing the order
compatible → status → Kconfig keeps the most specific structural diagnostic
first. Note also that `BUILD_ASSERT` expands to `_Static_assert`, which is a
non-fatal diagnostic: GCC reports **every** failing assertion in one pass. All
probe criteria are therefore substring matches over the whole build log, never
"exactly one message".

For the disabled-parent case the expected log must contain, unambiguously:

```text
Enabled odp,pico-de-gallo-i2c controllers require their odp,pico-de-gallo parent to have status okay
```

and will additionally contain the Kconfig message for the reason above. Both
appearing is the expected outcome, not an implementation defect.

Messages must be readable and name the required topology, for example:

```text
Enabled odp,pico-de-gallo-i2c controllers must be direct children of an odp,pico-de-gallo parent
Enabled odp,pico-de-gallo-i2c controllers require their odp,pico-de-gallo parent to have status okay
Enabled odp,pico-de-gallo-spi controllers must be direct children of an odp,pico-de-gallo parent
Enabled odp,pico-de-gallo-spi controllers require their odp,pico-de-gallo parent to have status okay
```

`DT_INST_PARENT(inst)` on a stale root child is `/`; therefore status alone is
not sufficient and compatible must be asserted separately. Exact Zephyr macro
spellings must be checked against installed Zephyr 4.4.99 during implementation;
do not guess between forms such as `DT_NODE_HAS_COMPAT`,
`DT_NODE_HAS_STATUS_OKAY`, and `DT_NODE_HAS_STATUS(..., okay)`.

Add a third per-instance assertion requiring `CONFIG_MFD_PICO_DE_GALLO`. An
okay compatible parent makes the MFD symbol default to `y`, but a downstream
configuration can explicitly override that default to `n` while leaving a child
driver enabled. The third assertion does not strengthen structural type safety,
but it replaces the resulting undefined parent-device ordinal — or, because the
MFD driver directory is not added to the build at all when the symbol is `n`, a
bare "pdg_mfd.h: No such file or directory" — with a readable configuration
error. Implementations must therefore emit the assertions **before** including
`pdg_mfd.h`, so the readable message precedes the fatal include error. Its
message is:

```text
Enabled Pico de Gallo child controllers require CONFIG_MFD_PICO_DE_GALLO=y
```

Use Zephyr's established compile-time Kconfig predicate (normally
`IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO)`); verify its availability in the
installed tree. This diagnostic value is consistent with the existing SPI
comment's policy that an unresolved device ordinal is inferior to an explicit
assertion.

An unresolved parent ordinal remains defence in depth, never the contract. These
assertions also disambiguate M2 topology errors from the two known R5 failures:
topology errors fail compilation with the messages above, while R5 reaches the
runner link with one resolved `is31fl3743b@0` ordinal.

## 4. Handle-migration and callback-safety contract

### 4.1 Configuration shape

```c
struct pdg_i2c_config {
	const struct device *mfd;
	uint32_t clock_frequency;
};

struct pdg_spi_config {
	const struct device *mfd;
	const uint8_t *cs_indices;
	size_t cs_indices_len;
};
```

Each instance initializes `.mfd = DEVICE_DT_GET(DT_INST_PARENT(inst))`.
Remove `serial` and every child `DT_INST_PROP_OR(...serial_number...)`.
Both drivers include `pdg_mfd.h`.

### 4.2 Init sequence and diagnostics

For every allocated child, initialize its mutex before any early return:

```c
k_mutex_init(&data->lock);
```

Then perform the mandatory M1 sequence before frequency conversion,
informational logging, metadata reads, or any other bottom-half call:

```c
if (!device_is_ready(config->mfd)) {
	LOG_ERR("%s: Pico de Gallo parent %s is not ready. Returning -ENODEV.",
		dev->name, config->mfd->name);
	return -ENODEV;
}

data->ctx = pdg_mfd_ctx(config->mfd);
if (data->ctx == NULL) {
	LOG_ERR("%s: Pico de Gallo parent %s is ready but returned a NULL context; "
		"this is an MFD ownership invariant failure. Returning -ENODEV.",
		dev->name, config->mfd->name);
	return -ENODEV;
}
```

A false readiness check is dependency failure. NULL after passing readiness is
an ownership invariant failure. Both return `-ENODEV` without an RPC.

After this prefix, I2C preserves its frequency/configuration sequence. SPI
preserves `pdg_spi_bottom_num_gpios()` and ready logging. Update SPI's current
warm-cache comment: the **parent's** strict open and validation populate the
shared `num_gpios` cache; `pdg_spi_bottom_open()` is no longer called.
The GPIO-count read remains because it is validated device metadata, not CS
logic.

### 4.3 I2C direct-call guards

`z_impl_i2c_transfer()` and direct configure/get-config calls do not guarantee
readiness. At the top of all three callbacks, before locking or reading cached
state, require:

```c
if (data->ctx == NULL) {
	LOG_ERR("%s: Pico de Gallo I2C bridge context is NULL; check device readiness. Returning -ENODEV.",
		dev->name);
	return -ENODEV;
}
```

This applies to `pdg_i2c_configure`, `pdg_i2c_get_config`, and
`pdg_i2c_transfer`. It prevents an uninitialized/failed child from locking an
invalid mutex, issuing an RPC, or returning zero-initialized cached config as a
false success. SPI's existing load-bearing NULL guard remains unchanged.

The I2C hole predates M2. It is closed here because the migration promises that
clearing a failed child's cached borrow makes direct calls fail safely, and
because M3 must copy a correct child-driver pattern rather than propagate the
hole. This is safety completion of the ownership migration, not unrelated
feature work.

### 4.4 Borrowed-handle failure paths

Delete the only three executed child close calls:

- `pdg_i2c.c:315` and `:323` (`pdg_i2c_bottom_close`);
- `pdg_spi.c:399` (`pdg_spi_bottom_close`).

Their chain reaches `pdg_registry_close`; with the M2 parent holding sole
reference (`rc == 1`), it removes the registry entry and calls `gallo_free()`,
dropping the Rust box. A child close would therefore create a use-after-free
while the parent still reports ready and a sibling caches the same pointer.

Clear `data->ctx = NULL` on **every** post-borrow init failure:

1. I2C `freq_to_speed_()` failure;
2. I2C second `speed_to_code_()` failure;
3. I2C `pdg_i2c_bottom_set_config()` failure;
4. SPI `pdg_spi_bottom_num_gpios()` failure.

This is defensive invalidation of a non-owning child cache, never reference
release. NULL is guardable and becomes `-ENODEV`; a valid-looking unowned
pointer bypasses NULL checks and is strictly worse.

The legacy I2C/SPI/common bottom-half open/close wrappers remain unused in M2.
M4 must remove them or rename the ownership-specific surface while it already
reopens the SPI bottom half.

## 5. Binding contracts

### 5.1 Child bindings

Delete the entire `serial-number` property from both child bindings. A stale
child property in a downstream overlay fails loudly in `edtlib.py`'s
undeclared-property check; it is not silently ignored.

Add to both child descriptions:

```text
This controller must be a direct child of an enabled odp,pico-de-gallo MFD
parent. It borrows the parent's host connection and inherits the parent's
serial-number selection; serial-number is not valid on the child controller.
```

The I2C binding otherwise remains unchanged. The SPI binding keeps every current
`cs-gpio-indices`, firmware batching, and `cs-gpios` rejection statement exactly
as-is until M4.

### 5.2 Parent replacement selector text

Keep `serial-number`, but replace its description with:

```yaml
      Optional USB serial number selecting the physical Pico de Gallo board
      that this parent represents. The selection applies to all child
      peripheral controllers.

      If omitted, the first matching board is selected, but the host API cannot
      report which serial number it chose. Omission is safe only when exactly
      one matching Pico de Gallo board is physically attached. With multiple
      matching boards, a selector-less parent and all its children can become
      ready against the wrong board without a diagnostic.

      At most one enabled parent may omit this property; a build-time assertion
      rejects multiple enabled selector-less parents. A multi-board devicetree
      must set a unique explicit serial-number on every enabled parent. The
      build-time check verifies property presence, not value uniqueness: two
      parents with the same explicit value silently alias the same physical
      board. Devicetree authors must ensure explicit values are distinct.
```

Delete the transitional three-node paragraph and intra-board mixed-selector
rule. Children can no longer mix selectors. The registry's global guard remains
defence for other callers, not this binding's child contract.

## 6. Sample overlays

Zephyr does not implicitly disable an enabled child when its parent is disabled.
Every sample explicitly enables `pdg0` before its child stanza.

### 6.1 `i2c_bridge`

```dts
&pdg0 {
	status = "okay";
};

&pdg_i2c0 {
	status = "okay";

	tmp117: tmp117@48 {
		compatible = "ti,tmp11x";
		reg = <0x48>;
		status = "okay";
	};
};
```

### 6.2 `spi_bridge`

```dts
&pdg0 {
	status = "okay";
};

&pdg_spi0 {
	status = "okay";
	cs-gpio-indices = <0>;

	led_matrix: is31fl3743b@0 {
		compatible = "issi,is31fl3743b";
		reg = <0>;
		spi-max-frequency = <1000000>;
		current-limit = <90>;
		status = "okay";
	};
};
```

### 6.3 `spi_nor_id`

```dts
&pdg0 {
	status = "okay";
};

&pdg_spi0 {
	status = "okay";
	cs-gpio-indices = <0>;

	/*
	 * A JEDEC SPI NOR flash on chip-select index 0, which the Pico de
	 * Gallo bridge drives on GPIO 8.
	 *
	 * The omissions here are deliberate. Every write-capable path in
	 * Zephyr's spi_nor driver is gated on a devicetree property, and none
	 * of them are set: has-lock (WREN+WRSR to clear block protect),
	 * requires-ulbpr (ULBPR), enter-4byte-addr (WREN+4BA), has-dpd
	 * (DPD/RDPD), mxicy-mx25r-power-mode (WREN+WRSR on config registers)
	 * and use-flag-status-register (CLRFLSR). With those absent, and with
	 * the application calling only flash_read(), the driver never issues a
	 * write, erase or write-enable opcode.
	 *
	 * jedec-id and size are informational under CONFIG_SPI_NOR_SFDP_RUNTIME,
	 * which discovers the real geometry from the device. Note that Zephyr
	 * expresses size in *bits*: 16 Mbit is 2 MiB.
	 */
	nor: nor@0 {
		compatible = "jedec,spi-nor";
		reg = <0>;
		spi-max-frequency = <10000000>;
		jedec-id = [c8 40 15];
		size = <0x1000000>;
		status = "okay";
	};
};
```

### 6.4 `combined_i2c_spi_bridge`

```dts
&pdg0 {
	status = "okay";
};

&pdg_i2c0 {
	status = "okay";

	tmp117: tmp117@48 {
		compatible = "ti,tmp11x";
		reg = <0x48>;
		status = "okay";
	};
};

&pdg_spi0 {
	status = "okay";
	cs-gpio-indices = <0>;

	led_matrix: is31fl3743b@0 {
		compatible = "issi,is31fl3743b";
		reg = <0>;
		spi-max-frequency = <1000000>;
		current-limit = <90>;
		status = "okay";
	};
};
```

## 7. Initialization and failure semantics

Nesting creates devicetree dependencies and may renumber ordinals. It does not
schedule device initialization. Parent remains `POST_KERNEL/40`; I2C and SPI
remain `POST_KERNEL/50`. Numeric priority plus runtime readiness is the init
contract; compile-time assertions are the topology/type contract.

A disabled parent with enabled child, a stale root child, or a child under any
non-PDG parent fails at compile time with §3's readable assertion, before an
unsafe accessor can be compiled into a runnable topology. Any later undefined
ordinal is only defence in depth.

If an enabled parent open fails, its object exists but is not ready. Each child
has an initialized mutex, logs the parent-not-ready diagnostic, and returns
`-ENODEV` before accessor or RPC. The one parent validation failure is shared by
both children. Compared with M1, failure moves from child priority 50 to parent
priority 40 and worst-case strict-open latency falls from as many as three
five-minute attempts to one attempt plus fast child failures.

M1's `BUILD_ASSERT` still filters the status-okay compatible set for
`odp,pico-de-gallo`; nested children have different compatibles. The assertion
must still be re-verified empirically with §10.6's nested probe and controls.

## 8. Selector and multi-board safety

After M2, only the parent executes the common open path on behalf of children.
Physical USB opens remain one both before and after M2 because registry hits
already deduplicated by selector. Registry call count and refcount change from
three to one.

Each multi-board parent carries a unique explicit serial and all children
structurally share it. Intra-board mixed-selector partial topology becomes
impossible. R8 shrinks from three registry references per board to one. Two
selector-less enabled parents remain compile-time rejected and duplicate
explicit parent serials still alias.

One selector-less parent remains unsafe when more than one matching physical
board is attached (R11). The devicetree assertion counts parents, not USB
devices, and the default open cannot report its selected serial. This is not
closable in M2; parent binding documentation is the mitigation.

## 9. Invariants and failure modes

### 9.1 Invariants

1. Every enabled I2C/SPI instance is compile-time proven to be a direct child of
   a status-okay `odp,pico-de-gallo` parent.
2. Parent alone selects and opens the board; child bindings have no selector.
3. Child mutex initialization precedes every init return.
4. Runtime readiness precedes accessor and all child work.
5. NULL after readiness is an invariant failure returning `-ENODEV`.
6. I2C and SPI direct-call paths reject NULL context before lock/cache/RPC use.
7. Children never close/free borrowed context.
8. Every post-borrow init failure clears only the child's cached pointer.
9. Child mutexes remain; no parent lock.
10. SPI retains batch, `cs-gpio-indices`, and GPIO-count read.
11. Parent/child priorities remain 40/50; nesting is not scheduling.
12. Parent has no address/size cells and creates no new `dtc` warning.
13. No crate, wire, firmware, or version contract changes.

### 9.2 Failure modes

- Wrong/missing/disabled structural parent: readable compile-time assertion.
- Enabled parent open failure: parent and both children not ready; one slow
  attempt followed by child `-ENODEV` fast failures.
- Ready parent returning NULL: invariant log and `-ENODEV`.
- Direct call on failed I2C/SPI child: `-ENODEV`, no uninitialized lock/cache/RPC.
- Two selector-less parents: exact M1 compile-time assertion failure.
- Duplicate explicit serials: build passes and runtime aliases; binding forbids.
- One selector-less parent with multiple attached boards: may silently select
  wrong board; binding forbids omission in that physical setup.
- I2C config or SPI GPIO-count failure: child clears borrow without closing;
  parent/sibling remain valid.
- Priority override inversion: initialized child mutex plus readiness guard yields
  `-ENODEV` rather than uninitialized state use.

## 10. Verification plan

### 10.1 Safety and negative-probe rule

Do not invoke `gallo_*` MCP, `probe-rs`, `cargo run -p gallo`, built samples,
`west build -t run`, or hardware. Build/link only, under `/tmp`.

Every negative topology probe must use baseline-clean `i2c_bridge` or
`spi_nor_id`, never R5-affected `spi_bridge` or
`combined_i2c_spi_bridge`. This prevents a known runner-link failure from
masking the intended compile-time assertion.

### 10.2 Baseline and four-sample apples-to-apples gate

Measured baseline at `dd3a7834f3de`:

| Sample | Result | Undefined | Resolved node | Corrected TU grep |
| --- | --- | --- | --- | --- |
| `i2c_bridge` | PASS | — | — | `pdg_i2c.c` |
| `spi_nor_id` | PASS | — | — | `pdg_spi.c` |
| `spi_bridge` | FAIL, runner link | `__device_dts_ord_44` | `/pdg-spi/is31fl3743b@0` | `pdg_spi.c` |
| `combined_i2c_spi_bridge` | FAIL, runner link | `__device_dts_ord_45` | `/pdg-spi/is31fl3743b@0` | `pdg_i2c.c`, `pdg_spi.c` |

The `Resolved node` column above is the **baseline** (pre-nesting) path. After
the §2 rename the same node is `/pico-de-gallo/spi/is31fl3743b@0`; that is the
path each post-change R5 ordinal must resolve to. A post-change resolution
still reading `/pdg-spi/...` means the shield was not renamed.

All four have `CONFIG_PICO_DE_GALLO=y`; all four lack
`CONFIG_MFD_PICO_DE_GALLO` because the parent is disabled. The differing 44/45
ordinals at one commit prove raw ordinal comparison invalid.

For each post-change sample run **without** explicit shield flag, exactly matching
the measured baseline command:

```sh
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/<DIR> -b native_sim/native/64 zephyr/samples/<NAME>'
```

Required outcomes remain two clean successes and two runner-link failures with
exactly one undefined symbol attributable to each build's `is31fl3743b@0`.
Happy-path driver code must not remediate R5. An R5-affected sample that now
**passes** is a **failure**, not a bonus: it means a stub compatible was added
outside M2's inventory.

Then perform one additional clean build of `i2c_bridge` with:

```text
-- -DSHIELD=pico_de_gallo
```

It must be equivalent to the no-flag success and non-vacuity result. This
confirms the deterministic explicit form recommended by the README without
changing the apples-to-apples primary comparison.

### 10.3 Per-build failure fingerprint and ordinal resolution

For each R5 failure, match **count + undefined symbol + resolved node path**,
never raw error text, function name, or literal ordinal. The enclosing function
already differs (`.text.main+0x27` versus `.text.spi_worker+0x27`). Resolve from
each build's own header:

```sh
header=<build>/zephyr/include/generated/zephyr/devicetree_generated.h
# Substitute NN from the sole linker undefined.
grep -n 'ORD  *NN$' "$header"
grep -n 'is31fl3743b' "$header"
```

The ordinal-table comment and `_PATH` define must both attribute NN to that
build's nested `is31fl3743b@0` path. Require exactly one linker undefined.

### 10.4 Non-vacuity — corrected regex

The plan/spec's old regex excluded digits and could not match `pdg_i2c.c`.
Use prominently and exclusively:

```sh
grep '^CONFIG_PICO_DE_GALLO=y$' <build>/zephyr/.config
grep '^CONFIG_MFD_PICO_DE_GALLO=y$' <build>/zephyr/.config
grep -o 'pdg_[a-z0-9_]*\.c' <build>/compile_commands.json | sort -u
```

Expected changed embedded TUs after M2:

- `i2c_bridge`: `pdg_mfd.c`, `pdg_i2c.c`;
- `spi_bridge`: `pdg_mfd.c`, `pdg_spi.c`;
- `spi_nor_id`: `pdg_mfd.c`, `pdg_spi.c`;
- combined: `pdg_mfd.c`, `pdg_i2c.c`, `pdg_spi.c`.

Bottom halves may also appear. `CONFIG_MFD_PICO_DE_GALLO=y` and `pdg_mfd.c` in
every sample are positive evidence that parent enablement/migration took effect.
Build logs must be checked for zero new `dtc` warnings.

### 10.5 Structural-negative probes

Using baseline-clean `i2c_bridge`, supply throwaway overlays for **all** of:

1. a stale enabled root I2C child (`DT_INST_PARENT` yields `/`): compile must
   fail with the compatible/topology assertion;
2. an enabled I2C child under an unrelated enabled and *ready* parent
   (`uart0`): compile must fail with the compatible/topology assertion. Cases
   1 and 2 are materially different hazards — rejecting `/` versus R9 proper —
   and **both** are required; neither substitutes for the other;
3. an enabled I2C child nested **two levels deep** under an enabled PDG parent:
   compile must fail with the compatible/topology assertion. §3 asserts
   "**direct** children"; an implementation that walked ancestors would pass
   cases 1 and 2 and fail only this one. Covered by probe P17;
4. enabled nested child whose PDG parent remains disabled: compile must fail
   with the parent-status assertion (see §3 on the simultaneous Kconfig
   assertion);
5. valid enabled nested parent/child plus a throwaway config fragment containing
   `CONFIG_MFD_PICO_DE_GALLO=n`: compile must fail with the explicit MFD Kconfig
   assertion, not an undefined parent ordinal and not a bare missing-header
   error. Kconfig `default y` can win over a fragment under some dependency
   shapes, and a fragment that did not apply yields a *passing* build that
   looks exactly like an assertion regression. The probe must therefore first
   confirm the fragment took, by finding `CONFIG_MFD_PICO_DE_GALLO=n` or
   `# CONFIG_MFD_PICO_DE_GALLO is not set` in the build's `.config`. If it
   shows `=y`, the verdict is **inconclusive**, never pass;
6. an anti-vacuity control: the shipped, valid nested topology must emit
   **zero** static assertions and build to the §10.2 result. Without it the
   negative probes above could pass for free on unconditionally-true
   assertions.

The exact overlay forms may follow installed Zephyr fixture availability, but
must activate the malformed child and prove the asserted message, not an ordinal
link failure. Pass the third probe's fragment through Zephyr's standard
`EXTRA_CONF_FILE` mechanism from `/tmp`.

### 10.6 M1 multi-parent assertion and controls

Primary negative overlay must enable nested `pdg0` without serial, enable at
least one nested child, and add a second enabled serial-less parent:

```dts
&pdg0 {
	status = "okay";
};

&pdg_i2c0 {
	status = "okay";
};

/ {
	pdg1: pico-de-gallo-second {
		compatible = "odp,pico-de-gallo";
		status = "okay";
	};
};
```

Build baseline-clean `i2c_bridge`; compilation must fail exactly with:

```text
Multiple enabled odp,pico-de-gallo parents require serial-number on every parent
```

Controls:

1. same nested active tree with second parent `status = "disabled"` succeeds;
2. two enabled parents with distinct explicit `serial-number` values succeeds.

These controls prove the macro still inspects the compatible/status set after
nesting rather than rejecting any second node or missing active M2 topology.

### 10.7 Host gate

Run `cargo test --workspace --locked` after build-slot serialization. Baseline is
561 passed, 0 failed, 7 ignored **aggregated across the host workspace only**;
the firmware workspace is a separate workspace and is not tested here, because
M2 touches nothing in it. Post-change must match. M2 modifies no Rust.

## 11. Documentation parity and changelog

### 11.1 `zephyr/README.md` — bounded M2 edits

Update only truth made false by M2:

- missing-board output: parent open failure followed by child parent-not-ready;
- default-disabled explanation: parent and controller must both be enabled;
- I2C and SPI overlay examples: add enabled `&pdg0` stanza;
- board selection: set `serial-number` on `&pdg0`, explain child inheritance and
  that child property is invalid;
- troubleshooting cross-reference to parent selection;
- not-ready advice: check both parent and child status.

### 11.2 `book/src/interfaces/spi.md` — bounded M2 edit

Only the Zephyr mapping excerpt around current lines 118–130 changes: add enabled
`&pdg0` before `&pdg_spi0`, and state that `pdg0` owns board selection while the
SPI child borrows its connection. Existing `cs-gpio-indices`, batching, and
`cs-gpios` rejection text stays exactly unchanged.

### 11.3 `zephyr/CHANGELOG.md`

Add Unreleased/Breaking Changes stating:

- I2C/SPI controllers must be direct children of enabled MFD parent;
- `serial-number` moved from controllers to parent;
- absolute paths/generated identifiers change while labels remain stable;
- physical USB opens remain one, registry references change 3 → 1;
- one parent validation now gates both children, moving failure earlier,
  coherently coupling failures, and reducing worst-case strict-open attempts.

Amend M1's Added entry to replace “independent root siblings for now” with the
nested borrowed-handle state. The implementation commit message must use the
same qualified behaviour statement from §1.

### 11.4 Explicit M6 carry-over

Defer to M6: GPIO usage, `cs-gpios` examples, deletion of
`cs-gpio-indices`, M4 atomicity consequences, final integrated topology and
migration guide, and `book/src/interfaces/gpio.md`. M2 must not pre-write M3/M4
truth.

## 12. Acceptance criteria

1. Shield matches §2 with stable labels and `i2c`/`spi` node names.
2. No parent address/size cells and no new `dtc` warning.
3. Every child instance has compatible and status-okay parent assertions with
   readable topology messages; exact Zephyr macros are verified, not guessed.
4. A third per-instance assertion provides a readable failure when an okay
   parent exists but `CONFIG_MFD_PICO_DE_GALLO` is explicitly disabled.
5. Child drivers/bindings contain no `serial-number`; stale property fails binding
   validation; parent text includes R11 warning.
6. Child bindings explicitly document direct enabled parent, borrowed connection,
   and inherited selector.
7. Mutexes initialize before parent-related early returns.
8. Drivers follow readiness → accessor → invariant-NULL sequence and exact logs.
9. All I2C callbacks and existing SPI transfer guard reject NULL before state use.
10. No child closes/frees context; every post-borrow init failure clears only its
    child cache, including I2C `freq_to_speed_()`.
11. SPI keeps GPIO-count read, batch, mapping, and `cs-gpio-indices`; warm-cache
    comment attributes validation to parent.
12. All samples enable parent and preserve peripheral content/CS semantics.
13. Parent remains POST_KERNEL/40 and children POST_KERNEL/50; hierarchy is not
    treated as scheduling.
14. Four builds match measured result categories; each R5 ordinal is resolved per
    build by symbol/path/count; explicit-shield control is equivalent.
15. Every sample has both Pico de Gallo Kconfigs `=y`; corrected grep shows
    `pdg_mfd.c` plus expected child TUs.
16. Malformed-parent probes fail at compile time with readable assertions using
    baseline-clean samples.
17. Nested two-parent assertion and both controls behave as §10.6.
18. Host tests match 561 passed, 0 failed, 7 ignored.
19. README, bounded SPI book section, changelog, and commit message state M2 truth.
20. No bottom-half/GPIO/CS-semantic/crate/wire/firmware/version/lockfile change.
21. All touched text has LF endings.

## 13. Alternatives considered and rejected

- Runtime type assumption after readiness: unsafe foreign-data reinterpretation;
  compile-time compatible/status assertions are mandatory.
- Rely on the MFD Kconfig default without asserting it: rejected. An explicit
  downstream `n` override is possible and otherwise degrades to an ordinal link
  failure; the third assertion improves diagnosis, though not structural safety.
- Root siblings plus parent phandle: contradicts D1 and keeps membership
  conventional rather than structural.
- Child selectors/private opens: defeats ownership and preserves R8 surface.
- Parent address/size cells: semantically false; children are not addressed
  resources.
- Keep `pdg-i2c`/`pdg-spi`: viable but contradicts approved design; labels preserve
  consumers and path break is documented.
- Omit readiness because 40 < 50: priorities configurable; nesting does not
  schedule.
- Initialize mutex after readiness: failed device remains unsafe for direct API
  dispatch.
- Close borrowed context on child failure: use-after-free of parent's sole
  reference and sibling pointer.
- Leave failed child context non-NULL: bypasses direct-call NULL guards.
- Remove GPIO-count read: out of scope; validated metadata, not CS migration.
- Compare literal R5 ordinals/errors: ordinals and enclosing functions differ per
  build; resolve path and count.
- Use R5 sample for negative topology probe: known link failure can mask result.
- Defer all docs to M6: leaves false user workflows; bounded M2 parity avoids M3/M4
  collision.
- Create T10 harness: ephemeral predecessor absent; new framework outside M2.

## 14. Residual risks and explicit R9–R11 register

### R9 — Ready non-PDG parent can be reinterpreted as a USB handle

Without structural assertions, a child under a ready unrelated device passes
readiness and `pdg_mfd_ctx()` interprets foreign `dev->data` as MFD data. The
result may be a non-NULL arbitrary pointer, causing crash or silent wrong-target.
**Closed in M2** by per-instance compatible and status-okay parent assertions;
link failure is defence in depth.

### R10 — Failed I2C device direct calls lack context protection

I2C API dispatch can reach configure/get-config/transfer without readiness.
Without guards it may lock an uninitialized mutex, issue RPC through NULL, or
return zero-initialized config as success. **Closed in M2** by unconditional
mutex initialization, top-of-callback NULL guards, and clearing every
post-borrow failure cache.

### R11 — One selector-less parent is ambiguous with multiple attached boards

M1's assertion constrains devicetree parents, not attached USB devices. With one
selector-less parent and multiple matching boards, `gallo_init_strict()` chooses
the first and cannot report its serial. Parent and children can become ready
against the wrong board without diagnosis; this becomes actuation-unsafe in M3.
**Not closable in M2.** Parent binding requires explicit serial whenever more
than one matching board is attached. Runtime selection remains unverified.

Additional residuals:

- no M1/M2 Zephyr image has executed;
- USB open/schema validation, actual five-minute timeout, logs, refcount
  transitions, multi-board selection, interface release, and runtime child
  behaviour remain unexecuted;
- duplicate explicit parent serials still silently alias;
- legacy bottom-half ownership wrappers remain callable until M4 cleanup;
- exact malformed-topology macro behaviour and absence of new `dtc` warnings are
  build gates, not claimed measurements.

## 15. Amendments and contradictions against parent plan/design

1. Plan §3's M2 inventory omits
   `zephyr/dts/bindings/mfd/odp,pico-de-gallo.yaml`, although plan §8.3 requires
   deleting its transitional paragraph.
2. Plan §3 omits `zephyr/README.md` and `book/src/interfaces/spi.md`; both contain
   workflows made false by M2. AGENTS.md §15.1 requires bounded same-change
   parity despite plan §1 assigning final documentation to M6.
3. Plan §3 omits `zephyr/CHANGELOG.md`; removing two public properties and
   mandating parent-child topology is a breaking devicetree contract.
4. Plan calls M2 a pure refactor with no behaviour change. Happy-path transfer
   behaviour is unchanged, but initialization ownership/failure semantics are
   not: physical USB opens remain one, registry references become 3 → 1, parent
   failure coherently gates both children at priority 40, and worst-case strict
   validation attempts fall from as many as three to one. This is a net
   reliability improvement with a small independent-child availability cost.
5. Design §3's explicit shield serial is illustrative, not shipped. The shield
   parent remains selector-less and disabled by default; downstream samples
   enable it.
6. Design §4.3 says parent becomes the only open caller. After M2 it is the only
   **executed module ownership path**, but unused I2C/SPI/common bottom-half
   wrappers remain textually present. Their removal/ownership renaming carries
   to M4.
7. Design §3's `i2c`/`spi` node names are adopted. Absolute paths, generated
   identifiers, and ordinals change; labels remain stable. Exhaustive Zephyr
   consumers use labels, but the path change is documented as breaking.
8. M1 spec §5's no-parent-address/size-cells decision is confirmed because direct
   children lack `reg`/unit addresses. M2 adds an empirical no-new-warning gate.
9. M1's child contract required runtime readiness before accessor but did not
   prevent a ready foreign parent. M2 adds per-child compile-time compatible and
   status-okay assertions to close R9.
10. A third child assertion requires `CONFIG_MFD_PICO_DE_GALLO=y`. An okay
    compatible parent normally defaults it on, but an explicit downstream `n`
    override is possible; the assertion replaces an inferior ordinal link error
    with a readable configuration diagnostic.
11. M1 carry-over T10 cannot be strengthened: its harness was ephemeral,
    `zephyr/tests` does not exist, no tracked file mentions harness/T10, and the
    handoff tree was clean. Creating a framework contradicts M2 inventory.
12. Plan §4's non-vacuity regex, `pdg_[a-z_]*\.c`, is defective because it cannot
    match `pdg_i2c.c`. M2 replaces it with `pdg_[a-z0-9_]*\.c` and records exact
    baseline/post-change TU expectations.
13. Plan §4's generic sample gate is strengthened: primary four-build comparison
    omits `-DSHIELD` to match measured baseline; one additional clean explicit-
    shield build proves equivalence and determinism.
14. Plan/design did not distinguish malformed-topology ordinal failures from R5's
    benign ordinal failures. M2's readable compile-time assertions and
    baseline-clean negative-probe rule make them distinguishable.
15. Plan/design omitted I2C direct-call safety. The pre-existing R10 hole conflicts
    with M2's child-cache invalidation invariant and would be copied by M3, so M2
    initializes mutexes before early returns, guards all I2C callbacks, and clears
    every post-borrow failure path.
16. Parent binding's former “first matching board” guidance was insufficient.
    R11 requires stating that omission is safe only with exactly one matching
    physical board attached; one selector-less DT parent does not prove that.
17. Design's broad M6 documentation scope remains deferred: GPIO, `cs-gpios`,
    removal of `cs-gpio-indices`, M4 atomicity consequences, final topology/
    migration guide, and GPIO book chapter. M2 edits only currently false text.

## 16. Open questions

No implementation-blocking question remains. Exact installed-Zephyr macro
spellings and all build/link outcomes are explicit implementation-time gates,
not silently deferred design decisions.
