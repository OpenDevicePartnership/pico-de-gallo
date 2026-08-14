# SPI chip-select contract — design

Date: 2026-08-14
Issue: [#104](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/104)
Related: [#98](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/98) (umbrella), [#99](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/99) (`SPI_CS` pin unclaimed)
Branch: `zephyr`
Status: approved

---

## 1. Problem

Issue #104 reports that the Zephyr SPI driver forwards `struct spi_config.slave`
straight to the firmware as a GPIO pin index, so a devicetree child with
`reg = <0>` silently reconfigures board GPIO0 as a chip select.

Every claim in the issue is confirmed. The line numbers cited are stale — the
issue was written against `f92dd10`, and `docs/superpowers/plans/2026-08-11-zephyr-handoff.md:108-111`
already warns about this. Current locations:

| Issue cites | Actual |
| --- | --- |
| `pdg_spi.c:253` | `zephyr/drivers/spi/pdg_spi.c:280` |
| `pdg_spi.c:179` (`> 3U`) | `zephyr/drivers/spi/pdg_spi.c:206` |
| `internal/src/lib.rs:1317-1320` | `crates/pico-de-gallo-internal/src/lib.rs:1329-1332` |

The issue's "GPIO0" is correct in board terms: firmware index 0 is header pin
11, silkscreened `GPIO0`, physically RP2350 `GPIO8` (`book/src/hardware/pinout.md:81`).

### 1.1 The root cause is in the firmware, not in Zephyr

The Zephyr driver is a faithful consumer of a wire contract that is itself
broken. `crates/pico-de-gallo-firmware/src/handlers/spi.rs:138`:

```rust
cs.set_as_output();   // unconditional; never consults context.pin_modes[cs_idx]
cs.set_high();
```

The pin is never restored and `pin_modes[cs_idx]` is never written. This affects
every host surface — `gallo` CLI, `pico-de-gallo-lib`, FFI, Python, MCP, Zephyr —
so filing it as `bug(zephyr)` understates the blast radius.

### 1.2 The corruption exists in exactly one pin mode

`crates/pico-de-gallo-firmware/src/context.rs:55-63` defines three modes. Tracing
each through an `spi/batch`:

| `pin_modes[cs]` | Hardware after batch | Later `gpio/get` | Later `gpio/put` | Corrupted? |
| --- | --- | --- | --- | --- |
| `LegacyAuto` | output-high | `set_as_input()` first (`gpio.rs:32`), reads correctly | `set_as_output()`, writes | No — self-heals |
| `ExplicitOutput` | output-high | `WrongDirection` (correct) | writes | No — consistent |
| `ExplicitInput` | output-high, mode still claims input | returns `High` — the firmware reading its own drive — **with no error** | `WrongDirection` on a pin that *is* an output | **Yes** |

This is the central finding of the investigation. Because `LegacyAuto` self-heals
and `ExplicitOutput` is already consistent, **the firmware fix is three refusals
and no save/restore machinery at all.** No snapshot, no restore path, no new
state to keep coherent.

The `ExplicitInput` case is a realistic configuration, not a contrived one: an
SPI sensor's `INT`/`DRDY` line wired to a user GPIO while the same board talks
to that sensor. Today, one `spi/batch` turns that interrupt line into a 3V3
push-pull output, permanently, with no error and no way back short of a power
cycle.

### 1.3 Adjacent defects found during investigation

1. **Undiagnosable errors.** `SpiError` (`internal:419`) has exactly two variants,
   `BufferTooLong` and `Other`. A CS index ≥ 4 and a CS pin with a live GPIO
   subscription both collapse into `Other` (`spi.rs:129-137`), indistinguishable
   from a bus fault.
2. **No host-side bound check.** `pico-de-gallo-lib/src/lib.rs:471` and
   `gallo_spi_batch` (`ffi/src/lib.rs:1171`) forward `cs_pin: u8` verbatim,
   despite `NUM_GPIOS` being re-exported at `lib.rs:76`.
3. **Book↔code parity violations** (AGENTS.md §15.1):
   - `book/src/hardware/pinout.md:79` lists GPIO5 as `Direction: Output`,
     `Function: SPI0 CSn`. The firmware never claims `p.PIN_5` — a repo-wide
     search finds zero hits in any Rust source.
   - `book/src/interfaces/spi.md:13-18` implies the user-GPIO workaround is a
     v1.0-only concern. It is the only mechanism on both revisions.

### 1.4 Coupling to #99

`embassy_rp::spi::Spi::new` is called with three pins and no CSn
(`crates/pico-de-gallo-firmware/src/main.rs:393-402`). Chip select here is purely
a software `Flex` toggle.

Routing GPIO5 to the RP2350 hardware CSn would break the "hold CS across an
entire batch" contract, because hardware CS deasserts between FIFO-drained
bursts. Any future #99 work must therefore claim `p.PIN_5` as a **software-toggled**
pin. The addressing scheme chosen here must leave room for that.

---

## 2. Decisions

| # | Decision | Rationale |
| --- | --- | --- |
| D1 | Fix the full stack: firmware root cause, host surfaces, and the Zephyr DT contract. | The Zephyr `reg` overload is the symptom; the unconditional `set_as_output()` is the disease. Fixing only Zephyr leaves every other host corrupting GPIO state. |
| D2 | Mode-aware CS policy: refuse only `ExplicitInput`. | Per §1.2, this is the only corrupting case. Preserves every currently-working flow — `gallo` CLI examples, book snippets, all Zephyr samples — while making the destructive case an explicit error. |
| D3 | Do not restore the CS pin's level after a batch. | Deasserted-high is the correct terminal state for a chip select. Restoring a user-set level could leave CS asserted. Documented rather than fixed. |
| D4 | Do not write `pin_modes[cs_idx]`. | Both surviving modes are already self-consistent (§1.2). Writing `ExplicitOutput` would break `gpio/get` for `LegacyAuto` callers. |
| D5 | Zephyr: `cs-gpio-indices` on the controller node, indexed by child `reg`. | Mirrors the upstream `cs-gpios` convention; `reg` recovers its correct Zephyr meaning; documented in the binding where a DT author will look; extends cleanly for #99. This is the issue's own suggestion. |
| D6 | Missing `cs-gpio-indices` is a hard `-EINVAL`, not an identity fallback. | A silent fallback is precisely how this bug arose. |
| D7 | Reject full `cs-gpios` support. | Zephyr's SPI context would toggle CS through the GPIO API, splitting one atomic `spi/batch` into `gpio/put` + `spi/batch` + `gpio/put`: three USB round-trips, CS no longer held atomically, plus a new GPIO driver to arbitrate the same firmware pins. Strictly worse for this hardware. |
| D8 | Add `num_gpios: u8` to `DeviceInfo`. | The "firmware-reported GPIO count" the issue asks for. Makes #99 a firmware-only follow-up (count 4→5) with no further wire break. |
| D9 | Append three `SpiError` variants. | Turns three distinct failures out of `Other` into diagnosable errors. |
| D10 | Do **not** add `#[non_exhaustive]` to any wire type. | Considered and rejected; see §2.1. |
| D11 | No `[package].version` bumps in this work. | AGENTS.md §4 rule #12. The maintainer bumps versions before opening the PR. |
| D12 | Do not implement #99 here. | It has unresolved design questions of its own (is GPIO5 a fifth user GPIO on `gpio/*`, or CS-only in a separate namespace?). D8 leaves room for it. |

### 2.1 `#[non_exhaustive]` — considered and rejected

Recorded so it is not re-litigated. The initial draft of this design proposed
`#[non_exhaustive]` on the seven error enums and on `DeviceInfo`, reasoning that
the breaking-change cost was already sunk. That reasoning was wrong on two
counts.

**It has zero wire-format impact.** postcard never sees it; it is a Rust-visibility
attribute. It is an *API*-stability tool, not a *wire*-compat tool — postcard
still fails on an unknown variant index, so `PicoDeGallo::validate()` remains the
only wire guard.

**The benefit it exists to provide is unavailable here by policy.**
`#[non_exhaustive]` exists so a crate can append an enum variant *without* a
breaking release. AGENTS.md §6.2 (`AGENTS.md:292-293`) mandates a schema-minor
bump for *"append a variant to a wire enum (even though append-only is
technically non-breaking on the wire, host validation is strict)."* Every
appended variant is therefore a breaking release in this project regardless of
the attribute. It saves nothing that is actually available to be saved.

**It would make this codebase measurably worse.** Without the attribute,
appending a variant *breaks the build* at `pico-de-gallo-ffi`'s `SpiError`→`Status`
mapping and at Zephyr's status→errno mapping (`zephyr/drivers/common/common.c`)
— loudly, at compile time, forcing a deliberate decision about how the new error
surfaces to C and to Zephyr callers. With the attribute, both sites silently fall
into `_ =>` and collapse a new, specific error into a generic one. That is the
same class of defect this very issue is about: `SpiError::Other` swallowing three
distinct failures (§1.3). Trading a compile error for silent degradation, in a
project whose recurring failure mode per AGENTS.md §13.17 is precisely "silent
mismatch nobody notices," is a bad trade.

The exhaustive-match requirement is therefore treated as a **feature** of the
wire crate, not a wart: it is the mechanism that keeps the eight lockstep-coupled
crates honest when the error taxonomy grows.

Consequences of this decision, load-bearing for later milestones:

- `DeviceInfo` keeps public fields and the firmware keeps its struct literal. No
  constructor is needed, which removes the misuse-resistance concern about six
  similarly-typed numeric fields entirely.
- M3 **must** update the `ffi` status mapping and M4 **must** update the Zephyr
  errno mapping for the three new `SpiError` variants. These will not compile
  otherwise — which is the point.

---

## 3. Milestones

Bottom-up by layer. The wire contract lands first — it is the irreversible part
and gets the hardest review; everything downstream then compiles against it.

Each milestone carries its own `book/` changes in the same commit, per
AGENTS.md §15.1. Commits are serialized; no pushing.

### M1 — Wire protocol (`pico-de-gallo-internal`)

**Changes**

1. Append to `SpiError` (`internal:419`), in this order, after `Other`:
   - `InvalidCsPin` — index ≥ `num_gpios`
   - `CsPinUnavailable` — pin is `ExplicitInput`
   - `CsPinMonitored` — pin has a live GPIO subscription (slot is `None`)

   Extend the existing `Display` impl (`internal:426-433`) for each.

2. Append `num_gpios: u8` to `DeviceInfo` (`internal:1508`), last field. Fields
   stay public and the firmware keeps its struct literal — no constructor, per
   D10/§2.1.

3. Doc updates that are part of the contract, not commentary:
   - `SpiBatchRequest::cs_pin` (`internal:1329-1332`) must state that the pin is
     driven as an output for the duration of the batch, is left deasserted-high
     afterwards (D3), and that `ExplicitInput` pins are refused.
   - `NUM_GPIOS` (`internal:467-477`) must state it is the compile-time default
     and that `DeviceInfo::num_gpios` is authoritative at runtime.

**Tests**

- Round-trip serialization for the new `DeviceInfo` shape and each new
  `SpiError` variant.
- A **variant-index pinning test** asserting `BufferTooLong == 0` and
  `Other == 1` in the postcard encoding — a permanent tripwire for AGENTS.md
  §6.1.

**Book:** `book/src/internals/wire-protocol.md`, `book/src/appendix/endpoints.md`.

**Acceptance:** `cargo test --locked` green in the host workspace; the pinning
test fails if a variant is inserted rather than appended.

**Expected downstream breakage — this is intended.** Appending to `SpiError`
will fail to compile at every exhaustive match site, notably the `ffi`
status mapping (M3) and Zephyr's errno mapping (M4). Per §2.1 that is the
mechanism keeping the lockstep crates honest, not an obstacle to work around.
Do not silence it with a wildcard arm.

**Known-red check:** CI's `semver` job (`obi1kenobi/cargo-semver-checks-action@v2`
against `pico-de-gallo-internal`) will fail — `constructible_struct_adds_field`
and `enum_variant_added` are both breaking. It goes green once the maintainer
bumps `internal`'s version (D11). `@integrator` must record this rationale in the
commit body so it is not later mistaken for a regression.

