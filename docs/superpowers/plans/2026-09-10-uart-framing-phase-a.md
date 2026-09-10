# UART framing configuration — Phase A implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the UART wire configuration from baud-rate-only to full framing (data bits, parity, stop bits), apply it on the RP2350, and surface it through every host crate — so that Phase B's Zephyr `uart_configure()` has something honest to call.

**Architecture:** Three new postcard enums in `pico-de-gallo-internal`, sized to what the RP2350's PL011 can actually produce. The firmware applies them by writing `UARTLCR_H` through the public `embassy_rp::pac`, replicating embassy's own disable / 15-baud-delay / modify / restore sequence, because `BufferedUart` exposes `set_baudrate` and nothing else. A second, independent firmware change gives `uart/read` a single-poll fast path so an idle poll costs a USB round trip instead of a round trip plus a millisecond.

**Tech Stack:** Rust (no_std firmware on `thumbv8m.main-none-eabihf`, std host crates), postcard-rpc 0.12, embassy-rp 0.10, rp-pac 7.0, PyO3, cbindgen.

**Design doc:** `docs/superpowers/specs/2026-09-10-zephyr-uart-driver-design.md`

**Scope note:** This is Phase A of two. Phase B (the Zephyr `pdg_uart` driver) is planned separately, after this lands, so it can cite real compiled FFI signatures. Both phases ship in one PR.

---

## Ground rules for this plan

Read these once. They are repo rules that apply to nearly every task.

1. **Never bump any `[package].version`.** `main` already carries an unreleased schema 0.7 → 0.8 break; this work rides it. See AGENTS.md §4 rule 12.
2. **Never reorder enum variants** in `pico-de-gallo-internal`. postcard encodes by variant index. Append only.
3. **LF line endings on every file.** If you create a file on Windows, run `dos2unix <file>`.
4. **Commit `Cargo.toml` and `Cargo.lock` together** if you touch a manifest. This plan should not need to.
5. **AI commit trailers**, no `Signed-off-by`:
   ```
   Assisted-by: OpenCode:claude-opus-5
   Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
   ```
6. **Two workspaces.** Host commands run from the repo root; firmware commands run from `crates/pico-de-gallo-firmware/`.

**Environment.** The board is attached to WSL via usbip. Host tools run inside WSL:

```bash
wsl -d Ubuntu-26.04 -- bash -c 'export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$HOME/.cargo/bin; cd /mnt/d/workspace/pico-de-gallo; export CARGO_TARGET_DIR=/tmp/pdg-host-target; <command>'
```

A hidden `wsl … sleep infinity` process keeps the distro alive; killing it detaches the board. If `lsusb | grep 067d` shows nothing, re-attach from PowerShell with `usbipd attach --wsl --busid 9-3`.

---

## File structure

| File | Responsibility | Action |
|---|---|---|
| `crates/pico-de-gallo-internal/src/lib.rs` | Wire enums `UartDataBits`/`UartParity`/`UartStopBits`; extended request and info structs | Modify |
| `crates/pico-de-gallo-firmware/src/context.rs` | Replace `uart_baud_rate: u32` with a four-field config | Modify |
| `crates/pico-de-gallo-firmware/src/handlers/uart.rs` | `UARTLCR_H` framing apply; single-poll read fast path; full get-config | Modify |
| `crates/pico-de-gallo-lib/src/lib.rs` | `uart_set_config` takes framing | Modify |
| `crates/pico-de-gallo-ffi/src/lib.rs` | `GalloUart*` enums; extended signatures | Modify |
| `crates/pico-de-gallo-ffi/cbindgen.toml` | Export the new enums or cbindgen prunes them | Modify |
| `crates/pico-de-gallo-app/src/lib.rs` | `--data-bits` / `--parity` / `--stop-bits` | Modify |
| `crates/pico-de-gallo-mcp/src/uart.rs` | Tool arguments and response | Modify |
| `crates/pyco-de-gallo/src/lib.rs` | `#[pyclass]` enums | Modify |
| `crates/pico-de-gallo-hal/src/lib.rs` | Doc comment only, no functional change | Modify |

---

## Task 1: Wire enums

**Files:**
- Modify: `crates/pico-de-gallo-internal/src/lib.rs` (insert before `UartError` at line 1085)

- [ ] **Step 1: Write the failing round-trip tests**

Add to the `#[cfg(test)] mod tests` block in `crates/pico-de-gallo-internal/src/lib.rs`:

```rust
#[test]
fn uart_data_bits_round_trip() {
    for v in [
        UartDataBits::Five,
        UartDataBits::Six,
        UartDataBits::Seven,
        UartDataBits::Eight,
    ] {
        let bytes = postcard::to_allocvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<UartDataBits>(&bytes).unwrap(), v);
    }
}

#[test]
fn uart_parity_round_trip() {
    for v in [
        UartParity::None,
        UartParity::Odd,
        UartParity::Even,
        UartParity::Mark,
        UartParity::Space,
    ] {
        let bytes = postcard::to_allocvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<UartParity>(&bytes).unwrap(), v);
    }
}

#[test]
fn uart_stop_bits_round_trip() {
    for v in [UartStopBits::One, UartStopBits::Two] {
        let bytes = postcard::to_allocvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<UartStopBits>(&bytes).unwrap(), v);
    }
}

/// Variant indices are ABI. This pins them so a reorder is a test failure
/// rather than a silent wire break in the field.
#[test]
fn uart_framing_enum_indices_are_stable() {
    assert_eq!(postcard::to_allocvec(&UartDataBits::Five).unwrap(), [0]);
    assert_eq!(postcard::to_allocvec(&UartDataBits::Eight).unwrap(), [3]);
    assert_eq!(postcard::to_allocvec(&UartParity::None).unwrap(), [0]);
    assert_eq!(postcard::to_allocvec(&UartParity::Space).unwrap(), [4]);
    assert_eq!(postcard::to_allocvec(&UartStopBits::One).unwrap(), [0]);
    assert_eq!(postcard::to_allocvec(&UartStopBits::Two).unwrap(), [1]);
}

/// `UartDataBits`' discriminants are deliberately the RP2350 `UARTLCR_H.WLEN`
/// encoding, so the firmware maps with a cast rather than a table. If this
/// ever stops holding, `apply_framing` in the firmware must gain a match.
#[test]
fn uart_data_bits_discriminants_are_the_wlen_encoding() {
    assert_eq!(UartDataBits::Five as u8, 0b00);
    assert_eq!(UartDataBits::Six as u8, 0b01);
    assert_eq!(UartDataBits::Seven as u8, 0b10);
    assert_eq!(UartDataBits::Eight as u8, 0b11);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --locked -p pico-de-gallo-internal --features use-std uart_
```

Expected: FAIL, `cannot find type UartDataBits in this scope`.

> Note the `--features use-std`. Without it these tests fail on the `vec!`
> macro instead, which looks like a different bug. See AGENTS.md §13.14.

- [ ] **Step 3: Add the enums**

Insert immediately **before** `/// Error from UART operations` at line 1085:

```rust
// WARNING: do not reorder variants - postcard encodes by index, not discriminant.
/// UART word length, in data bits per character.
///
/// The RP2350's PL011 encodes this in `UARTLCR_H.WLEN`, which is two bits
/// wide, so 5..=8 is the complete hardware range and there is no 9-bit mode.
/// The discriminants below **are** the `WLEN` encoding, which the firmware
/// relies on; `uart_data_bits_discriminants_are_the_wlen_encoding` pins it.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UartDataBits {
    /// Five data bits.
    Five = 0,
    /// Six data bits.
    Six = 1,
    /// Seven data bits.
    Seven = 2,
    /// Eight data bits. The power-on default.
    Eight = 3,
}

// WARNING: do not reorder variants - postcard encodes by index, not discriminant.
/// UART parity mode.
///
/// `None`, `Odd` and `Even` use the PL011's `PEN`/`EPS` bits. `Mark` and
/// `Space` additionally set `SPS` (stick parity): with `SPS` set, `EPS = 0`
/// transmits and checks the parity bit as 1, and `EPS = 1` as 0.
///
/// Mark and space are unreachable through embassy's own `uart::Parity`, which
/// has only three variants. They are offered here because the firmware writes
/// the register directly.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UartParity {
    /// No parity bit. The power-on default.
    None = 0,
    /// Odd parity.
    Odd = 1,
    /// Even parity.
    Even = 2,
    /// Parity bit always 1.
    Mark = 3,
    /// Parity bit always 0.
    Space = 4,
}

// WARNING: do not reorder variants - postcard encodes by index, not discriminant.
/// UART stop-bit count.
///
/// The PL011 has a single `STP2` bit, so one and two stop bits are the
/// complete hardware range. Half stop bits are not representable.
///
/// These indices deliberately do **not** match Zephyr's
/// `uart_config_stop_bits` (`0_5 = 0, 1 = 1, 1_5 = 2, 2 = 3`). Matching it
/// would bake a foreign subsystem's ABI into this wire format and leave two
/// permanently unrepresentable holes; consumers map explicitly instead.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UartStopBits {
    /// One stop bit. The power-on default.
    One = 0,
    /// Two stop bits.
    Two = 1,
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --locked -p pico-de-gallo-internal --features use-std uart_
```

