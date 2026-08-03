# gallo-mcp Per-Call Serial Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the MCP server's bound Pico de Gallo board selectable per tool call, echoed in every response, and impossible to guess wrong when two or more boards are attached.

**Architecture:** A new pure module `select.rs` decides which board a call targets from `(attached serials, server pin, per-call argument)`. `GalloMcp::connect` takes that decision and always opens by serial. Every device tool gains an optional `serial_number` argument and returns `{serial_number, result}`. `status` and `list_devices` build their responses through pure functions so their JSON shapes are unit-testable without hardware.

**Tech Stack:** Rust 2024 (MSRV 1.90), `rmcp` 2.2.0 (`#[tool]` / `#[tool_router]` macros), `schemars` 1.2, `serde`, `tokio`, `pico-de-gallo-lib` 0.7.1.

**Spec:** `docs/superpowers/specs/2026-07-29-gallo-mcp-per-call-serial-design.md`
**Issue:** https://github.com/OpenDevicePartnership/pico-de-gallo/issues/89
**Branch:** `feat/mcp-per-call-serial`

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/pico-de-gallo-mcp/src/select.rs` | **New.** Selection policy: `SelectError`, `resolve_target`, error text, `ErrorData` mapping, and the shared `TargetParams`. Pure — no USB, no async. |
| `crates/pico-de-gallo-mcp/src/lib.rs` | `Device` carries the resolved serial; `connect(requested)`; `Envelope` + `ok_device_json`; `attached_serials()`; server instructions; surface-wide schema test; hardware tests. |
| `crates/pico-de-gallo-mcp/src/device.rs` | `list_devices` object shape, rewritten `status`, and `device_info` / `version` / `ping`. Response building extracted into pure functions. |
| `crates/pico-de-gallo-mcp/src/{i2c,spi,uart,gpio,pwm,adc,onewire}.rs` | `serial_number` on each params struct; thread into `connect`; return through the envelope. |
| `crates/pico-de-gallo-mcp/README.md`, `book/src/crates/mcp.md`, `crates/pico-de-gallo-mcp/CHANGELOG.md`, `AGENTS.md` | Documentation parity (AGENTS.md §15.1). |

## Amendments During Execution

Task 1's code review found a real hole the plan missed, so the shipped
`select.rs` differs from the Task 1 text below. Later tasks assume the
**shipped** shape:

- `SelectError` has a sixth variant, `Duplicate { serial, count }`. Two boards
  can report the same serial: the firmware falls back to chip ID `0`
  (`crates/pico-de-gallo-firmware/src/main.rs:243-246`) when the OTP read
  fails, which formats to the constant `"0000000000000000"`. `resolve_target`
  now counts matches rather than testing membership, on both the pinned and
  the requested path, and refuses a serial that names more than one board.
- `SelectError::Ambiguous` no longer carries `total`; it is derived as
  `available.len() + unaddressable` where the message needs it.
- `resolve_target`'s pin parameter and the `PinConflict` / `PinnedNotFound`
  fields are named `pinned_serial`, not `pin`, because `pin` means a GPIO pin
  everywhere else in this repository. Task 7 below already reflects this.
- The `list` formatter is named `format_serials`.
- `map_select_err` classifies `Ambiguous` and `NotFound` as `internal_error`
  when `available` is empty — no argument can fix a bench where nothing is
  addressable.

Commits: `d37f626c` (Task 1 as planned), `d378cfac` (review fixes).

Task 2's code review found a messaging bug and some structure worth changing, so
the shipped `lib.rs` also differs from the Task 2 text below:

- `connect` no longer contains the retry loop. It was extracted to a free
  `async fn open_with_retry(serial: Option<&str>)`, which owns `MAX_ATTEMPTS`
  and `BACKOFF`. `connect` now reads lock → resolve → open → validate → reset →
  construct.
- The not-found message choice was extracted to a pure
  `fn vanished_board_msg(serial: Option<&str>) -> String`, and both its arms now
  say the board vanished. The plan's `None` arm said "no device attached",
  which is **unreachable in that literal meaning**: `serial` is `None` only when
  `resolve_target` returned `Ok(None)`, which requires exactly one attached
  board, so reaching that arm always means the board went away between
  enumeration and open.
- `Envelope` dropped its redundant `T: Serialize` bound (the derive regenerates
  it on the impl).

Commits: `992ac875` (Task 2 as planned), `75955228` (review fixes).

### Deferred, deliberately out of scope for this branch

- **`Device::serial()` stores intent, not proof, on the `try_new()` path.** When
  the sole attached board reports no serial, `connect` opens by VID/PID; a
  hot-swap inside the resolve→open window could label a different board `null`.
  The clean fix is for `pico-de-gallo-lib` to expose the serial it actually
  opened. Narrow (needs a serial-less board *and* a millisecond-window swap).
- **`pico_de_gallo_lib::list_devices()` swallows `nusb`'s error into an empty
  `Vec`**, so a driver or permissions failure surfaces as "no device attached",
  which is unactionable. Pre-existing; fixing it changes a published crate's
  signature.
- **`uart_read`'s `timeout_ms` description does not say `0` is legal.** The
  GPIO wait tools document "must be non-zero" and enforce it, because there `0`
  means an unbounded wait — the device-wedging hazard in AGENTS.md §13.17. For
  UART reads the firmware treats `0` as a supported non-blocking poll, so the
  asymmetry is correct behaviour but invisible to an agent reading both
  schemas. Worth a sentence in Task 10.
- **`uart_set_config` does not reject `baud_rate == 0` host-side.** The firmware
  rejects it cleanly, so this costs only a lock acquisition, a USB claim, a
  `validate()` round trip and a subscription reset to learn. It is the one
  cheaply-checkable argument in the converted modules that is not checked; the
  `gallo` CLI behaves the same way. Recorded as a decision, not an oversight.

## Task Order and Parallelism

Task 1 → Task 2 → **Tasks 3-6 are independent**: they touch disjoint peripheral
files and depend only on Task 2's `connect` signature and `ok_device_json`. Task
7 runs after them because it also edits `lib.rs`. Then Task 8 → Task 9 → Task
10, in order.

Independent is not the same as parallelisable here. Tasks 3-6 were executed
**sequentially**, because every task ends in a commit and a single checkout has
one git index — concurrent implementers would race on `index.lock` and could
stage each other's work. Run them in parallel only from separate worktrees.

## Conventions For Every Task

- **Line endings:** run `dos2unix <file>` on any file you create. See AGENTS.md §3.
- **Commit trailers:** every commit body ends with, and never includes `Signed-off-by`:

```text
Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

- **Commit command:** PowerShell has no heredoc. Write the message to a temp file and use `git commit -F`:

```powershell
Set-Content -Path "$env:TEMP\msg.txt" -Value $msg -NoNewline
dos2unix -q "$env:TEMP\msg.txt"
git commit -F "$env:TEMP\msg.txt"
```

- **Verification after every task** (run from `crates/pico-de-gallo-mcp`):

```bash
cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked
```

Expected: no output from `fmt`, no warnings from `clippy`, all tests pass.

### Conversion rules learned in Tasks 2-4

The peripheral conversions are near-identical, and two rules emerged from
review that Tasks 5-6 should apply mechanically rather than by judgement.

**Swap the `ok_json` import, do not extend it.** Each converted module's import
becomes:

```rust
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
```

With `ok_json` out of scope, a call site you forget to convert is a hard
`E0425`. Keeping both imports silently defeats that guard.

**Declaring the field is not enough — thread it.** A handler that gains
`serial_number` on its params struct and still calls `connect(None)` compiles,
passes the registration test, passes the deserialization test, and reproduces
the exact bug this branch exists to fix: with one board the argument is
silently dropped, and with two boards the agent is told `serial_number` is
required in response to a call that supplied it. The compiler only catches this
where the params struct is `TargetParams` alone, because only there is the
binding unused. Read every `connect` call site rather than trusting a grep.

**Widen a pre-existing params test only when it covers a struct — or a
structural placement — that the module's new `serial_number` test does not.**
Field count is irrelevant. Task 3's SPI widenings were correct because
`transfer_params_deserialize` and `batch_params_deserialize` cover two structs
the new test never touches. Task 4's UART widening was wrong because it hit the
same struct with the same JSON literal as the new test, and in doing so dropped
the only assertion that the *legacy* payload still deserializes its sibling
fields. When in doubt, add nothing: the surface-wide guards in Task 8 cover all
28 structs better than a per-module test can.

**Match the reference spacing.** A blank line between each params struct, each
handler in the `#[tool_router]` impl, and each test function. `cargo fmt` does
not enforce this, and Tasks 5-6 copy whichever module the author has open.

---

### Task 1: Selection policy module

**Files:**
- Create: `crates/pico-de-gallo-mcp/src/select.rs`
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs` (add `pub mod select;`)

This module is pure: it never touches USB and has no async. That is the whole
point — the 0/1/N matrix, the pin rules, and every error string become
unit-testable with no board attached.

- [ ] **Step 1: Create the module file with types and a stub, and register it**

Create `crates/pico-de-gallo-mcp/src/select.rs`:

```rust
//! Device selection policy.
//!
//! Which board a tool call targets is decided here, by a pure function over
//! the attached serials, the server's `--serial-number` pin, and the call's
//! optional `serial_number` argument. Keeping this free of USB lets the whole
//! zero/one/many matrix — and every error string an agent will read — be
//! unit-tested with no hardware present.

use rmcp::ErrorData;

/// The optional per-call device selector.
///
/// Shared by every device tool that takes no other argument. Tools with their
/// own parameters declare an identical `serial_number` field instead.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetParams {
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

/// Why device selection failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    /// No Pico de Gallo is attached at all.
    NoDevice,
    /// Two or more boards are attached and the call named none of them.
    Ambiguous {
        /// Serials that can be addressed.
        available: Vec<String>,
        /// Attached boards reporting no USB serial, which cannot be named.
        unaddressable: usize,
        /// Total attached boards.
        total: usize,
    },
    /// The requested serial is not among the attached boards.
    NotFound {
        /// The serial the call asked for.
        requested: String,
        /// Serials that can be addressed.
        available: Vec<String>,
    },
    /// The server is pinned and the call asked for a different board.
    PinConflict {
        /// The server's `--serial-number`.
        pin: String,
        /// The serial the call asked for.
        requested: String,
    },
    /// The server is pinned to a board that is not attached.
    PinnedNotFound {
        /// The server's `--serial-number`.
        pin: String,
        /// Serials that can be addressed.
        available: Vec<String>,
    },
}

