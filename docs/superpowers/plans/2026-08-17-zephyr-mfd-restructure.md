# Zephyr MFD Restructure — Implementation Plan (SP1)

> **For agentic workers:** This is a **milestone-level** plan. A fresh
> `@coordinator` is dispatched per milestone and runs the pipeline in §6;
> step-level design is produced by that milestone's `@architect`.

**Goal:** Restructure the Zephyr module as an MFD with a real GPIO controller, so
SPI chip-select uses the standard `cs-gpios` binding instead of the bespoke
`cs-gpio-indices`.

**Architecture:** One parent node owns the USB handle; `gpio`, `i2c` and `spi`
become devicetree children reached via `DT_INST_PARENT`. `spi_context_cs_control`
drives CS through the GPIO child, so the SPI child has no pin concept at all.

**Spec:** `docs/superpowers/specs/2026-08-17-zephyr-mfd-restructure-design.md`
(commit `a06e8d823df5`). The spec is authoritative; this plan sequences it.

**Related:** #98 (upstreaming tracker), #104 (SPI chip-select contract, landed).

---

## 1. Milestones and ordering

```
M1 (parent) → M2 (nest+migrate) → M3 (gpio) → M4 (cs-gpios) → M5 (verify) → M6 (docs)
```

Essentially serial: M2 needs M1's parent, M3 needs M2's topology, M4 needs M3's
controller, M5 needs M4's semantics. **Parallelism lives inside each milestone**,
where a coordinator may fan out across independent files.

| # | Milestone | Nature | Gate |
| --- | --- | --- | --- |
| M1 | MFD parent driver, binding, Kconfig/CMake; parent node added to the shield overlay | **Additive.** i2c/spi untouched and still siblings | 4 samples build; parent initialises |
| M2 | Nest `i2c` and `spi` under the parent; both take the handle from `DT_INST_PARENT` | **Pure refactor**, no behaviour change | 4 samples build; i2c/spi behaviour identical |
| M3 | `pdg_gpio` controller: 6 callbacks, bottom half, binding | **Additive.** Nothing consumes it yet | Builds; GPIO round-trip on the jumper |
| M4 | SPI → `cs-gpios`; delete `cs-gpio-indices`; `spi/batch` → `spi/transfer` | **The semantic change** | Builds; CS edges witnessed |
| M5 | Integrate `spi_loopback` + board overlay; hardware acceptance | Verification | Loopback suite passes on hardware |
| M6 | Book parity, `zephyr/README.md`, CHANGELOGs | Documentation | `mdbook build` clean; §15.1 checklist |

> **Corrected after M1–M5 — this table is the original sequencing summary, not
> the final acceptance record.** M1 could not make all four samples build and
> its parent was disabled by default; the achieved gate was two clean builds,
> two failures matching the measured baseline, and an enabled-parent probe
> (§8.1). M2 was not a pure refactor: it changed initialization failure
> coupling, failure location, and worst-case boot latency (§9.1). M4 was
> compile-only, so chip-select edges could only be witnessed in M5 (§11.3).
> M5's data-path and chip-select checks passed, but its overall verdict was
> **FAIL** because it found three defects; the upstream loopback result was
> 41 PASS / 12 SKIP / 1 structurally unrunnable FAIL / 2 NOT BUILT, not a clean
> suite pass (§12 and `zephyr/CHANGELOG.md`).

**On M1's temporary redundancy.** In M1 the parent opens a handle that nothing
consumes, while i2c and spi still open their own. This is safe — `gallo_registry.c`
deduplicates by serial and refcounts — but the **mixed-selector guard**
(`gallo_registry.c:160-171`) rejects mixing an omitted and an explicit selector.
The parent node must therefore carry the *same* `serial-number` treatment as the
existing children until M2 removes theirs.

---

## 2. Operational hazards — the risk register

The conductor owns this. Every coordinator must read it.

### R1 — Never invoke a `gallo_*` MCP tool

The board runs the firmware built during #104 M2, which reports a **nine-field**
`DeviceInfo`. The pre-built MCP server in this environment was compiled against
the eight-field shape. Per the #104 plan §2.1, an old host reading new firmware
succeeds *silently* while ignoring a trailing byte — and because postcard-rpc
response keys hash the response schema, it may never match at all, and
`send_resp` has **no timeout**: it waits forever holding the USB interface. On
Windows, WinUSB grants exclusive interface access, so a hung call also locks out
every other tool.

**Do not call them. Use the branch-built CLI:** `cargo run -p gallo --locked -- …`

### R2 — RP2350 pull-downs cannot pull a high node low

Measured on this board during #104 (plan §8.11):

| Starting state | Pull applied | Result |
| --- | --- | --- |
| node LOW | pull-**up** | rises — works |
| node HIGH | pull-**down** | **stays HIGH** |
| driven LOW, released to pull-down | — | holds LOW |
| driven LOW, released to **no pull** | — | drifts HIGH in seconds |

Any test that configures a pull-down and expects LOW **without first forcing the
node low** is invalid and will pass against broken code. Pre-drive the node low
and release to a pull-down, or use a pull-up and invert the expectation.

### R3 — Loopback echo may be mode-dependent

MOSI and MISO are shorted on this board. A short *should* echo each byte exactly,
but whether the sampled bit is the current or previous one depends on CPOL/CPHA.
**Verify empirically with a known pattern before relying on it** — a mismatch
presents as a one-bit shift, not an obvious failure.

### R4 — `ngpios` mismatch asserts rather than errors

`gpio.h:933` asserts the pin lies within `port_pin_mask`, derived from `ngpios`.
If `ngpios` disagrees with the firmware's `num_gpios` the failure is an assert,
not a graceful error. Consider a runtime check in the GPIO driver's init.

### R5 — No sample is a viable end-to-end gate

| Sample | Status |
| --- | --- |
| `spi_bridge` | **Cannot link** — `issi,is31fl3743b` exists in neither Zephyr nor this repo |
| `combined_i2c_spi_bridge` | **Cannot link** — same cause |
| `spi_nor_id` | Links, but **no NOR is attached** — cannot pass at runtime |
| `i2c_bridge` | Needs a TMP117 at 0x48; present state unverified |

