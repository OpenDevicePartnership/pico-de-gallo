# Dispatcher Progress Supervision Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the RP2350 watchdog prove *dispatcher progress* rather than *executor liveness*, so a handler that never returns resets the board within a bounded time instead of wedging it until USB re-enumeration.

**Architecture:** Newtype wrappers around postcard-rpc's `WireRx` and `WireTx` publish an exact idle/in-flight edge into a shared `progress` module. The existing `watchdog_feeder_task` becomes a supervisor that polls that state, feeds the watchdog while progress is plausible, and on expiry logs, stashes a breadcrumb in watchdog scratch registers, and calls `trigger_reset()`. A `bounded()` primitive clamps every caller-supplied handler timeout at 30 minutes and declares the matching supervisor budget in the same call.

**Tech Stack:** Rust `no_std`, Embassy (`embassy-rp` 0.10, `embassy-sync` 0.7.2, `embassy-time` 0.5), postcard-rpc 0.12.1, target `thumbv8m.main-none-eabihf`, `defmt` over RTT.

**Spec:** `docs/superpowers/specs/2026-08-31-dispatcher-progress-watchdog-design.md`. Read it before starting.

---

## Read this before Task 1

**There is no unit-test harness for this crate, and you are not building one.**

`crates/pico-de-gallo-firmware` is `#![no_std] #![no_main]`, its `main()` carries `#[embassy_executor::main]`, and `embassy-rp` is built with `critical-section-impl`. `cargo test` cannot link it for the host target. AGENTS.md §5.5 confirms the crate contributes zero tests to the workspace baseline.

Consequences for every task below:

- The normal TDD red step is **not available**. The substitute gate is `cargo clippy -D warnings` plus `cargo build --release --locked`, run for **both** hardware revisions, exactly as `.github/workflows/nostd.yml` does.
- `decide()` in Task 1 is still written as a pure function with no hardware or `embassy-time` dependency, because the spec requires it (§6.4) and because a follow-up issue will add the harness. Do **not** add `#[cfg(test)] mod tests` — it will not compile.
- Real behavioural proof comes from Task 8, which is board-attached and includes a mutation control.

**Do not bump any `[package].version`.** Spec §3.5. The maintainer bumps at release time. Task 9 records the obligation this creates.

**Every file you create or edit must have LF line endings.** Run `dos2unix <file>` after writing, per AGENTS.md §3.

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `crates/pico-de-gallo-firmware/src/progress.rs` | **create** | Budget constants, the shared slot state, `WatchedRx`, `WatchedTx`, the pure `decide()` policy, and the `bounded()` handler primitive. Everything the supervisor and the handlers share lives here and nowhere else. |
| `crates/pico-de-gallo-firmware/src/main.rs` | modify | `mod progress;`, retype `AppTx`/`AppRx`, wrap the two wire impls at construction, replace `watchdog_feeder_task` with the supervisor, read the boot breadcrumb. |
| `crates/pico-de-gallo-firmware/src/handlers/gpio.rs` | modify | Five `gpio/wait-*` handlers route through `bounded()`. |
| `crates/pico-de-gallo-firmware/src/handlers/uart.rs` | modify | `uart/read` routes through `bounded()`; `uart/write` and `uart/flush` declare a fixed budget. |
| `crates/pico-de-gallo-firmware/src/handlers/spi.rs` | modify | `spi/batch` sums its `DelayNs` ops during the existing validation pass and declares. |
| `crates/pico-de-gallo-firmware/src/handlers/onewire.rs` | modify | `onewire/write-pullup` declares `pullup_duration_ms`. |
| `crates/pico-de-gallo-firmware/src/handlers/i2c.rs` | modify | `wedge-test` cfg-gate on the zero-length write guard. |
| `crates/pico-de-gallo-firmware/Cargo.toml` | modify | Add the `wedge-test` feature. |
| `.github/workflows/nostd.yml` | modify | Add a clippy-only `wedge-test` matrix entry. |
| book, AGENTS.md, ROADMAP.md, CHANGELOGs, host rustdoc | modify | Task 9. |

`progress.rs` is deliberately one file. The wrappers, the state and the policy are a single responsibility — "observe and bound dispatch progress" — and splitting them would force the state to become `pub` across module boundaries for no benefit.

---

## Task 1: The `progress` module — state and pure policy

**Files:**
- Create: `crates/pico-de-gallo-firmware/src/progress.rs`
- Modify: `crates/pico-de-gallo-firmware/src/main.rs:48-49` (module declarations)

- [ ] **Step 1: Create `progress.rs` with constants, state and the pure policy**

```rust
//! Dispatch progress supervision.
//!
//! The watchdog feeder used to be an independent task, so it proved
//! **executor liveness** rather than **dispatcher progress**: postcard-rpc
//! dispatches handlers serially on one `&mut Context`, so a handler that never
//! returns blocks every endpoint while the feeder keeps being scheduled and
//! keeps feeding. Three device-wide wedges survived it (AGENTS.md §13.17).
//!
//! This module publishes an exact idle/in-flight edge from wrappers around
//! postcard-rpc's [`WireRx`] and [`WireTx`], which the supervisor task in
//! `main.rs` polls. See
//! `docs/superpowers/specs/2026-08-31-dispatcher-progress-watchdog-design.md`.

use core::cell::Cell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Instant};

/// Ceiling for any caller-supplied handler timeout.
///
/// `timeout_ms == 0` no longer means "wait forever": both `0` and oversized
/// values clamp to this. The handler's own `with_timeout` still fires and
/// still returns its normal `Timeout` error, so callers get a clean error
/// rather than a reboot.
pub(crate) const MAX_HANDLER_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Budget for a dispatch that declares nothing.
///
/// Sized to cover `i2c/scan`'s worst case (128 addresses × 50 ms = 6.4 s,
/// `handlers/i2c.rs`) with margin. Every other undeclared handler is µs–ms.
pub(crate) const DEFAULT_DISPATCH_BUDGET: Duration = Duration::from_secs(10);

/// Added to **declared** budgets only, never to [`DEFAULT_DISPATCH_BUDGET`].
///
/// Absorbs the handler's own `with_timeout` firing plus reply serialisation,
/// so a handler that legitimately runs to its declared ceiling completes
/// normally instead of tripping the supervisor.
pub(crate) const DISPATCH_SLACK: Duration = Duration::from_secs(30);

/// Budget for an in-flight `WireTx` operation.
///
/// Covers TX-mutex starvation only. postcard-rpc already bounds the endpoint
/// writes themselves at `frames * timeout_ms_per_frame`, clamped to
/// `1..=60000` ms.
pub(crate) const TX_BUDGET: Duration = Duration::from_secs(60);

/// Supervisor poll period. Worst-case reset latency is budget + this.
pub(crate) const SUPERVISOR_POLL: Duration = Duration::from_millis(250);

/// Wake-gap above which the supervisor assumes a time discontinuity.
///
/// `pause_on_debug(true)` stops the watchdog counter while a debugger holds
/// the core halted, but the embassy time driver keeps counting. Without this,
/// resuming from a breakpoint would look identical to a wedge and reset the
/// board, making debugging worse rather than better.
pub(crate) const DISCONTINUITY: Duration = Duration::from_millis(500);

/// Which slot tripped.
///
/// `#[repr(u32)]` because the supervisor casts this into a watchdog scratch
/// register with `as u32`.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
#[repr(u32)]
pub(crate) enum SlotKind {
    /// The dispatcher: a frame was received and its handler never returned.
    Dispatch,
    /// The wire transmitter, including the `gpio_monitor_task` topic paths.
    Tx,
}