/// Decide which board a tool call targets.
///
/// `attached` is the serial of every attached board in enumeration order;
/// a `None` entry is a board that reports no USB serial number.
///
/// Returns the serial to open. `Ok(None)` means "open the sole attached
/// board, which reports no serial" — the only case where a target cannot be
/// named.
pub fn resolve_target(
    _attached: &[Option<String>],
    _pin: Option<&str>,
    _requested: Option<&str>,
) -> Result<Option<String>, SelectError> {
    todo!("implemented in step 3")
}
```

Add to `crates/pico-de-gallo-mcp/src/lib.rs`, in the module list after
`pub mod pwm;` (keep alphabetical order):

```rust
pub mod select;
```

Then run `dos2unix crates/pico-de-gallo-mcp/src/select.rs`.

- [ ] **Step 2: Write the failing policy tests**

Append to `crates/pico-de-gallo-mcp/src/select.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `attached` slice from string serials.
    fn attached(serials: &[&str]) -> Vec<Option<String>> {
        serials.iter().map(|s| Some((*s).to_string())).collect()
    }

    const A: &str = "9A54ED7E3A1D9D98";
    const B: &str = "5256657D8A5D7F03";

    #[test]
    fn no_devices_is_no_device_regardless_of_arguments() {
        assert_eq!(resolve_target(&[], None, None), Err(SelectError::NoDevice));
        assert_eq!(
            resolve_target(&[], None, Some(A)),
            Err(SelectError::NoDevice)
        );
        assert_eq!(
            resolve_target(&[], Some(A), None),
            Err(SelectError::NoDevice)
        );
    }

    #[test]
    fn sole_device_resolves_without_an_argument() {
        assert_eq!(
            resolve_target(&attached(&[A]), None, None),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn sole_device_accepts_its_own_serial() {
        assert_eq!(
            resolve_target(&attached(&[A]), None, Some(A)),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn sole_device_rejects_a_different_serial() {
        assert_eq!(
            resolve_target(&attached(&[A]), None, Some(B)),
            Err(SelectError::NotFound {
                requested: B.to_string(),
                available: vec![A.to_string()],
            })
        );
    }

    #[test]
    fn two_devices_without_an_argument_are_ambiguous() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, None),
            Err(SelectError::Ambiguous {
                available: vec![A.to_string(), B.to_string()],
                unaddressable: 0,
                total: 2,
            })
        );
    }

    #[test]
    fn two_devices_resolve_either_by_serial() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, Some(A)),
            Ok(Some(A.to_string()))
        );
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, Some(B)),
            Ok(Some(B.to_string()))
        );
    }

    #[test]
    fn two_devices_reject_an_unknown_serial() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, Some("BOGUS")),
            Err(SelectError::NotFound {
                requested: "BOGUS".to_string(),
                available: vec![A.to_string(), B.to_string()],
            })
        );
    }

    #[test]
    fn pin_wins_without_an_argument_even_with_two_attached() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), Some(A), None),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn pin_accepts_a_matching_argument() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), Some(A), Some(A)),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn pin_rejects_a_conflicting_argument() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), Some(A), Some(B)),
            Err(SelectError::PinConflict {
                pin: A.to_string(),
                requested: B.to_string(),
            })
        );
    }

    #[test]
    fn pin_conflict_is_reported_before_absence() {
        // B is attached, A (the pin) is not. The conflict is the agent's
        // actionable mistake, so it must win over PinnedNotFound.
        assert_eq!(
            resolve_target(&attached(&[B]), Some(A), Some(B)),
            Err(SelectError::PinConflict {
                pin: A.to_string(),
                requested: B.to_string(),
            })
        );
    }

    #[test]
    fn pinned_board_not_attached_is_reported() {
        assert_eq!(
            resolve_target(&attached(&[B]), Some(A), None),
            Err(SelectError::PinnedNotFound {
                pin: A.to_string(),
                available: vec![B.to_string()],
            })
        );
    }

    #[test]
    fn sole_serialless_board_resolves_to_none() {
        assert_eq!(resolve_target(&[None], None, None), Ok(None));
    }

    #[test]
    fn sole_serialless_board_cannot_be_named() {
        assert_eq!(
            resolve_target(&[None], None, Some(A)),
            Err(SelectError::NotFound {
                requested: A.to_string(),
                available: vec![],
            })
        );
    }

    #[test]
    fn two_serialless_boards_are_ambiguous_with_nothing_to_list() {
        assert_eq!(
            resolve_target(&[None, None], None, None),
            Err(SelectError::Ambiguous {
                available: vec![],
                unaddressable: 2,
                total: 2,
            })
        );
    }

    #[test]
    fn mixed_boards_list_only_the_addressable_ones() {
        let mixed = vec![Some(A.to_string()), None, None];
        assert_eq!(
            resolve_target(&mixed, None, None),
            Err(SelectError::Ambiguous {
                available: vec![A.to_string()],
                unaddressable: 2,
                total: 3,
            })
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --locked -p gallo-mcp select::`
Expected: every test panics at `not yet implemented` from the `todo!`.

- [ ] **Step 4: Implement `resolve_target`**

Replace the stub body in `crates/pico-de-gallo-mcp/src/select.rs`:

```rust
pub fn resolve_target(
    attached: &[Option<String>],
    pin: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>, SelectError> {
    if attached.is_empty() {
        return Err(SelectError::NoDevice);
    }
    // Boards that report no USB serial cannot be named, so they never appear
    // in an error's `available` list — but they still count toward ambiguity.
    let available: Vec<String> = attached.iter().flatten().cloned().collect();

    if let Some(pin) = pin {
        if let Some(req) = requested
            && req != pin
        {
            return Err(SelectError::PinConflict {
                pin: pin.to_string(),
                requested: req.to_string(),
            });
        }
        if !available.iter().any(|s| s == pin) {
            return Err(SelectError::PinnedNotFound {
                pin: pin.to_string(),
                available,
            });
        }
        return Ok(Some(pin.to_string()));
    }

    match requested {
        Some(req) if available.iter().any(|s| s == req) => Ok(Some(req.to_string())),
        Some(req) => Err(SelectError::NotFound {
            requested: req.to_string(),
            available,
        }),
        // Exactly one board is unambiguous even when it reports no serial,
        // which is the only way `Ok(None)` is produced.
        None if attached.len() == 1 => Ok(attached[0].clone()),
        None => Err(SelectError::Ambiguous {
            unaddressable: attached.len() - available.len(),
            total: attached.len(),
            available,
        }),
    }
}
```

Also remove the `todo!` stub's `unimplemented` marker by replacing the whole
function. Leave `use rmcp::ErrorData;` in place — it is first used in step 8.
Clippy is not run until step 10, so an intermediate unused-import warning here
is expected and resolves itself.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --locked -p gallo-mcp select::`
Expected: 16 tests pass.

- [ ] **Step 6: Write failing error-text and mapping tests**

Append inside the existing `mod tests` block in `select.rs`:

```rust
    #[test]
    fn ambiguous_text_names_every_serial_and_the_argument() {
        let msg = resolve_target(&attached(&[A, B]), None, None)
            .unwrap_err()
            .to_string();
        // Issue #89 §3 specifies this wording; the agent recovers from it.
        assert!(msg.contains("`serial_number` is required"), "{msg}");
        assert!(msg.contains(A), "{msg}");
        assert!(msg.contains(B), "{msg}");
    }

    #[test]
    fn ambiguous_text_explains_unaddressable_boards() {
        let mixed = vec![Some(A.to_string()), None, None];
        let msg = resolve_target(&mixed, None, None).unwrap_err().to_string();
        assert!(msg.contains("3 Pico de Gallo devices"), "{msg}");
        assert!(msg.contains("2 of them report no USB serial number"), "{msg}");
        assert!(msg.contains(A), "{msg}");
    }

    #[test]
    fn not_found_text_names_the_request_and_the_alternatives() {
        let msg = resolve_target(&attached(&[A, B]), None, Some("BOGUS"))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("'BOGUS'"), "{msg}");
        assert!(msg.contains(A) && msg.contains(B), "{msg}");
    }

    #[test]
    fn pin_conflict_text_tells_the_agent_what_to_do_instead() {
        let msg = resolve_target(&attached(&[A, B]), Some(A), Some(B))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("--serial-number"), "{msg}");
        assert!(msg.contains("Omit serial_number"), "{msg}");
        assert!(msg.contains(A) && msg.contains(B), "{msg}");
    }

    #[test]
    fn pinned_not_found_text_names_the_pin() {
        let msg = resolve_target(&attached(&[B]), Some(A), None)
            .unwrap_err()
            .to_string();
        assert!(msg.contains(A), "{msg}");
        assert!(msg.contains("not attached"), "{msg}");
    }

    #[test]
    fn agent_fixable_errors_map_to_invalid_params() {
        for err in [
            SelectError::Ambiguous {
                available: vec![A.to_string()],
                unaddressable: 0,
                total: 2,
            },
            SelectError::NotFound {
                requested: B.to_string(),
                available: vec![A.to_string()],
            },
            SelectError::PinConflict {
                pin: A.to_string(),
                requested: B.to_string(),
            },
        ] {
            assert_eq!(
                map_select_err(err.clone()).code,
                rmcp::model::ErrorCode::INVALID_PARAMS,
                "{err:?} should be invalid_params"
            );
        }
    }

    #[test]
    fn environment_errors_map_to_internal_error() {
        for err in [
            SelectError::NoDevice,
            SelectError::PinnedNotFound {
                pin: A.to_string(),
                available: vec![],
            },
        ] {
            assert_eq!(
                map_select_err(err.clone()).code,
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                "{err:?} should be internal_error"
            );
        }
    }

    #[test]
    fn empty_available_list_reads_sensibly() {
        let msg = resolve_target(&[None, None], None, None)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("(none addressable)"), "{msg}");
    }
```

- [ ] **Step 7: Run to verify they fail**

Run: `cargo test --locked -p gallo-mcp select::`
Expected: compile error — `to_string` is unavailable on `SelectError` (no
`Display`) and `map_select_err` does not exist.

- [ ] **Step 8: Implement `Display` and `map_select_err`**

Insert into `crates/pico-de-gallo-mcp/src/select.rs`, after the `SelectError`
definition and before `resolve_target`:

```rust
/// Render a serial list for an error message.
///
/// One formatter for every message, so the list format cannot drift between
/// error cases.
fn list(available: &[String]) -> String {
    if available.is_empty() {
        "(none addressable)".to_string()
    } else {
        available.join(", ")
    }
}

impl std::fmt::Display for SelectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => {
                write!(f, "No Pico de Gallo device attached: connect one and retry.")
            }
            Self::Ambiguous {
                available,
                unaddressable: 0,
                ..
            } => write!(
                f,
                "Multiple Pico de Gallo devices attached; `serial_number` is required.\n\
                 Available: {}",
                list(available)
            ),
            Self::Ambiguous {
                available,
                unaddressable,
                total,
            } => write!(
                f,
                "{total} Pico de Gallo devices attached; `serial_number` is required, \
                 but {unaddressable} of them report no USB serial number and cannot \
                 be addressed.\nAvailable: {}",
                list(available)
            ),
            Self::NotFound {
                requested,
                available,
            } => write!(
                f,
                "No Pico de Gallo with serial number '{requested}' is attached.\n\
                 Available: {}",
                list(available)
            ),
            Self::PinConflict { pin, requested } => write!(
                f,
                "This server is pinned to serial number '{pin}' (--serial-number); \
                 it cannot address '{requested}'. Omit serial_number, or pass '{pin}'."
            ),
            Self::PinnedNotFound { pin, available } => write!(
                f,
                "This server is pinned to serial number '{pin}' (--serial-number), \
                 which is not attached.\nAvailable: {}",
                list(available)
            ),
        }
    }
}

impl std::error::Error for SelectError {}

/// Map a [`SelectError`] to an MCP tool error.
///
/// Errors the agent can fix by changing its arguments are `invalid_params`;
/// errors about the environment are `internal_error`, because no argument
/// change helps.
pub fn map_select_err(err: SelectError) -> ErrorData {
    let msg = err.to_string();
    match err {
        SelectError::NoDevice | SelectError::PinnedNotFound { .. } => {
            ErrorData::internal_error(msg, None)
        }
        SelectError::Ambiguous { .. }
        | SelectError::NotFound { .. }
        | SelectError::PinConflict { .. } => ErrorData::invalid_params(msg, None),
    }
}
```

- [ ] **Step 9: Run to verify they pass**

Run: `cargo test --locked -p gallo-mcp select::`
Expected: 24 tests pass.

- [ ] **Step 10: Full verification**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

- [ ] **Step 11: Commit**

```bash
git add crates/pico-de-gallo-mcp/src/select.rs crates/pico-de-gallo-mcp/src/lib.rs
```

Commit message:

```text
feat(mcp): Add pure device selection policy

Decide which board a tool call targets from the attached serials, the
server's --serial-number pin, and the call's optional serial_number.

The policy is deliberately free of USB and async so the whole matrix is
unit-testable with no hardware: zero attached, one attached (with and
without an argument), two or more attached, a pinned server, and boards
that report no USB serial at all.

Omitting serial_number stays legal at N==1, which keeps the common
single-board path frictionless, and becomes an error at N>=2, where
guessing would convert a recoverable mistake into a silent wrong
answer. The error names the available serials so an agent recovers on
its next call rather than depending on its own diligence.

Errors an agent can fix by changing arguments map to invalid_params;
errors about the environment map to internal_error.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 2: Wire selection into `connect`, add the envelope, convert I2C

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs`
- Modify: `crates/pico-de-gallo-mcp/src/i2c.rs`
- Modify (mechanically): `crates/pico-de-gallo-mcp/src/{device,spi,uart,gpio,pwm,adc,onewire}.rs`

This task proves the whole vertical slice on one peripheral before Tasks 3-7
replicate it. Every other module gets a mechanical `self.connect(None)` so the
tree stays green; they are converted properly in later tasks.

- [ ] **Step 1: Write the failing envelope tests**

Append to the existing `mod tests` in `crates/pico-de-gallo-mcp/src/lib.rs`:

```rust
    #[test]
    fn envelope_puts_the_payload_under_result() {
        let payload = serde_json::json!({ "hex": "0x48" });
        let env = crate::Envelope {
            serial_number: Some("9A54ED7E3A1D9D98"),
            result: &payload,
        };
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(v["serial_number"], "9A54ED7E3A1D9D98");
        assert_eq!(v["result"]["hex"], "0x48");
    }

    #[test]
    fn envelope_reports_a_serialless_board_as_null() {
        let env = crate::Envelope {
            serial_number: None,
            result: &"ok",
        };
        let v = serde_json::to_value(&env).unwrap();
        assert!(v["serial_number"].is_null());
        assert_eq!(v["result"], "ok");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked -p gallo-mcp envelope_`
Expected: compile error — `crate::Envelope` does not exist.

- [ ] **Step 3: Add `Envelope`, `ok_device_json`, and `attached_serials`**

In `crates/pico-de-gallo-mcp/src/lib.rs`, immediately after the existing
`ok_json` function:

```rust
/// A tool response tagged with the device that served it.
///
/// Every device tool returns this shape so an agent can see, on any call,
/// which board answered — rather than only when it thinks to ask.
#[derive(Serialize)]
pub(crate) struct Envelope<'a, T: Serialize> {
    /// Serial of the board that served the call. `null` only when the sole
    /// attached board reports no USB serial number.
    pub(crate) serial_number: Option<&'a str>,
    /// The tool's own payload, unchanged.
    pub(crate) result: &'a T,
}

/// Wrap a tool payload together with the serial of the device that served it.
pub(crate) fn ok_device_json<T: Serialize>(
    dev: &Device,
    value: &T,
) -> Result<CallToolResult, ErrorData> {
    ok_json(&Envelope {
        serial_number: dev.serial(),
        result: value,
    })
}

/// Serial of every attached Pico de Gallo, in enumeration order.
///
/// A `None` entry is a board that reports no USB serial number.
pub(crate) fn attached_serials() -> Vec<Option<String>> {
    pico_de_gallo_lib::list_devices()
        .into_iter()
        .map(|d| d.serial_number)
        .collect()
}
```

- [ ] **Step 4: Teach `Device` its serial**

In `crates/pico-de-gallo-mcp/src/lib.rs`, add the field to `Device` (after
`info`) and the accessor to its `impl`:

```rust
pub(crate) struct Device {
    inner: PicoDeGallo,
    info: DeviceInfo,
    /// Serial this connection was opened with, as chosen by
    /// [`select::resolve_target`]. `None` only for a sole serial-less board.
    serial: Option<String>,
    /// Serializes device access across concurrent tool calls; held for the
    /// lifetime of the connection. Released (after `inner`) when this drops.
    _claim: OwnedMutexGuard<()>,
}
```

```rust
impl Device {
    /// The device info captured during validation on connect.
    pub(crate) fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// USB serial of the board this connection is bound to.
    ///
    /// `None` only when the sole attached board reports no serial number.
    pub(crate) fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }
}
```

- [ ] **Step 5: Resolve the target inside `connect`**

Replace `GalloMcp::connect` in `crates/pico-de-gallo-mcp/src/lib.rs`. Note the
new argument, the resolve step, opening **by serial**, and the reworded
not-found message.

```rust
    /// Open and validate a fresh connection to the target device.
    ///
    /// `requested` is the call's optional `serial_number`. The target is
    /// chosen by [`select::resolve_target`] from the attached boards, the
    /// server's `--serial-number` pin, and `requested` — so an ambiguous
    /// choice is refused rather than guessed.
    ///
    /// Serializes device access with the shared lock (rmcp dispatches each
    /// tool call on its own `tokio::spawn` task, so handlers can run
    /// concurrently), constructs the [`PicoDeGallo`] with the fallible
    /// `try_new*`, validates schema compatibility, and runs the connect-time
    /// subscription reset. The returned [`Device`] owns the connection, the
    /// resolved serial, and the lock.
    ///
    /// If the interface claim fails transiently — e.g. the previous
    /// connection's asynchronous teardown has not released the exclusive USB
    /// claim yet, the Windows double-claim hazard in AGENTS.md §13.17 —
    /// retries a few times with a short backoff before giving up.
    pub(crate) async fn connect(&self, requested: Option<&str>) -> Result<Device, ErrorData> {
        /// Total attempts to claim the interface before giving up.
        const MAX_ATTEMPTS: u32 = 5;
        /// Backoff between claim attempts (absorbs async release window).
        const BACKOFF: Duration = Duration::from_millis(100);

        let claim = self.connection.clone().lock_owned().await;

        // Resolve before opening: with no board attached this returns
        // `NoDevice` without touching USB at all.
        let serial = select::resolve_target(
            &attached_serials(),
            self.serial_number.as_deref(),
            requested,
        )
        .map_err(select::map_select_err)?;

        let mut attempt: u32 = 1;
        let inner = loop {
            let result = match serial.as_deref() {
                Some(sn) => PicoDeGallo::try_new_with_serial_number(sn),
                None => PicoDeGallo::try_new(),
            };
            match result {
                Ok(dev) => break dev,
                Err(e) if e.contains(NOT_FOUND) => {
                    // Enumeration just saw this board, so it went away
                    // mid-call. Saying "no device attached" here would be
                    // misleading.
                    return Err(ErrorData::internal_error(
                        match serial.as_deref() {
                            Some(sn) => format!(
                                "device {sn} was attached a moment ago but is gone now; \
                                 check the USB connection and retry"
                            ),
                            None => "no device attached: connect a Pico de Gallo and retry"
                                .to_string(),
                        },
                        None,
                    ));
                }
                Err(e) if attempt >= MAX_ATTEMPTS => {
                    return Err(ErrorData::internal_error(
                        format!("failed to open device after {attempt} attempts: {e}"),
                        None,
                    ));
                }
                Err(_) => {
                    attempt += 1;
                    tokio::time::sleep(BACKOFF).await;
                }
            }
        };
        let info = inner.validate().await.map_err(error::map_validate_err)?;
        let _ = inner.system_reset_subscriptions().await;
        Ok(Device {
            inner,
            info,
            serial,
            _claim: claim,
        })
    }
```

- [ ] **Step 6: Keep every other module compiling**

In `crates/pico-de-gallo-mcp/src/{device,spi,uart,gpio,pwm,adc,onewire}.rs`,
replace every `self.connect().await` with `self.connect(None).await`. Do not
change anything else in those files yet.

Verify none were missed:

Run: `rg -n "self\.connect\(\)" crates/pico-de-gallo-mcp/src/`
Expected: no matches.

- [ ] **Step 7: Write the failing I2C parameter test**

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/i2c.rs`:

```rust
    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: I2cReadParams = serde_json::from_str(r#"{"address":72,"count":2}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: I2cReadParams =
            serde_json::from_str(r#"{"address":72,"count":2,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
```

- [ ] **Step 8: Run to verify it fails**

Run: `cargo test --locked -p gallo-mcp read_params_accept_an_optional_serial_number`
Expected: compile error — no field `serial_number` on `I2cReadParams`.

- [ ] **Step 9: Add the field to all six I2C params structs**

Add this field as the **last** field of `I2cReadParams`, `I2cWriteParams`,
`I2cWriteReadParams`, `I2cScanParams`, `I2cSetConfigParams`, and
`I2cBatchParams` in `crates/pico-de-gallo-mcp/src/i2c.rs`:

```rust
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
```

- [ ] **Step 10: Thread it through every I2C handler**

In `crates/pico-de-gallo-mcp/src/i2c.rs`, change the import line
`use crate::{GalloMcp, ok_json};` to:

```rust
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
```

Then apply these edits. Each handler's `self.connect(None)` becomes
`self.connect(p.serial_number.as_deref())`, and each `ok_json(&X)` becomes
`ok_device_json(&dev, &X)`:

| Handler | connect argument | return |
|---|---|---|
| `i2c_read` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&data))` |
| `i2c_write` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `i2c_write_read` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&data))` |
| `i2c_scan` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &serde_json::json!({ "addresses": hex, "raw": addrs }))` |
| `i2c_set_config` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `i2c_batch` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&out))` |

`i2c_get_config` has no params today, so give it the shared selector:

```rust
    /// Get the current I2C frequency.
    #[tool(
        description = "Get the current I2C frequency",
        annotations(read_only_hint = true)
    )]
    async fn i2c_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let f = dev.i2c_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{f:?}"))
    }