Samples verify **compilation and devicetree correctness only**. Behavioural
acceptance rests on M5's `spi_loopback` integration.

> **Corrected after M5 — this table remains a compile-time sample inventory.**
> `i2c_bridge` was not exercised on hardware by SP1, so its runtime state
> remains unverified. The two IS31 samples still do not link, and `spi_nor_id`
> still has no attached NOR. Runtime evidence instead came from the dedicated
> M5 fixtures; it must not be summarized as “all four samples pass.”

### R6 — WSL build environment

- Ubuntu-26.04; repo at `/mnt/d/workspace/pico-de-gallo`; Zephyr at
  `~/zephyrproject/zephyr`, **v4.4.0-6123-g26f811ee9d0**.
- Samples are `native_sim` (`depends on ARCH_POSIX`), host gcc — no Zephyr SDK.
- **`~/zephyrproject/.venv` must be activated** or the build dies in
  `gen_kobject_list.py` (system python lacks `pyelftools`).
- GitHub rate-limits the corrosion tarball; `-DFETCHCONTENT_SOURCE_DIR_CORROSION`
  sidesteps it.
- PowerShell: use **single** quotes around the bash string, or `$(...)` is eaten.
- `[Console]::OutputEncoding` is `ibm437`; it mojibakes UTF-8 when capturing
  commit messages into strings. Write messages to a file and `git commit -F`.

### R7 — Board state

Dirty from #104 acceptance: an **orphaned GPIO subscription on pin 2** and **pin
3 left an output parked high**. Neither has a software reset path. A power cycle
is required before M3's GPIO work. The **jumper between header pins 13 and 14**
(firmware GPIO indices 2 and 3) is fitted and must stay.

> **Corrected after M3 and M5 — the original recovery claim and residue premise
> were stale.** `gallo_system_reset_subscriptions()` exists and is the software
> cleanup path for orphaned subscriptions (§10.4 and §11.3). M5 measured
> `M5_RESET_COUNT=0`, so the expected orphaned pin-2 subscription was not present
> (§12.5). Separately, a different 1015-byte SPI dispatcher wedge was observed
> to recover after USB re-enumeration (§12.2). That observation does not prove
> the mechanism or generalize to the GPIO-wait trigger. Keep resets explicit in
> acceptance setup because their necessity depends on actual board state.

---

## 3. File inventory

> **Corrected after M1–M5 — this inventory was an indicative planning baseline,
> not an exhaustive allow-list.** Every implementation milestone discovered
> justified parity, build, test, or specification files omitted here; see
> §§8.1, 9.3, 10.4, 11.3, and 12.5. The milestone specifications and shipped
> diffs are authoritative for the files actually changed.

Anything not listed is out of scope; adding a file requires the milestone
`@architect` to justify it to `@reviewer`.

| Milestone | Action | Path |
| --- | --- | --- |
| M1 | Create | `zephyr/drivers/mfd/{pdg_mfd.c,pdg_mfd.h,CMakeLists.txt,Kconfig}` |
| M1 | Create | `zephyr/dts/bindings/mfd/odp,pico-de-gallo.yaml` |
| M1 | Modify | `zephyr/drivers/{CMakeLists.txt,Kconfig}` |
| M1 | Modify | `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay` |
| M2 | Modify | `zephyr/drivers/i2c/pdg_i2c.c`, `zephyr/drivers/spi/pdg_spi.c` |
| M2 | Modify | `zephyr/dts/bindings/{i2c,spi}/odp,pico-de-gallo-*.yaml` |
| M2 | Modify | shield overlay (nest), 4 × `zephyr/samples/*/app.overlay` |
| M3 | Create | `zephyr/drivers/gpio/{pdg_gpio.c,pdg_gpio_bottom.c,pdg_gpio_bottom.h,CMakeLists.txt,Kconfig}` |
| M3 | Create | `zephyr/dts/bindings/gpio/odp,pico-de-gallo-gpio.yaml` |
| M3 | Modify | `zephyr/drivers/{CMakeLists.txt,Kconfig}`, shield overlay |
| M4 | Modify | `zephyr/drivers/spi/pdg_spi.c`, `pdg_spi_bottom.{c,h}` |
| M4 | Modify | `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml` (**delete `cs-gpio-indices`**) |
| M4 | Modify | 3 × SPI sample overlays |
| M5 | Create | `zephyr/tests/` loopback board overlay + any glue |
| M6 | Modify | `zephyr/README.md`, `book/src/interfaces/{spi,gpio}.md`, CHANGELOGs |

---

## 4. Verification

> **Corrected after M1 — the original command in this section was defective and
> every gate run with it was vacuous.** `zephyr/Kconfig:6` has `depends on
> 64BIT`, and plain `native_sim` is the **32-bit** variant, so
> `CONFIG_PICO_DE_GALLO=n`. `zephyr/CMakeLists.txt:4` then wraps all
> corrosion/FFI setup in `if(CONFIG_PICO_DE_GALLO)` and `zephyr/Kconfig:11-19`
> sources the drivers only inside `if PICO_DE_GALLO`. The result builds
> cleanly **with the entire module absent**. Verified by the conductor.

Per milestone, minimum:

```bash
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo \
  && source ~/zephyrproject/.venv/bin/activate \
  && source ~/zephyrproject/zephyr/zephyr-env.sh \
  && export ZEPHYR_TOOLCHAIN_VARIANT=host \
  && west build -p always -d /tmp/<DIR> -b native_sim/native/64 zephyr/samples/<NAME>'
```

Each element is load-bearing:

