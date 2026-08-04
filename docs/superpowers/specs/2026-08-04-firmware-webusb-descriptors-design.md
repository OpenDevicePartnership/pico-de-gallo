# Firmware WebUSB Descriptors — Design

- **Date:** 2026-08-04
- **Issue:** [#87 — feat: Add WebUSB descriptors to FW](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/87)
- **Branch:** `feat/firmware-webusb-descriptors`
- **Component:** `pico-de-gallo-firmware` only
- **Wire-protocol impact:** none

## 1. Problem

Pico de Gallo cannot be driven from a web browser. Issue #87 asks for
WebUSB descriptors so that a browser-based host has a supported path to
the device.

The firmware currently emits a BOS descriptor with two capabilities —
`USB_2_0_EXTENSION` (written automatically by embassy-usb) and a
Microsoft OS 2.0 `PLATFORM` capability (written by postcard-rpc so
Windows binds WinUSB). It emits no WebUSB platform capability and no
landing-page URL descriptor.

### Honest scoping note

These descriptors are **not strictly required** for WebUSB to function.
`navigator.usb.requestDevice()` followed by `claimInterface()` already
works against a vendor-class device, and Windows WinUSB binding is
already handled by the existing MS OS 2.0 descriptor. What the WebUSB
BOS capability adds is:

1. The Chrome landing-page notification when the device is plugged in.
2. Explicit, discoverable signalling that the device is WebUSB-aware.

This is polish and intent-signalling, not an unblocker. It is still
worth doing, and it is what #87 asks for, but the spec should not
pretend it unlocks browser support on its own.

## 2. Scope

**In scope.** Emit a WebUSB BOS platform capability and a landing-page
URL descriptor from `pico-de-gallo-firmware`.

**Out of scope**, deferred to a separate spec:

- Making `pico-de-gallo-internal`, `pico-de-gallo-lib`, and
  `pico-de-gallo-hal` build for `wasm32-unknown-unknown`.
- Migrating to nusb 0.2.6 (see §3.2).
- Any `[package].version` bump. Per AGENTS.md §4 rule 12, releases are
  a separate deliberate commit.
- Any new firmware feature flag. WebUSB is unconditional.

## 3. Background research

### 3.1 postcard-rpc owns the `embassy_usb::Builder`

`main.rs:449` calls `STORAGE.init(...)`, which internally constructs the
`embassy_usb::Builder`, registers the MS OS 2.0 descriptors, creates the
vendor-class function with the two bulk endpoints, and calls
`builder.build()` — returning a finished `UsbDevice`. The firmware never
sees a `Builder`.

postcard-rpc 0.12.1 exposes an escape hatch at
`src/server/impls/embassy_usb_v0_5.rs:191`:

```rust
pub fn init_without_build(
    &'static self,
    driver: D,
    config: Config<'static>,
    tx_buf: &'static mut [u8],
    max_usb_frame_size: usize,
) -> (Builder<'static, D>, WireTxImpl<M, D>, WireRxImpl<D>)
```

This returns the un-built `Builder`, which is the hook this design uses.

### 3.2 nusb is not the path

The premise that "nusb has gotten WebUSB support" does not hold for this
project:

- Locked `nusb 0.1.14` has zero wasm code; `src/platform/mod.rs` covers
  only linux/android, macos, and windows.
- WebUSB dependencies (`js-sys`, `wasm-bindgen`, `web-sys`) first appear
  in **nusb 0.2.6**.
- `postcard-rpc 0.12.1` declares `nusb ^0.1.9`. The 0.1 → 0.2 jump is
  semver-incompatible, so adopting nusb 0.2.6 means abandoning
  postcard-rpc's transport layer and hand-rolling `WireTx` / `WireRx` /
  `WireSpawn`.

postcard-rpc instead ships **its own** WebUSB transport at
`src/host_client/webusb.rs`, gated
`cfg(all(feature = "webusb", target_family = "wasm"))`. That is the
correct host-side path when the deferred wasm work happens. It does not
affect this firmware change.

### 3.3 embassy-usb 0.5.1 already has WebUSB

`embassy_usb::class::web_usb` is compiled unconditionally — it is not
behind any Cargo feature. It provides `WebUsb::configure`,
`web_usb::{Config, State, Url}`, and a `Handler` that answers the
`GET_URL` vendor request.

Two things it does **not** do:

- It cannot append a capability to an existing interface.
  `bos_capability` exists only on `InterfaceAltBuilder`
  (`builder.rs:444`), the `Builder::bos_descriptor` field is private, and
  there is no public `Builder::bos_writer()`. So `WebUsb::configure`
  allocates its own function, interface, and alt-setting
  (`web_usb.rs:145-147`), all class `0xFF`, with **no endpoints**.
- It does not pick a vendor code for you.

### 3.4 Facts checked and found harmless

- **`bcdUSB` is already correct.** `embassy_usb::Config` defaults
  `bcd_usb` to `UsbVersion::TwoOne` = `0x0210` (`builder.rs:133`), so
  hosts already request the BOS descriptor. No change needed.
- **Interface and handler limits are fine.** `MAX_INTERFACE_COUNT` and
  `MAX_HANDLER_COUNT` both default to `4` (embassy-usb `build.rs:9-10`).
  This change takes interfaces 1 → 2 and handlers 1 → 2.

### 3.5 The vendor-code constraint

embassy-usb intercepts vendor/device control requests matching the MS OS
descriptor's vendor code *before* dispatching to user handlers
(`embassy-usb-0.5.1/src/lib.rs:677-683`). postcard-rpc registers that
descriptor with vendor code `0` (`embassy_usb_v0_5.rs:216`). The WebUSB
vendor code must therefore be non-zero, or the WebUSB `GET_URL` handler
would never be reached.

## 4. Design

### 4.1 Firmware change

In `crates/pico-de-gallo-firmware/src/main.rs`, replace the
`STORAGE.init(...)` call with `init_without_build(...)`, inject the
WebUSB capability, then build the device:

```rust
let (mut builder, tx_impl, rx_impl) = STORAGE.init_without_build(
    driver,
    config,
    pbufs.tx_buf.as_mut_slice(),
    USB_FS_MAX_PACKET_SIZE,
);

// MUST run after init_without_build(): postcard-rpc's bulk interface has to
// stay interface 0, because every host transport selects the FIRST class-0xFF
// interface. WebUsb::configure() adds a second, endpoint-less 0xFF interface.
static WEBUSB_STATE: StaticCell<WebUsbState<'static>> = StaticCell::new();
static WEBUSB_CONFIG: StaticCell<WebUsbConfig<'static>> = StaticCell::new();

let webusb_config = WEBUSB_CONFIG.init(WebUsbConfig {
    max_packet_size: USB_FS_MAX_PACKET_SIZE as u16,
    landing_url: Some(Url::new(WEBUSB_LANDING_URL)),
    vendor_code: WEBUSB_VENDOR_CODE,
});
WebUsb::configure(&mut builder, WEBUSB_STATE.init(WebUsbState::new()), webusb_config);

let device = builder.build();
```

`embassy_usb::Config` is already imported unaliased at `main.rs:76`, so
the new import must alias to avoid a collision:

```rust
use embassy_usb::class::web_usb::{Config as WebUsbConfig, State as WebUsbState, Url, WebUsb};
```

Two details the snippet elides:

- `USB_FS_MAX_PACKET_SIZE` is currently referenced fully-qualified at
  `main.rs:453`. The implementation should import it
  (`use postcard_rpc::server::impls::embassy_usb_v0_5::USB_FS_MAX_PACKET_SIZE;`)
  since it is now used twice.
- **`WebUsbConfig::max_packet_size` is a dead field.** It is declared at
  `web_usb.rs:59` and read nowhere in embassy-usb 0.5.1 — grep confirms
  the only occurrence is the declaration itself. `WebUsb::configure`
  creates no endpoints, so the value is inert. We populate it with
  `USB_FS_MAX_PACKET_SIZE as u16` because the struct has no `Default`
  and the field must be given something; nothing depends on the value.

### 4.2 Constants

Both live in the firmware crate. They are deliberately **not** added to
`pico-de-gallo-internal`: that crate is the published wire-protocol
contract, and these values never appear on the wire.

```rust
/// WebUSB landing page advertised in the URL descriptor.
const WEBUSB_LANDING_URL: &str = "https://balbi.sh/pico-de-gallo/";

/// bRequest for WebUSB vendor control transfers. Must not be 0: postcard-rpc
/// registers the MS OS 2.0 descriptor at vendor code 0, and embassy-usb
/// intercepts that before user handlers (embassy-usb-0.5.1 src/lib.rs:677).
const WEBUSB_VENDOR_CODE: u8 = 0x01;
```

The URL is 24 bytes after `Url::new` strips the `https://` prefix, well
inside the 252-byte limit asserted at `web_usb.rs:34-36`.

### 4.3 No dependency changes

`embassy_usb::class::web_usb` is unconditional in embassy-usb 0.5.1, so
there is no new dependency, no `Cargo.toml` edit, no `Cargo.lock` churn,
no `deny.toml` impact, and no new row for the AGENTS.md §7.2 pin table.

## 5. Resulting descriptor layout

BOS descriptor, 2 capabilities → 3:

| Capability | Bytes | Written by |
|---|---:|---|
| BOS header | 5 | embassy-usb `descriptor.rs:380` |
| `USB_2_0_EXTENSION` | 7 | embassy-usb `descriptor.rs:394` |
| `PLATFORM` — MS OS 2.0 | 28 | postcard-rpc, via `msos.rs:180` |
| `PLATFORM` — WebUSB **(new)** | 24 | `web_usb.rs:149` |
| **Total** | **64** | of a 256-byte buffer |

The WebUSB capability's 24 bytes are a 3-byte descriptor header plus 21
bytes of payload: 1 reserved, 16 UUID, 2 `bcdVersion`, 1 `bVendorCode`,
1 `iLandingPage`.

Configuration descriptor grows by one 9-byte interface descriptor,
32 → 41 bytes of a 256-byte buffer.

Both buffer sizes are set by the `WireStorage<_, _, 256, 256, 64, 256>`
type alias at `main.rs:132` and need no change.

Static RAM grows by roughly 176 bytes of `.bss`, dominated by the
WebUSB `Control` handler's `ep_buf: [u8; 128]`. Negligible against the
RP2350's 520 KB of SRAM.

## 6. Risks

### 6.1 Interface ordering invariant (primary risk)

The device now exposes two class-`0xFF` interfaces, and postcard-rpc's
must remain index 0. Every host transport selects the *first* 0xFF
match:

| Consumer | Selection logic |
|---|---|
| `raw_nusb`, Linux/macOS | `raw_nusb.rs:95` — `position(\|i\| i.class() == 0xFF)` |
| `raw_nusb`, Windows | `raw_nusb.rs:101` — hardcoded interface `0` |
| postcard-rpc WebUSB | `webusb.rs:194-203` — first 0xFF, then requires IN+OUT endpoints |

The invariant is enforced only by call order — `WebUsb::configure` must
run after `init_without_build`. It is therefore documented in an
explicit code comment and verified by inspecting `lsusb` output
(§7 step 1). If it ever inverts, the browser path fails with "Failed to
find usable interface" and native Linux/macOS silently claims an
endpoint-less interface.

### 6.2 Windows WinUSB binding (needs hardware evidence)

Today the device is single-interface with a device-level
`CompatibleIdFeatureDescriptor::new("WINUSB", "")`. This change makes it
multi-interface. The expectation is that `bDeviceClass = 0xFF`
(`main.rs:277`) prevents Windows from loading `usbccgp`, so the
device-level compatible ID still binds WinUSB to the whole device.

**That is reasoning, not evidence.** It is the one plausible way this
change regresses existing users, and it must be verified on Windows
hardware before merge (§7 step 3).

### 6.3 Endpoint-less vendor interface

The second interface has no endpoints, which is unusual but legal. It is
inert for all three host transports given the ordering invariant holds.

## 7. Verification

Build gates, mirroring `.github/workflows/nostd.yml`, run from
`crates/pico-de-gallo-firmware/` for **both** hardware revisions:

```bash
# hw-rev1 (default)
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf

# hw-rev2
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev2
```

The firmware has no unit tests — it is `no_std` and cross-compiled — so
correctness is established on hardware:

1. **Descriptors.** `lsusb -v -d 045e:067d` shows 3 BOS capabilities;
   the WebUSB platform UUID `3408b638-09a9-47a0-8bfd-a0768815b665` is
   present; `bNumInterfaces = 2`; and **interface 0 carries both bulk
   endpoints**.
2. **Native regression, Linux.** `gallo list`, `gallo version`, and
   `gallo i2c scan` all still pass.
3. **Native regression, Windows (blocking).** Replug the device, confirm
   WinUSB still binds, and confirm `gallo i2c scan` works. This gates
   merge because of §6.2.
4. **Browser.** In Chrome,
   `navigator.usb.requestDevice({filters:[{vendorId:0x045e,productId:0x067d}]})`
   succeeds, and `chrome://device-log` shows the URL descriptor fetch
   and the landing-page notification for
   `https://balbi.sh/pico-de-gallo/`.

## 8. Documentation

AGENTS.md §15.1 makes book/code parity a hard rule. Firmware peripheral
behaviour maps to `book/src/internals/firmware.md` and the relevant
getting-started page.

- **`book/src/getting-started/usb.md`** — add a "Browser / WebUSB"
  section alongside the existing Linux, Windows, and macOS sections:
  what the descriptors advertise, the secure-context (HTTPS or
  localhost) and user-gesture requirements for
  `navigator.usb.requestDevice()`, and the §1 note that WebUSB works
  without these descriptors.
- **`book/src/internals/firmware.md`** — document the actual USB
  descriptor layout. The page currently has a single passing mention of
  embassy-usb at line 14. Cover the two interfaces, the three BOS
  capabilities, both vendor codes, and why the WebUSB one must not be
  `0`.
- **`crates/pico-de-gallo-firmware/CHANGELOG.md`** — add an
  `## [Unreleased]` section (none exists today; the newest entry is
  `## [0.10.0] — 2026-06-22`) with an `### Added` entry referencing #87.

No `book/src/SUMMARY.md` change: both pages already exist.

## 9. Commit and PR conventions

Per AGENTS.md §10, a Conventional Commit with a crate scope:

```text
feat(firmware): Add WebUSB BOS platform capability

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

No `Signed-off-by:` — DCO is for humans only (§4 rule 7). Open as a
draft PR, let CI go green, and reference "Closes #87".