### M2 — Firmware (`pico-de-gallo-firmware`)

**Changes**

Replace `handlers/spi.rs:125-139` with ordered guards, leaving the existing drive
sequence unchanged:

```
cs_idx >= NUM_GPIOS         -> SpiError::InvalidCsPin
gpios[cs_idx].is_none()     -> SpiError::CsPinMonitored
pin_modes[cs_idx] == Input  -> SpiError::CsPinUnavailable
otherwise                   -> set_as_output(); set_high();   (pin_modes untouched, per D4)
```

`device/info` reports `num_gpios: NUM_GPIOS as u8`.

**Open point for `@architect`:** confirm against `embassy-rp` whether
`Flex::set_as_output()` disturbs the pull configured by `gpio/set-config`
(`gpio.rs:280`). If it does, document the interaction; do not add restore
machinery (D3/D4).

**Tests** — the firmware crate has no unit tests (no_std). Verification is
hardware-in-the-loop on the attached board, serial `5256657D8A5D7F03`.

> **Corrected after M2.** The original version of this table specified
> `gpio/set-config{0, Input, Up}` for test 1 and expected `gpio/get{0}` to
> "still read the external signal". **That test could not fail.** A healthy
> pull-up input reads `High`; a *corrupted* pin driven high by
> `set_as_output(); set_high();` also reads `High`. Worse,
> `gpio_for_input!` (`handlers/gpio.rs:31-35`) has `PinMode::ExplicitInput => {}`
> — a genuine no-op that does **not** re-assert `set_as_input()` — so the read
> genuinely returns the firmware's own stale output drive. The decisive
> regression test for #104 would therefore have passed against unfixed
> firmware. Two independent agents found this. Corrected below.