| Element | Why |
| --- | --- |
| `native_sim/native/64` | 32-bit `native_sim` silently disables the whole module |
| `zephyr-env.sh` | the repo root is not a west workspace |
| `ZEPHYR_TOOLCHAIN_VARIANT=host` | no Zephyr SDK is installed; these are `ARCH_POSIX` builds |
| `.venv` | system python lacks `pyelftools`; the build dies in `gen_kobject_list.py` |
| `-d /tmp/<DIR>` | the default lands a **non-gitignored** `build/` at the repo root (`zephyr/.gitignore` covers only `zephyr/build/`) |

**Prove the build is not vacuous.** Confirm `CONFIG_PICO_DE_GALLO=y` in
`<build>/zephyr/.config` and that the driver translation units you changed appear
in `<build>/compile_commands.json`:

```bash
grep -o 'pdg_[a-z0-9_]*\.c' <build>/compile_commands.json | sort -u
```

A driver whose devicetree node is `status = "disabled"` will **not** appear. To
exercise it, enable the node with an extra overlay:

```bash
west build ... -- -DEXTRA_DTC_OVERLAY_FILE=/tmp/enable.overlay
```

All four samples must build. Plus, unchanged from #104:

> **Corrected after M1 — “all four” means reproduce the measured baseline, not
> four successful links.** Build the two viable samples successfully and verify
> the two IS31 samples fail identically to baseline; use an enabling overlay to
> prove any otherwise-disabled translation unit is compiled (§8.1). Do not turn
> a pre-existing missing upstream device driver into an SP1 regression.

```bash
cargo test --workspace --locked      # host crates untouched, must stay green
mdbook build book
```

**Conductor re-verifies independently** — never on a coordinator's self-report.

---

## 5. Commit protocol

- Branch `zephyr`. **Never push.** Commits through `c40f597e1e67` are published
  on `origin/zephyr` and are immutable.
- Conventional Commits, scope `zephyr` (or `repo` for docs/AGENTS.md).
- One logical change per commit; each commit builds on its own.
- **Only the `@integrator` commits**, one at a time — the serialization point.
- Trailers, exactly, no `Signed-off-by`:
  ```
  Assisted-by: OpenCode:claude-opus-5
  Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
  ```
- `dos2unix` every touched file before staging (AGENTS.md §3). CRLF in a `.yaml`
  binding breaks subtly.
- **No `[package].version` edits, no `Cargo.lock` regeneration.** No wire or
  firmware change is in scope at all.

### Tree protection

A subagent editing one file **must not** run tree-wide `git checkout`/`restore`/
`reset`/`clean`, or a repo-wide format. Those silently destroy other agents'
uncommitted work. Agents format only their own files; any repo-wide formatting
happens at the serialized commit point.

### Resource protection

At most **one** compile/build invocation across all agents at a time — Zephyr
builds are large and share build directories. Bound every "run until done" loop
with a fixed cap so a failure is fast and clean rather than unbounded.

---

## 6. Per-milestone agent pipeline

One **fresh** `@coordinator` per milestone; sessions are ephemeral and never
reused. Each runs:

1. `@architect` — spec and plan for the milestone. No implementation.
2. `@reviewer` **and** `@reliability` in parallel — architecture review. Findings
   fold into the spec **before any code**.
3. `@tester` — adversarial tests written **without reading the implementation**,
   black-box against the spec. Suite is RED after this.
4. `@coder` — implement until GREEN. A coder who finds a test unsatisfiable
   **escalates**; it never silently weakens the test.
5. `@integrator` — commits.
6. `@reviewer` — spec/plan compliance review.

Every coordinator is handed: this plan, the spec, `AGENTS.md`, and §2's risk
register.

---

## 7. Definition of done (branch-level, after M6)

- [ ] M1–M6 committed on `zephyr`, nothing pushed
- [ ] All four samples build for `native_sim`
- [ ] `spi_loopback` passes on hardware, including the CS witness
- [ ] `cs-gpio-indices` appears nowhere in the repo
- [ ] `cargo test --workspace --locked` still green (host crates untouched)
- [ ] `mdbook build book` clean
- [ ] No `[package].version`, `Cargo.lock`, wire-protocol or firmware change

### Final evidence (after M4–M6)

- [ ] M1–M6 committed on `zephyr`, nothing pushed *(M6 awaits integration)*
- [ ] The original “all four samples build” criterion needs a maintainer ruling:
  the measured outcome is two clean builds plus two failures identical to the
  pre-existing missing-driver baseline
- [ ] The original clean hardware-acceptance criterion needs a maintainer
  ruling: M5 observed the required data path and chip-select behaviour, but its
  own overall verdict was **FAIL** after finding three defects
- [x] Corrected property-removal gate: `cs-gpio-indices` has zero hits under
  `zephyr/`, `book/`, and `crates/`; historical planning records remain
- [x] `cargo test --workspace --locked` was reported green before M6
  (561 unit tests and 7 doctests); documentation-only M6 did not rerun it
- [x] `mdbook build book` clean *(verified during M6)*
- [x] No `[package].version`, `Cargo.lock`, wire-protocol, firmware, or
  `crates/` change in SP1 (`git diff --stat e7087bdc4eee~1..HEAD -- crates/`
  is empty)

> **Corrected after M4–M5 — repository-wide absence was never a valid gate.**
> Historical plans, specifications, and AGENTS.md legitimately name the deleted
> property. The scoped zero-hit gate above is the one M4 verified (§11.3).
> Likewise, “the loopback suite passes” is not the outcome: see §12 and the
> Zephyr CHANGELOG for the mixed upstream result and crash-class finding.

The original checklist above is retained as the branch's definition at planning
time. The appended checklist records final evidence without rewriting history.

### R8 — Two selector-less parents silently alias to one board

**Found by M1's `@reliability` pass; missed by both the design and this plan.**

`gallo_registry.c:152-158` early-returns on a lookup hit **before** the
mixed-selector guard at `:164-171` is ever consulted. Two parent nodes that both
*omit* `serial-number` therefore both normalise to `""`, and the second silently
receives the **first board's handle**. Two devicetree devices, one physical
board, no diagnostic.

