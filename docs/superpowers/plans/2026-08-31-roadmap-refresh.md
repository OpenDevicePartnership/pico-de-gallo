# ROADMAP.md Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite `ROADMAP.md` around active workstreams instead of completed phases, remove every hand-maintained figure in favour of citations, and correct AGENTS.md §5.5.

**Architecture:** `ROADMAP.md` is rewritten top-to-bottom in seven ordered passes, one commit each, so a reviewer can follow the restructure section by section. Phases 1–3 collapse into a `Delivered` table; Phase 4 prose is retained and relocated into a Hardware workstream; Phase 5 becomes reconciled 1.0 criteria. An eighth pass fixes AGENTS.md §5.5, and a ninth verifies the acceptance criteria.

**Tech Stack:** Markdown. `rg` for verification, `dos2unix` for LF normalisation (AGENTS.md §3). No code, no build, no CI impact beyond `actionlint` being untouched.

**Spec:** `docs/superpowers/specs/2026-08-31-roadmap-refresh-design.md`

**Ground rules for every task:**

- Documentation-only. Do not touch any `Cargo.toml`, `Cargo.lock`, or source file. AGENTS.md §4 rule 12 is not engaged.
- Never invent a figure. Every number in this plan was measured on 2026-08-31 at `e129b58` and is recorded in spec §2. If you believe one is wrong, stop and report rather than substituting your own.
- Run `dos2unix -q <file>` after editing any file, before `git add`.
- Commit trailers (AGENTS.md §10): `Assisted-by:` and `Co-authored-by: Copilot`. **Never** `Signed-off-by:`.

---

### Task 1: Header, intro, table of contents, and Where We Are Today

**Files:**
- Modify: `ROADMAP.md:1-62` (through the end of `What's Missing`)

This replaces the `> Last updated` line, the intro, the Table of Contents, the `Progress Overview` table, and the whole `Where We Are Today` section.

- [ ] **Step 1: Write the verification checks and confirm they currently fail**

Run each of these. All five must report the "before" result shown.

```powershell
rg -c '2026-04-24' ROADMAP.md                      # before: 1
rg -c 'Progress Overview' ROADMAP.md               # before: 3
rg -c '298 unit' ROADMAP.md                        # before: 1
rg -c 'gallo-mcp|pyco-de-gallo' ROADMAP.md         # before: 0
rg -c 'book/src/appendix/endpoints.md' ROADMAP.md  # before: 0
```

- [ ] **Step 2: Replace lines 1 through 62**

Delete everything from line 1 up to and including the `What's Missing` bullet list (the line `- **Target power** — users must externally power their target`), and the `---` that follows it. Replace with:

````markdown
# Pico de Gallo — Roadmap to 1.0

> Last updated: 2026-08-31

This document describes where Pico de Gallo is going and what stands
between it and a 1.0 release. Work is grouped by active workstream;
completed work is summarised, not re-explained.