Expected: PASS, 5 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-internal/src/lib.rs
git commit -m "feat(internal): Add UART framing wire enums

The UART wire configuration carries only baud_rate, so a caller asking
for 7E1 has no way to be honoured. Add the three framing enums, sized to
what the RP2350 PL011 can produce rather than to any consumer's
numbering.

UartDataBits stops at Eight because WLEN is two bits wide, and
UartStopBits omits half stop bits because the PL011 has a single STP2
bit. UartParity includes Mark and Space, which embassy's own Parity
enum cannot express, because the firmware writes SPS directly.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 2: Extend the request and info structs

**Files:**
- Modify: `crates/pico-de-gallo-internal/src/lib.rs:1151-1180`

- [ ] **Step 1: Write the failing round-trip test**

Add to the test module:

```rust
#[test]
fn uart_set_configuration_request_round_trip() {
    let req = UartSetConfigurationRequest {
        baud_rate: 115_200,
        data_bits: UartDataBits::Seven,
        parity: UartParity::Even,
        stop_bits: UartStopBits::Two,
    };
    let bytes = postcard::to_allocvec(&req).unwrap();
    assert_eq!(
        postcard::from_bytes::<UartSetConfigurationRequest>(&bytes).unwrap(),
        req
    );
}

#[test]
fn uart_configuration_info_round_trip() {
    let info = UartConfigurationInfo {
        baud_rate: 9600,
        data_bits: UartDataBits::Eight,
        parity: UartParity::None,
        stop_bits: UartStopBits::One,
    };
    let bytes = postcard::to_allocvec(&info).unwrap();
    assert_eq!(
        postcard::from_bytes::<UartConfigurationInfo>(&bytes).unwrap(),
        info
    );
}

/// The old one-field request encoded to 3 bytes for 115200. The new one
/// appends three single-byte enum indices. This pins the encoding so a
/// field reorder is caught here rather than on a board.
#[test]
fn uart_set_configuration_request_encoding_is_pinned() {
    let req = UartSetConfigurationRequest {
        baud_rate: 115_200,
        data_bits: UartDataBits::Eight,
        parity: UartParity::None,
        stop_bits: UartStopBits::One,
    };
    // 115200 is a 3-byte postcard varint, then WLEN=3, parity=0, stop=0.
    assert_eq!(
        postcard::to_allocvec(&req).unwrap(),
        [0x80, 0x84, 0x07, 0x03, 0x00, 0x00]
    );
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test --locked -p pico-de-gallo-internal --features use-std uart_set_config uart_configuration
```

Expected: FAIL, `struct UartSetConfigurationRequest has no field named data_bits`.

- [ ] **Step 3: Replace the two structs**

Replace lines 1151-1164 (the doc comment through the closing brace of
`UartSetConfigurationRequest`) with:

```rust
/// Request to reconfigure UART bus parameters.
///
/// Takes effect immediately. The firmware applies the new configuration
/// before processing the next UART operation.
///
/// All four parameters are applied together; there is no partial update. A
/// caller that wants to change only the baud rate must read the current
/// configuration with `uart/get-config` first and echo the framing fields
/// back.
///
/// The framing values are limited to what the RP2350 PL011 can produce. See
/// [`UartDataBits`], [`UartParity`] and [`UartStopBits`].
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct UartSetConfigurationRequest {
    /// UART baud rate in bits per second. Must be non-zero.
    pub baud_rate: u32,
    /// Word length in data bits.
    pub data_bits: UartDataBits,
    /// Parity mode.
    pub parity: UartParity,
    /// Stop-bit count.
    pub stop_bits: UartStopBits,
}
```

Then replace lines 1172-1180 (`UartConfigurationInfo` and its doc comment) with:

```rust
/// Current UART bus configuration as reported by the firmware.
///
/// Returned by `uart/get-config`. Reflects the last successfully applied
/// configuration.
///
/// This is a software shadow of what was requested, not a register
/// read-back, so `baud_rate` is the value that was asked for rather than the
/// value the divisor actually achieves after rounding.
#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct UartConfigurationInfo {
    /// UART baud rate in bits per second.
    pub baud_rate: u32,
    /// Word length in data bits.
    pub data_bits: UartDataBits,
    /// Parity mode.
    pub parity: UartParity,
    /// Stop-bit count.
    pub stop_bits: UartStopBits,
}
```

> The deleted doc comment claimed "In v1, only `baud_rate` is configurable …
> These fields are reserved for future use and must be set to their default
> values (`Eight`, `None`, `One`)" while naming fields that did not exist.
> That is now true and the fields exist.

- [ ] **Step 4: Run to verify pass**

```bash
cargo test --locked -p pico-de-gallo-internal --features use-std uart_
```

Expected: PASS, 8 tests.

If `uart_set_configuration_request_encoding_is_pinned` fails, print the actual
bytes and check the varint by hand rather than editing the expectation to
match — the point of the test is to catch exactly this.

- [ ] **Step 5: Build the whole host workspace to find every caller**

```bash
cargo check --workspace --locked
```

Expected: FAIL, with errors in `pico-de-gallo-lib`, `-ffi`, `-app`, `-mcp` and
`pyco-de-gallo` about missing fields. That error list is the work queue for
Tasks 7-11. Do not fix them yet.

- [ ] **Step 6: Commit**

```bash
git add crates/pico-de-gallo-internal/src/lib.rs
git commit -m "feat(internal): Carry UART framing on the wire

UartSetConfigurationRequest and UartConfigurationInfo gain data_bits,
parity and stop_bits alongside baud_rate.

No version bump: main already carries an unreleased 0.7 to 0.8 schema
break and this rides it, per AGENTS.md 4 rule 12.

No new UartError variant either. The enums encode only representable
values, so anything that decodes is something the hardware can do;
unsupported combinations are refused at the consumer boundary and never
reach the wire. That keeps UartError's ABI untouched.

This also makes the struct doc comment true for the first time: it
previously described data-bits, parity and stop-bits fields that did not
exist.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 3: Verify the request-key claim before relying on it

The design's §4.5 asserts that adding request fields re-keys `uart/set-config`
via `REQ_KEY`, so a mismatched firmware fails loudly instead of mis-decoding.
That is a **code reading**, and the 2026-09-01 regression row records a similar
assumption being wrong twice. Settle it now, before the rest of the stack is
built on it.

**Files:**
- Create: `crates/pico-de-gallo-internal/tests/req_key_changes_with_request_shape.rs`

- [ ] **Step 1: Write the test**

```rust
//! Pins the skew-detection property the design relies on.
//!
//! `validate()` compares schema versions only, and this change rides an
//! already-unreleased 0.8 bump, so firmware built either side of it reports
//! the same version. The thing that actually distinguishes them is the
//! endpoint key: postcard-rpc derives `REQ_KEY` from the request type's
//! schema, so adding fields re-keys the endpoint and a mismatched firmware
//! does not dispatch the request at all.
//!
//! If this test fails, the design's 4.5 mitigation is wrong and the change
//! needs a different one - do not just delete the test.

use pico_de_gallo_internal::{UartDataBits, UartParity, UartSetConfiguration, UartStopBits};
use postcard_rpc::{Endpoint, Key};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

/// The request shape as it existed before this change.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
struct OldUartSetConfigurationRequest {
    baud_rate: u32,
}

#[test]
fn req_key_changes_when_the_request_gains_fields() {
    let old = Key::for_path::<OldUartSetConfigurationRequest>("uart/set-config");
    let new = <UartSetConfiguration as Endpoint>::REQ_KEY;

    assert_ne!(
        old.to_bytes(),
        new.to_bytes(),
        "REQ_KEY did not change when the request type gained fields. The \
         design's 4.5 skew mitigation assumes it does; if this holds, a \
         mismatched firmware would silently mis-decode instead of refusing."
    );
}

/// Sanity control: the same type against the same path must be stable, or the
/// assertion above would pass for the wrong reason.
#[test]
fn req_key_is_stable_for_an_unchanged_type() {
    let a = Key::for_path::<OldUartSetConfigurationRequest>("uart/set-config");
    let b = Key::for_path::<OldUartSetConfigurationRequest>("uart/set-config");
    assert_eq!(a.to_bytes(), b.to_bytes());
}

