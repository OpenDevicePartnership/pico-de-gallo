# Firmware WebUSB Descriptors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit a WebUSB BOS platform capability and landing-page URL descriptor from the Pico de Gallo firmware, closing issue #87, then cut firmware release 0.10.1.

**Architecture:** postcard-rpc currently builds the `embassy_usb::Builder` internally via `WireStorage::init()`. Switch to `init_without_build()`, which hands back the un-built `Builder`, call `embassy_usb::class::web_usb::WebUsb::configure()` on it to append the WebUSB platform capability, then `build()`. Because `WebUsb::configure()` appends a second class-`0xFF` interface, ordering is load-bearing and is the primary risk.

**Tech Stack:** Rust 1.90 `no_std`, `thumbv8m.main-none-eabihf` (RP2350), embassy-rp 0.10, embassy-usb 0.5.1, postcard-rpc 0.12.1, defmt.

**Spec:** `docs/superpowers/specs/2026-08-04-firmware-webusb-descriptors-design.md`

**Branch:** `feat/firmware-webusb-descriptors` (already created, currently at the two spec commits)

---

## Ground rules for this plan

Read these before starting. They are project policy from `AGENTS.md`
and they differ from typical Rust defaults.

1. **All firmware commands run from `crates/pico-de-gallo-firmware/`.**
   It is a *separate Cargo workspace*, excluded from the host workspace.
2. **`defmt` only.** No `println!`, no `log`. The crate is `no_std`.
3. **LF line endings** on every file touched.
4. **Exactly two commits.** Tasks 1-3 stage work without committing;
   Task 4 makes the `feat` commit; Task 5 makes the `chore(release)`
   commit. Do not commit in between — the spec (§9) and AGENTS.md §4
   rule 9 require each of these two commits to build cleanly on its own.
5. **AI attribution trailers on both commits**, and **never**
   `Signed-off-by:` (AGENTS.md §4 rule 7):
   ```text
   Assisted-by: OpenCode:claude-opus-5
   Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
   ```

## File structure

| File | Change | Responsibility |
|---|---|---|
| `crates/pico-de-gallo-firmware/src/main.rs` | Modify: imports near `:76`, constants after `:127`, USB init at `:449-454` | The entire functional change |
| `book/src/getting-started/usb.md` | Modify: new section after the macOS section (`:52`) | Per-OS user guidance, now including browsers |
| `book/src/internals/firmware.md` | Modify: new section after `:42`, before `## \`no_std\` and logging` | Descriptor-layout reference |
| `crates/pico-de-gallo-firmware/CHANGELOG.md` | Modify: insert after `:6`, before `## [0.10.0]` | Release notes |
| `crates/pico-de-gallo-firmware/Cargo.toml` | Modify: `:3` | Version bump (Task 5 only) |
| `crates/pico-de-gallo-firmware/Cargo.lock` | Regenerate | Records the firmware's own version at `:1018` (Task 5 only) |

No new files. No dependency changes — `embassy_usb::class::web_usb` is
compiled unconditionally in embassy-usb 0.5.1, behind no Cargo feature.

---

