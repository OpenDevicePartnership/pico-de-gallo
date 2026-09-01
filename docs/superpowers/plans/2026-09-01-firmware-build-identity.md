# Firmware Build Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `build_id` field to `device/info` so a host can tell two firmware builds apart when they report the same schema version.

**Architecture:** The firmware `build.rs` runs `git describe --always --dirty --tags --match firmware-v*` on every build and emits a `BUILD_ID` const. The `device/info` handler returns it as a `heapless::String<64>` appended to the end of the `DeviceInfo` wire struct. Every host surface (lib, CLI, MCP, FFI, Python) then exposes it as informational data — it is never a compatibility gate.

**Tech Stack:** Rust 2024, `postcard` / `postcard-rpc` 0.12 / `postcard-schema` 0.2.5, `heapless` 0.9, `embassy` (no_std RP2350), `tabled` 0.21, PyO3, cbindgen.

**Design doc:** `docs/superpowers/specs/2026-09-01-firmware-build-identity-design.md`

---

## Ground rules for every task

Read these once before Task 1. They are repo policy from `AGENTS.md`, and violating them fails CI.

1. **Every commit must build cleanly on its own** (§4 rule 9). That is why Task 2 is large: appending a field to `DeviceInfo` breaks every construction site at once, so they are fixed in the same commit.
2. **Never bump `[package].version`** anywhere in this plan (§4 rule 12). No exceptions.
3. **LF line endings** on every file (§3).
4. **Firmware logs with `defmt` only** — no `println!`, no `log` (§4 rule 5).
5. **Commit trailers** on every commit:
   ```
   Assisted-by: OpenCode:claude-opus-5
   Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
   ```
   Never `Signed-off-by:` (§4 rule 7).
6. **Two Cargo workspaces.** Host commands run from the repo root. Firmware commands run from `crates/pico-de-gallo-firmware/` and need `--target thumbv8m.main-none-eabihf`.
7. **`pico-de-gallo-internal` tests need `use-std`** or must be run from the workspace root, or the `vec!` macro fails under `no_std` (§13.14).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/pico-de-gallo-firmware/build.rs` | Run `git describe`, emit `BUILD_ID`, force unconditional re-run | 1 |
| `crates/pico-de-gallo-firmware/src/main.rs` | Boot-time `defmt` log of `BUILD_ID`; compile-time length assert | 1, 2 |
| `crates/pico-de-gallo-internal/Cargo.toml` | `heapless` dep, `postcard-schema` feature | 2 |
| `crates/pico-de-gallo-internal/src/lib.rs` | `BUILD_ID_CAPACITY`, the `build_id` field, the `build_id()` accessor, freeze-marker hygiene | 2 |
| `crates/pico-de-gallo-firmware/src/handlers/info.rs` | Populate `build_id` in the handler | 2 |
| `crates/pico-de-gallo-lib/src/lib.rs` | Test fixtures; `validate()` doc; the "never a gate" regression test | 2, 3 |
| `crates/pico-de-gallo-app/src/lib.rs` | `render_device_info` (pure, testable) + `version` printing it | 4 |
| `crates/pico-de-gallo-mcp/src/device.rs` | `StatusResult.build_id` | 5 |
| `crates/pico-de-gallo-mcp/src/lib.rs` | Connect-time `tracing` line | 5 |
| `crates/pico-de-gallo-ffi/src/lib.rs` | `GalloDeviceInfo.build_id` as `char[65]` | 6 |
| `crates/pyco-de-gallo/src/lib.rs` | Python `DeviceInfo.build_id` | 7 |
| `book/src/**`, `crates/*/CHANGELOG.md`, `AGENTS.md` | Documentation parity (§15.1) | 8 |

---

## Task 1: Firmware `build.rs` emits `BUILD_ID`

This lands first and alone because it must not create dead code: the boot log in
`main.rs` consumes `BUILD_ID` immediately, so `clippy -D warnings` stays clean.

**Files:**
- Modify: `crates/pico-de-gallo-firmware/build.rs`
- Modify: `crates/pico-de-gallo-firmware/src/main.rs`

- [ ] **Step 1: Confirm the git incantation on this machine**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
git describe --always --dirty --tags --match 'firmware-v*'
```
Expected: something shaped like `firmware-v0.11.0-27-g8ddb1da5a681` (the commit
count will differ). If it prints an `application-v*` string, the `--match` was
dropped; if it prints `firmware-v0.10.0-3xx-g...`, the `--tags` was dropped.

- [ ] **Step 2: Add the build-ID generation to `build.rs`**

In `crates/pico-de-gallo-firmware/build.rs`, add `use std::process::Command;` to
the imports at the top (joining `use std::env;`, `use std::fs::File;`,
`use std::io::Write;`, `use std::path::PathBuf;`).

Then add this function below `fn main()`:

```rust
/// Describe the current firmware build from git.
///
/// Every flag is load-bearing, and each has a *silent* failure mode:
///
/// * `--tags` — the `firmware-v*` tags are a MIX of annotated and lightweight.
///   Without this, `git describe` considers only annotated tags and resolves
///   hundreds of commits too far back (measured: `firmware-v0.10.0-302-g...`).
/// * `--match firmware-v*` — without it, describe picks the nearest tag in ANY
///   namespace (measured: `application-v0.9.0-25-g...`).
/// * `--always` — falls back to a bare hash instead of failing when no matching
///   tag is reachable, e.g. in a shallow CI clone.
/// * `--dirty` — marks a locally modified tree. This is the single most
///   valuable part of the field for a bisecting developer.
///
/// No shell is involved: the arguments are separate argv entries, so
/// `firmware-v*` is passed through literally and is never glob-expanded.
///
/// Returns `"unknown"` when git is unavailable, exits non-zero, or there is no
/// repository. The build always succeeds; a source tarball must still build.
fn build_id() -> String {
    let output = Command::new("git")
        .args([
            "describe",
            "--always",
            "--dirty",
            "--tags",
            "--match",
            "firmware-v*",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();

    let described = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            println!(
                "cargo:warning=`git describe` unavailable; \
                 device/info will report build_id=\"unknown\""
            );
            return "unknown".to_string();
        }
    };

    if described.is_empty() {
        return "unknown".to_string();
    }

    // Truncate on a char boundary. git describe output is ASCII in practice,
    // but a tag name may legally carry UTF-8 and a byte-index slice would
    // panic mid-codepoint. Keep this in sync with
    // `pico_de_gallo_internal::BUILD_ID_CAPACITY`.
    const BUILD_ID_CAPACITY: usize = 64;
    if described.len() <= BUILD_ID_CAPACITY {
        described
    } else {
        let mut end = BUILD_ID_CAPACITY;
        while !described.is_char_boundary(end) {
            end -= 1;
        }
        described[..end].to_string()
    }
}
```

- [ ] **Step 3: Emit `BUILD_ID` into the generated `version.rs`**

In `crates/pico-de-gallo-firmware/build.rs`, replace the `File::create(out.join("version.rs"))` block (currently lines 26-39) with:

```rust
    let build_id = build_id();

    // `{:?}` on a &str emits a properly escaped Rust string literal, quotes
    // included. Git refnames forbid backslash but permit `"`, so a tag like
    // `firmware-v1.0"x` would otherwise emit a syntax error.
    File::create(out.join("version.rs"))
        .unwrap()
        .write_all(
            format!(
                r##"
pub(crate) const VERSION_MAJOR: u16 = {major};
pub(crate) const VERSION_MINOR: u16 = {minor};
pub(crate) const VERSION_PATCH: u32 = {patch};

/// Firmware build identity from `git describe`, or `"unknown"`.
pub(crate) const BUILD_ID: &str = {build_id:?};
"##
            )
            .as_bytes(),
        )
        .unwrap();
```

Note this switches the `format!` from positional to inline captured
identifiers, matching the style already used in
`crates/pico-de-gallo-internal/build.rs`.

- [ ] **Step 4: Force `build.rs` to re-run on every build**

In `crates/pico-de-gallo-firmware/build.rs`, replace the single line
`println!("cargo:rerun-if-changed=memory.x");` (currently line 41) with:

```rust
    println!("cargo:rerun-if-changed=memory.x");

    // Force this build script to re-run on EVERY build. Cargo treats a
    // nonexistent path as always-changed.
    //
    // This is deliberate and load-bearing. `rerun-if-changed=memory.x` above
    // NARROWS re-runs to that one file, so without this line the embedded
    // BUILD_ID goes stale across incremental builds: after a commit, or after
    // editing a handler, cargo rebuilds the crate but does not re-run this
    // script, and the firmware keeps reporting the previous commit with no
    // `-dirty` marker. A stale build ID is worse than none — it would CONFIRM
    // a wrong conclusion, which is exactly the misidentification this field
    // exists to prevent (issue #159; AGENTS.md §13.17, 2026-08-26).
    //
    // Cost is one `git describe` (~5 ms) per build. Do not "optimise" it away.
    // The path is resolved relative to the package root and MUST NOT exist.
    println!("cargo:rerun-if-changed=.pdg-always-rerun");