```

- [ ] **Step 11: Run to verify it passes**

Run: `cargo test --locked -p gallo-mcp`
Expected: all tests pass, including the two envelope tests and the new I2C
parameter test.

- [ ] **Step 12: Full verification**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

- [ ] **Step 13: Commit**

```bash
git add crates/pico-de-gallo-mcp/src/
```

Commit message:

```text
feat(mcp): Select per call and echo the serial, via I2C

Wire the selection policy into connect(), which now takes the call's
optional serial_number and always opens the board by serial. Zero
attached boards is caught before touching USB; a board that vanishes
between enumeration and open now says so instead of claiming none was
ever attached.

Every device tool response gains a {serial_number, result} envelope so
an agent can see which board answered on any call, not only when it
thinks to ask. Payloads move under `result` unchanged.

I2C is converted end to end as the first vertical slice, including a
shared TargetParams for i2c_get_config, which previously took no
arguments. The remaining peripherals pass None for now and are
converted next.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 3: Convert SPI

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/spi.rs`

Independent of Tasks 4-7 — touches only this file.

- [ ] **Step 1: Write the failing parameter test**

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/spi.rs`:

```rust
    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: SpiReadParams = serde_json::from_str(r#"{"count":4}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: SpiReadParams =
            serde_json::from_str(r#"{"count":4,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked -p gallo-mcp spi::tests::read_params_accept_an_optional_serial_number`
