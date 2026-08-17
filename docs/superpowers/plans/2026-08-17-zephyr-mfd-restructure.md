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

Per milestone, minimum:

```bash
wsl -e bash -lc 'cd /mnt/d/workspace/pico-de-gallo && source ~/zephyrproject/.venv/bin/activate && west build -p always -b native_sim zephyr/samples/<name>'
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
