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

Runs BEFORE M3 — see §8.2. (The original `Runs concurrently` was superseded.)

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
branch-built CLI, not the MCP tools. **The test table below was rewritten after
M2 found the original could not fail; see §8.6 for the full corrected procedure
and the reasoning.**

**Required jumper:** header pin 11 (GPIO0, `PIN_8`) to header pin 12 (GPIO1,
`PIN_9`). GPIO1 is a witness for what GPIO0 is physically doing.

| # | Setup | Action | Expected |
| --- | --- | --- | --- |
| 1 | pins 0 and 1 both `Input`/**`Down`**; witness reads `Low` | `spi/batch{cs:0}` | `CsPinUnavailable` **and** witness still `Low` — either pin `High` is a failure |
| 2 | pin 1 `Input`/`Down`; pin 0 `Output`, driven low | `spi/batch{cs:0}` | succeeds; witness transitions to `High` |
| 3 | fresh boot (`LegacyAuto`); pin 1 `Input`/`Down` | `spi/batch{cs:0}` | succeeds; witness `High`; pin 0 still usable both directions |
| 4 | — | `spi/batch{cs:4}` | `InvalidCsPin`, **not** `Other`; device stays responsive |
| 5 | `gpio/subscribe{0}`, host killed so the subscription is **orphaned** | `spi/batch{cs:0}` | `CsPinMonitored`, **not** `Other` |

Test 1 is the decisive #104 regression test. A milestone that cannot demonstrate
test 1 is not complete.

### M3 — Host surfaces

Runs AFTER M2 — see §8.2. (The original `Runs concurrently` was superseded.)

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

### 8.6 M2 outcome — and the test that could not fail

M2 landed as `9b2cdb0fb7c1` — `fix(firmware): Validate the SPI chip-select pin
before driving it`. Independently verified: guards present in the specified
order, every accessor fallible, **both pre-existing `.unwrap()` calls on the CS
path removed**, `SpiError::Other` at the acquisition site replaced with
`CsPinMonitored`, both revisions build clean, `mdbook build` clean, four files
changed, no `Cargo.toml`/`Cargo.lock`/`main.rs` touched.

**The most important finding of the milestone is that plan and spec test 1 was
broken.** It specified `gpio/set-config{0, Input, Up}` and expected `gpio/get{0}`
to still read the external signal. A healthy pull-up input reads `High`; a pin
corrupted into a driven output also reads `High`. And `gpio_for_input!`
(`handlers/gpio.rs:31-35`) has `PinMode::ExplicitInput => {}` — a genuine no-op
that does not re-assert `set_as_input()` — so the read returns the firmware's own
stale drive. **The decisive #104 regression test would have passed against
unfixed firmware.** Corrected in §4 M2 and spec §3 M2: `Pull::Down` plus a
GPIO0↔GPIO1 jumper witness.

Test 2's original expectation was also unobservable: after `set-config output`
every read path returns `WrongDirection` (`gpio.rs:34`), so nothing can report an
`ExplicitOutput` pin's level. The witness solves it.

### 8.7 Executing the hardware tests

- Build once: `cargo build -p gallo --locked`. Never run two `gallo` processes
  concurrently — WinUSB exclusivity yields `Access is denied`, which is not a
  test result (AGENTS.md §13.17, 2026-07-20).
- The CLI renders errors with `{:?}` (`app/src/lib.rs:986`), so expect
  `Endpoint(SpiBatchError { failed_op: 0, kind: <Variant> })` on stderr, exit 1.
  Match the payload, not the `color_eyre` framing.
- Power-cycle between tests 1, 2 and 3 — `pin_modes` has no reset path.
- Test 4: `--op` is `required = true`, so pass one. `--cs` is an unvalidated
  `u8` today, which is exactly why test 4 must run **before** M3.
- Test 5: kill the monitor process hard. Ctrl+C is trapped
  (`app/src/lib.rs:1063-1071`) and unsubscribes gracefully. Cross-check with
  `gpio get --pin 0` first — it must return `PinMonitored`; if it prints a level,
  the subscription was not orphaned and the batch result is meaningless.
- Worthwhile extras: `--cs 255` (catches future `as u8` truncation — `255 & 3`
  would drive GPIO3); `--cs 3` on fresh boot (upper boundary, distinguishes `>`
  from `>=`); re-run test 1's batch (idempotence — a refusal must not mutate
  `pin_modes`); `gpio put --pin 0` after test 1 (must be `WrongDirection`,
  proving `pin_modes[0]` untouched, D4).

### 8.8 Corrections to my own briefing, and to AGENTS.md

- **I was wrong about `elf2uf2-rs`.** I told M2 the installed binary was the
  stale crates.io 2.2.0 with no `--family`. It *reports* 2.2.0 but
  `cargo install --list` shows `elf2uf2-rs v2.2.0
  (https://github.com/JoNil/elf2uf2-rs#f14bf2d9)` — the git build AGENTS.md
  §13.9 prescribes — and it does have `--family rp2350-arm-s`. Version output
  alone cannot distinguish the two binaries. **AGENTS.md §13.9 should say to
  check `cargo install --list`.** `picotool` is genuinely absent and was not
  needed.
- **AGENTS.md §13.17 (2026-05-29 row) is wrong about `system/reset-subscriptions`.**
  It says "host calls it after `validate()`". Only `gallo-mcp` does
  (`mcp/src/lib.rs:435`); `pico-de-gallo-lib` merely exposes it and the `gallo`
  CLI never calls it. This is load-bearing: it is why an orphaned subscription
  survives across CLI invocations, and therefore why test 5 is executable.

### 8.9 Additional constraints on M3

- **The CLI prints `Debug`, not `Display`** (`app/src/lib.rs:986` uses
  `eyre!("{:?}", e)`). M1 wrote three `Display` arms that are unreachable
  end-to-end. If M3 fixes this, **every expected-output string in §8.7 changes** —
  land both in the same commit and re-derive the strings.
- `SpiBatchError::failed_op` overloads two meanings: "refusal, no operation
  attempted" and "operation 0 failed" are byte-identical on the wire. A rustdoc
  note costs nothing.

### 8.10 Pre-existing hazards found by M2, for M5 or their own issues

None are M2 regressions; all were found while reviewing adjacent code.

- **`DelayNs` is a stuck-CS amplifier.** `ns` is `u32` (~4.29 s) × `MAX_BATCH_OPS`
  64 ≈ **275 s of CS held low**, starving every other endpoint under serial
  dispatch. **The watchdog does not catch it** — `Timer::after` yields, so
  `watchdog_feeder_task` keeps feeding. A bounded total transaction duration is a
  wire/API decision, not a drive-by fix.
- **Cancellation is not CS-safe.** If the handler future is dropped while CS is
  low, deassertion is ordinary post-await code and never runs. Not reachable
  today (`Server::run` awaits dispatch directly), but any future
  `select(handler, disconnect)` makes it live.
- **A watchdog reset leaves CS floating**, not high: `Flex::new` sets the GPIO
  function but not output-enable, and clears pulls.
- **`set_as_output()` before `set_high()` can glitch CS low.** On a `LegacyAuto`
  pin the remembered level defaults low. Pre-existing and preserved deliberately;
  a one-line reordering would fix it.
- **One `.unwrap()` remains** in `spi_batch_execute` (`spi.rs:209`) plus
  unchecked slicing at `:215`/`:230`. Off the CS path, but the §13.17
  dispatcher-wedge precedent makes it worth a look.
### 8.11 RP2350 pull-downs are not usable as a test witness

**Third correction to the test design.** §8.6 replaced `Pull::Up` with
`Pull::Down` plus a jumper witness. That is still not sufficient, for a reason
that is in the silicon rather than the code.

Measured on board `5256657D8A5D7F03` (hw rev 2, RP2350), same node, controlled
A/B:

| Starting state | Pull applied | Result |
| --- | --- | --- |
| node LOW | pull-**up** | rises to HIGH — pull-up works |
| node HIGH | pull-**down** | **stays HIGH** — pull-down cannot pull it down |
| node driven LOW, released to pull-**down** | — | holds LOW correctly |
| node driven LOW, released to pull-**none** | — | drifts HIGH within seconds |

**An RP2350 internal pull-down can *hold* a low node low, but cannot *pull down*
a node that is already high, and a floating pad drifts high.** A pad that was
ever driven high — including by the very #104 bug under test — therefore reads
HIGH indefinitely regardless of the configured pull.

Consequences:

1. **Any test that configures a pull-down and expects LOW without first forcing
   the node low is invalid.** Both the original (§4 M2, pre-8.6) and the first
   correction (§8.6) had this defect.
2. **GPIO0 and GPIO1 on this board read HIGH against pull-downs** across four
   independent observations, with and without a jumper. They are almost
   certainly latched high from earlier work — plausibly by #104 itself, since
   `spi_nor_id`'s overlay drives CS on index 0. They are unusable as witnesses
   until a power cycle. GPIO2 and GPIO3 were verified clean and are used instead.
   The guards are index-generic, so `gpios[2]` exercises the identical path.

**Corrected witness protocol — pre-charge, then hold:**

1. Witness pin → `output`, `put low`. This *forces* the shared node low; driving
   works even though pulling does not.
2. Witness pin → `input`, pull **down** (not `none` — `none` drifts high).
3. Victim pin → `input`, pull **down**. This is the `ExplicitInput` state under
   test.
4. **Verify the baseline reads LOW on both pins.** If either reads HIGH, stop —
   the setup is invalid and any subsequent result is meaningless. Both earlier
   attempts failed here and would have been reported as passes.
5. Act (`spi/batch`), then read the **witness** first. The witness is never named
   as CS, so a LOW→HIGH transition on it is independent evidence that something
   began driving the node.

Validate the jumper itself before trusting it: drive one pin low and confirm the
other reads low *against its own pull-up*. Only a driven output can do that.

### 8.12 #104 reproduced on hardware, before the fix

Run against firmware 0.10.1 (pre-M2) using the protocol above, CS index 2,
witness index 3:

| Step | Action | Observed |
| --- | --- | --- |
| 1 | `gpio/set-config{2, Input, Down}` | ok — pin 2 is `ExplicitInput` |
| 2 | baseline | pin 2 **LOW**, witness **LOW** |
| 3 | `spi/batch{cs:2, [write 0x00]}` | **succeeded, no error** |
| 4 | `gpio/get{3}` (witness, never named as CS) | **HIGH** |
| 5 | `gpio/get{2}` | **HIGH, no error** |
| 6 | `gpio/put{2}` | **`WrongDirection`** |

Steps 4–6 are the divergence spec §1.2 predicted: the hardware is an output, the
tracked mode still says input, and no call reports it. Step 4 is the load-bearing
one — the witness is not the CS pin, so only an external drive explains it.

This is the "before" half of the acceptance evidence. The "after" half re-runs
the identical sequence on M2 firmware and must show `CsPinUnavailable` at step 3
with the witness still LOW at step 4.
### 8.13 M2 hardware acceptance — PASSED

Executed on board `5256657D8A5D7F03` (hw rev 2) after flashing
`pico-de-gallo-firmware-hw-rev2.uf2`, using the branch-built CLI
(`cargo run -p gallo --locked`). No `gallo_*` MCP tool was used after flashing.

The flash itself is confirmed by decode, not by version string: D11 means the
version is unchanged at `FW v0.10.1 / Schema v0.6.1`, but the branch CLI expects
a nine-field `DeviceInfo` and would fail `DeserializeUnexpectedEnd` against the
old firmware. It decoded cleanly, so the new firmware is running.

**Before / after on identical hardware, CS index 2, witness index 3:**

| Observation | Before (pre-M2) | After (M2) |
| --- | --- | --- |
| `spi/batch{cs:2}` on an `ExplicitInput` pin | **succeeded silently** | **`CsPinUnavailable`**, exit 1 |
| witness pin 3 (never named as CS) | LOW → **HIGH** | LOW → **LOW** |
| `gpio/get{2}` | **HIGH**, no error | **LOW** |
| `gpio/put{2}` | **`WrongDirection`** | n/a — pin never corrupted |

**Full results:**

| # | Test | Expected | Result |
| --- | --- | --- | --- |
| 1 | `ExplicitInput` CS | `CsPinUnavailable`, pin untouched | **PASS** — witness stayed LOW |
| 2 | `ExplicitOutput` CS | succeeds, CS parked high | **PASS** — witness LOW → HIGH |
| 3 | `LegacyAuto` CS | succeeds, pin still usable | **PASS** — `gpio/get{0}` returned Ok |
| 4 | `--cs 4` | `InvalidCsPin`, not `Other` | **PASS**, device stayed responsive |
| 5 | orphaned subscription | `CsPinMonitored`, not `Other` | **PASS** — cross-check gave `PinMonitored` first |
| E1 | `--cs 255` | `InvalidCsPin`, no truncation | **PASS** — did not alias to index 3 |
| E2 | `--cs 3` upper boundary | succeeds | **PASS** — guard is `>= NUM_GPIOS`, not off by one |
| E4 | re-run a refused batch | same refusal, no state change | **PASS** — refusal is idempotent |

Two results carry more weight than the rest. Test 1's witness is not the
chip-select pin, so only an external drive explains a LOW→HIGH transition;
its absence after M2 is direct evidence the pin was never touched. And test 3's
`gpio/get{0}` returning `Ok` rather than `WrongDirection` proves `pin_modes` was
not written, confirming **D4** on hardware rather than by inspection.

Test 2 proves the guard is not simply refusing everything: an `ExplicitOutput`
pin is accepted and genuinely driven, confirming **D2**.

**Left in a dirty state:** pin 2 still carries the orphaned subscription from
test 5, and pin 3 is `ExplicitOutput` parked high. Neither has a software reset
path — power-cycle the board before further hardware work.

### 8.14 Pre-existing CLI bug found during acceptance — for M3

**`gallo gpio put` panics on every invocation and cannot be used at all.**

```
The application panicked (crashed).
Message:  Command put: Short option names must be unique for each argument,
          but '-h' is in use by both 'high' and 'help'
```

`crates/pico-de-gallo-app/src/lib.rs:378` declares `#[arg(short, long)] high: bool`.
`short` derives `-h` from `high`, colliding with clap's auto-generated `-h` for
`--help`. It is a builder assertion, so it fires before any parsing.

Confirmed **pre-existing**: identical code is present at the merge base
`d201e7fb8240`, and the most recent commits touching that file (`74bb845eff77`,
`33f8ff3e4810`) predate this work. Not introduced by M1 or M2.

A second defect is stacked on it: `high: bool` in clap derive is a **flag**
(`SetTrue`), so `--high false` would be rejected even after the collision is
fixed. As written the subcommand cannot set a pin LOW. A correct fix needs both
`#[arg(long)]` (dropping `short`) and a decision on the value syntax — either
`--high` / `--low` flags, or an explicit `--level <high|low>`.

M3 owns the `app` crate. This is adjacent rather than in scope for #104, so
either fix it there or file it separately — but it should not be left silently
broken, and it invalidates any test procedure that calls `gallo gpio put`.
### 8.15 M3 outcome, and corrections it supersedes

M3 landed as three commits: `80832cd3810d`
(`feat(lib,ffi,hal,application,mcp,pyco): Bound SPI chip-select host-side`),
`c5ac92f80d50` (`fix(application): Make gpio put usable and able to drive a pin
low`), and `d45bd5517f3f` (`test(ffi): Rename a num_gpios test that never
reached its branch`). Independently verified: no `Cargo.toml`, `Cargo.lock`,
`pico-de-gallo-internal` or firmware file touched; no `Signed-off-by`; workspace
tests green; `gallo gpio put --pin N --level high|low` no longer panics.

**Supersedes §3's M3 inventory and §4's M3 checkbox.** Both still said "append
three `Status` codes" — M1 had already done that (§8.3). The accurate statement
is: preserve `-71..-73` unchanged, and append two *new* codes, `SpiNoGpios = -74`
and `DeviceInfoTimeout = -75`. The book-parity set also grew from 15 files to 17,
adding `book/src/interfaces/batching.md`, `book/src/interfaces/spi.md`,
`docs/ai-agents/pico-de-gallo-hal-examples.md` and
`crates/pico-de-gallo-mcp/README.md`.

**Supersedes spec §3 M3's premise.** The spec said M3 turns a round-trip
returning `Other` into a local error. After M1 and M2 the firmware returns
`InvalidCsPin`, so M3's actual benefit is refusing *before* the RPC is sent, not
improving the error. Accessor distribution, previously ambiguous, resolved as:
`lib`, `ffi`, `hal` and `pyco` get public accessors; the CLI and MCP reuse
already-retained metadata.

**How `num_gpios` is obtained.** An `Arc<OnceLock<u8>>` on `PicoDeGallo`, written
only after a `device/info` that completed, passed schema validation, *and* did so
within a timeout. Anything less leaves the cell empty so the next call retries
rather than caching a guess. Where `validate()` was never called — `Hal::new`,
`gallo_init`, Python `open` — the first bound-checked operation performs that
validated fetch itself. That is deliberate: a bound check is only as trustworthy
as the metadata behind it, and metadata decoded from an unvalidated device may
have been read against the wrong struct shape. Opening stays lax; bound-checking
cannot.

**The binding constraint holds.** Every surface early-returns the metadata error,
so the classifier is only ever reached with a bound from an `Ok`. A `device/info`
failure surfaces as `SpiBatchCallError::DeviceInfo` / FFI `-62`/`-63`/`-64`/`-75`
/ `SpiHalError::DeviceInfo` / a CLI validation abort / JSON-RPC `-32603` /
Python `RuntimeError` — never as `InvalidCsPin`. `num_gpios == 0` is its own
outcome (`SpiNoGpios = -74`), not an ordinary out-of-range index.

**Two blockers were caught in review, both worth recording.**

The first: a draft reordered `mcp/src/spi.rs` to connect-then-parse. `connect()`
calls `system_reset_subscriptions()`, which tears down **every** GPIO
subscription on the board including other processes'. That would have made a
malformed hex argument a destructive cross-process side effect. The current
source was already correct, so this became a regression guard rather than a fix.

The second: adding a `device/info` RPC to the `spi_batch` path introduced a *new*
instance of the AGENTS.md §13.17 (2026-06-03) hang class, because postcard-rpc's
`send_resp` has no timeout. Bounded with `DEVICE_INFO_TIMEOUT = 300 s`,
`ValidateError::Timeout` and `DeviceInfoTimeout = -75`.

**The CLI's `Debug` rendering was deliberately NOT changed**, so §8.7 and §8.13's
expected-output strings remain accurate. §8.13 is a historical record of hardware
observations; rewriting it to match new output would falsify evidence. Worth its
own issue instead.

### 8.16 Open items carried forward

- **300 s is a long time to hold a USB interface.** It cannot fire on a legal
  operation — `DelayNs` × 64 ≈ 275 s is legal today — but an operator cannot
  distinguish a five-minute freeze from a hang. The real fix is bounding total
  batch duration, which is the wire/API decision already flagged in §8.10.
- **The branch must not be tagged or released** while the changed wire shape
  still reports schema 0.6.1. Under D11 `validate()` is a *false negative*:
  matching version numbers do not prove matching wire shape. The maintainer's
  lockstep bump resolves it. This is the sharpest argument for doing that bump
  before any tag.
- **`book/src/crates/app.md` and `book/src/appendix/endpoints.md` have no
  trailing newline** — both pre-existing and byte-verified. M5 sweep.
- **Known coverage gaps, recorded in-code rather than papered over:** Python's
  `num_gpios()` dispatch ordering (needs `pyo3` `auto-initialize`, a manifest
  edit); the MCP ordering test is a behavioural discriminator rather than a true
  connect-counter; `gallo_num_gpios`'s valid-handle/null-out branch needs a
  public seam from `pico-de-gallo-lib`.
- **`gallo gpio put` fix explains a months-old CI blind spot:** none of the 51
  pre-existing CLI tests parsed a `gpio` subcommand, and clap builds a
  subcommand's arguments only when parsing reaches it. The new
  `cli_command_builder_is_well_formed` test calls `Cli::command().debug_assert()`
  recursively, so it now guards **every** subcommand.
### 8.17 M4 outcome

M4 landed as `fde02d042364` — `fix(zephyr): Map SPI child reg to a firmware GPIO
chip-select index`. Independently verified: the `3U` literal is gone,
`pdg_spi.c:250` computes `cs_index` from the DT array and `:333` passes **that**
to `pdg_spi_bottom_batch` rather than `config->slave`, all five statuses are
mapped, every committed blob is LF-clean, and nothing outside `zephyr/` and
`book/` was touched.

**The Zephyr half of #104 is now closed:** `reg` recovers its correct Zephyr
meaning as a slave selector, and the firmware GPIO index is declared explicitly
by the board author.

**Errno mapping,** all five explicit, with `-Werror=switch` enforcing it:

| Status | errno | Rationale |
| --- | --- | --- |
| `SpiInvalidCsPin` −71 | `-EINVAL` | bad argument; the caller fixes the mapping |
| `SpiCsPinUnavailable` −72 | `-EACCES` | mirrors existing `GpioWrongDirection` |
| `SpiCsPinMonitored` −73 | `-EBUSY` | mirrors existing `GpioPinMonitored` |
| `SpiNoGpios` −74 | `-ENODEV` | the resource does not exist |
| `DeviceInfoTimeout` −75 | `-ETIMEDOUT` | bounded request still outstanding |

**Corrections M4 forced elsewhere:**

- **Spec §2.1 was factually wrong about C** and has been corrected in place. See
  that section — the short version is that all five statuses were silently
  mapping to `-EIO` on `main`, because C does not fail a `switch` when an enum
  grows and cbindgen's `int32_t` typedef defeats `-Wswitch` regardless.
- **§3's M4 inventory was short by two files** — `zephyr/drivers/CMakeLists.txt`
  and `book/src/interfaces/spi.md`.
- **My "removing the property must fail the build" instruction was wrong.** D6
  requires a *runtime* `-EINVAL`, which requires the binding property to stay
  optional; a `required: true` or `BUILD_ASSERT` would make the specified error
  unreachable. M4 substituted stronger evidence: preprocessing the production
  translation unit shows `<2 0>` propagating as `{2, 0}` while a deleted property
  yields length 0 and routes to the refusal guard — where a silent identity
  fallback would have produced `{0, 1}` in both cases.

**Duplicate CS indices are permitted, deliberately.** Upstream Zephyr does not
validate `cs-gpios` uniqueness either, and rejecting duplicates would forbid
legitimate shared-select wiring. The hazard — two distinct peripherals selected
simultaneously, both driving MISO, returned bytes being contention rather than
data — is documented in the binding, the README and the book instead.

### 8.18 Two samples cannot link, and never could

`spi_bridge` and `combined_i2c_spi_bridge` fail to link at `main`, before any
#104 work. Their overlays instantiate `issi,is31fl3743b`, and **that driver
exists nowhere** — not in Zephyr v4.4.99 (`drivers/led/` ships `is31fl319x`,
`is31fl3216a`, `is31fl3733` only) and not in this repo. Confirmed independently:
the compatible string appears solely in those two overlay files.

Devicetree accepts the node, so everything including `pdg_spi.c` compiles; only
the final executable link fails on `__device_dts_ord_43` / `__device_dts_ord_44`.

M4's gate was therefore made falsifiable rather than aspirational: `spi_nor_id`
must build **and link**, while the other two must reach *exactly* the
pre-existing ordinal with zero non-link errors and exactly one undefined symbol.
All three passed on that basis.

**Two of the four samples are unbuildable end-to-end and CI does not build them.
This deserves its own issue.** M5 should not silently absorb it.

### 8.19 Build environment notes

For anyone repeating the Zephyr builds:

- WSL **Ubuntu-26.04**; repo visible at `/mnt/d/workspace/pico-de-gallo`;
  Zephyr `~/zephyrproject/zephyr` at **v4.4.0-6123-g26f811ee9d0**.
- Samples are `native_sim` (`zephyr/drivers/spi/Kconfig` has
  `depends on ARCH_POSIX`), so host gcc suffices — no Zephyr SDK needed.
- **`~/zephyrproject/.venv` must be activated**; WSL's system python lacks
  `pyelftools` and the build dies in `gen_kobject_list.py`.
- GitHub intermittently rate-limits the corrosion tarball;
  `-DFETCHCONTENT_SOURCE_DIR_CORROSION` sidesteps it.
- `[Console]::OutputEncoding` is `ibm437` here, which mojibakes UTF-8 (`§`, em
  dashes) when capturing commit messages into PowerShell strings. Set it to UTF8
  before scripting message rewrites.