/// One supervised slot.
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    /// `None` means idle — blocking here is expected, not a fault.
    pub(crate) deadline: Option<Instant>,
    /// First four header bytes of the frame in flight, as a breadcrumb.
    pub(crate) key: u32,
}

impl Slot {
    const IDLE: Self = Self {
        deadline: None,
        key: 0,
    };
}

/// Full supervised state. `Copy` so it can live in a `Cell`.
#[derive(Clone, Copy)]
pub(crate) struct State {
    pub(crate) dispatch: Slot,
    pub(crate) tx: Slot,
    /// Concurrent `WireTx` senders. The TX slot arms on 0→1 and disarms on
    /// 1→0, because `AppTx` is cloned into four `gpio_monitor_task`s.
    pub(crate) tx_inflight: u16,
}

impl State {
    const INIT: Self = Self {
        dispatch: Slot::IDLE,
        tx: Slot::IDLE,
        tx_inflight: 0,
    };
}

/// What the supervisor should do this tick.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub(crate) enum Action {
    /// Progress is plausible. Feed the watchdog.
    Feed,
    /// The supervisor itself was not scheduled for an implausibly long time.
    /// Re-arm the live slots and feed; do not treat this as a wedge.
    Discontinuity,
    /// A slot blew its deadline. Log, stash a breadcrumb, reset.
    Expired(SlotKind, u32),
}

/// The supervision policy, as a pure function.
///
/// No hardware access and no `Instant::now()`, so it is unit-testable once the
/// crate grows a test harness (spec §6.4). Keep it that way.
pub(crate) fn decide(now: Instant, last_wake: Option<Instant>, state: &State) -> Action {
    if let Some(last) = last_wake
        && now.saturating_duration_since(last) > DISCONTINUITY
    {
        return Action::Discontinuity;
    }
    if let Some(deadline) = state.dispatch.deadline
        && now > deadline
    {
        return Action::Expired(SlotKind::Dispatch, state.dispatch.key);
    }
    if let Some(deadline) = state.tx.deadline
        && now > deadline
    {
        return Action::Expired(SlotKind::Tx, state.tx.key);
    }
    Action::Feed
}

static STATE: Mutex<CriticalSectionRawMutex, Cell<State>> = Mutex::new(Cell::new(State::INIT));

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.lock(|cell| {
        let mut state = cell.get();
        let out = f(&mut state);
        cell.set(state);
        out
    })
}

/// Read the current state. Used by the supervisor.
pub(crate) fn snapshot() -> State {
    STATE.lock(Cell::get)
}

/// Mark the dispatcher idle. Blocking after this point is expected.
pub(crate) fn dispatch_disarm() {
    with_state(|state| state.dispatch = Slot::IDLE);
}

/// Arm the dispatch slot for a newly received frame.
pub(crate) fn dispatch_arm(budget: Duration, key: u32) {
    let deadline = Instant::now() + budget;
    with_state(|state| {
        state.dispatch = Slot {
            deadline: Some(deadline),
            key,
        }
    });
}

/// Push the dispatch deadline out, returning the previous value so a
/// [`BudgetGuard`] can restore it.
fn dispatch_extend(budget: Duration) -> Option<Instant> {
    let deadline = Instant::now() + budget;
    with_state(|state| {
        let previous = state.dispatch.deadline;
        // Never shorten an existing deadline.
        if previous.is_none_or(|p| deadline > p) {
            state.dispatch.deadline = Some(deadline);
        }
        previous
    })
}

fn dispatch_restore(previous: Option<Instant>) {
    with_state(|state| state.dispatch.deadline = previous);
}

/// Re-arm every live slot relative to now, after a detected discontinuity.
pub(crate) fn rearm_live_slots() {
    let now = Instant::now();
    with_state(|state| {
        if state.dispatch.deadline.is_some() {
            state.dispatch.deadline = Some(now + DEFAULT_DISPATCH_BUDGET);
        }
        if state.tx.deadline.is_some() {
            state.tx.deadline = Some(now + TX_BUDGET);
        }
    });
}

/// A `WireTx` operation started. Arms the TX slot on the 0→1 edge.
pub(crate) fn tx_enter() {
    let deadline = Instant::now() + TX_BUDGET;
    with_state(|state| {
        state.tx_inflight = state.tx_inflight.saturating_add(1);
        if state.tx_inflight == 1 {
            state.tx = Slot {
                deadline: Some(deadline),
                key: 0,
            };
        }
    });
}

/// A `WireTx` operation finished. Disarms the TX slot on the 1→0 edge.
pub(crate) fn tx_exit() {
    with_state(|state| {
        state.tx_inflight = state.tx_inflight.saturating_sub(1);
        if state.tx_inflight == 0 {
            state.tx = Slot::IDLE;
        }
    });
}

/// Restores the previous dispatch deadline when dropped, including when the
/// handler future is cancelled.
///
/// Returned by value rather than as `impl Drop` so callers can name the type
/// and so the lifetime is obvious at the binding site.
pub(crate) struct BudgetGuard {
    previous: Option<Instant>,
}

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        dispatch_restore(self.previous);
    }
}

/// The caller-supplied timeout expired.
pub(crate) struct Expired;

/// Clamp a caller-supplied timeout, declare the matching supervisor budget,
/// and run `fut` under it.
///
/// Clamping and declaring happen in one call so the two cannot drift apart.
/// `requested_ms == 0` clamps to [`MAX_HANDLER_TIMEOUT`] — it no longer means
/// "wait forever".
pub(crate) async fn bounded<F: core::future::Future>(requested_ms: u32, fut: F) -> Result<F::Output, Expired> {
    let requested = if requested_ms == 0 {
        MAX_HANDLER_TIMEOUT
    } else {
        Duration::from_millis(u64::from(requested_ms)).min(MAX_HANDLER_TIMEOUT)
    };
    let _guard = BudgetGuard {
        previous: dispatch_extend(requested + DISPATCH_SLACK),
    };
    embassy_time::with_timeout(requested, fut).await.map_err(|_| Expired)
}