### Task 1: Add the WebUSB descriptors to the firmware

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/main.rs`

There is no unit-test harness here — the crate is `no_std` and
cross-compiled, so `cargo test` cannot run it. Instead we get a genuine
red/green cycle by inspecting the built ELF for two byte patterns that
can only appear if the descriptors were emitted: the WebUSB platform
capability UUID and the landing-page URL string.

- [ ] **Step 1: Write the failing check and confirm it fails**

Run from `crates/pico-de-gallo-firmware/`:

```bash
cargo build --release --locked --target thumbv8m.main-none-eabihf
ELF=target/thumbv8m.main-none-eabihf/release/pico-de-gallo-firmware
echo "URL  matches: $(strings $ELF | grep -c 'balbi.sh/pico-de-gallo')"
echo "UUID matches: $(xxd -p $ELF | tr -d '\n' | grep -o '38b60834a909a0478bfda0768815b665' | wc -l)"
```

Expected (RED — this is the pre-change baseline, already verified):

```text
URL  matches: 0
UUID matches: 0
```

The UUID pattern is the WebUSB `PlatformCapabilityUUID`
`3408b638-09a9-47a0-8bfd-a0768815b665` in the little-endian byte order
embassy-usb writes it (`embassy-usb-0.5.1/src/class/web_usb.rs:152-154`).

- [ ] **Step 2: Add the import**

In `crates/pico-de-gallo-firmware/src/main.rs`, next to the existing
`use embassy_usb::{Config, UsbDevice};` at line 76, add:

```rust
use embassy_usb::class::web_usb::{Config as WebUsbConfig, State as WebUsbState, Url, WebUsb};
```

The `as` renames are mandatory: `embassy_usb::Config` is already
imported unaliased on line 76 and `web_usb::Config` would collide.

Do not hand-place it in sort order — `cargo fmt` in Step 6 will order it.

- [ ] **Step 3: Import `USB_FS_MAX_PACKET_SIZE`**

It is currently referenced fully-qualified once, at line 453. After this
task it is used twice, so import it. Change the `postcard_rpc` use block
at lines 87-97 so the `impls::embassy_usb_v0_5` group reads:

```rust
        impls::embassy_usb_v0_5::{
            PacketBuffers, USB_FS_MAX_PACKET_SIZE,
            dispatch_impl::{WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorage, WireTxImpl},
        },
```

- [ ] **Step 4: Add the two constants**

Insert immediately after the `HW_VERSION` block that ends at line 127
(i.e. before the `/// USB driver type for the RP2350.` comment):

```rust
/// WebUSB landing page advertised in the URL descriptor.
///
/// Chrome surfaces this as a notification when the device is plugged in.
/// [`Url::new`] strips the scheme prefix and asserts that the remainder is at
/// most 252 bytes.
const WEBUSB_LANDING_URL: &str = "https://balbi.sh/pico-de-gallo/";

/// `bRequest` value for WebUSB vendor control transfers.
///
/// Must not be `0`. postcard-rpc registers the Microsoft OS 2.0 descriptor at
/// vendor code `0`, and embassy-usb answers that request before dispatching to
/// user handlers, so a `0` here would shadow the WebUSB `GET_URL` handler.
const WEBUSB_VENDOR_CODE: u8 = 0x01;
```

- [ ] **Step 5: Replace the USB device construction**

Replace lines 449-454 exactly. The current code is:

```rust
    let (device, tx_impl, rx_impl) = STORAGE.init(
        driver,
        config,
        pbufs.tx_buf.as_mut_slice(),
        postcard_rpc::server::impls::embassy_usb_v0_5::USB_FS_MAX_PACKET_SIZE,
    );
```

Replace it with:

```rust
    let (mut builder, tx_impl, rx_impl) =
        STORAGE.init_without_build(driver, config, pbufs.tx_buf.as_mut_slice(), USB_FS_MAX_PACKET_SIZE);

    // Advertise WebUSB so browsers can discover the device and surface the
    // landing page.
    //
    // This MUST run after init_without_build(): postcard-rpc's bulk interface
    // has to stay interface 0, because every host transport selects the FIRST
    // class-0xFF interface -- nusb on Linux/macOS (raw_nusb.rs:95), WinUSB on
    // Windows (hardcoded 0, raw_nusb.rs:101), and postcard-rpc's own WebUSB
    // transport (webusb.rs:194). WebUsb::configure() appends a second,
    // endpoint-less class-0xFF interface; if it ran first, the browser would
    // fail with "Failed to find usable interface" and Linux/macOS would
    // silently claim the wrong one.
    static WEBUSB_STATE: StaticCell<WebUsbState<'static>> = StaticCell::new();
    static WEBUSB_CONFIG: StaticCell<WebUsbConfig<'static>> = StaticCell::new();
    let webusb_config = WEBUSB_CONFIG.init(WebUsbConfig {
        // Inert: WebUsb::configure() creates no endpoints, and embassy-usb
        // 0.5.1 never reads this field. It has no Default, so it must be set.
        max_packet_size: USB_FS_MAX_PACKET_SIZE as u16,
        landing_url: Some(Url::new(WEBUSB_LANDING_URL)),
        vendor_code: WEBUSB_VENDOR_CODE,
    });
    WebUsb::configure(&mut builder, WEBUSB_STATE.init(WebUsbState::new()), webusb_config);

    let device = builder.build();
```