Expected: compile error — no field `serial_number` on `SpiReadParams`.

- [ ] **Step 3: Add the field to all five SPI params structs**

Add as the **last** field of `SpiReadParams`, `SpiWriteParams`,
`SpiTransferParams`, `SpiSetConfigParams`, and `SpiBatchParams`. Do **not**
add it to `SpiBatchOpParam` — that is an operation, not a call.

```rust
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
```

- [ ] **Step 4: Thread it through every SPI handler**

Change the import line `use crate::{GalloMcp, ok_json};` to:

```rust
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
```

| Handler | connect argument | return |
|---|---|---|
| `spi_read` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&data))` |
| `spi_write` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `spi_transfer` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&data))` |
| `spi_set_config` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `spi_batch` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&out))` |

`spi_flush` and `spi_get_config` take no arguments today, so give them the
shared selector:

```rust
    /// Flush the SPI TX buffer.
    #[tool(
        description = "Flush the SPI TX buffer",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn spi_flush(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.spi_flush().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Get the current SPI configuration.
    #[tool(
        description = "Get the current SPI configuration",
        annotations(read_only_hint = true)
    )]
    async fn spi_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.spi_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
    }
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test --locked -p gallo-mcp spi::`
Expected: all SPI tests pass.

- [ ] **Step 6: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/spi.rs
```

Commit message:

```text
feat(mcp): Select the SPI target per call

Add the optional serial_number argument to every SPI tool and return
results through the device envelope. spi_flush and spi_get_config took
no arguments before and now take the shared selector.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 4: Convert UART and ADC

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/uart.rs`
- Modify: `crates/pico-de-gallo-mcp/src/adc.rs`

Independent of Tasks 3, 5, 6, 7 — touches only these two files.

- [ ] **Step 1: Write the failing parameter tests**

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/uart.rs`:

```rust
    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: UartReadParams =
            serde_json::from_str(r#"{"count":4,"timeout_ms":1000}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: UartReadParams =
            serde_json::from_str(r#"{"count":4,"timeout_ms":1000,"serial_number":"ABC123"}"#)
                .unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
```

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/adc.rs`:

```rust
    #[test]
    fn read_params_accept_an_optional_serial_number() {
        let without: AdcReadParams = serde_json::from_str(r#"{"channel":2}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: AdcReadParams =
            serde_json::from_str(r#"{"channel":2,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked -p gallo-mcp read_params_accept_an_optional_serial_number`
Expected: compile errors — no field `serial_number` on `UartReadParams` or
`AdcReadParams`.

- [ ] **Step 3: Add the field to the UART and ADC params structs**

Add as the **last** field of `UartReadParams`, `UartWriteParams`,
`UartSetConfigParams` (in `uart.rs`) and `AdcReadParams` (in `adc.rs`):

```rust
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
```

- [ ] **Step 4: Thread it through the UART handlers**

In `uart.rs`, change `use crate::{GalloMcp, ok_json};` to:

```rust
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
```

| Handler | connect argument | return |
|---|---|---|
| `uart_read` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&data))` |
| `uart_write` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `uart_set_config` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |

`uart_flush` and `uart_get_config` take no arguments today:

```rust
    /// Flush the UART TX buffer.
    #[tool(
        description = "Flush the UART TX buffer",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn uart_flush(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        dev.uart_flush().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &"ok")
    }

    /// Get the current UART configuration.
    #[tool(
        description = "Get the current UART configuration",
        annotations(read_only_hint = true)
    )]
    async fn uart_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.uart_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
    }
```

- [ ] **Step 5: Thread it through the ADC handlers**

In `adc.rs`, change `use crate::{GalloMcp, ok_json};` to:

```rust
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
```

`adc_read` uses `self.connect(p.serial_number.as_deref())` and returns
`ok_device_json(&dev, &serde_json::json!({ "raw": raw }))`.

`adc_get_config` takes no arguments today:

```rust
    /// Get ADC capabilities.
    #[tool(
        description = "Get ADC capabilities",
        annotations(read_only_hint = true)
    )]
    async fn adc_get_config(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let c = dev.adc_get_config().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &format!("{c:?}"))
    }
```

- [ ] **Step 6: Run to verify they pass**

Run: `cargo test --locked -p gallo-mcp`
Expected: all tests pass.

- [ ] **Step 7: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/uart.rs crates/pico-de-gallo-mcp/src/adc.rs
```

Commit message:

```text
feat(mcp): Select the UART and ADC target per call

Add the optional serial_number argument to every UART and ADC tool and
return results through the device envelope. uart_flush,
uart_get_config, and adc_get_config took no arguments before and now
take the shared selector.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 5: Convert GPIO and PWM

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/gpio.rs`
- Modify: `crates/pico-de-gallo-mcp/src/pwm.rs`

Independent of Tasks 3, 4, 6, 7 — touches only these two files. Every tool in
both modules already takes parameters, so no `TargetParams` is needed here.

Note `GpioWaitParams` serves three tools and `PwmChannelParams` serves four;
one field addition covers all of them.

- [ ] **Step 1: Write the failing parameter tests**

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/gpio.rs`:

```rust
    #[test]
    fn wait_params_accept_an_optional_serial_number() {
        let without: GpioWaitParams =
            serde_json::from_str(r#"{"pin":5,"timeout_ms":1000}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: GpioWaitParams =
            serde_json::from_str(r#"{"pin":5,"timeout_ms":1000,"serial_number":"ABC123"}"#)
                .unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
```

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/pwm.rs`:

```rust
    #[test]
    fn channel_params_accept_an_optional_serial_number() {
        let without: PwmChannelParams = serde_json::from_str(r#"{"channel":0}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: PwmChannelParams =
            serde_json::from_str(r#"{"channel":0,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked -p gallo-mcp accept_an_optional_serial_number`
Expected: compile errors — no field `serial_number` on `GpioWaitParams` or
`PwmChannelParams`.

- [ ] **Step 3: Add the field to the GPIO and PWM params structs**

Add as the **last** field of `GpioGetParams`, `GpioPutParams`,
`GpioSetConfigParams`, `GpioWaitParams` (in `gpio.rs`) and
`PwmChannelParams`, `PwmSetDutyParams`, `PwmSetConfigParams` (in `pwm.rs`):

```rust
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
```

- [ ] **Step 4: Thread it through the GPIO handlers**

In `gpio.rs`, change `use crate::{GalloMcp, ok_json};` to:

```rust
use crate::{GalloMcp, ok_device_json};
```

| Handler | connect argument | return |
|---|---|---|
| `gpio_get` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &serde_json::json!({ "high": matches!(state, pico_de_gallo_lib::GpioState::High) }))` |
| `gpio_put` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `gpio_set_config` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `gpio_wait_for_rising_edge_with_timeout` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"edge")` |
| `gpio_wait_for_falling_edge_with_timeout` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"edge")` |
| `gpio_wait_for_any_edge_with_timeout` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"edge")` |

- [ ] **Step 5: Thread it through the PWM handlers**

In `pwm.rs`, change `use crate::{GalloMcp, ok_json};` to:

```rust
use crate::{GalloMcp, ok_device_json};
```

| Handler | connect argument | return |
|---|---|---|
| `pwm_get_duty_cycle` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &format!("{d:?}"))` |
| `pwm_get_config` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &format!("{c:?}"))` |
| `pwm_set_duty_cycle` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `pwm_enable` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `pwm_disable` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `pwm_set_config` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |

- [ ] **Step 6: Run to verify they pass**

Run: `cargo test --locked -p gallo-mcp`
Expected: all tests pass.

- [ ] **Step 7: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/gpio.rs crates/pico-de-gallo-mcp/src/pwm.rs
```

Commit message:

```text
feat(mcp): Select the GPIO and PWM target per call

