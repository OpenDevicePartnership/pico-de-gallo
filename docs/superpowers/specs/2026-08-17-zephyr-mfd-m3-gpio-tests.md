# Zephyr MFD restructure M3 — adversarial probe suite

Date: 2026-08-17
Branch: `zephyr`
Milestone: M3 — additive `pdg_gpio` controller
Source of truth: `docs/superpowers/specs/2026-08-17-zephyr-mfd-m3-gpio.md`
Written black-box against the spec. **No implementation was read; none exists.**

---

## 0. Assurance boundary — read this first

Nothing on this branch has ever executed (plan §9.3). M5 is the first milestone
that runs anything. There is no `zephyr/tests` directory and M2 resolved
"invent a test framework" as out of inventory. These probes are therefore
**ephemeral**: overlays and Kconfig fragments are materialised into `/tmp` at
execution time, greps are run from a shell, and nothing is added to the
repository.

Every probe below is labelled with exactly one class:

| Class | Meaning | Strength |
| --- | --- | --- |
| **A** | Compile-time provable now. A build that must succeed, or must fail with a *named* diagnostic substring. | Real gate. |
| **B** | Source-structural. A grep/awk assertion that a required construct exists, or a forbidden one does not. | Weaker than execution. Proves shape, never behaviour. |
| **C** | Requires execution. **Cannot be verified in M3 at all.** Listed, deferred, owned by a named later milestone. | No assurance in M3. |

**No Class C case is disguised as A or B.** Where a runtime behaviour has a
structural shadow, the structural probe is listed as B and the behaviour is
*separately* listed as C. Passing the B probe is not evidence for the C case.

### 0.1 Precedent this suite is bound by

- M1's harness test **T10 was explicitly criticised** for grepping the bare
  string `mix`, which an under-documented binding satisfied by luck
  (plan §8.3). Every Class B probe below therefore asserts on **structure with
  positional constraints** (function extents, byte offsets, exact counts),
  never on a lone word that could appear in prose.
- Plan §9.1 records that `pdg_[a-z_]*\.c` **can never match `pdg_i2c.c`** —
  digits are excluded. Every TU grep here uses `pdg_[a-z0-9_]*\.c`.
- #104 acceptance confirmed its suite by **re-introducing the bug** and
  observing 3 of 7 tests fail. §5 below specifies mutations for the strongest
  probes on the same principle.

---

## 1. Execution environment

### 1.1 The build command (do not substitute)

Plain `native_sim` is 32-bit; `zephyr/Kconfig:6` has `depends on 64BIT`, so
`CONFIG_PICO_DE_GALLO=n` and `zephyr/CMakeLists.txt:4` elides the entire module.
Every gate run that way is vacuous (plan §4).

```bash
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && source ~/zephyrproject/zephyr/zephyr-env.sh && export ZEPHYR_TOOLCHAIN_VARIANT=host && west build -p always -d /tmp/<DIR> -b native_sim/native/64 zephyr/samples/<NAME> -- -DEXTRA_DTC_OVERLAY_FILE=/tmp/<FILE>.overlay'
```

From PowerShell use **single** quotes around the bash string (plan §2 R6).
Build directories must be under `/tmp` — the default drops a non-gitignored
`build/` at the repo root.

### 1.2 Capture convention

Every Class A probe is run as:

```bash
... 2>&1 | tee /tmp/m3-<ID>.log ; echo "EXIT=${PIPESTATUS[0]}"
```

Pass criteria are stated against `/tmp/m3-<ID>.log` and `EXIT`.

**A probe that passes on any nonzero exit is worthless.** The tree already
contains two samples that fail for unrelated pre-existing reasons (`spi_bridge`
and `combined_i2c_spi_bridge` cannot link — `issi,is31fl3743b` exists nowhere;
plan §2 R5). Every negative probe below therefore requires a **specific
diagnostic substring**, and several additionally require the *absence* of a
substring.

### 1.3 Base sample

All Class A probes except A-10 use **`zephyr/samples/i2c_bridge`**, which M2
verified builds clean with `CONFIG_PICO_DE_GALLO=y` and
`CONFIG_MFD_PICO_DE_GALLO=y` (plan §9). Its `app.overlay` already sets
`&pdg0 { status = "okay"; }` and enables `pdg_i2c0`, so an extra overlay need
only add the GPIO node state and the parent serial.

Using a sample with a clean baseline is load-bearing: a negative probe on
`spi_bridge` could not distinguish the assertion we want from the pre-existing
link failure.

### 1.4 Build cost and `/tmp` budget

`/tmp` is a 16 GB tmpfs; each Zephyr build directory is ~200–500 MB (plan R12).

| Build dir | Probes served | Note |
| --- | --- | --- |
| `/tmp/m3-pos` | A-01, A-11, A-12 companion control | Positive/permissive builds; must be inspected before reuse, so run A-01 first and archive `.config` + `compile_commands.json` to `/tmp/m3-a01/` before the next build overwrites them. |
| `/tmp/m3-neg` | A-02 … A-09, A-12 | All fail at compile. `-p always` makes sequential reuse safe. Copy each log out before the next run. |
| `/tmp/m3-base-{1..4}` | A-10 | Four-sample category preservation. May reuse M2's existing dirs if still present. |

Total ≈ 6 concurrent directories worst case, ~3 GB. Class B probes cost nothing.

---

## 2. Shared helpers for Class B probes

Class B probes must not grep the whole file for a token; that is the T10
mistake. They operate on **function extents**. Define once:

```bash
# extent <file> <function-name>
# Prints the source text of one top-level function, from its definition line
# to the closing brace at column 1. Requires the project brace style used by
# pdg_i2c.c (opening brace at column 1 of the following line, closing brace at
# column 1). Fails loudly if the function is not found.
extent() {
  awk -v fn="$2" '
    $0 ~ "^(static )?[a-zA-Z_].*[ *]" fn "\\(" { cap=1 }
    cap { print }
    cap && /^}/ { exit }
  ' "$1" | { out=$(cat); [ -n "$out" ] || { echo "EXTENT-NOT-FOUND:$2" >&2; exit 9; }; printf '%s\n' "$out"; }
}

SRC=zephyr/drivers/gpio/pdg_gpio.c
BOT=zephyr/drivers/gpio/pdg_gpio_bottom.c
BOTH=zephyr/drivers/gpio/pdg_gpio_bottom.h
BIND=zephyr/dts/bindings/gpio/odp,pico-de-gallo-gpio.yaml

# norm: collapse whitespace so a reflowed expression still matches.
norm() { tr -s ' \t\n' ' '; }

# off <file> <extended-regex>: byte offset of first match, or -1.
off() { grep -abo -E "$2" "$1" | head -n1 | cut -d: -f1 | grep -q . && grep -abo -E "$2" "$1" | head -n1 | cut -d: -f1 || echo -1; }
```

The six callback names referenced throughout:

```
pdg_gpio_pin_configure
pdg_gpio_port_get_raw
pdg_gpio_port_set_masked_raw
pdg_gpio_port_set_bits_raw
pdg_gpio_port_clear_bits_raw
pdg_gpio_port_toggle_bits
```

**`extent` failing to find a function is a probe FAILURE, not a skip.** This is
the anti-vacuity guard for the whole Class B set: if the implementation renames
a callback, the probes must go red rather than silently match nothing.

---

## 3. Class A — compile-time gates

### M3-A-01 — Positive control: `pdg_gpio.c` is actually compiled and linked

**Class:** A
**Intent:** Prevent the entire suite from passing vacuously against a driver
that is never built.

**Artefact** — `/tmp/m3-enable-gpio.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "5256657D8A5D7F03";
};

&pdg_gpio0 {
	status = "okay";
};
```

**Command:** base command, `-d /tmp/m3-pos`, sample `zephyr/samples/i2c_bridge`,
`-DEXTRA_DTC_OVERLAY_FILE=/tmp/m3-enable-gpio.overlay`.

**Pass criterion — all six must hold:**

1. `EXIT=0`.
2. `grep -c '^CONFIG_PICO_DE_GALLO=y$' /tmp/m3-pos/zephyr/.config` = 1
3. `grep -c '^CONFIG_MFD_PICO_DE_GALLO=y$' /tmp/m3-pos/zephyr/.config` = 1
4. `grep -c '^CONFIG_GPIO=y$' /tmp/m3-pos/zephyr/.config` = 1
5. `grep -c '^CONFIG_GPIO_PICO_DE_GALLO=y$' /tmp/m3-pos/zephyr/.config` = 1
   and `grep -c '^CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY=45$' ... ` = 1
6. `grep -o 'pdg_[a-z0-9_]*\.c' /tmp/m3-pos/compile_commands.json | sort -u`
   contains **both** `pdg_gpio.c` and `pdg_gpio_bottom.c`, and still contains
   `pdg_mfd.c` and `pdg_i2c.c`.

   > Note the `0-9` in the character class. Its omission produced a false
   > negative on this exact check in M1 (plan §9.1). Verify the class is
   > present in the command actually run.

7. No new devicetree warning: `grep -c 'Warning' /tmp/m3-pos/zephyr/*.log`
   (and the build log) matches the M2 baseline count for `i2c_bridge`.

**Caveat, stated honestly:** `pdg_gpio_bottom.c` is compiled by the
native-simulator Makefile via `target_sources(native_simulator INTERFACE ...)`,
exactly as `common.c` is — and `zephyr/drivers/CMakeLists.txt` already documents
that such files **never appear in `compile_commands.json`**. So criterion 6 can
only be required for `pdg_gpio.c`. For the bottom half, substitute:

6b. `find /tmp/m3-pos -name 'pdg_gpio_bottom*.o' -o -name '*pdg_gpio_bottom*' | grep -q .`
    — the object file must exist somewhere under the build tree.

If 6b cannot be satisfied because of the native-simulator build layout, the
fallback of last resort is to require the linked ELF to contain the symbol:
`nm /tmp/m3-pos/zephyr/zephyr.exe | grep -c ' T pdg_gpio_bottom_num_gpios'` = 1.
**Do not skip this.** Without it the bottom half is unproven.

**Build cost:** 1 full build (~500 MB, several minutes).

---

### M3-A-02 — GPIO child at devicetree root

**Class:** A
**Intent:** A GPIO node that is not a child of a PDG parent must fail with the
readable *compatible* assertion, not with an opaque include error or an
unresolved `__device_dts_ord_N` at link.

**Artefact** — `/tmp/m3-root-gpio.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "5256657D8A5D7F03";
};

/ {
	stray_gpio: stray-gpio {
		compatible = "odp,pico-de-gallo-gpio";
		gpio-controller;
		#gpio-cells = <2>;
		ngpios = <4>;
		status = "okay";
	};
};
```

`DT_INST_PARENT` of a root-level child is `/`, which carries no
`odp_pico_de_gallo` compatible.

**Pass criterion:**

- `EXIT != 0`, and
- log contains the substring
  `must be direct children of an odp,pico-de-gallo parent`, and