/// The dispatcher truncates to two bytes. A full-width difference that
/// collapses under truncation would not protect anything.
#[test]
fn req_key_differs_in_the_truncated_two_byte_form() {
    use postcard_rpc::Key2;

    let old = Key2::from_key8(Key::for_path::<OldUartSetConfigurationRequest>(
        "uart/set-config",
    ));
    let new = <UartSetConfiguration as Endpoint>::REQ_KEY2;

    assert_ne!(
        format!("{old:?}"),
        format!("{new:?}"),
        "REQ_KEY differs at full width but collides after truncation to two \
         bytes, which is what the dispatcher actually matches on."
    );
}

/// Guards the premise: the new request really does carry four fields.
#[test]
fn the_new_request_carries_framing() {
    let req = pico_de_gallo_internal::UartSetConfigurationRequest {
        baud_rate: 115_200,
        data_bits: UartDataBits::Eight,
        parity: UartParity::None,
        stop_bits: UartStopBits::One,
    };
    assert_eq!(req.baud_rate, 115_200);
}
```

- [ ] **Step 2: Run it**

```bash
cargo test --locked -p pico-de-gallo-internal --features use-std --test req_key_changes_with_request_shape
```

Expected: PASS, 4 tests.

- [ ] **Step 3: If it fails, stop and reassess**

A failure means §4.5 is wrong. Do not proceed to the firmware tasks. Record
what actually happened and raise it — the mitigation has to change, and the
options are a new endpoint path (`uart/set-config-v2`) or an explicit
capability probe. Note also that `Key2` may not expose comparison directly; if
the third test does not compile, adapt it to whatever accessor
`postcard_rpc::Key2` offers rather than deleting it.

- [ ] **Step 4: Commit**

```bash
git add crates/pico-de-gallo-internal/tests/req_key_changes_with_request_shape.rs
git commit -m "test(internal): Pin the UART set-config skew mitigation

The design relies on REQ_KEY changing when the request type gains
fields, so a firmware built before this change refuses the new request
rather than mis-decoding three extra varints as framing.

That was a code reading of postcard-rpc's macros.rs, and a comparable
assumption about device/info was wrong twice on the 159 branch, so pin
it. The truncated two-byte check matters as much as the full-width one,
because two bytes is what the dispatcher matches on.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 4: Firmware context holds the full configuration

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/context.rs:87-88`, `:121-125`, and the non-rev2 constructor around `:153`

- [ ] **Step 1: Change the field**

In `crates/pico-de-gallo-firmware/src/context.rs`, replace lines 87-88:

```rust
    #[cfg_attr(not(feature = "hw-rev2"), allow(dead_code))]
    pub(crate) uart_baud_rate: u32,
```

with:

```rust
    /// Software shadow of the applied UART configuration.
    ///
    /// Not a register read-back: `uart/get-config` returns what was last
    /// requested, not what the divisor rounds to. It cannot diverge from
    /// intent because the shadow update and the register writes are adjacent
    /// and unconditional in `uart_set_config_handler`.
    #[cfg_attr(not(feature = "hw-rev2"), allow(dead_code))]
    pub(crate) uart_config: UartConfigurationInfo,
```

- [ ] **Step 2: Add the import**

`UartConfigurationInfo` must be imported unconditionally, because the field is
present on both hardware revisions. Find the existing
`use pico_de_gallo_internal::{...}` block in `context.rs` and add
`UartConfigurationInfo`, `UartDataBits`, `UartParity`, `UartStopBits` to it.

- [ ] **Step 3: Update both constructors**

In the `hw-rev2` constructor, replace line 125:

```rust
            uart_baud_rate: 115_200,
```

with:

```rust
            // Matches uart::Config::default() as applied by BufferedUart::new
            // in main.rs: 115200 8N1, no flow control.
            uart_config: UartConfigurationInfo {
                baud_rate: 115_200,
                data_bits: UartDataBits::Eight,
                parity: UartParity::None,
                stop_bits: UartStopBits::One,
            },
```

Apply the identical replacement to the `#[cfg(not(feature = "hw-rev2"))]`
constructor, which has the same line near `:153`.

- [ ] **Step 4: Build both revisions to verify it compiles**

```bash
cd crates/pico-de-gallo-firmware
cargo check --locked --target thumbv8m.main-none-eabihf
cargo check --locked --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1
```

Expected: FAIL on both, in `handlers/uart.rs`, `no field uart_baud_rate on
type Context`. That is the next task.

- [ ] **Step 5: Do not commit yet**

The tree does not build. Task 5 completes this change.

---

## Task 5: Firmware applies the framing

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/handlers/uart.rs:111-148`

- [ ] **Step 1: Add the framing helper**

Insert into `crates/pico-de-gallo-firmware/src/handlers/uart.rs`, immediately
before `uart_set_config_handler` at line 111:

```rust
/// Applies word length, parity and stop bits to `UART0`'s `UARTLCR_H`.
///
/// embassy-rp 0.10 exposes no runtime API for these: `BufferedUart`'s entire
/// public surface is `new`, `new_with_rtscts`, `blocking_{read,write,flush}`,
/// `busy`, `send_break`, `set_baudrate`, `split` and `split_ref`, and
/// `uart::Config` is consumed only by the constructors. So this writes the
/// register directly through the public `embassy_rp::pac`.
///
/// The sequence below is a deliberate replication of embassy's own private
/// `Uart::lcr_modify` (embassy-rp-0.10.0/src/uart/mod.rs:998-1046). Keep them
/// in step: if an embassy bump changes that function, review this one.
///
/// The delay is not optional, and embassy's comment explains why: the PL011's
/// `BUSY` flag is OR'd with TX-FIFO-not-full, so with FIFOs enabled there is
/// no way to poll for end-of-character, and FIFOs cannot be disabled
/// mid-character without losing integrity. Fifteen baud periods comfortably
/// exceeds start + data + parity + stop.
///
/// Call this *after* `set_baudrate`, so the divisor the delay is computed
/// from is the one that will be in force.
#[cfg(feature = "hw-rev2")]
fn apply_framing(data_bits: UartDataBits, parity: UartParity, stop_bits: UartStopBits) {
    use embassy_rp::pac;

    // `UartDataBits`' discriminants are the WLEN encoding by construction;
    // pinned by uart_data_bits_discriminants_are_the_wlen_encoding.
    let wlen = data_bits as u8;
    let stp2 = matches!(stop_bits, UartStopBits::Two);
    let (pen, eps, sps) = match parity {
        UartParity::None => (false, false, false),
        UartParity::Odd => (true, false, false),
        UartParity::Even => (true, true, false),
        // Stick parity: SPS set, EPS selects the constant. EPS=0 sends 1.
        UartParity::Mark => (true, false, true),
        UartParity::Space => (true, true, true),
    };

    let r = pac::UART0;
    let cr = r.uartcr().read();

    if cr.uarten() {
        r.uartcr().modify(|w| {
            w.set_uarten(false);
            w.set_txe(false);
            w.set_rxe(false);
        });

        // Maximise precision: a 16.6 fixed-point ratio scaled to 32 bits.
        // 3662 is ~(15 * 244.14), where 244.14 is 16e6 / 2^16.
        let mut brdiv_ratio = 64 * r.uartibrd().read().0 + r.uartfbrd().read().0;
        brdiv_ratio <<= 10;
        let scaled_freq = embassy_rp::clocks::clk_peri_freq() / 3662;
        let wait_time_us = brdiv_ratio / scaled_freq;
        // Busy-wait rather than Timer::after, matching embassy. The UART is
        // disabled across this window and postcard-rpc dispatches serially,
        // so yielding would buy nothing and widen the window.
        embassy_time::block_for(Duration::from_micros(u64::from(wait_time_us)));
    }

    r.uartlcr_h().modify(|w| {
        w.set_wlen(wlen);
        w.set_stp2(stp2);
        w.set_pen(pen);
        w.set_eps(eps);
        w.set_sps(sps);
    });

    r.uartcr().write_value(cr);
}
```

- [ ] **Step 2: Rewrite the set-config handler**

Replace lines 111-127 (the rev2 `uart_set_config_handler`) with:

```rust
/// Handler for `uart/set-config` - changes baud rate and framing.
///
/// All four parameters are applied together. Baud first, then framing, so the
/// two never disagree across the reconfiguration window.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_set_config_handler(
    context: &mut Context,
    _header: VarHeader,
    req: UartSetConfigurationRequest,
) -> UartSetConfigurationResponse {
    if req.baud_rate == 0 {
        warn!("uart_set_config: baud_rate must be non-zero");
        return Err(UartError::InvalidBaudRate);
    }

    debug!("uart_set_config: baud_rate={=u32}", req.baud_rate);
    context.uart.set_baudrate(req.baud_rate);
    apply_framing(req.data_bits, req.parity, req.stop_bits);

    context.uart_config = UartConfigurationInfo {
        baud_rate: req.baud_rate,
        data_bits: req.data_bits,
        parity: req.parity,
        stop_bits: req.stop_bits,
    };
    Ok(())
}
```

- [ ] **Step 3: Rewrite the get-config handler**

Replace lines 139-148 (the rev2 `uart_get_config_handler` body) with:

```rust
/// Handler for `uart/get-config` - returns the current UART configuration.
#[cfg(feature = "hw-rev2")]
pub(crate) fn uart_get_config_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> UartGetConfigurationResponse {
    Ok(context.uart_config.clone())
}
```

- [ ] **Step 4: Fix the imports**

At the top of `handlers/uart.rs`, extend the `hw-rev2` import at line 10:

```rust
#[cfg(feature = "hw-rev2")]
use pico_de_gallo_internal::{
    MAX_TRANSFER_SIZE, UartConfigurationInfo, UartDataBits, UartParity, UartStopBits,
};
```

- [ ] **Step 5: Build both revisions**

```bash
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1
```

Expected: both succeed. The pre-existing
`address (0x1000013c) of section .text is not a multiple of alignment (8)`
linker warning is normal and unrelated.

- [ ] **Step 6: Lint both revisions**

```bash
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
cargo fmt --check
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/context.rs \
        crates/pico-de-gallo-firmware/src/handlers/uart.rs