It is **not** a status dashboard. Figures that drift — endpoint counts,
test counts, crate versions — are *cited* here, never copied. Anything
derivable from the source tree belongs in the source tree. See
[Derived figures](#derived-figures) for where each one lives.

---

## Table of Contents

- [Where We Are Today](#where-we-are-today)
- [Design Philosophy](#design-philosophy)
- [Delivered](#delivered)
- [Active Workstreams](#active-workstreams)
- [1.0 Release Criteria](#10-release-criteria)
- [Should the RP2350 Stay?](#should-the-rp2350-stay)
- [Appendix A — embedded-hal Trait Coverage Matrix](#appendix-a--embedded-hal-trait-coverage-matrix)
- [Appendix B — Competitive Landscape](#appendix-b--competitive-landscape)
- [Appendix C — RP2350 Peripheral Budget](#appendix-c--rp2350-peripheral-budget)
- [Conventions — How to Update This File](#conventions--how-to-update-this-file)

---

## Where We Are Today

### Capabilities

| Area           | Status                                                                                                                                                                                                                 |
|----------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **I2C**        | 1 bus (I2C1, GPIO2/3), 7-bit addressing, read/write/write-read/scan/batch, configurable frequency (Standard/Fast/Fast+). A batch executes as a single `embedded-hal` transaction.                                     |
| **SPI**        | 1 bus (SPI0, GPIO4/6/7), read/write/flush/transfer/batch, configurable polarity and phase, DMA-backed                                                                                                                 |
| **UART**       | 1 bus (UART0, GPIO0/1), read/write/flush, configurable baud rate, interrupt-driven with 1024-byte TX/RX buffers                                                                                                       |
| **GPIO**       | 4 user pins (GPIO8–11), input/output with pull configuration, wait-for-edge with timeout, push-based edge event topics                                                                                                |
| **PWM**        | 4 channels (GPIO12–15) on 2 slices, frequency, duty cycle, phase-correct mode                                                                                                                                          |
| **ADC**        | 4 channels (GPIO26–29), 12-bit, single-shot reads                                                                                                                                                                      |
| **1-Wire**     | PIO-driven (GPIO16): reset with presence detect, read, write, strong-pullup write, ROM search                                                                                                                          |
| **USB**        | Full Speed (12 Mbps), postcard-rpc over raw USB bulk, WebUSB descriptors                                                                                                                                                |
| **HAL traits** | See [Appendix A](#appendix-a--embedded-hal-trait-coverage-matrix)                                                                                                                                                      |
| **Hardware**   | v1.0 landing board (7 connectors, 13/20 signals); v1.1 landing board (2×12 header, all 20 signals, I²C pull-ups, ADC protection). No level shifters, no ESD.                                                          |
| **Revisions**  | `hw-rev2` is the default. `hw-rev1` is deprecated — removal no earlier than 2031-09-01 — and returns `Unsupported` for all UART, ADC and 1-Wire endpoints.                                                            |

### Components

| Component        | Package                  | Role                                              | Release                 |
|------------------|--------------------------|---------------------------------------------------|-------------------------|
| Wire protocol    | `pico-de-gallo-internal` | postcard-rpc types; derives `SCHEMA_VERSION_*`    | crates.io               |
| Host library     | `pico-de-gallo-lib`      | async API over `nusb` + tokio                     | crates.io               |
| HAL bridge       | `pico-de-gallo-hal`      | `embedded-hal` / `embedded-io` trait impls        | crates.io               |
| C bindings       | `pico-de-gallo-ffi`      | cdylib, header generated by cbindgen              | crates.io               |
| CLI              | `gallo`                  | command-line front end                            | crates.io + binaries    |
| MCP server       | `gallo-mcp`              | Model Context Protocol server for agents          | crates.io               |
| Python bindings  | `pyco-de-gallo`          | PyO3 + maturin                                    | PyPI wheels             |
| Firmware         | `pico-de-gallo-firmware` | RP2350, `no_std`, separate Cargo workspace        | `.uf2` / `.elf`         |
| Zephyr module    | `zephyr/`                | MFD parent plus GPIO, I2C and SPI drivers         | unreleased              |

### Derived figures

These are deliberately **not** reproduced here. They drift, and each
already has an authoritative home that something else keeps honest.

| Figure                    | Authoritative source                                                                 |
|---------------------------|--------------------------------------------------------------------------------------|
| Endpoints and topics      | [`book/src/appendix/endpoints.md`](book/src/appendix/endpoints.md), which AGENTS.md §15.1 requires to track the `endpoints!` macro |
| Test counts               | AGENTS.md §5.5                                                                        |
| Crate versions            | each crate's `Cargo.toml` `[package].version`                                          |
| Wire schema version       | derived from `pico-de-gallo-internal`'s version by its `build.rs`                     |
| Transfer and batch limits | `MAX_TRANSFER_SIZE` and `MAX_BATCH_OPS` in `crates/pico-de-gallo-internal/src/lib.rs`, but see [Workstream A](#a-reliability--correctness) — the constant is a packet budget, not usable payload |

### What's Missing

- **10-bit I2C** — blocked upstream; see [Workstream E](#e-blocked-and-deferred)
- **Voltage flexibility** — hardware is 3.3 V only, no path to 1.8 V or 5 V
- **Target power** — users must externally power their target
- **Zephyr peripheral coverage** — no UART, ADC, PWM or 1-Wire driver; see [Workstream B](#b-zephyr-module)

---
````

- [ ] **Step 3: Re-run the verification checks**

```powershell
rg -c '2026-08-31' ROADMAP.md                      # expect: 1
rg -c 'Progress Overview' ROADMAP.md               # expect: 0
rg -c '298 unit' ROADMAP.md                        # expect: 0
rg -c 'gallo-mcp' ROADMAP.md                       # expect: >= 1
rg -c 'book/src/appendix/endpoints.md' ROADMAP.md  # expect: >= 1
```

`rg -c` exits non-zero and prints nothing when the count is zero. That is the expected outcome for the two "expect: 0" checks.

- [ ] **Step 4: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Commit with subject `docs(repo): Restate ROADMAP status without hand-maintained counts` and a body explaining that the Progress Overview table was deleted rather than corrected because a phase-completion count is the same class of drifting figure as the endpoint count, and that all nine components are now listed.

---

### Task 2: Replace Phases 1–3 with a Delivered section

**Files:**
- Modify: `ROADMAP.md` — delete from `## Phase 1 — Polish What Exists` through the `---` immediately before `## Phase 4 — Hardware Rev 2`

This deletes roughly 280 lines: three phase tables plus the `### 1.1`–`### 3.6` prose. Per spec §4.1 that rationale describes shipped work, is preserved behind the issue links, and is duplicated by the book's interface chapters.

**Do not delete the §1.6 `MAX_TRANSFER_SIZE` note yet.** Copy its text aside; Task 3 relocates it. It currently reads:

> **Note (M5, measured):** `MAX_TRANSFER_SIZE` is a **packet-buffer budget**, not usable payload. The buffer must also hold the postcard-rpc header, the length varint and COBS framing, and the budget covers the request frame *and* the response frame. On hardware the largest TX-only `spi/transfer` payload observed to work is **1013 bytes**, not 4096: 4096 TX-only and 3072 full duplex both fail `-ECOMM` at the transport. Full duplex is documented safe only at 512 bytes or less.

- [ ] **Step 1: Confirm the pre-state**

```powershell
rg -c '^## Phase 1|^## Phase 2|^## Phase 3' ROADMAP.md   # before: 3
rg -c 'deferred to post-1.0' ROADMAP.md                  # before: 3
```

- [ ] **Step 2: Replace the three phase sections**

````markdown
## Delivered

Completed work, kept for provenance. Each row links the issue that
carries the original rationale; the book documents the resulting
behaviour.

### Polish

| Item                             | Issue                                                                   | Outcome |
|----------------------------------|-------------------------------------------------------------------------|---------|
| Rich error types                 | [#1](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/1)   | Shipped |
| `SpiDevice` trait                | [#2](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/2)   | Shipped |
| I2C bus scan                     | [#3](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/3)   | Shipped |
| GPIO direction and pull control  | [#4](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/4)   | Shipped |
| Configuration query endpoints    | [#5](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/5)   | Shipped |
| `MAX_TRANSFER_SIZE` audit        | [#6](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/6)   | Shipped, but see [Workstream A](#a-reliability--correctness) — the audit's conclusion was later measured to be wrong |

### Protocols

| Item             | Issue                                                                     | Outcome |
|------------------|---------------------------------------------------------------------------|---------|
| UART support     | [#7](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/7)     | Shipped (hw-rev2) |
| PWM support      | [#8](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/8)     | Shipped |
| ADC support      | [#9](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/9)     | Shipped (hw-rev2) |
| Second I2C bus   | [#10](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/10)   | Dropped — one bus is sufficient; the pins are worth more as GPIO |
| Second SPI bus   | [#11](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/11)   | Dropped — same reasoning |

### Advanced

| Item                       | Issue                                                                     | Outcome |
|----------------------------|---------------------------------------------------------------------------|---------|
| GPIO event topics          | [#13](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/13)   | Shipped |
| I2C/SPI transaction batching | [#14](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/14) | Shipped; `i2c/batch` made genuinely atomic by [#128](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/128) |
| 1-Wire via PIO             | [#15](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/15)   | Shipped (hw-rev2) |
| Protocol sniffing          | [#16](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/16)   | **Dropped** |
| Multi-device host support  | [#18](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/18)   | **Dropped** |

### Beyond the original phases

Work that shipped without ever having a roadmap row.

| Item                                    | Issue                                                                     | Notes |
|-----------------------------------------|---------------------------------------------------------------------------|-------|
| Python bindings (`pyco-de-gallo`)        | [#30](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/30)   | PyO3 + maturin; wheels on PyPI |
| MCP server (`gallo-mcp`)                 | [#85](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/85)   | Per-call board selection by serial number |
| WebUSB descriptors                       | [#87](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/87)   | |
| `i2c/batch` atomicity                    | [#128](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/128) | One `transaction()`; repeated START on direction change. Framing not yet analyser-verified — [#160](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/160) |
| Zephyr module                            | —                                                                         | See [Workstream B](#b-zephyr-module) |
| `hw-rev1` / `hw-rev2` feature gating     | —                                                                         | `hw-rev2` default since `d1167c8` |
| `system/reset-subscriptions`             | —                                                                         | Recovers GPIO subscriptions orphaned by a host crash |

---
````

- [ ] **Step 3: Verify**

```powershell
rg -c '^## Phase 1|^## Phase 2|^## Phase 3' ROADMAP.md   # expect: 0
rg -c 'deferred to post-1.0' ROADMAP.md                  # expect: 0
rg -n '#16|#18' ROADMAP.md                                # both rows must read Dropped
rg -c '^### 1\.1|^### 2\.1|^### 3\.1' ROADMAP.md          # expect: 0
```

- [ ] **Step 4: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Subject: `docs(repo): Collapse completed phases into a Delivered section`. The body must state that #16 and #18 were **dropped**, not deferred as the document previously claimed, and that #10 and #11 were dropped too.

---

### Task 3: Workstream A — Reliability & Correctness

**Files:**
- Modify: `ROADMAP.md` — insert a new `## Active Workstreams` heading and section A immediately after the `Delivered` section

This is the section #161 cares most about: *"a roadmap that does not mention the project's recurring device-bricking failure mode is misleading about where effort should go."*

- [ ] **Step 1: Confirm the pre-state**

```powershell
rg -c 'dispatcher' ROADMAP.md   # before: 0
```

- [ ] **Step 2: Insert the workstream heading and section A**

````markdown
## Active Workstreams

Live work, grouped by the kind of effort it needs. Issue numbers are
listed rather than status markers — GitHub renders their state, so
these cannot go stale.

### A. Reliability & Correctness

**This is the highest-priority workstream.** Pico de Gallo has a
recurring, device-wide failure mode that has now appeared three times
from three unrelated triggers.

#### The dispatcher-wedge defect class

postcard-rpc dispatches handlers serially on a single `&mut Context`.
A handler that never returns therefore blocks **every** endpoint, not
just its own protocol family. The symptom is not "I2C stopped working"
but "the board stopped answering anything", recoverable only by USB
re-enumeration or a power cycle.

The 2 s watchdog does not catch it. `watchdog_feeder_task` is an
independent embassy task, so it keeps feeding while the dispatcher is
parked — the watchdog proves executor liveness, not dispatcher
progress. That gap is [#157](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/157).

Three instances so far. Each is documented in full in the regression
log at **AGENTS.md §13.17**; they are listed here only to establish
that this is a class rather than three unrelated bugs.

| Date       | Trigger                                            |
|------------|----------------------------------------------------|
| 2026-06-03 | `gpio/wait-*` on a pin that never transitions      |
| 2026-08-19 | `spi/transfer` at the packet-framing boundary      |
| 2026-08-26 | Zero-length I2C write                              |

Each was fixed or contained individually — a `timeout_ms` field on the
GPIO wait request, a 1013-byte cap in the Zephyr SPI driver, a firmware
guard rejecting empty writes. None of those addresses the shared root
cause, which is that a non-returning handler can still wedge the device.

#### The transfer-size ceiling

`MAX_TRANSFER_SIZE = 4096` is a **packet-buffer budget, not usable
payload**. The buffer must also hold the postcard-rpc header, the
length varint and COBS framing, and the budget covers the request frame
*and* the response frame.

Measured on hardware: the largest TX-only `spi/transfer` payload that
works is **1013 bytes**. 4096 TX-only and 3072 full duplex both fail
`-ECOMM` at the transport, and 1015 wedges the dispatcher. Full duplex
is documented safe only at 512 bytes or fewer.

The Zephyr driver caps itself at 1013. **Every other host surface — the
CLI, `pico-de-gallo-lib`, the HAL, the FFI, Python and MCP — can still
reach the wedge**, which is
[#158](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/158).

#### Open work

| Item                                                    | Issue                                                                     |
|---------------------------------------------------------|---------------------------------------------------------------------------|
| Watchdog proves executor liveness, not dispatcher progress | [#157](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/157) |
| Real payload ceiling unenforced on every host surface   | [#158](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/158) |
| Firmware build identity observable in `device/info`     | [#159](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/159) |
| Verify `i2c/batch` repeated-START framing on an analyser | [#160](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/160) |
| `SPI_CS` pin cannot be used as a chip select            | [#99](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/99)   |

[#159](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/159)
belongs here because `validate()` cannot currently distinguish two
firmware builds that report the same version but behave differently —
which has already caused a misidentified flash during a hardware
verification.
````

- [ ] **Step 3: Verify**

```powershell
rg -c 'dispatcher-wedge|dispatcher' ROADMAP.md      # expect: >= 3
rg -c 'AGENTS.md .13.17' ROADMAP.md                 # expect: >= 1
rg -c '1013' ROADMAP.md                             # expect: >= 1
rg -c 'issues/157|issues/158|issues/159|issues/160|issues/99' ROADMAP.md  # expect: >= 5
```

- [ ] **Step 4: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Subject: `docs(repo): Add the reliability workstream and the wedge defect class`. Body should note that the `MAX_TRANSFER_SIZE` finding moved out of a section describing completed work, which is what #161 asked for.

---

### Task 4: Workstreams B through E

**Files:**
- Modify: `ROADMAP.md` — append B, C, D, E after workstream A; delete the `## Phase 4 — Hardware Rev 2` heading and its intro, retaining the `### 4.1`–`### 4.6` prose bodies under workstream C

The Phase 4 prose subsections are **kept verbatim**. That work is open, so its rationale is live. Only the heading and the phase table change.

- [ ] **Step 1: Confirm the pre-state**

```powershell
rg -c '^## Phase 4' ROADMAP.md      # before: 1
rg -c '^### 4\.1' ROADMAP.md        # before: 1
rg -c 'zephyr' ROADMAP.md           # before: 0
```

- [ ] **Step 2: Insert workstreams B and C's table before the retained 4.x prose**

Place this immediately after workstream A. Then move the existing
`### 4.1` through `### 4.6` prose bodies so they sit beneath workstream
C's `#### Open work` table, applying exactly these six heading
rewrites and leaving each body paragraph untouched:

| Existing heading                      | Becomes                                |
|---------------------------------------|----------------------------------------|
| `### 4.1 Voltage Level Translators`   | `#### Voltage Level Translators (#20)` |
| `### 4.2 Target Power Output`         | `#### Target Power Output (#21)`       |
| `### 4.3 Dedicated Connector Layout`  | `#### Dedicated Connector Layout — shipped in v1.1 (#22)` |
| `### 4.4 ESD Protection`              | `#### ESD Protection (#23)`            |
| `### 4.5 Activity LEDs`               | `#### Activity LEDs (#24)`             |
| `### 4.6 Board Size and Mounting`     | `#### Board Size and Mounting (#25)`   |

Nothing links to these headings, so no anchor updates are needed —
Task 9 Step 2 verifies that. Keep 4.3's body even though it shipped;
it documents the connector choices the v1.1 board actually made.

````markdown
### B. Zephyr Module

`zephyr/` provides an out-of-tree Zephyr module that presents a
Pico de Gallo board as a normal Zephyr device tree node, so drivers
written against Zephyr's I2C, SPI and GPIO APIs run unmodified on a
host PC.

The module is documented in `zephyr/README.md`, which is deliberately
authoritative — there is no book chapter for it. See AGENTS.md §15.1
for that ruling.

Shipped: an MFD parent (`odp,pico-de-gallo`) with GPIO, I2C and SPI
children, four device tree bindings, four samples, three test suites,
and a path-filtered CI job.

Two limits are worth stating plainly. CI is **build-only** — it never
executes a produced binary, because that reaches `gallo_init_strict()`
and needs an attached board, so a green run proves the module compiles
and links, not that it works. And nothing here has been released;
`zephyr/CHANGELOG.md` is entirely `[Unreleased]`.

#### Open work

| Item                                        | Issue                                                                     |
|---------------------------------------------|---------------------------------------------------------------------------|
| UART driver                                 | [#152](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/152) |
| ADC driver                                  | [#153](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/153) |
| 1-Wire (w1) driver                          | [#154](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/154) |
| PWM driver                                  | [#155](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/155) |
| `PDG_I2C_MAX_BUFFER` is unmeasured          | [#146](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/146) |
| Upstream the module                         | [#98](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/98)   |

[#146](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/146)
is the I2C analogue of the SPI ceiling in
[Workstream A](#a-reliability--correctness): 4096 was assumed for SPI
and turned out to be 1013, and the I2C limit has never been measured.

### C. Hardware Rev 2

*Requires a PCB re-spin, component sourcing, and potentially a case
redesign.*

v1.1 shipped the connector rework
([#22](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/22),
tag `hardware-v1.1.1`). The remaining items are ordered by
impact-to-cost ratio, and the re-spin should be done **once** with all
of them. Level translators and ESD protection are the non-negotiable
additions.

#### Open work

| Item                        | Issue                                                                   |
|-----------------------------|-------------------------------------------------------------------------|
| Voltage level translators   | [#20](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/20) |
| Target power output         | [#21](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/21) |
| ESD protection              | [#23](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/23) |
| Activity LEDs               | [#24](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/24) |
| Board size and mounting     | [#25](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/25) |
````

- [ ] **Step 3: Append workstreams D and E after the relocated 4.x prose**

````markdown
### D. Documentation & Tooling

| Item                                                        | Issue                                                                     |
|-------------------------------------------------------------|---------------------------------------------------------------------------|
| A book on driver development with Rust, device-driver and Pico de Gallo | [#79](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/79)  |
| Hardware-in-the-loop CI runners for board-attached tests    | [#162](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/162) |

[#162](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/162)
gates a lot. Every behavioural claim in this project currently rests on
a manual, board-attached procedure — the Zephyr M5 script, the
hardware verifications behind the AGENTS.md §13.17 entries, and the 11
`#[ignore]`d tests in the host workspace. Until CI can run those, the
reliability workstream cannot have regression coverage.

### E. Blocked and Deferred

| Item                     | Issue                                                                   | State |
|--------------------------|-------------------------------------------------------------------------|-------|
| 10-bit I2C addressing    | [#12](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/12) | **Blocked upstream.** Needs embassy-rs/embassy#5927 to land plus an embassy-rp release, and a postcard-rpc dependency PR |
| Configuration persistence | [#17](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/17) | Deferred to post-1.0 |

---
````

- [ ] **Step 4: Verify**

```powershell
rg -c '^## Phase 4' ROADMAP.md                        # expect: 0
rg -c '^### A\.|^### B\.|^### C\.|^### D\.|^### E\.' ROADMAP.md   # expect: 5
rg -c 'Voltage Level Translators' ROADMAP.md          # expect: >= 1  (prose retained)
rg -c 'issues/152|issues/153|issues/154|issues/155' ROADMAP.md    # expect: >= 4
rg -c 'issues/12|issues/17' ROADMAP.md                # expect: >= 2
```

Then confirm every issue from spec §2.6 appears exactly once as a workstream row:

```powershell
foreach ($n in 157,158,159,160,99,98,146,152,153,154,155,20,21,23,24,25,79,162,12,17) {
  $c = (rg -c "issues/$n\b" ROADMAP.md); Write-Output "$n -> $c"
}
```

Every issue must report at least 1.

- [ ] **Step 5: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Subject: `docs(repo): Add Zephyr, hardware, tooling and deferred workstreams`.

---

### Task 5: Reconcile the 1.0 release criteria

**Files:**
- Modify: `ROADMAP.md` — replace `## Phase 5 — 1.0 Release Criteria` and its three tables

- [ ] **Step 1: Confirm the pre-state**

```powershell
rg -c '^## Phase 5' ROADMAP.md              # before: 1
rg -c 'Protocol sniffing' ROADMAP.md        # before: >= 1 (in the nice-to-have table)
```

- [ ] **Step 2: Replace the section**

````markdown
## 1.0 Release Criteria

### Must have

| Requirement                                                        | State |
|--------------------------------------------------------------------|-------|
| `I2c`, `SpiBus`, `SpiDevice`, `InputPin`, `OutputPin`, `StatefulOutputPin`, `Wait`, `DelayNs` and `SetDutyCycle` implemented, blocking and async where the trait defines both | ✅ Done — see [Appendix A](#appendix-a--embedded-hal-trait-coverage-matrix) |
| `embedded-io` `Read`/`Write` for UART                              | ✅ Done, for both the 0.6 and 0.7 majors |
| Rich error types mapping firmware errors to `ErrorKind`            | ✅ Done |
| I2C bus scan                                                       | ✅ Done |
| GPIO direction and pull configuration                              | ✅ Done |
| UART support                                                       | ✅ Done (hw-rev2) |
| Configuration query endpoints                                      | ✅ Done |
| All public API types documented with rustdoc                       | Ongoing |
| Book covers every interface, with examples                         | Ongoing |
| Stable wire protocol — no breaking serialization changes after 1.0 | ❌ **Not met.** The schema is still moving; wire-behaviour changes landed as recently as the unreleased schema 0.7 |
| No known device-wide wedge reachable from a supported host surface | ❌ **Not met** — [Workstream A](#a-reliability--correctness) |

The trait list is deliberately explicit rather than "all
`embedded-hal` traits". Three traits are excluded, each with a reason:

- **`I2c<TenBitAddress>`** — blocked upstream
  ([#12](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/12)).
- **`embedded_io::ReadReady`** and **`WriteReady`** — not implemented.
  Both need a firmware endpoint reporting buffer occupancy, which does
  not exist. Not a 1.0 blocker.

### Should have

| Requirement                     | State |
|---------------------------------|-------|
| PWM support                     | ✅ Done |
| ADC support                     | ✅ Done (hw-rev2) |
| GPIO event topics               | ✅ Done |
| I2C transaction batching        | ✅ Done, and atomic since [#128](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/128) |
| 10-bit I2C addressing           | ❌ Blocked upstream — [Workstream E](#e-blocked-and-deferred) |
| CLI: I2C bus scan               | ✅ Done — `gallo i2c scan` |
| CLI: interactive UART terminal  | ❌ Not implemented. The CLI has `uart read`, `uart write` and `uart flush`, but no terminal mode |
| CLI: interactive GPIO           | ❌ Not implemented. The CLI has `gpio get`, `gpio put`, `gpio set-config` and `gpio monitor`, but no interactive mode |

### Post-1.0

| Requirement               | State |
|---------------------------|-------|
| 1-Wire via PIO            | ✅ **Already shipped** ahead of 1.0 |
| Configuration persistence | Deferred — [#17](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/17) |
| SPI target mode           | Not started |
| Hardware Rev 2            | [Workstream C](#c-hardware-rev-2) |
| Zephyr peripheral coverage | [Workstream B](#b-zephyr-module) |

### Where 1.0 actually stands

No must-have is waiting on a feature that is both unimplemented and
unblocked. What remains is not feature work:

1. **Wire-protocol stability.** 1.0 means committing to the
   serialization format. Workstream A is still changing wire behaviour.
2. **Reliability.** Shipping 1.0 with a known, reachable device-wide
   wedge would be dishonest about what the version number means.
3. **Documentation.** Ongoing, and gated in part on
   [#79](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/79).

Protocol sniffing and multi-device host support were previously listed
here as post-1.0 deliverables. Both are
[dropped](#delivered) and are no longer planned in any timeframe.

---
````

- [ ] **Step 3: Verify**

```powershell
rg -c '^## Phase 5' ROADMAP.md            # expect: 0
rg -c 'Not implemented' ROADMAP.md        # expect: 2  (UART terminal, interactive GPIO)
rg -c 'TenBitAddress' ROADMAP.md          # expect: >= 1
rg -c 'ReadReady' ROADMAP.md              # expect: >= 1
rg -n '1-Wire via PIO' ROADMAP.md         # must read "Already shipped"
```

- [ ] **Step 4: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Subject: `docs(repo): Reconcile the 1.0 criteria against shipped work`. Body must record that 1-Wire was listed post-1.0 but shipped, that sniffing and multi-device were dropped, that the CLI row was split into three because two of its three parts are not implemented, and that the "all embedded-hal traits" must-have was reworded because read literally it was not met.

---

### Task 6: Correct Appendices A, B and C

**Files:**
- Modify: `ROADMAP.md` — the three appendix sections

- [ ] **Step 1: Confirm the pre-state**

```powershell
rg -n '8 pins' ROADMAP.md            # before: 1 line, Appendix B's GPIO row
rg -n 'user: 8' ROADMAP.md           # before: 1 line, Appendix C's GPIO row
rg -n '4 slices' ROADMAP.md          # before: 1 line, Appendix C's PWM row (two occurrences on it)
rg -c 'sniffing' ROADMAP.md          # before: 6 lines
```

Note that `rg -c` counts *matching lines*, not occurrences. The PWM row
contains `4 slices` twice on one line.

- [ ] **Step 2: Appendix A — fix the embedded-io rows**

Replace the two `embedded-io` rows and the note beneath the table:

````markdown
| `Read`                 | `embedded-io`        | ✅       | ✅    | Done      |
| `Write`                | `embedded-io`        | ✅       | ✅    | Done      |
| `ReadReady`            | `embedded-io`        | ❌       | —     | Excluded  |
| `WriteReady`           | `embedded-io`        | ❌       | —     | Excluded  |

`Read` and `Write` are implemented **eight** times over: blocking and
async, for both the 0.6 and the 0.7 majors. `pico-de-gallo-hal` carries
both majors side by side behind additive features — `embedded-io-06` is
on by default, `embedded-io-07` is opt-in — so a driver written against
either can be developed against a board. See AGENTS.md §7.3 for why
this is multi-major by design rather than a migration.

`ReadReady` and `WriteReady` need a firmware endpoint reporting buffer
occupancy, which does not exist. They are excluded from 1.0 rather than
pending; see [1.0 Release Criteria](#10-release-criteria).
````

- [ ] **Step 3: Appendix B — fix the GPIO row and the 1-Wire row**

Two cell edits in the competitive table:

| Row        | Column                     | From             | To               |
|------------|----------------------------|------------------|------------------|
| **GPIO**   | Pico de Gallo (current)    | `✅ (8 pins)`    | `✅ (4 pins)`    |
| **1-Wire** | Pico de Gallo (1.0 target) | `✅`             | `✅ (PIO)`       |

Then replace the fourth bullet under **Unique differentiators at 1.0**,
which currently implies sniffing is planned:

````markdown
4. PIO enables protocols no other bridge at this price can match —
   1-Wire today, and the state machines remain available for more
````

- [ ] **Step 4: Appendix C — fix the PWM, GPIO and PIO rows**

````markdown
| **PWM**    | 12 slices (24 ch) | 2 slices (4 channels)                 | 2 slices         | GPIO12–15                                 |
| **PIO**    | 3 (PIO0–2)        | 1 (1-Wire)                            | 1                | Sniffing dropped; two state machines free |
| **GPIO**   | 30 (on Pico 2)    | 16 (I2C 2, SPI 3, UART 2, user 4, PWM 4, 1-Wire 1) | ~16 | Plenty of headroom                        |
````

Leave the conclusion paragraph beneath the table unchanged; it still holds.

- [ ] **Step 5: Verify**

```powershell
rg -c '8 pins' ROADMAP.md         # expect: 0
rg -c 'user: 8' ROADMAP.md        # expect: 0
rg -c '4 slices' ROADMAP.md       # expect: 0
rg -c '2 slices' ROADMAP.md       # expect: >= 2
rg -c 'Excluded' ROADMAP.md       # expect: >= 2
```

- [ ] **Step 6: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Subject: `docs(repo): Correct the appendix pin, slice and trait counts`. Body must name the two self-contradictions: the document claimed both 4 and 8 user GPIOs, and claimed 4 PWM slices where the firmware uses 2.

---

### Task 7: Rewrite the update conventions

**Files:**
- Modify: `ROADMAP.md` — the `## Conventions — How to Update This File` section, and the `## Summary` section that precedes the appendices

The existing conventions describe checkbox tables and a Progress Overview that no longer exist.

- [ ] **Step 1: Confirm the pre-state**

```powershell
rg -c 'Progress Overview' ROADMAP.md   # expect: 0 already; the conventions reference it by prose
rg -c 'Checking off items' ROADMAP.md  # before: 1
```

- [ ] **Step 2: Replace the Summary section**

The old `## Summary` recommends a phase sequence that no longer exists. Replace it with:

````markdown
## Summary

The path to 1.0 is no longer a feature effort. Every must-have feature
that is not blocked upstream has shipped.

What remains, in priority order:

1. **Reliability** — close the dispatcher-wedge class rather than
   containing each instance. [Workstream A](#a-reliability--correctness).
2. **Wire-protocol stability** — 1.0 is a commitment to the
   serialization format, and it cannot be made while that format is
   still moving.
3. **Zephyr** — peripheral coverage and upstreaming.
   [Workstream B](#b-zephyr-module).
4. **Documentation** — rustdoc, the book, and the driver-development
   guide.
5. **Hardware Rev 2** — planned as one re-spin, not incrementally.
   [Workstream C](#c-hardware-rev-2).

The RP2350 remains the right MCU. See
[Should the RP2350 Stay?](#should-the-rp2350-stay).

---
````

- [ ] **Step 3: Replace the conventions section**

````markdown
## Conventions — How to Update This File

### Do not add counts

This file must not carry any figure that can be derived from the source
tree. Endpoint counts, test counts, crate versions and schema versions
belong to their authoritative homes, listed under
[Derived figures](#derived-figures). Cite the home; do not copy the
number.

This rule exists because the figures previously kept here drifted badly
enough to make the document misleading — at one point it claimed 298
unit tests against an actual 589, and 27 endpoints against an actual 47.

### Moving an item

When work completes, move its row from the relevant workstream table
into the matching [Delivered](#delivered) table and set the outcome to
`Shipped`. When work is abandoned, move it the same way and set the
outcome to `Dropped`. Do not delete rows — provenance is the point of
that section.

### Adding an item

Add a row to whichever workstream it belongs to, with a link to its
issue. Do not add status markers, checkboxes or done-counts; GitHub
already renders issue state, and a hand-maintained count is the exact
failure this document is recovering from.

If the item is a 1.0 blocker, also add a row to
[1.0 Release Criteria](#10-release-criteria).

### When a new dispatcher wedge is found

Add the row to AGENTS.md §13.17, which is the authoritative regression
log, then add one line to the instance table in
[Workstream A](#a-reliability--correctness) — date and trigger only.
Do not restate the detail here; it will drift.

### Updating the date

Update the `> Last updated:` line whenever you commit changes to this
file.
````

- [ ] **Step 4: Verify**

```powershell
rg -c 'Checking off items' ROADMAP.md    # expect: 0
rg -c 'Do not add counts' ROADMAP.md     # expect: 1
rg -c 'Derived figures' ROADMAP.md       # expect: >= 2
```

- [ ] **Step 5: Normalise and commit**

```powershell
dos2unix -q ROADMAP.md
git add ROADMAP.md
```

Subject: `docs(repo): Rewrite the roadmap update conventions`.

---

### Task 8: Correct AGENTS.md §5.5

**Files:**
- Modify: `AGENTS.md:259-267`

Required because Task 1 makes `ROADMAP.md` cite §5.5 as the authoritative home for test counts, and §5.5 is wrong. See spec §2.2 and §4.7.

- [ ] **Step 1: Confirm the measured figures still hold**

```powershell
cargo test --locked 2>&1 | Select-String -Pattern 'test result:'
```

Expected, summed: **589 passed, 11 ignored** across seven binaries, plus 7 doctests (`hal` 2, `internal` 1, `lib` 4). If your run differs, use your own numbers and say so in the commit body — do not copy stale ones.

- [ ] **Step 2: Replace lines 261-267**

Current text:

```
About **561 unit tests + 7 doctests** across the host workspace,
concentrated in `pico-de-gallo-internal` (159), `pico-de-gallo-ffi`
(116), and `pico-de-gallo-mcp` (114). Seven of the `pico-de-gallo-mcp`
tests are `#[ignore]`d because they need two boards attached; run
them with `cargo test -p gallo-mcp -- --ignored`.
`pyco-de-gallo` has 8 Rust-side unit tests. If you add code, add tests
next to it; round-trip serialization tests are the norm for wire types.
```

Replace with:

````markdown
About **589 unit tests + 7 doctests** across the host workspace,
measured 2026-08-31:

| Crate                    | Passing | `#[ignore]`d |
|--------------------------|---------|--------------|
| `pico-de-gallo-internal` | 163     | 0            |
| `pico-de-gallo-ffi`      | 123     | 0            |
| `gallo-mcp`              | 111     | 7            |
| `pico-de-gallo-lib`      | 74      | 4            |
| `gallo`                  | 67      | 0            |
| `pico-de-gallo-hal`      | 43      | 0            |
| `pyco-de-gallo`          | 8       | 0            |

Doctests: `pico-de-gallo-lib` 4, `pico-de-gallo-hal` 2,
`pico-de-gallo-internal` 1.

All 11 ignored tests are board-attached and therefore never run in CI:

- The 7 in `gallo-mcp` need **two** boards, because they cover
  per-call serial-number target selection. Run with
  `cargo test -p gallo-mcp -- --ignored`.
- The 4 in `pico-de-gallo-lib` are the #135 zero-length-write
  regression tests. They need one board, and
  `empty_batch_write_never_reaches_the_bus` additionally needs a
  TMP102-like target on the I2C bus. Run with
  `cargo test -p pico-de-gallo-lib -- --ignored`.

If you add code, add tests next to it; round-trip serialization tests
are the norm for wire types.
````

- [ ] **Step 3: Verify**

```powershell
rg -c '561 unit' AGENTS.md    # expect: 0
rg -c '589 unit' AGENTS.md    # expect: 1
rg -c 'TMP102-like target' AGENTS.md  # expect: >= 1
```

- [ ] **Step 4: Normalise and commit**

```powershell
dos2unix -q AGENTS.md
git add AGENTS.md
```

Subject: `docs(repo): Correct the AGENTS.md test baseline`. Body must state that the roadmap now cites §5.5 as the authoritative source for test counts, so a stale figure there would be laundered into the roadmap, and that the previous text undercounted ignored tests by omitting `pico-de-gallo-lib` entirely.

---

### Task 9: Final acceptance sweep

**Files:** none modified unless a check fails.

- [ ] **Step 1: Run every acceptance criterion from spec §6**

```powershell
Write-Output "--- 1. no hand-maintained counts"
rg -n '\b(298|561|27 total|47 total)\b' ROADMAP.md          # expect: no matches

Write-Output "--- 2. all nine components"
foreach ($c in 'pico-de-gallo-internal','pico-de-gallo-lib','pico-de-gallo-hal','pico-de-gallo-ffi','`gallo`','gallo-mcp','pyco-de-gallo','pico-de-gallo-firmware','zephyr') {
  $n = (rg -c --fixed-strings $c ROADMAP.md); Write-Output "$c -> $n"
}

Write-Output "--- 3. #16 and #18 dropped"
rg -n 'issues/16\)|issues/18\)' ROADMAP.md

Write-Output "--- 4/5. wedge class and relocated note"
rg -c 'AGENTS.md .13.17' ROADMAP.md
rg -c '1013' ROADMAP.md

Write-Output "--- 6. every issue in exactly one workstream"
foreach ($n in 157,158,159,160,99,98,146,152,153,154,155,20,21,23,24,25,79,162,12,17) {
  $c = (rg -c "issues/$n\b" ROADMAP.md); Write-Output "$n -> $c"
}

Write-Output "--- 7/8. appendix corrections"
rg -c '8 pins|user: 8|4 slices' ROADMAP.md                  # expect: no matches

Write-Output "--- 9. trait must-have reworded"
rg -c 'TenBitAddress' ROADMAP.md

Write-Output "--- 10. AGENTS 5.5"
rg -c '589 unit' AGENTS.md

Write-Output "--- 11. date"
rg -n 'Last updated' ROADMAP.md
```

- [ ] **Step 2: Verify no internal link points at a deleted anchor**

Extract every in-document anchor link and confirm a matching heading exists:

```powershell
$anchors = (rg -o --no-filename '\]\(#([a-z0-9-]+)\)' -r '$1' ROADMAP.md) | Sort-Object -Unique
$headings = (rg -o --no-filename '^#{2,4} (.+)$' -r '$1' ROADMAP.md) |
  ForEach-Object { ($_ -replace '[^\w\s-]','' -replace '\s+','-').ToLower() }
$anchors | Where-Object { $headings -notcontains $_ } |
  ForEach-Object { Write-Output "BROKEN ANCHOR: $_" }
```

Expected output: nothing. If an anchor is reported broken, verify it by hand — the slug approximation above does not handle every GitHub edge case, notably em dashes and ampersands — and fix any genuine break.

- [ ] **Step 3: Confirm LF endings on both files**

```powershell
git diff --cached --check
file ROADMAP.md AGENTS.md 2>$null
```

Neither file may report CRLF. If either does, run `dos2unix -q` on it and re-stage.

- [ ] **Step 4: Confirm the change is documentation-only**

```powershell
git diff --stat main...HEAD
```

Expected: only `ROADMAP.md`, `AGENTS.md`, and the two files under `docs/superpowers/`. **No `Cargo.toml`, no `Cargo.lock`, no `crates/**`, no `zephyr/**`.** If anything else appears, stop and report.

- [ ] **Step 5: Confirm the host workspace is untouched**

```powershell
cargo check --workspace --locked
```

Expected: success, with no lockfile drift. This is a documentation PR, so this should be a no-op — it is here to catch an accidental source edit, not to validate anything.

- [ ] **Step 6: Fix anything that failed, then report**

If every check passes, report the result and stop. Do **not** push or open a PR without explicit permission (AGENTS.md §4 rule 8).

---

## Notes for the implementer

- **`rg -c` prints nothing and exits 1 when the count is zero.** That is success for the "expect: 0" checks, not an error. It also counts *matching lines*, not occurrences.
- **Intermediate commits contain forward references.** Task 1 links to `#a-reliability--correctness`, which Task 3 creates. Tasks 1–7 are one logical change to one file, split for reviewability; intra-document anchors only all resolve at Task 7, and Task 9 Step 2 is the gate. Nothing in CI builds `ROADMAP.md`, so no check fails in between. If you would rather each commit stand alone, squash Tasks 1–7 into a single commit before opening the PR — that is a legitimate reading of AGENTS.md §4 rule 9.
- **Do not reintroduce a Progress Overview.** If you feel the document needs an at-a-glance status view, that is a design change; raise it rather than adding one.
- **Preserve `## Should the RP2350 Stay?` verbatim.** It was reviewed and is still accurate. No task touches it.
- **The book needs no change.** `ROADMAP.md` has no book chapter and is absent from the AGENTS.md §15.1 per-area mapping, so the book-parity rule does not bind here. The book's endpoint catalog was verified in sync on 2026-08-31.
- **Leave `docs/superpowers/specs/2026-08-19-zephyr-mfd-m5-acceptance.md:741` alone.** It says "ROADMAP §1.6 treats 4096 as generally usable payload", and §1.6 ceases to exist in Task 2. That reference becomes historical rather than broken: past specs are point-in-time records of what was true when they were written, and rewriting them to match a later tree destroys their value as evidence. Do not update it.
- **If a measured figure disagrees with spec §2**, trust your own measurement, use it, and say so in the commit body. Do not silently keep the spec's number.