- log **does not** contain `pdg_mfd.h: No such file`, and
- log **does not** contain `undefined reference to \`__device_dts_ord_`.

The two negative conditions are the point of the probe. A build that merely
fails proves nothing.

**Build cost:** compile-stage failure, ~1–2 min, `/tmp/m3-neg`.

---

### M3-A-03 — GPIO child under a ready but unrelated parent

**Class:** A
**Intent:** R9's exact hazard. A child under an enabled, ready, *foreign* device
passes `device_is_ready()` and `pdg_mfd_ctx()` would cast that driver's
`dev->data` to `struct pdg_mfd_data`, yielding a non-NULL garbage pointer no
NULL check can catch. Must be rejected at compile time.

**Artefact** — `/tmp/m3-foreign-parent.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "5256657D8A5D7F03";
};

&uart0 {
	foreign_gpio: gpio {
		compatible = "odp,pico-de-gallo-gpio";
		gpio-controller;
		#gpio-cells = <2>;
		ngpios = <4>;
		status = "okay";
	};
};
```

> If `uart0` is not the label present in this Zephyr revision's `native_sim`
> devicetree, substitute any enabled node that has a bound driver; resolve it
> from `/tmp/m3-pos/zephyr/include/generated/zephyr/devicetree_generated.h`
> produced by A-01. **Record which node was used in the probe log** — a
> substituted node that turns out to be `status = "disabled"` would make this
> probe collapse into A-04 and prove the wrong thing.

**Pass criterion:** identical to A-02 (same compatible-assertion substring, same
two absence conditions).

**Build cost:** shares `/tmp/m3-neg`.

---

### M3-A-04 — GPIO child under a *disabled* PDG parent, and assertion ordering

**Class:** A
**Intent:** Two things at once. (a) The parent-status assertion fires. (b) It
is emitted **before** the Kconfig assertion, which is the assertion-ordering
requirement of spec §4.1 and mandatory-coverage item 8.

Disabling the parent also drops `DT_HAS_ODP_PICO_DE_GALLO_ENABLED`, which makes
`CONFIG_MFD_PICO_DE_GALLO=n`, so assertions 2 **and** 4 are simultaneously
false. `_Static_assert` is non-fatal, so GCC reports both in one pass — which is
precisely what makes the ordering observable.

**Artefact** — `/tmp/m3-disabled-parent.overlay`:

```dts
&pdg0 {
	status = "disabled";
};

&pdg_i2c0 {
	status = "disabled";
};

&pdg_gpio0 {
	status = "okay";
};
```

(The I2C child is disabled too, so the only reported assertions come from the
GPIO driver and the ordering is unambiguous.)

**Pass criterion — all five:**

1. `EXIT != 0`.
2. Log contains `require their odp,pico-de-gallo parent to have status okay`.
3. Log contains `require CONFIG_MFD_PICO_DE_GALLO=y`.
4. **Ordering:** the byte offset of the first occurrence of (2) is strictly less
   than the byte offset of the first occurrence of (3):

```bash
S=$(grep -abo 'to have status okay' /tmp/m3-A04.log | head -1 | cut -d: -f1)
K=$(grep -abo 'CONFIG_MFD_PICO_DE_GALLO=y' /tmp/m3-A04.log | head -1 | cut -d: -f1)
[ -n "$S" ] && [ -n "$K" ] && [ "$S" -lt "$K" ]
```

5. If the log contains `pdg_mfd.h: No such file`, the byte offsets of both assertion diagnostics must be strictly less than the first include-error offset. Absence of the include error also satisfies this condition.

Conditions 4 and 5 are the ordering tests. Condition 4 is genuinely diagnostic: swapping the two `BUILD_ASSERT`s in the source inverts the offsets, because GCC emits static assertions in source order. Condition 5 enforces spec §4.1: an include error may follow, but it must not mask the readable assertions.

**Known fragility, stated:** if a future compiler batches or reorders
`_Static_assert` diagnostics, condition 4 becomes unreliable. It is backed by
the purely textual **M3-B-08**, which asserts the source order directly. Both
must pass; neither alone is sufficient.

**Build cost:** shares `/tmp/m3-neg`.

---

### M3-A-05 — Enabled GPIO child whose parent has no `serial-number`

**Class:** A
**Intent:** The R11 mitigation newly required by spec §4.1 assertion 3. A
selector-less strict open cannot report which attached board it selected, so
unidentifiable physical-pin actuation must be rejected at build time.

**Artefact** — `/tmp/m3-no-serial.overlay`:

```dts
&pdg0 {
	status = "okay";
};

&pdg_gpio0 {
	status = "okay";
};
```

Note `serial-number` is deliberately absent, and the shield's default `pdg0` has
none. `i2c_bridge`'s own `app.overlay` also does not set one, so this is the
*natural* state — which is exactly why the assertion is needed.

**Pass criterion:**

1. `EXIT != 0`.
2. Log contains
   `odp,pico-de-gallo-gpio parent must define serial-number`.
3. Log **does not** contain `to have status okay` (the parent *is* okay — if
   that assertion also fires, the probe has caught an ordering or predicate bug).
4. Log **does not** contain `must be direct children of an odp,pico-de-gallo parent`.
5. Log **does not** contain `CONFIG_MFD_PICO_DE_GALLO=y` (MFD is enabled here).

Conditions 3–5 make this the **isolating** probe for assertion 3: it must be the
only assertion that fires.

**Build cost:** shares `/tmp/m3-neg`.

---

### M3-A-06 — MFD Kconfig off with GPIO enabled: assertion must beat the include

**Class:** A
**Intent:** Mandatory-coverage item 9. `add_subdirectory_ifdef(CONFIG_MFD_PICO_DE_GALLO mfd)`
drops `pdg_mfd.h` from the include path when the Kconfig is off. If the
assertion block sits **below** the include, the readable configuration error is
masked by a fatal `No such file or directory`.

**Artefact** — reuse `/tmp/m3-enable-gpio.overlay` from A-01, plus
`/tmp/m3-nomfd.conf`:

```
CONFIG_MFD_PICO_DE_GALLO=n
```

**Command:** base command with both
`-DEXTRA_DTC_OVERLAY_FILE=/tmp/m3-enable-gpio.overlay` and
`-DEXTRA_CONF_FILE=/tmp/m3-nomfd.conf`.

**Pass criterion:**

1. `EXIT != 0`.
2. `grep -c '^CONFIG_MFD_PICO_DE_GALLO=y$' /tmp/m3-neg/zephyr/.config` = 0
   — **prove the fragment took effect.** Without this the probe is vacuous:
   `MFD_PICO_DE_GALLO` has `default y` and all its dependencies are met, so a
   fragment that is silently ignored would leave the driver enabled and the
   probe would fail for the wrong reason (or pass for none).
3. Log contains `require CONFIG_MFD_PICO_DE_GALLO=y`.
4. The byte offset of `require CONFIG_MFD_PICO_DE_GALLO=y` is strictly less than the first `pdg_mfd.h: No such file or directory` offset. Absence of the include error also satisfies this condition.

Condition 4 is the whole probe: the include error may follow because `_Static_assert` is non-fatal, but it must not mask the readable assertion. Conditions 2 and 3 stop the probe being satisfied by an unrelated failure.

**Risk:** the I2C child is also enabled in `i2c_bridge` and will emit its own
identical Kconfig assertion. That is harmless for condition 3 but makes
condition 4 ambiguous about *which* driver's include was reached. **Mitigation:**
extend the overlay to disable `pdg_i2c0` (`&pdg_i2c0 { status = "disabled"; };`)
so `pdg_gpio.c` is the only PDG child compiled. Record that this was done.

**Build cost:** shares `/tmp/m3-neg`.

---

### M3-A-07 — `ngpios` above the supported width

**Class:** A
**Intent:** Spec §3.1 requires `ngpios` bounded to 1..32 (the supported
`gpio_port_pins_t` width). **See §7 defect D1 — the spec does not say what
enforces this, and a Zephyr binding YAML cannot express a numeric range.** This
probe asserts a `BUILD_ASSERT` in the driver does.

**Artefact** — `/tmp/m3-ngpios-33.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "5256657D8A5D7F03";
};

&pdg_gpio0 {
	status = "okay";
	ngpios = <33>;
};
```

**Pass criterion:**

1. `EXIT != 0`.
2. Log contains a diagnostic naming both `ngpios` and the bound — the exact
   substring is implementation-chosen but **must be registered here by the coder
   before this probe is run**, and must contain the literal token `ngpios`.
   Suggested: `odp,pico-de-gallo-gpio ngpios must be between 1 and 32`.
3. Log **does not** contain `undefined reference` (i.e. it fails at compile,
   not at link).

**If no such assertion exists**, the build will instead succeed and
`GPIO_COMMON_CONFIG_FROM_DT_INST` will produce a 33-bit mask truncated into a
32-bit `gpio_port_pins_t` — silent, and exactly the class of defect this
milestone exists to avoid. A successful build is a **FAIL** of this probe.

**Build cost:** shares `/tmp/m3-neg`.

---

### M3-A-08 — `ngpios = <0>`

**Class:** A
**Intent:** Lower bound. Zero GPIOs yields `port_pin_mask == 0`, under which
every masked write is a no-op that returns success — a confident lie.

**Artefact:** as A-07 with `ngpios = <0>;`.

**Pass criterion:** identical to A-07, same registered diagnostic substring.

**Build cost:** shares `/tmp/m3-neg`.

---

### M3-A-09 — GPIO node present but `status = "disabled"` (shield default)

**Class:** A
**Intent:** Anti-over-enablement. The shield must ship the GPIO child disabled
(spec §3.2); M4 enables it. If the default shield state compiles the driver,
M3 has silently changed every existing sample's boot path.

**Artefact:** none — build `zephyr/samples/i2c_bridge` with **no** extra overlay.

**Pass criterion:**

1. `EXIT = 0`.
2. `grep -c '^CONFIG_GPIO_PICO_DE_GALLO=y$' /tmp/m3-pos/zephyr/.config` = 0.
3. `grep -o 'pdg_[a-z0-9_]*\.c' /tmp/m3-pos/compile_commands.json | sort -u`
   **does not** contain `pdg_gpio.c`, but **does** contain `pdg_mfd.c` and
   `pdg_i2c.c`.

Criterion 3's positive half is the anti-vacuity guard: it proves the grep works
at all on this build.

**Build cost:** 1 build; run before A-01 in `/tmp/m3-pos`, archiving artefacts.

---

### M3-A-10 — Out-of-range GPIO cell in a phandle is **not** a devicetree gate

**Class:** A
**Intent:** Mandatory-coverage item 4, mechanism disambiguation. A test that
cannot tell *which* layer enforces a pin bound proves little. This probe
establishes, positively, that devicetree does **not** range-check GPIO cells
against `ngpios` — so pin 255 and pin `ngpios` are *not* build-time errors and
any claim that they are is false.