Add the optional serial_number argument to every GPIO and PWM tool and
return results through the device envelope. Both modules already took
parameters everywhere, so no shared selector is needed here; one field
on GpioWaitParams covers three tools and one on PwmChannelParams covers
four.

Driving the wrong pin on the wrong board is the sharpest edge of the
ambiguity this fixes, since gpio_set_config and gpio_put are a stateful
pair across calls.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 6: Convert 1-Wire

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/onewire.rs`

Independent of Tasks 3, 4, 5, 7 — touches only this file.

- [ ] **Step 1: Write the failing parameter test**

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/onewire.rs`:

```rust
    #[test]
    fn search_params_accept_an_optional_serial_number() {
        let without: OneWireSearchParams = serde_json::from_str("{}").unwrap();
        assert_eq!(without.serial_number, None);

        let with: OneWireSearchParams =
            serde_json::from_str(r#"{"continue_search":true,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
        assert!(with.continue_search);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked -p gallo-mcp onewire::tests::search_params_accept_an_optional_serial_number`
Expected: compile error — no field `serial_number` on `OneWireSearchParams`.

- [ ] **Step 3: Add the field to all four 1-Wire params structs**

Add as the **last** field of `OneWireReadParams`, `OneWireWriteParams`,
`OneWireWritePullupParams`, and `OneWireSearchParams`:

```rust
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
```

- [ ] **Step 4: Thread it through every 1-Wire handler**

Change the import line `use crate::{GalloMcp, ok_json};` to:

```rust
use crate::select::TargetParams;
use crate::{GalloMcp, ok_device_json};
```

| Handler | connect argument | return |
|---|---|---|
| `onewire_read` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &Bytes::from_slice(&data))` |
| `onewire_write` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |
| `onewire_write_pullup` | `p.serial_number.as_deref()` | `ok_device_json(&dev, &"ok")` |

`onewire_search` returns from two branches; both go through the envelope:

```rust
        match rom {
            Some(id) => ok_device_json(
                &dev,
                &serde_json::json!({ "rom": format!("0x{id:016X}"), "raw": id }),
            ),
            None => ok_device_json(&dev, &serde_json::json!({ "rom": null })),
        }
```

`onewire_reset` takes no arguments today:

```rust
    /// 1-Wire reset + presence detect.
    #[tool(
        description = "1-Wire reset + presence detect",
        annotations(destructive_hint = true, read_only_hint = false)
    )]
    async fn onewire_reset(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let present = dev.onewire_reset().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &serde_json::json!({ "presence": present }))
    }
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test --locked -p gallo-mcp onewire::`
Expected: all 1-Wire tests pass.

- [ ] **Step 6: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/onewire.rs
```

Commit message:

```text
feat(mcp): Select the 1-Wire target per call

Add the optional serial_number argument to every 1-Wire tool and return
results through the device envelope. onewire_reset took no arguments
before and now takes the shared selector.

ROM search is stateful across calls: onewire_search followed by
onewire_search with continue_search only means anything against the
board that started it. An echoed serial makes a continuation against
the wrong board visible instead of silent.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 7: Rework the device tools

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/device.rs`
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs` (add `GalloMcp::pinned_serial`)

Run this **after** Tasks 3-6, because it also touches `lib.rs`.

`list_devices` and `status` build their responses through **pure functions**,
so their JSON shapes — including the ambiguity reporting that this whole issue
turns on — are unit-testable with no board attached.

- [ ] **Step 1: Add the pinned-serial accessor**

In `crates/pico-de-gallo-mcp/src/lib.rs`, add to `impl GalloMcp` immediately
before `connect`:

```rust
    /// The server's `--serial-number` pin, if any.
    ///
    /// A pinned server cannot address any other board; that is the only
    /// guarantee enforced by construction rather than by agent diligence.
    ///
    /// Named `pinned_serial` rather than `pin` because in this repository
    /// `pin` means a GPIO pin.
    pub(crate) fn pinned_serial(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }
```

Then, inside `connect`, replace `self.serial_number.as_deref()` in the
`resolve_target` call with `self.pinned_serial()`.

- [ ] **Step 2: Write the failing response-shape tests**

Replace the whole `mod tests` block at the end of
`crates/pico-de-gallo-mcp/src/device.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::GalloMcp;

    const A: &str = "9A54ED7E3A1D9D98";
    const B: &str = "5256657D8A5D7F03";

    fn desc(serial: Option<&str>) -> DeviceDescription {
        DeviceDescription {
            serial_number: serial.map(str::to_string),
            manufacturer: Some("Microsoft".to_string()),
            product: Some("Pico de Gallo".to_string()),
        }
    }

    #[test]
    fn device_tools_are_registered() {
        let names: Vec<String> = GalloMcp::router_for_test()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for e in ["list_devices", "status", "device_info", "version", "ping"] {
            assert!(names.contains(&e.to_string()), "missing {e}");
        }
    }

    #[test]
    fn ping_params_accept_an_optional_serial_number() {
        let without: PingParams = serde_json::from_str(r#"{"id":7}"#).unwrap();
        assert_eq!(without.serial_number, None);

        let with: PingParams =
            serde_json::from_str(r#"{"id":7,"serial_number":"ABC123"}"#).unwrap();
        assert_eq!(with.serial_number.as_deref(), Some("ABC123"));
    }

    #[test]
    fn sole_board_is_the_default_target_and_needs_no_serial() {
        let out = build_list_result(vec![desc(Some(A))], None);
        assert!(!out.serial_number_required);
        assert!(out.note.is_none());
        assert!(out.devices[0].default_target);
        assert!(!out.devices[0].pinned);
    }

    #[test]
    fn sole_serialless_board_is_still_the_default_target() {
        let out = build_list_result(vec![desc(None)], None);
        assert!(!out.serial_number_required);
        assert!(out.devices[0].default_target);
    }

    #[test]
    fn two_boards_require_a_serial_and_have_no_default() {
        let out = build_list_result(vec![desc(Some(A)), desc(Some(B))], None);
        assert!(out.serial_number_required);
        assert!(out.note.as_deref().unwrap().contains("2 devices attached"));
        assert!(out.devices.iter().all(|d| !d.default_target));
        assert!(out.devices.iter().all(|d| !d.pinned));
    }

    #[test]
    fn a_pin_removes_the_ambiguity_and_marks_its_board() {
        let out = build_list_result(vec![desc(Some(A)), desc(Some(B))], Some(A));
        assert!(!out.serial_number_required);
        assert!(out.note.is_none());
        assert_eq!(out.pinned.as_deref(), Some(A));
        assert!(out.devices[0].pinned && out.devices[0].default_target);
        assert!(!out.devices[1].pinned && !out.devices[1].default_target);
    }

    #[test]
    fn a_pin_to_an_absent_board_is_still_reported() {
        // No entry can carry the flag, which is exactly when the top-level
        // field earns its place.
        let out = build_list_result(vec![desc(Some(B))], Some(A));
        assert_eq!(out.pinned.as_deref(), Some(A));
        assert!(out.devices.iter().all(|d| !d.pinned));
        assert!(out.devices.iter().all(|d| !d.default_target));
    }

    #[test]
    fn status_reports_ambiguity_rather_than_claiming_no_board() {
        let available = vec![Some(A.to_string()), Some(B.to_string())];
        let out = build_status(available, None, None);
        assert!(out.attached, "two boards are attached");
        assert!(out.ambiguous);
        assert!(out.serial_number.is_none());
        assert_eq!(out.available.len(), 2);
        let reason = out.reason.unwrap();
        assert!(reason.contains(A) && reason.contains(B), "{reason}");
    }

    #[test]
    fn status_reports_an_empty_bus_as_unattached() {
        let out = build_status(vec![], None, None);
        assert!(!out.attached);
        assert!(!out.ambiguous);
        assert!(out.reason.is_some());
    }

    #[test]
    fn status_explains_a_pin_conflict_instead_of_answering_for_another_board() {
        let available = vec![Some(A.to_string()), Some(B.to_string())];
        let out = build_status(available, Some(A), Some(B));
        assert!(out.serial_number.is_none());
        let reason = out.reason.unwrap();
        assert!(reason.contains("--serial-number"), "{reason}");
    }

    #[test]
    fn status_has_no_reason_when_selection_succeeds() {
        let out = build_status(vec![Some(A.to_string())], None, None);
        assert!(out.reason.is_none());
        assert!(!out.ambiguous);
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test --locked -p gallo-mcp device::`
Expected: compile errors — `DeviceDescription`, `build_list_result`,
`build_status`, and `PingParams::serial_number` do not exist.

- [ ] **Step 4: Rewrite the response types and builders**

Replace everything in `crates/pico-de-gallo-mcp/src/device.rs` from the top of
the file down to (but not including) the `#[tool_router(...)]` line with:

```rust
//! Device-level tools: enumeration, status, info, version, ping.

use pico_de_gallo_lib::DeviceDescription;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use serde::Serialize;

use crate::error::map_pdg_err;
use crate::select::{SelectError, TargetParams, resolve_target};
use crate::{GalloMcp, attached_serials, ok_device_json, ok_json};

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DeviceEntry {
    serial_number: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
    /// True when this board is the server's `--serial-number` pin.
    pinned: bool,
    /// True when a call that omits `serial_number` will use this board.
    default_target: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ListDevicesResult {
    /// Every attached Pico de Gallo.
    devices: Vec<DeviceEntry>,
    /// The server's `--serial-number` pin, if any. Reported separately from
    /// the per-entry flag because a pinned board that is not attached
    /// produces no entry at all — precisely when you want to be told.
    pinned: Option<String>,
    /// True when `serial_number` must be supplied on every device tool call.
    serial_number_required: bool,
    /// Present only when `serial_number_required`.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct StatusResult {
    /// True when at least one board is attached.
    attached: bool,
    /// Serial of the board actually reached; null when none was.
    serial_number: Option<String>,
    /// True when two or more boards are attached and the server is unpinned.
    ambiguous: bool,
    /// Serial of every attached board; a null entry reports no serial.
    available: Vec<Option<String>>,
    /// The server's `--serial-number` pin, if any.
    pinned: Option<String>,
    /// Why `serial_number` is null, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    firmware_version: Option<String>,
    schema_major: Option<u16>,
    schema_minor: Option<u16>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PingParams {
    /// Value to echo back.
    id: u32,
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    serial_number: Option<String>,
}

/// Build the `list_devices` response.
///
/// Asks [`resolve_target`] the same question `connect` asks, so the advertised
/// default can never drift from the board a bare call actually reaches.
fn build_list_result(
    descs: Vec<DeviceDescription>,
    pinned_serial: Option<&str>,
) -> ListDevicesResult {
    let attached: Vec<Option<String>> = descs.iter().map(|d| d.serial_number.clone()).collect();
    let resolved = resolve_target(&attached, pinned_serial, None);
    let serial_number_required = matches!(resolved, Err(SelectError::Ambiguous { .. }));
    // `Some(t)` is the board a bare call would use. `t` is itself optional
    // because a sole serial-less board is still a valid target.
    let default_target = resolved.ok();

    let devices: Vec<DeviceEntry> = descs
        .into_iter()
        .map(|d| DeviceEntry {
            pinned: pinned_serial.is_some() && d.serial_number.as_deref() == pinned_serial,
            default_target: matches!(&default_target, Some(t) if *t == d.serial_number),
            serial_number: d.serial_number,
            manufacturer: d.manufacturer,
            product: d.product,
        })
        .collect();

    let note = serial_number_required.then(|| {
        format!(
            "{} devices attached and this server is not pinned; pass serial_number \
             on every device tool call.",
            devices.len()
        )
    });

    ListDevicesResult {
        devices,
        pinned: pinned_serial.map(str::to_string),
        serial_number_required,
        note,
    }
}

/// Build the part of the `status` response that needs no connection.
///
/// `status` must stay answerable when nothing is resolvable — that is the
/// whole point of the tool — so this never fails. The caller fills in the
/// device fields if it manages to connect.
fn build_status(
    available: Vec<Option<String>>,
    pinned_serial: Option<&str>,
    requested: Option<&str>,
) -> StatusResult {
    let resolved = resolve_target(&available, pinned_serial, requested);
    StatusResult {
        attached: !available.is_empty(),
        serial_number: None,
        ambiguous: matches!(resolved, Err(SelectError::Ambiguous { .. })),
        pinned: pinned_serial.map(str::to_string),
        reason: resolved.err().map(|e| e.to_string()),
        available,
        firmware_version: None,
        schema_major: None,
        schema_minor: None,
    }
}
```

- [ ] **Step 5: Rewrite the five handlers**

Replace the body of the `#[tool_router(router = device_router, vis = "pub(crate)")]`
`impl GalloMcp` block in `crates/pico-de-gallo-mcp/src/device.rs` with:

```rust
    /// List all Pico de Gallo devices currently attached (no connection needed).
    #[tool(
        description = "List attached Pico de Gallo devices",
        annotations(read_only_hint = true)
    )]
    async fn list_devices(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&build_list_result(
            pico_de_gallo_lib::list_devices(),
            self.pinned_serial(),
        ))
    }

    /// Report which board is reachable, and why not when none is.
    #[tool(
        description = "Get device attachment status and version",
        annotations(read_only_hint = true)
    )]
    async fn status(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let requested = p.serial_number.as_deref();
        let mut out = build_status(attached_serials(), self.pinned_serial(), requested);
        if out.reason.is_none() {
            match self.connect(requested).await {
                Ok(dev) => {
                    let info = dev.info();
                    out.serial_number = dev.serial().map(str::to_string);
                    out.firmware_version = Some(format!(
                        "{}.{}.{}",
                        info.fw_major, info.fw_minor, info.fw_patch
                    ));
                    out.schema_major = Some(info.schema_major);
                    out.schema_minor = Some(info.schema_minor);
                }
                // Selection succeeded but the board could not be opened or
                // validated. Say so rather than leaving a bare null.
                Err(e) => out.reason = Some(e.message.to_string()),
            }
        }
        ok_json(&out)
    }

    /// Get full device info (firmware version, schema version, capabilities).
    #[tool(
        description = "Get firmware and schema device info",
        annotations(read_only_hint = true)
    )]
    async fn device_info(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let info = dev.device_info().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &info)
    }

    /// Get the firmware version string.
    #[tool(
        description = "Get firmware version",
        annotations(read_only_hint = true)
    )]
    async fn version(
        &self,
        Parameters(p): Parameters<TargetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let v = dev.version().await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &v)
    }

    /// Echo a u32 (connectivity test).
    #[tool(
        description = "Ping the device (echo a u32)",
        annotations(read_only_hint = true)
    )]
    async fn ping(
        &self,
        Parameters(p): Parameters<PingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dev = self.connect(p.serial_number.as_deref()).await?;
        let echoed = dev.ping(p.id).await.map_err(map_pdg_err)?;
        ok_device_json(&dev, &echoed)
    }
```

- [ ] **Step 6: Run to verify they pass**

Run: `cargo test --locked -p gallo-mcp device::`
Expected: 11 tests pass.

- [ ] **Step 7: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/device.rs crates/pico-de-gallo-mcp/src/lib.rs
```

Commit message:

```text
feat(mcp): Make list_devices and status honest about the target

status previously turned any connect failure into attached:false. Under
the new ambiguity rule that would report "no board" while two were
attached — a fresh silent lie in exactly the scenario this fixes. It now
never errors and never lies: it reports attached, ambiguous, available,
pinned, and a reason explaining any unresolved target, so it remains the
one call an agent can always make to orient itself.

list_devices becomes an object carrying per-entry pinned and
default_target flags plus serial_number_required and a note, stating the
rule where the agent is already reading serials. The top-level pinned
field is separate from the per-entry flag because a pinned board that is
not attached produces no entry at all.

Both responses are built by pure functions, so the flag and ambiguity
logic is unit-tested with no board attached, and both ask resolve_target
the same question connect asks — the advertised default cannot drift
from the board a bare call reaches.

device_info, version, and ping take the selector and return through the
envelope.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 8: Surface-wide guard and server instructions

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs`

Run **after** Tasks 3-7. These tests are the safety net for the 28 hand-edited
structs and 42 hand-edited handlers: they are the only checks that catch the
one that got missed, which review reliably does not.

**Why three tests and not one.** The conversion is *two* independent hand edits
per handler — declare the field on the params struct, and thread
`p.serial_number.as_deref()` into `connect`. A schema test proves only the
first. A handler that declares the field and then calls `connect(None)` passes
the schema test, the per-module registration test, and the per-module
deserialization test. The compiler catches it only for handlers whose params
struct is `TargetParams` alone, where `p` would be entirely unused and
`-D warnings` fires; for handlers with other parameters `p` is still used, so
nothing complains. That was 5 of 7 in SPI alone.

The failure mode is the one issue #89 exists to eliminate, in a worse form:
with one board the argument is silently ignored and the envelope still reports
the right serial, so the response looks correct; with two boards the agent is
told *"`serial_number` is required"* in response to a call that supplied it,
and will retry forever.

- [ ] **Step 1: Write the failing surface tests**

Append inside the existing `mod tests` in `crates/pico-de-gallo-mcp/src/lib.rs`:

```rust
    /// The exact text the `serial_number` rustdoc compiles to.
    ///
    /// Hand-copied into 27 params structs plus `TargetParams`. It becomes the
    /// JSON Schema `description` an agent reads, so a typo, a reflow, or a
    /// well-meaning rewording in one copy leaves one tool's schema saying
    /// something different from every other, with nothing failing.
    const SERIAL_DESC: &str = "USB serial number of the board to use. \
        Required when two or more\nboards are attached; optional when exactly one is.";

    /// Every device tool must accept an optional `serial_number`, described
    /// identically.
    ///
    /// The field was added by hand to 27 params structs plus one shared
    /// selector. This is what catches the struct that got missed.
    #[test]
    fn every_device_tool_accepts_an_optional_serial_number() {
        for tool in crate::GalloMcp::router_for_test().list_all() {
            // The only tool that touches no device.
            if tool.name == "list_devices" {
                continue;
            }
            let schema = tool.input_schema.as_ref();
            let props = schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .unwrap_or_else(|| panic!("{}: input schema has no properties", tool.name));
            assert!(
                props.contains_key("serial_number"),
                "{}: input schema is missing serial_number",
                tool.name
            );
            assert_eq!(
                props["serial_number"]
                    .get("description")
                    .and_then(serde_json::Value::as_str),
                Some(SERIAL_DESC),
                "{}: serial_number description has drifted from the canonical text",
                tool.name
            );
            if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
                assert!(
                    !required.iter().any(|v| v.as_str() == Some("serial_number")),
                    "{}: serial_number must stay optional",
                    tool.name
                );
            }
        }
    }

    /// Every handler must *use* the selector it declares.
    ///
    /// Declaring `serial_number` and then calling `connect(None)` is invisible
    /// to the schema test above: the property is there, the argument is
    /// dropped. Only handlers taking `TargetParams` alone are compile-guarded,
    /// because there `p` would be unused and `-D warnings` fires.
    ///
    /// Crude, but it catches the whole class. After Task 7 no legitimate
    /// `connect(None)` remains, so the invariant is trivially maintainable.
    #[test]
    fn every_handler_threads_the_selector_into_connect() {
        for (name, src) in [
            ("adc.rs", include_str!("adc.rs")),
            ("device.rs", include_str!("device.rs")),
            ("gpio.rs", include_str!("gpio.rs")),
            ("i2c.rs", include_str!("i2c.rs")),
            ("onewire.rs", include_str!("onewire.rs")),
            ("pwm.rs", include_str!("pwm.rs")),
            ("spi.rs", include_str!("spi.rs")),
            ("uart.rs", include_str!("uart.rs")),
        ] {
            assert!(
                !src.contains("self.connect(None)"),
                "{name}: a handler still hard-codes connect(None); \
                 pass p.serial_number.as_deref() instead"
            );
        }
    }

    #[test]
    fn server_instructions_state_the_disambiguation_rule() {
        use rmcp::ServerHandler;
        let info = crate::GalloMcp::new(None).get_info();
        let instructions = info.instructions.expect("server sets instructions");
        assert!(instructions.contains("serial_number"), "{instructions}");
        assert!(
            instructions.contains("two or more boards"),
            "{instructions}"
        );
    }
```

`SERIAL_DESC` must match what `schemars` actually emits, including the embedded
newline where the rustdoc wraps. If the assertion fails on first run, print the
observed value and use it verbatim rather than reformatting the doc comments —
the 28 copies are the source of truth, not this constant.

- [ ] **Step 2: Run to verify the instructions test fails**

Run: `cargo test --locked -p gallo-mcp tests::`

Expected: `every_device_tool_accepts_an_optional_serial_number` and
`every_handler_threads_the_selector_into_connect` both **pass** — Tasks 3-7
already added every field and threaded every handler. If either fails, a struct
or a handler was missed; fix it before continuing, and note which one, because
it is evidence about where the conversion is error-prone.
`server_instructions_state_the_disambiguation_rule` fails because the
instructions do not mention `serial_number`.

- [ ] **Step 3: State the rule in the server instructions**

Replace `get_info` in `crates/pico-de-gallo-mcp/src/lib.rs`:

```rust
#[tool_handler(router = self.tool_router)]
impl ServerHandler for GalloMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Bridge to a Pico de Gallo USB device (I2C/SPI/UART/GPIO/PWM/ADC/1-Wire). \
             Bytes are hex strings like \"0x48,0x00\". Read tools are safe; tools that \
             write or actuate pins are marked destructive and may require approval. \
             Every device tool takes an optional serial_number choosing the board, and \
             every response echoes the serial of the board that served the call. \
             serial_number is REQUIRED when two or more boards are attached: without it \
             the call fails and lists the serials you can use. Call list_devices first \
             to see what is attached.",
        )
    }
}
```

- [ ] **Step 4: Run to verify both pass**

Run: `cargo test --locked -p gallo-mcp tests::`
Expected: both pass.

- [ ] **Step 5: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/lib.rs
```