That is the same silent-wrong-target class as AGENTS.md §13.17 (2026-07-29,
`gallo-mcp`), and it becomes actuation-unsafe the moment M3's GPIO child exists —
a `gpio_pin_set_dt` on what the tree calls board B would drive board A.

M1 mitigated it with a **compile-time** `BUILD_ASSERT` in `pdg_mfd.c`, chosen
over a runtime check because a build-time failure is provable by exactly the gate
these milestones have. Conductor-verified: two enabled selector-less parents fail
with *"Multiple enabled odp,pico-de-gallo parents require serial-number on every
parent"*.

**Residual, deliberately not closed:** the assert checks *presence*, not
*uniqueness*. Two parents sharing one explicit serial still alias. Documented in
the binding and the source.

---

## 8. M1 outcome and corrections

M1 landed as `a17b997749aa` (spec) + `e7087bdc4eee` (driver, 12 files, +288/−11).

**Conductor-verified independently**, not on self-report:

- `zephyr/Kconfig:6` does gate on `64BIT`; `zephyr/CMakeLists.txt:4` wraps
  everything in `if(CONFIG_PICO_DE_GALLO)` — §4's original command was vacuous.
- With the corrected command, `spi_nor_id` builds 117/117 clean, cbindgen runs,
  and `CONFIG_PICO_DE_GALLO=y`.
- With the parent enabled, **`pdg_mfd.c` and `pdg_spi.c` both compile**;
  `CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY=40` as claimed.
- The `BUILD_ASSERT` fires on two selector-less parents (R8).
- **#104's M4 work is genuinely compiled** — `pdg_spi.c` is in
  `compile_commands.json` and `cs_index` appears 7× in the source. Its
  verification was not vacuous.

### 8.1 Corrections this plan and the spec required

1. **§4's verification command** — fixed above. This was the most consequential
   finding; it would have made every remaining gate meaningless.
2. **§1's M1 gate ("4 samples build; parent initialises") was unachievable.** Two
   samples cannot link (R5, pre-existing), and the parent ships `disabled` so no
   sample compiles it. The real gate is: two clean, two failing *identically to a
   measured baseline*, plus an enabled-parent probe.
3. **§3's inventory was short four files**: `zephyr/Kconfig`,
   `zephyr/drivers/common/common_bottom.h`, `zephyr/drivers/common/common.h`,
   `zephyr/CHANGELOG.md`.
4. **Spec §4.3 said the parent calls `pdg_registry_open` directly.** It must call
   `pdg_common_bottom_open` — calling the registry directly would type-couple the
   embedded translation unit to the host-only FFI.
5. **Spec §4.6's "Kconfig priority defaults" was insufficient.** Upstream's
   `MFD_INIT_PRIORITY = 80` is *backwards* for this topology: measured,
   `CONFIG_I2C_INIT_PRIORITY` and `CONFIG_SPI_INIT_PRIORITY` are both **50**, so
   the parent must be lower. M1 chose **40**, which also sits after libc at 35 —
   necessary because the registry mallocs and uses pthreads.

### 8.2 The `pdg_mfd.h` contract M2 and M3 must honour

```c
void *pdg_mfd_ctx(const struct device *dev);
```

Borrowed opaque handle; NULL on a NULL device or a failed open. The documented
sequence is mandatory: `device_is_ready(parent)` first → child logs and returns
`-ENODEV` if false → only then call the accessor. A NULL **after** a passing
readiness check is an invariant failure, not an expected case. Children must
never close or free it.

`void *` matches the `void *ctx` idiom the children already use. Note the
architect's original claim that a typed pointer was *technically impossible* was
**refuted** in review — an incomplete `struct PicoDeGallo;` needs no FFI header.
The honest rationale is consistency, not necessity.

M3 may add a separate accessor for `num_gpios` (R4) without disturbing this
signature or M2's callers.

### 8.3 Carried into M2

- **Delete the transitional three-node selector warning from the parent binding
  in the same change** that removes the child `serial-number` properties.
- M1's harness test **T10 is weak** — it greps for the string "mix", which is how
  an under-documented binding initially passed. Strengthen it to assert the three
  node names.
- **Unverified by M1, and still unverified:** USB open, schema validation,
  failure logging, refcount transitions, multi-board selection, timeout
  behaviour, interface release. M1's assurance is compile-time only; nothing has
  executed. Init can block up to **five minutes** on strict validation, and
  registry/FFI diagnostics go to host `stderr` and may never reach the Zephyr log.
### R9 — A child under a ready non-PDG parent reinterprets foreign driver data

**Found by M2's `@reliability` pass. Closed by M2.**

The M2 draft claimed a child placed outside a PDG parent has no device object and
fails at link. That is false when the child sits under an enabled, *ready*, but
**unrelated** device: `device_is_ready()` passes, and `pdg_mfd_ctx()` then casts
that foreign driver's `dev->data` to `struct pdg_mfd_data`, yielding an arbitrary
non-NULL pointer the FFI's NULL checks cannot catch.

Closed with structural `BUILD_ASSERT`s on both children, in the fixed order
**compatible → status → Kconfig**. The order matters: disabling the parent also
disables the Kconfig, so without a fixed order the status assertion might never
be independently reachable.

The assertion block must sit **above** the `pdg_mfd.h` include —
`add_subdirectory_ifdef` removes that header from the include path when the
Kconfig is off, and a fatal include error would otherwise mask the readable
assertion.

### R10 — Failed I2C devices had no direct-call context guard

**Found in M2 review. Closed by M2.**

`pdg_spi_transceive` carried a load-bearing NULL guard; **I2C had none**, and
`z_impl_i2c_transfer()` dispatches without checking readiness, so `get_config`
could return zero-initialised state as a false success. Pre-existed M2; the
ownership migration is what made the claimed invalidation guarantee false.

**M3 must copy the child pattern exactly** or it inherits R9 with actuation
consequences: mutex init before every early return; the three assertions in
order; assertion block above the include; readiness → accessor → invariant-NULL;
callback NULL guards before locks; clear-never-close on failure.