**Artefact** — `/tmp/m3-oob-cell.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "5256657D8A5D7F03";
};

&pdg_gpio0 {
	status = "okay";
};

/ {
	zephyr,user {
		probe-gpios = <&pdg_gpio0 255 0>;
	};
};
```

**Pass criterion:**

1. `EXIT = 0` — **the build must SUCCEED.**
2. Log contains no new devicetree error or warning mentioning `probe-gpios`.

**What this proves:** the pin bound at 255 (and at `ngpios`) is enforced
**only** by (i) Zephyr's `port_pin_mask` assertion at `gpio.h:1040`, which is
compiled out under `CONFIG_ASSERT=n`; (ii) the driver's defensive
`pin >= config->ngpios` check in `pin_configure` (structure asserted by
**M3-B-20**, behaviour deferred to **M3-C-07**); and (iii) the firmware's own
`GpioInvalidPin`. Three different mechanisms with three different failure
signatures, none of them devicetree.

**What it does not prove:** that any of those three mechanisms actually work.

**Build cost:** 1 build in `/tmp/m3-pos` (run last).

---

### M3-A-11 — Four-sample category preservation

**Class:** A
**Intent:** M3 is additive; it must not change any sample's build category.

**Command:** build all four samples unmodified into `/tmp/m3-base-{1..4}`.

**Pass criterion:**

| Sample | Required outcome |
| --- | --- |
| `i2c_bridge` | `EXIT = 0` |
| `spi_nor_id` | `EXIT = 0` |
| `spi_bridge` | `EXIT != 0`, failing **only** on the pre-existing `issi,is31fl3743b` cause |
| `combined_i2c_spi_bridge` | same |

For the two failing samples, compare **structurally, never on the literal
ordinal** (plan §9.2 — the two baseline ordinals already differed from each
other at the same commit, 44 vs 45, and nesting renumbered them to 48/49):

```bash
# undefined-symbol count
grep -c 'undefined reference to' /tmp/m3-base-3.log
# resolve each __device_dts_ord_N to a node path from THIS build
grep -o '__device_dts_ord_[0-9]*' /tmp/m3-base-3.log | sort -u | while read s; do
  n=${s##*_}
  grep -n "DT_N_.*ORD $n\$" /tmp/m3-base-3/zephyr/include/generated/zephyr/devicetree_generated.h
done
```

Compare **(undefined-symbol count, set of resolved node paths)** against the M2
baseline. A literal string comparison produces a false regression.

**Pass:** counts equal and resolved node-path sets equal.

**Build cost:** 4 builds, ~2 GB. May reuse M2's dirs if intact.

---

### M3-A-12 — Malformed downstream node: `#gpio-cells = <1>`

**Class:** A
**Intent:** Spec §3.1's binding constrains the cell count. `gpio-cells: [pin,
flags]` only *names* the cells; it does not constrain the property, so without
an explicit `const: 2` a downstream node may declare `#gpio-cells = <1>` or
`<3>` and be accepted by edtlib, failing later, elsewhere, or inconsistently —
a phandle-array consumer would then silently mis-parse the specifier. This
probe asserts the binding rejects a wrong cell count at devicetree-processing
time.

It also, in passing, proves that the `const: 2` override merges cleanly against
`gpio-controller.yaml`'s `required: true` for `#gpio-cells`, which was the
stated reason the override was originally omitted. Upstream precedent for the
same override: `~/zephyrproject/zephyr/dts/bindings/gpio/nordic,nrf-gpio.yaml:19-20`.

**Artefact** — `/tmp/m3-gpiocells-1.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "5256657D8A5D7F03";
};

&pdg_gpio0 {
	status = "okay";
	#gpio-cells = <1>;
};
```

**Pass criterion:**

1. `EXIT != 0`.
2. Log contains the edtlib diagnostic naming the property and the constraint.
   **Registered substring (observed):**

   ```
   devicetree error: value of property '#gpio-cells' on /pico-de-gallo/gpio in
   <build>/zephyr/zephyr.dts.pre (1) is different from the 'const' value
   specified in .../odp,pico-de-gallo-gpio.yaml (2)
   ```

   The minimal matched tokens are the literals `#gpio-cells` and `'const'`,
   which must appear together with a non-zero exit **and** with no `undefined
   reference` in the log (i.e. it fails during devicetree processing, not at
   link).
3. The build must **not** reach compilation: `compile_commands.json` is absent
   from the build directory.

Criterion 3 is the anti-vacuity guard. Two samples in this tree already fail at
the native_simulator link for unrelated pre-existing reasons, so "the build
failed" on its own proves nothing; failing before `compile_commands.json` is
even generated distinguishes a devicetree gate from a link failure.

**A companion positive control is mandatory:** the same overlay with
`#gpio-cells = <2>;` must build to the A-01 positive-control outcome. Without
it, this probe cannot distinguish "the binding rejects `<1>`" from "the binding
rejects any `#gpio-cells` override at all", which would mean the `const: 2`
merge is broken rather than working.

**Build cost:** shares `/tmp/m3-neg`; the companion control shares `/tmp/m3-pos`.

---

## 4. Class B — source-structural probes

Every probe in this section proves **shape only**. None of them proves the
driver behaves correctly at runtime; see §6.

### M3-B-01 — Exactly six API slots, no interrupt fields

**Class:** B

```bash
API=$(awk '/DEVICE_API\(gpio, pdg_gpio_api\)/,/^};/' "$SRC")
# exactly six assignments
[ "$(printf '%s\n' "$API" | grep -c '^\s*\.[a-z_]* *=')" -eq 6 ]
for f in pin_configure port_get_raw port_set_masked_raw \
         port_set_bits_raw port_clear_bits_raw port_toggle_bits; do
  printf '%s\n' "$API" | grep -q "^\s*\.$f *= *pdg_gpio_$f,\?\s*$" || exit 1
done
# forbidden slots absent
for f in pin_interrupt_configure manage_callback get_pending_int \
         pin_get_config port_get_direction; do
  printf '%s\n' "$API" | grep -q "\.$f" && exit 1
done
exit 0
```

**Proves:** the API object has exactly the six unconditionally-dispatched slots
non-NULL and no interrupt fields (spec §6, invariant 10).
**Does not prove:** any of the six behaves correctly, or that `-ENOSYS` is
actually what a caller sees from the omitted five.

---

### M3-B-02 — **The §6.2 ↔ §7 coupling probe** (highest value in this suite)

**Class:** B
**Intent:** Spec §6.2 states that `port_get_raw`'s zero-for-output rule is valid
*only* because `pin_configure` rejects `GPIO_INPUT | GPIO_OUTPUT` with
`-ENOTSUP`, and the two must never be changed independently. If that coupling
rots, the driver returns confident false levels — and under `GPIO_ACTIVE_LOW` a
false logical `1`, because `z_impl_gpio_port_get` XORs `data->invert` over our
zero (`gpio.h:1322-1325`). That is the issue-#104 failure mode reborn.

**Requirement placed on the implementation (must be stated to the coder before
they write it):** `pdg_gpio.c` must contain the token
`PDG_GPIO_COUPLING_6_2_7` **exactly twice** — once inside the body of
`pdg_gpio_port_get_raw`, once inside the body of `pdg_gpio_pin_configure`,
each in a comment that cites the other site. The token is an anchor, not the
evidence; the evidence is conjuncts (b) and (c).

**Artefact:**

```bash
set -e
SRC=zephyr/drivers/gpio/pdg_gpio.c

GET=$(extent "$SRC" pdg_gpio_port_get_raw)
CFG=$(extent "$SRC" pdg_gpio_pin_configure)

fail() { echo "M3-B-02 FAIL: $1"; exit 1; }

# (a) anchor present exactly twice, once in each extent, and nowhere else.
[ "$(grep -c 'PDG_GPIO_COUPLING_6_2_7' "$SRC")" -eq 2 ] \
  || fail "anchor token count != 2 in file"
[ "$(printf '%s\n' "$GET" | grep -c 'PDG_GPIO_COUPLING_6_2_7')" -eq 1 ] \
  || fail "anchor missing from port_get_raw extent"
[ "$(printf '%s\n' "$CFG" | grep -c 'PDG_GPIO_COUPLING_6_2_7')" -eq 1 ] \
  || fail "anchor missing from pin_configure extent"

# (b) the read-masking behaviour: port_get_raw must special-case EACCES and
#     continue, and must NOT return it.
printf '%s\n' "$GET" | grep -q 'EACCES' \
  || fail "port_get_raw does not mention EACCES: output masking deleted"
printf '%s\n' "$GET" | grep -Eq 'continue|/\* skip' \
  || fail "port_get_raw does not continue past an EACCES pin"
printf '%s\n' "$GET" | grep -Eq 'return[[:space:]]+-EACCES' \
  && fail "port_get_raw propagates -EACCES; gpio.h:1275-1277 does not enumerate it"

# (c) the §7 rejection: pin_configure must contain the exact detection
#     expression AND return -ENOTSUP for it.
printf '%s\n' "$CFG" | norm | grep -qF \
  '(flags & (GPIO_INPUT | GPIO_OUTPUT)) == (GPIO_INPUT | GPIO_OUTPUT)' \
  || fail "pin_configure lacks the input+output detection expression"
printf '%s\n' "$CFG" | norm | grep -Eq \
  '\(flags & \(GPIO_INPUT \| GPIO_OUTPUT\)\) == \(GPIO_INPUT \| GPIO_OUTPUT\)\).{0,160}-ENOTSUP' \
  || fail "input+output is detected but not rejected with -ENOTSUP"

# (d) the coupling must also be recorded where a reader will meet it: the
#     binding description must state both halves.
grep -qi 'GPIO_INPUT | GPIO_OUTPUT' "$BIND" \
  || fail "binding does not document the input+output rejection"

echo "M3-B-02 PASS"
```

**Pass criterion:** the script exits 0 and prints `M3-B-02 PASS`.

**Why deleting *either* half fails it:**

| Deletion | Failing conjunct |
| --- | --- |
| Remove the `-EACCES` skip from `port_get_raw` (e.g. propagate it, or drop the branch) | (b) — `EACCES` absent, or `return -EACCES` present |
| Remove the `GPIO_INPUT | GPIO_OUTPUT` rejection from `pin_configure` | (c) — expression absent, or not followed by `-ENOTSUP` |
| Delete either function body wholesale | (a) — anchor count drops to 1, and `extent` returns `EXTENT-NOT-FOUND` |
| Relax the rejection from `-ENOTSUP` to acceptance | (c) second half |
| Rename a callback to evade the probe | `extent` fails loudly (exit 9) |

**Honest limits.** This is Class B. It proves the two constructs are *present
and mutually anchored in source*. It does **not** prove that
`gpio_port_get_raw()` returns 0 for an output pin at runtime, nor that
`gpio_pin_configure(GPIO_INPUT | GPIO_OUTPUT)` actually returns `-ENOTSUP` when
called. Those are **M3-C-01** and **M3-C-02**, deferred to M5. A reviewer must
not report this probe as behavioural evidence.