Note `mut builder` — `WebUsb::configure` takes `&mut Builder`. The
function-local `static` declarations match the existing style in this
file (see `SERIAL_STRING` at line 248). `device` keeps its name so line
465's `spawner.must_spawn(usb_task(device));` is unchanged.

- [ ] **Step 6: Format**

```bash
cargo fmt
git diff --stat
```

Expected: only `src/main.rs` modified. Accept whatever import ordering
`cargo fmt` produces.

- [ ] **Step 7: Run the check again to verify it passes**

```bash
cargo build --release --locked --target thumbv8m.main-none-eabihf
ELF=target/thumbv8m.main-none-eabihf/release/pico-de-gallo-firmware
echo "URL  matches: $(strings $ELF | grep -c 'balbi.sh/pico-de-gallo')"
echo "UUID matches: $(xxd -p $ELF | tr -d '\n' | grep -o '38b60834a909a0478bfda0768815b665' | wc -l)"
```

Expected (GREEN):

```text
URL  matches: 1
UUID matches: 1
```

If either is still `0`, the descriptor is not being emitted — do not
proceed. If a count is greater than 1, that is also fine (the linker may
retain more than one copy); the assertion is "at least one".

- [ ] **Step 8: Lint, both hardware revisions**

```bash
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2 -- -D warnings
```

Expected: clean exit, no warnings.

If clippy flags `USB_FS_MAX_PACKET_SIZE as u16` (it should not —
`cast_possible_truncation` is a `pedantic` lint and is not enabled here),
replace the cast with `u16::try_from(USB_FS_MAX_PACKET_SIZE).unwrap()`
rather than adding an `#[allow]`.

- [ ] **Step 9: Build hw-rev2**

```bash
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2
```

Expected: `Finished \`release\` profile`.

**Do not commit yet.** Tasks 2-4 stage into the same commit.

---

### Task 2: Document the browser path in the USB & OS notes

**Files:**
- Modify: `book/src/getting-started/usb.md`

This page has one section per OS (Linux `:8`, Windows `:37`, macOS
`:50`) followed by `## Troubleshooting` at `:59`. Add a browser section
between macOS and Troubleshooting.

- [ ] **Step 1: Insert the new section**

Insert after line 57 (the end of the macOS section, the paragraph ending
"try the pre-built release artifact.") and before `## Troubleshooting`:

```markdown
## Browsers (WebUSB)

The firmware advertises a WebUSB platform capability in its BOS
descriptor, along with a landing-page URL. Chrome and Edge use this to
show a notification pointing at
<https://balbi.sh/pico-de-gallo/> when you plug the device in.

Two browser requirements are worth knowing up front, because neither is
something the firmware can influence:

- **Secure context.** `navigator.usb` only exists on pages served over
  HTTPS, or on `localhost`.
- **User gesture.** `navigator.usb.requestDevice()` must be called from
  a click or keypress handler. There is no way to connect
  automatically on page load; the user picks the device from a browser
  dialog every time a new origin asks.

```js
const device = await navigator.usb.requestDevice({
  filters: [{ vendorId: 0x045e, productId: 0x067d }],
});
await device.open();
await device.claimInterface(0);
```

Interface `0` is the vendor-specific interface carrying the two bulk
endpoints that the RPC protocol uses. The device also exposes a second,
endpoint-less vendor interface that exists only to carry the WebUSB
capability descriptor — do not claim it.

> [!NOTE]
>
> These descriptors are a convenience, not a gate. WebUSB can talk to
> any vendor-class device, so a browser could already reach Pico de
> Gallo without them. What they add is the landing-page notification
> and explicit signalling that the device is WebUSB-aware.

The same OS-level permissions still apply: a Linux udev rule is
required (see above), and on Windows the device must be bound to
WinUSB, which the Microsoft OS 2.0 descriptor handles automatically.
```