```

- [ ] **Step 5: Log the build ID at boot**

In `crates/pico-de-gallo-firmware/src/main.rs`, find the `hw-rev1` deprecation
warning inside `main()`. Search for it:

```bash
cd /home/balbi/workspace/pico-de-gallo
rg -n 'hw-rev1' crates/pico-de-gallo-firmware/src/main.rs
```

Immediately **before** that `#[cfg(feature = "hw-rev1")]` warn block, add:

```rust
    // Logged unconditionally at boot so an RTT capture records exactly which
    // image is running, independent of any host-side query.
    defmt::info!("build {}", BUILD_ID);
```

`BUILD_ID` is already in scope via the existing
`include!(concat!(env!("OUT_DIR"), "/version.rs"));` at `main.rs:122`.

- [ ] **Step 6: Build the firmware and verify it compiles**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
```
Expected: PASS, no warnings about unused `BUILD_ID`.

- [ ] **Step 7: Verify the generated constant actually contains the tag**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
grep -rh 'BUILD_ID' target/thumbv8m.main-none-eabihf/release/build/*/out/version.rs
```
Expected: a line like
`pub(crate) const BUILD_ID: &str = "firmware-v0.11.0-27-g8ddb1da5a681";`

It must start with `firmware-v` (not `application-v`), and the commit count
must be small (tens, not hundreds).

- [ ] **Step 8: Verify the always-rerun trigger works**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
touch /tmp/pdg-dirty-probe
cargo build --release --locked --target thumbv8m.main-none-eabihf 2>&1 | tail -3
grep -rh 'BUILD_ID' target/thumbv8m.main-none-eabihf/release/build/*/out/version.rs
```
Expected: the build re-runs the script (the `out/version.rs` timestamp updates)
rather than being fully cached. Confirm with:
```bash
stat -c '%y' crates/pico-de-gallo-firmware/target/thumbv8m.main-none-eabihf/release/build/*/out/version.rs
```
Expected: a timestamp from the run you just did.

- [ ] **Step 9: Lint the firmware**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
```
Expected: both PASS.

- [ ] **Step 10: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-firmware/build.rs crates/pico-de-gallo-firmware/src/main.rs
git commit -F - <<'EOF'
feat(firmware): Derive a build identity from git describe

`validate()` gates on the schema version, which tracks type changes
rather than behaviour changes, so two builds can report the same
version and still frame the bus differently. Generate a build identity
so the running image can be named.

`build.rs` now runs `git describe --always --dirty --tags --match
firmware-v*` and emits `BUILD_ID`, which `main()` logs at boot. Every
flag is load-bearing and commented as such: without `--tags` describe
sees only annotated tags and resolves 302 commits too far back, since
the `firmware-v*` tags are a mix of annotated and lightweight; without
`--match` it returns an `application-v*` description.

Force the script to re-run on every build. The pre-existing
`rerun-if-changed=memory.x` narrows re-runs to one file, so the
embedded identity would go stale across incremental builds and report
a clean tree for a dirty one. A stale identity is worse than none
because it confirms a wrong conclusion.

Fall back to "unknown" when git is unavailable so a source tarball
still builds, and truncate on a char boundary at 64 bytes.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 2: Append `build_id` to the `DeviceInfo` wire type

This is the largest commit by necessity: appending a field breaks every
construction site simultaneously, and AGENTS.md §4 rule 9 requires each commit to
build cleanly on its own.

**Files:**
- Modify: `crates/pico-de-gallo-internal/Cargo.toml`
- Modify: `crates/pico-de-gallo-internal/src/lib.rs`
- Modify: `crates/pico-de-gallo-firmware/Cargo.toml` (lockfile only — see Step 9)
- Modify: `crates/pico-de-gallo-firmware/src/handlers/info.rs`
- Modify: `crates/pico-de-gallo-firmware/src/main.rs`
- Modify: `crates/pico-de-gallo-lib/src/lib.rs` (test fixture only)

- [ ] **Step 1: Write the failing tests**

In `crates/pico-de-gallo-internal/src/lib.rs`, inside `mod tests`, add these
three tests immediately after the existing `device_info_no_capabilities_round_trip`
test (which ends around line 3627):

```rust
    #[test]
    fn device_info_round_trip_carries_build_id() {
        // Sweep the three shapes the firmware can actually produce: a full
        // describe string, the git-unavailable fallback, and empty (which a
        // buggy build.rs could emit and which must still decode).
        for id in [
            "firmware-v0.11.0-27-g8ddb1da5a681-dirty",
            "unknown",
            "",
            // Exactly BUILD_ID_CAPACITY bytes.
            "0123456789012345678901234567890123456789012345678901234567890123",
        ] {
            let info = DeviceInfo {
                fw_major: 0,
                fw_minor: 11,
                fw_patch: 0,
                schema_major: 0,
                schema_minor: 7,
                schema_patch: 0,
                hw_version: 2,
                capabilities: Capabilities::I2C,
                num_gpios: NUM_GPIOS as u8,
                build_id: heapless::String::try_from(id).unwrap(),
            };
            let bytes = to_allocvec(&info).unwrap();
            let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
            assert_eq!(info, decoded, "build_id {id:?} must round-trip");
            assert_eq!(decoded.build_id(), id);
        }
    }

    #[test]
    fn build_id_capacity_is_sixty_four() {
        // Pinned because the firmware build script hardcodes the same number
        // for truncation and cannot import this constant.
        assert_eq!(BUILD_ID_CAPACITY, 64);
    }

    #[test]
    fn device_info_rejects_overlong_build_id() {
        // heapless deserialization errors rather than truncating, so an
        // over-long id is a decode failure, not silent data loss. This is why
        // the firmware build script must do the truncating.
        #[derive(Serialize)]
        struct OverlongDeviceInfo {
            fw_major: u16,
            fw_minor: u16,
            fw_patch: u32,
            schema_major: u16,
            schema_minor: u16,
            schema_patch: u32,
            hw_version: u8,
            capabilities: Capabilities,
            num_gpios: u8,
            build_id: &'static str,
        }

        let overlong = OverlongDeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 4,
            schema_minor: 5,
            schema_patch: 6,
            hw_version: 7,
            capabilities: Capabilities(8),
            num_gpios: 9,
            // 65 bytes: one past BUILD_ID_CAPACITY.
            build_id: "01234567890123456789012345678901234567890123456789012345678901234",
        };
        let bytes = to_allocvec(&overlong).unwrap();
        assert!(from_bytes::<DeviceInfo>(&bytes).is_err());
    }
```

- [ ] **Step 2: Extend the byte-image test**

The existing `device_info_encodes_fields_in_declared_order` test (around line
3713) asserts an exact 9-byte encoding and **will** fail once the field is
added. That is by design — it is the test that proves the field is genuinely on
the wire in the declared position. Update it.

In `crates/pico-de-gallo-internal/src/lib.rs`, replace the body of
`device_info_encodes_fields_in_declared_order` with:

```rust
    fn device_info_encodes_fields_in_declared_order() {
        // A plain round-trip is self-consistent by construction: it would
        // still pass if a field were never serialized at all. This test pins
        // the actual byte image, which is what proves each field is genuinely
        // on the wire, in the declared position. Every numeric value is below
        // 128 so each postcard varint occupies exactly one byte.
        //
        // `build_id` is last, and postcard encodes a string as a varint length
        // followed by UTF-8 bytes -- `heapless`'s Serialize impl is
        // `serialize_str`, so neither N nor LenT appears on the wire.
        let info = DeviceInfo {
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            schema_major: 4,
            schema_minor: 5,
            schema_patch: 6,
            hw_version: 7,
            capabilities: Capabilities(8),
            num_gpios: 9,
            build_id: heapless::String::try_from("ab").unwrap(),
        };
        let bytes = to_allocvec(&info).unwrap();
        assert_eq!(bytes.as_slice(), &[1u8, 2, 3, 4, 5, 6, 7, 8, 9, 2, b'a', b'b'][..]);
        assert_eq!(bytes.len(), 12);
        let decoded: DeviceInfo = from_bytes(&bytes).unwrap();
        assert_eq!(info, decoded);

        // An empty build_id costs exactly one byte (the zero length).
        let empty = DeviceInfo {
            build_id: heapless::String::new(),
            ..info
        };
        let bytes = to_allocvec(&empty).unwrap();
        assert_eq!(bytes.as_slice(), &[1u8, 2, 3, 4, 5, 6, 7, 8, 9, 0][..]);
    }
```

Note: `..info` requires `DeviceInfo` to be movable here; `info` is consumed by
the struct-update syntax after its last use, which is fine because the
assertions on `info` happen before it.