/// Declare a supervisor budget for a handler whose duration is bounded but
/// not expressed as a caller timeout, such as `spi/batch`'s accumulated
/// `DelayNs` ops or `uart/flush`'s buffer drain.
///
/// The returned guard restores the previous deadline when dropped, so bind it
/// with `let _budget = ...` — a bare `let _ = ...` drops it immediately and
/// silently does nothing.
pub(crate) fn declare(budget: Duration) -> BudgetGuard {
    BudgetGuard {
        previous: dispatch_extend(budget.min(MAX_HANDLER_TIMEOUT) + DISPATCH_SLACK),
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/pico-de-gallo-firmware/src/main.rs`, change lines 48-49 from:

```rust
mod context;
mod handlers;
```

to:

```rust
mod context;
mod handlers;
mod progress;
```

- [ ] **Step 3: Normalise line endings**

```bash
dos2unix crates/pico-de-gallo-firmware/src/progress.rs
```

- [ ] **Step 4: Verify it compiles and lints clean, both revisions**

```bash
cd crates/pico-de-gallo-firmware
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
```

Expected: both clean.

Expected *warnings you must not silence with `#[allow]`*: `bounded`, `declare`, `snapshot`, `dispatch_arm`, `dispatch_disarm`, `tx_enter`, `tx_exit`, `rearm_live_slots` and `decide` are unused until Tasks 2–6 wire them up. If clippy fails on dead code at this step, add `#![allow(dead_code)]` **at the top of `progress.rs` only**, and delete it in Task 6 Step 5 once every item has a caller.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/progress.rs crates/pico-de-gallo-firmware/src/main.rs
git commit -m "feat(firmware): Add dispatch progress state and policy

The watchdog feeder is an independent embassy task, so it proves executor
liveness rather than dispatcher progress. Three device-wide wedges have
survived it (AGENTS.md 13.17).

Adds the shared slot state, the budget constants derived from the measured
long-dispatch tail, a pure decide() policy function, and the bounded()
primitive that clamps a caller timeout and declares the matching supervisor
budget in one call. Nothing observes this state yet.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 2: Wrap `WireRx`

The dispatch slot's arm/disarm edge. `Server::run()` (postcard-rpc `src/server/mod.rs:455-491`) blocks in `rx.receive()` while idle and in `d.handle()` while dispatching, so wrapping `WireRx` distinguishes the two exactly.

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/progress.rs` (append)
- Modify: `crates/pico-de-gallo-firmware/src/main.rs:162,164,523`

- [ ] **Step 1: Append `WatchedRx` to `progress.rs`**

```rust
use postcard_rpc::server::WireRx;

/// Wraps a [`WireRx`] so the dispatch slot arms exactly when a frame is
/// received and disarms exactly when the server goes back to waiting.
///
/// Blocking inside `receive()` is legitimate idle, not a wedge, which is why
/// the slot is disarmed *before* delegating.
pub(crate) struct WatchedRx<R> {
    inner: R,
}

impl<R> WatchedRx<R> {
    pub(crate) const fn new(inner: R) -> Self {
        Self { inner }
    }
}

/// First four header bytes of a frame, as a breadcrumb.
///
/// Deliberately not a decoded `VarKey`: the key width varies with
/// `VarKeyKind`, and this value only has to survive a reset and identify the
/// culprit in a log, not round-trip.
fn frame_breadcrumb(frame: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    let n = frame.len().min(4);
    bytes[..n].copy_from_slice(&frame[..n]);
    u32::from_le_bytes(bytes)
}

impl<R: WireRx> WireRx for WatchedRx<R> {
    type Error = R::Error;

    async fn wait_connection(&mut self) {
        dispatch_disarm();
        self.inner.wait_connection().await;
    }

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        dispatch_disarm();
        let result = self.inner.receive(buf).await;
        if let Ok(frame) = &result {
            dispatch_arm(DEFAULT_DISPATCH_BUDGET, frame_breadcrumb(frame));
        }
        result
    }
}
```

- [ ] **Step 2: Retype `AppRx` in `main.rs`**

Change line 162 from:

```rust
type AppRx = WireRxImpl<AppDriver>;
```

to:

```rust
type AppRx = progress::WatchedRx<WireRxImpl<AppDriver>>;
```

Line 164 (`type AppServer = Server<AppTx, AppRx, WireRxBuf, PicoDeGallo>;`) needs no edit — it already refers to the alias.

- [ ] **Step 3: Wrap the receiver at construction**

Change line 523 from:

```rust
    let mut server: AppServer = Server::new(tx_impl, rx_impl, pbufs.rx_buf.as_mut_slice(), dispatcher, vkk);
```

to:

```rust
    let mut server: AppServer = Server::new(
        tx_impl,
        progress::WatchedRx::new(rx_impl),
        pbufs.rx_buf.as_mut_slice(),
        dispatcher,
        vkk,
    );
```

- [ ] **Step 4: Verify**

```bash
cd crates/pico-de-gallo-firmware
dos2unix src/progress.rs
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
```

Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/progress.rs crates/pico-de-gallo-firmware/src/main.rs
git commit -m "feat(firmware): Observe dispatch progress via a WireRx wrapper

WireRx is public, unsealed and two methods wide, so a newtype gives an exact
idle/in-flight edge without forking postcard-rpc or instrumenting 47 handlers.
Server::run() blocks in receive() while idle and in handle() while
dispatching, so disarming before delegating and arming on a returned frame
distinguishes the two precisely.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 3: Wrap `WireTx`

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/progress.rs` (append)
- Modify: `crates/pico-de-gallo-firmware/src/main.rs:160,483-484`

Context you need: `AppTx` is cloned into four `gpio_monitor_task`s at `main.rs:520` and referenced by `define_dispatch!`'s `tx_impl: AppTx;` at `main.rs:312`. Retyping the alias therefore propagates everywhere, and wrapping `tx_impl` once immediately after construction keeps the diff to two lines.

`WireTx`'s five methods all take `&self` (postcard-rpc `src/server/mod.rs:45-77`), so the wrapper needs no interior mutability of its own — the slot state is global.

- [ ] **Step 1: Append `WatchedTx` to `progress.rs`**

```rust
use core::fmt::Arguments;

use postcard_rpc::header::{VarHeader, VarKeyKind};
use postcard_rpc::server::WireTx;
use serde::Serialize;

/// Wraps a [`WireTx`] so the TX slot is armed while any send is outstanding.
///
/// Coverage is narrower than it looks. postcard-rpc already bounds the
/// endpoint writes themselves (`send_all` in
/// `server/impls/embassy_usb_v0_5.rs` times out at
/// `frames * timeout_ms_per_frame`). What it does not bound is
/// `self.inner.lock().await` on `send()`'s first line — its own docs say the
/// timer "is not started until the sender has exclusive access".
///
/// So this wrapper's *unique* coverage is the four `gpio_monitor_task` topic
/// paths, which run outside `Server::run()` entirely and which the dispatch
/// slot cannot see at all, plus TX-mutex starvation. Handler-initiated sends
/// are already inside the dispatch slot.
#[derive(Clone, Copy)]
pub(crate) struct WatchedTx<T> {
    inner: T,
}

impl<T> WatchedTx<T> {
    pub(crate) const fn new(inner: T) -> Self {
        Self { inner }
    }
}

/// Arms the TX slot on construction and disarms on drop, so a cancelled send
/// cannot leave the slot armed forever.
struct TxGuard;

impl TxGuard {
    fn new() -> Self {
        tx_enter();
        Self
    }
}

impl Drop for TxGuard {
    fn drop(&mut self) {
        tx_exit();
    }
}

impl<T: WireTx> WireTx for WatchedTx<T> {
    type Error = T::Error;

    async fn wait_connection(&self) {
        // A host that is not attached is legitimate idle, not a fault.
        self.inner.wait_connection().await;
    }

    async fn send<M: Serialize + ?Sized>(&self, hdr: VarHeader, msg: &M) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send(hdr, msg).await
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send_raw(buf).await
    }

    async fn send_log_str(&self, kkind: VarKeyKind, s: &str) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send_log_str(kkind, s).await
    }

    async fn send_log_fmt<'a>(&self, kkind: VarKeyKind, a: Arguments<'a>) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send_log_fmt(kkind, a).await
    }
}
```

- [ ] **Step 2: Add the `serde` dependency if it is not already present**

```bash
cd crates/pico-de-gallo-firmware
grep -n '^serde' Cargo.toml
```

If there is no `serde` line, add to `[dependencies]`:

```toml
serde = { version = "1.0", default-features = false }
```

then update the lockfile in the **firmware** workspace and commit both together, per AGENTS.md §7.1. Do not delete the lockfile — that re-resolves the whole graph:

```bash
cd crates/pico-de-gallo-firmware
cargo check --target thumbv8m.main-none-eabihf
cargo check --locked --target thumbv8m.main-none-eabihf
```

- [ ] **Step 3: Retype `AppTx` in `main.rs`**

Change line 160 from:

```rust
type AppTx = WireTxImpl<ThreadModeRawMutex, AppDriver>;
```

to:

```rust
type AppTx = progress::WatchedTx<WireTxImpl<ThreadModeRawMutex, AppDriver>>;
```

- [ ] **Step 4: Wrap the transmitter at construction**

Change lines 483-484 from:

```rust
    let (mut builder, tx_impl, rx_impl) =
        STORAGE.init_without_build(driver, config, pbufs.tx_buf.as_mut_slice(), USB_FS_MAX_PACKET_SIZE);
```

to:

```rust
    let (mut builder, tx_impl, rx_impl) =
        STORAGE.init_without_build(driver, config, pbufs.tx_buf.as_mut_slice(), USB_FS_MAX_PACKET_SIZE);
    // Wrap once here; `AppTx` is cloned into the GPIO monitor tasks and named
    // by `define_dispatch!`, so every downstream use picks the wrapper up.
    let tx_impl = progress::WatchedTx::new(tx_impl);
```

Nothing else changes — `spawner.must_spawn(gpio_monitor_task(slot, tx_impl.clone(), vkk))` at line 520 and `Server::new(tx_impl, ...)` now pass the wrapper.

- [ ] **Step 5: Verify**

```bash
cd crates/pico-de-gallo-firmware
dos2unix src/progress.rs
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
```

Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/progress.rs crates/pico-de-gallo-firmware/src/main.rs \
        crates/pico-de-gallo-firmware/Cargo.toml crates/pico-de-gallo-firmware/Cargo.lock
git commit -m "feat(firmware): Observe wire transmit progress via a WireTx wrapper

Concurrent senders force an in-flight counter rather than a flag, because
AppTx is cloned into four gpio_monitor_task instances. The TX slot is separate
from the dispatch slot so bounded()'s 30 minute extension cannot silently
widen it.

Unique coverage is narrow and documented on the type: postcard-rpc already
bounds the endpoint writes, so this catches TX-mutex starvation and the
gpio_monitor_task topic paths, which run outside Server::run() and which the
dispatch slot cannot see at all.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 4: Turn the feeder into a supervisor

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/main.rs:385-386` (boot breadcrumb read), `:539-560` (the task)

- [ ] **Step 1: Confirm the RP2350 bootrom's watchdog scratch usage**

Spec §4.8 requires this and it is **not** an assumption you may skip. On RP2040 the bootrom owns `SCRATCH4`–`SCRATCH7` for the watchdog reboot vector. Check the RP2350 datasheet's watchdog and bootrom chapters for which `WATCHDOG_SCRATCH` registers the bootrom reserves.

Record the finding as a comment above the constants in Step 2. If the datasheet shows indices 0 and 1 are reserved on RP2350, pick two indices that are not, and adjust Step 2 accordingly.

- [ ] **Step 2: Replace `watchdog_feeder_task`**

Replace `crates/pico-de-gallo-firmware/src/main.rs:539-560` entirely with:

```rust
/// Marks watchdog scratch slot 0 as ours.
///
/// `trigger_reset()` sets `CTRL.TRIGGER`, which `reset_reason()` reports as
/// `ResetReason::Forced` — but so does a `picotool reboot`, which goes through
/// the same path. This magic is what separates the two.
const WEDGE_MAGIC: u32 = 0x5044_4700; // "PDG\0"

/// Scratch register holding [`WEDGE_MAGIC`] plus the slot discriminant.
///
/// Index chosen per the RP2350 datasheet check in this task's Step 1.
const SCRATCH_REASON: usize = 0;
/// Scratch register holding the in-flight frame breadcrumb.
const SCRATCH_KEY: usize = 1;

/// Dispatch supervisor and watchdog feeder.
///
/// Feeds the embassy-rp watchdog only while dispatcher progress is plausible.
/// This task used to feed unconditionally, which made the watchdog prove
/// **executor liveness** rather than **dispatcher progress**: postcard-rpc
/// dispatches handlers serially, so a handler that never returns blocks every
/// endpoint while this task keeps being scheduled. Three device-wide wedges
/// survived that (AGENTS.md §13.17); recovery needed USB re-enumeration or a
/// power cycle.
///
/// On expiry the device is reset. That drops USB and every GPIO subscription,
/// which is a deliberate, documented loss and strictly better than a wedge
/// that loses them anyway and does not come back.
#[embassy_executor::task]
async fn watchdog_supervisor_task(mut watchdog: Watchdog) {
    watchdog.start(Duration::from_secs(2));
    watchdog.pause_on_debug(true);

    let mut last_wake: Option<Instant> = None;

    loop {
        Timer::after(progress::SUPERVISOR_POLL).await;

        let now = Instant::now();
        let state = progress::snapshot();

        match progress::decide(now, last_wake, &state) {
            progress::Action::Feed => {
                watchdog.feed(Duration::from_secs(2));
            }
            progress::Action::Discontinuity => {
                // A debugger halt or severe executor starvation, not a wedge.
                // Re-arm rather than punishing the next resume.
                warn!("supervisor: time discontinuity, re-arming");
                progress::rearm_live_slots();
                watchdog.feed(Duration::from_secs(2));
            }
            progress::Action::Expired(slot, key) => {
                defmt::error!(
                    "supervisor: {} slot expired (key={=u32:#010x}) — resetting",
                    slot,
                    key
                );
                watchdog.set_scratch(SCRATCH_REASON, WEDGE_MAGIC | (slot as u32));
                watchdog.set_scratch(SCRATCH_KEY, key);
                watchdog.trigger_reset();
                // trigger_reset() does not return, but the compiler does not
                // know that.
                core::future::pending::<()>().await;
            }
        }

        last_wake = Some(now);
    }
}
```

- [ ] **Step 3: Read the breadcrumb at boot**

Replace `crates/pico-de-gallo-firmware/src/main.rs:385-386`:

```rust
    // Arm the hardware watchdog as defence-in-depth against handler hangs.
    spawner.must_spawn(watchdog_feeder_task(Watchdog::new(p.WATCHDOG)));
```

with:

```rust
    // Arm the hardware watchdog as defence-in-depth against handler hangs.
    // Read and clear any breadcrumb from a previous supervisor-forced reset
    // before handing the peripheral over.
    let mut watchdog = Watchdog::new(p.WATCHDOG);
    let reason = watchdog.reset_reason();
    let scratch_reason = watchdog.get_scratch(SCRATCH_REASON);
    if scratch_reason & 0xFFFF_FF00 == WEDGE_MAGIC {
        warn!(
            "previous boot ended in a supervisor-forced reset: slot={=u32} key={=u32:#010x}",
            scratch_reason & 0xFF,
            watchdog.get_scratch(SCRATCH_KEY)
        );
        watchdog.set_scratch(SCRATCH_REASON, 0);
        watchdog.set_scratch(SCRATCH_KEY, 0);
    } else if reason.is_some() {
        warn!("previous boot ended in a watchdog reset not raised by the supervisor");
    }
    spawner.must_spawn(watchdog_supervisor_task(watchdog));
```

- [ ] **Step 4: Verify**

```bash
cd crates/pico-de-gallo-firmware
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf
```

Expected: all clean. If clippy objects to `slot as u32` on a `defmt::Format` enum, add an explicit `fn discriminant(self) -> u32` to `SlotKind` in `progress.rs` and use it.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/main.rs
git commit -m "feat(firmware): Gate the watchdog feed on dispatcher progress

watchdog_feeder_task becomes watchdog_supervisor_task: it polls the progress
slots every 250 ms and feeds only while progress is plausible. On expiry it
logs, stashes a magic-tagged breadcrumb in watchdog scratch, and calls
trigger_reset(); main() reads and clears that breadcrumb at boot so a forced
reset is identifiable over RTT.

A debugger halt stops the watchdog counter but not the embassy time driver,
so the supervisor detects its own wake gap and re-arms instead of resetting
on resume.

Closes the mechanism gap behind three device-wide wedges. Reporting the reset
reason over the wire stays with #159.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 5: Route the six caller-timeout handlers through `bounded()`

This is where `timeout_ms == 0` stops meaning "wait forever".

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/handlers/gpio.rs:86-259`
- Modify: `crates/pico-de-gallo-firmware/src/handlers/uart.rs:19-55`

- [ ] **Step 1: Rewrite the five `gpio/wait-*` handlers**

Replace `crates/pico-de-gallo-firmware/src/handlers/gpio.rs:86-259` with:

```rust
/// Handler for `gpio/wait-high` — blocks until the pin goes high.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_high_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_high: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_high())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_high timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-low` — blocks until the pin goes low.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_low_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_low: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_low())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_low timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-rising` — blocks until a rising edge.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_rising_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_rising: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_rising_edge())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_rising timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-falling` — blocks until a falling edge.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_falling_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_falling: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_falling_edge())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_falling timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-any` — blocks until any edge.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_any_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_any: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_any_edge())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_any timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}
```