- [ ] **Step 2: Verify the book builds**

```bash
cd /home/balbi/workspace/pico-de-gallo
mdbook build book
```

Expected: completes with no error and no broken-link warnings.

If `mdbook` is not installed, install it with
`cargo install mdbook --locked` — CI builds the book on every PR via
`.github/workflows/gh-pages.yml`, so this must pass.

**Do not commit yet.**

---

### Task 3: Document the descriptor layout in the firmware internals chapter

**Files:**
- Modify: `book/src/internals/firmware.md`

The page currently mentions USB once, in passing, at line 14. The
Watchdog subsection ends at line 42; `## \`no_std\` and logging` begins
at line 44.

- [ ] **Step 1: Insert the new section**

Insert after line 42 and before `## \`no_std\` and logging`:

```markdown
## USB descriptors

The device enumerates as VID `045e` / PID `067d` with
`bDeviceClass = 0xFF` (vendor-specific), so no OS class driver claims
it. `embassy_usb::Config` defaults `bcdUSB` to `0x0210`, which is what
makes hosts request the BOS descriptor at all.

### Interfaces

| # | Class | Endpoints | Purpose |
|---|-------|-----------|---------|
| 0 | `0xFF` | bulk IN + bulk OUT | The postcard-rpc transport |
| 1 | `0xFF` | none | Carries the WebUSB capability only |

Interface 0 **must** stay first. Every host transport picks the first
class-`0xFF` interface it finds, so reordering these silently breaks
all of them. Interface 1 exists because `WebUsb::configure()` cannot
append a capability to an existing interface — `bos_capability()` is
only reachable through an `InterfaceAltBuilder`, and embassy-usb
exposes no public `Builder::bos_writer()`.

### BOS capabilities

| Capability | Bytes | Written by |
|---|---:|---|
| BOS header | 5 | embassy-usb, automatically |
| `USB_2_0_EXTENSION` | 7 | embassy-usb, automatically |
| `PLATFORM` — Microsoft OS 2.0 | 28 | postcard-rpc |
| `PLATFORM` — WebUSB | 24 | firmware, via `WebUsb::configure()` |

That is 64 bytes of the 256-byte BOS buffer sized by the `WireStorage`
type alias in `main.rs`.

The MS OS 2.0 capability is what makes Windows bind WinUSB without an
INF file. The WebUSB capability advertises the landing page described
in [USB & OS Notes](../getting-started/usb.md).

### Vendor codes

Both platform capabilities reserve a `bRequest` value for
vendor-specific control transfers on the device:

| Descriptor | Vendor code |
|---|---|
| Microsoft OS 2.0 | `0x00` (set by postcard-rpc) |
| WebUSB | `0x01` (`WEBUSB_VENDOR_CODE` in `main.rs`) |

These must differ. embassy-usb answers any vendor/device request
matching the MS OS 2.0 vendor code *before* dispatching to registered
handlers, so giving WebUSB `0x00` would shadow its `GET_URL` handler
and the landing page would never be served.
```

- [ ] **Step 2: Verify the book builds**

```bash
cd /home/balbi/workspace/pico-de-gallo
mdbook build book
```

Expected: completes with no error and no broken-link warnings. The
relative link `../getting-started/usb.md` must resolve.

**Do not commit yet.**

---

### Task 4: Write the CHANGELOG entry and make the feature commit

**Files:**
- Modify: `crates/pico-de-gallo-firmware/CHANGELOG.md`

The file has no `## [Unreleased]` section; line 6 is the "based on Keep
a Changelog" line, line 7 is blank, and line 8 is
`## [0.10.0] — 2026-06-22`.

- [ ] **Step 1: Insert the Unreleased section**

Insert after line 7 (the blank line) and before `## [0.10.0] — 2026-06-22`:

```markdown
## [Unreleased]

### Added

- WebUSB platform capability in the BOS descriptor, plus a
  landing-page URL descriptor pointing at
  <https://balbi.sh/pico-de-gallo/>. Browsers now surface a
  notification when the device is plugged in and can discover it as a
  WebUSB-aware device. Closes #87.

  The USB device is now built through postcard-rpc's
  `WireStorage::init_without_build()` so that
  `embassy_usb::class::web_usb::WebUsb::configure()` can append the
  capability before `Builder::build()`. This adds a second,
  endpoint-less vendor-class interface; postcard-rpc's bulk interface
  remains interface 0, which all host transports depend on. No new
  dependencies and no wire-protocol change.

```

- [ ] **Step 2: Confirm LF endings on every file touched**

```bash
cd /home/balbi/workspace/pico-de-gallo
for f in crates/pico-de-gallo-firmware/src/main.rs \
         crates/pico-de-gallo-firmware/CHANGELOG.md \
         book/src/getting-started/usb.md \
         book/src/internals/firmware.md; do
  printf '%-50s CRLF=%s\n' "$f" "$(grep -c $'\r' "$f" || true)"
done
```

Expected: `CRLF=0` on all four. If any is non-zero, run `dos2unix` on it
(AGENTS.md §3).

- [ ] **Step 3: Re-run the full firmware gate**

From `crates/pico-de-gallo-firmware/`:

```bash
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2
```

Expected: all five commands exit 0 with no warnings.

- [ ] **Step 4: Review the staged diff before committing**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-firmware/src/main.rs \
        crates/pico-de-gallo-firmware/CHANGELOG.md \
        book/src/getting-started/usb.md \
        book/src/internals/firmware.md
git status --porcelain
git --no-pager diff --cached --stat
```

Expected: exactly four files staged. `Cargo.toml` and `Cargo.lock` must
**not** appear — the version bump is Task 5.

- [ ] **Step 5: Commit**

```bash
git commit -F - <<'EOF'
feat(firmware): Add WebUSB BOS platform capability

Emit a WebUSB platform capability in the BOS descriptor along with a
landing-page URL descriptor, so browsers can discover the device and
Chrome surfaces a notification on plug-in.

The USB device is now built through postcard-rpc's
init_without_build(), which returns the un-built embassy_usb::Builder,
so WebUsb::configure() can append the capability before build().
embassy-usb 0.5.1 ships class::web_usb unconditionally, so this needs
no new dependency and leaves both Cargo.lock files untouched.

WebUsb::configure() cannot append a capability to an existing
interface -- bos_capability() is only reachable through an
InterfaceAltBuilder and there is no public Builder::bos_writer() -- so
it allocates a second, endpoint-less class-0xFF interface. Call
ordering is therefore load-bearing: postcard-rpc's bulk interface must
stay interface 0 because every host transport selects the first
class-0xFF interface it finds.

The WebUSB vendor code is 0x01 rather than 0x00 because postcard-rpc
registers the Microsoft OS 2.0 descriptor at vendor code 0, and
embassy-usb answers that request before dispatching to registered
handlers.

No wire-protocol impact: SCHEMA_VERSION_* derives from
pico-de-gallo-internal's version, which is unchanged.

Closes #87

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
git --no-pager log --oneline -1
```

Expected: a new commit titled `feat(firmware): Add WebUSB BOS platform capability`.

---

### Task 5: Cut firmware release 0.10.1

**Files:**
- Modify: `crates/pico-de-gallo-firmware/Cargo.toml:3`
- Modify: `crates/pico-de-gallo-firmware/CHANGELOG.md` (rename the heading from Task 4)
- Modify: `crates/pico-de-gallo-firmware/Cargo.lock` (regenerate)

- [ ] **Step 1: Bump the package version**

In `crates/pico-de-gallo-firmware/Cargo.toml`, line 3, change:

```toml
version = "0.10.0"
```

to:

```toml
version = "0.10.1"
```

Change nothing else. In particular leave
`pico-de-gallo-internal = { version = "0.6.0", path = "../pico-de-gallo-internal" }`
on line 37 alone — `internal` is not being released, and the firmware
has no dependents whose dep specs would need updating.

- [ ] **Step 2: Rename the CHANGELOG heading**

In `crates/pico-de-gallo-firmware/CHANGELOG.md`, change the heading
added in Task 4 from:

```markdown
## [Unreleased]
```

to:

```markdown
## [0.10.1] — 2026-08-04
```

Leave the `### Added` body unchanged.

