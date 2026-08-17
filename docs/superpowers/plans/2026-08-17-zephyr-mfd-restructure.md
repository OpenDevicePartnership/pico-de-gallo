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

---

## 3. File inventory

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