- [ ] **Step 2: Drop the now-unused import in `gpio.rs`**

`with_timeout` and `Duration` are no longer referenced. Change line 5 from:

```rust
use embassy_time::{Duration, with_timeout};
```

to nothing — delete the line. If `Duration` is still used elsewhere in the file, keep only that:

```bash
grep -n 'Duration\|with_timeout' crates/pico-de-gallo-firmware/src/handlers/gpio.rs
```

Delete the import only if that returns no remaining uses.

- [ ] **Step 3: Rewrite `uart_read_handler`**

Replace `crates/pico-de-gallo-firmware/src/handlers/uart.rs:19-55` with:

```rust
/// Handler for `uart/read` — reads bytes from the UART receive buffer.
///
/// Reads up to `count` bytes. `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT); a value of
/// `0` still selects the non-blocking 1 ms poll. Returns whatever bytes are
/// available (1 to count), or an empty slice on timeout.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: UartReadRequest,
) -> UartReadResponse<'a> {
    let count = (req.count as usize).min(MAX_TRANSFER_SIZE);
    if count == 0 {
        return Ok(&[]);
    }

    let buf = &mut context.buf[..count];

    if req.timeout_ms == 0 {
        // Non-blocking: try to read whatever is buffered. Well inside the
        // default dispatch budget, so no declaration is needed.
        match with_timeout(Duration::from_millis(1), AsyncRead::read(&mut context.uart, buf)).await {
            Ok(Ok(n)) => Ok(&context.buf[..n]),
            Ok(Err(_)) => Err(UartError::Other),
            Err(_) => Ok(&[]),
        }
    } else {
        match crate::progress::bounded(req.timeout_ms, AsyncRead::read(&mut context.uart, buf)).await {
            Ok(Ok(n)) => Ok(&context.buf[..n]),
            Ok(Err(_)) => Err(UartError::Other),
            Err(_) => Ok(&[]),
        }
    }
}
```

Note the deliberate asymmetry against GPIO: `uart/read` already treated `0` as a 1 ms non-blocking poll rather than an infinite wait, so `0` keeps that meaning and only the non-zero path is clamped.

- [ ] **Step 4: Verify**

```bash
cd crates/pico-de-gallo-firmware
dos2unix src/handlers/gpio.rs src/handlers/uart.rs
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
```

Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/handlers/gpio.rs crates/pico-de-gallo-firmware/src/handlers/uart.rs
git commit -m "feat(firmware): Clamp GPIO wait and UART read timeouts at 30 minutes

timeout_ms == 0 no longer means wait forever on the five gpio/wait-*
endpoints: it now selects MAX_HANDLER_TIMEOUT, and the handler's own
with_timeout returns GpioError::Timeout as usual, so callers get a clean
error rather than a device-wide block.

The 2026-06-03 wedge trigger was exactly this documented wait-forever path,
which a supervisor cannot distinguish from a genuine wedge. bounded() clamps
and declares the supervisor budget in one call so the two cannot drift.

uart/read keeps 0 as its existing 1 ms non-blocking poll; only its non-zero
path is clamped.

This changes documented wire semantics without changing wire shape, so
validate() cannot detect a mismatch. The lockstep schema bump is deliberately
deferred to release time.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 6: Declare budgets for the four remaining long handlers

**Files:**
- Modify: `crates/pico-de-gallo-firmware/src/handlers/spi.rs:97-125`
- Modify: `crates/pico-de-gallo-firmware/src/handlers/onewire.rs:102-108`
- Modify: `crates/pico-de-gallo-firmware/src/handlers/uart.rs:66-95`
- Modify: `crates/pico-de-gallo-firmware/src/progress.rs` (remove the temporary allow)

- [ ] **Step 1: Sum `DelayNs` in the `spi/batch` validation pass and declare**

`spi_batch_handler` already walks every op to compute `total_read`. Add delay accumulation to that same walk. Replace `crates/pico-de-gallo-firmware/src/handlers/spi.rs:97-125` with:

```rust
    // Pre-validate: walk the ops to compute total read length and the
    // accumulated delay, which sets this dispatch's supervisor budget.
    // MAX_BATCH_OPS (64) DelayNs ops of u32::MAX ns each is 274.9 s, well
    // above DEFAULT_DISPATCH_BUDGET.
    let mut total_read = 0usize;
    let mut total_delay_ns = 0u64;
    let mut remaining = ops;
    let mut validated = 0usize;
    while !remaining.is_empty() {
        let (op, rest) = postcard::take_from_bytes::<SpiBatchOp>(remaining).map_err(|_| SpiBatchError {
            failed_op: validated as u16,
            kind: SpiError::Other,
        })?;
        match op {
            SpiBatchOp::Read { len } => total_read += len as usize,
            SpiBatchOp::Transfer { data } => total_read += data.len(),
            SpiBatchOp::DelayNs { ns } => total_delay_ns = total_delay_ns.saturating_add(u64::from(ns)),
            _ => {}
        }
        remaining = rest;
        validated += 1;
    }
    if validated != count {
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::Other,
        });
    }
    if total_read > MAX_TRANSFER_SIZE {
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::BufferTooLong,
        });
    }

    // Declared before chip-select is touched, so the guard covers the whole
    // transaction including deassertion.
    let _budget = crate::progress::declare(
        crate::progress::DEFAULT_DISPATCH_BUDGET + embassy_time::Duration::from_millis(total_delay_ns / 1_000_000),
    );