### R11 — One selector-less parent is still ambiguous with multiple boards attached

**Open. Not closable in M2.**

M1's `BUILD_ASSERT` constrains *devicetree parents*, not *attached USB devices*.
A single selector-less parent with two boards plugged in resolves through
`gallo_init_strict()`, which **cannot report which serial it chose**. Silent
wrong-target — the same class as AGENTS.md §13.17's 2026-07-29 `gallo-mcp`
incident, and **actuation-unsafe once M3's GPIO child exists**.

Recorded in the parent binding and the M2 spec. Revisit in M3.

### R12 — `/tmp` exhaustion from accumulated build directories

WSL `/tmp` is a **16 GB tmpfs**. Zephyr build directories are ~200 MB each and
milestones had left ~35 behind, reaching 91% and killing one build with
`No space left on device`.

**Conductor owns this**: sweep stale `/tmp` build directories between
milestones. Agents delete only their own (tree/resource protection, §5). Reclaimed
to 53% after M2.

---

## 9. M2 outcome and corrections

M2 landed as `6f006658bcab` (spec) + `592f725a7681` (implementation, 14 files).

**Conductor-verified independently:**

- `i2c_bridge` and `spi_nor_id` both build clean, `CONFIG_PICO_DE_GALLO=y` **and**
  `CONFIG_MFD_PICO_DE_GALLO=y`.
- **`pdg_mfd.c` now compiles into both samples** — it was absent from all four at
  baseline. That is positive evidence the migration took effect, not merely that
  nothing broke.
- M1's `BUILD_ASSERT` still fires after nesting.
- Nothing under `crates/`; tree clean; nothing pushed.

### 9.1 Two errors of mine that M2 caught

1. **Plan §4's TU grep was broken.** `pdg_[a-z_]*\.c` excludes digits, so it can
   **never** match `pdg_i2c.c` and reported zero PDG translation units for
   `i2c_bridge` — a false negative on the very check meant to prove non-vacuity.
   Corrected throughout to `pdg_[a-z0-9_]*\.c`.
2. **§1's M2 row calling this a "pure refactor" is wrong.** Physical USB opens
   were *already* one — the registry's extra hits only incremented `rc` — so only
   the call count changes, 3 → 1. But **failure coupling** changes (a parent
   failure now hard-fails both children), failure **location** moves earlier
   (priority 40, not 50), and worst-case boot latency changes (previously up to
   three independent ~5-minute strict opens, now one). Judged a net reliability
   improvement — coherent fail-closed identity — at a small availability cost.
   The commit correctly carries a `BREAKING CHANGE:` footer.

### 9.2 Ordinal comparison must be structural, never literal

The two baseline ordinals **already differed from each other at the same commit**
(44 vs 45), before nesting renumbered them to 48/49. The two failures also differ
in enclosing function (`.text.main` vs `.text.spi_worker`).

Compare on **symbol + resolved node path + count**, resolved from each build's own
`devicetree_generated.h`. A literal string comparison produces a false regression.
The resolved path moving from `/pdg-spi/is31fl3743b@0` to
`/pico-de-gallo/spi/is31fl3743b@0` is itself confirmation the nesting took.

### 9.3 Other corrections

- **§3's M2 inventory was short five files**: the parent binding,
  `zephyr/README.md`, `book/src/interfaces/spi.md`, `zephyr/CHANGELOG.md`, and the
  M2 spec.
- **§8.3's request to strengthen T10 is resolved as unactionable** — M1's harness
  was ephemeral; `zephyr/tests` does not exist and `git ls-files` matches nothing.
  No framework was invented, correctly, since that is outside the inventory.
- A **stale child `serial-number` fails loudly**, not silently — `edtlib.py`'s
  `_check_undeclared_props()` errors naming both node and binding. Verified by
  probe. Reassuring: a silently-ignored selector would have meant stale
  multi-board overlays quietly targeting the wrong board.
- **Assurance boundary unchanged**: everything on this branch is compile-time
  only. **Nothing has ever executed.** Runtime open, schema validation, refcount
  transitions, multi-board selection, the five-minute strict-open timeout and USB
  interface release all remain unverified — and M2 makes them *more* load-bearing,
  since one parent now gates both children. M5 is the first milestone that runs
  anything.
---

## 10. M3 outcome, and a contradiction in the parent design

M3 landed as `56f116828758` (spec) + `c9b4c9d50556` (driver, 13 files).