- [ ] **Step 3: Run the tests to verify they fail**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-internal 2>&1 | tail -20
```
Expected: FAIL to compile — `DeviceInfo` has no field named `build_id`, and
`BUILD_ID_CAPACITY` is not found.

- [ ] **Step 4: Add the dependencies**

In `crates/pico-de-gallo-internal/Cargo.toml`, in `[dependencies]`, add the
`heapless` line and extend the `postcard-schema` features:

```toml
serde = { version = "1.0.228", default-features = false, features = ["derive"] }
postcard = { version = "1", default-features = false }
postcard-rpc = "0.12"
postcard-schema = { version = "0.2.5", features = ["derive", "heapless-v0_9"] }
# Bounded string for `DeviceInfo::build_id`, so the field needs no `use-std`
# split. 0.9.3 is already in both lockfiles via postcard-rpc, and is already a
# direct firmware dependency, so this introduces no new major to either graph.
heapless = { version = "0.9", default-features = false, features = ["serde"] }
```

- [ ] **Step 5: Add the capacity constant and the field**

In `crates/pico-de-gallo-internal/src/lib.rs`, add the constant immediately
above the `DeviceInfo` doc comment (just before the `/// Extended device
information...` block that precedes the struct at ~line 1578):

```rust
/// Maximum length of [`DeviceInfo::build_id()`], in bytes.
///
/// The field must always be spelled `heapless::String<BUILD_ID_CAPACITY>`,
/// leaving `LenT` at its default. `postcard-schema` implements `Schema` only
/// for `String<N>` at the default `LenT`, so naming a non-default `LenT`
/// silently breaks the `Schema` derive.
///
/// This is a *receive-side* bound. `heapless` deserialization rejects a longer
/// string rather than truncating it, so the firmware build script is
/// responsible for truncating; see `crates/pico-de-gallo-firmware/build.rs`.
/// Raising this value is not a wire-format change (the encoding is a plain
/// postcard string either way), but it is a `Schema` change and still needs a
/// lockstep bump per AGENTS.md §6.2.
pub const BUILD_ID_CAPACITY: usize = 64;
```

Then append the field as the **last** field of `DeviceInfo`, after `num_gpios`:

```rust
    /// Firmware build identity.
    ///
    /// The output of `git describe --always --dirty --tags --match
    /// firmware-v*` captured when the firmware was built, or `"unknown"` when
    /// git was unavailable at build time.
    ///
    /// Informational only. This is **never** a compatibility gate: schema
    /// version answers "can we talk?", and this field answers "are you the
    /// build I think you are?". `PicoDeGallo::validate()` deliberately ignores
    /// it. Use [`DeviceInfo::build_id()`] to read it as a `&str`.
    pub build_id: heapless::String<BUILD_ID_CAPACITY>,
```

- [ ] **Step 6: Add the accessor**

In `crates/pico-de-gallo-internal/src/lib.rs`, immediately after the closing
brace of the `DeviceInfo` struct, add:

```rust
impl DeviceInfo {
    /// The firmware build identity as a string slice.
    ///
    /// Provided so host crates never need to name `heapless` themselves.
    ///
    /// Note for doc authors: this method and the `build_id` field share a
    /// name, so a bare intra-doc link `[DeviceInfo::build_id]` is ambiguous
    /// and fails the `RUSTDOCFLAGS` doc job. Always write
    /// `[DeviceInfo::build_id()]` for this method.
    pub fn build_id(&self) -> &str {
        &self.build_id
    }
}
```

- [ ] **Step 7: Fix the schema-freeze markers**

Two `SCHEMA FREEZE` comments are stale — the bump they demanded was performed
and released as `internal-v0.7.0`. Verify that first:

```bash
cd /home/balbi/workspace/pico-de-gallo
git show internal-v0.7.0:crates/pico-de-gallo-internal/Cargo.toml | grep '^version'
git show internal-v0.7.0:crates/pico-de-gallo-internal/src/lib.rs | grep -c 'num_gpios: u8'
```
Expected: `version = "0.7.0"` and a count of `1` — proving the released tag
already contains the change the markers describe.

Now **delete** the stale marker above `SpiError` (currently at `lib.rs:433`):

```rust
// SCHEMA FREEZE: SpiError and DeviceInfo change shape on the `zephyr` branch
```
(delete that comment and its continuation lines)

And **replace** the stale marker above `DeviceInfo` (currently at `lib.rs:1583`,
the four-line `// SCHEMA FREEZE: This field addition is intentionally
incompatible...` block) with:

```rust
// SCHEMA FREEZE: `build_id` was appended after the schema 0.7.0 release
// (tags `internal-v0.7.0` / `firmware-v0.11.0`). The addition is append-only,
// but a host built from this tree cannot decode `device/info` from a released
// 0.11.0 firmware -- postcard hits end-of-input on the new field -- and
// `validate()` cannot warn, because both peers still report schema 0.7.
// Host and firmware must therefore be built from the same tree until release.
//
// Do not release or tag this branch until the maintainer performs the lockstep
// version bump required by AGENTS.md §6.5. The next `pico-de-gallo-internal`
// release must be 0.8.0, NOT 0.7.1: a new wire field is not a patch.
```

- [ ] **Step 8: Populate the field in the firmware handler**

In `crates/pico-de-gallo-firmware/src/handlers/info.rs`:

Change the `crate` import on line 10 from:
```rust
use crate::{HW_VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH};
```
to:
```rust
use crate::{BUILD_ID, HW_VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH};
```

Then add the field as the last entry of the returned `DeviceInfo` literal,
after `num_gpios: NUM_GPIOS as u8,`:

```rust
        // Cannot fail: `build.rs` truncates to BUILD_ID_CAPACITY and
        // `main.rs` asserts the bound at compile time. `unwrap_or_default()`
        // rather than `unwrap()` because postcard-rpc dispatches handlers
        // serially on a shared context, so a panic here would take the whole
        // dispatcher down rather than failing one call.
        build_id: heapless::String::try_from(BUILD_ID).unwrap_or_default(),
```

- [ ] **Step 9: Add the compile-time length assertion**

In `crates/pico-de-gallo-firmware/src/main.rs`, immediately after the
`include!(concat!(env!("OUT_DIR"), "/version.rs"));` line (currently line 122),
add:

```rust
// Bounds the generated value against the wire capacity: a BUILD_ID that
// would not fit `DeviceInfo::build_id` is a build failure here rather than
// a runtime `unwrap_or_default()` silently reporting an empty identity.
//
// Note what this does NOT do. `build.rs` duplicates the capacity literal
// because a build script cannot import from the crate it is building, and
// this assertion does not compare the two literals -- it only checks the
// value that was actually produced. A build.rs truncating at 128 would
// still pass here whenever `git describe` happened to emit under 64 bytes.
const _: () = assert!(BUILD_ID.len() <= pico_de_gallo_internal::BUILD_ID_CAPACITY);
```

- [ ] **Step 10: Fix the host-lib test fixture**

In `crates/pico-de-gallo-lib/src/lib.rs`, in `mod tests`, the `make_device_info`
helper (around line 1875) constructs a `DeviceInfo` and will no longer compile.
Add the field as its last entry, after `num_gpios: NUM_GPIOS as u8,`:

```rust
            build_id: "test-build".try_into().unwrap(),
```

- [ ] **Step 11: Run the host tests**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test --locked 2>&1 | tail -30
```
Expected: PASS, including the three new `internal` tests and the updated
`device_info_encodes_fields_in_declared_order`.

- [ ] **Step 12: Refresh both lockfiles**

`heapless` becomes a direct dependency of `pico-de-gallo-internal`, so both
workspaces need their locks regenerated (AGENTS.md §7.1).

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo check --workspace --locked 2>&1 | tail -5 || cargo generate-lockfile
cargo check --workspace --locked
cd crates/pico-de-gallo-firmware
cargo check --locked --target thumbv8m.main-none-eabihf 2>&1 | tail -5 \
  || cargo update -p pico-de-gallo-internal
cargo check --locked --target thumbv8m.main-none-eabihf
```
Expected: both `cargo check --locked` invocations PASS. If the first attempt
reports the lock is out of date, the fallback regenerates it; re-run the check.

- [ ] **Step 13: Confirm cargo-deny is still clean**

`heapless` 0.7 / 0.8 / 0.9 already coexist, so `bans.multiple-versions = "warn"`
status must be unchanged — not newly worse.

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo deny --manifest-path Cargo.toml check 2>&1 | tail -20
```
Expected: no *new* errors. Pre-existing `multiple-versions` warnings for
`heapless` are acceptable and already documented in `deny.toml`.

- [ ] **Step 14: Build the firmware**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
```
Expected: all PASS.

- [ ] **Step 15: Lint and format the host workspace**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo fmt --all
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```
Expected: all PASS.

- [ ] **Step 16: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-internal crates/pico-de-gallo-firmware \
        crates/pico-de-gallo-lib/src/lib.rs Cargo.lock
git commit -F - <<'EOF'
feat(internal,firmware): Report the firmware build identity in device/info