```

- [ ] **Step 2: Declare in `onewire/write-pullup`**

Replace `crates/pico-de-gallo-firmware/src/handlers/onewire.rs:102-108` with:

```rust
    let duration = Duration::from_millis(u64::from(req.pullup_duration_ms));
    debug!(
        "onewire write-pullup: len={=usize} pullup_ms={=u16}",
        req.data.len(),
        req.pullup_duration_ms
    );
    // pullup_duration_ms is u16, so up to 65.5 s — well above the default
    // dispatch budget.
    let _budget = crate::progress::declare(duration + crate::progress::DEFAULT_DISPATCH_BUDGET);
    context.onewire.write_bytes_pullup(req.data, duration).await;
```

- [ ] **Step 3: Declare in `uart/write` and `uart/flush`**

Replace `crates/pico-de-gallo-firmware/src/handlers/uart.rs:66-95` with:

```rust
/// Fixed supervisor budget for UART transmit paths.
///
/// The 1024-byte TX buffer takes about 27 s to drain at 300 baud, which
/// exceeds the default dispatch budget. Deriving this from the configured
/// baud rate is possible but couples the handler to UART configuration state
/// for no practical gain.
#[cfg(feature = "hw-rev2")]
const UART_TX_BUDGET: Duration = Duration::from_secs(60);