- [ ] **Step 3: Refresh the firmware lockfile**

The lock records the firmware's own version at line 1018, so it is now
stale. From `crates/pico-de-gallo-firmware/`:

```bash
cargo check --target thumbv8m.main-none-eabihf
grep -A1 'name = "pico-de-gallo-firmware"' Cargo.lock
```

Expected: `version = "0.10.1"`.

If the lock still reads `0.10.0`, fall back to the AGENTS.md §7.1
ritual:

```bash
rm -f Cargo.lock && cargo generate-lockfile
```

- [ ] **Step 4: Verify the lock is in sync — this is the CI gate**

```bash
cargo check --locked --target thumbv8m.main-none-eabihf
```

Expected: succeeds. This is exactly what CI's `lockfile` job runs; if it
fails, the PR cannot merge (AGENTS.md §13.3).

- [ ] **Step 5: Confirm the host lockfile is untouched**

```bash
cd /home/balbi/workspace/pico-de-gallo
git status --porcelain Cargo.lock
grep -c 'pico-de-gallo-firmware' Cargo.lock || true
```

Expected: no output from `git status` (unmodified), and `0` occurrences.
The firmware is an excluded workspace (root `Cargo.toml:12`) and must
not appear in the host lock.

- [ ] **Step 6: Rebuild and confirm the reported version changed**

`build.rs` generates `VERSION_PATCH` from `CARGO_PKG_VERSION_PATCH`, so
the `version` endpoint now reports `0.10.1`.

```bash
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
grep VERSION_PATCH target/thumbv8m.main-none-eabihf/release/build/pico-de-gallo-firmware-*/out/version.rs
```

Expected: `pub(crate) const VERSION_PATCH: u32 = 1;`

- [ ] **Step 7: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add crates/pico-de-gallo-firmware/Cargo.toml \
        crates/pico-de-gallo-firmware/Cargo.lock \
        crates/pico-de-gallo-firmware/CHANGELOG.md
git --no-pager diff --cached --stat
git commit -F - <<'EOF'
chore(release): Bump pico-de-gallo-firmware to 0.10.1

Cut the firmware release carrying the WebUSB BOS platform capability.

Patch rather than minor, even though the change is a feat: AGENTS.md
section 6.5 ties minor bumps to wire-protocol changes, and there is
none here. SCHEMA_VERSION_* derives from pico-de-gallo-internal's
version, so host validate() is unaffected.

The firmware Cargo.lock records the crate's own version and is
regenerated here. The host Cargo.lock needs no change: the firmware is
an excluded workspace and is absent from it. No cross-crate dep specs
change either -- the firmware has no dependents, and
pico-de-gallo-internal is not being released.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
git --no-pager log --oneline -3
```

Expected: three commits shown — the release commit, the feat commit,
and `docs(repo): Add firmware 0.10.1 release to WebUSB spec`.

- [ ] **Step 8: Verify each commit builds independently**

AGENTS.md §4 rule 9 requires this. Check the feature commit in
isolation:

```bash
git stash list  # must be empty; abort if not
git checkout HEAD~1
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
cd /home/balbi/workspace/pico-de-gallo
git checkout feat/firmware-webusb-descriptors
```

Expected: the build at `HEAD~1` succeeds. Confirm you are back on the
branch with `git branch --show-current`.

---

### Task 6: Hardware verification

**Files:** none — this task changes nothing. It gates the PR.

These checks need a physical board and cannot be automated here. Flash
`target/thumbv8m.main-none-eabihf/release/pico-de-gallo-firmware` first
(see `book/src/hardware/assembly.md`).

- [ ] **Step 1: Confirm the descriptor layout**

```bash
lsusb -v -d 045e:067d 2>/dev/null | grep -A4 'Binary Object Store\|bNumInterfaces\|bInterfaceNumber\|bNumDeviceCaps\|Platform Device Capability'
```

Expected:
- `bNumDeviceCaps` is `3`
- `bNumInterfaces` is `2`
- A Platform Device Capability whose UUID is
  `{3408b638-09a9-47a0-8bfd-a0768815b665}`
- Interface `0` reports two bulk endpoints; interface `1` reports
  `bNumEndpoints 0`

**If interface 1 has the bulk endpoints instead of interface 0, stop.**
The ordering invariant has inverted and Task 1 Step 5 was applied in the
wrong place.

- [ ] **Step 2: Native regression on Linux**

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo -- list
cargo run -p gallo -- version
cargo run -p gallo -- i2c scan
```