**Required hardware:** a jumper from header **pin 11 (GPIO0, `PIN_8`)** to
header **pin 12 (GPIO1, `PIN_9`)**. GPIO1 is a *witness*: it observes what GPIO0
is physically doing, which no software path on GPIO0 itself can report. Pulls
are `Down`, so a driven-high GPIO0 dominates GPIO1's pull-down and is visible.

| # | Setup | Action | Expected |
| --- | --- | --- | --- |
| 1 | `gpio/set-config{1, Input, Down}`, `gpio/set-config{0, Input, Down}`, witness reads `Low` | `spi/batch{cs:0}` | `CsPinUnavailable`; **and** `gpio/get{1}` still `Low` — either pin reading `High` is a failure |
| 2 | `gpio/set-config{1, Input, Down}`, `gpio/set-config{0, Output}`, `gpio/put{0,low}`, witness `Low` | `spi/batch{cs:0}` | succeeds; witness `gpio/get{1}` transitions to `High` |
| 3 | fresh boot (`LegacyAuto`), `gpio/set-config{1, Input, Down}` | `spi/batch{cs:0}` | succeeds; witness `High`; `gpio/get{0}` returns `Ok`; `gpio/put{0}` still works |
| 4 | — | `spi/batch{cs:4}` | `InvalidCsPin`, not `Other`, and the device stays responsive |
| 5 | `gpio/subscribe{0}`, host killed so the subscription is **orphaned** | `spi/batch{cs:0}` | `CsPinMonitored`, not `Other` |