/// Handler for `uart/write` — writes bytes to the UART transmit buffer.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_write_handler(
    context: &mut Context,
    _header: VarHeader,
    req: UartWriteRequest<'_>,
) -> UartWriteResponse {
    if req.contents.len() > MAX_TRANSFER_SIZE {
        return Err(UartError::BufferTooLong);
    }

    let _budget = crate::progress::declare(UART_TX_BUDGET);
    AsyncWrite::write_all(&mut context.uart, req.contents)
        .await
        .map_err(|_| UartError::Other)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn uart_write_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: UartWriteRequest<'_>,
) -> UartWriteResponse {
    Err(UartError::Unsupported)
}

/// Handler for `uart/flush` — flushes the UART transmit buffer.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_flush_handler(context: &mut Context, _header: VarHeader, _req: ()) -> UartFlushResponse {
    let _budget = crate::progress::declare(UART_TX_BUDGET);
    AsyncWrite::flush(&mut context.uart).await.map_err(|_| UartError::Other)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn uart_flush_handler(_context: &mut Context, _header: VarHeader, _req: ()) -> UartFlushResponse {
    Err(UartError::Unsupported)
}
```

- [ ] **Step 4: Remove the temporary dead-code allow**

If Task 1 Step 4 required `#![allow(dead_code)]` at the top of `progress.rs`, delete it now. Every item has a caller.

- [ ] **Step 5: Verify**

```bash
cd crates/pico-de-gallo-firmware
dos2unix src/handlers/spi.rs src/handlers/onewire.rs src/handlers/uart.rs src/progress.rs
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo build --release --locked --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1
cargo fmt --check
```

Expected: all clean. `hw-rev1` compiles out `uart` and `onewire` entirely, so only the `spi/batch` declaration is live there — confirm no unused-import warning appears in that configuration.

- [ ] **Step 6: Commit**

```bash
git add crates/pico-de-gallo-firmware/src/handlers/ crates/pico-de-gallo-firmware/src/progress.rs
git commit -m "feat(firmware): Declare supervisor budgets for the long handlers

Four handlers legitimately outrun the 10 s default dispatch budget:
spi/batch (64 DelayNs ops of u32::MAX ns is 274.9 s), onewire/write-pullup
(u16 ms, so 65.5 s), and uart/write and uart/flush (a 1024-byte buffer takes
about 27 s to drain at 300 baud).

spi/batch accumulates its delay during the validation walk it already
performs, and declares before chip-select is touched so the guard spans
deassertion too. A handler that forgets to declare resets at 10 s, which is
loud and immediate rather than silent.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 7: The `wedge-test` acceptance hatch

The hatch must reproduce a *real* historical trigger. It cannot live in `ping_handler`: `main.rs:321` registers `PingEndpoint` as `blocking`, and a blocking handler cannot `.await`.

The zero-length I2C write (2026-08-26, issue #101) is the right choice. Re-enabling it corrupts nothing — the whole defect is that `write_async_internal` queues no command and starts no transaction, then awaits a `STOP_DET`/`TX_ABRT` interrupt that only a started transaction can raise. No I2C target is needed, and the trigger is reachable from `gallo-mcp` and from `pico-de-gallo-lib`'s existing `#[ignore]`d #135 regression tests.

**Files:**
- Modify: `crates/pico-de-gallo-firmware/Cargo.toml:13-21`
- Modify: `crates/pico-de-gallo-firmware/src/handlers/i2c.rs:57-60`
- Modify: `.github/workflows/nostd.yml:21-30`

- [ ] **Step 1: Add the feature**

In `crates/pico-de-gallo-firmware/Cargo.toml`, after the `hw-rev2 = []` line (line 21), add:

```toml
# TEST ONLY. Disables the zero-length I2C write guard (#101) so the
# 2026-08-26 dispatcher wedge can be reproduced on demand, to prove the
# supervisor resets the device instead of letting it hang. Re-enabling the
# trigger corrupts nothing: the defect is that no I2C transaction ever
# starts. NEVER enable this in a release build — release-firmware.yml must
# not reference it.
wedge-test = []
```

- [ ] **Step 2: Gate the guard**

Replace `crates/pico-de-gallo-firmware/src/handlers/i2c.rs:57-60`:

```rust
    if req.contents.is_empty() {
        warn!("i2c write: empty payload refused (addr={=u8:#x})", req.address);
        return Err(I2cError::ZeroLengthWrite);
    }
```