**Mutation control:** see §5, mutation M1.

---

### M3-B-03 — `k_is_in_isr()` guard in all six callbacks

**Class:** B

```bash
[ "$(grep -c 'k_is_in_isr()' "$SRC")" -eq 6 ] || fail "not exactly 6 ISR guards"
for fn in pdg_gpio_pin_configure pdg_gpio_port_get_raw \
          pdg_gpio_port_set_masked_raw pdg_gpio_port_set_bits_raw \
          pdg_gpio_port_clear_bits_raw pdg_gpio_port_toggle_bits; do
  E=$(extent "$SRC" "$fn") || fail "missing $fn"
  printf '%s\n' "$E" | norm | grep -Eq 'if \(k_is_in_isr\(\)\) \{ ?return -EWOULDBLOCK;' \
    || fail "$fn: no k_is_in_isr -> -EWOULDBLOCK guard"
done
```

**Proves:** the guard exists in every callback and returns `-EWOULDBLOCK`
(design §4.7, spec §6).
**Does not prove:** it is *first*, that ISR context is actually detected, or
that `-EWOULDBLOCK` reaches the caller. Ordering is B-04; behaviour is M3-C-09.

---

### M3-B-04 — ISR guard, then NULL guard, then lock — in that order

**Class:** B
**Intent:** Spec §6 invariant 6 and the R10 pattern from `pdg_i2c.c`. A lock
taken before the `ctx == NULL` check means a failed child locks an
uninitialised mutex.

```bash
for fn in <the six>; do
  E=$(extent "$SRC" "$fn")
  printf '%s\n' "$E" > /tmp/e.$fn
  ISR=$(off /tmp/e.$fn 'k_is_in_isr')
  NUL=$(off /tmp/e.$fn '(data->)?ctx == NULL')
  LCK=$(off /tmp/e.$fn 'k_mutex_lock')
  [ "$ISR" -ge 0 ] || fail "$fn: no ISR guard"
  [ "$NUL" -ge 0 ] || fail "$fn: no NULL ctx guard"
  [ "$ISR" -lt "$NUL" ] || fail "$fn: NULL guard precedes ISR guard"
  if [ "$LCK" -ge 0 ]; then
    [ "$NUL" -lt "$LCK" ] || fail "$fn: lock taken before NULL guard"
  fi
done
```

`port_toggle_bits` legitimately has no lock (`LCK = -1`); the conditional
handles that, and **M3-B-13** asserts that absence positively.

**Proves:** textual ordering of the three guards.
**Does not prove:** that a NULL `ctx` is reachable, or that `-ENODEV` results.

---

### M3-B-05 — `k_mutex_init` precedes every return in `pdg_gpio_init`

**Class:** B

```bash
E=$(extent "$SRC" pdg_gpio_init); printf '%s\n' "$E" > /tmp/e.init
INIT=$(off /tmp/e.init 'k_mutex_init')
[ "$INIT" -ge 0 ] || fail "no k_mutex_init"
FIRSTRET=$(off /tmp/e.init 'return')
[ "$INIT" -lt "$FIRSTRET" ] || fail "a return precedes k_mutex_init"
```

**Proves:** the mutex is initialised before the earliest textual return
(spec §4.1 step 1, invariant 3).
**Does not prove:** the absence of a `goto`-based path around it. Reviewer must
confirm no `goto` exists in `pdg_gpio_init`: `printf '%s\n' "$E" | grep -q goto`
must be false — add that as a conjunct.

---

### M3-B-06 — No cache of level, direction, or pull

**Class:** B
**Intent:** Design §6 and spec invariant 8. A cache has to live somewhere: in
`struct pdg_gpio_data`, or in a file-scope mutable.

```bash
D=$(awk '/^struct pdg_gpio_data \{/,/^};/' "$SRC")
# exactly three members, exactly these
[ "$(printf '%s\n' "$D" | grep -cE '^\s+[a-zA-Z_].*;')" -eq 3 ] || fail "unexpected member count"
printf '%s\n' "$D" | grep -q 'struct gpio_driver_data common;' || fail
printf '%s\n' "$D" | grep -q 'void \*ctx;'                     || fail
printf '%s\n' "$D" | grep -q 'struct k_mutex lock;'            || fail
# common must be FIRST (gpio.h casts data directly)
printf '%s\n' "$D" | grep -nE '^\s+[a-zA-Z_].*;' | head -1 | grep -q 'gpio_driver_data common' || fail

C=$(awk '/^struct pdg_gpio_config \{/,/^};/' "$SRC")
printf '%s\n' "$C" | grep -nE '^\s+[a-zA-Z_].*;' | head -1 | grep -q 'gpio_driver_config common' || fail

# no file-scope mutable statics other than the generated per-instance objects
grep -nE '^static [a-z].*;' "$SRC" \
  | grep -vE 'pdg_gpio_(data|config)_' \
  | grep -vE '^\s*[0-9]+:static (const )?struct (gpio_driver_api|.*_api)' \
  | grep -q . && fail "file-scope mutable state present (candidate cache)"
```

**Proves:** there is no field or file-scope variable in which a firmware pin
state could be cached, and the common prefixes are first.
**Does not prove:** that every query actually reaches firmware — a cache could
in principle live in the bottom half. Add the same file-scope check to `$BOT`.
Runtime "every query reaches firmware" is **M3-C-05**.

---

### M3-B-07 / M3-B-08 — Assertion block placement and order (textual)

**Class:** B
**Intent:** Backs A-06 and A-04 with a diagnostic-independent assertion.

```bash
INC=$(off "$SRC" '#include "pdg_mfd.h"')
[ "$INC" -ge 0 ] || fail "pdg_mfd.h never included"
FE=$(off "$SRC" 'DT_INST_FOREACH_STATUS_OKAY\(PDG_GPIO_PARENT_ASSERTS\)')
[ "$FE" -ge 0 ] || fail "no per-instance assertion expansion"
[ "$FE" -lt "$INC" ] || fail "B-07: assertion block sits BELOW the pdg_mfd.h include"

# B-08: order inside the assertion macro
M=$(awk '/#define PDG_GPIO_PARENT_ASSERTS/,/^$/' "$SRC"); printf '%s\n' "$M" > /tmp/e.asrt
C1=$(off /tmp/e.asrt 'DT_NODE_HAS_COMPAT\(DT_INST_PARENT')
C2=$(off /tmp/e.asrt 'DT_NODE_HAS_STATUS_OKAY\(DT_INST_PARENT')
C3=$(off /tmp/e.asrt 'DT_NODE_HAS_PROP\(DT_INST_PARENT\(inst\), serial_number\)')
C4=$(off /tmp/e.asrt 'IS_ENABLED\(CONFIG_MFD_PICO_DE_GALLO\)')
for v in $C1 $C2 $C3 $C4; do [ "$v" -ge 0 ] || fail "B-08: an assertion is missing"; done
[ "$C1" -lt "$C2" ] && [ "$C2" -lt "$C3" ] && [ "$C3" -lt "$C4" ] \
  || fail "B-08: assertion order is not compatible -> status -> serial -> Kconfig"
```

**Proves:** placement above the include, and the fixed four-step order.
**Does not prove:** which diagnostic a human actually reads first — that is
A-04 condition 4. **Both are required**; A-04 can rot under a compiler change,
B-08 cannot detect a compiler that reorders output.

---

### M3-B-09 — Every §7 flag row is detected, with the correct errno

**Class:** B
**Intent:** Mandatory-coverage item 1, structural half. Ten rows, each asserted
individually with its exact detection expression and its exact result.

```bash
CFG=$(extent "$SRC" pdg_gpio_pin_configure); printf '%s\n' "$CFG" | norm > /tmp/e.cfg

check() { grep -qF "$1" /tmp/e.cfg || fail "missing detection: $1"
          grep -Eq "$2" /tmp/e.cfg || fail "wrong result for: $1"; }

check '(flags & (GPIO_INPUT | GPIO_OUTPUT)) == 0U' \
      '\(flags & \(GPIO_INPUT \| GPIO_OUTPUT\)\) == 0U\).{0,160}-ENOTSUP'
check '(flags & (GPIO_INPUT | GPIO_OUTPUT)) == (GPIO_INPUT | GPIO_OUTPUT)' \
      'GPIO_INPUT \| GPIO_OUTPUT\)\).{0,160}-ENOTSUP'
check '(flags & GPIO_SINGLE_ENDED) != 0U' \
      'GPIO_SINGLE_ENDED\) != 0U\).{0,160}-ENOTSUP'
check '(flags & GPIO_LINE_OPEN_DRAIN) != 0U' \
      'GPIO_LINE_OPEN_DRAIN\) != 0U\).{0,160}-ENOTSUP'
check '(flags & (GPIO_PULL_UP | GPIO_PULL_DOWN)) == (GPIO_PULL_UP | GPIO_PULL_DOWN)' \
      'GPIO_PULL_UP \| GPIO_PULL_DOWN\)\).{0,160}-EINVAL'
check '(flags & (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) == (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)' \
      'GPIO_OUTPUT_INIT_LOW \| GPIO_OUTPUT_INIT_HIGH\)\).{0,200}-EINVAL'
check '(flags & GPIO_OUTPUT) == 0U' \
      'GPIO_OUTPUT\) == 0U\).{0,200}-EINVAL'
check '(flags & GPIO_INT_MASK) != 0U' \
      'GPIO_INT_MASK\) != 0U\).{0,160}-ENOTSUP'
check '(flags & GPIO_INT_WAKEUP) != 0U' \
      'GPIO_INT_WAKEUP\) != 0U\).{0,160}-ENOTSUP'
check '(flags & ~PDG_GPIO_ALLOWED_FLAGS) != 0U' \
      'PDG_GPIO_ALLOWED_FLAGS\) != 0U\).{0,160}-ENOTSUP'
```

**Important:** `GPIO_INT_WAKEUP` is bit 6 and is **not** in `GPIO_INT_MASK`
(`gpio.h:157-162`; `dt-bindings/gpio/gpio.h:83`), so it reaches the driver in
*all* builds, including `CONFIG_ASSERT=y`. `GPIO_INT_MASK` bits 21–26 reach the
driver only in `CONFIG_ASSERT=n` or direct-dispatch builds. These are different
reachability classes and the probe keeps them as separate rows deliberately.

**Additional conjunct — ordering:** the explicit drive checks must precede the
residual check, so the contract is visible in diagnostics:

```bash
RES=$(off /tmp/e.cfg 'PDG_GPIO_ALLOWED_FLAGS')
SE=$(off /tmp/e.cfg 'GPIO_SINGLE_ENDED')
[ "$SE" -lt "$RES" ] || fail "residual check precedes explicit drive checks"
```

**Additional conjunct — no RPC on rejection:** every `return -ENOTSUP` / `-EINVAL`
in the extent must occur at a byte offset **before** the first
`pdg_gpio_bottom_set_config` call:

```bash
RPC=$(off /tmp/e.cfg 'pdg_gpio_bottom_set_config')
grep -abo -E 'return -(ENOTSUP|EINVAL)' /tmp/e.cfg | cut -d: -f1 | while read o; do
  [ "$o" -lt "$RPC" ] || { echo "rejection return at $o is after the RPC at $RPC"; exit 1; }
done
```

**Proves:** each rejection is written, with the right errno, in the right order,
before any RPC.
**Does not prove:** that calling `gpio_pin_configure` with those flags returns
those values. That is **M3-C-02**.

---

### M3-B-10 — `PDG_GPIO_ALLOWED_FLAGS` exact membership

**Class:** B

```bash
A=$(awk '/#define PDG_GPIO_ALLOWED_FLAGS/,/[^\\]$/' "$SRC" | norm)
for f in GPIO_INPUT GPIO_OUTPUT GPIO_PULL_UP GPIO_PULL_DOWN \
         GPIO_OUTPUT_INIT_LOW GPIO_OUTPUT_INIT_HIGH GPIO_ACTIVE_LOW; do
  printf '%s' "$A" | grep -qw "$f" || fail "allow-list missing $f"
done
# exactly seven members
[ "$(printf '%s' "$A" | grep -o 'GPIO_[A-Z_]*' | grep -vc '^GPIO_ALLOWED')" -eq 7 ] || fail
# forbidden members
for f in GPIO_SINGLE_ENDED GPIO_LINE_OPEN_DRAIN GPIO_INT_WAKEUP GPIO_OUTPUT_INIT_LOGICAL; do
  printf '%s' "$A" | grep -qw "$f" && fail "allow-list wrongly contains $f"
done
```

**Proves:** the positive allow-list is exactly the seven flags of spec §7.
A new Zephyr bit is therefore rejected unless a reviewer deliberately adds it.
**Does not prove:** that the residual check is actually applied — that is the
last row of B-09, and behaviourally **M3-C-02**.

---

### M3-B-11 — Bottom-half header contract

**Class:** B

```bash
# exactly four declarations, exact signatures
[ "$(grep -c '^int pdg_gpio_bottom_' "$BOTH")" -eq 4 ] || fail
grep -qF 'int pdg_gpio_bottom_get(void *ctx, uint8_t pin, bool *state);' "$BOTH" || fail
grep -qF 'int pdg_gpio_bottom_put(void *ctx, uint8_t pin, bool state);'   "$BOTH" || fail
grep -qF 'int pdg_gpio_bottom_num_gpios(void *ctx, uint8_t *out_num_gpios);' "$BOTH" || fail
grep -q  'int pdg_gpio_bottom_set_config(void \*ctx, uint8_t pin,'        "$BOTH" || fail
# no open/close: this child is born into MFD ownership
grep -Eq 'pdg_gpio_bottom_(open|close|free)' "$BOTH" && fail "open/close wrapper present"
# includes limited to stdbool/stdint (+ C++ guards)
[ "$(grep -c '^#include' "$BOTH")" -eq 2 ] || fail "unexpected header includes"
grep -q '#include <stdbool.h>' "$BOTH" || fail
grep -q '#include <stdint.h>'  "$BOTH" || fail
# no Zephyr header anywhere in the bottom half
grep -q '#include <zephyr/' "$BOT" && fail "bottom half includes a Zephyr header"
grep -q '#include <zephyr/' "$BOTH" && fail
```

**Proves:** the host/embedded split is respected and the child cannot release
the borrow (spec §5, invariant 5).
**Does not prove:** correct argument forwarding — **M3-C-11**.

---

### M3-B-12 — `-ECOMM` → `-EIO` normaliser is GPIO-local and tied to its premise

**Class:** B
**Intent:** Spec §5 and acceptance criterion 16. The normaliser's validity rests
entirely on the claim that all currently reachable GPIO `-ECOMM` originates from
`CommsFailed`, because `pdg_common_status_to_errno()` collapses both
`CommsFailed` and `OneWireNoPresence` to `-ECOMM` (`common.c:31-32`) and cannot
tell them apart. If either the common mapping or `gpio_error_to_status()`
changes, this assumption must break loudly.

```bash
# the normaliser exists, is static/private, and lives only in the GPIO bottom half
grep -Eq '^static int .*(ecomm|normali[sz]e).*\(' "$BOT" || fail "no private normaliser"
grep -q '\-ECOMM' "$BOT" || fail
grep -q '\-EIO'   "$BOT" || fail
# The shared mapper must retain the exact two-status collapse on which the
# GPIO-local normaliser relies, and M3 must not modify that shared policy.
COMMON=zephyr/drivers/common/common.c
grep -Eq 'case[[:space:]]+CommsFailed:[[:space:]]+return -ECOMM;' "$COMMON" \
  || fail "common mapper no longer maps CommsFailed to -ECOMM"
grep -Eq 'case[[:space:]]+OneWireNoPresence:[[:space:]]+return -ECOMM;' "$COMMON" \
  || fail "common mapper no longer maps OneWireNoPresence to -ECOMM"
git diff --quiet HEAD -- "$COMMON" \
  || fail "common.c was modified; the mapping change must stay GPIO-local"

# the source-contract tie: the normaliser's comment must cite BOTH premises by
# file:line so a change to either forces re-examination in review.
N=$(awk '/ecomm|normali[sz]e/,/^}/' "$BOT")
printf '%s\n' "$N" | grep -q 'gpio_error_to_status' \
  || fail "normaliser does not cite gpio_error_to_status() as its premise"
printf '%s\n' "$N" | grep -qE 'common\.c:3[0-9]' \
  || fail "normaliser does not cite the common.c:31-32 collapse it depends on"
printf '%s\n' "$N" | grep -q 'OneWireNoPresence' \
  || fail "normaliser does not name the status it cannot distinguish"
```

**Proves:** the normaliser is local, and its two premises are written down where
a reviewer changing either will see them.
**Does not prove:** that `gpio_error_to_status()` still exposes only GPIO
statuses. That is a **human review obligation** which this probe only makes
visible. Flagged in §7 as defect D4.

---

### M3-B-13 — Toggle returns `-ENOTSUP` with no lock and no RPC

**Class:** B

```bash
T=$(extent "$SRC" pdg_gpio_port_toggle_bits); printf '%s\n' "$T" > /tmp/e.tog
grep -q 'k_mutex_lock' /tmp/e.tog && fail "toggle takes a lock"
grep -Eq 'pdg_gpio_bottom_(get|put|set_config)' /tmp/e.tog && fail "toggle issues an RPC"
grep -q 'return -ENOTSUP' /tmp/e.tog || fail "toggle does not return -ENOTSUP"
# zero pins still succeeds
norm < /tmp/e.tog | grep -Eq 'pins == 0U?\).{0,60}return 0' || fail "zero-pin toggle does not succeed"
```

**Proves:** spec §6.4 exactly — ABI-safe non-NULL dispatch exists, capability
does not, no hardware is touched. Also means there is no mid-sequence transport
state in M3 (spec §9.2).
**Does not prove:** that `gpio_pin_toggle_dt()` returns `-ENOTSUP` to a caller.

---

### M3-B-14 — Init sequence order

**Class:** B

```bash
printf '%s\n' "$(extent "$SRC" pdg_gpio_init)" > /tmp/e.init
P1=$(off /tmp/e.init 'k_mutex_init')
P2=$(off /tmp/e.init 'device_is_ready\(config->mfd\)')
P3=$(off /tmp/e.init 'pdg_mfd_ctx\(config->mfd\)')
P4=$(off /tmp/e.init 'ctx == NULL')
P5=$(off /tmp/e.init 'pdg_gpio_bottom_num_gpios')
P6=$(off /tmp/e.init 'config->ngpios')
for v in $P1 $P2 $P3 $P4 $P5 $P6; do [ "$v" -ge 0 ] || fail "init step missing"; done
[ "$P1" -lt "$P2" ] && [ "$P2" -lt "$P3" ] && [ "$P3" -lt "$P4" ] \
  && [ "$P4" -lt "$P5" ] && [ "$P5" -lt "$P6" ] || fail "init sequence out of order"
# mismatch is -EINVAL and logs BOTH numbers
norm < /tmp/e.init | grep -Eq 'ngpios.{0,200}-EINVAL' || fail "mismatch is not -EINVAL"
# success logs the configured serial
grep -Eq 'LOG_INF.*serial' /tmp/e.init || fail "configured serial not logged on success"
# no pin is touched during init
grep -Eq 'pdg_gpio_bottom_(get|put|set_config)' /tmp/e.init \
  && fail "init touches a pin; spec §4.1 step 9 forbids it"
```

**Proves:** readiness → accessor → NULL → count → compare → log, mutex first,
no pin actuation at init.
**Does not prove:** that `gallo_num_gpios()` is a warm read with no USB traffic
(spec §4.2 R4). That claim rests on `pico-de-gallo-lib/src/lib.rs:1055-1067`
and is **unverifiable in M3** — see **M3-C-13**.

The "no pin touched at init" conjunct is more valuable than it looks: plan §2 R7
records that the board has an **orphaned GPIO subscription on pin 2**, and a
first `set_config` on a monitored pin returns `GpioPinMonitored` → `-EBUSY`
(spec §15 finding 3). M3 init touching a pin would fail on real hardware.

---

### M3-B-15 — Clear-never-close

**Class:** B

```bash
E=$(extent "$SRC" pdg_gpio_init)
printf '%s\n' "$E" | grep -q 'data->ctx = NULL' || fail "no defensive clear"
grep -Eq '(gallo_free|gallo_close|pdg_common_bottom_close|pdg_registry_close)' "$SRC" \
  && fail "child releases a borrowed handle"
grep -Eq '(gallo_free|gallo_close)' "$BOT" && fail "bottom half releases the handle"
```

**Proves:** invariant 5. Closing here would drop the parent's sole registry
reference and leave the parent and I2C/SPI siblings holding a freed pointer.

---

### M3-B-16 — One shared locked write helper; public callbacks guard and delegate

**Class:** B

Spec §6.3 mandates a *single* private locked helper carrying the write shape,
with each public callback owning its own ISR, context, mask and zero-mask
checks plus the locking, then delegating with the correct operation and value.
An earlier revision of this probe demanded the ascending loop inside
`pdg_gpio_port_set_masked_raw` itself, which forced a duplicated loop and
contradicted §6.3; that demand was wrong and is withdrawn.