Test 1 is the decisive regression test for #104. Do not read `gpio/get{0}` in
test 2 — `ExplicitOutput` correctly returns `WrongDirection` there, which looks
like a failure but is not.

Test 5 requires the subscription to be **orphaned**: the `gallo` CLI traps
Ctrl+C and unsubscribes gracefully, so the host must be killed hard. Note also
that only `gallo-mcp` calls `system/reset-subscriptions` on connect — the CLI
never does — so an orphaned subscription survives across CLI invocations.

**Book:** `book/src/internals/firmware.md`, `book/src/interfaces/spi.md`.

**Acceptance:** builds clean for both `hw-rev1` and `hw-rev2` per AGENTS.md §5.2;
all five hardware tests pass.

### M3 — Host surfaces

`lib`, `ffi`, `hal`, `app` (`gallo`), `mcp`, `pyco`.

**Changes**

- `spi_batch` gains a pre-flight `cs_pin < num_gpios` check, turning a USB
  round-trip that returns `Other` into a local error.
- **Open design point for `@architect`:** where `num_gpios` comes from —
  whether `PicoDeGallo` caches `DeviceInfo` at `validate()` time or re-fetches
  per call. Caching is preferred; confirm `validate()` is guaranteed to have run.
- `ffi`: append **three** `Status` codes, one per new `SpiError` variant
  (AGENTS.md §8 — append-only, never renumber). The `SpiError`→`Status` match
  will not compile until all three are handled; per §2.1 that is intended. Do
  **not** collapse them into an existing generic status — the whole point of M1
  is that these three failures stop being indistinguishable.