with:

```rust
    #[cfg(not(feature = "wedge-test"))]
    if req.contents.is_empty() {
        warn!("i2c write: empty payload refused (addr={=u8:#x})", req.address);
        return Err(I2cError::ZeroLengthWrite);
    }
```

Leave the `i2c_batch_handler` guard at line 188 alone — one trigger is enough, and the batch guard is the one that protects against partial bus writes.

- [ ] **Step 3: Add the CI configuration**

In `.github/workflows/nostd.yml`, change lines 21-30 from:

```yaml
      matrix:
        target: [thumbv8m.main-none-eabihf]
        hw-rev: [hw-rev1, hw-rev2]
        include:
          # `hw-rev2` is the default feature; `hw-rev1` is deprecated and must
          # opt in explicitly. Keep these aligned with release-firmware.yml.
          - hw-rev: hw-rev1
            feature-flags: "--no-default-features --features hw-rev1"
          - hw-rev: hw-rev2
            feature-flags: ""
```

to:

```yaml
      matrix:
        target: [thumbv8m.main-none-eabihf]
        hw-rev: [hw-rev1, hw-rev2, wedge-test]
        include:
          # `hw-rev2` is the default feature; `hw-rev1` is deprecated and must
          # opt in explicitly. Keep these aligned with release-firmware.yml.
          - hw-rev: hw-rev1
            feature-flags: "--no-default-features --features hw-rev1"
          - hw-rev: hw-rev2
            feature-flags: ""
          # TEST ONLY. Keeps the acceptance hatch from bit-rotting. This entry
          # must NEVER be mirrored into release-firmware.yml.
          - hw-rev: wedge-test
            feature-flags: "--features wedge-test"
```

- [ ] **Step 4: Confirm the hatch cannot reach a release artifact**

```bash
grep -n 'wedge-test' .github/workflows/release-firmware.yml
```

Expected: no output. If there is any output, remove it — that is a release-blocking defect.

- [ ] **Step 5: Verify**

```bash
dos2unix .github/workflows/nostd.yml
cd crates/pico-de-gallo-firmware
cargo clippy --target thumbv8m.main-none-eabihf --features wedge-test -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf --features wedge-test
```

Expected: clean.

CRLF in a workflow `run:` block breaks `actionlint` with `unexpected character $'\r'` (AGENTS.md §13.1), which is why `dos2unix` runs before the build here.

- [ ] **Step 6: Commit**

```bash
git add crates/pico-de-gallo-firmware/Cargo.toml crates/pico-de-gallo-firmware/src/handlers/i2c.rs \
        .github/workflows/nostd.yml
git commit -m "feat(firmware): Add the wedge-test acceptance hatch

Reproducing a dispatcher wedge on demand needs a build with a known trigger
unguarded. wedge-test disables the zero-length I2C write guard (#101), which
is the real 2026-08-26 trigger and corrupts nothing, since the defect is that
no I2C transaction ever starts.

ping_handler was not an option: main.rs registers PingEndpoint as blocking,
and a blocking handler cannot await.

nostd.yml lints the configuration so it cannot bit-rot;
release-firmware.yml deliberately does not reference it.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Task 8: Board-attached acceptance run

Nothing before this proves behaviour. CI proves only that the firmware compiles and lints.

**This task produces evidence that goes into the AGENTS.md §13.17 row in Task 9. Record actual observed timings, not expected ones.**

**Prerequisites:** one Pico de Gallo board, an RTT viewer (`probe-rs attach` or `cargo run` with `defmt-rtt`), and a host that can issue a zero-length I2C write. The `gallo` CLI **cannot** — clap's `num_args(1..)` requires at least one byte. Use `gallo-mcp`'s `i2c_write` with `data: ""`, or `pico-de-gallo-lib`'s `#[ignore]`d #135 regression tests.

- [ ] **Step 1: Build and flash the mutation control — supervisor disabled**

Build a `wedge-test` image from a working tree with the supervisor neutered, so the A/B has a baseline. Temporarily change the `Action::Expired` arm in `watchdog_supervisor_task` to feed instead of reset:

```rust
            progress::Action::Expired(slot, key) => {
                defmt::error!("supervisor: {} slot expired (key={=u32:#010x}) — MUTATION CONTROL, not resetting", slot, key);
                watchdog.feed(Duration::from_secs(2));
            }
```

```bash
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf --features wedge-test
```

Flash it. **Do not commit this change.**

- [ ] **Step 2: Confirm the wedge still reproduces**

Issue a zero-length `i2c/write`. Then, from a **fresh host process**, attempt `version`, `ping`, `device/info`, `gpio/get` and `adc/read`.

Expected: the empty write never returns, and every other endpoint times out — confirming the wedge is device-wide, not I2C-only. Hold for at least 60 s and confirm the watchdog never fires.

Record: how long the empty write hung, which endpoints timed out, and whether a fresh host process could reach anything.

If the wedge does **not** reproduce, stop. Either the trigger has been fixed upstream in embassy-rp, or the hatch is not doing what it claims. Investigate before continuing — a passing Step 4 means nothing without a failing Step 2.

- [ ] **Step 3: Revert the mutation and flash the real image**

```bash
git checkout crates/pico-de-gallo-firmware/src/main.rs
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf --features wedge-test
```

Flash it.

AGENTS.md §13.17 warns that `validate()` cannot distinguish two firmware builds that report the same version. **Track which image is on the board yourself** — that exact confusion misidentified a flash during the #135 verification.

- [ ] **Step 4: Confirm the supervisor resets the device**

Issue the same zero-length `i2c/write` with RTT attached.

Expected, within `DEFAULT_DISPATCH_BUDGET + SUPERVISOR_POLL` ≈ 10.25 s:

1. `supervisor: Dispatch slot expired (key=0x........) — resetting` on RTT.
2. USB re-enumeration.
3. On the next boot: `previous boot ended in a supervisor-forced reset: slot=0 key=0x........`.
4. Every endpoint works again with no power cycle and no physical intervention.

Record the measured time from request to reset.

- [ ] **Step 5: Confirm no false positive on a long legitimate wait**

Flash the **default** image (no `wedge-test`):

```bash
cd crates/pico-de-gallo-firmware
cargo build --release --locked --target thumbv8m.main-none-eabihf
```

Issue `gpio/wait-high` on a pin held low with `timeout_ms = 60000`. Expected: the call returns `GpioError::Timeout` after about 60 s, the board does **not** reset, and RTT shows no supervisor message.

Then issue `i2c/scan`. Expected: completes normally, no reset. This is the endpoint closest to the 10 s default budget at 6.4 s worst case.

- [ ] **Step 6: Confirm the debugger discontinuity rule**

Attach a debugger, halt the core for at least 5 s during an idle period, then resume.

Expected: `supervisor: time discontinuity, re-arming` on RTT, and **no** reset.

If the board resets here, `DISCONTINUITY` is too tight — raise it and re-run. Do not work around it by disabling `pause_on_debug`.

- [ ] **Step 7: Record the results**

Write the observed numbers into the PR body and keep them for Task 9. At minimum: Step 2's hang duration and dead endpoints, Step 4's measured reset latency, Step 5's two negative results, Step 6's outcome.