```bash
H=$(extent "$SRC" pdg_gpio_write_locked) || fail "no private locked helper"
printf '%s\n' "$H" > /tmp/e.hlp

# --- The helper carries the write shape, and carries it exactly once. ---
norm < /tmp/e.hlp | grep -Eq 'for \(.*pin = 0.*pin < .*ngpios.*pin\+\+' \
  || fail "helper does not use an ascending 0..ngpios-1 loop"
grep -q 'pdg_gpio_bottom_get' /tmp/e.hlp \
  && fail "read-modify-write present in helper; forbidden by §6.3"
norm < /tmp/e.hlp | grep -Eq 'if \(ret < 0\).{0,40}break' \
  || fail "helper does not stop on first failure"
norm < /tmp/e.hlp | grep -q 'acked |= BIT(pin)' \
  || fail "helper does not track the acknowledged prefix"
# The helper must not lock: its callers hold the mutex (no double lock).
grep -q 'k_mutex_lock' /tmp/e.hlp && fail "helper locks; callers already hold the mutex"

# Exactly ONE ascending put loop exists in the whole driver.
[ "$(grep -c 'pdg_gpio_bottom_put' "$SRC")" -eq 2 ] \
  || fail "expected exactly 2 pdg_gpio_bottom_put call sites (helper + output init)"

# --- Each public write callback: guards, validation, zero-mask, lock, delegate. ---
for fn in pdg_gpio_port_set_masked_raw pdg_gpio_port_set_bits_raw \
          pdg_gpio_port_clear_bits_raw; do
  E=$(extent "$SRC" "$fn"); printf '%s\n' "$E" > /tmp/e.cb
  N=$(norm < /tmp/e.cb)
  printf '%s' "$N" | grep -Eq 'k_is_in_isr\(\)\).{0,40}-EWOULDBLOCK' \
    || fail "$fn: missing ISR guard"
  printf '%s' "$N" | grep -Eq 'data->ctx == NULL\).{0,200}-ENODEV' \
    || fail "$fn: missing context NULL guard"
  printf '%s' "$N" | grep -Eq '& ~.*port_pin_mask\).{0,120}-EINVAL' \
    || fail "$fn: missing port_pin_mask validation"
  printf '%s' "$N" | grep -Eq '== 0U\).{0,60}return 0' \
    || fail "$fn: missing zero-mask short-circuit"
  # ISR guard, then NULL guard, then mask check, then lock, then delegate.
  I=$(off /tmp/e.cb 'k_is_in_isr');   C=$(off /tmp/e.cb 'data->ctx == NULL')
  M=$(off /tmp/e.cb 'port_pin_mask'); L=$(off /tmp/e.cb 'k_mutex_lock')
  D=$(off /tmp/e.cb 'pdg_gpio_write_locked')
  [ "$I" -ge 0 ] && [ "$I" -lt "$C" ] && [ "$C" -lt "$M" ] \
    && [ "$M" -lt "$L" ] && [ "$L" -lt "$D" ] \
    || fail "$fn: guard/lock/delegate ordering violated"
  printf '%s' "$N" | grep -q 'k_mutex_unlock' || fail "$fn: does not unlock"
  # No loop, no direct RPC, no recursion into another public callback.
  printf '%s' "$N" | grep -Eq 'for \(' && fail "$fn: carries its own loop; §6.3 forbids duplication"
  grep -q 'pdg_gpio_bottom_put' /tmp/e.cb && fail "$fn: issues RPC directly instead of delegating"
  for other in pdg_gpio_port_set_masked_raw pdg_gpio_port_set_bits_raw \
               pdg_gpio_port_clear_bits_raw; do
    [ "$other" = "$fn" ] && continue
    grep -q "$other" /tmp/e.cb && fail "$fn recursively calls public $other (double lock)"
  done
done

# --- Each callback delegates with the correct operation string and value. ---
norm < <(extent "$SRC" pdg_gpio_port_set_masked_raw) \
  | grep -q 'pdg_gpio_write_locked(port, "masked write", mask, value)' \
  || fail "masked write does not delegate (mask, value)"
norm < <(extent "$SRC" pdg_gpio_port_set_bits_raw) \
  | grep -q 'pdg_gpio_write_locked(port, "set-bits", pins, pins)' \
  || fail "set-bits does not delegate (pins, pins)"
norm < <(extent "$SRC" pdg_gpio_port_clear_bits_raw) \
  | grep -q 'pdg_gpio_write_locked(port, "clear-bits", pins, 0U)' \
  || fail "clear-bits does not delegate (pins, 0U)"
```

**Proves:** the §6.3 single-helper shape — one ascending, stop-on-failure,
prefix-tracking write loop; three public callbacks that each guard, validate,
short-circuit, lock and delegate; and that clear passes zero rather than the
mask.
**Does not prove:** the acknowledged-prefix residue semantics at runtime —
**M3-C-04**.

**Mutation control (must fail):** in `pdg_gpio_port_clear_bits_raw`, change the
delegated value argument from `0U` to `pins`, so clear-bits would *set* the
selected pins. The final delegation check goes red with `clear-bits does not
delegate (pins, 0U)`. A second, independent mutation — deleting the
`if (pins == 0U) { return 0; }` short-circuit from `pdg_gpio_port_set_bits_raw`
— goes red with `missing zero-mask short-circuit`. Both confirmed red on a
scratch copy and reverted.

---

### M3-B-17 — Partial-failure `LOG_ERR` lives in the helper and carries every field

**Class:** B

The diagnostic is now emitted once, in the shared helper, so all three write
operations report identically. It must name the operation, the failed pin, the
requested mask and value, the acknowledged prefix, and the errno.

```bash
H=$(extent "$SRC" pdg_gpio_write_locked); printf '%s\n' "$H" | norm > /tmp/e.f
L=$(grep -o 'LOG_ERR([^;]*' /tmp/e.f)
[ -n "$L" ] || fail "no LOG_ERR on partial failure"
# Exactly one diagnostic, guarded by the failure condition.
[ "$(grep -c 'LOG_ERR(' /tmp/e.f)" -eq 1 ] || fail "expected exactly one LOG_ERR in the helper"
grep -Eq 'if \(ret < 0\) \{ LOG_ERR' /tmp/e.f || fail "LOG_ERR not guarded by ret < 0"
# Field-name based, not string based: a LOG_ERR that merely says "failed" fails.
for tok in pin mask value prefix errno; do
  printf '%s' "$L" | grep -qi "$tok" || fail "LOG_ERR missing field: $tok"
done
# The operation name is a parameter, so all three callers are covered by it.
printf '%s' "$L" | grep -q '%s: %s' || fail "LOG_ERR does not interpolate the operation name"
grep -Eq 'const char \*op' <<<"$H" || fail "helper takes no operation-name parameter"
# No public write callback emits its own competing partial-failure diagnostic.
for fn in pdg_gpio_port_set_masked_raw pdg_gpio_port_set_bits_raw \
          pdg_gpio_port_clear_bits_raw; do
  extent "$SRC" "$fn" | grep -q 'acknowledged prefix' \
    && fail "$fn duplicates the helper's partial-failure diagnostic"
done
# and the hot path must NOT warn
G=$(extent "$SRC" pdg_gpio_port_get_raw)
printf '%s\n' "$G" | grep -q 'LOG_WRN' \
  && fail "port_get_raw warns per skipped output; M4 CS activity would log at transfer rate"
```

**Proves:** acceptance criteria 14 and 15's log requirements, now against the
single emission site.
**Note:** the token check is deliberately field-name-based rather than
string-based. This is the anti-T10 shape.

**Mutation control (must fail):** delete the
`"acknowledged prefix mask 0x%08x, "` fragment and its `(uint32_t)acked`
argument from the helper's `LOG_ERR`. The probe goes red with `LOG_ERR missing
field: prefix`. A second, independent mutation — dropping the `const char *op`
parameter and hard-coding `"masked write"` in the format string — goes red with
`helper takes no operation-name parameter`. Both confirmed red on a scratch copy
and reverted.


---

### M3-B-18 — Init priority and level

**Class:** B

```bash
grep -q 'POST_KERNEL' "$SRC" || fail
grep -q 'CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY' "$SRC" || fail
grep -A3 'GPIO_PICO_DE_GALLO_INIT_PRIORITY' zephyr/drivers/gpio/Kconfig | grep -q 'default 45' || fail
```

Combined with A-01 criterion 5 (`=45` in the generated `.config`), this
establishes parent 40 < GPIO 45 < I2C/SPI 50 (invariant, spec §8).
**Does not prove:** ordering at runtime — priorities are configurable, which is
exactly why the runtime `device_is_ready` guard remains load-bearing.

---

### M3-B-19 — Defensive pin bound in `pin_configure`

**Class:** B

```bash
CFG=$(extent "$SRC" pdg_gpio_pin_configure); printf '%s\n' "$CFG" | norm > /tmp/e.cfg
grep -Eq 'pin >= config->ngpios\).{0,120}-EINVAL' /tmp/e.cfg \
  || fail "no defensive pin>=ngpios check"
B=$(off /tmp/e.cfg 'pin >= config->ngpios'); R=$(off /tmp/e.cfg 'pdg_gpio_bottom_set_config')
[ "$B" -lt "$R" ] || fail "bound check occurs after the RPC"
```

**Proves:** the third of the three pin-bound mechanisms named in A-10 exists in
source.
**Does not prove:** that it is reached. Normal public use hits Zephyr's
`gpio.h:1040` assertion first; this check protects direct dispatch and
`CONFIG_ASSERT=n` builds only, and cannot promise graceful invalid-public-pin
recovery (spec §6.1 says so explicitly).

---

### M3-B-20 — Shield topology exact

**Class:** B

```bash
O=zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay
grep -q 'pdg_gpio0: gpio {'                        "$O" || fail
grep -q 'compatible = "odp,pico-de-gallo-gpio";'   "$O" || fail
grep -q 'gpio-controller;'                         "$O" || fail
grep -q '#gpio-cells = <2>;'                       "$O" || fail
grep -q 'ngpios = <4>;'                            "$O" || fail
# child must be DISABLED in M3
awk '/pdg_gpio0: gpio \{/,/\};/' "$O" | grep -q 'status = "disabled";' || fail
# ordering: gpio before i2c before spi
G=$(off "$O" 'pdg_gpio0:'); I=$(off "$O" 'pdg_i2c0:'); S=$(off "$O" 'pdg_spi0:')
[ "$G" -lt "$I" ] && [ "$I" -lt "$S" ] || fail "gpio->i2c->spi order violated"
# M3 must not touch SPI
git diff HEAD -- zephyr/drivers/spi zephyr/dts/bindings/spi | grep -q . && fail "SPI modified"
grep -q 'cs-gpios' "$O" && fail "cs-gpios introduced; that is M4"
```

---

### M3-B-21 — Binding file contract

**Class:** B

```bash
grep -q 'compatible: "odp,pico-de-gallo-gpio"' "$BIND" || fail
grep -qE 'include: \[gpio-controller\.yaml, base\.yaml\]' "$BIND" || fail
grep -qE 'gpio-cells:' "$BIND" || fail
grep -q 'on-bus:' "$BIND" && fail "on-bus present; this is a controller child, not a bus device"
# the normative description must state every §3.1 limitation, each by a
# distinct structural token, not by a single word
for tok in EWOULDBLOCK EIO ENOTSUP EINVAL ENOSYS serial-number ngpios \
           non-atomic toggle blinky; do
  grep -q "$tok" "$BIND" || fail "binding description omits: $tok"