Commit message:

```text
test(mcp): Guard the whole tool surface, state the rule up front

Assert over every registered tool that serial_number is present in the
input schema and absent from `required`. The field was added by hand to
28 structs; a single missed struct is invisible to review but would
silently keep one tool unselectable.

Also state the N>=2 rule in the server instructions. The consumer is an
LLM, which emits minimal argument sets unless something forces
otherwise, so the rule needs to be visible before the first call rather
than only in the error that follows it.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 9: Two-board hardware tests

**Files:**
- Modify: `crates/pico-de-gallo-mcp/src/lib.rs`

These live in `lib.rs` rather than `tests/` because `connect` is `pub(crate)`.
They are `#[ignore]`d, so CI never runs them.

**Wiring requirement:** the two boards must have **distinguishable I2C buses**
— e.g. one bare, one with a sensor at a known address. Without that, the
critical test cannot tell "selection works" from "selection silently returns
the same board twice".

**Coverage note:** the spec lists 11 hardware cases. Cases 4 (`list_devices`
shape) and 5 (`status` shape) became **unit** tests in Task 7 once their
response building was extracted into pure functions, so they need no board.
Cases 1, 2, 3, 6, 7 and 11 are the automated tests below; cases 8, 9 and 10
need a physical replug and stay manual (Step 4).

- [ ] **Step 1: Add the hardware test module**

Append to `crates/pico-de-gallo-mcp/src/lib.rs`, after the existing
`#[cfg(test)] mod tests` block:

```rust
/// Two-board hardware tests.
///
/// Ignored by default and never run in CI: they need two Pico de Gallo boards
/// attached with **distinguishable I2C buses** (e.g. one bare, one with a
/// sensor), and their serials in the environment.
///
/// ```console
/// $ GALLO_MCP_TEST_SERIAL_A=9A54ED7E3A1D9D98 \
///   GALLO_MCP_TEST_SERIAL_B=5256657D8A5D7F03 \
///   cargo test -p gallo-mcp --locked -- --ignored --test-threads=1
/// ```
///
/// `--test-threads=1` is required: each test builds its own [`GalloMcp`], so
/// each has its own connection mutex and concurrent tests would race for the
/// exclusive USB claim.
#[cfg(test)]
mod hardware {
    use crate::GalloMcp;

    /// The two board serials, or a loud failure if they are not configured.
    fn serials() -> (String, String) {
        let a = std::env::var("GALLO_MCP_TEST_SERIAL_A")
            .expect("set GALLO_MCP_TEST_SERIAL_A to the first board's serial");
        let b = std::env::var("GALLO_MCP_TEST_SERIAL_B")
            .expect("set GALLO_MCP_TEST_SERIAL_B to the second board's serial");
        assert_ne!(a, b, "the two serials must differ");
        (a, b)
    }

    #[tokio::test]
    #[ignore = "requires two attached boards; see module docs"]
    async fn a_bare_call_is_refused_and_lists_both_serials() {
        let (a, b) = serials();
        let err = GalloMcp::new(None)
            .connect(None)
            .await
            .expect_err("two boards attached: a bare connect must be refused");
        assert!(err.message.contains("serial_number"), "{}", err.message);
        assert!(err.message.contains(&a), "{}", err.message);
        assert!(err.message.contains(&b), "{}", err.message);
    }

    #[tokio::test]
    #[ignore = "requires two attached boards; see module docs"]
    async fn each_serial_reaches_its_own_board() {
        let (a, b) = serials();
        let mcp = GalloMcp::new(None);

        let scan_a = {
            let dev = mcp.connect(Some(&a)).await.expect("connect to A");
            assert_eq!(dev.serial(), Some(a.as_str()));
            dev.i2c_scan(false).await.expect("scan A")
        };
        let scan_b = {
            let dev = mcp.connect(Some(&b)).await.expect("connect to B");
            assert_eq!(dev.serial(), Some(b.as_str()));
            dev.i2c_scan(false).await.expect("scan B")
        };

        // The whole point of the change: the same call with different serials
        // must reach different silicon, not merely echo different strings.
        assert_ne!(
            scan_a, scan_b,
            "both serials returned identical bus contents — either the boards \
             are wired the same or selection is not reaching them"
        );
    }

    #[tokio::test]
    #[ignore = "requires two attached boards; see module docs"]
    async fn an_unknown_serial_is_refused_with_the_alternatives() {
        let (a, b) = serials();
        let err = GalloMcp::new(None)
            .connect(Some("BOGUSSERIAL"))
            .await
            .expect_err("an unattached serial must be refused");
        assert!(err.message.contains("BOGUSSERIAL"), "{}", err.message);
        assert!(err.message.contains(&a), "{}", err.message);
        assert!(err.message.contains(&b), "{}", err.message);
    }

    #[tokio::test]
    #[ignore = "requires two attached boards; see module docs"]
    async fn a_pinned_server_serves_its_own_board_two_ways() {
        let (a, _b) = serials();
        let mcp = GalloMcp::new(Some(&a));

        let bare = mcp.connect(None).await.expect("pinned bare connect");
        assert_eq!(bare.serial(), Some(a.as_str()));
        drop(bare);

        let explicit = mcp.connect(Some(&a)).await.expect("pinned explicit connect");
        assert_eq!(explicit.serial(), Some(a.as_str()));
    }

    #[tokio::test]
    #[ignore = "requires two attached boards; see module docs"]
    async fn a_pinned_server_cannot_be_talked_onto_the_other_board() {
        let (a, b) = serials();
        let err = GalloMcp::new(Some(&a))
            .connect(Some(&b))
            .await
            .expect_err("a pinned server must refuse another board");
        assert!(err.message.contains("--serial-number"), "{}", err.message);
        assert!(err.message.contains(&a), "{}", err.message);
        assert!(err.message.contains(&b), "{}", err.message);
    }

    #[tokio::test]
    #[ignore = "requires two attached boards; see module docs"]
    async fn configuration_set_on_one_board_does_not_leak_to_the_other() {
        use pico_de_gallo_lib::I2cFrequency;
        let (a, b) = serials();
        let mcp = GalloMcp::new(None);

        {
            let dev = mcp.connect(Some(&b)).await.expect("connect to B");
            dev.i2c_set_config(I2cFrequency::Standard)
                .await
                .expect("set B to standard");
        }
        {
            let dev = mcp.connect(Some(&a)).await.expect("connect to A");
            dev.i2c_set_config(I2cFrequency::Fast)
                .await
                .expect("set A to fast");
        }

        let (freq_a, freq_b) = {
            let dev = mcp.connect(Some(&a)).await.expect("reconnect to A");
            let fa = format!("{:?}", dev.i2c_get_config().await.expect("read A config"));
            drop(dev);
            let dev = mcp.connect(Some(&b)).await.expect("reconnect to B");
            let fb = format!("{:?}", dev.i2c_get_config().await.expect("read B config"));
            (fa, fb)
        };

        assert_eq!(freq_a, format!("{:?}", I2cFrequency::Fast));
        assert_eq!(
            freq_b,
            format!("{:?}", I2cFrequency::Standard),
            "configuration written to A leaked onto B"
        );
    }
}
```

- [ ] **Step 2: Verify they compile and are skipped by default**

Run from `crates/pico-de-gallo-mcp`: `cargo test --locked`
Expected: all existing tests pass; the six hardware tests report `ignored`.

- [ ] **Step 3: Run them against the two boards**

With both boards attached and wired so their I2C buses differ, from the
repository root:

```bash
GALLO_MCP_TEST_SERIAL_A=<serial-A> GALLO_MCP_TEST_SERIAL_B=<serial-B> \
  cargo test -p gallo-mcp --locked -- --ignored --test-threads=1
```

On PowerShell:

```powershell
$env:GALLO_MCP_TEST_SERIAL_A="<serial-A>"; $env:GALLO_MCP_TEST_SERIAL_B="<serial-B>"
cargo test -p gallo-mcp --locked -- --ignored --test-threads=1
```

Expected: 6 passed. Obtain the serials with `gallo list`.

- [ ] **Step 4: Run the three replug cases by hand and record the output**

These need physical unplugging, so they stay manual. Record the results in
the PR body.

| # | Setup | Action | Expected |
|---|---|---|---|
| 8 | pinned to A, **A unplugged**, B attached | `gallo-mcp -s <A>` then any device tool | error naming A and "not attached" |
| 9 | **one board only**, unpinned | any device tool with no `serial_number` | succeeds; response echoes that board's serial |
| 10 | **no boards attached** | any device tool | "No Pico de Gallo device attached" |

Drive them through an MCP client configured to launch the freshly built
binary (`cargo build -p gallo-mcp` → `target/debug/gallo-mcp`). The `gallo_*`
tools inside an agent session are bound to the previously installed server and
cannot verify this build.

- [ ] **Step 5: Full verification and commit**

Run from `crates/pico-de-gallo-mcp`:
`cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clean.

```bash
git add crates/pico-de-gallo-mcp/src/lib.rs
```

Commit message:

```text
test(mcp): Add two-board hardware tests for device selection

Ignored by default and never run in CI. They need two boards with
distinguishable I2C buses and their serials in the environment.

The load-bearing test is each_serial_reaches_its_own_board: it scans
both boards through one server and asserts the results differ. Every
other test could pass while selection quietly returned the same board
twice; this one cannot.

The rest cover the refusal paths that have no unit-test equivalent
because they need a real claim: a bare call with two attached, an
unknown serial, a pinned server serving its own board both bare and
explicitly, a pinned server refusing the other board, and configuration
written to one board not leaking onto the other.

The three replug cases stay manual and are recorded in the PR.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

### Task 10: Documentation parity

**Files:**
- Modify: `book/src/crates/mcp.md`
- Modify: `crates/pico-de-gallo-mcp/README.md`
- Modify: `crates/pico-de-gallo-mcp/CHANGELOG.md`
- Modify: `AGENTS.md`

AGENTS.md §15.1 makes this part of the same change, not a follow-up.

- [ ] **Step 1: Update the book's run section**

In `book/src/crates/mcp.md`, replace the `-s, --serial-number` bullet (line 37)
with:

```markdown
- `-s, --serial-number <SN>` **pins** the server to one board. A pinned
  server cannot address any other board: a tool call naming a different
  serial is refused. This is the way to scope an agent session to a
  single board.