**Conductor-verified independently:** `pdg_gpio.c` compiles and links under an
enabling overlay; `CONFIG_GPIO_PICO_DE_GALLO=y` with
`INIT_PRIORITY=45` (correctly above the parent's 40); `pdg_gpio_bottom.o` is
linked via the native-simulator path; no SPI file touched; nothing under
`crates/`; tree clean; nothing pushed.

M3 also found that **`pdg_gpio_bottom.c` can never appear in
`compile_commands.json`** — it is built by the native-simulator Makefile via
`target_sources(native_simulator INTERFACE …)`, as `zephyr/drivers/CMakeLists.txt`
already documents for `common.c`. Verify it by object file and `nm` instead.
Without that, a correct build looks like a failure.

### 10.1 The M5 acceptance vehicle — my design was wrong, and so was M3's fix

Spec §7.2 nominates `spi_loopback`'s `cs-loopback-gpios` as the chip-select
witness. M3 reported this fails outright because
`spi_loopback/src/spi.c:233-240` calls `gpio_pin_interrupt_configure_dt()` and
`gpio_add_callback()`, which D5 leaves as `-ENOSYS`.

**Both halves of that need correcting.**

1. **M3's conclusion is too strong.** Those calls sit inside
   `#if DT_NODE_HAS_PROP(DT_PATH(zephyr_user), cs_loopback_gpios)` at
   `src/spi.c:140`. The witness is **compile-time optional**. Omit the property
   and the suite needs no interrupts at all — the SPI **data path** is fully
   verifiable today.
2. **M3's proposed substitute does not work either.**
   `gpio_basic_api/CMakeLists.txt` globs `src/test*.c` unconditionally, pulling in
   `test_callback_manage.c`, `test_callback_trigger.c` and
   `test_config_trigger.c`. It needs interrupts just as much.

So the honest position: **the data path is verifiable, the CS *edges* are not** —
by any interrupt-free route M3 or I proposed. And a loopback cannot substitute,
because it passes regardless of what CS does.

### 10.2 The fix: `SPI_HOLD_ON_CS`

`spi_context.h:396-401` honours `SPI_HOLD_ON_CS` by *skipping* the deassert:

```c
if (!force_off && ctx->config->operation & SPI_HOLD_ON_CS) {
        return;
}
```

That yields a fully deterministic, polled, interrupt-free CS check:

1. `spi_transceive` with `SPI_HOLD_ON_CS` → CS remains **asserted** afterwards.
2. Poll the witness pin → must read asserted.
3. `spi_release()` → forces the deassert.
4. Poll the witness → must read deasserted.

This tests the exact mechanism, needs no second thread, and has no race.

`pdg_spi.c:241-244` currently rejects `SPI_HOLD_ON_CS` with `-ENOTSUP` — but only
because the *batch* design could not hold CS across separate calls. Once CS is an
ordinary GPIO that constraint disappears. **M4 should therefore support
`SPI_HOLD_ON_CS`**, which is both a capability gain and the enabler for M5's
acceptance.

### 10.3 M3's other findings, carried forward

- **R4 resolved** by exact-equality check of `ngpios` against the firmware's
  `num_gpios` at init, `-EINVAL` on mismatch. Clamping was rejected both ways:
  up exposes pins the firmware refuses, down silently hides valid GPIOs. Crucially
  the feared 300-second `device/info` round-trip **does not occur** — the parent's
  strict open already populates the shared `OnceLock`, so `gallo_num_gpios()`
  reads a warm cache. Verified by two agents independently.
- **R11 is worsened, not narrowed.** A GPIO child actuates physical pins, so a
  selector-less parent with two boards attached now drives *the wrong board's
  hardware*. No FFI accessor reports the serial an existing handle selected, so it
  cannot be made observable from within M3. Mitigated by a fifth `BUILD_ASSERT`
  requiring `serial-number` on the parent of any enabled GPIO child. Residual
  unchanged: presence, not uniqueness.
- **`port_get_raw` returns bit=0 for an output pin**, matching Zephyr's own
  reference controller `gpio_emul.c:525`. This is only safe because §4.5 rejects
  `GPIO_INPUT | GPIO_OUTPUT`, making the confident-lie state unreachable. **The two
  rules are coupled** — relaxing the flag rejection alone would start returning
  false levels, and a false logical `1` under `GPIO_ACTIVE_LOW`.
- **Flag rejection is a positive allow-list plus residual-bit rejection**, so
  silence is impossible by construction rather than by enumeration. Two corrections
  landed: `z_impl_gpio_pin_configure` **asserts** on interrupt bits but does not
  **strip** them, so they reach the driver in a `CONFIG_ASSERT=n` build; and
  `GPIO_INT_WAKEUP` is bit 6, **outside** `GPIO_INT_MASK`, so it always does.

### 10.4 Corrections to the parent design and this plan

- **Spec §4.4 is wrong twice.** Get-then-set `port_toggle_bits` is infeasible on
  this hardware, and the `gpio.h:933` citation is stale — that is the
  *interrupt-wrapper* assertion; the `pin_configure` one is `gpio.h:1040`. **R4
  repeats the stale citation.**
- **Plan R7 is stale.** `gallo_system_reset_subscriptions` **does** exist
  (`ffi/lib.rs:706-748`), documented idempotent and intended for reconnect
  cleanup, so the orphaned pin-2 subscription is not power-cycle-only. Forward
  hazard: **M4's CS init on a monitored pin returns `-EBUSY`** and makes the SPI
  device not ready. Decide before M5 whether to power-cycle or call it once after
  strict open — do **not** bolt a global reset into an ordinary pin callback.
- **`gpio/get` on a `LegacyAuto` pin is not state-neutral** — it calls
  `set_as_input()`, so a whole-port read silently flips every unconfigured pin to
  hardware input. M4's CS is safe (it will be `ExplicitOutput`, hence skipped),
  but reads are not queries.
- **M4 blocker:** `spi_context_cs_control()` returns `void` and **discards** both
  `gpio_pin_set_dt()` results (`spi_context.h:390-418`). A CS assert failing with
  `-EIO`/`-EBUSY`/`-EWOULDBLOCK` is silently ignored, so SPI may transfer with CS
  unasserted or report success with CS still asserted. **M4 must make CS failures
  observable or fail closed** — calling upstream blindly is insufficient for a
  USB-backed controller.
- **§3's M3 inventory was short five files** — third milestone running. §3 should
  be treated as indicative, not exhaustive.
- **Documentation sequencing:** AGENTS.md §15.1 requires same-change parity, so
  each milestone lands its own docs and **M6 is consolidation only**, not the
  first time docs are written.

### 10.5 Assurance boundary — do not overstate M3

M3's own tester graded the probe suite: **~20% genuinely proved, ~35% source-shape
only** (catches deletion, blind to present-but-wrong), **~45% zero**. The two
properties the spec argues hardest for — the `port_get_raw`↔flag-rejection
coupling, and no-caching — are **both** in the source-shape class.

**Nothing on this branch has ever executed.** M5 remains the first milestone that
runs anything.
---

## 11. M4 outcome — the sub-project's goal, achieved

M4 landed as `919b55ad7a5b` (spec) + `0affe9206553` (implementation, 13 files,
`feat(zephyr)!` with a `BREAKING CHANGE:` footer).

**Conductor-verified independently:** `cs-gpio-indices` appears in **zero** files
under `zephyr/`, `book/` or `crates/`; all three SPI overlays carry
`cs-gpios = <&pdg_gpio0 0 GPIO_ACTIVE_LOW>`; `gpio_pin_set_dt` results are
captured at both edges (`pdg_spi.c:220,236`); `SPI_HOLD_ON_CS` is gated on
`SPI_LOCK_ON` (`:413`); `i2c_bridge` and `spi_nor_id` build clean; **`pdg_gpio.c`
now compiles into the SPI sample** where it was absent at baseline; init
priorities resolve **40 < 45 < 50**.

### 11.1 Two protections beyond the brief, both justified

**A failed deassert latches the controller.** Per-transfer honesty was
insufficient: a failed deassert on slave A would let the *next* transfer to slave
B succeed and return **0** with A possibly still selected. An unacknowledged
force-deassert now latches; later transfers get `-EHOSTDOWN` after taking the
lock but **before any hardware I/O**, cleared only by a successful checked
deassert through `spi_release()`.

**`SPI_HOLD_ON_CS` requires `SPI_LOCK_ON`.** HOLD alone returns success and then
*releases the controller*, letting another thread select a second peripheral
while the first is still asserted — simultaneous selection and MISO contention.
On a bus with MOSI and MISO shorted (§7.1) that is not hypothetical.

**If a caller never releases:** the line stays asserted, `ctx->config`/`ctx->owner`
stay set, and any transceive with a different config **blocks forever**. No
timeout, no watchdog, not detectable across process death. Documented, not fixed.

### 11.2 Ruling — sample board identity

Enabling `pdg_gpio0` trips M3's `BUILD_ASSERT` requiring `serial-number` on the
parent, so all three SPI samples became **board-specific**. They build anywhere
but fail strict-open on any board but the named one.

**Decision: keep the placeholder** (`REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL`),
documented so a user substitutes their own from `gallo list`. No private serial
ships in public sample code, and the failure is loud and self-explaining.
**M5 supplies its own fixture overlay carrying the real serial, kept out of the
samples.**

### 11.3 Corrections

- **§1's M4 gate "CS edges witnessed" was impossible** under a compile-only M4 —
  it belongs to M5.
- **§7's "`cs-gpio-indices` appears nowhere in the repo" is untenable** against
  mandated history (AGENTS.md §13.17, the #104 records, and the M2/M3/M4 specs
  all legitimately cite it). **Corrected gate: zero hits under `zephyr/`,
  `book/` and `crates/`.** Verified.
- **R7 is stale** — `gallo_system_reset_subscriptions` exists at
  `ffi/lib.rs:706-748`.
- **§3's inventory was short again** — fifth milestone running. Treat as
  indicative. The SPI `CMakeLists.txt` gained `${ZEPHYR_BASE}/drivers/spi` on the
  include path because `spi_context.h` is a private in-tree header.
- **Spec §4.2 orders assert before set-config**; M4 correctly reverses it to
  shorten the selected-idle window.
- **Spec §5 says "3+" RPCs — it is four**, and misdescribes the delay: Zephyr
  *collapses* setup and hold into a single `DIV_ROUND_UP(MAX(...), 1000)` µs
  value applied at both edges.
- **Spec §6's "GPIO child is the sole writer" is too broad.** It is the sole
  *driver path*, not an ownership reservation — a direct GPIO consumer can still
  move the CS pin between SPI operations, and Zephyr arbitrates nothing against
  non-DT consumers. Exclusive CS-pin ownership is a documented **user obligation**.

### 11.4 Mandatory for M5

- **Pair `SPI_HOLD_ON_CS | SPI_LOCK_ON`** — HOLD alone is rejected.
- **Call `gallo_system_reset_subscriptions()` explicitly in acceptance *setup***,
  after strict open. **Not** in a pin callback, SPI init, or ordinary parent init
  — a hidden global mutation would destroy deliberately-retained subscriptions.
  Required because pin 2 still carries the orphaned subscription and
  `gpio_pin_configure_dt()` returns `-EBUSY` on it.
- **Use a separate fixture overlay** with the real serial (§11.2), not the
  samples.
- **Four properties need new instrumentation to be testable at all:**
  first-errno preservation; "no second GPIO edge" (electrically identical to the
  first, API-invisible); "`-EHOSTDOWN` before *any* I/O" (proving RPC absence);
  and the non-returning-RPC row. A `CONFIG`-gated fault shim plus a
  `pdg_gpio_bottom_put` call counter would cover the first three.

### 11.5 Assurance boundary

M4's devicetree topology rejections are the **first genuinely proved properties**
in this restructure — a foreign CS controller, a cross-parent one, a disabled one
and a missing `cs-gpios` each fail with their own distinct diagnostic, and the
same-parent clause is shown to fire independently.

But the four properties the spec argues *hardest* for — the defanged unlock, the
latch refusing a different slave before any I/O, the RX commit barrier, and
HOLD-requires-LOCK — remain source-shape assertions. The latch is currently
"tested" by asserting a byte offset between two source constructs.

**Nothing on this branch has ever executed. M5 is the first milestone that runs
anything, and it is where these four stop being claims.**
---

## 12. M5 outcome — first execution, and its verdict was FAIL

M5 landed as `abb32fe3f42f` (acceptance suite) + `0bb4b6afae3b` (ceiling fix).
**Its own verdict is FAIL**, correctly: the suite found three real defects, one
crash-class. That is what a first execution is for.

**Conductor-verified:** `PDG_SPI_MAX_BUFFER 1013U` with an explanatory comment;
`crates/` untouched; only `zephyr/drivers/spi/{Kconfig,pdg_spi.c}` changed;
`spi_nor_id` still builds clean at 121/121 with all three drivers compiled; tree
clean; nothing pushed.

One M5 claim was **wrong**: `book/book/` is **not** committed — it is ignored by
`book/.gitignore:1`. Nothing to fix.

### 12.1 The 3072 value was falsified — and my brief propagated it

The maintainer supplied 3072 and I sanctioned it in the M5 brief without
challenge. **It does not hold.**

| Arm | Request | Result | Failed where |
| --- | --- | --- | --- |
| before (`4096U`) | 4096 TX-only | `-70` `-ECOMM` | **transport** — reached the wire |
| 3072 candidate | 3072 TX **+ 3072 RX** | `-70` `-ECOMM` | still transport |
| after (`1013U`) | 4096 | `-90` `-EMSGSIZE` | **local**, no CS edge |

Every estimate — the maintainer's, mine, the architect's, and the reviewer's
`MAX_TRANSFER_SIZE + 1024` — reasoned about **one direction**. The original
failing case was TX-only; a loopback transfer is **full-duplex**, so both buffers
ride the same frame. The original account called 1013 the “true ceiling” and
made an invalid relative comparison with the 4096 packet-buffer bound; both
claims are superseded immediately below.

> **Corrected during M6 — “true ceiling” and “binary sweep” overstate the
> evidence, and the relative-shortfall comparison mixes unlike limits.** 1013
> is the largest **TX-only** length observed to work. The exact TX-only boundary
> remains unresolved between 1013 and 1015 because 1014 was not probed and 1015
> wedges the dispatcher. Full duplex is verified at 512, fails at 3072, and was
> not tested from 513 through 1013. Therefore 1013 is a conservative local
> containment limit, not a derived protocol limit, a duplex-capacity guarantee,
> or a generally usable payload. Applications needing a documented-safe Zephyr
> duplex size must use 512 bytes or less. Do not infer 1013-byte duplex support
> from `PDG_SPI_MAX_BUFFER`, and do not run the non-converging `--ceiling-sweep`.

M5 refused to guess a third number and measured instead. The constant now carries
a comment saying **do not raise this by guesswork either** — the defensible fix is
deriving the ceiling from worst-case request and response framing, expressed as
one shared contract rather than a constant duplicated per consumer. That needs a
wire-crate change with schema and lockstep implications, which D7 excludes.

**Lesson for future briefs:** a measured boundary from one direction does not
bound a duplex operation. Do not sanction a magic number without asking which
direction produced it.

### 12.2 Defect: a 1015-byte transfer wedges the device — crash-class

**A 1015-byte TX-only `spi/transfer` never returns and wedges the firmware
dispatcher device-wide.** Deterministic, reproduced twice with byte-identical
logs. It **survives host process death** — every subsequent RPC from a *fresh*
process hangs, including `system/reset-subscriptions`. The 2 s watchdog does
**not** catch it: the feeder task keeps feeding while a handler blocks.

That is exactly the gap AGENTS.md §13.17's 2026-06-03 row leaves open. Root cause
is in `crates/`, reachable from the CLI, Python and FFI — **not** a Zephyr defect.

**`1013` is containment, not a fix.** Decision: file it as its own issue, keep the
containment, finish SP1.

The original account treated `usbipd detach`/attach as the recovery mechanism
and generalized it to §13.17's 2026-06-03 row. That claim is superseded
immediately below. **Do not run `--ceiling-sweep`**: it is non-converging and
steps into the hang.

> **Corrected during M6 — recovery is an observation, not a demonstrated
> mechanism or a general correction to the older GPIO-wait incident.** In the
> reproduced SPI tests, the device resumed responding after USB re-enumeration;
> on Windows/WSL this used `usbipd detach` followed by attach. This does not
> prove that detach directly cancels the blocked firmware handler, and the
> GPIO-wait trigger was not rerun. On Linux or macOS, force equivalent
> re-enumeration by unplugging/reconnecting or USB unbind/rebind. If that is
> unavailable or ineffective, power-cycle the board. The wedged dispatcher
> cannot service `system/reset-subscriptions`.

### 12.3 Third defect, reported not fixed

`pdg_spi_cs_control_checked()` returns `0` when `cs_is_gpio` is false, so a
malformed config yields a **fully successful transfer with CS never asserted** and
no diagnostic. Upstream has the same shape so it is not a regression — but on this
controller `cs-gpios` is structurally mandatory, making the state
self-contradictory. Recommend a `LOG_WRN`; behaviour unchanged.

### 12.4 What passed

`M5_ACCEPTANCE_PASS`. CS edges **observed** via `SPI_HOLD_ON_CS | SPI_LOCK_ON`:
witness LOW while held, `LOW_TO_HIGH` across `spi_release()` in one process,
`-EINVAL` on a second release, `-ENOTSUP` for HOLD-without-LOCK **with the witness
unchanged**, proving rejection before any CS edge. The `-EHOSTDOWN` latch was
entered and recovered. Loopback echo is **exact** across all four modes, measured
not assumed — though a MOSI↔MISO short is **mode-blind**, so this proves byte
exactness, **not** CPOL/CPHA wire mapping.

Fixture validated in both directions before any measurement was trusted, honouring
the RP2350 pull-down trap.

### 12.5 Corrections

- **R7 and §11.4 were stale:** `M5_RESET_COUNT=0` — the orphaned pin-2
  subscription was **not present**. The reset step stays (its necessity is
  conditional on board state) but the premise was wrong.
- **§11.5 overstated M5.** T4 landed behaviourally; the other three properties did
  not. M5 records them in a committed `explicitly_untested` array — the right
  outcome.
- **M4's C-18 "4096 is a local boundary" is disproven** — `4096 > 4096 - 0` is
  false, so it passed locally and died at transport.
- **`native_sim`'s simulated clock does not advance during blocking USB calls**, so
  upstream `test_spi_complete_multiple_timed` is structurally unrunnable here — a
  lower-bound assert no multiplier can satisfy. Not flaky.
- **§3's inventory was short again** — sixth milestone running.
- **A `tee` pipeline masks the program's exit status**, which both reviewers caught
  independently: a failed reset or invalid fixture gate would have reported green
  and continued into physical actuation.

### 12.6 Board state

Subscriptions **0**; pin 2 `ExplicitOutput` witnessed **HIGH** (deasserted); pin 3
`Input/PullUp`; SPI left at mode3 @ 8 MHz by the loopback FAST spec; both jumpers
fitted; **no wedge**; `usbipd` detached; `/tmp` at 0%.