done
```

The token list is chosen so no single sentence can satisfy several at once —
the T10 failure mode. `blinky` in particular can only appear if the author
actually wrote the named-incompatible-consumer paragraph the spec requires.

---

### M3-B-22 — Common-source guard names GPIO directly

**Class:** B

```bash
grep -q 'CONFIG_GPIO_PICO_DE_GALLO' zephyr/drivers/CMakeLists.txt || fail
grep -q 'add_subdirectory_ifdef(CONFIG_GPIO_PICO_DE_GALLO gpio)' zephyr/drivers/CMakeLists.txt || fail
grep -q 'rsource "gpio/Kconfig"' zephyr/drivers/Kconfig || fail
grep -q 'DT_HAS_ODP_PICO_DE_GALLO_GPIO_ENABLED' zephyr/Kconfig || fail
# gpio must be added between mfd and i2c
M=$(off zephyr/drivers/CMakeLists.txt 'add_subdirectory_ifdef\(CONFIG_MFD')
G=$(off zephyr/drivers/CMakeLists.txt 'add_subdirectory_ifdef\(CONFIG_GPIO')
I=$(off zephyr/drivers/CMakeLists.txt 'add_subdirectory_ifdef\(CONFIG_I2C')
[ "$M" -lt "$G" ] && [ "$G" -lt "$I" ] || fail "gpio not placed between mfd and i2c"
```

A GPIO-only tree would otherwise omit `common.c` while `pdg_gpio_bottom.c`
references `pdg_common_status_to_errno` (spec §14 point 5). Note the guard is a
plain `OR` chain — GPIO implies MFD today, so this guard is *currently*
redundant, which is exactly why a probe is needed: nothing would fail if it were
omitted until someone builds a GPIO-only app.

---

### M3-B-23 — Scope containment

**Class:** B

```bash
git diff --name-only HEAD | grep -E '^crates/' && fail "crates/ touched"
git diff --name-only HEAD | grep -E 'Cargo\.(toml|lock)' && fail "Cargo touched"
git diff --name-only HEAD | grep -E 'zephyr/samples/' && fail "sample overlays touched in M3"
git diff --check | grep -q . && fail "whitespace errors"
# LF everywhere
git diff --name-only HEAD | while read f; do
  [ -f "$f" ] && file "$f" | grep -q CRLF && { echo "CRLF: $f"; exit 1; }
done
```

---

### M3-B-24 — Documentation parity (AGENTS.md §15.1)

**Class:** B

```bash
git diff --name-only HEAD | grep -q 'zephyr/README.md'        || fail
git diff --name-only HEAD | grep -q 'book/src/interfaces/gpio.md' || fail
git diff --name-only HEAD | grep -q 'zephyr/CHANGELOG.md'     || fail
# the false "pins default to input" claim must be gone
grep -qi 'default.*to input' book/src/interfaces/gpio.md \
  && fail "stale 'pins default to input' claim survives; firmware starts in LegacyAuto"
grep -q 'LegacyAuto' book/src/interfaces/gpio.md || fail "corrected claim not written"
```

---

## 5. Mutation controls

A probe nobody has confirmed can fail is not evidence. Precedent: #104's
acceptance re-introduced "first match" and confirmed 3 of 7 tests failed.

Each mutation is applied to a **scratch copy** of the tree or reverted
immediately; **never committed**, and never while another agent holds the build
slot.

| # | Mutation | Must fail | Must NOT fail (specificity check) |
| --- | --- | --- | --- |
| **M1** | In `pdg_gpio_pin_configure`, delete the `(flags & (GPIO_INPUT \| GPIO_OUTPUT)) == (GPIO_INPUT \| GPIO_OUTPUT)` rejection entirely. | **M3-B-02** conjunct (c); **M3-B-09** row 2 | A-01 must still build and pass — proving B-02 is not merely detecting a broken build |
| **M2** | In `pdg_gpio_port_get_raw`, change the `-EACCES` branch from `continue` to `return ret`. | **M3-B-02** conjunct (b) | B-09 must still pass — proving the two halves are independently detected |
| **M3** | Swap `BUILD_ASSERT` 2 (parent status) and `BUILD_ASSERT` 4 (Kconfig) in `PDG_GPIO_PARENT_ASSERTS`. | **M3-B-08** (textual order) and **M3-A-04** condition 4 (diagnostic order) | A-05 must still pass — the serial assertion is unaffected |
| **M4** | Move the `DT_INST_FOREACH_STATUS_OKAY(PDG_GPIO_PARENT_ASSERTS)` line to *below* `#include "pdg_mfd.h"`. | **M3-B-07**, and **M3-A-06** conditions 3–4 (the fatal include prevents the assertion from being emitted first) | A-01, A-04, A-05 unaffected |
| **M5** | Add `bool last_level[32];` to `struct pdg_gpio_data`. | **M3-B-06** (member count = 4) | everything else |
| **M6** | In `pdg_gpio_port_clear_bits_raw`, change the delegated value argument `0U` to `pins`. | **M3-B-16** (delegation arguments) | **M3-B-17** must still pass — the helper is untouched |
| **M7** | Delete the `if (pins == 0U) { return 0; }` short-circuit from `pdg_gpio_port_set_bits_raw`. | **M3-B-16** (zero-mask short-circuit) | B-17 unaffected |
| **M8** | Delete the `"acknowledged prefix mask 0x%08x, "` fragment and its `(uint32_t)acked` argument from the helper’s `LOG_ERR`. | **M3-B-17** (missing field: prefix) | **M3-B-16** must still pass — the loop shape is untouched |
| **M9** | Drop the `const char *op` parameter and hard-code `"masked write"` in the helper format string. | **M3-B-17** (operation-name parameter) | B-16’s loop checks unaffected |

M1 and M2 together are the confirmation that the §6.2↔§7 coupling probe is
genuinely bidirectional. **Run both.** Running only one leaves the possibility
that B-02 is satisfied by a single conjunct.

---

## 6. Summary table

| ID | Class | Proves | Does **not** prove |
| --- | --- | --- | --- |
| A-01 | A | `pdg_gpio.c` compiles and links; four Kconfig symbols y; priority 45 | any behaviour |
| A-02 | A | Root-level GPIO child rejected with the compatible assertion, not an include/link error | that the assertion is reached in other topologies |
| A-03 | A | GPIO child under a ready foreign parent rejected (R9) | that `pdg_mfd_ctx` would actually mis-cast |
| A-04 | A | Disabled-parent status assertion fires, **and precedes** the Kconfig assertion | ordering under a compiler that reorders diagnostics (see B-08) |
| A-05 | A | Missing parent `serial-number` rejected in isolation (R11 mitigation) | selector *uniqueness*; two parents sharing a serial still alias |
| A-06 | A | Assertion block beats the dropped `pdg_mfd.h` include | anything about MFD-on builds |
| A-07 | A | `ngpios = 33` rejected at compile | that the diagnostic is readable to a user (human review) |
| A-08 | A | `ngpios = 0` rejected at compile | as above |
| A-09 | A | Shield default keeps GPIO disabled; driver not compiled | that M4 will enable it correctly |
| A-10 | A | Devicetree does **not** range-check GPIO cells — pin 255 builds fine | that pin 255 is rejected anywhere; only names the three real mechanisms |
| A-11 | A | Four-sample categories preserved, structurally compared | that the two failing samples' failures are harmless |
| A-12 | A | Binding constrains `#gpio-cells` to 2; `const: 2` merges cleanly against `gpio-controller.yaml`; `<1>` fails before compilation | that a well-formed consumer phandle resolves correctly (C-class) |
| B-01 | B | Six slots non-NULL, five interrupt/optional slots absent | that omitted slots yield `-ENOSYS` to a caller |
| **B-02** | **B** | **§6.2 read-masking and §7 input+output rejection both present and mutually anchored** | **either behaviour at runtime (C-01, C-02)** |
| B-03 | B | `k_is_in_isr` → `-EWOULDBLOCK` in all six | ISR detection actually working (C-09) |
| B-04 | B | ISR guard < NULL guard < lock, textually, in all six | reachability of a NULL `ctx` |
| B-05 | B | `k_mutex_init` precedes the first return; no `goto` | absence of an exotic control-flow path |
| B-06 | B | No cache field, no file-scope mutable state | that every query reaches firmware (C-05) |
| B-07 | B | Assertion block above the include | which diagnostic a human reads (A-06) |
| B-08 | B | Assertion order compatible→status→serial→Kconfig in source | emitted diagnostic order (A-04) |
| B-09 | B | All ten §7 rows detected, correct errno, drive-before-residual, no RPC on rejection | that any of them returns that errno (C-02) |
| B-10 | B | Allow-list is exactly the seven permitted flags | that the residual check runs |
| B-11 | B | Four bottom-half signatures, no open/close, no Zephyr header | argument forwarding (C-11) |
| B-12 | B | `-ECOMM`→`-EIO` normaliser is GPIO-local and cites both premises | that `gpio_error_to_status()` still exposes only GPIO statuses (human review; defect D4) |
| B-13 | B | Toggle: no lock, no RPC, `-ENOTSUP`, zero-pins succeeds | caller-visible return (C-08) |
| B-14 | B | Init order; mismatch `-EINVAL`; serial logged; no pin touched | R4 warm-cache claim (C-13); actual count comparison (C-03) |
| B-15 | B | Clear-never-close | that the parent's refcount survives (C-14) |
| B-16 | B | One shared locked helper carries the only ascending, no-pre-read, stop-on-failure, prefix-tracking loop; all three public callbacks guard, validate, short-circuit, lock, delegate with the correct op/value, and never recurse or double-lock | prefix residue semantics (C-04) |
| B-17 | B | Exactly one partial-failure `LOG_ERR`, in the helper, naming op/pin/mask/value/prefix/errno; no duplicate in any callback; no `LOG_WRN` in the hot path | that the log is emitted |
| B-18 | B | Priority 45, POST_KERNEL | runtime init ordering |
| B-19 | B | Defensive `pin >= ngpios` before the RPC | that it is ever reached (A-10 explains why) |
| B-20 | B | Shield child exact, disabled, ordered; no SPI edit | — |
| B-21 | B | Binding compatible/includes/cells/no-on-bus; ten distinct limitation tokens | that the prose is accurate |
| B-22 | B | GPIO in Kconfig discovery, CMake subdirectory, and the common-source guard | that a GPIO-only tree links (untested configuration) |
| B-23 | B | No `crates/`, Cargo, sample, CRLF, or whitespace change | — |
| B-24 | B | README/book/CHANGELOG updated; stale "default to input" claim removed | doc accuracy |

**Counts: Class A = 11, Class B = 24, Class C = 18. Total 53.**

---

## 7. Spec defects and ambiguities found while writing this suite