Append `build_id` to `DeviceInfo` as a `heapless::String<64>` and
populate it from the `BUILD_ID` the firmware build script derives from
`git describe`. `postcard-schema` already implements `Schema` for
heapless strings, and heapless 0.9.3 is already in both lockfiles via
postcard-rpc, so this adds no new major to either dependency graph.

The field is informational and is never a compatibility gate.
`validate()` is deliberately untouched: the schema version answers
"can we talk?", and this field answers "are you the build I think you
are?". Those are separate questions, and conflating them would force a
dishonest schema bump for every behavioural change.

Delete two stale SCHEMA FREEZE markers. Both demanded a lockstep bump
that has since been performed and released; `internal-v0.7.0` is
tagged and already contains the changes they describe. Leaving false
markers beside a live one destroys the signal. Replace them with one
that names `build_id` and records that the next internal release must
be 0.8.0, not 0.7.1, because a new wire field is not a patch.

Because the field was appended after 0.7.0 shipped, a host built from
this tree cannot decode `device/info` from a released 0.11.0 firmware,
and `validate()` cannot warn since both peers still report schema 0.7.
Host and firmware must be built from the same tree until release.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 3: Pin the "never a gate" policy in `pico-de-gallo-lib`

**Files:**
- Modify: `crates/pico-de-gallo-lib/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/pico-de-gallo-lib/src/lib.rs`, in `mod tests`, add this after the
existing `check_schema_compatible_accepts_matching_versions` test:

```rust
    #[test]
    fn check_schema_compatible_ignores_build_id() {
        // `build_id` is informational: it names the image, it does not gate
        // compatibility. If this ever starts failing, someone has wired the
        // field into the compatibility policy, which would force a dishonest
        // schema bump for every behavioural change (issue #159).
        for id in ["", "unknown", "firmware-v0.11.0-27-gdeadbee-dirty"] {
            let mut info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
            info.build_id = id.try_into().unwrap();
            check_schema_compatible(&info)
                .unwrap_or_else(|e| panic!("build_id {id:?} must not gate: {e}"));
        }
    }

    #[test]
    fn device_info_exposes_build_id_accessor() {
        let mut info = make_device_info(SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR);
        info.build_id = "firmware-v0.11.0".try_into().unwrap();
        assert_eq!(info.build_id(), "firmware-v0.11.0");
    }
```

- [ ] **Step 2: Run the tests to verify they pass**

These should pass immediately — `check_schema_compatible` already ignores the
field. That is the point: the tests are regression guards, not drivers.

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-lib check_schema_compatible_ignores_build_id -- --nocapture
cargo test -p pico-de-gallo-lib device_info_exposes_build_id_accessor
```
Expected: both PASS.

- [ ] **Step 3: Verify the guard actually guards**

A regression test that cannot fail is worthless. Temporarily add this to
`check_schema_compatible` in `crates/pico-de-gallo-lib/src/lib.rs`:

```rust
    if info.build_id() != "expected" {
        return Err(ValidateError::LegacyFirmware);
    }
```

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-lib check_schema_compatible_ignores_build_id 2>&1 | tail -5
```
Expected: FAIL with `build_id "" must not gate`. **Now remove those three
lines again** and re-run to confirm PASS.

- [ ] **Step 4: Update the `validate()` rustdoc**

In `crates/pico-de-gallo-lib/src/lib.rs`, find the `validate()` doc comment
(around line 1114). Replace the stale paragraph that begins `/// Note: during
the schema freeze on this branch,` and references `0.6.1` with:

```rust
    /// This checks the reported numbers; it cannot make them trustworthy,
    /// and there are two distinct ways they can mislead.
    ///
    /// Wire *shape*: during the current unreleased schema freeze a
    /// matching schema version does not prove shape compatibility.
    /// `DeviceInfo` gained `build_id` after schema 0.7.0 was released, so
    /// this host cannot decode `device/info` from a released firmware
    /// 0.11.0 even though both peers report schema 0.7. Build host and
    /// firmware from the same tree until the 0.8.0 release.
    ///
    /// Wire *behaviour*: the schema version is derived from the wire
    /// crate's package version, so it is intended to track wire-type
    /// changes, not handler changes. Two firmware builds can report
    /// identical versions and still frame the bus differently. To
    /// identify the image, read
    /// [`DeviceInfo::build_id()`](method@pico_de_gallo_internal::DeviceInfo::build_id).
    /// It is informational only and never affects the outcome of this
    /// call.
```

- [ ] **Step 4b: Guard the dated claim with an executable expiry test**

The freeze warning added in Step 4 has an expiry date, and nothing else
forces anyone to revisit it — the paragraph it replaced was itself a stale
dated claim about an already-shipped 0.6.1 freeze. Add this to `mod tests`,
next to the other `build_id` tests:

```rust
    #[test]
    // Both operands are compile-time constants, which is exactly the point:
    // the assertion exists to fail the build's test run when the schema
    // reaches 0.8, not to check anything about runtime state.
    #[allow(clippy::assertions_on_constants)]
    fn validate_schema_freeze_rustdoc_must_be_revisited_before_schema_0_8() {
        // `validate()`'s rustdoc carries a DATED claim: that host and firmware
        // must be built from the same tree "until the 0.8.0 release". Nothing
        // else forces anyone to revisit it, and the paragraph it replaced was
        // itself a stale dated claim about a 0.6.1 freeze that had already
        // shipped. This test is the guard: it fails the moment the schema
        // reaches 0.8, so whoever cuts that release must delete or rewrite the
        // freeze warning rather than leaving public rustdoc quietly wrong.
        assert!(
            SCHEMA_VERSION_MAJOR == 0 && SCHEMA_VERSION_MINOR < 8,
            "validate() rustdoc says host and firmware must be built from the \
             same tree only until schema 0.8.0; remove or rewrite that dated \
             freeze warning as part of the 0.8.0 release"
        );
    }
```

Prove the guard can fire: temporarily change `< 8` to `< 0`, run
`cargo test -p pico-de-gallo-lib validate_schema_freeze`, observe the
failure, then restore `< 8` and confirm it passes.

- [ ] **Step 4c: Add the matching release-checklist item**