git commit -m "feat(firmware): Apply UART framing via UARTLCR_H

embassy-rp 0.10 exposes set_baudrate and nothing else for runtime UART
reconfiguration - uart::Config is consumed only by the constructors - so
apply word length, parity and stop bits through the public
embassy_rp::pac.

apply_framing replicates embassy's own private Uart::lcr_modify
(mod.rs:998-1046), including the fifteen-baud-period delay, which is
load-bearing: the PL011's BUSY flag is OR'd with TX-FIFO-not-full, so
there is no way to poll for end-of-character while FIFOs are enabled.

This is the first embassy_rp::pac use in the firmware and it does write
registers behind embassy's back. That is a real hazard, but embassy's own
set_baudrate performs the identical disable/delay/restore dance on the
same registers and already ships, so the exposure is unchanged.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 6: Firmware read fast path

Independent of framing. `uart/read` with `timeout_ms == 0` currently always
burns `with_timeout(1ms, …)`, measured at **1422 µs** per empty poll against
this board. Phase B's `uart_poll_in` is the main consumer.

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/handlers/uart.rs:38-45`

- [ ] **Step 1: Add the fast path**

Replace lines 38-45 of `handlers/uart.rs`:

```rust
    if req.timeout_ms == 0 {
        // Non-blocking: try to read whatever is buffered. Well inside the
        // default dispatch budget, so no declaration is needed.
        match with_timeout(Duration::from_millis(1), AsyncRead::read(&mut context.uart, buf)).await {
            Ok(Ok(n)) => Ok(&context.buf[..n]),
            Ok(Err(_)) => Err(UartError::Other),
            Err(_) => Ok(&[]),
        }
    } else {
```

with:

```rust
    if req.timeout_ms == 0 {
        // Non-blocking. Poll the read exactly once instead of waiting out a
        // 1 ms timeout: an empty poll cost about 1422 us end to end, which a
        // Zephyr uart_poll_in loop pays on every idle iteration.
        //
        // Deliberately NOT ReadReady::read_ready(). That inspects only the
        // software RX ring, and on a framing/parity/break/overrun error the
        // ISR records rx_error and disables RX interrupts; only try_read
        // consumes the error and re-enables them. Returning early on an empty
        // ring would leave the error latched and RX dead for a client that
        // only ever polls with timeout_ms == 0. A single poll runs try_read,
        // so the error surfaces here instead.
        //
        // Dropping the future on Pending is safe: try_read never pops bytes
        // and then returns Pending, and poll_once's no-op waker registration
        // is replaced by the next read.
        match poll_once(AsyncRead::read(&mut context.uart, buf)) {
            Poll::Ready(Ok(n)) => Ok(&context.buf[..n]),
            Poll::Ready(Err(_)) => Err(UartError::Other),
            Poll::Pending => Ok(&[]),
        }
    } else {
```

> **CORRECTED AFTER M2.** This task originally prescribed
> `ReadReady::read_ready()`, which is **unsafe for this consumer** and would
> have shipped a latent RX-death bug. The reasoning, verified against
> `embassy-rp-0.10.0/src/uart/buffered.rs`:
>
> - `read_ready` is `Ok(!state.rx_buf.is_empty())` and nothing more (`:337`).
> - On any RX error the ISR latches `rx_error` **and disables the RX
>   interrupts** — `uartimsc().write_clear()` on `rxim`/`rtim` (`:588-593`).
> - `try_read` is the **only** thing that re-enables them (`:283-288`), and it
>   is also the only consumer of `rx_error` (`:275`).
>
> So after one framing/parity/break/overrun error, a client that only ever
> polls with `timeout_ms == 0` would find the ring empty forever and never
> reach `try_read`. RX would be permanently dead until reboot — for exactly
> the `uart_poll_in` consumer this optimisation targets. Worse, this branch
> *raises* the trigger probability, because callers can now select framing and
> get it wrong.
>
> `poll_once` reaches `try_read`, which on an empty ring with an error set
> returns `Poll::Ready(Err(e))` rather than `Pending` (`:274-281`) and
> re-enables the interrupts on the way out. Full performance win, hazard
> closed.
>
> Two incidental corrections came with it. The original also said to add
> `embedded-io = "0.6"`; that was wrong twice over — `embedded-io-async`
> resolves to **0.7.1**, and `embassy-rp` implements
> `embedded_io_async::ReadReady` (`:650`), a different trait generation. The
> `poll_once` form needs neither: `embassy-futures` was already a direct
> dependency. **No manifest or lockfile change is required for this task.**

        // Well inside the default dispatch budget, so no declaration needed.
        match with_timeout(Duration::from_millis(1), AsyncRead::read(&mut context.uart, buf)).await {
            Ok(Ok(n)) => Ok(&context.buf[..n]),
            Ok(Err(_)) => Err(UartError::Other),
            Err(_) => Ok(&[]),
        }
    } else {
```

- [ ] **Step 2: Add the imports**

Extend the `hw-rev2` import block at the top of `handlers/uart.rs`:

```rust
#[cfg(feature = "hw-rev2")]
use core::task::Poll;
#[cfg(feature = "hw-rev2")]
use embassy_futures::poll_once;
#[cfg(feature = "hw-rev2")]
use embedded_io_async::{Read as AsyncRead, Write as AsyncWrite};
```

**No manifest change.** `embassy-futures` is already a direct dependency of the
firmware crate and is already used in `main.rs`. Do not add `embedded-io`; see
the correction note above for why the original instruction to do so was wrong.

- [ ] **Step 3: Build and lint both revisions**

```bash
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/handlers/uart.rs
git commit -m "perf(firmware): Return immediately from an empty UART poll

uart/read with timeout_ms == 0 wrapped the read in with_timeout(1ms),
so an empty poll always waited out the full millisecond. Measured
in-process against board 5256657D8A5D7F03, one such poll cost 1422 us
against 335 us for a single-byte write.

Poll the read future exactly once instead. Deliberately not
ReadReady::read_ready(): that inspects only the software RX ring, and
on an RX error the ISR latches rx_error and disables the RX interrupts,
which only try_read consumes and re-enables. A client polling solely
with timeout_ms == 0 would then find the ring empty forever and never
reach try_read, leaving RX dead until reboot -- for exactly the
uart_poll_in consumer this optimisation targets. A single poll runs
try_read, so the error surfaces instead of latching.

Dropping the future on Pending is safe: try_read never pops bytes and
then returns Pending.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 7: Host library

**Files:**
- Modify: `crates/pico-de-gallo-lib/src/lib.rs:1820-1829`

- [ ] **Step 1: Change the signature**

Replace lines 1820-1829:

```rust
    /// Set UART bus configuration parameters.
    ///
    /// Changes baud rate and framing. Takes effect immediately, before the
    /// next UART operation. The power-on default is 115200 8N1.
    ///
    /// All four parameters are applied together; there is no partial update.
    /// To change only the baud rate, read [`Self::uart_get_config`] first and
    /// pass its framing fields back.
    ///
    /// Returns [`UartError::InvalidBaudRate`] if `baud_rate` is zero, and
    /// [`UartError::Unsupported`] if the firmware's hardware revision has no
    /// UART.
    pub async fn uart_set_config(
        &self,
        baud_rate: u32,
        data_bits: UartDataBits,
        parity: UartParity,
        stop_bits: UartStopBits,
    ) -> Result<(), PicoDeGalloError<UartError>> {
        self.bounded()
            .send_resp::<UartSetConfiguration>(&UartSetConfigurationRequest {
                baud_rate,
                data_bits,
                parity,
                stop_bits,
            })
            .await?
            .map_err(PicoDeGalloError::Endpoint)
    }
```

- [ ] **Step 2: Re-export the enums**

This is load-bearing, not housekeeping. `pico-de-gallo-lib` is how every other
host crate sees the wire types — the FFI imports them as
`use pico_de_gallo_lib::{... UartError}`, not from `pico-de-gallo-internal`
directly. Task 8's tests import `pico_de_gallo_lib::UartDataBits` and will not
compile without this.

Extend the existing re-export block near the top of
`crates/pico-de-gallo-lib/src/lib.rs`:

```rust
pub use pico_de_gallo_internal::{
    AdcChannel, AdcConfigurationInfo, Capabilities, DeviceInfo, GpioDirection, GpioEdge, GpioEvent, GpioPull,
    GpioState, I2cBatchOp, I2cFrequency, PwmConfigurationInfo, PwmDutyCycleInfo, SpiBatchOp, SpiConfigurationInfo,
    SpiPhase, SpiPolarity, UartConfigurationInfo, UartDataBits, UartParity, UartStopBits, VersionInfo,
};
```

Also update the `uart_get_config` doc at lines 1831-1834 to say it returns
baud rate **and framing** rather than "the active baud rate".

- [ ] **Step 3: Build**

```bash
cargo check -p pico-de-gallo-lib --locked
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/pico-de-gallo-lib/src/lib.rs
git commit -m "feat(lib): Take UART framing in uart_set_config

uart_set_config gains data_bits, parity and stop_bits; the three enums
are re-exported so the FFI, CLI, MCP and Python bindings see them
through this crate as they already do for every other wire type.

All four parameters are applied together, so a caller changing only the
baud rate must read the current configuration and echo the framing back.
That is the wire contract, not a limitation of this signature.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 8: C FFI

**Files:**
- Modify: `crates/pico-de-gallo-ffi/src/lib.rs` (enums near `GalloGpioPull`; functions at `:2678` and `:2718`)
- Modify: `crates/pico-de-gallo-ffi/cbindgen.toml:39-46`

- [ ] **Step 1: Write the failing ABI-correspondence test**

Add to the FFI crate's test module:

```rust
#[test]
fn uart_config_enums_match_wire_enums() {
    use pico_de_gallo_lib::{UartDataBits, UartParity, UartStopBits};

    assert_eq!(GalloUartDataBits::Five as u8, UartDataBits::Five as u8);
    assert_eq!(GalloUartDataBits::Six as u8, UartDataBits::Six as u8);
    assert_eq!(GalloUartDataBits::Seven as u8, UartDataBits::Seven as u8);
    assert_eq!(GalloUartDataBits::Eight as u8, UartDataBits::Eight as u8);

    assert_eq!(GalloUartParity::None as u8, UartParity::None as u8);
    assert_eq!(GalloUartParity::Odd as u8, UartParity::Odd as u8);
    assert_eq!(GalloUartParity::Even as u8, UartParity::Even as u8);
    assert_eq!(GalloUartParity::Mark as u8, UartParity::Mark as u8);
    assert_eq!(GalloUartParity::Space as u8, UartParity::Space as u8);

    assert_eq!(GalloUartStopBits::One as u8, UartStopBits::One as u8);
    assert_eq!(GalloUartStopBits::Two as u8, UartStopBits::Two as u8);
}

#[test]
fn uart_set_config_rejects_out_of_range_framing() {
    // Null context is checked first, so these exercise argument validation
    // only once a real handle exists. Range checks that precede the null
    // check would be a behaviour change; these assert the documented order.
    assert_eq!(
        unsafe { gallo_uart_set_config(std::ptr::null(), 115_200, 0, 0, 0) },
        Status::Uninitialized
    );
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p pico-de-gallo-ffi --locked uart_config_enums
```

Expected: FAIL, `cannot find type GalloUartDataBits`.

- [ ] **Step 3: Add the mirror enums**

Insert after the `GalloGpioEdge` enum in `crates/pico-de-gallo-ffi/src/lib.rs`:

```rust
/// UART word length, as accepted by [`gallo_uart_set_config`].
///
/// Values are stable C ABI and mirror `pico_de_gallo_internal::UartDataBits`,
/// whose variant order is itself wire ABI. `uart_config_enums_match_wire_enums`
/// pins the correspondence.
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloUartDataBits {
    /// Five data bits.
    Five = 0,
    /// Six data bits.
    Six = 1,
    /// Seven data bits.
    Seven = 2,
    /// Eight data bits. The power-on default.
    Eight = 3,
}

/// UART parity mode, as accepted by [`gallo_uart_set_config`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloUartParity {
    /// No parity bit. The power-on default.
    None = 0,
    /// Odd parity.
    Odd = 1,
    /// Even parity.
    Even = 2,
    /// Parity bit always 1.
    Mark = 3,
    /// Parity bit always 0.
    Space = 4,
}

/// UART stop-bit count, as accepted by [`gallo_uart_set_config`].
/// cbindgen:prefix-with-name=true
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalloUartStopBits {
    /// One stop bit. The power-on default.
    One = 0,
    /// Two stop bits.
    Two = 1,
}
```

- [ ] **Step 4: Extend `gallo_uart_set_config`**

Replace lines 2678-2702 with:

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_set_config(
    gallo: *const PicoDeGallo,
    baud_rate: u32,
    data_bits: u8,
    parity: u8,
    stop_bits: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if baud_rate == 0 {
        eprintln!("Invalid baud rate: 0");
        return Status::InvalidArgument;
    }

    let data_bits = match data_bits {
        0 => UartDataBits::Five,
        1 => UartDataBits::Six,
        2 => UartDataBits::Seven,
        3 => UartDataBits::Eight,
        other => {
            eprintln!("Invalid data_bits: {other}");
            return Status::InvalidArgument;
        }
    };

    let parity = match parity {
        0 => UartParity::None,
        1 => UartParity::Odd,
        2 => UartParity::Even,
        3 => UartParity::Mark,
        4 => UartParity::Space,
        other => {
            eprintln!("Invalid parity: {other}");
            return Status::InvalidArgument;
        }
    };

    let stop_bits = match stop_bits {
        0 => UartStopBits::One,
        1 => UartStopBits::Two,
        other => {
            eprintln!("Invalid stop_bits: {other}");
            return Status::InvalidArgument;
        }
    };

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.uart_set_config(baud_rate, data_bits, parity, stop_bits));

    match result {
        Ok(()) => Status::Ok,
        Err(e) => uart_error_to_status(e),
    }
}
```

Update its doc comment above line 2678 to document the three new parameters
and name the `GalloUart*` enums as the source of valid values.

- [ ] **Step 5: Extend `gallo_uart_get_config`**

Replace the function at line 2718 with:

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_uart_get_config(
    gallo: *const PicoDeGallo,
    out_baud_rate: *mut u32,
    out_data_bits: *mut u8,
    out_parity: *mut u8,
    out_stop_bits: *mut u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if out_baud_rate.is_null()
        || out_data_bits.is_null()
        || out_parity.is_null()
        || out_stop_bits.is_null()
    {
        eprintln!("Unexpected NULL output pointer");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { &*gallo };

    let result = block_on(gallo.0.uart_get_config());

    match result {
        Ok(info) => {
            // Safety: all four pointers were null-checked above, and the
            // caller contract requires them to be valid and writable.
            unsafe {
                *out_baud_rate = info.baud_rate;
                *out_data_bits = info.data_bits as u8;
                *out_parity = info.parity as u8;
                *out_stop_bits = info.stop_bits as u8;
            }
            Status::Ok
        }
        Err(e) => uart_error_to_status(e),
    }
}
```

Update its doc comment above the function to describe all four outputs and
name `GalloUartDataBits`, `GalloUartParity` and `GalloUartStopBits` as the
meaning of the three `uint8_t` values.

> This is a breaking C ABI change to an existing function's arity. That is
> acceptable because the crate is unreleased at its current version and the
> whole wire change is unreleased too, but it must appear in the FFI
> CHANGELOG's `### Breaking Changes` section in Task 15.

- [ ] **Step 6: Export the enums to the header**

In `crates/pico-de-gallo-ffi/cbindgen.toml`, extend the `include` list at
lines 39-46 to:

```toml
include = [
    "GalloGpioDirection",
    "GalloGpioEdge",
    "GalloGpioPull",
    "GalloI2cBatchOpTag",
    "GalloI2cFrequency",
    "GalloSpiBatchOpTag",
    "GalloUartDataBits",
    "GalloUartParity",
    "GalloUartStopBits",
]
```

This is not optional. cbindgen emits only types reachable from an exported
signature, and these signatures take `uint8_t`. Omitting them means the enums
silently vanish from `pico_de_gallo.h`. See AGENTS.md §8.

- [ ] **Step 7: Verify the enums actually reach the header**

```bash
cargo build -p pico-de-gallo-ffi --locked
find . -name pico_de_gallo.h -newer crates/pico-de-gallo-ffi/cbindgen.toml \
  -exec grep -c 'GalloUartDataBits\|GalloUartParity\|GalloUartStopBits' {} +
```

Expected: a non-zero count. A zero means cbindgen pruned them — re-check
step 6 before continuing.

- [ ] **Step 8: Run the tests**

```bash
cargo test -p pico-de-gallo-ffi --locked
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/pico-de-gallo-ffi/src/lib.rs crates/pico-de-gallo-ffi/cbindgen.toml
git commit -m "feat(ffi): Expose UART framing configuration

gallo_uart_set_config gains data_bits, parity and stop_bits as uint8_t
with range validation, matching how I2cFrequency, GpioDirection and
GpioPull are already handled; gallo_uart_get_config gains the matching
out-parameters.

The three GalloUart* enums are listed in cbindgen.toml's export include
because the signatures take uint8_t and cbindgen emits only types
reachable from an exported signature - without the listing they are
silently pruned from the header.

No new Status value. Out-of-range framing maps onto the existing
InvalidArgument, because Status values are stable C ABI and consumers
are told to write an exhaustive switch with no default.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 9: CLI

**Files:**
- Modify: `crates/pico-de-gallo-app/src/lib.rs:502-509`, `:814`, `:1261-1277`

- [ ] **Step 1: Write the failing parse test**

Add near the existing CLI parse tests (around line 1887):

```rust
#[test]
fn uart_set_config_parses_framing() {
    let cli = Cli::try_parse_from([
        "gallo", "uart", "set-config",
        "--baud-rate", "9600",
        "--data-bits", "7",
        "--parity", "even",
        "--stop-bits", "2",
    ])
    .unwrap();

    match cli.command {
        Commands::Uart {
            command: UartCommands::SetConfig { baud_rate, data_bits, parity, stop_bits },
        } => {
            assert_eq!(baud_rate, 9600);
            assert_eq!(data_bits, DataBitsArg::Seven);
            assert_eq!(parity, ParityArg::Even);
            assert_eq!(stop_bits, StopBitsArg::Two);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn uart_set_config_framing_defaults_to_8n1() {
    let cli = Cli::try_parse_from(["gallo", "uart", "set-config", "--baud-rate", "115200"]).unwrap();

    match cli.command {
        Commands::Uart {
            command: UartCommands::SetConfig { data_bits, parity, stop_bits, .. },
        } => {
            assert_eq!(data_bits, DataBitsArg::Eight);
            assert_eq!(parity, ParityArg::None);
            assert_eq!(stop_bits, StopBitsArg::One);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn uart_set_config_rejects_nine_data_bits() {
    assert!(
        Cli::try_parse_from([
            "gallo", "uart", "set-config", "--baud-rate", "115200", "--data-bits", "9",
        ])
        .is_err(),
        "9 data bits is not representable on the RP2350 and must not parse"
    );
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p gallo --locked uart_set_config
```

Expected: FAIL, `cannot find type DataBitsArg`.

- [ ] **Step 3: Add the clap value enums**

Add near the other `ValueEnum` definitions in `crates/pico-de-gallo-app/src/lib.rs`:

```rust
/// UART word length. The RP2350 has no 9-bit mode, so 9 is not offered.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum DataBitsArg {
    #[value(name = "5")]
    Five,
    #[value(name = "6")]
    Six,
    #[value(name = "7")]
    Seven,
    #[value(name = "8")]
    Eight,
}

impl From<DataBitsArg> for UartDataBits {
    fn from(v: DataBitsArg) -> Self {
        match v {
            DataBitsArg::Five => Self::Five,
            DataBitsArg::Six => Self::Six,
            DataBitsArg::Seven => Self::Seven,
            DataBitsArg::Eight => Self::Eight,
        }
    }
}

/// UART parity. Mark and space use the PL011's stick-parity bit.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ParityArg {
    None,
    Odd,
    Even,
    Mark,
    Space,
}

impl From<ParityArg> for UartParity {
    fn from(v: ParityArg) -> Self {
        match v {
            ParityArg::None => Self::None,
            ParityArg::Odd => Self::Odd,
            ParityArg::Even => Self::Even,
            ParityArg::Mark => Self::Mark,
            ParityArg::Space => Self::Space,
        }
    }
}

/// UART stop bits. The PL011 has a single STP2 bit, so halves are not offered.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum StopBitsArg {
    #[value(name = "1")]
    One,
    #[value(name = "2")]
    Two,
}

impl From<StopBitsArg> for UartStopBits {
    fn from(v: StopBitsArg) -> Self {
        match v {
            StopBitsArg::One => Self::One,
            StopBitsArg::Two => Self::Two,
        }
    }
}
```

- [ ] **Step 4: Extend the subcommand**

Replace the `UartCommands::SetConfig` variant at lines 502-506:

```rust
    /// Set UART bus parameters
    SetConfig {
        /// UART baud rate in bits per second
        #[arg(short, long)]
        baud_rate: u32,

        /// Data bits per character
        #[arg(short, long, value_enum, default_value_t = DataBitsArg::Eight)]
        data_bits: DataBitsArg,

        /// Parity mode
        #[arg(short, long, value_enum, default_value_t = ParityArg::None)]
        parity: ParityArg,

        /// Stop bits
        #[arg(short, long, value_enum, default_value_t = StopBitsArg::One)]
        stop_bits: StopBitsArg,
    },
```

> Framing defaults to 8N1 so that existing `gallo uart set-config --baud-rate N`
> invocations keep working unchanged. This is a deliberate compatibility
> choice: without defaults, every scripted caller would break.

- [ ] **Step 5: Update the dispatch and handler**

At line 814, replace the match arm:

```rust
            UartCommands::SetConfig { baud_rate, data_bits, parity, stop_bits } => {
                self.uart_set_config(&pg, *baud_rate, (*data_bits).into(), (*parity).into(), (*stop_bits).into())
                    .await
            }
```

Replace `uart_set_config` at lines 1261-1267:

```rust
    async fn uart_set_config(
        &self,
        pg: &PicoDeGallo,
        baud_rate: u32,
        data_bits: UartDataBits,
        parity: UartParity,
        stop_bits: UartStopBits,
    ) -> Result<()> {
        pg.uart_set_config(baud_rate, data_bits, parity, stop_bits)
            .await
            .wrap_err("uart set-config failed")?;
        println!("UART configuration set to {baud_rate} {}", describe_framing(data_bits, parity, stop_bits));
        Ok(())
    }
```

Add the shared formatter near the other helpers:

```rust
/// Renders framing in the conventional "8N1" shorthand.
fn describe_framing(data_bits: UartDataBits, parity: UartParity, stop_bits: UartStopBits) -> String {
    let d = match data_bits {
        UartDataBits::Five => '5',
        UartDataBits::Six => '6',
        UartDataBits::Seven => '7',
        UartDataBits::Eight => '8',
    };
    let p = match parity {
        UartParity::None => 'N',
        UartParity::Odd => 'O',
        UartParity::Even => 'E',
        UartParity::Mark => 'M',
        UartParity::Space => 'S',
    };
    let s = match stop_bits {
        UartStopBits::One => '1',
        UartStopBits::Two => '2',
    };
    format!("{d}{p}{s}")
}
```

Replace the `get-config` print at line 1276:

```rust
        println!(
            "UART: {} bps {}",
            info.baud_rate,
            describe_framing(info.data_bits, info.parity, info.stop_bits)
        );
```

- [ ] **Step 6: Run the tests**

```bash
cargo test -p gallo --locked
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/pico-de-gallo-app/src/lib.rs
git commit -m "feat(application): Add UART framing flags to set-config

gallo uart set-config gains --data-bits, --parity and --stop-bits, and
get-config now prints framing in the conventional 8N1 shorthand.

All three default to the 8N1 power-on values so existing scripted
invocations that pass only --baud-rate keep working unchanged.

--data-bits offers 5..8 and --stop-bits offers 1 and 2, because the
RP2350 PL011 has a two-bit WLEN field and a single STP2 bit. Asking for
9 data bits is a parse error rather than a device round trip.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 10: MCP server

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/uart.rs:103-130`

- [ ] **Step 1: Extend the tool parameter struct**

Add three optional fields to the `uart_set_config` parameter struct, each
defaulting to the 8N1 value, described as strings so an agent gets readable
choices:

```rust
    /// Data bits per character: "5", "6", "7" or "8". Defaults to "8".
    /// The RP2350 has no 9-bit mode.
    #[serde(default)]
    pub data_bits: Option<String>,
    /// Parity: "none", "odd", "even", "mark" or "space". Defaults to "none".
    #[serde(default)]
    pub parity: Option<String>,
    /// Stop bits: "1" or "2". Defaults to "1". Half stop bits are not
    /// supported by the hardware.
    #[serde(default)]
    pub stop_bits: Option<String>,
```

- [ ] **Step 2: Parse them, rejecting unknown values locally**

In `uart_set_config` at line 117, parse each before touching the device, so an
agent gets the valid set rather than a device error:

```rust
        let data_bits = match p.data_bits.as_deref().unwrap_or("8") {
            "5" => UartDataBits::Five,
            "6" => UartDataBits::Six,
            "7" => UartDataBits::Seven,
            "8" => UartDataBits::Eight,
            other => {
                return Err(McpError::invalid_params(
                    format!("invalid data_bits {other:?}; expected \"5\", \"6\", \"7\" or \"8\" (the RP2350 has no 9-bit mode)"),
                    None,
                ));
            }
        };
```

        let parity = match p.parity.as_deref().unwrap_or("none") {
            "none" => UartParity::None,
            "odd" => UartParity::Odd,
            "even" => UartParity::Even,
            "mark" => UartParity::Mark,
            "space" => UartParity::Space,
            other => {
                return Err(McpError::invalid_params(
                    format!("invalid parity {other:?}; expected \"none\", \"odd\", \"even\", \"mark\" or \"space\""),
                    None,
                ));
            }
        };

        let stop_bits = match p.stop_bits.as_deref().unwrap_or("1") {
            "1" => UartStopBits::One,
            "2" => UartStopBits::Two,
            other => {
                return Err(McpError::invalid_params(
                    format!("invalid stop_bits {other:?}; expected \"1\" or \"2\" (the hardware has no half stop bits)"),
                    None,
                ));
            }
        };
```

Then pass all four:

```rust
        dev.uart_set_config(p.baud_rate, data_bits, parity, stop_bits)
```

- [ ] **Step 3: Extend the get-config response**

At line 108, return all four fields, spelling the framing exactly as
`uart_set_config` accepts it so a round trip between the two tools works:

```rust
        let c = dev.uart_get_config().await.map_err(map_pdg_err)?;
        let body = serde_json::json!({
            "baud_rate": c.baud_rate,
            "data_bits": match c.data_bits {
                UartDataBits::Five => "5",
                UartDataBits::Six => "6",
                UartDataBits::Seven => "7",
                UartDataBits::Eight => "8",
            },
            "parity": match c.parity {
                UartParity::None => "none",
                UartParity::Odd => "odd",
                UartParity::Even => "even",
                UartParity::Mark => "mark",
                UartParity::Space => "space",
            },
            "stop_bits": match c.stop_bits {
                UartStopBits::One => "1",
                UartStopBits::Two => "2",
            },
        });
```

Wrap it in the existing `{serial_number, result}` envelope this server uses
for every device response — do not return the bare body. That envelope is what
makes the target board observable per call; see the 2026-07-29 regression row.

- [ ] **Step 4: Build and test**

```bash
cargo test -p gallo-mcp --locked
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-mcp/src/uart.rs
git commit -m "feat(mcp): Accept and report UART framing

uart_set_config gains data_bits, parity and stop_bits, each optional and
defaulting to the 8N1 power-on value, and uart_get_config reports them.

Unknown values are refused locally with a message naming the valid set,
so an agent can self-correct instead of seeing a device error that does
not say what it should have sent.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 11: Python bindings

**Files:**
- Modify: `crates/pyco-de-gallo/src/lib.rs:44`, `:194`, `:574-590`, `:1546-1560`

- [ ] **Step 1: Add the pyclass enums**

Near the other `#[pyclass]` enums:

```rust
/// UART word length.
///
/// The RP2350 has no 9-bit mode, so only five through eight are available.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UartDataBits {
    Five,
    Six,
    Seven,
    Eight,
}
```

`Clone` and `from_py_object` are both required: PyO3 cannot extract a
`#[pyclass]` enum by value without them. See AGENTS.md §13.15.

```rust
/// UART parity mode.
///
/// ``Mark`` and ``Space`` use the PL011's stick-parity bit and are not
/// reachable through embassy's own parity type.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UartParity {
    None,
    Odd,
    Even,
    Mark,
    Space,
}

/// UART stop-bit count.
///
/// The hardware has a single STP2 bit, so half stop bits are unavailable.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UartStopBits {
    One,
    Two,
}
```

Add a `From` conversion for each, in both directions, following the pattern
the existing `I2cFrequency` wrapper uses:

```rust
impl From<UartDataBits> for LibUartDataBits {
    fn from(v: UartDataBits) -> Self {
        match v {
            UartDataBits::Five => Self::Five,
            UartDataBits::Six => Self::Six,
            UartDataBits::Seven => Self::Seven,
            UartDataBits::Eight => Self::Eight,
        }
    }
}

impl From<LibUartDataBits> for UartDataBits {
    fn from(v: LibUartDataBits) -> Self {
        match v {
            LibUartDataBits::Five => Self::Five,
            LibUartDataBits::Six => Self::Six,
            LibUartDataBits::Seven => Self::Seven,
            LibUartDataBits::Eight => Self::Eight,
        }
    }
}
```

Write the matching four impls for `UartParity` and `UartStopBits` the same
way, and add `UartDataBits as LibUartDataBits, UartParity as LibUartParity,
UartStopBits as LibUartStopBits` to the aliased import at line 44. The `Lib`
prefix is this crate's convention for avoiding collisions between the Python-
facing type and the wire type.

> `None` is legal as a Python enum member (`UartParity.None` parses), but if
> it proves awkward in practice rename it to `NoParity` and say so in the
> docstring. Do not silently renumber the variants.

- [ ] **Step 2: Register them**

At line 194, alongside `m.add_class::<UartConfigurationInfo>()?;`:

```rust
    m.add_class::<UartDataBits>()?;
    m.add_class::<UartParity>()?;
    m.add_class::<UartStopBits>()?;
```

- [ ] **Step 3: Extend `UartConfigurationInfo`**

Add the three fields to the struct at line 576 and to the
`From<LibUartConfigurationInfo>` impl at line 582.

- [ ] **Step 4: Extend `uart_set_config`**

Replace lines 1546-1548:

```rust
    /// Set the UART baud rate and framing.
    ///
    /// Args:
    ///     baud_rate: Baud rate in bits per second. Must be non-zero.
    ///     data_bits: Word length. Defaults to ``UartDataBits.Eight``.
    ///     parity: Parity mode. Defaults to ``UartParity.None``.
    ///     stop_bits: Stop bits. Defaults to ``UartStopBits.One``.
    ///
    /// Raises:
    ///     RuntimeError: If the device rejects the configuration, or the
    ///         hardware revision has no UART.
    #[pyo3(signature = (baud_rate, data_bits = UartDataBits::Eight, parity = UartParity::None, stop_bits = UartStopBits::One))]
    fn uart_set_config(
        &self,
        py: Python<'_>,
        baud_rate: u32,
        data_bits: UartDataBits,
        parity: UartParity,
        stop_bits: UartStopBits,
    ) -> PyResult<()> {
        self.block(
            py,
            self.inner
                .uart_set_config(baud_rate, data_bits.into(), parity.into(), stop_bits.into()),
        )
    }
```

- [ ] **Step 5: Build and test**

```bash
cargo test -p pyco-de-gallo --locked
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/pyco-de-gallo/src/lib.rs
git commit -m "feat(pyco): Expose UART framing configuration

The three framing enums derive Clone and use from_py_object, without
which PyO3 cannot extract them by value.

uart_set_config's framing arguments default to the 8N1 power-on values,
so existing callers passing only a baud rate keep working.

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 12: HAL doc correction

**Files:**
- Modify: `crates/pico-de-gallo-hal/src/lib.rs:1882-1884`

- [ ] **Step 1: Correct the comment**

`Uart` exposes no configuration surface, only `set_timeout_ms`, and that stays
true. Only the wording changes. Replace lines 1882-1884:

```rust
/// **Baud rate and framing are fixed** at whatever the device is currently
/// configured for and cannot be changed through this HAL — to change them,
/// depend on `pico-de-gallo-lib` directly and call
/// `PicoDeGallo::uart_set_config`.
```

- [ ] **Step 2: Verify no functional change**

```bash
cargo test -p pico-de-gallo-hal --locked
cargo hack --feature-powerset check -p pico-de-gallo-hal
```

Expected: PASS. The feature powerset matters here because `Uart`'s
`embedded-io` impls are multi-major by design; see AGENTS.md §7.3.

- [ ] **Step 3: Commit**

```bash
git add crates/pico-de-gallo-hal/src/lib.rs
git commit -m "docs(hal): Note that framing is also not configurable here

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 13: Full host gate

- [ ] **Step 1: Run everything CI runs**

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo check --workspace --locked
cargo deny --manifest-path Cargo.toml check
```

Expected: all clean. Test count should be the §5.5 baseline plus the roughly
12 tests added by Tasks 1, 2, 3, 8 and 9.

- [ ] **Step 2: Firmware gate, both revisions**

```bash
cd crates/pico-de-gallo-firmware
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1
cargo deny --manifest-path Cargo.toml check
```

Expected: all clean.

- [ ] **Step 3: Confirm no version moved**

```bash
git diff main --stat -- '*/Cargo.toml' 'Cargo.toml'
git diff main -- '*/Cargo.toml' | grep -E '^[+-]version =' || echo "no version changes - correct"
```

Expected: `no version changes - correct`. If a version moved, revert it — the
release bump happens once, separately.

---

## Task 14: Hardware verification

Nothing before this proves the framing reaches the wire. Board
`5256657D8A5D7F03`, hw-rev2, TX and RX shorted with no series resistors.

- [ ] **Step 1: Flash the new firmware**

Put the board in BOOTSEL, then from PowerShell:

```powershell
elf2uf2-rs convert -f rp2350-arm-s `
  "crates\pico-de-gallo-firmware\target\thumbv8m.main-none-eabihf\release\pico-de-gallo-firmware" `
  "$env:TEMP\pdg-fw.uf2"
Copy-Item "$env:TEMP\pdg-fw.uf2" F:\ -Force
```

Re-attach to WSL once it re-enumerates: `usbipd attach --wsl --busid 9-3`.

- [ ] **Step 2: Confirm which image is running**

```bash
gallo version
```

Expected: the `Build` row matches `git describe --always --dirty --tags --match 'firmware-v*'`.
Do not proceed on a mismatch — the #135 verification was misled by exactly this.

- [ ] **Step 3: Verify framing actually changes the wire**

Loopback alone cannot prove this, because both ends retune together and stale
framing round-trips just as cleanly. The discriminator is a **deliberate
mismatch**: transmit under one framing and receive under another.

```bash
gallo uart set-config --baud-rate 115200 --data-bits 8 --parity none --stop-bits 1
gallo uart read --count 1014 --timeout 0 >/dev/null   # drain
gallo uart write --bytes 0xFF 0x00 0x55 0xAA
# Immediately reframe. Bytes already in the RX ring were captured under 8N1.
gallo uart set-config --baud-rate 115200 --data-bits 7 --parity even --stop-bits 1
gallo uart read --count 8 --timeout 500
```

Expected: the received bytes differ from `ff 00 55 aa`, because a 7-bit
receiver masks the eighth bit and interprets it as parity.

If the reframe lands after the bytes are already buffered, this shows no
difference for the wrong reason. In that case do the reverse: reframe to 7E1
**first**, then write `0xFF 0x00 0x55 0xAA`, and confirm the loopback returns
7-bit-masked values (`7f 00 55 2a`) rather than the original bytes.

If neither is conclusive, capture the TX pin on a logic analyser and count
bits per character directly, in the style of the 2026-09-03 I2C measurement.
**Record which method actually produced the evidence** — do not claim a
verification that was not performed.

- [ ] **Step 4: Verify the read fast path**

Re-run the benchmark used to derive the 1422 µs figure:

```bash
cd /tmp/pdgbench && cargo run --release --quiet
```

Expected: `uart_read(1, empty ring)` drops from ~1422 µs to roughly the
`uart_write(1 byte)` figure (~335 µs), since both are now one USB round trip
with no added wait. `uart_write` should be unchanged.

- [ ] **Step 5: Verify 8N1 still round-trips and defaults hold**

```bash
gallo uart set-config --baud-rate 115200
gallo uart get-config          # expect: 115200 bps 8N1
gallo uart read --count 1014 --timeout 0 >/dev/null
gallo uart write --bytes 0xDE 0xAD 0xBE 0xEF
gallo uart read --count 4 --timeout 500     # expect: de ad be ef
```

- [ ] **Step 6: Verify rejection paths**

```bash
gallo uart set-config --baud-rate 0        # expect InvalidBaudRate, exit 1
gallo uart set-config --baud-rate 115200 --data-bits 9   # expect clap parse error
```

- [ ] **Step 7: Record the results**

Write the measured numbers, the board serial, the firmware `build_id`, and
**which** discriminator method worked into the design doc's §8.3, replacing the
predicted procedure with what was actually done. State any limit honestly — in
particular, whether mark and space parity were exercised at all.

- [ ] **Step 8: Commit the verification record**

```bash
git add docs/superpowers/specs/2026-09-10-zephyr-uart-driver-design.md
git commit -m "docs(repo): Record the Phase A hardware verification

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 15: Documentation

**Files:**
- Modify: `book/src/appendix/endpoints.md`, `book/src/internals/wire-protocol.md`, `book/src/interfaces/uart.md`, `book/src/crates/{app,ffi,lib,mcp,python}.md`
- Modify: `crates/{pico-de-gallo-internal,pico-de-gallo-lib,pico-de-gallo-ffi,pico-de-gallo-app,pico-de-gallo-mcp,pyco-de-gallo,pico-de-gallo-firmware}/CHANGELOG.md`

- [ ] **Step 1: Update the book**

- `wire-protocol.md` — document the three enums, their variant indices, and
  that `UartStopBits` deliberately does not match Zephyr's numbering.
- `endpoints.md` — update the `uart/set-config` and `uart/get-config` rows to
  say framing, not just frequency.
- `interfaces/uart.md` — a framing section with the capability matrix from the
  design's §3, including that 9 data bits, half stop bits and hardware flow
  control are not available, and that framing is applied together with baud.
- `crates/{app,ffi,lib,mcp,python}.md` — the new parameters.

- [ ] **Step 2: Verify every CLI snippet you touched still matches reality**

```bash
gallo uart set-config --help
gallo uart get-config --help
```

Any example in the book must match this output exactly. AGENTS.md §15.1
reviewer checklist item 2.

- [ ] **Step 3: Build the book**

```bash
mdbook build book
```

Expected: clean, no broken links.

- [ ] **Step 4: Write the CHANGELOG entries**

Add an `### Added` entry under `## [Unreleased]` in each affected crate's
`CHANGELOG.md`, in Keep a Changelog style, each citing `#152`. There is no root
`CHANGELOG.md`.

Three entries need more than a one-liner:

- **`crates/pico-de-gallo-ffi/CHANGELOG.md`** additionally needs a
  `### Breaking Changes` entry: `gallo_uart_set_config` and
  `gallo_uart_get_config` both changed arity, so any C consumer must be
  recompiled. Name the three `GalloUart*` enums as the source of valid values
  for the new `uint8_t` parameters.
- **`crates/pico-de-gallo-internal/CHANGELOG.md`** should state that the wire
  change rides the already-pending unreleased 0.8 schema bump and that no
  version moved, so a reader does not go looking for the bump that would
  normally accompany it.
- **`crates/pico-de-gallo-firmware/CHANGELOG.md`** should list the read fast
  path as a separate, independent change from the framing work, with the
  measured before-and-after numbers from Task 14 step 4. They were motivated
  by the same consumer but neither depends on the other.

- [ ] **Step 5: Commit**

```bash
git add book crates/*/CHANGELOG.md
git commit -m "docs(repo): Document UART framing configuration

Assisted-by: OpenCode:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Phase A done

At this point `gallo uart set-config --baud-rate 9600 --data-bits 7 --parity even`
works end to end and is hardware-verified, and `uart/read`'s idle poll is cheap.

Phase B — the Zephyr `pdg_uart` driver — is planned next, against the FFI
signatures this phase actually produced rather than predicted ones.

Do **not** open the PR yet: AGENTS.md §15.1 requires the Zephyr driver and its
docs in the same logical change, and the `GalloUart*` enums exist to serve it.