There is no commit for this task.

---

## Task 9: Documentation

AGENTS.md §15.1 makes this a blocker, not a nit.

**Files:**
- Modify: `book/src/interfaces/gpio.md`, `book/src/interfaces/uart.md`
- Modify: `book/src/crates/{ffi,lib,mcp}.md`
- Modify: `book/src/internals/firmware.md`, `book/src/appendix/troubleshooting.md`
- Modify: `crates/pico-de-gallo-firmware/CHANGELOG.md`
- Modify: `AGENTS.md`, `ROADMAP.md`
- Modify: `timeout_ms` rustdoc in `internal`, `lib`, `hal`, `ffi`, `app`, `mcp`, `pyco`

- [ ] **Step 1: Read the pages you are about to change**

Do not edit these blind. Read each file first — the wording below has to fit
the surrounding chapter's voice and structure:

```bash
cd D:\workspace\pico-de-gallo
rg -n 'wait forever|wait-forever|0 = forever|means forever|blocks indefinitely' crates/ book/ zephyr/
rg -n 'timeout_ms' crates/*/src book/src
```

- [ ] **Step 2: Update every place that documents the old semantics**
Update every hit that describes `timeout_ms == 0` as unbounded. The known surfaces are `pico-de-gallo-internal`, `-lib`, `-hal`, `-ffi`, `-app`, `-mcp` (`gpio.rs`, `uart.rs`, `encoding.rs`) and `pyco-de-gallo`.

For each, the new wording is: *`0` selects the firmware's 30-minute ceiling; oversized values are clamped to the same ceiling. The endpoint returns its `Timeout` error on expiry.*

- [ ] **Step 3: Confirm Zephyr is genuinely unaffected**

```bash
rg -n 'gallo_gpio_wait|gallo_uart_read' zephyr/
```

Expected: no output. Record that result in the PR body. §15.1's reverse-direction obligation is satisfied by inspection here — a green `zephyr.yml` run does **not** cover behaviour, and the workflow is path-filtered so it may not run at all.

If this returns hits, stop and reassess: the clamp then changes Zephyr driver behaviour and needs its own analysis plus a `zephyr/CHANGELOG.md` entry.

- [ ] **Step 4: Add the missing `book/src/interfaces/gpio.md` timeout section**

`book/src/interfaces/uart.md` documents `timeout_ms`; `gpio.md` apparently does not mention it at all. Add a section covering all five `gpio/wait-*` endpoints, the 30-minute ceiling, the meaning of `0`, and `GpioError::Timeout`.

- [ ] **Step 5: Document the supervisor**

In `book/src/internals/firmware.md`, replace whatever describes the watchdog as an independent feeder. Cover: the two slots, the default and declared budgets, the 30-minute ceiling, what a forced reset costs (USB drops, GPIO subscriptions are lost), the debugger discontinuity rule, and — explicitly — the two things it does **not** cover, per spec §6.3.

In `book/src/appendix/troubleshooting.md`, add "The board reset itself" with the RTT lines to look for and what they mean.

- [ ] **Step 6: Update `crates/pico-de-gallo-firmware/CHANGELOG.md`**

Add to the unreleased section, Keep a Changelog format. Cover both the supervisor (`Added`) and the timeout clamp (`Changed`, and call it out as a behaviour change).

- [ ] **Step 7: Add the AGENTS.md §13.17 row**

Append a row to the regression table using the measured numbers from Task 8. Follow the existing rows' structure: Date / Trigger / Symptom / Fix. State plainly what remains uncovered — a wedge inside `receive()`, and the TX slot having no hardware trigger.

- [ ] **Step 8: Update ROADMAP.md**

Remove the #157 line from the open-work table at `ROADMAP.md:240`. Rewrite the dispatcher-wedge narrative at `ROADMAP.md:190-194`, which currently asserts the watchdog cannot catch this — it now can, within the documented limits.

- [ ] **Step 9: Record the deferred version bump**

This is the single highest-risk item in the change and nothing automated enforces it.

Add to `crates/pico-de-gallo-firmware/CHANGELOG.md` under the unreleased section, and to the PR body:

> The `timeout_ms == 0` clamp changes documented wire semantics without
> changing wire shape. `pico-de-gallo-internal` remains at `0.7.0`, and
> `internal-v0.7.0` is already published, so `validate()` cannot distinguish a
> pre-clamp 0.7 firmware from a post-clamp one. Host and firmware must be
> built from the same tree until the bump lands. Before any release,
> AGENTS.md §16 step 2 requires bumping `internal` to `0.8.0` in lockstep
> across all eight released crates, with dep-spec rewrites, per-crate
> CHANGELOGs and both regenerated `Cargo.lock`s.

- [ ] **Step 10: Verify the book builds**

```bash
mdbook build book
```

Expected: no broken links, no missing referenced files.

- [ ] **Step 11: Commit**

```bash
dos2unix AGENTS.md ROADMAP.md book/src/interfaces/gpio.md book/src/interfaces/uart.md \
         book/src/internals/firmware.md book/src/appendix/troubleshooting.md \
         crates/pico-de-gallo-firmware/CHANGELOG.md
git add -A
git commit -m "docs(firmware,internal,lib,hal,ffi,application,mcp,pyco): Document the timeout ceiling

timeout_ms == 0 no longer means wait forever on gpio/wait-*; it selects the
firmware's 30 minute ceiling. Updates every host surface that documented the
old semantics, adds the missing timeout section to the GPIO interface
chapter, documents the dispatch supervisor and its two uncovered cases, and
adds a troubleshooting entry for a self-reset board.

Records the deferred lockstep schema bump: the wire shape is unchanged, so
validate() cannot detect the semantic difference, and AGENTS.md 16 step 2
must catch it before any release.

Zephyr is unaffected: it calls neither gallo_gpio_wait* nor gallo_uart_read.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Final verification before opening the PR

- [ ] **Full preflight, both revisions plus the hatch**

```bash
cd D:\workspace\pico-de-gallo
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo check --workspace --locked

cd crates/pico-de-gallo-firmware
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1 -- -D warnings
cargo clippy --target thumbv8m.main-none-eabihf --features wedge-test -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf
cargo build --release --locked --target thumbv8m.main-none-eabihf --no-default-features --features hw-rev1
cargo check --locked --target thumbv8m.main-none-eabihf
```

Expected: all clean. Host tests should still be at the AGENTS.md §5.5 baseline — no host crate changed behaviour.

- [ ] **Confirm no line-ending damage**

```bash
cd D:\workspace\pico-de-gallo
git diff --stat main...HEAD
```

Expected: no whole-file rewrites. A file showing every line changed means CRLF crept in — run `dos2unix` on it and amend.

- [ ] **Open a draft PR and let CI run**

Per AGENTS.md §11: draft first, do not request review until `lockfile`, `deny`, `semver`, `actionlint` and `no-std` are all green. Do not squash-merge.

The PR body must carry the Task 8 measurements, the Task 9 Step 3 Zephyr result, and the Task 9 Step 9 deferred-bump warning.