These are reported **before code is written**, as requested.

**D1 — `ngpios` bounds have no stated enforcement mechanism.**
Spec §3.1 says "Override inherited `ngpios` as required, minimum 1, maximum 32".
Zephyr binding YAML has **no numeric range facility** for `type: int` — only
`required`, `default`, `const` and `enum`. So the bound can only be enforced by
(a) an `enum:` listing 1..32 explicitly, or (b) a `BUILD_ASSERT` in the driver.
The spec picks neither, and states no diagnostic text. **Probes A-07 and A-08
cannot have a deterministic pass criterion until this is resolved.** They are
written with a placeholder substring that the coder must register. Recommended
resolution: `BUILD_ASSERT` in `PDG_GPIO_PARENT_ASSERTS`, message
`odp,pico-de-gallo-gpio ngpios must be between 1 and 32`.

**D2 — Assertion-ordering observability is compiler-dependent.**
Spec §4.1 fixes the order compatible → status → serial → Kconfig, and justifies
it by which diagnostic "a human actually reads". But that outcome depends on
GCC emitting `_Static_assert` failures in source order — an unspecified
behaviour. The spec asserts the human-readability benefit as if it were
guaranteed. It is not. This suite hedges with A-04 (observed order) **and**
B-08 (source order); the spec should say which is normative. As written I have
treated **source order** as the contract and emitted order as corroborating
evidence.

**D3 — "Presence is not uniqueness" is acknowledged but the residual is
untestable.** Spec §4.1 and invariant 11 concede that two parents naming the
same explicit serial still alias. Nothing at compile time can detect this
(devicetree string comparison across instances is expressible via
`DT_INST_FOREACH` but the spec does not require it), and nothing at runtime can
either, because `gallo_init_strict_with_serial_number` has no "already claimed"
report. **This is untestable even in principle at M3** and I have not written a
probe for it. If the conductor wants it closed, it needs a registry-level
change, which is out of M3 scope. Listed as **M3-C-18**.

**D4 — The `-ECOMM`→`-EIO` premise cannot be mechanically tied to its source.**
Spec §5 and acceptance criterion 16 require "a source-contract test [that ties]
this assumption to `gpio_error_to_status()`" so that a change to the common map
or the GPIO status set "must make this assumption fail review/tests rather than
silently broaden it". A grep in `zephyr/` **cannot** observe a change in
`crates/pico-de-gallo-ffi/src/lib.rs`. The strongest available mechanism is
B-12's citation requirement plus a Rust-side test, and M3 forbids touching
`crates/`. **The requirement as written is not satisfiable within M3's
inventory.** I have implemented the citation half and flagged the rest. A real
closure would be a `#[test]` in `pico-de-gallo-ffi` asserting the exact status
set `gpio_error_to_status()` can return — recommend deferring that to a
follow-up, and downgrading criterion 16's claim in the meantime.

**D5 — `port_get_raw` "leave that output bit zero and continue" is
underspecified for a pin that is neither input nor output.** Firmware pin mode
`LegacyAuto` (spec §10, §15) is neither `ExplicitInput` nor `ExplicitOutput`.
Spec §6.2 handles only `-EACCES` (explicit output) and "any other failure
aborts". What does `gallo_gpio_get` return for a `LegacyAuto` pin that has
never been configured? If it succeeds, the read returns a pad level for a pin
Zephyr believes is disconnected. If it errors with something other than
`-EACCES`, then **a single unconfigured pin makes the entire port read fail** —
and since `GPIO_COMMON_CONFIG_FROM_DT_INST` derives a 4-bit mask from `ngpios`,
`gpio_port_get_raw()` will loop over all four pins even if the application
configured only one. That is a plausible, silent, total-failure mode on first
use. **The spec does not address it.** I could not write a deterministic pass
criterion for `port_get_raw` on a partially-configured port; recorded as
**M3-C-06** and flagged here as the ambiguity most likely to bite at M5.

**D6 — `port_pin_mask` vs `ngpios` in the masked-write rejection.** Spec §6.3
rejects `(mask & ~config->common.port_pin_mask) != 0U`, while §6.1 rejects
`pin >= config->ngpios`. These are the same bound expressed two ways and will
agree — *unless* D1 is resolved by allowing `ngpios > 32`, at which point
`port_pin_mask` truncates and the two disagree silently. Resolving D1 closes
this; noting it because the two expressions are in different functions and
nothing couples them (the same rot risk §6.2↔§7 is guarded against).

---

## 8. Deferred to execution — Class C

**Nothing in this section has any assurance in M3.** Each entry names the
milestone that should own it. M5 is the first milestone that runs anything
(plan §9.3).

| ID | Behaviour | Why M3 cannot verify it | Owner |
| --- | --- | --- | --- |
| C-01 | `gpio_port_get_raw()` returns 0 for an output pin and succeeds | Requires execution against firmware that returns `GpioWrongDirection` | **M5** |
| C-02 | Each of the ten §7 flag rows returns exactly `-ENOTSUP` / `-EINVAL` to a caller | Requires calling `gpio_pin_configure()` | **M5** |
| C-02a | `GPIO_INT_MASK` bits reach the driver in a `CONFIG_ASSERT=n` build and are rejected | Needs a second, assertion-free build **and** execution | **M5** (dedicated `CONFIG_ASSERT=n` run) |
| C-02b | `GPIO_INT_WAKEUP` (bit 6) reaches the driver in a `CONFIG_ASSERT=y` build and is rejected | Execution | **M5** |
| C-03 | `ngpios` mismatch against firmware `num_gpios` fails init with `-EINVAL` and logs both values | Requires the firmware round-trip; DT `ngpios=3` builds fine | **M5** |
| C-04 | Masked-write partial failure: acknowledged prefix changed, failed pin indeterminate, later pins unissued | Requires an induced transport failure mid-sequence | **M5**, needs a fault-injection fixture that does not exist |
| C-05 | Every query reaches firmware (no cache) | Requires observing USB traffic; B-06 only proves there is nowhere to cache | **M5**, ideally with a `gallo` CLI-side trace |
| C-06 | `port_get_raw` on a partially-configured port (`LegacyAuto` pins) | **Spec ambiguity D5** — no defined expected result | **Spec fix first**, then M5 |
| C-07 | Pin `ngpios` and pin 255 are rejected — and by *which* of the three mechanisms | A-10 proves DT does not gate it; distinguishing `gpio.h:1040` assert vs driver `-EINVAL` vs firmware `GpioInvalidPin` needs three separate runs (`CONFIG_ASSERT` on and off, plus direct dispatch) | **M5** |
| C-08 | `gpio_pin_toggle_dt()` returns `-ENOTSUP` and touches no hardware | Execution | **M5** |
| C-09 | `-EWOULDBLOCK` from ISR context | Needs an ISR on `native_sim` | **M5** |
| C-10 | Omitted API slots yield `-ENOSYS` (interrupt configure, callback mgmt, pending int, `pin_get_config`, `port_get_direction`) | Execution | **M5** |
| C-11 | Bottom-half argument forwarding and output-pointer behaviour | Needs a host-context harness | **M5** |
| C-12 | `-ECOMM` → `-EIO` normalisation actually occurs | Requires inducing `CommsFailed` — e.g. unplugging mid-call | **M5**, if a fixture exists; otherwise permanently unverified |
| C-13 | R4's claim that `gallo_num_gpios()` is a warm read with no second USB round-trip and no 300 s timeout exposure | Requires timing the child boot path | **M5** |
| C-14 | Parent refcount survives a failed child init (clear-never-close) | Requires inducing a child init failure with a live parent | **M5** |
| C-15 | Init ordering parent 40 → GPIO 45 → SPI 50 actually holds at boot | Execution | **M5** |
| C-16 | Configured serial is logged on successful init, and matches the board actually opened | Execution; and the *matching* half is unverifiable even then — `gallo_init_strict_with_serial_number` reports no chosen serial | **M5** for the log; the match is **D3/C-18** |
| C-17 | Output initialisation exposes the previous/HAL-defined level between the two RPCs; a failed `set_config` may already have been applied | Requires a scope or a witness pin, and is inherently racy | **M5**, with the 13↔14 jumper as witness — see plan R2: pre-drive low, release to pull-down, verify baseline |
| C-18 | Two parents with the *same* explicit serial alias to one board | **Untestable in principle** (spec defect D3) — no compile-time and no runtime detector exists | **Out of scope**; needs a registry change |

### 8.1 Hazards M5 must budget for

- **Board state (plan R7).** An orphaned GPIO subscription on pin 2 and pin 3
  parked as an output high. First `set_config` on pin 2 returns
  `GpioPinMonitored` → `-EBUSY` (spec §15 finding 3). **A power cycle is
  required before any Class C probe.** M3 init touches no pin (B-14), so M3 is
  unaffected — but C-03 through C-17 all are.
- **RP2350 pull-down trap (plan R2).** A pull-down can hold a low node low but
  cannot pull a high node low, and a floating pad drifts high within seconds.
  Any C-probe configuring a pull-down and expecting LOW without first driving
  the node low is invalid and **will pass against broken firmware**.
- **Never call a `gallo_*` MCP tool (plan R1).** Use the branch-built CLI.

---

## 9. Honest assessment of the assurance boundary

Asked as a fraction of M3's contract that is actually provable at compile time,
my answer is:

**Roughly one third — and only about one sixth is provable by a real gate.**

Decomposed:

| Category | Share of M3's contract | Assurance |
| --- | --- | --- |
| Topology, discovery, build glue, binding, scope containment | ~20% | **Class A — genuinely proved.** A-01 through A-11 are real gates with named diagnostics and non-vacuity checks. |
| Structural shape of the driver (guard order, allow-list membership, no cache field, API slots, log fields) | ~35% | **Class B — shape only.** Strong against deletion and rot, useless against a construct that is present but wrong. |
| Runtime behaviour: flag returns, port read masking, partial-failure residue, ISR guard, count mismatch, error mapping, blocking, init ordering | ~45% | **Class C — zero.** |

The most important thing to report upward is that **the two properties M3's spec
argues hardest for — the §6.2↔§7 coupling and the no-caching rule — are both
Class B**. B-02 and B-06 are the strongest structural probes I can write, and
they will catch deletion, renaming and refactoring rot. They will **not** catch
an implementation that keeps both constructs and gets the logic wrong. Anyone
reading "the coupling is tested" should read it as "the coupling is *anchored in
source and cannot be silently deleted*", nothing more.

Second: the flag table is ten rows of pure runtime contract, and M3 proves that
ten expressions are *written*, not that any of them *fires*. That is the largest
single block of unverified surface.

Third: two spec requirements are **not satisfiable within M3's inventory at all**
— acceptance criterion 16's source-contract test for `-ECOMM` (D4, needs a
`crates/` test M3 forbids) and the duplicate-serial residual (D3/C-18, has no
detector anywhere). Both should be downgraded in the spec rather than reported
as met.
