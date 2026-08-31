# ROADMAP.md refresh — design

> Issue: [#161](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/161)
> Branch: `issue-161`
> Date: 2026-08-31

## 1. Problem

`ROADMAP.md` carries `> Last updated: 2026-04-24`. It is the document
iteration planning runs off, so drift in it is actively harmful rather
than cosmetic.

Two separate defects compound:

1. **Stale figures.** Hand-maintained counts have drifted from their
   sources of truth.
2. **Stale organisation.** The five-phase structure described the work
   as it was planned in April. Phases 1–3 are now closed, and the work
   that actually consumes effort today — the Zephyr module, the
   dispatcher-wedge defect class, 1.0 polish — has no place in the
   document at all.

The second defect is the more serious one. A roadmap with correct
numbers that still points at finished work is misleading about where
effort should go.

### 1.1 The issue is itself partly stale

`ROADMAP.md` was edited after #161 was filed, so three of the issue's
four "verified inaccuracies" no longer hold on `main`. This spec
supersedes the issue's table.

| #161 claim | State of `main` at `e129b58` |
|---|---|
| line 51 reads "27 total" | reads **47 total** — count correct, but the parenthetical omits the `gpio/event` topic |
| families are "I2C×5, SPI×5, UART×5, GPIO×8, config×4" | already reads I2C×7, SPI×7, UART×5, GPIO×10, PWM×6, ADC×2, 1-Wire×6 |
| Phase 4 treats v1.1 as future | 4.3 already marked ✅ shipped in v1.1 |
| "Tests: 298 unit + 5 doctests" | **still wrong** |
| "Host crates: internal, lib, hal, ffi, app" | **still wrong** |

### 1.2 A defect the issue could not have known

Issues [#16](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/16)
(Protocol Sniffing) and
[#18](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/18)
(Multi-Device Host Support) were both closed on 2026-08-31 with the
comment "Dropping". `ROADMAP.md` shows both as `⏳ deferred to
post-1.0`, which is the opposite of true, and lists sniffing and
multi-device as post-1.0 deliverables in Phase 5.

## 2. Verified ground truth

Every figure below was derived on 2026-08-31 at `e129b58`. The
implementer should not re-derive these; they should copy them, and cite
the source rather than the number wherever the target document allows.

### 2.1 Endpoints and topics

`crates/pico-de-gallo-internal/src/lib.rs`, `endpoints!` at line 255,
rows 259–305; `topics!` `TOPICS_OUT_LIST` at 315–321.

| Family | Count |
|---|---|
| `ping`, `version`, `device/info`, `system/reset-subscriptions` | 1 each |
| i2c | 7 |
| spi | 7 |
| uart | 5 |
| gpio | 10 |
| pwm | 6 |
| adc | 2 |
| onewire | 6 |

**47 endpoints + 1 topic (`gpio/event`) = 48 unique paths.**
`TOPICS_IN_LIST` is empty.

`book/src/appendix/endpoints.md` lines 25–71 and 94 already match this
exactly, 1:1, with no missing or extra paths.

### 2.2 Tests

Measured with `cargo test --locked` from the repository root:

| Crate | Passing | Ignored |
|---|---|---|
| `pico-de-gallo-internal` | 163 | 0 |
| `pico-de-gallo-ffi` | 123 | 0 |
| `gallo-mcp` | 111 | 7 |
| `pico-de-gallo-lib` | 74 | 4 |
| `gallo` | 67 | 0 |
| `pico-de-gallo-hal` | 43 | 0 |
| `pyco-de-gallo` | 8 | 0 |
| **Total** | **589** | **11** |

Doctests: `hal` 2, `internal` 1, `lib` 4 — **7 total**.

All 11 ignored tests are board-attached. The 7 in `gallo-mcp` need two
boards. The 4 in `pico-de-gallo-lib` are the #135 zero-length-write
regression tests at `crates/pico-de-gallo-lib/src/lib.rs:2623`, `:2642`,
`:2654`, `:2684`; one of them additionally needs a TMP102-like target.

AGENTS.md §5.5 currently claims "~561 unit + 7 doctests", with a
per-crate breakdown of internal 159 / ffi 116 / mcp 114, and says seven
tests are ignored. All four of those figures are wrong, and the ignored
count omits `pico-de-gallo-lib` entirely.

### 2.3 Components

Root `Cargo.toml` lists seven host members; the firmware is a separate
workspace; the Zephyr module is neither.

| Component | Package | Version | Notes |
|---|---|---|---|
| protocol | `pico-de-gallo-internal` | 0.7.0 | wire types; derives `SCHEMA_VERSION_*` |
| host library | `pico-de-gallo-lib` | 0.8.0 | async, nusb + tokio |
| HAL bridge | `pico-de-gallo-hal` | 0.7.0 | embedded-hal impls |
| C bindings | `pico-de-gallo-ffi` | 0.8.0 | cdylib + cbindgen |
| CLI | `gallo` | 0.9.0 | binary `gallo` |
| MCP server | `gallo-mcp` | 0.3.0 | binary `gallo-mcp`; shipped #85 |
| Python bindings | `pyco-de-gallo` | 0.5.0 | `publish = false`; PyPI wheels |
| firmware | `pico-de-gallo-firmware` | 0.11.0 | separate workspace, no_std |
| Zephyr module | `zephyr/` | — | `CHANGELOG.md` is entirely `[Unreleased]` |

### 2.4 Zephyr module

MFD parent (`odp,pico-de-gallo`) plus GPIO, I2C and SPI children. Four
DT bindings, four samples, three test suites (`pdg_fake/i2c`,
`pdg_i2c_burst`, `pdg_mfd_m5` with four apps). CI
(`.github/workflows/zephyr.yml`) is path-filtered and **build-only** —
it never executes a produced binary, because that would reach
`gallo_init_strict()` and need an attached board.

**No UART, ADC, PWM or 1-Wire driver exists.** Those are issues #152,
#153, #155 and #154 respectively.

### 2.5 embedded-hal coverage

Verified against `crates/pico-de-gallo-hal/src/lib.rs`. Every Appendix A
row is confirmed, with one correction: the two `embedded-io` rows
understate reality. There are eight impls, not two — `Read` and `Write`,
each blocking and async, each for both 0.6 and 0.7, at `:1867`, `:1874`,
`:1885`, `:1892`, `:1908`, `:1915`, `:1926`, `:1933`.

`TenBitAddress`, `ReadReady` and `WriteReady` are confirmed absent
(zero grep matches).

### 2.6 Open issues, clustered

| Cluster | Issues |
|---|---|
| Reliability & correctness | #157, #158, #159, #160, #99 |
| Zephyr module | #98, #146, #152, #153, #154, #155 |
| Hardware Rev 2 | #20, #21, #23, #24, #25 |
| Documentation & tooling | #79, #162 |
| Blocked / deferred | #12, #17 |

#12 (10-bit I2C) is confirmed blocked on two upstream items:
embassy-rs/embassy#5927 must land plus an embassy-rp release, and
postcard-rpc needs a dependency PR.

#161 is deliberately absent from this table. It is closed by this PR,
so listing it in the roadmap would ship a document referencing its own
completed authorship.

## 3. Decisions

Four forks were settled before writing this spec.

### D1 — Restructure into workstreams

Phases 1–3 collapse into a `Delivered` section. Live work is organised
by workstream, mirroring the clustering in §2.6.

Rejected: correcting in place, which leaves the live work homeless; and
a hybrid that keeps phase numbering alongside a "Current Focus"
section, which would run two organising schemes in one document.

The usual cost of restructuring — broken anchors — **does not apply
here**. A repository-wide search found no inbound links to any
`ROADMAP.md` anchor. The only references are AGENTS.md:34 (a filename
in a directory tree) and
`docs/superpowers/specs/2026-08-19-zephyr-mfd-m5-acceptance.md:741` (a
prose mention of "ROADMAP §1.6", addressed by §4.4 below).

### D2 — Drop volatile figures, cite their homes

The endpoint count and test count are removed from `ROADMAP.md`
entirely, replaced by pointers to their authoritative homes.

The reasoning is that `book/src/appendix/endpoints.md` already tracks
the endpoint list exactly and AGENTS.md §15.1 already *mandates* that
it stay in sync with `endpoints!`. Copying that count into
`ROADMAP.md` creates a third copy governed by nothing. This is the
mechanism that produced "298 unit tests" in the first place.

Rejected: annotating each figure with its derivation, which is #161's
literal deliverable #1 but still re-drifts; and adding a CI check,
which is real enforcement but lands new machinery inside a docs PR.

### D3 — Name the wedge class, link its detail

Workstream A opens with the dispatcher-wedge defect class, stated only
at the level that does not drift: the mechanism, and why the watchdog
does not catch it. Instances are listed as date plus one-line trigger
plus a link to AGENTS.md §13.17, which remains the authoritative log.

Rejected: a bare link, which makes a reader leave the document to learn
why the reliability issues matter; and a full inline treatment, which
would duplicate a log that grows each time a new instance is found —
the same failure mode D2 exists to prevent.

### D4 — Scope

`ROADMAP.md` plus the AGENTS.md §5.5 test-count correction.

§5.5 is not optional scope creep. D2 makes `ROADMAP.md` cite §5.5 as
the authoritative home for test counts, and §5.5 is wrong (§2.2).
Citing a stale number launders it.

Rejected: deferring §5.5 to a follow-up issue, which ships a document
knowingly pointing at a wrong figure; and additionally adding
`ROADMAP.md` to the §15.1 per-area mapping, which is defensible but
widens a docs PR into policy change.

## 4. Target structure

```
# Pico de Gallo — Roadmap to 1.0
> Last updated: 2026-08-31

  Purpose, plus an explicit non-goal: not a status dashboard.
  Figures that drift are cited, never copied.

## Table of Contents
## Where We Are Today
## Design Philosophy
## Delivered
## Active Workstreams
     A. Reliability & Correctness
     B. Zephyr Module
     C. Hardware Rev 2
     D. Documentation & Tooling
     E. Blocked / Deferred
## 1.0 Release Criteria
## Should the RP2350 Stay?
## Appendix A — embedded-hal Trait Coverage
## Appendix B — Competitive Landscape
## Appendix C — RP2350 Peripheral Budget
## Conventions — How to Update This File
```

### 4.1 Deletions

**The `Progress Overview` table is deleted, not corrected.** A
phase-completion count is exactly the hand-maintained figure D2 exists
to eliminate. Workstream tables list issue numbers instead; GitHub
renders their state, so they cannot go stale.

**The `### 1.1`–`### 3.6` prose subsections are deleted** — roughly 400
lines. That rationale describes shipped work, is preserved behind the
issue link in the `Delivered` table, and is duplicated by the book's
interface chapters.

Phase 4's prose subsections (4.1–4.6) are **kept** and moved into
Workstream C. That work is still open, so its rationale is live.

### 4.2 Where We Are Today

The capability table stays, qualitative only. The `Endpoints` and
`Tests` rows are replaced by a pointer block:

| Figure | Authoritative source |
|---|---|
| Endpoints and topics | `book/src/appendix/endpoints.md` |
| Test counts | AGENTS.md §5.5 |
| Crate versions | each crate's `Cargo.toml` `[package].version` |

The `Host crates` row becomes a **Components** table listing all nine
entries from §2.3 with purpose and release state.

`What's Missing` is retained and re-checked: 10-bit I2C (blocked
upstream, #12), voltage flexibility, target power.

### 4.3 Delivered

Three tables — Polish, Protocols, Advanced — with columns
`Item | Issue | Outcome`. Checkboxes are dropped; nothing there is
pending.

**#16 and #18 must read `Dropped`, not `Deferred`** (§1.2).

A fourth table, *Beyond the original phases*, captures work that never
had a roadmap row: `gallo-mcp` (#85), WebUSB descriptors (#87), Python
bindings (#30), `i2c/batch` atomicity (#128), the Zephyr module,
`hw-rev1`/`hw-rev2` gating, and `system/reset-subscriptions`.

### 4.4 Workstream A — Reliability & Correctness

Opens with the defect class:

> postcard-rpc dispatches handlers serially on one `&mut Context`. Any
> handler that fails to return blocks *every* endpoint, not just its own
> family — the failure is device-wide and needs USB re-enumeration or a
> power cycle. The 2 s watchdog does not catch it, because
> `watchdog_feeder_task` is an independent task and keeps feeding while
> the dispatcher is stuck.

Then three instances — 2026-06-03 GPIO wait, 2026-08-19 SPI framing,
2026-08-26 zero-length I2C write — as date plus one-line trigger plus a
link to AGENTS.md §13.17. No restated detail (D3).

Then the open issues: #157, #158, #159, #160, #99.

**The buried §1.6 M5 note relocates here.** The finding that
`MAX_TRANSFER_SIZE = 4096` is a packet-buffer budget rather than usable
payload, with a measured 1013-byte TX-only ceiling and full duplex
documented safe only at 512 bytes, is the context for #158. #161
complains that it sits inside a phase marked complete; moving it into
the live workstream is the fix. The M5 spec's prose reference to
"ROADMAP §1.6" (§3, D1) is resolved by this relocation.

### 4.5 1.0 Release Criteria

Reconciled against §2.6.

One must-have row needs resolving before the section can be written
honestly. *"All `embedded-hal` 1.0 sync + async traits implemented"*
is ambiguous. Its `Phase Item` column cites 1.2 and 2.2, so the intent
was `SpiDevice` and `SetDutyCycle`, both of which shipped. Read
literally, however, the row is **not met**: `I2c<TenBitAddress>` is
absent and blocked upstream (#12), and `ReadReady`/`WriteReady` are
absent (§2.5).

The implementer must reword this row to name the traits actually
required for 1.0, rather than leaving "all" to be read either way, and
list the excluded traits with their reason — which is what Appendix A's
closing line ("At 1.0: every cell should be ✅ or have a documented
reason for exclusion") already asks for.

Once that row is resolved, the reconciliation supports a conclusion the
document should state outright: **no must-have is waiting on a feature
that is both unimplemented and unblocked.** What remains is
wire-protocol stability, documentation, and the reliability workstream.
State it in those terms rather than as "all features are done", which
overclaims.

Required corrections:

| Row | Correction |
|---|---|
| `All embedded-hal 1.0 sync + async traits implemented` (must-have) | **reword** — see above; name the required traits and document the exclusions |
| `1-Wire via PIO` (nice-to-have) | **shipped**, #15 — currently listed as post-1.0 |
| `Protocol sniffing` (nice-to-have) | **dropped**, #16 |
| `Multi-device host support` (nice-to-have) | **dropped**, #18 |
| `10-bit I2C` (should-have) | **blocked upstream**, #12 — cite both blockers |
| `CLI app with bus scan, UART terminal, interactive GPIO` | **split into three rows.** `i2c scan` exists. There is no interactive UART terminal and no interactive GPIO mode — the CLI has `uart read/write/flush` and `gpio get/put/set-config/monitor`. |
| `Stable wire protocol` (must-have) | **not met** — schema is 0.7 and wire changes are still landing |

### 4.6 Appendices and conventions

- **Appendix A:** correct the two `embedded-io` rows to reflect eight
  impls across both majors, blocking and async (§2.5). All other rows
  are verified correct and stay.
- **Appendix B:** 1-Wire moves from a 1.0 target to shipped. Remove any
  implication that sniffing is planned.
- **Appendix C:** the PIO row reads "1–2 (1-Wire, sniffing)"; sniffing is
  dropped, so planned becomes 1.
- **Conventions:** rewritten for the workstream structure, and gains an
  explicit rule that hand-maintained counts must not be reintroduced,
  with a pointer to the §4.2 sources instead.

### 4.7 AGENTS.md

§5.5 only. Correct to 589 passing + 11 ignored + 7 doctests, with the
per-crate breakdown from §2.2, and correct the ignored-test description
to cover both `gallo-mcp` (7, two boards) and `pico-de-gallo-lib` (4,
one board, one needing a TMP102-like target).

## 5. Out of scope

- Adding `ROADMAP.md` to the AGENTS.md §15.1 per-area mapping (D4).
- Any CI check that verifies counts (D2).
- Any change to `book/`. `ROADMAP.md` has no book chapter and is absent
  from the §15.1 mapping, so the book-parity rule does not bind. The
  book's endpoint catalog is already in sync (§2.1).
- Any code, firmware, wire-protocol or version change. This is a
  documentation-only PR, so §4 rule 12 is not engaged.
- Resolving any of the open issues the roadmap will now reference.

## 6. Acceptance criteria

1. `ROADMAP.md` contains no hand-maintained endpoint count or test
   count; both are replaced by pointers per §4.2.
2. Every one of the nine components in §2.3 appears in the Components
   table.
3. #16 and #18 read `Dropped` everywhere they appear, including the
   1.0 criteria tables.
4. Workstream A states the wedge mechanism and links AGENTS.md §13.17
   without restating instance detail.
5. The relocated `MAX_TRANSFER_SIZE` note appears in Workstream A and
   no longer inside a section describing completed work.
6. Every issue in §2.6 appears in exactly one workstream.
7. Appendices A, B and C carry the corrections in §4.6.
8. The `All embedded-hal 1.0 sync + async traits implemented` must-have
   is reworded to name specific traits, and the exclusions
   (`TenBitAddress`, `ReadReady`, `WriteReady`) each carry a reason.
9. AGENTS.md §5.5 matches §2.2.
10. `> Last updated:` reads `2026-08-31`.
11. Both files are LF-terminated (AGENTS.md §3); run `dos2unix` on each.
12. No `ROADMAP.md` internal cross-reference points at a deleted anchor.

## 7. Commit plan

A single documentation commit:

```
docs(repo): Refresh ROADMAP.md around active workstreams

Closes #161
```

Body should record that Phases 1–3 collapsed into `Delivered`, that
#16 and #18 were dropped rather than deferred, that volatile counts now
cite their sources instead of copying them, and that AGENTS.md §5.5 was
corrected because the roadmap now cites it.

Trailers per AGENTS.md §10: `Assisted-by:` and
`Co-authored-by: Copilot`. No `Signed-off-by:`.