```

- [ ] **Step 2: Add a "Choosing a board" section to the book**

In `book/src/crates/mcp.md`, insert immediately before the
`## Using it with an MCP client` heading:

````markdown
## Choosing a board

Every tool that touches a device takes an optional `serial_number`, and
every response reports which board served the call:

```json
// i2c_scan {"serial_number":"5256657D8A5D7F03"}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "addresses": ["0x48"], "raw": [72] }
}
```

How the target is chosen:

| Boards attached | `serial_number` | Result |
|---|---|---|
| 0 | — | error: no device attached |
| 1 | omitted | that board |
| 1 | given | that board, if it matches |
| ≥2 | omitted | **error**, listing the available serials |
| ≥2 | given | the named board |

With one board attached nothing changes — omit `serial_number` and it
just works. With two or more, omitting it is an error rather than a
guess:

```text
Multiple Pico de Gallo devices attached; `serial_number` is required.
Available: 9A54ED7E3A1D9D98, 5256657D8A5D7F03
```

That is deliberate. Guessing turns a recoverable mistake into a
confident wrong answer with no signal that anything went wrong; the
error names the serials, so the next call succeeds.

If the server was started with `-s`, it is pinned: an omitted
`serial_number` uses the pinned board, a matching one is accepted, and a
different one is refused.

`list_devices` tells you which case you are in without connecting:

```json
{
  "devices": [
    { "serial_number": "9A54ED7E3A1D9D98", "manufacturer": "Microsoft",
      "product": "Pico de Gallo", "pinned": false, "default_target": false },
    { "serial_number": "5256657D8A5D7F03", "manufacturer": "Microsoft",
      "product": "Pico de Gallo", "pinned": false, "default_target": false }
  ],
  "pinned": null,
  "serial_number_required": true,
  "note": "2 devices attached and this server is not pinned; pass serial_number on every device tool call."
}
```

`status` never errors, so it stays answerable even when the target is
ambiguous:

```json
{
  "attached": true,
  "serial_number": null,
  "ambiguous": true,
  "available": ["9A54ED7E3A1D9D98", "5256657D8A5D7F03"],
  "pinned": null,
  "reason": "Multiple Pico de Gallo devices attached; `serial_number` is required.\nAvailable: 9A54ED7E3A1D9D98, 5256657D8A5D7F03"
}
```
````

- [ ] **Step 3: Update the book's byte-conventions example and tool count**

In `book/src/crates/mcp.md`, replace the example under `## Byte conventions`
(lines 82-85) with the enveloped form:

````markdown
```json
// i2c_write_read {"address":72,"data":"0x00","count":2}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "hex": "0x0B,0xCF", "bytes": [11, 207] }
}
```
````

Then replace the first sentence under `## Tool catalog`:

```markdown
43 tools, grouped by peripheral. Read-only tools carry the
`readOnlyHint` annotation; write/actuation tools carry `destructiveHint`.
Every tool except `list_devices` accepts an optional `serial_number`.
```

The catalog tables already list 43 tools; "35" was stale.

- [ ] **Step 4: Update the book's validation section**

In `book/src/crates/mcp.md`, replace the two JSON blocks under
`## Validation` (the `status` output at lines 209-211 and the `i2c_scan`
output at lines 215-218) with the new shapes:

````markdown
```json
{
  "attached": true,
  "serial_number": "5256657D8A5D7F03",
  "ambiguous": false,
  "available": ["5256657D8A5D7F03"],
  "pinned": null,
  "firmware_version": "0.10.0",
  "schema_major": 0,
  "schema_minor": 6
}
```

`i2c_scan` finds the sensor at address `0x48`:

```json
// i2c_scan {"include_reserved":false}
{
  "serial_number": "5256657D8A5D7F03",
  "result": { "addresses": ["0x48"], "raw": [72] }
}
```
````

Also update the `i2c_write_read` block at lines 222-225 to the enveloped form
shown in Step 3.

Confirm these against the real output captured during Task 9's hardware run
and correct any difference — the section documents recorded behaviour, not
expected behaviour.

- [ ] **Step 5: Update the crate README**

In `crates/pico-de-gallo-mcp/README.md`, insert after the paragraph ending
"MCP protocol." (line 30) and before `## Use with an MCP client`:

````markdown
## Choosing a board

Every tool that touches a device takes an optional `serial_number`, and
every response echoes the serial of the board that served the call. With one
board attached you can omit it. With two or more, omitting it is an error
that lists the available serials rather than a silent guess:

```text
Multiple Pico de Gallo devices attached; `serial_number` is required.
Available: 9A54ED7E3A1D9D98, 5256657D8A5D7F03
```

`--serial-number` pins the server to one board, which then refuses any tool
call naming a different one — use it to scope an agent session to a single
board. Call `list_devices` to see what is attached and whether a serial is
required.
````

- [ ] **Step 6: Update the CHANGELOG**

In `crates/pico-de-gallo-mcp/CHANGELOG.md`, insert immediately after the
"adheres to Semantic Versioning" line and before `## [0.1.0]`:

```markdown
## [Unreleased]

### Added

- Optional `serial_number` argument on every device-touching tool, so one
  server instance can address any attached board per call.
- Every device tool response is now wrapped as
  `{ "serial_number": ..., "result": ... }`, reporting which board served the
  call.
- `list_devices` now returns an object with per-entry `pinned` and
  `default_target` flags plus `serial_number_required` and an explanatory
  `note`.

### Changed

- **Breaking (tool surface).** Omitting `serial_number` with two or more
  boards attached is now an error listing the available serials, instead of
  silently binding to whichever board enumerated first. The single-board case
  is unchanged: `serial_number` stays optional.
- `--serial-number` now pins the server: a tool call naming a different board
  is refused.
- `status` no longer reports `attached: false` when selection fails. It never
  errors and reports `attached`, `serial_number`, `ambiguous`, `available`,
  `pinned`, and a `reason` for an unresolved target.
```

- [ ] **Step 7: Record the incident in AGENTS.md §13.17**

Append this row to the end of the regression table in `AGENTS.md` §13.17:

```markdown
| 2026-07-29 | Two boards attached; `gallo-mcp` running unpinned, so `connect()` fell back to `PicoDeGallo::try_new()` ("first match"). | An agent asked which board carried a temperature sensor saw `list_devices` report **both** serials, `i2c_scan` report an **empty** bus, and `device_info`/`status` give no indication of which board answered. The only conclusion the evidence supported — "neither board has a sensor" — was wrong: the server was bound to the empty board and the sensor board was unreachable. Two independently configured server instances returned byte-identical `device_info`, so nothing revealed they had both grabbed the same board. Caught only because an unrelated `gallo` CLI result contradicted the MCP scan. Worse than one bad read: `i2c_set_config`→`i2c_write`, `gpio_set_config`→`gpio_put`, and `onewire_search`→`onewire_search(continue)` are stateful across calls, so an ambiguous target can drive the wrong pins on the wrong board. | Added a pure `select::resolve_target` deciding the target from the attached serials, the `--serial-number` pin, and a new per-call `serial_number` argument on every device tool. Fallback is now conditional: kept at N==1 (frictionless single-board path), an **error** at N>=2 that names the available serials so the agent self-corrects. Every device response is wrapped as `{serial_number, result}`, making the binding observable on every call. `--serial-number` became a hard pin that refuses any other board. `status` never errors and reports ambiguity explicitly instead of `attached:false`. Host-only, `gallo-mcp` only; no wire-protocol or firmware change. Issue #89. |
```

- [ ] **Step 8: Verify the book builds**

Run from the repository root: `mdbook build book`
Expected: builds with no broken-link or missing-file errors.

If `mdbook` is not installed: `cargo install mdbook --locked`.

- [ ] **Step 9: Normalize line endings and commit**

```bash
dos2unix book/src/crates/mcp.md crates/pico-de-gallo-mcp/README.md \
         crates/pico-de-gallo-mcp/CHANGELOG.md AGENTS.md
git add book/src/crates/mcp.md crates/pico-de-gallo-mcp/README.md \
        crates/pico-de-gallo-mcp/CHANGELOG.md AGENTS.md
```

Commit message:

```text
docs(mcp): Document per-call board selection

Add a "Choosing a board" section to the book chapter and the crate
README covering the optional serial_number argument, the response
envelope, the N>=2 rule and why it errors rather than guesses, and the
pin semantics of --serial-number. Update the byte-convention and
validation examples to the enveloped shape and the new status output.

Correct the book's tool count from 35 to 43; the catalog tables already
listed 43, so the figure was stale.

Record the silent-misattribution incident in AGENTS.md §13.17 so the
next agent does not reintroduce an unobservable device binding.

Refs: #89

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

---

## Final Verification

- [ ] **Whole-workspace check from the repository root**

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo check --workspace --locked
```

Expected: all clean. `cargo check --workspace --locked` guards against
`Cargo.lock` drift (AGENTS.md §13.3); no dependency changed in this work, so
neither `Cargo.toml` nor `Cargo.lock` should appear in `git status`.

- [ ] **Confirm the scope stayed host-only**

```bash
git diff --stat main...HEAD
```

Expected: only `crates/pico-de-gallo-mcp/`, `book/src/crates/mcp.md`,
`AGENTS.md`, and `docs/superpowers/`. Any change under
`crates/pico-de-gallo-internal/` or `crates/pico-de-gallo-firmware/` means the
scope was exceeded — this change has no wire-protocol impact and needs no
schema-version bump (AGENTS.md §6).

- [ ] **Confirm no version was bumped**

```bash
git diff main...HEAD -- '*/Cargo.toml'
```

Expected: empty. `gallo-mcp` stays at `0.1.0`; feature PRs never bump versions
(AGENTS.md §4 rule 12).

- [ ] **Open a draft PR**

Include in the body: the two-board test results from Task 9 Step 3, the three
manual replug results from Task 9 Step 4, and `Closes #89`. Do not request
review until CI is green — especially `lockfile`, `deny`, and `actionlint`.