Expected: `list` shows the device; `version` reports firmware **0.10.1**
(this confirms Task 5 Step 6 end-to-end); `i2c scan` completes without
hanging.

- [ ] **Step 3: Native regression on Windows — blocking**

This is the highest-risk check (spec §6.2). The device changes from
single-interface to multi-interface while keeping a *device-level*
WinUSB compatible ID. The expectation is that `bDeviceClass = 0xFF`
keeps Windows from loading `usbccgp`, so WinUSB still binds to the whole
device — but that is reasoning, not evidence.

On a Windows machine: replug the device, then run

```console
gallo list
gallo i2c scan
```

Expected: both succeed, with no "installing device" failure and no
`Access is denied` error.

**If WinUSB fails to bind, stop and report.** Do not merge. The likely
remedy is moving the MS OS 2.0 compatible-ID feature from device level
to a per-function descriptor, which is a design change requiring a spec
revision.

- [ ] **Step 4: Browser check**

In Chrome or Edge, open any `https://` page or `http://localhost` page,
open DevTools, and run from a user gesture (paste into the console and
click the page first, or bind it to a button):

```js
const device = await navigator.usb.requestDevice({
  filters: [{ vendorId: 0x045e, productId: 0x067d }],
});
await device.open();
await device.claimInterface(0);
console.log(device.configuration.interfaces.length,
            device.configuration.interfaces[0].alternate.endpoints.length);
```

Expected: the picker lists "Pico de Gallo", `open()` and
`claimInterface(0)` both resolve, and the log prints `2 2` — two
interfaces, two endpoints on interface 0.

Then open `chrome://device-log` and confirm the URL descriptor was
fetched. Unplugging and replugging should surface a notification
pointing at `https://balbi.sh/pico-de-gallo/`.

---

### Task 7: Open the pull request

**Files:** none.

- [ ] **Step 1: Push the branch**

```bash
cd /home/balbi/workspace/pico-de-gallo
git push -u origin feat/firmware-webusb-descriptors
```

Note `origin` is the maintainer's personal fork; the canonical repo is
`OpenDevicePartnership/pico-de-gallo`, which is the `upstream` remote
(AGENTS.md §4 rule 10). Confirm the intended target with
`git remote -v` before pushing.

- [ ] **Step 2: Open a draft PR**