Defense in depth, so a human sees it too. Add a numbered step to the
"Step-by-step: cutting a release" list in `.github/RELEASE.md` (renumbering
the following steps to match the file's existing style):

```markdown
5. **If this release bumps the schema minor, revisit the dated
   wire-shape freeze warning** in `PicoDeGallo::validate()`'s rustdoc
   (`crates/pico-de-gallo-lib/src/lib.rs`) and remove or rewrite it.
   The `validate_schema_freeze_rustdoc_must_be_revisited_before_schema_0_8`
   test fails until this is done, so a forgotten warning blocks the
   release rather than shipping stale public documentation.
```

- [ ] **Step 5: Verify the docs build cleanly**

The field and the accessor share a name, so a bare intra-doc link would be
ambiguous and fail the doc job.

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc -p pico-de-gallo-lib \
    -p pico-de-gallo-internal --no-deps --all-features 2>&1 | tail -20
```
Expected: PASS with **no** `ambiguous link` warnings.

- [ ] **Step 6: Run the full lib test suite and lint**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-lib --locked
cargo clippy -p pico-de-gallo-lib --all-targets --locked -- -D warnings
cargo fmt --check
```
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-lib/src/lib.rs
git commit -F - <<'EOF'
test(lib): Pin build_id as informational, never a compatibility gate

`check_schema_compatible` already ignores `build_id`, which is the
intended policy but was not enforced by anything. Add regression tests
that fail if someone wires the field into the compatibility decision,
verified by temporarily introducing that bug and watching them fail.

Conflating the two would be actively harmful: the schema version
answers "can we talk?" and must move only when the wire types change,
so gating on build identity would force a dishonest schema bump for
every behavioural change.

Also replace the stale `validate()` note, which described a schema
freeze at 0.6.1 that has since been released, with a pointer to
`DeviceInfo::build_id()` as the way to answer the question `validate()
cannot.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 4: Render `gallo version` with `tabled`

**Files:**
- Modify: `crates/pico-de-gallo-app/src/lib.rs:786-833` (the `version` method)

- [ ] **Step 1: Write the failing test**

The current `version` method prints directly with `println!`, which is
untestable. The fix is to extract a pure rendering function.

In `crates/pico-de-gallo-app/src/lib.rs`, in `mod tests` (starts line 1586), add:

```rust
    fn sample_device_info() -> pico_de_gallo_lib::DeviceInfo {
        pico_de_gallo_lib::DeviceInfo {
            fw_major: 0,
            fw_minor: 11,
            fw_patch: 0,
            schema_major: 0,
            schema_minor: 7,
            schema_patch: 0,
            hw_version: 2,
            capabilities: pico_de_gallo_lib::Capabilities::I2C
                | pico_de_gallo_lib::Capabilities::SPI,
            num_gpios: 4,
            build_id: "firmware-v0.11.0-27-gdeadbee-dirty".try_into().unwrap(),
        }
    }

    #[test]
    fn render_device_info_reports_every_field() {
        let out = render_device_info(&sample_device_info());
        assert!(out.contains("v0.11.0"), "firmware version missing:\n{out}");
        assert!(out.contains("v0.7.0"), "schema version missing:\n{out}");
        assert!(out.contains("HW revision"), "hw revision row missing:\n{out}");
        assert!(out.contains("GPIOs"), "gpio count row missing:\n{out}");
        assert!(
            out.contains("firmware-v0.11.0-27-gdeadbee-dirty"),
            "build id missing:\n{out}"
        );
    }

    #[test]
    fn render_device_info_marks_capabilities() {
        let out = render_device_info(&sample_device_info());
        // I2C and SPI are set; UART is not.
        assert!(out.contains('✓'), "no capability ticks:\n{out}");
        assert!(out.contains('✗'), "no capability crosses:\n{out}");
        assert!(out.contains("1-Wire"), "capability header missing:\n{out}");
    }

    #[test]
    fn render_device_info_shows_dirty_marker_verbatim() {
        // The `-dirty` suffix is the most valuable part of the build id for a
        // bisecting developer, so make sure nothing strips or reformats it.
        let out = render_device_info(&sample_device_info());
        assert!(out.contains("-dirty"), "dirty marker lost:\n{out}");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo render_device_info 2>&1 | tail -10
```
Expected: FAIL to compile — `render_device_info` not found.

- [ ] **Step 3: Write the rendering function**

In `crates/pico-de-gallo-app/src/lib.rs`, add this as a free function at module
level (outside the `impl Cli` block, near the other helpers):

```rust
/// Render `device/info` as two tables.
///
/// Pure and returning `String` rather than printing, so the formatting is
/// testable. Two tables rather than one because capabilities are naturally a
/// wide boolean row; packing seven ticks into a single value cell is what the
/// previous hand-rolled output did badly.
fn render_device_info(info: &DeviceInfo) -> String {
    use pico_de_gallo_lib::Capabilities;

    let mut summary = Builder::with_capacity(5, 2);
    summary.push_record([
        "Firmware".to_string(),
        format!("v{}.{}.{}", info.fw_major, info.fw_minor, info.fw_patch),
    ]);
    summary.push_record([
        "Schema".to_string(),
        format!(
            "v{}.{}.{}",
            info.schema_major, info.schema_minor, info.schema_patch
        ),
    ]);
    summary.push_record(["HW revision".to_string(), info.hw_version.to_string()]);
    summary.push_record(["GPIOs".to_string(), info.num_gpios.to_string()]);
    // Last, mirroring `build_id` being the last wire field.
    summary.push_record(["Build".to_string(), info.build_id().to_string()]);

    let mut summary = summary.build();
    summary.with(Style::rounded());

    let caps = [
        ("I2C", Capabilities::I2C),
        ("SPI", Capabilities::SPI),
        ("UART", Capabilities::UART),
        ("GPIO", Capabilities::GPIO),
        ("PWM", Capabilities::PWM),
        ("ADC", Capabilities::ADC),
        ("1-Wire", Capabilities::ONEWIRE),
    ];

    let mut caps_table = Builder::with_capacity(2, caps.len());
    caps_table.push_record(caps.iter().map(|(name, _)| (*name).to_string()));
    caps_table.push_record(caps.iter().map(|(_, flag)| {
        if info.capabilities.contains(*flag) {
            "✓".to_string()
        } else {
            "✗".to_string()
        }
    }));

    let mut caps_table = caps_table.build();
    caps_table.with(Style::rounded());

    format!("{summary}\n{caps_table}")
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo render_device_info 2>&1 | tail -10
```
Expected: 3 tests PASS.

- [ ] **Step 5: Wire it into the `version` command**

In `crates/pico-de-gallo-app/src/lib.rs`, in the `version` method, replace the
entire `Ok(info) => { ... }` arm body (the five `println!` calls and the
`status` closure, currently lines 789-816) with:

```rust
            Ok(info) => {
                println!("{}", render_device_info(&info));
                Ok(())
            }
```

Leave the `Err(_) =>` legacy-firmware arm exactly as it is: that branch has only
a version number and one explanatory sentence, and exists for firmware too old
to have `device/info` at all, so a one-row table would be worse.

- [ ] **Step 6: Verify the whole crate still builds and lints**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo --locked
cargo clippy -p gallo --all-targets --locked -- -D warnings
cargo fmt --check
```
Expected: all PASS. If clippy complains about an unused `Alignment` or `Rows`
import, that means the `i2c scan` code no longer needs it — do **not** remove
those imports; `i2c_scan` still uses them.

- [ ] **Step 7: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-app/src/lib.rs
git commit -F - <<'EOF'
feat(application): Show the firmware build identity in gallo version

Print the build identity so a developer can tell which image is
actually flashed, which the firmware and schema versions cannot
answer on their own.

Rebuild the output on `tabled`, which the crate already depends on for
`i2c scan`, leaving one table idiom rather than two. `version` was the
last hand-rolled `println!` block. Render into a `String` from a pure
function so the formatting is covered by tests instead of eyeballed.

Also print the GPIO count, which is on the wire type and on the Python
class but has never been shown by the CLI.

The legacy-firmware fallback keeps plain `println!`: it has only a
version number and one explanatory sentence, and exists for firmware
predating `device/info` entirely.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 5: Surface the build identity in `gallo-mcp`

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/device.rs` (`StatusResult`, `build_status`, `status`)
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs` (`connect`)

- [ ] **Step 1: Write the failing test**

In `crates/pico-de-gallo-mcp/src/device.rs`, in the existing `mod tests` block
(near line 252), add:

```rust
    #[test]
    fn status_result_serializes_build_id() {
        let mut out = build_status(vec![Some("ABC123".to_string())], None, None);
        out.build_id = Some("firmware-v0.11.0-27-gdeadbee-dirty".to_string());
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(
            json["build_id"],
            serde_json::json!("firmware-v0.11.0-27-gdeadbee-dirty")
        );
    }

    #[test]
    fn status_result_build_id_is_null_before_connecting() {
        // `build_status` runs before any connection, so it cannot know the
        // build identity. It must say null rather than inventing one.
        let out = build_status(vec![Some("ABC123".to_string())], None, None);
        assert!(out.build_id.is_none());
        let json = serde_json::to_value(&out).unwrap();
        assert!(json.get("build_id").is_some(), "field must still be present");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo-mcp status_result_ 2>&1 | tail -10
```
Expected: FAIL to compile — `StatusResult` has no field `build_id`.

- [ ] **Step 3: Add the field to `StatusResult`**

In `crates/pico-de-gallo-mcp/src/device.rs`, add to the `StatusResult` struct as
the last field, after `schema_minor: Option<u16>,`:

```rust
    /// Firmware build identity (`git describe`), null until a board is
    /// reached. Informational: it names the running image, and never affects
    /// whether a call succeeds.
    build_id: Option<String>,
```

- [ ] **Step 4: Initialize it in `build_status`**

In the same file, in `build_status`, add to the returned `StatusResult` literal
as the last field, after `schema_minor: None,`:

```rust
        build_id: None,
```

- [ ] **Step 5: Populate it in the `status` tool**

In the same file, in the `status` method, inside the `Ok(dev) => { ... }` arm,
add after the `out.schema_minor = Some(info.schema_minor);` line:

```rust
                    out.build_id = Some(info.build_id().to_string());
```

This reads the **cached** `dev.info()` captured at connect-time validation, so
it costs no extra RPC.

- [ ] **Step 6: Run the tests to verify they pass**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo-mcp status_result_ 2>&1 | tail -10
```
Expected: 2 tests PASS.

- [ ] **Step 7: Log the build identity on connect**

In `crates/pico-de-gallo-mcp/src/lib.rs`, in `connect`, immediately after the
line `let info = inner.validate().await.map_err(error::map_validate_err)?;`
(currently line 420), add:

```rust
        // Put the build identity in the transcript on every connect. An
        // agent-driven session otherwise has no record of which image it
        // talked to, and the schema version cannot supply one: two builds can
        // report the same version and behave differently (issue #159).
        tracing::info!(
            serial = serial.as_deref().unwrap_or("<none>"),
            firmware = %format_args!("{}.{}.{}", info.fw_major, info.fw_minor, info.fw_patch),
            build_id = info.build_id(),
            "connected"
        );
```

- [ ] **Step 8: Verify the whole crate builds, lints and tests**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo-mcp --locked
cargo clippy -p gallo-mcp --all-targets --locked -- -D warnings
cargo fmt --check
```
Expected: all PASS. Note the 7 board-attached tests remain `#[ignore]`d and are
not expected to run.

- [ ] **Step 9: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-mcp/src/device.rs crates/pico-de-gallo-mcp/src/lib.rs
git commit -F - <<'EOF'
feat(mcp): Report the firmware build identity in status and on connect

`device_info` already serializes the whole wire struct, so it gained
the field for free. Add it to `status`, which is the tool an agent
reaches for first, reading the DeviceInfo cached at connect-time
validation so it costs no extra round trip.

Also log serial, firmware version and build identity once per connect.
An agent-driven session otherwise leaves no record of which image it
talked to, and the schema version cannot supply one, which is how a
hardware verification session previously reached a wrong conclusion.

The field is null until a board is actually reached, rather than
being invented: `build_status` runs before any connection exists.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 6: Expose the build identity through the C FFI

**Files:**
- Modify: `crates/pico-de-gallo-ffi/src/lib.rs` (`GalloDeviceInfo`, `gallo_get_device_info`)

- [ ] **Step 1: Write the failing test**

In `crates/pico-de-gallo-ffi/src/lib.rs`, in `mod tests`, add:

```rust
    #[test]
    fn gallo_build_id_len_matches_wire_capacity() {
        // cbindgen folds const initializers syntactically, so the array bound
        // must be a literal. This assertion is what stops the literal from
        // drifting away from the wire capacity.
        assert_eq!(GALLO_BUILD_ID_LEN, lib::BUILD_ID_CAPACITY + 1);
    }

    #[test]
    fn write_build_id_nul_terminates() {
        let mut buf = [0xAA_u8 as c_char; GALLO_BUILD_ID_LEN];
        write_build_id(&mut buf, "abc");
        assert_eq!(buf[0], b'a' as c_char);
        assert_eq!(buf[1], b'b' as c_char);
        assert_eq!(buf[2], b'c' as c_char);
        assert_eq!(buf[3], 0, "must be NUL terminated");
    }

    #[test]
    fn write_build_id_handles_maximum_length() {
        // A full-capacity id must fit with room for the terminator, and must
        // not run off the end of the array.
        let full = "0123456789012345678901234567890123456789012345678901234567890123";
        assert_eq!(full.len(), lib::BUILD_ID_CAPACITY);
        let mut buf = [0xAA_u8 as c_char; GALLO_BUILD_ID_LEN];
        write_build_id(&mut buf, full);
        assert_eq!(buf[lib::BUILD_ID_CAPACITY], 0, "terminator must be present");
        for (i, b) in full.bytes().enumerate() {
            assert_eq!(buf[i], b as c_char);
        }
    }

    #[test]
    fn write_build_id_handles_empty() {
        let mut buf = [0xAA_u8 as c_char; GALLO_BUILD_ID_LEN];
        write_build_id(&mut buf, "");
        assert_eq!(buf[0], 0);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-ffi build_id 2>&1 | tail -10
```
Expected: FAIL to compile — `GALLO_BUILD_ID_LEN` and `write_build_id` not found.

- [ ] **Step 3: Add the length constant**

In `crates/pico-de-gallo-ffi/src/lib.rs`, next to the existing
`GALLO_NUM_GPIOS` constant (around line 298), add:

```rust
/// Size of [`GalloDeviceInfo::build_id`], in bytes.
///
/// One more than `pico_de_gallo_internal::BUILD_ID_CAPACITY` to leave room for
/// the NUL terminator.
///
/// Written as a literal on purpose: cbindgen folds const initializers
/// syntactically and silently emits nothing for a computed value, so
/// `BUILD_ID_CAPACITY + 1` here would vanish from the generated header. The
/// assertion below is what keeps the literal honest.
pub const GALLO_BUILD_ID_LEN: usize = 65;
```

And add to the existing block of assertions just below (which already contains
`const _: () = assert!(GALLO_NUM_GPIOS == lib::NUM_GPIOS);`):

```rust
const _: () = assert!(GALLO_BUILD_ID_LEN == lib::BUILD_ID_CAPACITY + 1);
```

- [ ] **Step 4: Add the struct field**

In `crates/pico-de-gallo-ffi/src/lib.rs`, add to `GalloDeviceInfo` as the
**last** member, after `pub capabilities: u64,`:

```rust
    /// Firmware build identity, NUL-terminated.
    ///
    /// The firmware's `git describe --always --dirty --tags --match
    /// firmware-v*` output, or `"unknown"` when git was unavailable when the
    /// firmware was built. A trailing `-dirty` means the firmware was built
    /// from a modified working tree.
    ///
    /// Informational only: it names the running image and never affects
    /// whether a call succeeds.
    pub build_id: [c_char; GALLO_BUILD_ID_LEN],
```

Also update the struct's doc comment, changing
`schema (wire protocol) version, hardware revision, and peripheral
capabilities.` to
`schema (wire protocol) version, hardware revision, peripheral capabilities,
and the firmware build identity.`

- [ ] **Step 5: Add the writer helper**

In `crates/pico-de-gallo-ffi/src/lib.rs`, immediately above
`gallo_get_device_info`, add:

```rust
/// Copy `src` into a fixed C buffer and NUL-terminate it.
///
/// Truncates rather than overflowing if `src` somehow exceeds the buffer. That
/// cannot happen for a wire-decoded `build_id`, whose length is already bounded
/// by `BUILD_ID_CAPACITY`, but the bound is enforced here rather than assumed.
fn write_build_id(dst: &mut [c_char; GALLO_BUILD_ID_LEN], src: &str) {
    let n = src.len().min(GALLO_BUILD_ID_LEN - 1);
    for (slot, byte) in dst.iter_mut().zip(src.as_bytes()[..n].iter()) {
        *slot = *byte as c_char;
    }
    dst[n] = 0;
}
```

- [ ] **Step 6: Populate the field**

In `gallo_get_device_info`, inside the `unsafe { ... }` block, after
`(*out).capabilities = info.capabilities.bits();`, add:

```rust
                write_build_id(&mut (*out).build_id, info.build_id());
```

- [ ] **Step 7: Run the tests to verify they pass**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-ffi build_id 2>&1 | tail -10
```
Expected: 4 tests PASS.

- [ ] **Step 8: Verify the generated header contains the field**

cbindgen prunes anything not reachable from an exported prototype.
`GalloDeviceInfo` is reachable via `gallo_get_device_info`, so no
`cbindgen.toml` change should be needed — but verify rather than assume.

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo build -p pico-de-gallo-ffi 2>&1 | tail -3
find . -name 'pico_de_gallo.h' -newer crates/pico-de-gallo-ffi/src/lib.rs \
  -exec grep -n -A2 'build_id\|GALLO_BUILD_ID_LEN' {} +
```
Expected: the header shows `#define GALLO_BUILD_ID_LEN 65` and a
`char build_id[GALLO_BUILD_ID_LEN];` member inside `GalloDeviceInfo`.

If `GALLO_BUILD_ID_LEN` is **absent** from the header, the constant was pruned:
add `"GALLO_BUILD_ID_LEN"` to the `[export] include` list in
`crates/pico-de-gallo-ffi/cbindgen.toml` and re-run.

- [ ] **Step 9: Check the Zephyr consumer still compiles**

`zephyr/` links the FFI. Growing `GalloDeviceInfo` at the end should not break
it, but check that nothing there does a positional initialization.

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
rg -n 'GalloDeviceInfo' zephyr/
```
Expected: any hits either use designated initializers or only read fields. If a
positional initializer exists, add `.build_id = {0}` to it.

- [ ] **Step 10: Lint and format**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pico-de-gallo-ffi --locked
cargo clippy -p pico-de-gallo-ffi --all-targets --locked -- -D warnings
cargo fmt --check
```
Expected: all PASS.

- [ ] **Step 11: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-ffi
git commit -F - <<'EOF'
feat(ffi): Expose the firmware build identity in GalloDeviceInfo

Add `build_id` as a NUL-terminated `char[65]`, the last member of
`GalloDeviceInfo`, filled by `gallo_get_device_info`.

Inline rather than a separate accessor so C callers get the identity
from the call they already make. The struct grows only at the end, so
the layout change reaches only consumers that recompile, and `zephyr/`
recompiles in-tree.

The array bound is a literal because cbindgen folds const initializers
syntactically and would silently emit nothing for a computed value; a
const assertion ties it to `BUILD_ID_CAPACITY + 1` so the literal
cannot drift. This follows the existing `GALLO_NUM_GPIOS` precedent.

No new `Status` code: status values are stable C ABI and consumers are
told to write an exhaustive switch with no default, so a new value
would fall through. The field cannot fail independently of the call.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 7: Expose the build identity in the Python bindings

**Files:**
- Modify: `crates/pyco-de-gallo/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/pyco-de-gallo/src/lib.rs`, in `mod tests`, add after the existing
`device_info_conversion_carries_num_gpios` test:

```rust
    #[test]
    fn device_info_conversion_carries_build_id() {
        for id in ["", "unknown", "firmware-v0.11.0-27-gdeadbee-dirty"] {
            let mut info = lib_info(4);
            info.build_id = id.try_into().unwrap();
            let converted: DeviceInfo = info.into();
            assert_eq!(converted.build_id, id);
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pyco-de-gallo build_id 2>&1 | tail -10
```
Expected: FAIL to compile — `DeviceInfo` has no field `build_id`.

- [ ] **Step 3: Add the field to the Python class**

In `crates/pyco-de-gallo/src/lib.rs`, add to the `#[pyclass] struct DeviceInfo`
as the last field, after the `num_gpios: u8,` entry:

```rust
    /// Firmware build identity.
    ///
    /// The firmware's ``git describe --always --dirty --tags --match
    /// firmware-v*`` output captured at build time, or ``"unknown"`` when git
    /// was unavailable. A trailing ``-dirty`` means the firmware was built
    /// from a modified working tree.
    ///
    /// Informational only: it names the running image and never affects
    /// whether a call succeeds. Two firmware builds can report the same
    /// firmware and schema version and still behave differently, so this is
    /// the field to log when reproducing a result.
    #[pyo3(get)]
    build_id: String,
```

- [ ] **Step 4: Convert it**

In the same file, in `impl From<LibDeviceInfo> for DeviceInfo`, add as the last
entry after `num_gpios: info.num_gpios,`:

```rust
            build_id: info.build_id().to_string(),
```

- [ ] **Step 5: Run the test to verify it passes**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pyco-de-gallo build_id 2>&1 | tail -10
```
Expected: PASS.

- [ ] **Step 6: Lint, format and run the full crate suite**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p pyco-de-gallo --locked
cargo clippy -p pyco-de-gallo --all-targets --locked -- -D warnings
cargo fmt --check
```
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pyco-de-gallo/src/lib.rs
git commit -F - <<'EOF'
feat(pyco): Expose the firmware build identity on DeviceInfo

Add `build_id` to the Python `DeviceInfo` class, converted from the
wire type's accessor. The docstring is Google/Sphinx style so it
renders as the attribute's `__doc__`.

No `Clone` or `from_py_object` needed: `DeviceInfo` is only ever
returned to Python, never accepted as an argument.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 8: Documentation parity

AGENTS.md §15.1 makes this a blocker, not a nit: a PR that ships code without the
matching `book/` edits is incomplete.

**Files:**
- Modify: `book/src/internals/wire-protocol.md`
- Modify: `book/src/appendix/endpoints.md`
- Modify: `book/src/getting-started/verify.md`
- Modify: `book/src/internals/firmware.md`
- Modify: `book/src/crates/{lib,ffi,mcp,python}.md`
- Modify: `crates/pico-de-gallo-mcp/README.md`
- Modify: `crates/pico-de-gallo-{internal,lib,app,mcp,ffi,firmware}/CHANGELOG.md`, `crates/pyco-de-gallo/CHANGELOG.md`
- Modify: `AGENTS.md`

- [ ] **Step 1: Capture the real CLI output to paste into the book**

The book must describe the code that is on `main`, so copy real output rather
than inventing it. If no board is attached, render it from the test fixture
instead:

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo test -p gallo render_device_info_reports_every_field -- --nocapture 2>&1 | head -30
```

Use the actual table characters from that output.

- [ ] **Step 2: Update `book/src/internals/wire-protocol.md`**

In the `DeviceInfo` discussion (the section around the `num_gpios`
runtime-authoritative paragraph, ~lines 123-127), add after that paragraph:

```markdown
### Build identity

`DeviceInfo::build_id` carries the firmware's
`git describe --always --dirty --tags --match firmware-v*` output, captured when
the firmware was built, or `"unknown"` when git was unavailable. A trailing
`-dirty` means the image was built from a modified working tree.

It is **informational only and never a compatibility gate.** `validate()`
deliberately ignores it.

The two fields answer different questions, and conflating them would be a
mistake in both directions:

| Question | Field |
|---|---|
| *Can we talk?* | `schema_major` / `schema_minor` |
| *Are you the build I think you are?* | `build_id` |

The schema version is derived from `pico-de-gallo-internal`'s package version,
so it moves when the wire **types** change. Firmware behaviour can change while
the types do not — `i2c/batch` moved from per-operation START/STOP to a single
transaction inside an unchanged schema 0.7 — and bumping the schema version for
such a change would falsely signal a wire-format break. `build_id` covers that
gap without disturbing the compatibility axis.

The field is a `heapless::String<BUILD_ID_CAPACITY>` (64 bytes), encoded by
postcard as a plain varint-length string, so an empty value costs one byte.
Decoding an over-long string fails rather than truncating; the firmware build
script truncates.
```

- [ ] **Step 3: Update `book/src/appendix/endpoints.md`**

Change the `device/info` row (line 27) from:

```markdown
| device/info | Firmware version + schema version + capability bitfield + runtime GPIO count. |
```
to:
```markdown
| device/info | Firmware version + schema version + capability bitfield + runtime GPIO count + firmware build identity. |
```

- [ ] **Step 4: Update `book/src/getting-started/verify.md`**

The sample output block there is stale — it shows `FW v0.8.0` / `Schema v0.4.0`
and the old flat format. Replace the output block with the real two-table output
captured in Step 1, and add two rows to the "What Each Field Means" table:

```markdown
| `GPIOs` | Number of user-controllable GPIO pins this firmware exposes. This is the authoritative bound for a GPIO index and for an SPI chip-select pin. |
| `Build` | Firmware build identity from `git describe`. A trailing `-dirty` means the image was built from a modified working tree. Informational only — it never affects whether a command succeeds. Quote this when reporting a bug or reproducing a measurement. |
```

- [ ] **Step 5: Update `book/src/internals/firmware.md`**

Add a subsection describing the build-script behaviour. Note the outer fence
is four backticks because the content itself contains a fenced block:

````markdown
### Build identity

`crates/pico-de-gallo-firmware/build.rs` runs

```text
git describe --always --dirty --tags --match firmware-v*
```

and emits the result as a `BUILD_ID` constant, which `main()` logs at boot over
`defmt` and the `device/info` handler returns.

Every flag matters, and each has a *silent* failure mode:

| Flag | Removing it causes |
|---|---|
| `--tags` | The `firmware-v*` tags are a mix of annotated and lightweight. Without this, `git describe` sees only annotated tags and resolves hundreds of commits too far back. |
| `--match firmware-v*` | Describe picks the nearest tag in any namespace, returning an `application-v*` description. |
| `--always` | Describe fails outright when no matching tag is reachable, e.g. in a shallow CI clone. |
| `--dirty` | The locally-modified marker is lost — the single most useful part for a developer mid-bisect. |

When git is unavailable, exits non-zero, or there is no repository, the script
emits `"unknown"` and a `cargo:warning`. The build still succeeds, so a source
tarball remains buildable.

The script re-runs on **every** build, by design. The pre-existing
`rerun-if-changed=memory.x` narrows re-runs to a single file, so without an
unconditional trigger the embedded identity would go stale across incremental
builds and keep reporting a clean tree for a dirty one. A stale identity is
worse than none, because it confirms a wrong conclusion rather than merely
failing to prevent one.

Release workflows must fetch tags (`fetch-depth: 0`), or `git describe`
degrades to a bare commit hash.
````

- [ ] **Step 6: Update the four crate chapters and the MCP README**

- `book/src/crates/lib.md` — note `DeviceInfo::build_id()` and that `validate()` ignores it.
- `book/src/crates/ffi.md` — add `build_id` to the `GalloDeviceInfo` description and mention `GALLO_BUILD_ID_LEN`.
- `book/src/crates/mcp.md` and `crates/pico-de-gallo-mcp/README.md` — `status` gains `build_id`; the server logs it on connect.
- `book/src/crates/python.md` — `DeviceInfo.build_id`.

- [ ] **Step 7: Add CHANGELOG entries**

For each of `crates/pico-de-gallo-internal`, `pico-de-gallo-lib`,
`pico-de-gallo-app`, `pico-de-gallo-mcp`, `pico-de-gallo-ffi`,
`pyco-de-gallo`, `pico-de-gallo-firmware`, insert an `## [Unreleased]` section
immediately below the Keep-a-Changelog preamble and above the current top
release heading. No `[package].version` changes.

For `pico-de-gallo-internal/CHANGELOG.md`:

```markdown
## [Unreleased]

### Breaking Changes

- Appended `DeviceInfo::build_id`, a `heapless::String<64>` carrying the
  firmware's `git describe --always --dirty --tags --match firmware-v*`
  output, or `"unknown"` when git was unavailable. The field is
  informational and is never a compatibility gate: `validate()` ignores
  it. Closes #159.

  This is append-only on the wire, but it landed **after** schema 0.7.0
  shipped, so a host built from this tree cannot decode `device/info`
  from a released firmware 0.11.0 — postcard hits end-of-input on the new
  field — and `validate()` cannot warn, because both peers still report
  schema 0.7. **Host and firmware must be built from the same tree until
  the next release.** That release must be 0.8.0, not 0.7.1: a new wire
  field is not a patch.

### Added

- `BUILD_ID_CAPACITY` (64) and the `DeviceInfo::build_id()` accessor, so
  host crates never need to name `heapless` themselves.
```

Write the corresponding shorter entries for the other six crates, each
describing only that crate's surface.

- [ ] **Step 8: Add the AGENTS.md regression row**

In `AGENTS.md` §13.17, append a row to the table:

```markdown
| 2026-09-01 | Two firmware builds reporting the same schema version behaving differently on the wire — `i2c/batch` framing (2026-08-26) and the zero-length-write guard (2026-08-26), the latter of which **misidentified a flash during its own hardware verification**. | `validate()` compares schema versions only, and `SCHEMA_VERSION_*` is derived from `pico-de-gallo-internal`'s package version, so it tracks type changes rather than handler behaviour. Nothing distinguished the two images, and the recorded mitigation was "track the flashed image yourself", which is not a mitigation. | Appended `DeviceInfo::build_id` (`heapless::String<64>`), generated by the firmware `build.rs` from `git describe --always --dirty --tags --match firmware-v*` and surfaced by `gallo version`, `gallo-mcp` `status` / `device_info` / connect log, the FFI `GalloDeviceInfo`, and `pyco-de-gallo`. Informational only — never a gate, with a regression test that fails if anyone wires it into `check_schema_compatible`. Three traps recorded in `build.rs` comments: without `--tags` describe sees only annotated tags and resolves 302 commits too far back, since the `firmware-v*` tags are a mix of annotated and lightweight; without `--match` it returns an `application-v*` description; and the pre-existing `rerun-if-changed=memory.x` narrows re-runs to one file, so the script must be forced to re-run unconditionally or the embedded identity goes stale and reports a clean tree for a dirty one. Also deleted two stale `SCHEMA FREEZE` markers whose bump had already been released as `internal-v0.7.0`; they caused a wrong conclusion during this very design session. Issue #159. |
```

- [ ] **Step 9: Verify the book builds**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
mdbook build book 2>&1 | tail -20
```
Expected: PASS, no broken links or missing files.

- [ ] **Step 10: Confirm LF endings on every edited file**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
git diff --cached --name-only | xargs -r file | grep -i crlf || echo "all LF"
```
Expected: `all LF`.

- [ ] **Step 11: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add book AGENTS.md crates/*/CHANGELOG.md crates/pico-de-gallo-mcp/README.md
git commit -F - <<'EOF'
docs(repo): Document the firmware build identity

Cover the new `device/info` field across the book, the per-crate
CHANGELOGs, and the MCP README, per the book/code parity rule.

Explain why build identity and schema version are separate axes:
the schema version is derived from the wire crate's package version
and so tracks type changes, while firmware behaviour can change with
the types unchanged. Bumping the schema for a behavioural change would
falsely signal a wire-format break, and not bumping it leaves two
indistinguishable images.

Record the three build-script traps: the `firmware-v*` tags are a mix
of annotated and lightweight so `--tags` is required, `--match` is
required to avoid an `application-v*` description, and the script must
re-run unconditionally or the embedded identity goes stale.

Refresh the stale `gallo version` sample in the verify chapter, which
still showed FW v0.8.0 / Schema v0.4.0 in the old flat format, and add
an AGENTS.md regression row.

Refs: #159

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
```

---

## Task 9: Full CI preflight

**Files:** none — verification only.

- [ ] **Step 1: Run the host workspace gates**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo check --workspace --locked
cargo deny --manifest-path Cargo.toml check
```
Expected: all PASS. Test count should be the ~589 baseline plus the roughly
14 tests added by this plan.

- [ ] **Step 2: Run the per-crate feature powerset**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-internal
cargo hack --feature-powerset check
```
Expected: PASS. This specifically exercises `pico-de-gallo-internal` **without**
`use-std`, i.e. the `no_std` path that the firmware uses.

- [ ] **Step 3: Check the MSRV**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-internal
cargo +1.90 check
```
Expected: PASS. `heapless` 0.9.3 declares `rust-version = "1.87"`, comfortably
under the repo's 1.90.

- [ ] **Step 4: Build both firmware revisions**

Both are published as release artifacts, so a preflight that only exercises the
default leaves the deprecated-but-still-published image untested.

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1
cargo deny --manifest-path Cargo.toml check
```
Expected: all PASS.

- [ ] **Step 5: Build the docs**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
RUSTDOCFLAGS="--cfg docsrs" cargo +nightly doc --workspace --no-deps --all-features 2>&1 | tail -20
mdbook build book 2>&1 | tail -5
```
Expected: both PASS, with **no** ambiguous-intra-doc-link warnings (the
`build_id` field and `build_id()` method share a name).

- [ ] **Step 6: Confirm no version was bumped**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
git diff upstream/main --stat -- '*/Cargo.toml'
git diff upstream/main -- '*/Cargo.toml' | grep -E '^[-+]version' || echo "no version changes"
```
Expected: `no version changes`. The only `Cargo.toml` diff should be the
`heapless` dependency and the `postcard-schema` feature in
`pico-de-gallo-internal`.

- [ ] **Step 7: Confirm both lockfiles are in sync**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo check --workspace --locked && echo "host lock OK"
cd crates/pico-de-gallo-firmware
cargo check --locked --target thumbv8m.main-none-eabihf && echo "firmware lock OK"
```
Expected: both `OK`.

- [ ] **Step 8: Review the full diff**

Run:
```bash
cd /home/balbi/workspace/pico-de-gallo
git log --oneline upstream/main..HEAD
git diff upstream/main --stat
```
Expected: 8 commits (2 spec + 6 implementation) plus the docs commit. Confirm no
stray files, no `.pdg-always-rerun` file was created, and no generated header was
committed.

---

## Task 10: Hardware acceptance (manual, board required)

This is the issue's stated acceptance criterion. It **cannot** run in CI and
**must not** be skipped before claiming the issue is closed.

**Files:** none — verification only.

- [ ] **Step 1: Flash a clean build and record the identity**

```bash
cd /home/balbi/workspace/pico-de-gallo
git status --porcelain    # must be empty for a clean -dirty-free build
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
```
Flash the resulting image, then:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo -- version
```
Record the `Build` value. Expected: `firmware-v0.11.0-N-g<hash>` with **no**
`-dirty`.

- [ ] **Step 2: Make a behaviour-only change and rebuild**

Edit any firmware handler in a way that changes behaviour but no wire type — for
example add a `defmt::info!` line to `crates/pico-de-gallo-firmware/src/handlers/info.rs`.
Do **not** commit it. Rebuild and reflash:
```bash
cd /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
```

- [ ] **Step 3: Verify the two builds are now distinguishable**

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo -- version
```
Expected: the `Build` value now ends in `-dirty` and therefore **differs** from
Step 1, while `Firmware` and `Schema` are unchanged.

**This is the acceptance criterion.** Two images differing only in handler
behaviour now produce two distinguishable `device/info` responses, which is
precisely what `validate()` could not do.

- [ ] **Step 4: Verify the always-rerun trigger under incremental build**

This is the regression test for the staleness failure mode. Without Step 4 of
Task 1, this step reports a **stale** identity.

Revert the edit from Step 2, rebuild **without** cleaning, and re-check:
```bash
cd /home/balbi/workspace/pico-de-gallo
git checkout -- crates/pico-de-gallo-firmware/src/handlers/info.rs
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
```
Reflash, then:
```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo -- version
```
Expected: the `-dirty` suffix is **gone** without a `cargo clean`. If it is
still present, the always-rerun trigger is not working.

- [ ] **Step 5: Verify the MCP surfaces**

With a board attached, confirm the `status` tool reports `build_id`, and that
the server log contains a `connected` line carrying `build_id`.

- [ ] **Step 6: Record the results in the PR body**

State which board, which two build IDs were observed, and that Step 4 passed
without a clean. Per AGENTS.md, behavioural claims about hardware require the
manual procedure, and CI is not evidence for them.

---

## Post-implementation notes for the PR body

Include these explicitly — reviewers need them and they are easy to forget:

1. **No version bumps.** This PR deliberately moves no `[package].version`
   (AGENTS.md §4 rule 12). The maintainer cuts the lockstep 0.8.0 release
   separately.
2. **Same-tree requirement.** Until that release, a host built from this tree
   cannot decode `device/info` from released firmware 0.11.0, and `validate()`
   cannot warn. Build both sides from the same tree.
3. **Zephyr impact.** `GalloDeviceInfo` grew a member at the end.
   `zephyr.yml` is path-filtered on `crates/pico-de-gallo-ffi/`, so confirm it
   actually ran, and note that a green run proves it compiles and links, not
   that behaviour is unchanged.
4. **Stale markers removed.** Two `SCHEMA FREEZE` comments were deleted because
   their bump has already shipped as `internal-v0.7.0`; leaving them beside a
   live marker caused a wrong conclusion during design.
5. **Known gap.** Nothing automated covers the `build.rs` git invocation or the
   firmware handler; both need real git and real registers. Task 10 is the only
   coverage, and it is manual.