- `hal`: validate at `spi_device(cs_pin)` construction.
- `gallo` CLI: validate `--cs`.
- `mcp`, `pyco`: validate the `cs_pin` argument.
- Expose a `num_gpios` accessor on the host API.

**Book:** `book/src/crates/{lib,ffi,hal,app,mcp,python}.md`,
`book/src/appendix/status-codes.md`.

**Acceptance:** per-crate `cargo clippy --all-targets --locked -- -D warnings`
and `cargo test --locked` green, per AGENTS.md §5.1.

### M4 — Zephyr module

**Changes**

1. `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml`: add `cs-gpio-indices`
   (int array), indexed by child `reg`, with a `description:` spelling out the
   contract in full.
2. `zephyr/drivers/spi/pdg_spi.c`:
   - store the index array in `struct pdg_spi_config`;
   - `cs_indices_len == 0` → `-EINVAL` with a log line naming the fix (D6);
   - `config->slave >= cs_indices_len` → `-EINVAL`;
   - map `cs_index = cs_indices[config->slave]`;
   - bound `cs_index` against the firmware-reported count, replacing the `3U`
     literal at `:206`.
3. `zephyr/drivers/spi/pdg_spi_bottom.{c,h}`: add `pdg_spi_bottom_num_gpios()`,
   backed by `gallo_device_info`. Cache at init rather than per transceive.
4. All three sample overlays (`spi_bridge`, `spi_nor_id`,
   `combined_i2c_spi_bridge`) declare `cs-gpio-indices` explicitly.
5. `zephyr/README.md:271-300`: rewrite the CS section for the new contract.

**Acceptance:** samples build; the DT contract is inferable from the binding
alone, without reading the README.

### M5 — Docs parity sweep and CHANGELOG

Per AGENTS.md §15.1, per-milestone book edits ride along in M1–M4. M5 carries
only the cross-cutting corrections:

- `book/src/hardware/pinout.md:18,79` — GPIO5 is not claimed by the firmware;
  correct the `Direction` and `Function` columns.
- `book/src/interfaces/spi.md:11-18` — the user-GPIO mechanism is not a
  v1.0-only workaround.
- `CHANGELOG.md` — Keep a Changelog entries for the wire change, the firmware
  fix, the host validation, and the Zephyr DT contract.

---

## 4. Non-goals

- Implementing #99 (claiming `p.PIN_5`). D8 leaves room; the design work is
  separate.
- Full Zephyr `cs-gpios` support (D7).
- Restoring the CS pin's level or pull after a batch (D3).
- Bumping any `[package].version` (D11).
- `#[non_exhaustive]` on any wire type (D10, §2.1).

## 5. Risks

| Risk | Mitigation |
| --- | --- |
| CI `semver` job red for the life of the branch | Expected and accepted (D11). Rationale recorded in the M1 commit body. Goes green on the maintainer's version bump. |
| Appending `SpiError` variants breaks every exhaustive match site | Intended, per §2.1 — it is the mechanism that forces a deliberate decision at each site. Caught at compile time, never at runtime. M3 owns the `ffi` status mapping, M4 owns the Zephyr errno mapping. The risk to guard against is an implementer "fixing" it with a wildcard arm; `@reviewer` must reject that. |
| Hardware tests need a physically reconfigured pin | Single board attached (`5256657D8A5D7F03`); tests 1–5 in M2 need no rewiring, only `gpio/set-config`. |
| Mixed-version host/firmware on this branch mis-decodes `DeviceInfo` | Real but bounded: nothing is published until the maintainer tags. `validate()` will catch it after the version bump. |

## 6. Process

Per-milestone, one fresh `@coordinator`, ephemeral sessions, no reuse:

1. `@architect` — architecture and specification
2. fresh `@architect` — implementation plan
3. `@reviewer` and `@reliability` in parallel — re-spawn `@architect` on amendments
4. `@tester` — adversarial tests
5. `@coder` — implementation
6. `@integrator` — commit
7. `@reviewer` — spec-compliance review

Parallelize where independent; **serialize all commits**. Work stays on branch
`zephyr`. Do not push.
