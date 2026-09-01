# Firmware build identity in `device/info`

Design for issue [#159](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/159).

Date: 2026-09-01
Status: approved, ready for implementation planning
Scope: `pico-de-gallo-internal`, `pico-de-gallo-firmware`, `pico-de-gallo-lib`,
`pico-de-gallo-app`, `pico-de-gallo-mcp`, `pico-de-gallo-ffi`, `pyco-de-gallo`, `book/`

---

## 1. Problem

`PicoDeGallo::validate()` gates on the schema version. Two firmware builds
reporting the **same** schema version can behave differently on the wire, because
`SCHEMA_VERSION_*` is derived from `pico-de-gallo-internal`'s `[package].version`
and therefore tracks **type** changes, not **behaviour** changes.

This has bitten twice, both recorded in AGENTS.md §13.17:

- **2026-08-26, `i2c/batch` atomicity.** Framing moved from per-operation
  START/STOP to a single `transaction()` call inside unchanged schema 0.7.
  AGENTS.md: *"schema-0.7 firmware built before this commit reports the same
  version but frames the bus differently."*
- **2026-08-26, zero-length I2C write guard.** The A/B verification session
  *"misidentified a flash during this very verification"*, and the recorded
  mitigation is *"track the flashed image yourself"*.

The second case is the motivating one: this is not a theoretical hazard. It
produced a wrong conclusion during a hardware verification session, and the
offered mitigation is not a mitigation.

Version numbers answer *"can we talk?"*. They cannot answer *"are you the build I
think you are?"*. That needs a separate field.

### 1.1 Non-goals

- Making build identity a compatibility gate. It is informational only (§6).
- Recording host-side build identity. Firmware only.
- Reproducible-build metadata beyond the git description. No timestamp.

---

## 2. Decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | Carry the identity as `heapless::String<64>` appended to `DeviceInfo` | `postcard-schema` 0.2.5 ships a native `Schema` impl; `heapless` 0.9.3 is already in **both** lockfiles and is already a direct firmware dep, so no new major enters the graph |
| D2 | `git describe --always --dirty --tags --match firmware-v*` | Yields the tag on CI release builds, `tag-N-ghash` mid-branch, `-dirty` for a modified tree |
| D3 | Fall back to the literal `"unknown"`; the build always succeeds | A source tarball or shallow CI checkout must still build |
| D4 | `build.rs` re-runs on **every** build | A stale build ID is worse than none — it would have *confirmed* the 2026-08-26 misidentification |
| D5 | Inline `char build_id[65]` as the last member of `GalloDeviceInfo` | One call, no lifetime questions, mirrors the wire type; struct grows only at the end |
| D6 | Informational only; `validate()` never gates on it | Matches the issue framing; keeps the compatibility policy single-axis |
| D7 | No `[package].version` edits in this PR | AGENTS.md §4 rule 12: a version bump is a deliberate release commit, never a drive-by |
| D8 | `gallo version` renders via `tabled` | The crate already depends on `tabled` for `i2c scan`; `version` was the last hand-rolled `println!` block |
| D9 | `gallo list` is untouched | It makes zero RPCs today; adding one means opening every board, which is slow, per-board fallible, and re-enters the WinUSB single-claim hazard (AGENTS.md §13.17, 2026-07-20) |

---

## 3. Wire type — `pico-de-gallo-internal`

Append **one** field to the end of `DeviceInfo`. postcard encodes struct fields
positionally with no names, so appending is the only safe edit — identical to the
enum-variant rule in AGENTS.md §6.1.

```rust
/// Maximum length of [`DeviceInfo::build_id`], in bytes.
///
/// Must stay <= 255: `heapless::String<N>` defaults to `LenT = u8`, and
/// `postcard-schema`'s `Schema` impl is written for that default. Raising this
/// past 255 also requires changing `LenT`, at which point the `Schema` impl no
/// longer applies.
pub const BUILD_ID_CAPACITY: usize = 64;

pub struct DeviceInfo {
    // ... existing nine fields, unchanged, in order ...

    /// Firmware build identity: the output of
    /// `git describe --always --dirty --tags --match firmware-v*` at firmware
    /// build time, or `"unknown"` when git was unavailable.
    ///
    /// Informational only. This is **never** a compatibility gate — see
    /// [`DeviceInfo::build_id()`] and `PicoDeGallo::validate()`.
    pub build_id: heapless::String<BUILD_ID_CAPACITY>,
}

impl DeviceInfo {
    /// The firmware build identity as a string slice.
    ///
    /// Provided so host crates never need to name `heapless` themselves.
    pub fn build_id(&self) -> &str {
        &self.build_id
    }
}
```

**Rustdoc disambiguation.** A public field `build_id` and an inherent method
`build_id()` coexist fine in Rust, but an intra-doc link written as
`[`DeviceInfo::build_id`]` is **ambiguous** and rustdoc emits a warning — which
CI turns into a failure via the `RUSTDOCFLAGS` doc job. Every intra-doc link to
the accessor must therefore be written with parentheses,
`[`DeviceInfo::build_id()`]`, and links to the field with the `field@`
disambiguator. This applies to every crate that documents the field, not just
`internal`.

### 3.1 Dependencies

Added to `crates/pico-de-gallo-internal/Cargo.toml`:

```toml
heapless = { version = "0.9", default-features = false, features = ["serde"] }
postcard-schema = { version = "0.2.5", features = ["derive", "heapless-v0_9"] }
```

Cost analysis:

- `heapless` 0.9.3 is already in the host `Cargo.lock` (via postcard-rpc) and in
  the firmware `Cargo.lock`, and is already a **direct** firmware dependency
  (`crates/pico-de-gallo-firmware/Cargo.toml`, `heapless = "0.9"`). No new major
  enters either graph.
- `heapless` 0.7.17 and 0.8.0 also exist transitively; `deny.toml` already
  documents the 0.7 situation. `bans.multiple-versions = "warn"` status is
  therefore unchanged by this work.
- `heapless` 0.9.3 declares `rust-version = "1.87"`, below the repo MSRV of 1.90.

### 3.2 Derives

`DeviceInfo` derives `Serialize, Deserialize, Schema, Debug, PartialEq`.
`heapless::String` satisfies all five, so **no derive changes are required**. In
particular `DeviceInfo` deliberately remains non-`Clone` and non-`Copy`.

### 3.3 No `use-std` split

Unlike the byte-payload response types, `build_id` uses one type on both sides.
This is possible because `heapless::String` is `no_std`-native and the host does
not need to grow the value.

---

## 4. Firmware

### 4.1 `build.rs`

`crates/pico-de-gallo-firmware/build.rs` already writes `$OUT_DIR/version.rs`
containing `VERSION_MAJOR/MINOR/PATCH`. Extend that generated file with:

```rust
pub(crate) const BUILD_ID: &str = "...";
```

Generation, via `std::process::Command` — **no shell is involved**, so
`firmware-v*` is passed through literally as its own argv entry and is never
glob-expanded:

```rust
Command::new("git")
    .args(["describe", "--always", "--dirty", "--tags", "--match", "firmware-v*"])
    .current_dir(env!("CARGO_MANIFEST_DIR"))
    .output()
```

Every flag is load-bearing. `build.rs` carries a comment saying so, because each
one has a specific silent-failure mode if removed:

| Flag | Consequence of removing it |
|---|---|
| `--tags` | The `firmware-v*` tags are a **mix** of annotated and lightweight. Without `--tags`, describe considers only annotated tags and resolves to `firmware-v0.10.0-302-g417739147e1c` — 302 commits wrong, with no error. |
| `--match firmware-v*` | Describe picks the nearest tag in **any** namespace. Measured on this tree: `application-v0.9.0-25-g417739147e1c`. |
| `--always` | Describe fails outright in a repo with no matching tag instead of falling back to a bare hash. |
| `--dirty` | Loses the locally-modified marker, which is the state a bisecting developer is in and the single most valuable part of this field. |

Measured on `417739147e1c` at design time:

```
git describe --always --dirty --tags                        -> application-v0.9.0-25-g417739147e1c
git describe --always --dirty --match firmware-v*           -> firmware-v0.10.0-302-g417739147e1c
git describe --always --dirty --tags --match firmware-v*    -> firmware-v0.11.0-25-g417739147e1c
```

**Fallback.** If `git` is absent, exits non-zero, or there is no `.git`, emit the
literal `"unknown"` and additionally emit a `cargo:warning` so the degradation is
visible rather than silent. The build succeeds either way.

**Truncation.** `build.rs` truncates to `BUILD_ID_CAPACITY` bytes on a `char`
boundary. git describe output is ASCII in practice, but a tag name can legally
carry UTF-8, and a byte-index slice would panic mid-codepoint.

**Always re-run (D4).** The existing `cargo:rerun-if-changed=memory.x` *narrows*
re-runs to that single file, so after a commit — or after editing a handler —
cargo rebuilds the crate but does not re-run `build.rs`, and the embedded ID
would keep reporting the previous commit with no `-dirty` marker. That is the
exact misidentification this field exists to prevent. Therefore:

```rust
// Force build.rs to re-run on EVERY build. Cargo treats a nonexistent path as
// always-changed. This is deliberate and load-bearing: without it the embedded
// BUILD_ID goes stale across incremental builds and reports a clean tree for a
// dirty one, which is precisely the misidentification this field exists to
// prevent (issue #159, AGENTS.md §13.17 2026-08-26). Cost is one `git describe`
// (~5 ms) per build. Do not "optimise" this away.
println!("cargo:rerun-if-changed=.pdg-always-rerun");
```

Cargo resolves that path relative to the package root, i.e.
`crates/pico-de-gallo-firmware/.pdg-always-rerun`. The file **must not exist**
and must not be created; a `.gitignore` entry is unnecessary and would only
invite someone to create it.

The existing `rerun-if-changed=memory.x` line stays: it is harmless once an
always-rerun trigger is present, and it documents the real dependency.

### 4.2 Bound enforcement

The firmware carries a compile-time assertion:

```rust
const _: () = assert!(BUILD_ID.len() <= BUILD_ID_CAPACITY);
```

so a capacity mistake is a build failure rather than a runtime
`unwrap_or_default()` that silently reports an empty string.

### 4.3 Handler

`handlers/info.rs::device_info_handler` stays synchronous and infallible. It adds
one field:

```rust
build_id: heapless::String::try_from(BUILD_ID).unwrap_or_default(),
```

The `unwrap_or_default()` is unreachable given §4.2, but keeps the handler
panic-free. `main.rs` registers this handler as `blocking`, and postcard-rpc
dispatches handlers serially on a shared context, so a panic here takes the whole
dispatcher with it.

### 4.4 Boot log

`main()` logs the identity once at boot, beside the existing `hw-rev1`
deprecation warning:

```rust
defmt::info!("build {}", BUILD_ID);
```

`BUILD_ID` is a `&'static str`, so this needs no `heapless/defmt` feature.

---

## 5. Host surfaces

### 5.1 `pico-de-gallo-lib`

No structural change. `DeviceInfo` is re-exported verbatim, so the field arrives
on both call paths — `device_info()` (unbounded, unvalidated) and `validate()`
(bounded by `DEVICE_INFO_TIMEOUT`, schema-checked).

`check_schema_compatible` is **not** touched (D6).

`validate()`'s rustdoc gains a pointer to `DeviceInfo::build_id()`. That doc
already warns that a matching schema version is not proof of matching behaviour;
`build_id` is the field that closes the gap it describes.

### 5.2 `gallo` CLI

`version` is rebuilt on `tabled`, following the `i2c_scan` idiom exactly
(`Builder` → `build()` → `Style::rounded()`), so the crate has one table idiom:

```
╭─────────────┬─────────────────────────────────────────╮
│ Firmware    │ v0.11.0                                 │
│ Schema      │ v0.7.0                                  │
│ HW revision │ 2                                       │
│ GPIOs       │ 4                                       │
│ Build       │ firmware-v0.11.0-25-g417739147e1c-dirty │
╰─────────────┴─────────────────────────────────────────╯
╭─────┬─────┬──────┬──────┬─────┬─────┬────────╮
│ I2C │ SPI │ UART │ GPIO │ PWM │ ADC │ 1-Wire │
├─────┼─────┼──────┼──────┼─────┼─────┼────────┤
│ ✓   │ ✓   │ ✓    │ ✓    │ ✓   │ ✓   │ ✓      │
╰─────┴─────┴──────┴──────┴─────┴─────┴────────╯
```

Two tables, not one: capabilities are a wide boolean row, and packing seven ✓/✗
into a single value cell is what the current `println!` already does badly.

- **`GPIOs` is new output.** `num_gpios` is on the wire type and on the Python
  class but has never been printed by the CLI. Including it is coherent with
  rebuilding this block.
- **`Build` is last**, mirroring `build_id` being the last wire field and the
  last `GalloDeviceInfo` member.
- **The legacy-firmware fallback keeps plain `println!`.** That branch has only a
  version number and one explanatory sentence, and exists for firmware too old to
  have `device/info` at all.
- `tabled` is already a dependency, so there is no `Cargo.toml` or lockfile churn.

`list` is untouched (D9).

### 5.3 `gallo-mcp`

- `device_info` tool: free — it already serializes the whole wire struct.
- `StatusResult` gains `build_id: Option<String>`, filled from the **cached**
  `dev.info()` next to `firmware_version`. No extra RPC.
- One `tracing` line on connect carrying serial, firmware version, and build ID,
  so an agent-driven session has it in the transcript. This is the specific
  ask in the issue.

### 5.4 `pico-de-gallo-ffi`

`GalloDeviceInfo` gains `char build_id[65]` as its **last** member,
NUL-terminated, written by `gallo_get_device_info`.

- Growing only at the end means the layout change affects only consumers who
  recompile. `zephyr/` recompiles in-tree, and `zephyr.yml` is path-filtered to
  include `crates/pico-de-gallo-ffi/`, so the gate will run.
- `GalloDeviceInfo` survives cbindgen pruning today only because
  `gallo_get_device_info`'s signature references it (AGENTS.md §8). That is
  unchanged, so `cbindgen.toml` needs no edit.
- The `65` must be written as a **literal**, because cbindgen folds const
  initializers syntactically and silently emits nothing for a computed value.
  A `const` assertion ties it to `BUILD_ID_CAPACITY + 1`, following the
  `GALLO_NUM_GPIOS` precedent.
- No new `Status` code. Status values are stable C ABI, and C consumers are told
  to write an exhaustive `switch ((enum Status)x)` with no `default:`
  (AGENTS.md §8), so a new value would fall through.

### 5.5 `pyco-de-gallo`

`DeviceInfo` gains `build_id: String` with `#[pyo3(get)]`, converted in the
existing `From<LibDeviceInfo>` impl. It is only ever returned, never taken as an
argument, so no `Clone` / `from_py_object` is needed.

---

## 6. Compatibility policy

`build_id` is **informational and never a gate**.

`validate()` continues to compare schema major and minor only. A host that wants
to assert it is talking to a specific image reads `DeviceInfo::build_id()` and
decides for itself. No new `ValidateError` variant, no new FFI `Status` value, no
new policy surface in any binding.

This is the issue's own framing: version numbers answer *"can we talk?"*,
`build_id` answers *"are you the build I think you are?"*, and only the former is
a gate.

---

## 7. Versioning

**No `[package].version` is edited by this change** (D7).

### 7.1 Why the issue's Notes are not followed literally

The issue says *"schema bump, lockstep version bumps across all eight released
crates, both `Cargo.lock`s"*. That describes the **release commit**, not the
feature commit. AGENTS.md §4 rule 12 is unconditional: a version bump is a
deliberate, manual release step, never a drive-by edit in a feature PR.

The established two-step is visible in the tree: a feature PR lands the wire
change and leaves a `SCHEMA FREEZE` marker; the maintainer's later release commit
performs the lockstep bump across all eight crates, rewrites every cross-crate
dep spec, hand-writes the CHANGELOGs, and regenerates both lockfiles.

### 7.2 Stale markers to delete

`pico-de-gallo-internal/src/lib.rs` carries two `SCHEMA FREEZE` comments, at
`:433` (`SpiError`) and `:1583` (`DeviceInfo`), both saying *"Do not release or
tag this branch until the maintainer performs the lockstep version bump."*

**Both are stale.** Verified at design time:

- `internal-v0.7.0` is tagged, and that tag's `lib.rs` already contains
  `num_gpios: u8` and the appended `SpiError` variants.
- `git show internal-v0.7.0:crates/pico-de-gallo-internal/Cargo.toml` reads
  `version = "0.7.0"`.
- `firmware-v0.11.0` is tagged.

The bump those markers demanded **has been performed**; nobody removed them at
release. Delete both. Leaving false markers beside a live one destroys the
signal — during this very design session they caused an incorrect recommendation
that schema 0.7 was still unreleased. This cleanup is in scope precisely because
the change depends on that mechanism being readable.

### 7.3 New marker

Add a fresh `SCHEMA FREEZE` marker on `build_id` recording that:

- the addition is append-only but still requires a lockstep bump per §6.2;
- the next release of `pico-de-gallo-internal` must therefore be **0.8.0**, not
  0.7.1 — a new wire field is not a patch;
- the branch must not be released or tagged until that bump is performed.

### 7.4 CHANGELOGs

Each affected crate gets an entry under a new `## [Unreleased]` heading, which is
the Keep a Changelog answer for "landed, not yet versioned". There is no root
`CHANGELOG.md`.

---

## 8. Risks and accepted limitations

| # | Risk | Disposition |
|---|---|---|
| R1 | A **new host** talking to **released firmware 0.11.0** will fail to decode `device/info`: postcard hits end-of-input on the appended field. `validate()` cannot warn, because both sides still report schema 0.7. | **Accepted and unavoidable** for any append to an already-released struct. `map_validate_error` maps `DeserFailed` to `Comms`, deliberately not `LegacyFirmware`, so the error is honest but not self-explanatory. Host and firmware must be built from the same tree until the 0.8.0 release. Stated in the PR body and the CHANGELOG entries. |
| R2 | `GalloDeviceInfo` layout change breaks C consumers that do not recompile. | Accepted. The struct grows only at the end; every released crate bumps in lockstep at release; `zephyr/` recompiles in-tree and `zephyr.yml` covers the FFI path filter. |
| R3 | Always-rerunning `build.rs` costs one `git describe` per firmware build. | Accepted, ~5 ms. The alternative (tracking `.git/HEAD`, refs and index) misses unstaged `.rs` edits, which reintroduces the exact stale-ID failure. |
| R4 | No automated test covers the `build.rs` git invocation or the firmware handler. | **Honest gap.** The git invocation is not unit-testable in-tree and the handler needs real registers. Covered only by the manual acceptance procedure in §9.2. |
| R5 | A future capacity raise past 255 silently loses the `Schema` impl (`LenT = u8`). | Guarded by a unit test asserting `BUILD_ID_CAPACITY <= 255` and by the doc comment on the constant. |
| R6 | `git describe` in a shallow CI clone may not reach a `firmware-v*` tag, yielding a bare hash. | Acceptable — `--always` still gives a usable identity. Release workflows should fetch tags; noted in §9.1. |
| R7 | The field `build_id` and the accessor `build_id()` share a name, making bare intra-doc links ambiguous and failing the `RUSTDOCFLAGS` doc job. | Guarded by convention: always write `[`DeviceInfo::build_id()`]` for the method and the `field@` disambiguator for the field (§3). |

---

## 9. Testing

### 9.1 Automated

| Layer | Test |
|---|---|
| `internal` | `device_info_round_trip_carries_build_id` — postcard round-trip with a full 64-char ID, an empty ID, and `"unknown"`. `build_id_capacity_fits_len_type` asserts `BUILD_ID_CAPACITY <= 255` (R5). |
| `lib` | Extend the fake-transport `make_device_info` helper with a build ID. Assert `validate()` **succeeds** on an unexpected build ID, pinning D6. |
| `app` | Snapshot the two-table `version` output for a fixed `DeviceInfo`, so the `tabled` formatting is covered rather than eyeballed. |
| `ffi` | `build_id` is NUL-terminated; a full 64-char ID does not overflow `char[65]`; the `const` assert ties `65` to `BUILD_ID_CAPACITY + 1`. |
| `pyco` | Sibling of `device_info_conversion_carries_num_gpios` covering `build_id`. |
| `mcp` | `StatusResult` serializes `build_id`; it is absent when not connected. |
| firmware | None — see R4. |

Release workflows that build firmware must fetch tags (`fetch-depth: 0` or an
explicit tag fetch), or `git describe` degrades to a bare hash (R6).

### 9.2 Manual acceptance (board-attached, not CI)

From the issue: flash two firmware images built from commits that differ only in
handler behaviour, and confirm `gallo version` reports two **different** `Build`
values. Additionally confirm that editing a source file without committing
produces a `-dirty` suffix on the next build, which is the D4 regression test.

---

## 10. Documentation

Per AGENTS.md §15.1 book-parity, in the same PR:

| File | Change |
|---|---|
| `book/src/internals/wire-protocol.md` | New `DeviceInfo` field; the "informational, never a gate" ruling; the R1 same-tree caveat |
| `book/src/appendix/endpoints.md` | `device/info` row wording gains build identity |
| `book/src/getting-started/verify.md` | The `gallo version` sample output block — **currently stale** at `FW v0.8.0` / `Schema v0.4.0`, and now also the wrong format. Replace with the two-table output and extend the field-meanings table. |
| `book/src/internals/firmware.md` | `build.rs` behaviour, the four load-bearing git flags, and the always-rerun rationale |
| `book/src/crates/{lib,ffi,mcp,python}.md` | New field on each surface |
| `crates/pico-de-gallo-mcp/README.md` | `status` gains `build_id`; connect-time log line |
| `AGENTS.md` §13.17 | A row recording that this closes the documented misidentification class |

---

## 11. Out of scope

- Host-side (CLI / library) build identity.
- Any expected-build-id assertion API. Explicitly rejected in favour of D6;
  callers can compare `build_id()` themselves.
- `gallo list` build-ID column (D9).
- The lockstep 0.8.0 release itself (§7).
