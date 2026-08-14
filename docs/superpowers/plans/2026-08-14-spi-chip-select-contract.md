# SPI Chip-Select Contract Implementation Plan

> **For agentic workers:** This is a **milestone-level** plan. Per the
> maintainer's process, step-level design and TDD decomposition are produced by
> a fresh `@architect` at the start of each milestone — see §6. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop `spi/batch` from silently corrupting user-configured GPIO state,
make its three distinct CS failures diagnosable, and give the Zephyr devicetree
an honest chip-select contract.

**Architecture:** Bottom-up by layer. The wire crate (`pico-de-gallo-internal`)
lands the contract first; firmware and host surfaces then implement it in
parallel; the Zephyr module consumes the resulting FFI; docs land last. The
firmware fix is three guard clauses — no save/restore machinery — because
tracing all three pin modes showed only `ExplicitInput` corrupts.

**Tech Stack:** Rust (host workspace + `no_std` RP2350 firmware), postcard-rpc
over USB, cbindgen C FFI, Zephyr devicetree and device drivers, mdBook.

**Spec:** `docs/superpowers/specs/2026-08-14-spi-chip-select-contract-design.md`
(commit `5b259817e863`). The spec is authoritative; this plan sequences it.

**Issue:** [#104](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/104)

---

## 1. Dependency graph and parallelism

```
M1 (wire) ──┬──> M2 (firmware) ──┐
            │                    ├──> M5 (docs sweep + CHANGELOG)
            └──> M3 (host) ──> M4 (Zephyr) ──┘
```

| Pair | Relationship |
| --- | --- |
| M2 **before** M3 | **Revised after M1 — see §8.2.** Originally planned as parallel. Two reasons forced a sequence: they share one working tree, so concurrent `@coder` edits and `git add` would interleave; and M3's host-side bound check would make M2's hardware tests 4 and 5 *unreachable* by rejecting the request locally before it ever reaches the firmware guard. |
| M4 after M3 | M4 needs the FFI surface M3 adds (`num_gpios` accessor). The three `Status` codes M4 maps to errno already exist — M1 added them; see §8.3. |
| M4 hardware test after M2 | M4 compiles without M2, but running a sample against a board needs M2's firmware flashed. |
| M5 last | It records final state. |

**Commits are serialized.** With M2→M3→M4→M5 strictly sequential this is
automatic. Do not reintroduce parallelism without giving each coordinator its own
git worktree.

---

## 2. Operational hazards

These are verified facts about *this* workstation, not hypotheticals. Every
milestone coordinator must read them.

### 2.1 Flashing M2 firmware can HANG the `gallo` MCP tools indefinitely

**Revised after M1.** The original text here said "at best it silently ignores
the field; at worst `postcard::from_bytes` fails outright," and told readers not
to *trust* the MCP tools. M1's `@reliability` pass read postcard 1.1.3 and
postcard-rpc 0.12.1 from source and found the real behaviour is worse and
asymmetric:

| Direction | Behaviour |
| --- | --- |
| new host ← old firmware | `DeserializeUnexpectedEnd` — a clean, loud failure |
| old host ← new firmware | **silently succeeds**, trailing byte ignored |

Worse: postcard-rpc response keys hash the response *schema*. An old host may
therefore never match the new response at all, and `send_resp` has **no
timeout** — it waits forever while holding the USB interface. On Windows, WinUSB
grants exclusive interface access per session (AGENTS.md §13.17, 2026-07-20),
so a hung MCP call also locks out the branch-built CLI.

The attached board reports `fw 0.10.1, schema 0.6.1, hw_version 2`. The `gallo`
MCP server here is a pre-built binary compiled against the *old* eight-field
`DeviceInfo`. Because D11 forbids a version bump, `validate()` cannot catch the
mismatch — both sides still claim schema 0.6.1.

**Hard rule, from the moment M2 firmware is flashed until the maintainer's
release bump: do not invoke any `gallo_*` MCP tool.** Not "don't trust it" —
do not call it. A single call may hang unrecoverably and hold the interface.

Use the branch-built CLI for all verification:

```bash
cargo run -p gallo --locked -- <subcommand>
```

That binary carries M1's types and decodes correctly.

**Consequence:** after M2 flashes, do not trust `gallo_*` MCP tools for
verification. Build the CLI from the branch instead:

```bash
cargo run -p gallo --locked -- <subcommand>
```

That binary carries M1's types and decodes correctly. This is the concrete
instance of the "mixed-version mis-decode" risk in spec §5.

### 2.2 The board is hw-rev2; the firmware default is hw-rev1

`crates/pico-de-gallo-firmware/Cargo.toml:14` sets `default = ["hw-rev1"]`, but
the attached board reports `hw_version: 2`. Every build intended for **this**
board must pass:

```bash
--no-default-features --features hw-rev2
```

Building with defaults and flashing it will misconfigure the board. CI
(`nostd.yml`) builds both revisions, so both must still compile — but only the
`hw-rev2` artifact may be flashed here.

### 2.3 Pre-existing LSP noise in `zephyr/drivers/spi/*.c`

`pdg_spi.c` and `pdg_spi_bottom.c` report dozens of clangd errors
(`'zephyr/device.h' file not found`, `'pico_de_gallo.h' file not found`). These
are **pre-existing and expected** — those headers only exist inside a Zephyr
build tree with the FFI staticlib generated. Do not "fix" them. Do not treat
them as a regression introduced by M4.

---

## 3. File inventory

Locked-in decomposition. Anything not listed is out of scope; adding a file
requires the milestone `@architect` to justify it to `@reviewer`.

**Explicitly forbidden, all milestones (D12):** do not claim `p.PIN_5`, do not
change `NUM_GPIOS`, do not touch `crates/pico-de-gallo-firmware/src/main.rs`'s
`gpios[]` array. That is issue #99, which has unresolved design questions of its
own. D8's `num_gpios` field exists precisely so #99 can be a firmware-only
follow-up later. A milestone that grows a fifth GPIO has escaped its scope.

### M1 — wire protocol

| Action | Path | Responsibility |
| --- | --- | --- |
| Modify | `crates/pico-de-gallo-internal/src/lib.rs:419-433` | Append three `SpiError` variants + `Display` arms |
| Modify | `crates/pico-de-gallo-internal/src/lib.rs:1508` | Append `num_gpios: u8` to `DeviceInfo` |
| Modify | `crates/pico-de-gallo-internal/src/lib.rs:1329-1332` | `cs_pin` doc: side effects + refusal contract |
| Modify | `crates/pico-de-gallo-internal/src/lib.rs:467-477` | `NUM_GPIOS` doc: compile-time default vs runtime authority |
| Modify | `crates/pico-de-gallo-internal/src/lib.rs` (`mod tests`) | Round-trip + variant-index pinning tests |
| Modify | `book/src/internals/wire-protocol.md` | New variants, new `DeviceInfo` field |
| Modify | `book/src/appendix/endpoints.md` | `spi/batch` CS semantics |

### M2 — firmware

| Action | Path | Responsibility |
| --- | --- | --- |
| Modify | `crates/pico-de-gallo-firmware/src/handlers/spi.rs:125-139` | Three guard clauses before CS drive |
| Modify | `crates/pico-de-gallo-firmware/src/handlers/` (device-info site) | Report `num_gpios: NUM_GPIOS as u8` |
| Modify | `book/src/internals/firmware.md` | CS guard behaviour |
| Modify | `book/src/interfaces/spi.md` | CS side-effect contract |

### M3 — host surfaces

| Action | Path | Responsibility |
| --- | --- | --- |
| Modify | `crates/pico-de-gallo-lib/src/lib.rs:461-485` | Pre-flight `cs_pin` bound check; `num_gpios` accessor |
| Modify | `crates/pico-de-gallo-ffi/src/lib.rs:92` (`Status`) | Append three status codes |
| Modify | `crates/pico-de-gallo-ffi/src/lib.rs:1169-1241` | `gallo_spi_batch` bound check |
| Modify | `crates/pico-de-gallo-hal/src/` | Validate at `spi_device(cs_pin)` |
| Modify | `crates/pico-de-gallo-app/src/` | Validate `--cs` |
| Modify | `crates/pico-de-gallo-mcp/src/` | Validate `cs_pin` tool argument |
| Modify | `crates/pyco-de-gallo/src/` | Validate `cs_pin` argument |
| Modify | `book/src/crates/{lib,ffi,hal,app,mcp,python}.md`, `book/src/appendix/status-codes.md` | Match the code |

### M4 — Zephyr module

| Action | Path | Responsibility |
| --- | --- | --- |
| Modify | `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml` | `cs-gpio-indices` + contract description |
| Modify | `zephyr/drivers/spi/pdg_spi.c:206,280` | Index mapping; replace the `3U` literal |
| Modify | `zephyr/drivers/spi/pdg_spi_bottom.{c,h}` | `pdg_spi_bottom_num_gpios()`, cached at init |
| Modify | `zephyr/drivers/common/common.c` | Map three new statuses to errno |
| Modify | `zephyr/samples/{spi_bridge,spi_nor_id,combined_i2c_spi_bridge}/app.overlay` | Declare `cs-gpio-indices` |
| Modify | `zephyr/README.md:271-300` | Rewrite the CS section |

### M5 — docs sweep

| Action | Path | Responsibility |
| --- | --- | --- |
| Modify | `book/src/hardware/pinout.md:18,79` | GPIO5 is unclaimed by firmware |
| Modify | `book/src/interfaces/spi.md:11-18` | User-GPIO CS is not v1.0-only |
| Modify | `CHANGELOG.md` | Keep a Changelog entries |

---

## 4. Milestones

### M1 — Wire protocol

- [ ] Coordinator dispatched; §6 pipeline run to completion
- [ ] Three `SpiError` variants appended **after** `Other`, never inserted (D9)
- [ ] `num_gpios: u8` appended as `DeviceInfo`'s **last** field; fields stay public
- [ ] `cs_pin` and `NUM_GPIOS` rustdoc updated per spec §M1
- [ ] Round-trip tests for the new shape and each new variant
- [ ] Variant-index pinning test: `BufferTooLong == 0`, `Other == 1`
- [ ] Book chapters updated in the same commit
- [ ] Committed

**Verify:**

```bash
cd crates/pico-de-gallo-internal
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked --features use-std
```

`--features use-std` is mandatory here — AGENTS.md §13.14: bare `cargo test` in
this crate fails on the `vec!` macro under `#![no_std]`.

```bash
cd D:/workspace/pico-de-gallo
cargo check --workspace --locked
mdbook build book
```

**Expected failures — do not "fix" these:**
- CI `semver` goes red (`enum_variant_added`, `constructible_struct_adds_field`).
  Intended per D11; `@integrator` records the rationale in the commit body.
- Nothing else should break yet: `SpiError` is matched exhaustively downstream,
  so `cargo check --workspace` may surface those sites now. If it does, they are
  M3's and M4's work — M1 may add the arms only if the workspace will not
  otherwise compile, and must map each variant to a *distinct* status, never a
  wildcard.

### M2 — Firmware

Runs concurrently with M3.

- [ ] Coordinator dispatched; §6 pipeline run to completion
- [ ] Guards in `spi_batch_handler`, in this order: `InvalidCsPin`, then
      `CsPinMonitored`, then `CsPinUnavailable` (D2 — refuse only
      `ExplicitInput`; `LegacyAuto` and `ExplicitOutput` must keep working)
- [ ] `pin_modes` **not** written (D4); pin level **not** restored (D3)
- [ ] `device/info` reports `num_gpios`
- [ ] `@architect` has confirmed against `embassy-rp` whether
      `Flex::set_as_output()` disturbs the pull set by `gpio/set-config`, and
      documented the answer either way
- [ ] Book chapters updated in the same commit
- [ ] Committed

**Verify — both revisions must compile (CI `nostd.yml` builds both):**

```bash
cd crates/pico-de-gallo-firmware
cargo fmt --check

# hw-rev1 (CI default)
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf

# hw-rev2 (this board — the only artifact that may be flashed here)
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2
```

**Hardware verification** on board `5256657D8A5D7F03`. Read §2.1 first — use the
branch-built CLI, not the MCP tools.

| # | Setup | Action | Expected |
| --- | --- | --- | --- |
| 1 | `gpio/set-config{0, Input, Up}` | `spi/batch{cs:0}` | `CsPinUnavailable`; `gpio/get{0}` still reads the external signal |
| 2 | `gpio/set-config{0, Output}` | `spi/batch{cs:0}` | succeeds; pin left high |
| 3 | fresh boot (`LegacyAuto`) | `spi/batch{cs:0}` | succeeds; `gpio/get{0}` afterwards still works |
| 4 | — | `spi/batch{cs:4}` | `InvalidCsPin`, **not** `Other` |
| 5 | `gpio/subscribe{0}` | `spi/batch{cs:0}` | `CsPinMonitored`, **not** `Other` |

Test 1 is the decisive #104 regression test. A milestone that cannot demonstrate
test 1 is not complete.

### M3 — Host surfaces

Runs concurrently with M2.

- [ ] Coordinator dispatched; §6 pipeline run to completion
- [ ] `@architect` has decided where `num_gpios` comes from — cached at
      `validate()` (`lib.rs:855`) or re-fetched — and justified it
- [ ] `spi_batch` bound check in `lib`, `ffi`, `hal`, `app`, `mcp`, `pyco`
- [ ] **Three distinct** `Status` codes appended, one per new `SpiError` variant
- [ ] No wildcard arm added to any `SpiError` match (spec §2.1)
- [ ] `num_gpios` accessor exposed
- [ ] Book chapters + `status-codes.md` updated in the same commit
- [ ] Committed

**Verify, per crate — this is what CI gates on:**

```bash
foreach ($c in 'pico-de-gallo-lib','pico-de-gallo-ffi','pico-de-gallo-hal',
               'pico-de-gallo-app','pico-de-gallo-mcp','pyco-de-gallo') {
  Push-Location "crates/$c"
  cargo fmt --check
  cargo clippy --all-targets --locked -- -D warnings
  cargo test --locked
  Pop-Location
}
```

Seven `gallo-mcp` tests are `#[ignore]`d because they need two attached boards
(AGENTS.md §5.5). Only one board is attached — leave them ignored; do not
delete them.

### M4 — Zephyr module

Depends on M3. Hardware testing additionally depends on M2.

- [ ] Coordinator dispatched; §6 pipeline run to completion
- [ ] `cs-gpio-indices` added to the binding with a description that makes the
      contract inferable **without** reading `zephyr/README.md` (D5)
- [ ] Missing property → `-EINVAL` with a log line naming the fix (D6). **No
      identity fallback.**
- [ ] `slave >= len` → `-EINVAL`; mapped index bounded against firmware-reported
      `num_gpios`; the `3U` literal at `pdg_spi.c:206` is gone
- [ ] `pdg_spi_bottom_num_gpios()` caches at init, not per transceive
- [ ] Three new statuses mapped to errno in `common.c`
- [ ] All three sample overlays declare `cs-gpio-indices`
- [ ] `zephyr/README.md` CS section rewritten
- [ ] Committed

**Verify:** all three samples build. Do not treat §2.3's pre-existing clangd
errors as a regression.

### M5 — Docs parity sweep and CHANGELOG

- [ ] Coordinator dispatched; §6 pipeline run to completion
- [ ] `pinout.md:18,79` no longer claims GPIO5 is a firmware-driven `SPI0 CSn`
- [ ] `interfaces/spi.md:11-18` no longer frames user-GPIO CS as a v1.0 workaround
- [ ] `CHANGELOG.md` entries for wire, firmware, host, and Zephyr changes
- [ ] Committed

**Verify:**

```bash
mdbook build book
```

Then re-run the AGENTS.md §15.1 reviewer checklist across the whole branch:
endpoint table, status-code table, and wire-enum tables must match source.

---

## 5. Commit protocol

- Branch `zephyr`. **Never push.** No `--force`, no rebase of existing commits.
- **Serialize all commits.** When M2 and M3 run concurrently, only one
  `@integrator` may hold the working tree. The second waits for the first to
  report a clean `git status`.
- One logical change per commit; each commit builds on its own (AGENTS.md §13.12).
- Conventional Commits with crate scope. Expected subjects:
  - M1 `feat(internal)!: ...`
  - M2 `fix(firmware): ...`
  - M3 `feat(lib,ffi,hal,application,mcp,pyco): ...`
  - M4 `fix(zephyr): ...`
  - M5 `docs(repo): ...`
- Trailers on **every** commit, exactly:
  ```
  Assisted-by: OpenCode:claude-opus-5
  Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
  ```
  **Never** `Signed-off-by:` — DCO is for humans (AGENTS.md §4 rule #7).
- **No `[package].version` edits** (D11). The maintainer bumps before the PR.
- Every file created or touched on Windows: `dos2unix <file>` before staging
  (AGENTS.md §3). PowerShell has no heredoc — write commit messages to a file
  and use `git commit -F`.

---

## 6. Per-milestone agent pipeline

One **fresh** `@coordinator` per milestone. Sessions are ephemeral; never reuse
one across milestones. Each coordinator runs:

1. `@architect` — architecture and specification for this milestone
2. **fresh** `@architect` — implementation plan (step-level TDD detail)
3. `@reviewer` **and** `@reliability` **in parallel** — re-spawn `@architect` if
   the spec or plan needs amendment
4. `@tester` — adversarial tests
5. `@coder` — implementation
6. `@integrator` — commit (serialized; see §5)
7. `@reviewer` — spec-compliance review

Every coordinator must be handed: this plan, the spec
(`docs/superpowers/specs/2026-08-14-spi-chip-select-contract-design.md`),
`AGENTS.md`, and §2's operational hazards.

**Standing instruction to `@reviewer`, all milestones:** reject any wildcard
`_ =>` arm added to a `SpiError` match, and reject any `#[non_exhaustive]`
attribute on a wire type (D10). Spec §2.1 makes exhaustive matching a deliberate
feature — it is what forces each surface to decide how a new error is reported.
Silencing it reintroduces the exact defect this issue is about.

**Standing instruction to every `@architect`:** this plan (§1 goal, §3 inventory)
and the spec are the scope boundary. Milestone scope may not grow to include
issue #99 (D12), a `[package].version` bump (D11), full `cs-gpios` support (D7),
or CS pin level/pull restoration (D3).

---

## 7. Definition of done

These are **branch-level** gates, evaluated after M5 — not per-milestone gates.
M1 in particular cannot satisfy the both-workspaces item on its own; see §8.1.

- [ ] M1–M5 committed on `zephyr`, nothing pushed
- [ ] `cargo test --locked` green across the host workspace
- [ ] Firmware builds clean for `hw-rev1` **and** `hw-rev2`
- [ ] M2 hardware tests 1–5 pass on `5256657D8A5D7F03`, test 1 demonstrated
- [ ] `mdbook build book` clean
- [ ] `cargo check --locked` green in **both** workspaces (host and firmware)
- [ ] CI `semver` red, with the reason recorded in the M1 commit body — the only
      accepted red check
- [ ] No `[package].version` modified anywhere

---

## 8. M1 outcome and plan corrections

M1 landed as `8fd06483b206` — `feat(internal,lib,ffi)!: Define the SPI
chip-select contract`. Independently verified: host workspace green, 159 unit
tests + 1 doctest pass, clean tree, nothing pushed, no `Cargo.toml` or
`Cargo.lock` touched, `NUM_GPIOS` still `4`.

The milestone surfaced five things that change what later milestones must do.
They are recorded here because each coordinator reads only this document.

### 8.1 The firmware workspace does not compile at M1

`crates/pico-de-gallo-firmware/src/handlers/info.rs:42` constructs `DeviceInfo`
with a struct literal and now fails `E0063: missing field num_gpios`. Confirmed
by direct build.

This is not a defect in M1 — the firmware is a separate Cargo workspace, M1's
gate was `cargo check --workspace` (host only), and §3 already assigns that site
to M2. But it does mean §7's "green in both workspaces" is a branch-level gate,
not something M1 could ever have met.

**M2's first task is the one-line repair:** add `num_gpios: NUM_GPIOS as u8` to
that struct literal, before anything else. Everything M2 does afterwards is
blocked behind a compiling firmware workspace.

### 8.2 M2 and M3 must run in sequence, M2 first

Two independent reasons, either sufficient:

1. **Shared working tree.** Concurrent `@coder` sessions editing and staging in
   one checkout will interleave; `git add -u` from either would sweep the
   other's half-finished work into the wrong commit.
2. **M3 would make M2's tests unreachable.** M3 adds a host-side `cs_pin <
   num_gpios` pre-flight check. M2's hardware test 4 (`spi/batch{cs:4}` →
   `InvalidCsPin`) and test 5 depend on the request actually reaching the
   firmware. If M3 lands first, the CLI rejects `cs=4` locally and the firmware
   guard is never exercised — the defence-in-depth layer would ship unverified.

M2 therefore verifies its guards against a host that has no bound checks yet
(the state at `8fd06483b206`), and M3 adds the host layer afterwards.

### 8.3 M3's scope is reduced — M1 already did part of it

Appending to `SpiError` broke two exhaustive matches in the FFI, and repo policy
requires every commit to build on its own, so M1 carried the minimum repair.
**Already done, do not redo, do not renumber:**

| Item | State after M1 |
| --- | --- |
| `Status::SpiInvalidCsPin = -71` | added |
| `Status::SpiCsPinUnavailable = -72` | added |
| `Status::SpiCsPinMonitored = -73` | added |
| `spi_error_to_status` (`ffi/src/lib.rs:390`) | three explicit arms, no wildcard |
| `gallo_spi_transfer` (`ffi/src/lib.rs:1092`) | three explicit arms, same mapping |
| `book/src/appendix/status-codes.md` | three new rows, plus the pre-existing missing `GpioTimeout = -70` |
| `make_device_info` (`lib/src/lib.rs:1327`) | `num_gpios` added — test helper only |

Status-code injectivity is now test-enforced (`spi_error_to_status_is_injective`).

**M3 still owns:** the `cs_pin` bound checks in `lib`, `ffi`, `hal`, `app`,
`mcp`, `pyco`; the `num_gpios` accessor; and all book chapters under
`book/src/crates/`.

**Binding constraint on M3.** When `device/info` fails, the error must stay a
communications error. Never `unwrap_or(0)`, never `Default`, never silently fall
back to the compile-time `NUM_GPIOS`. A decode failure must not be reported as
`InvalidCsPin`. If a device genuinely reports `num_gpios == 0`, rejecting every
CS is fail-*safe* — no pin is driven — but it must be diagnosable as such.

### 8.4 `book/src/interfaces/spi.md` is contended by three milestones

M1 already edited it, scoped strictly to the error taxonomy; it also removed a
documented `SpiError::Unsupported` that **does not exist in source** — a
pre-existing §15.1 violation. Lines 11–18 were deliberately left untouched for
M5. M2 must edit only the CS side-effect contract. M5 owns lines 11–18.

### 8.5 Pre-existing defects found, for M5

- `> [!CAUTION]` and friends render **literally**: `book.toml` has no admonition
  preprocessor and CI runs plain mdBook 0.5.2. 46 instances book-wide. M1 used a
  plain `> **Warning**` to avoid adding a 47th.
- `book/src/appendix/endpoints.md` has no trailing newline.
- AGENTS.md §15.1 checklist item 5 wants a `book/src/internals/releases.md`
  mention for wire changes. D11 forbids the version bump that would give it a
  number. M1 judged the `wire-protocol.md` warning sufficient; M5 should confirm
  or add one.