```bash
gh pr create --repo OpenDevicePartnership/pico-de-gallo --draft \
  --title "feat(firmware): Add WebUSB BOS platform capability" \
  --body "$(cat <<'EOF'
Closes #87.

Emits a WebUSB platform capability in the BOS descriptor plus a
landing-page URL descriptor, then cuts firmware release 0.10.1.

## Commits

1. `feat(firmware)` — the descriptor change, both book chapters, and the
   CHANGELOG entry.
2. `chore(release)` — version bump to 0.10.1 and the regenerated
   firmware `Cargo.lock`.

## Wire-protocol impact

None. `SCHEMA_VERSION_*` derives from `pico-de-gallo-internal`'s
version, which is unchanged, so host `validate()` is unaffected. This
matches the assessment in #87.

## Dependencies

No `Cargo.toml` dependency changes. `embassy_usb::class::web_usb` is
compiled unconditionally in embassy-usb 0.5.1. The host `Cargo.lock` is
untouched; the firmware `Cargo.lock` changes only because it records the
firmware's own version.

## Reviewer attention

`WebUsb::configure()` appends a **second class-0xFF interface** with no
endpoints, because embassy-usb offers no way to attach a BOS capability
to an existing interface. Every host transport selects the *first*
class-0xFF interface, so postcard-rpc's bulk interface must remain
interface 0. That ordering is enforced only by call order and a code
comment.

The Windows WinUSB binding path deserves scrutiny: the device goes from
single- to multi-interface while keeping a device-level WinUSB
compatible ID. Verified on hardware (see checklist below).

## Docs

- `book/src/getting-started/usb.md` — new "Browsers (WebUSB)" section.
- `book/src/internals/firmware.md` — new "USB descriptors" section
  covering interfaces, BOS capabilities, and vendor codes.

## Hardware verification

- [ ] `lsusb -v` shows 3 BOS capabilities, 2 interfaces, bulk endpoints on interface 0
- [ ] Linux: `gallo list` / `version` (reports 0.10.1) / `i2c scan`
- [ ] Windows: WinUSB binds, `gallo i2c scan` works
- [ ] Chrome: `requestDevice` + `claimInterface(0)` succeed, landing page notification appears
EOF
)"
```

- [ ] **Step 3: Wait for CI, then mark ready**

Watch the checks:

```bash
gh pr checks --watch
```

All must pass — particularly `lockfile`, `deny`, `actionlint`, and the
`nostd` firmware builds for both revisions. Only then:

```bash
gh pr ready
```

Do not squash-merge (AGENTS.md §4 rule 9).

- [ ] **Step 4: After merge, tag the release**

Find the merged release commit — it is the `chore(release)` commit on
`main`, not the merge commit:

```bash
git fetch upstream
RELEASE_SHA=$(git log upstream/main --format='%H %s' -20 \
  | grep 'chore(release): Bump pico-de-gallo-firmware to 0.10.1' \
  | cut -d' ' -f1)
echo "$RELEASE_SHA"
git --no-pager show --stat --format='%s' "$RELEASE_SHA"
```

Expected: exactly one SHA, whose diffstat shows `Cargo.toml`,
`Cargo.lock`, and `CHANGELOG.md` under `crates/pico-de-gallo-firmware/`.

```bash
git tag firmware-v0.10.1 "$RELEASE_SHA"
git push upstream firmware-v0.10.1
```

The prefix is `firmware-v*`, **not** `fw-v*` (AGENTS.md §12). This fires
`release-firmware.yml`, which builds the `.uf2` and `.elf` artifacts.
Tag-triggered workflows run the workflow YAML as it existed at the
tagged commit (AGENTS.md §13.13), so verify with:

```bash
git --no-pager show firmware-v0.10.1:.github/workflows/release-firmware.yml | head -20
```

---

## Out of scope

Recorded so nobody expands this PR mid-flight:

- **Host wasm builds** for `pico-de-gallo-internal`, `pico-de-gallo-lib`,
  and `pico-de-gallo-hal`. `internal` already compiles for
  `wasm32-unknown-unknown`; `lib` needs target-gated dependencies plus
  postcard-rpc's `webusb` feature; `hal`'s 21 blocking `embedded-hal`
  impls cannot work on a browser main thread at all. Separate spec.
- **Migrating to nusb 0.2.6.** postcard-rpc 0.12.1 pins `nusb ^0.1.9`,
  and postcard-rpc ships its own WebUSB transport anyway.
- **`book/src/crates/mcp.md:204,210`**, which shows
  `"firmware_version": "0.10.0"` in captured sample output. Illustrative
  transcript text, not an API claim.
- **`book/src/internals/firmware.md:35`**, which refers to
  "`pico-de-gallo-firmware` 0.11.0" for the dispatcher-wedge fix that the
  CHANGELOG records under 0.10.0. Pre-existing drift, unrelated to this
  change.
